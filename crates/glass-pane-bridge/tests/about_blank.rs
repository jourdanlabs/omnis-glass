#![cfg(feature = "render")]
use futures_util::{SinkExt, StreamExt};
use glass_pane_bridge::observer::ObserverClient;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};

type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
async fn call(ws: &mut Socket, id: u64, method: &str, params: Value, session: Option<&str>) -> Value {
    let mut req = json!({"id":id,"method":method,"params":params});
    if let Some(session) = session { req["sessionId"] = json!(session); }
    ws.send(Message::Text(req.to_string().into())).await.unwrap();
    loop { let value: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if value["id"] == id { assert!(value.get("error").is_none(), "{value}"); return value; } }
}

#[tokio::test(flavor = "current_thread")]
async fn blank_reload_and_history_are_unreceipted_for_agent_and_human() {
    let state = tempfile::tempdir().unwrap();
    std::env::set_var("OMNIS_GLASS_STATE_DIR", state.path());
    std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    std::env::set_var("OMNIS_GLASS_CDP_AUTH", "1");
    std::env::set_var("OMNIS_GLASS_CDP_TOKEN", "blank-cdp");
    let fixture = std::net::TcpListener::bind("127.0.0.1:0").unwrap(); let url = format!("http://{}/page", fixture.local_addr().unwrap());
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        for stream in fixture.incoming() {
            let Ok(mut stream) = stream else { break; }; let mut buf = [0; 2048]; let _ = stream.read(&mut buf);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    });
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap(); let port = socket.local_addr().unwrap().port(); drop(socket);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(obscura_cdp::start_with_pane_observer_options(port, "127.0.0.1", None, false, None, false, None, true, 8,
            obscura_cdp::PaneObserverOptions { token: Some("blank-observer".into()), mirror_only: false })).unwrap();
    });
    let ws_url = format!("ws://127.0.0.1:{port}/devtools/browser/blank-cdp");
    let (mut agent, _) = tokio::time::timeout(Duration::from_secs(10), async {
        loop { if let Ok(ws) = connect_async(&ws_url).await { break ws; } tokio::time::sleep(Duration::from_millis(25)).await; }
    }).await.unwrap();
    let ledger = state.path().join("browser.actions.jsonl");
    let count = || std::fs::read_to_string(&ledger).unwrap_or_default().lines().count();
    let target = call(&mut agent, 1, "Target.createTarget", json!({"url":"about:blank"}), None).await;
    let target = target["result"]["targetId"].as_str().unwrap().to_string(); let session = format!("{target}-session");
    let before = count(); call(&mut agent, 2, "Page.reload", json!({}), Some(&session)).await; assert_eq!(count(), before, "agent blank reload wrote a receipt");
    call(&mut agent, 3, "Page.navigate", json!({"url":url}), Some(&session)).await;
    let history = call(&mut agent, 4, "Page.getNavigationHistory", json!({}), Some(&session)).await;
    let blank = history["result"]["entries"].as_array().unwrap().iter().find(|entry| entry["url"] == "about:blank").unwrap()["id"].clone();
    let before = count(); call(&mut agent, 5, "Page.navigateToHistoryEntry", json!({"entryId":blank}), Some(&session)).await; assert_eq!(count(), before, "agent blank history wrote a receipt");
    let frame = call(&mut agent, 6, "Page.getFrameTree", json!({}), Some(&session)).await; assert_eq!(frame["result"]["frameTree"]["frame"]["url"], "about:blank");
    let (observer, _events) = ObserverClient::connect(&ws_url).await.unwrap(); observer.authorize("blank-observer").await.unwrap();
    let found = observer.list_targets().await.unwrap().into_iter().find(|entry| entry.target_id == target).unwrap(); observer.observe(&found.connection_id, &target).await.unwrap(); observer.take_control().await.unwrap();
    let before = count(); observer.human_command("Page.reload", json!({})).await.unwrap(); assert_eq!(count(), before, "human blank reload wrote a receipt");
    observer.human_command("Page.navigate", json!({"url":url})).await.unwrap();
    let history = observer.human_command("Page.getNavigationHistory", json!({})).await.unwrap();
    let blank = history["entries"].as_array().unwrap().iter().find(|entry| entry["url"] == "about:blank").unwrap()["id"].clone();
    let before = count(); observer.human_command("Page.navigateToHistoryEntry", json!({"entryId":blank})).await.unwrap(); assert_eq!(count(), before, "human blank history wrote a receipt");
    assert_eq!(observer.list_targets().await.unwrap().into_iter().find(|entry| entry.target_id == target).unwrap().url, "about:blank");
}
