//! A13 — HTTP client gate: script-fetch + dump-original refuse before send.

use glass_govern::{gate_http_outbound, HttpGateAction};
use url::Url;

fn setup() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("OMNIS_GLASS_STATE_DIR", dir.path());
    std::env::set_var(
        "TERMINUS_AUTHORIZE",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts/terminus-authorize.mjs"
        ),
    );
    std::env::set_var("OMNIS_GLASS_LANE", "browse:research");
    std::env::set_var("OMNIS_GLASS_ENFORCE", "enforce");
}

#[test]
fn a13_disallowed_host_refused_at_http_boundary() {
    setup();
    let url = Url::parse("https://evil.example/data").unwrap();
    let err = gate_http_outbound(&url, None, HttpGateAction::DumpOriginal, "a13");
    assert!(err.is_err(), "disallowed host must REFUSE");
}

#[test]
fn a13_secret_body_refused_at_http_boundary() {
    setup();
    let url = Url::parse("https://example.com/post").unwrap();
    let secret = b"sk-abcdefghijklmnopqrstuvwxyz1234567890";
    let err = gate_http_outbound(&url, Some(secret), HttpGateAction::ScriptFetch, "a13");
    assert!(err.is_err(), "secret in fetch body must REFUSE");
}

#[test]
#[ignore = "can-fail mirror — run with --ignored; must fail when gate is live"]
fn a13_WEAKEN_would_allow_secret() {
    setup();
    let url = Url::parse("https://example.com/post").unwrap();
    let result = gate_http_outbound(
        &url,
        Some(b"sk-abcdefghijklmnopqrstuvwxyz1234567890"),
        HttpGateAction::ScriptFetch,
        "a13",
    );
    assert!(result.is_ok(), "WEAKEN: weakened gate would allow secret body");
}
