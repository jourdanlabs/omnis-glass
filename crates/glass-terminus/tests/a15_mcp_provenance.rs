//! A15 — MCP snapshot result carries provenance.

use glass_ledger::digest_sha256;
use obscura_mcp::glass::wrap_text_result;
use tempfile::tempdir;

fn setup() {
    let dir = tempdir().unwrap();
    std::env::set_var("OMNIS_GLASS_STATE_DIR", dir.path());
}

#[test]
fn a15_mcp_wrap_includes_provenance() {
    setup();
    let text = "hello world";
    let out = wrap_text_result(text, "https://example.com");
    let prov = out.get("provenance").expect("provenance");
    assert_eq!(
        prov.get("url").and_then(|v| v.as_str()).unwrap(),
        "https://example.com"
    );
    assert_eq!(
        prov.get("content_sha256").and_then(|v| v.as_str()).unwrap(),
        digest_sha256(text)
    );
    assert_eq!(
        prov.get("capture_kind").and_then(|v| v.as_str()).unwrap(),
        "live"
    );
}
