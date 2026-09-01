//! CDP-level acceptance — proves gate runs before domain handlers.

use obscura_cdp::dispatch::{dispatch, CdpContext};
use obscura_cdp::types::{CdpRequest, CdpResponse};
use serde_json::json;
use tempfile::TempDir;

fn setup() -> (TempDir, CdpContext) {
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
    let mut ctx = CdpContext::new();
    let page_id = ctx.create_page();
    let session_id = format!("{page_id}-session-test");
    ctx.sessions.insert(session_id.clone(), page_id);
    (dir, ctx)
}

async fn cdp(
    ctx: &mut CdpContext,
    id: u64,
    method: &str,
    params: serde_json::Value,
    session_id: &str,
) -> CdpResponse {
    let req = CdpRequest {
        id,
        method: method.into(),
        params,
        session_id: Some(session_id.into()),
    };
    dispatch(&req, ctx).await
}

#[tokio::test]
async fn cdp_insert_text_secret_refused_before_side_effect() {
    let (_dir, mut ctx) = setup();
    let sid = ctx.sessions.keys().next().unwrap().clone();
    // Navigate to allowed host first
    let nav = cdp(
        &mut ctx,
        1,
        "Page.navigate",
        json!({"url": "https://example.com", "waitUntil": "load"}),
        &sid,
    )
    .await;
    assert!(nav.error.is_none(), "navigate: {:?}", nav.error);

    let resp = cdp(
        &mut ctx,
        2,
        "Input.insertText",
        json!({"text": "sk-abcdefghijklmnopqrstuvwxyz1234567890"}),
        &sid,
    )
    .await;
    assert!(resp.error.is_some(), "secret insertText must be REFUSED");
    let msg = resp.error.unwrap().message;
    assert!(msg.contains("REFUSE") || msg.contains("TERMINUS"), "{msg}");
}

#[tokio::test]
async fn cdp_call_function_on_secret_refused() {
    let (_dir, mut ctx) = setup();
    let sid = ctx.sessions.keys().next().unwrap().clone();
    cdp(
        &mut ctx,
        1,
        "Page.navigate",
        json!({"url": "https://example.com", "waitUntil": "load"}),
        &sid,
    )
    .await;

    let resp = cdp(
        &mut ctx,
        2,
        "Runtime.callFunctionOn",
        json!({
            "functionDeclaration": "function() { return 'sk-abcdefghijklmnopqrstuvwxyz1234567890'; }",
            "returnByValue": true
        }),
        &sid,
    )
    .await;
    assert!(resp.error.is_some(), "callFunctionOn with secret must REFUSE");
}

#[tokio::test]
async fn cdp_navigate_ssrf_refused() {
    let (_dir, mut ctx) = setup();
    let sid = ctx.sessions.keys().next().unwrap().clone();
    let resp = cdp(
        &mut ctx,
        1,
        "Page.navigate",
        json!({"url": "http://127.0.0.1/admin", "waitUntil": "load"}),
        &sid,
    )
    .await;
    assert!(resp.error.is_some(), "SSRF navigate must REFUSE");
}
