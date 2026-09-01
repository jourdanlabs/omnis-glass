//! Inbound CDP gate proof — external WebSocket client hits `gate_cdp_command`.
//!
//! Evidence for `docs/PANE-SPIKE.md` § inbound CDP: a client connected over
//! the CDP WebSocket (not in-process `dispatch`) receives TERMINUS REFUSE
//! when sending `Page.navigate` to a disallowed host.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};

async fn pick_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    port
}

fn setup_glass_env(dir: &std::path::Path, token: &str) {
    std::env::set_var("OMNIS_GLASS_STATE_DIR", dir);
    std::env::set_var("OMNIS_GLASS_CDP_TOKEN", token);
    std::env::set_var("OMNIS_GLASS_CDP_AUTH", "1");
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
fn inbound_cdp_websocket_navigate_disallowed_host_refused() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();
    local.block_on(&rt, async {
        inbound_cdp_websocket_navigate_disallowed_host_refused_async().await;
    });
}

async fn inbound_cdp_websocket_navigate_disallowed_host_refused_async() {
    let dir = tempfile::tempdir().unwrap();
    let token = "spike-inbound-cdp-token-fixed-value";
    setup_glass_env(dir.path(), token);
    let port = pick_port().await;

    tokio::task::spawn_local(async move {
        let _ = obscura_cdp::start_with_serve_options_and_limit(
            port,
            "127.0.0.1",
            None,
            false,
            None,
            false,
            None,
            true,
            8,
        )
        .await;
    });
    tokio::time::sleep(Duration::from_millis(400)).await;

    let url = obscura_cdp::cdp_auth::debugger_url("127.0.0.1", port, token);
    let (mut ws, _) = connect_async(&url).await.expect("CDP WS handshake with token");

    ws.send(Message::Text(
        json!({"id": 1, "method": "Target.createTarget", "params": {"url": "about:blank"}})
            .to_string()
            .into(),
    ))
    .await
    .expect("createTarget");

    let mut session_id: Option<String> = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    while session_id.is_none() && tokio::time::Instant::now() < deadline {
        let msg = tokio::time::timeout(Duration::from_secs(2), ws.next())
            .await
            .expect("timeout")
            .expect("closed")
            .expect("ws err");
        if let Message::Text(t) = msg {
            let v: Value = serde_json::from_str(&t).unwrap();
            if let Some(s) = v
                .get("params")
                .and_then(|p| p.get("sessionId"))
                .and_then(|s| s.as_str())
            {
                session_id = Some(s.to_string());
            }
        }
    }
    let sid = session_id.expect("sessionId from attach");

    ws.send(Message::Text(
        json!({
            "id": 2,
            "method": "Page.navigate",
            "sessionId": sid,
            "params": {"url": "https://google.com", "waitUntil": "load"}
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("navigate send");

    let mut refuse_msg = String::new();
    while tokio::time::Instant::now() < deadline {
        let msg = tokio::time::timeout(Duration::from_secs(2), ws.next())
            .await
            .expect("timeout")
            .expect("closed")
            .expect("ws err");
        if let Message::Text(t) = msg {
            let v: Value = serde_json::from_str(&t).unwrap();
            if v.get("id").and_then(|i| i.as_u64()) == Some(2) {
                if let Some(err) = v.get("error") {
                    refuse_msg = err.to_string();
                    break;
                }
            }
        }
    }

    assert!(
        refuse_msg.contains("REFUSE") || refuse_msg.contains("TERMINUS"),
        "external WS Page.navigate must be gated; got: {refuse_msg}"
    );

    // Receipt evidence: browser.action should record REFUSE for the navigate attempt
    let ledger = std::fs::read_to_string(dir.path().join("browser.actions.jsonl"))
        .unwrap_or_default();
    assert!(
        ledger.contains("REFUSE") && ledger.contains("navigate"),
        "ledger must contain REFUSE navigate receipt; got: {ledger}"
    );
}
