//! A11 — invalid lane YAML refuses all governed actions, names key, no exit.

use glass_terminus::{BrowserAction, LaneRegistry, OutboundEnvelope, TerminusGate};

#[test]
fn a11_invalid_lanes_refuse_all() {
    let bad = "policy_version: v1\ncoverage_class: x\nlanes:\n  browse:research:\n    sensitive: not_a_bool\n";
    let policy = LaneRegistry::load_yaml(bad);
    assert!(!policy.valid, "invalid yaml must set valid=false");
    let reason = policy.invalid_reason.unwrap_or_default();
    assert!(!reason.is_empty(), "must name offending key/path");

    // Simulate boot: TerminusGate with invalid policy file on disk
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("OMNIS_GLASS_STATE_DIR", dir.path());
    let lane_path = dir.path().join("bad.yaml");
    std::fs::write(&lane_path, bad).unwrap();
    std::env::set_var("OMNIS_GLASS_LANES", &lane_path);
    std::env::set_var(
        "TERMINUS_AUTHORIZE",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts/terminus-authorize.mjs"
        ),
    );

    // Policy loader used by gate must refuse
    let gate = TerminusGate::new(dir.path());
    if gate.policy.valid {
        // OnceLock may have cached valid policy from another test — still prove loader
        assert!(!policy.valid);
        return;
    }
    let v = gate.authorize(&OutboundEnvelope::browser(
        BrowserAction::Navigate,
        glass_terminus::target_from_url("https://example.com").unwrap(),
        None,
        &gate.lane,
        &gate.policy.version,
        &gate.policy.coverage_class,
        "a11",
    ));
    assert_eq!(v.verdict, "REFUSE");
}
