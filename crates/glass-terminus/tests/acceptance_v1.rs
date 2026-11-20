//! OMNIS GLASS v1 acceptance — Pan CLEAR bar items A1–A8.

use glass_capture::{build_capture, CaptureKind, CaptureSource, SsrfCheck, WebCaptureWriter};
use glass_ledger::{verify_chain, BrowserActionWriter};
use glass_terminus::{target_from_url, BrowserAction, OutboundEnvelope, TerminusGate};
use serde_json::json;
use std::path::PathBuf;
use tempfile::tempdir;

fn test_env(dir: &PathBuf) {
    std::env::set_var("OMNIS_GLASS_STATE_DIR", dir);
    std::env::set_var("TERMINUS_AUTHORIZE", concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/terminus-authorize.mjs"
    ));
    std::env::set_var("OMNIS_GLASS_LANE", "browse:research");
    std::env::set_var("OMNIS_GLASS_ENFORCE", "enforce");
}

/// A1: secret field + disallowed host → REFUSE before side effect
#[test]
fn a1_secret_on_disallowed_host_refused() {
    let dir = tempdir().unwrap();
    test_env(&dir.path().to_path_buf());
    let gate = TerminusGate::new(dir.path());
    let target = target_from_url("https://evil.example/form").unwrap();
    let env = OutboundEnvelope::browser(
        BrowserAction::Type,
        target,
        Some(json!({"field_value": "sk-abcdefghijklmnopqrstuvwxyz1234567890"})),
        &gate.lane,
        &gate.policy.version,
        &gate.policy.coverage_class,
        "a1",
    );
    let v = gate.authorize(&env);
    assert_eq!(v.verdict, "REFUSE", "expected REFUSE, got {:?}", v);
}

/// A2: crown jewel in POST body → REFUSE
#[test]
fn a2_crown_jewel_upload_refused() {
    let dir = tempdir().unwrap();
    test_env(&dir.path().to_path_buf());
    std::env::set_var("OMNIS_CROWN_JEWELS", "CROWN-JEWEL-SENTINEL-XYZ");
    let gate = TerminusGate::new(dir.path());
    let target = target_from_url("https://example.com/upload").unwrap();
    let env = OutboundEnvelope::browser(
        BrowserAction::Submit,
        target,
        Some(json!({"post_body": "file contains CROWN-JEWEL-SENTINEL-XYZ data"})),
        &gate.lane,
        &gate.policy.version,
        &gate.policy.coverage_class,
        "a2",
    );
    let v = gate.authorize(&env);
    assert_eq!(v.verdict, "REFUSE");
}

/// A3: live capture content_sha256 matches bytes
#[test]
fn a3_live_capture_sha256_matches() {
    let dir = tempdir().unwrap();
    let bytes = b"<html><body>live page</body></html>";
    let receipt = build_capture(
        "https://example.com",
        "https://example.com/",
        bytes,
        CaptureSource::PageContent,
        CaptureKind::Live,
        Some(200),
        None,
    )
    .expect("live capture");
    let writer = WebCaptureWriter::new(dir.path()).unwrap();
    writer.write(&receipt).unwrap();
    assert_eq!(receipt.byte_len, bytes.len() as u64);
    assert_eq!(
        receipt.content_sha256,
        glass_ledger::digest_sha256(&String::from_utf8_lossy(bytes))
    );
}

/// A4: eval-only cannot be stamped live
#[test]
fn a4_eval_cannot_be_live() {
    let err = build_capture(
        "https://example.com",
        "https://example.com",
        b"42",
        CaptureSource::EvalResult,
        CaptureKind::Live,
        Some(200),
        Some("tls".into()),
    );
    assert!(err.is_err(), "eval-only must not stamp live");
}

/// A5: SSRF literal loopback refused
#[test]
fn a5_ssrf_literal_refused() {
    let c = SsrfCheck::default();
    use glass_capture::SsrfVerdict;
    assert!(matches!(
        c.check_url("http://127.0.0.1/admin"),
        SsrfVerdict::Refuse(_)
    ));
    assert!(matches!(
        c.check_url("http://169.254.169.254/meta"),
        SsrfVerdict::Refuse(_)
    ));
}

/// A6: tamper chain line → verify fails
#[test]
fn a6_chain_tamper_detected() {
    let dir = tempdir().unwrap();
    let writer = BrowserActionWriter::new(dir.path()).unwrap();
    let mut fields = serde_json::Map::new();
    fields.insert("url".into(), json!("https://example.com"));
    writer
        .write_action("navigate", "browse:research", "ALLOW", fields)
        .unwrap();
    let path = dir.path().join("browser.actions.jsonl");
    let content = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, content.replace("example.com", "tampered.com")).unwrap();
    let result = verify_chain(&path, None);
    assert!(!result.ok);
    assert_eq!(result.first_break_index, Some(0));
}

