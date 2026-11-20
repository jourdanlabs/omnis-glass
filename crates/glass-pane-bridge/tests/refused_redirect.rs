#![cfg(feature = "render")]
use futures_util::{SinkExt, StreamExt};
use glass_pane_bridge::observer::ObserverClient;
use serde_json::{json, Value};
use std::{sync::{Arc, Mutex}, time::Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message};

type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
async fn call(ws: &mut Socket, id: u64, method: &str, params: Value, session: Option<&str>) -> Value {
    let mut req = json!({"id":id,"method":method,"params":params});
    if let Some(session) = session { req["sessionId"] = json!(session); }
    ws.send(Message::Text(req.to_string().into())).await.unwrap();
    loop { let value: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if value["id"] == id { assert!(value.get("error").is_none(), "{value}"); return value; } }
}

#[tokio::test(flavor = "current_thread")]
async fn refused_redirect_keeps_the_displayed_document_url() {
    let state = tempfile::tempdir().unwrap();
    std::env::set_var("OMNIS_GLASS_STATE_DIR", state.path());
    std::env::set_var("OMNIS_GLASS_ENFORCE", "enforce");
    std::env::set_var("OMNIS_GLASS_LANE", "browse:human");
    std::env::set_var("OMNIS_GLASS_LANE_HUMAN", "browse:human");
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    std::env::set_var("OMNIS_GLASS_CDP_AUTH", "1");
    std::env::set_var("OMNIS_GLASS_CDP_TOKEN", "redirect-cdp");
    let fixture = std::net::TcpListener::bind("127.0.0.1:0").unwrap(); let base = format!("http://{}", fixture.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::<String>::new())); let seen = requests.clone();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        for stream in fixture.incoming() {
            let Ok(mut stream) = stream else { break; }; let mut buf = [0; 2048]; let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]); let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
            seen.lock().unwrap().push(path.clone());
            if path == "/redirect" { let _ = stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.2/blocked\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"); }
            else { let body = "<title>PREVIOUS</title><body>previous document</body>"; let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body); }
        }
    });
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap(); let port = socket.local_addr().unwrap().port(); drop(socket);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(obscura_cdp::start_with_pane_observer_options(port, "127.0.0.1", None, false, None, false, None, true, 8,
            obscura_cdp::PaneObserverOptions { token: Some("redirect-observer".into()), mirror_only: false })).unwrap();
    });
    let ws_url = format!("ws://127.0.0.1:{port}/devtools/browser/redirect-cdp");
    let (mut agent, _) = tokio::time::timeout(Duration::from_secs(10), async {
        loop { if let Ok(ws) = connect_async(&ws_url).await { break ws; } tokio::time::sleep(Duration::from_millis(25)).await; }
    }).await.unwrap();
    let target = call(&mut agent, 1, "Target.createTarget", json!({"url":"about:blank"}), None).await;
    let target = target["result"]["targetId"].as_str().unwrap().to_string(); let session = format!("{target}-session");
    let (observer, _events) = ObserverClient::connect(&ws_url).await.unwrap(); observer.authorize("redirect-observer").await.unwrap();
    let found = observer.list_targets().await.unwrap().into_iter().find(|entry| entry.target_id == target).unwrap(); observer.observe(&found.connection_id, &target).await.unwrap(); observer.take_control().await.unwrap();
    let prior = format!("{base}/prior"); observer.human_command("Page.navigate", json!({"url":prior})).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async { loop { if observer.list_targets().await.unwrap().iter().any(|entry| entry.target_id == target && entry.url == prior) { break; } tokio::task::yield_now().await; } }).await.unwrap();
    let error = observer.human_command("Page.navigate", json!({"url":format!("{base}/redirect")})).await.unwrap_err();
    assert!(error.message.contains("REFUSE_HOST"), "{error}");
    let listed = observer.list_targets().await.unwrap().into_iter().find(|entry| entry.target_id == target).unwrap();
    assert_eq!(listed.url, prior, "target URL must describe the displayed document");
    observer.release().await.unwrap();
    let document = call(&mut agent, 2, "Runtime.evaluate", json!({"expression":"JSON.stringify([document.title,document.body.textContent])","returnByValue":true}), Some(&session)).await;
    assert_eq!(document["result"]["result"]["value"], "[\"PREVIOUS\",\"previous document\"]", "{document}");
    assert_eq!(*requests.lock().unwrap(), vec!["/prior", "/redirect"]);
    let ledger = std::fs::read_to_string(state.path().join("browser.actions.jsonl")).unwrap();
    let entries: Vec<Value> = ledger.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert!(entries.iter().any(|entry| entry["url"] == "http://127.0.0.2/blocked" && entry["action"] == "navigate" && entry["verdict"] == "REFUSE" && entry["driver"] == "human"), "{entries:?}");
}
