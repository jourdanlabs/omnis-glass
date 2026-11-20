use glass_terminus::{target_from_url, BrowserAction, OutboundEnvelope, TerminusGate};
use std::{io::Write, path::Path, process::{Command, Stdio}};

fn script() -> std::path::PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/terminus-authorize.mjs") }

#[test]
fn authorize_ledger_path_respects_override_state_and_temporary_home() {
    let root = tempfile::tempdir().unwrap();
    for (name, state_set, override_set) in [("default", false, false), ("state", true, false), ("override", false, true), ("override_with_state", true, true)] {
        let case = root.path().join(name); let home = case.join("home"); let state = case.join("state"); let explicit = case.join("custom/actions.jsonl");
        std::fs::create_dir_all(&home).unwrap();
        let mut command = Command::new("node");
        command.arg(script()).env("HOME", &home).env_remove("OMNIS_GLASS_STATE_DIR").env_remove("TERMINUS_ACTION_LEDGER")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if state_set { command.env("OMNIS_GLASS_STATE_DIR", &state); }
        if override_set { command.env("TERMINUS_ACTION_LEDGER", &explicit); }
        let mut child = command.spawn().unwrap();
        child.stdin.take().unwrap().write_all(br#"{"kind":"browser","action":"navigate","lane":"browse:human","target":{"host":"example.com"}}"#).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{name}: {}", String::from_utf8_lossy(&output.stderr));
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap(); assert_eq!(response["verdict"], "ALLOW");
        let expected = if override_set { explicit.clone() } else if state_set { state.join("terminus.actions.jsonl") } else { home.join(".omnis-glass/terminus.actions.jsonl") };
        assert!(expected.is_file(), "{name}: missing {}", expected.display());
        assert_eq!(std::fs::read_to_string(&expected).unwrap().lines().count(), 1, "{name}");
        if override_set { assert!(!state.join("terminus.actions.jsonl").exists()); assert!(!home.join(".omnis-glass/terminus.actions.jsonl").exists()); }
        if state_set { assert!(!home.join(".omnis-glass/terminus.actions.jsonl").exists()); }
    }
}

#[test]
fn gate_supplies_its_own_state_directory_to_authorizer() {
    let root = tempfile::tempdir().unwrap(); let gate_state = root.path().join("gate"); let ambient_state = root.path().join("ambient");
    std::env::set_var("HOME", root.path().join("home"));
    std::env::set_var("OMNIS_GLASS_STATE_DIR", &ambient_state);
    std::env::set_var("OMNIS_GLASS_ENFORCE", "enforce");
    std::env::set_var("OMNIS_GLASS_LANE", "browse:human");
    std::env::set_var("TERMINUS_AUTHORIZE", script());
    std::env::remove_var("TERMINUS_ACTION_LEDGER"); std::env::remove_var("OMNIS_GLASS_LANES");
    let gate = TerminusGate::new(&gate_state);
    let target = target_from_url("http://127.0.0.1/allowed").unwrap();
    let envelope = OutboundEnvelope::browser(BrowserAction::Navigate, target, None, "browse:human", "test", "test", "session");
    let verdict = gate.authorize_with_network(&envelope, true, None);
    assert_eq!(verdict.verdict, "ALLOW", "{verdict:?}");
    assert!(gate_state.join("terminus.actions.jsonl").is_file());
    assert!(!ambient_state.join("terminus.actions.jsonl").exists());
}
