#![cfg(feature = "render")]
use futures_util::{SinkExt, StreamExt};
use glass_pane_bridge::observer::ObserverClient;
use serde_json::{json, Value};
use std::{sync::{Arc, Mutex}, time::{Duration, Instant}};
use tokio_tungstenite::{connect_async, tungstenite::Message};

type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
async fn call(ws: &mut Socket, id: u64, method: &str, params: Value, session: Option<&str>) -> Value {
    let mut request = json!({"id":id,"method":method,"params":params});
    if let Some(session) = session { request["sessionId"] = json!(session); }
    ws.send(Message::Text(request.to_string().into())).await.unwrap();
    loop { let value: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if value["id"] == id { return value; } }
}

#[tokio::test(flavor = "current_thread")]
async fn autonomous_requests_follow_controlled_target_and_gate_other_pages() {
    let state = tempfile::tempdir().unwrap();
    std::env::set_var("OMNIS_GLASS_STATE_DIR", state.path());
    std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    std::env::set_var("OMNIS_GLASS_CDP_AUTH", "1");
    std::env::set_var("OMNIS_GLASS_CDP_TOKEN", "autonomous-cdp");
    let fixture = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", fixture.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::<String>::new())); let seen = requests.clone();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        for stream in fixture.incoming() {
            let Ok(mut stream) = stream else { break; }; let mut buf = [0; 4096];
            let n = stream.read(&mut buf).unwrap_or(0); let request = String::from_utf8_lossy(&buf[..n]);
            let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
            seen.lock().unwrap().push(path.clone());
            let body = match path.as_str() {
                "/other" => "<script>setTimeout(()=>fetch('/other-late').catch(()=>{}),350);setTimeout(()=>location.href='/other-dest',650)</script>",
                "/human" => "<title>start</title><script>fetch('/early');setTimeout(()=>fetch('/late').then(r=>r.text()).then(t=>document.title='LOADED-'+t),700);setTimeout(()=>fetch('/after'),3500)</script>",
                "/human-nav" => "<script>setTimeout(()=>location.href='/human-dest',400)</script>",
                _ => "ok",
            };
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
        }
    });
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap(); let port = socket.local_addr().unwrap().port(); drop(socket);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(obscura_cdp::start_with_pane_observer_options(port, "127.0.0.1", None, false, None, false, None, true, 8,
            obscura_cdp::PaneObserverOptions { token: Some("autonomous-observer".into()), mirror_only: false })).unwrap();
    });
    let ws = format!("ws://127.0.0.1:{port}/devtools/browser/autonomous-cdp");
    let (mut agent, _) = tokio::time::timeout(Duration::from_secs(10), async {
        loop { if let Ok(ws) = connect_async(&ws).await { break ws; } tokio::time::sleep(Duration::from_millis(25)).await; }
    }).await.unwrap();
    let a = call(&mut agent, 1, "Target.createTarget", json!({"url":"about:blank"}), None).await;
    let target = a["result"]["targetId"].as_str().unwrap().to_string(); let session = format!("{target}-session");
    let (observer, mut events) = ObserverClient::connect(&ws).await.unwrap(); observer.authorize("autonomous-observer").await.unwrap();
    let found = observer.list_targets().await.unwrap().into_iter().find(|entry| entry.target_id == target).unwrap();
    let observation = observer.observe(&found.connection_id, &target).await.unwrap();
    let ack = observer.clone(); tokio::spawn(async move { while let Some(event) = events.recv().await { if event.method == "Glass.frame" { let _ = ack.frame_ack(&observation, &event.params["frameId"]).await; } } });
    let b = call(&mut agent, 2, "Target.createTarget", json!({"url":"about:blank"}), None).await;
    let other = format!("{}-session", b["result"]["targetId"].as_str().unwrap());
    let loaded = call(&mut agent, 3, "Page.navigate", json!({"url":format!("{base}/other")}), Some(&other)).await;
    assert!(loaded.get("error").is_none(), "{loaded}");
    observer.take_control().await.unwrap();
    let blocked = call(&mut agent, 4, "Runtime.evaluate", json!({"expression":"fetch('/forbidden-command')"}), Some(&session)).await;
    assert!(blocked["error"]["message"].as_str().unwrap_or("").contains("REFUSE_SESSION_LOCKED"), "{blocked}");
    let ledger = state.path().join("browser.actions.jsonl");
    let read = || -> Vec<Value> { std::fs::read_to_string(&ledger).unwrap_or_default().lines().map(|line| serde_json::from_str(line).unwrap()).collect() };
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let entries = read();
            let fetch = entries.iter().any(|entry| entry["url"] == format!("{base}/other-late") && entry["action"] == "script-fetch" && entry["verdict"] == "REFUSE" && entry["driver"] == "agent");
            let nav = entries.iter().any(|entry| entry["url"] == format!("{base}/other-dest") && entry["action"] == "navigate" && entry["verdict"] == "REFUSE" && entry["driver"] == "agent");
            if fetch && nav { break; } tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("other page's fetch and navigation must each be refused and receipted");
    assert!(!requests.lock().unwrap().iter().any(|path| path == "/other-late" || path == "/other-dest" || path == "/forbidden-command"));
    observer.human_command("Page.navigate", json!({"url":format!("{base}/human")})).await.unwrap();
    tokio::time::timeout(Duration::from_secs(4), async {
        loop { if requests.lock().unwrap().iter().any(|path| path == "/late") { break; } tokio::time::sleep(Duration::from_millis(20)).await; }
    }).await.expect("human page late request");
    let entries = read();
    for path in ["/early", "/late"] {
        assert_eq!(entries.iter().filter(|entry| entry["url"] == format!("{base}{path}") && entry["action"] == "script-fetch" && entry["verdict"] == "ALLOW" && entry["driver"] == "human").count(), 1, "{entries:?}");
    }
    observer.release().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop { if requests.lock().unwrap().iter().any(|path| path == "/after") { break; } tokio::time::sleep(Duration::from_millis(20)).await; }
    }).await.expect("request after release");
    let entries = read(); assert!(entries.iter().any(|entry| entry["url"] == format!("{base}/after") && entry["action"] == "script-fetch" && entry["driver"] == "agent" && entry["verdict"] == "ALLOW"), "{entries:?}");
    let title = call(&mut agent, 5, "Runtime.evaluate", json!({"expression":"document.title", "returnByValue":true}), Some(&session)).await;
    assert_eq!(title["result"]["result"]["value"], "LOADED-ok", "{title}");
    observer.take_control().await.unwrap();
    observer.human_command("Page.navigate", json!({"url":format!("{base}/human-nav")})).await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if observer.list_targets().await.unwrap().into_iter().any(|entry| entry.target_id == target && entry.url == format!("{base}/human-dest")) { break; }
        assert!(Instant::now() < deadline, "human page navigation did not complete"); tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let entries = read(); assert!(entries.iter().any(|entry| entry["url"] == format!("{base}/human-dest") && entry["action"] == "navigate" && entry["driver"] == "human" && entry["verdict"] == "ALLOW"), "{entries:?}");
    assert!(!requests.lock().unwrap().iter().any(|path| path == "/forbidden-command"));
}