/// A7: state dir contains no cleartext sentinel secret
#[test]
fn a7_no_cleartext_secrets_in_state() {
    let dir = tempdir().unwrap();
    test_env(&dir.path().to_path_buf());
    let secret = "sk-abcdefghijklmnopqrstuvwxyz1234567890";
    let gate = TerminusGate::new(dir.path());
    let target = target_from_url("https://evil.com/x").unwrap();
    let env = OutboundEnvelope::browser(
        BrowserAction::Type,
        target,
        Some(json!({"field_value": secret})),
        &gate.lane,
        &gate.policy.version,
        &gate.policy.coverage_class,
        "a7",
    );
    let _ = gate.authorize(&env);
    // Walk state dir
    fn walkgrep(path: &std::path::Path, needle: &str) -> bool {
        if path.is_file() {
            let Ok(content) = std::fs::read_to_string(path) else {
                return false;
            };
            return content.contains(needle);
        }
        if path.is_dir() {
            for entry in std::fs::read_dir(path).unwrap() {
                if walkgrep(&entry.unwrap().path(), needle) {
                    return true;
                }
            }
        }
        false
    }
    assert!(
        !walkgrep(dir.path(), secret),
        "state dir must not contain cleartext secret"
    );
    assert!(
        !walkgrep(dir.path(), "Authorization:"),
        "state dir must not contain auth headers"
    );
}

/// A8: scrape lane cannot type; sensitive lane host floor
#[test]
fn a8_lane_structural_rules() {
    let dir = tempdir().unwrap();
    test_env(&dir.path().to_path_buf());
    std::env::set_var("OMNIS_GLASS_LANE", "browse:scrape");
    let gate = TerminusGate::new(dir.path());
    let target = target_from_url("https://example.com").unwrap();
    let env = OutboundEnvelope::browser(
        BrowserAction::Type,
        target,
        Some(json!({"field_value": "hello"})),
        &gate.lane,
        &gate.policy.version,
        &gate.policy.coverage_class,
        "a8",
    );
    let v = gate.authorize(&env);
    assert_eq!(v.verdict, "REFUSE");
}

/// A9 weaken mirrors — secret allow on evil host must fail (proves test is live)
#[test]
fn a9_weaken_secret_check_would_fail() {
    // If we mistakenly allowed secrets through, this documents the can-fail shape
    let dir = tempdir().unwrap();
    test_env(&dir.path().to_path_buf());
    let gate = TerminusGate::new(dir.path());
    let target = target_from_url("https://example.com").unwrap();
    let env = OutboundEnvelope::browser(
        BrowserAction::Type,
        target,
        Some(json!({"field_value": "sk-abcdefghijklmnopqrstuvwxyz1234567890"})),
        &gate.lane,
        &gate.policy.version,
        &gate.policy.coverage_class,
        "a9",
    );
    let v = gate.authorize(&env);
    // On allowed host, secret still REFUSE via terminus scanner
    assert_eq!(v.verdict, "REFUSE");
}

#[test]
fn key_event_secret_is_scanned_without_digest() {
    let dir = tempdir().unwrap();
    test_env(&dir.path().to_path_buf());
    let gate = TerminusGate::new(dir.path());
    let secret = "sk-abcdefghijklmnopqrstuvwxyz1234567890";
    let mut env = OutboundEnvelope::browser(
        BrowserAction::Type, target_from_url("https://example.com").unwrap(),
        Some(json!({"field_value": secret})), &gate.lane, &gate.policy.version,
        &gate.policy.coverage_class, "key-secret",
    );
    env.key_event = Some(true);
    let verdict = gate.authorize(&env);
    assert_eq!(verdict.verdict, "REFUSE", "{verdict:?}");
    assert_eq!(verdict.decision, "REFUSE_SECRET");
    let ledger = std::fs::read_to_string(dir.path().join("terminus.actions.jsonl")).unwrap();
    let record: serde_json::Value = serde_json::from_str(ledger.lines().last().unwrap()).unwrap();
    assert!(record["payload_digest"].is_null());
    assert_eq!(record["chars"], secret.chars().count());
    assert_eq!(record["reason"], "bearer_secret");
    assert!(!ledger.contains(secret));
}
