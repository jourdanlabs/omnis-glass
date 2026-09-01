//! A14 — MCP tool surface: disallowed host → REFUSE (not helper-only).

use obscura_mcp::{dispatch_tool, BrowserState};
use serde_json::json;
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
    std::env::set_var("OMNIS_GLASS_ENFORCE", "enforce");
}

#[tokio::test(flavor = "current_thread")]
async fn a14_browser_tab_new_disallowed_host_refused() {
    setup();
    let mut state = BrowserState::new(None, None, false);
    let err = dispatch_tool(
        "browser_tab_new",
        &json!({ "url": "https://evil.example/page" }),
        &mut state,
    )
    .await;
    assert!(err.is_err(), "browser_tab_new must REFUSE evil.example");
    assert!(
        err.unwrap_err().contains("TERMINUS"),
        "refusal must come from TERMINUS gate"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a14_browser_back_disallowed_host_refused() {
    setup();
    let mut state = BrowserState::new(None, None, false);
    state.test_seed_page_history(
        vec![
            "https://evil.example/prev".into(),
            "about:blank".into(),
        ],
        1,
    );
    let err = dispatch_tool("browser_back", &json!({}), &mut state).await;
    assert!(err.is_err(), "browser_back must REFUSE evil.example in history");
}

#[test]
#[ignore = "can-fail mirror — run with --ignored; must fail when gate is live"]
fn a14_WEAKEN_would_allow_tab_new_evil_host() {
    setup();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = rt.block_on(async {
        let mut state = BrowserState::new(None, None, false);
        dispatch_tool(
            "browser_tab_new",
            &json!({ "url": "https://evil.example/page" }),
            &mut state,
        )
        .await
    });
    assert!(result.is_ok(), "WEAKEN: weakened gate would allow evil host");
}
