//! A14 — MCP navigate disallowed host → REFUSE.

use obscura_mcp::glass::gate_mcp_navigate;
use tempfile::tempdir;

fn setup() {
    let dir = tempdir().unwrap();
    std::env::set_var("OMNIS_GLASS_STATE_DIR", dir.path());
    std::env::set_var(
        "TERMINUS_AUTHORIZE",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts/terminus-authorize.mjs"
        ),
    );
    std::env::set_var("OMNIS_GLASS_LANE", "browse:research");
}

#[test]
fn a14_mcp_navigate_disallowed_host_refused() {
    setup();
    let err = gate_mcp_navigate("https://evil.example/page");
    assert!(err.is_err());
}

#[test]
#[ignore = "can-fail mirror — run with --ignored; must fail when gate is live"]
fn a14_WEAKEN_would_allow_evil_host() {
    setup();
    let result = gate_mcp_navigate("https://evil.example/page");
    assert!(result.is_ok(), "WEAKEN: weakened gate would allow evil host");
}
