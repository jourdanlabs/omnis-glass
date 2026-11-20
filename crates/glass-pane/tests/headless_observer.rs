use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::{net::TcpListener, time::{sleep, Duration}};
use tokio_tungstenite::{accept_async, tungstenite::Message};

async fn run_check(sent_frames: usize, required: usize) -> (std::process::Output, Vec<String>, Vec<u64>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        let mut seen = Vec::new();
        let mut acknowledgements = Vec::new();
        while let Some(Ok(message)) = ws.next().await {
            let Message::Text(text) = message else { continue };
            let req: Value = serde_json::from_str(&text).unwrap();
            let method = req["method"].as_str().unwrap();
            seen.push(method.to_string());
            if method == "Glass.frameAck" { acknowledgements.push(req["params"]["frameId"].as_u64().unwrap()); }
            let result = match method {
                "Glass.authorizeObserver" => json!({"ok":true}),
                "Glass.listTargets" => json!({"targets":[{"connectionId":"agent","targetId":"page","url":"https://example.test","title":"","lastActivityMs":10,"scriptsStopped":false}]}),
                "Glass.observe" => json!({"observationId":"obs"}),
                "Glass.getMode" => json!({"mode":"mirror","agentWaiting":false}),
                "Glass.frameAck" => json!({}),
                _ => panic!("unexpected {method}"),
            };
            if method == "Glass.getMode" && sent_frames > 0 {
                ws.send(Message::Text(json!({"method":"Glass.frame","params":{"observationId":"obs","frameId":0,"data":"AA==","metadata":{"deviceWidth":2,"deviceHeight":2,"timestamp":0}}}).to_string().into())).await.unwrap();
            }
            ws.send(Message::Text(json!({"id":req["id"],"result":result}).to_string().into())).await.unwrap();
            if method == "Glass.getMode" {
                for id in 1..sent_frames {
                    ws.send(Message::Text(json!({"method":"Glass.frame","params":{"observationId":"obs","frameId":id,"data":"AA==","metadata":{"deviceWidth":2,"deviceHeight":2,"timestamp":0}}}).to_string().into())).await.unwrap();
                }
            }
            if method == "Glass.frameAck" && seen.iter().filter(|m| m.as_str() == "Glass.frameAck").count() == sent_frames {
                if sent_frames < required { sleep(Duration::from_secs(2)).await; }
                break;
            }
        }
        (seen, acknowledgements)
    });
    let output = tokio::task::spawn_blocking(move || std::process::Command::new(env!("CARGO_BIN_EXE_glass-pane"))
        .args(["--headless", "--attach", &format!("ws://{addr}"), "--observer-token", "secret", "--frames", &required.to_string(), "--timeout-secs", "1"])
        .output().unwrap()).await.unwrap();
    let (seen, acknowledgements) = server.await.unwrap();
    (output, seen, acknowledgements)
}
#[tokio::test]
async fn headless_requires_requested_frames_and_acks_each() {
    let (success, seen, acknowledgements) = run_check(2, 2).await;
    assert!(success.status.success(), "{}", String::from_utf8_lossy(&success.stderr));
    let output: Value = serde_json::from_slice(&success.stdout).unwrap();
    assert_eq!(output["frames"], 2);
    assert_eq!(seen.iter().filter(|m| m.as_str() == "Glass.frameAck").count(), 2);
    assert_eq!(acknowledgements, vec![0, 1]);
    let (failure, _, _) = run_check(1, 2).await;
    assert!(!failure.status.success());
    let output: Value = serde_json::from_slice(&failure.stdout).unwrap();
    assert_eq!(output["frames"], 1);
    assert!(output["error"].is_string());
}

#[tokio::test]
async fn wrong_observer_token_stops_before_target_listing() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        let request: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(request["method"], "Glass.authorizeObserver");
        ws.send(Message::Text(json!({"id":request["id"],"error":{"message":"OBSERVER_TOKEN_INVALID wrong token"}}).to_string().into())).await.unwrap();
        match tokio::time::timeout(Duration::from_secs(2), ws.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => Some(text.to_string()),
            _ => None,
        }
    });
    let output = tokio::task::spawn_blocking(move || std::process::Command::new(env!("CARGO_BIN_EXE_glass-pane"))
        .args(["--headless", "--attach", &format!("ws://{addr}"), "--observer-token", "wrong", "--frames", "1", "--timeout-secs", "1"])
        .output().unwrap()).await.unwrap();
    assert!(!output.status.success());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["error"], "OBSERVER_TOKEN_INVALID wrong token");
    assert_eq!(server.await.unwrap(), None);
}

#[tokio::test]
async fn attach_supplies_cdp_token_without_losing_query() {
    use std::sync::{Arc, Mutex};
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(String::new()));
    let recorded = seen.clone();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_hdr_async(stream, move |request: &Request, response: Response| {
            *recorded.lock().unwrap() = request.uri().to_string();
            Ok(response)
        }).await.unwrap();
        let request: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        ws.send(Message::Text(json!({"id":request["id"],"error":{"message":"OBSERVER_TOKEN_INVALID"}}).to_string().into())).await.unwrap();
    });
    let output = tokio::task::spawn_blocking(move || std::process::Command::new(env!("CARGO_BIN_EXE_glass-pane"))
        .args(["--headless", "--attach", &format!("ws://{addr}/devtools/browser/?source=test"), "--cdp-token", "cdp-secret", "--observer-token", "bad", "--frames", "1", "--timeout-secs", "2"])
        .output().unwrap()).await.unwrap();
    assert!(!output.status.success());
    server.await.unwrap();
    assert_eq!(&*seen.lock().unwrap(), "/devtools/browser/cdp-secret?source=test");
}
