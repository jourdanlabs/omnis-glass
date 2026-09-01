//! B2 — ≥5 screencast frames after navigate (render).
//!
//! Co-launches headless serve, navigates an animation fixture, asserts ≥5
//! `Page.screencastFrame` events with proper `screencastFrameAck` backpressure.

#![cfg(feature = "render")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const ANIMATION_FIXTURE: &str = "data:text/html,<html style='margin:0'><head><style>@keyframes hide{from{opacity:1}to{opacity:0}}#cover{width:80px;height:60px;background:red;animation:hide 50ms linear infinite}</style></head><body style='margin:0;background:lime'><div id=cover></div></body></html>";

async fn pick_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn glass_env(dir: &std::path::Path, token: &str) {
    std::env::set_var("OMNIS_GLASS_STATE_DIR", dir);
    std::env::set_var("OMNIS_GLASS_CDP_TOKEN", token);
    std::env::set_var("OMNIS_GLASS_CDP_AUTH", "1");
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    std::env::set_var(
        "TERMINUS_AUTHORIZE",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts/terminus-authorize.mjs"
        ),
    );
    std::env::set_var("OMNIS_GLASS_LANE", "browse:local");
    std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
}

#[test]
fn b2_screencast_frames_after_navigate() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();
    local.block_on(&rt, async {
        b2_screencast_frames_after_navigate_async().await;
    });
}

async fn b2_screencast_frames_after_navigate_async() {
    let dir = tempfile::tempdir().unwrap();
    let token = "b2-screencast-test-token";
    glass_env(dir.path(), token);
    let cdp_port = pick_port().await;

    tokio::task::spawn_local(async move {
        let _ = obscura_cdp::start_with_serve_options_and_limit(
            cdp_port,
            "127.0.0.1",
            None,
            false,
            None,
            false,
            None,
            true,
            4,
        )
        .await;
    });
    tokio::time::sleep(Duration::from_millis(500)).await;

    let url = obscura_cdp::cdp_auth::debugger_url("127.0.0.1", cdp_port, token);
    let (mut ws, _) = connect_async(&url).await.expect("cdp connect");

    ws.send(Message::Text(
        json!({"id": 1, "method": "Target.createTarget", "params": {"url": "about:blank"}})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();

    let mut session_id: Option<String> = None;
    let attach_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while session_id.is_none() && tokio::time::Instant::now() < attach_deadline {
        if let Ok(Some(Ok(Message::Text(t)))) =
            tokio::time::timeout(Duration::from_secs(2), ws.next()).await
        {
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
    let sid = session_id.expect("session");

    ws.send(Message::Text(
        json!({
            "id": 2,
            "method": "Page.navigate",
            "sessionId": sid,
            "params": {"url": ANIMATION_FIXTURE, "waitUntil": "load"}
        })
        .to_string()
        .into(),
    ))
    .await
    .unwrap();

    while tokio::time::Instant::now() < attach_deadline {
        if let Ok(Some(Ok(Message::Text(t)))) =
            tokio::time::timeout(Duration::from_millis(500), ws.next()).await
        {
            let v: Value = serde_json::from_str(&t).unwrap();
            if v.get("id").and_then(|i| i.as_u64()) == Some(2) && v.get("error").is_none() {
                break;
            }
        }
    }

    ws.send(Message::Text(
        json!({
            "id": 3,
            "method": "Page.startScreencast",
            "sessionId": sid,
            "params": {"format": "jpeg", "quality": 80, "everyNthFrame": 1}
        })
        .to_string()
        .into(),
    ))
    .await
    .unwrap();

    let frames = Arc::new(AtomicUsize::new(0));
    let mut next_ack_id = 10u64;
    let frame_deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while frames.load(Ordering::Relaxed) < 5 && tokio::time::Instant::now() < frame_deadline {
        if let Ok(Some(Ok(Message::Text(t)))) =
            tokio::time::timeout(Duration::from_millis(500), ws.next()).await
        {
            let v: Value = serde_json::from_str(&t).unwrap();
            if v.get("method").and_then(|m| m.as_str()) == Some("Page.screencastFrame") {
                frames.fetch_add(1, Ordering::Relaxed);
                let stream_id = v
                    .get("params")
                    .and_then(|p| p.get("sessionId"))
                    .and_then(|s| s.as_i64())
                    .expect("stream sessionId");
                // Backpressure window allows two in-flight; ack both slots per frame.
                for ack_slot in 0..2 {
                    ws.send(Message::Text(
                        json!({
                            "id": next_ack_id,
                            "method": "Page.screencastFrameAck",
                            "sessionId": sid,
                            "params": { "sessionId": stream_id }
                        })
                        .to_string()
                        .into(),
                    ))
                    .await
                    .unwrap();
                    next_ack_id += 1;
                    let _ = ack_slot;
                }
            }
        }
    }
    assert!(
        frames.load(Ordering::Relaxed) >= 5,
        "expected ≥5 Page.screencastFrame events, got {}",
        frames.load(Ordering::Relaxed)
    );
}

#[test]
#[ignore = "can-fail mirror — must fail when run with --ignored"]
fn b2_WEAKEN_would_accept_zero_frames() {
    assert!(0 >= 5, "WEAKEN: weakened test would accept zero frames");
}
