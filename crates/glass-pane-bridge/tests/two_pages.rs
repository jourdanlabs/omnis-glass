#![cfg(feature = "render")]
use futures_util::{SinkExt, StreamExt};
use glass_pane_bridge::observer::{choose_target, refresh_target, ObserverClient};
use serde_json::{json, Value};
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};

type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
async fn call(ws: &mut Socket, id: u64, method: &str, params: Value, session: Option<&str>) -> Value {
    let mut request = json!({"id":id,"method":method,"params":params});
    if let Some(session) = session { request["sessionId"] = json!(session); }
    ws.send(Message::Text(request.to_string().into())).await.unwrap();
    loop { let v: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if v["id"] == id { assert!(v.get("error").is_none(), "{v}"); return v; } }
}
async fn setup() -> (tempfile::TempDir, Socket, String) {
    let state = tempfile::tempdir().unwrap();
    std::env::set_var("OMNIS_GLASS_STATE_DIR", state.path());
    std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    std::env::set_var("OMNIS_GLASS_CDP_AUTH", "1");
    std::env::set_var("OMNIS_GLASS_CDP_TOKEN", "two-pages-token");
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap(); let port = socket.local_addr().unwrap().port(); drop(socket);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(obscura_cdp::start_with_pane_observer_options(port, "127.0.0.1", None, false, None, false, None, true, 8,
            obscura_cdp::PaneObserverOptions { token: Some("observer-secret".into()), mirror_only: false })).unwrap();
    });
    let url = format!("ws://127.0.0.1:{port}/devtools/browser/two-pages-token");
    let (ws, _) = tokio::time::timeout(Duration::from_secs(10), async { loop { if let Ok(ws) = connect_async(&url).await { break ws; } tokio::time::sleep(Duration::from_millis(25)).await; } }).await.unwrap();
    (state, ws, url)
}
fn fixture() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break; }; let mut request = [0; 2048]; let _ = stream.read(&mut request);
            let body = "<html><head><style>@keyframes a{from{opacity:1}to{opacity:.2}}body{animation:a .2s infinite alternate}</style></head><body>page</body></html>";
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
        }
    }); url
}
async fn create(ws: &mut Socket, id: u64) -> (String, String) {
    let result = call(ws, id, "Target.createTarget", json!({"url":"about:blank"}), None).await;
    let target = result["result"]["targetId"].as_str().unwrap().to_string();
    let session = format!("{target}-session"); (target, session)
}
async fn animate(ws: &mut Socket, id: u64, session: &str) {
    call(ws, id, "Runtime.evaluate", json!({"expression":"document.body.innerHTML='<style>@keyframes a{from{opacity:1}to{opacity:.2}}body{animation:a .2s infinite alternate}</style>page'"}), Some(session)).await;
}
async fn frames(observer: &ObserverClient, events: &mut tokio::sync::mpsc::UnboundedReceiver<glass_pane_bridge::observer::ObserverEvent>, observation: &str) {
    let mut count = 0;
    tokio::time::timeout(Duration::from_secs(if cfg!(debug_assertions) { 6 } else { 2 }), async { while count < 5 {
        let e = events.recv().await.unwrap(); if e.method == "Glass.frame" && e.params["observationId"] == observation { observer.frame_ack(observation, &e.params["frameId"]).await.unwrap(); count += 1; }
    }}).await.unwrap_or_else(|_| panic!("five acknowledged frames, received {count}"));
}
#[tokio::test(flavor = "current_thread")]
async fn follower_tracks_working_page() {
    let (_state, mut agent, url) = setup().await;
    let page_url = fixture();
    let (a, sa) = create(&mut agent, 1).await;
    call(&mut agent, 2, "Page.navigate", json!({"url":page_url}), Some(&sa)).await;
    let (observer, mut events) = ObserverClient::connect(&url).await.unwrap(); observer.authorize("observer-secret").await.unwrap();
    let (mut current, _) = choose_target(&observer, None).await.unwrap(); assert_eq!(current.target_id, a);
    let mut observation = observer.observe(&current.connection_id, &current.target_id).await.unwrap();
    frames(&observer, &mut events, &observation).await;
    let (b, sb) = create(&mut agent, 3).await;
    call(&mut agent, 4, "Page.navigate", json!({"url":page_url}), Some(&sb)).await;
    call(&mut agent, 8, "Runtime.evaluate", json!({"expression":"1"}), Some(&sa)).await;
    while let Ok(event) = events.try_recv() {
        if event.method == "Glass.frame" && event.params["observationId"] == observation { observer.frame_ack(&observation, &event.params["frameId"]).await.unwrap(); }
    }
    for (id, target, session, method, params) in [
        (5, &b, &sb, "Runtime.evaluate", json!({"expression":"1"})),
        (6, &a, &sa, "Runtime.evaluate", json!({"expression":"1"})),
        (7, &b, &sb, "Input.dispatchMouseEvent", json!({"type":"mousePressed","x":1,"y":1,"button":"left"})),
    ] {
        call(&mut agent, id, method, params, Some(session)).await;
        tokio::time::timeout(Duration::from_secs(2), async {
            while current.target_id != *target {
                let event = events.recv().await.unwrap();
                if event.method == "Glass.frame" && event.params["observationId"] == observation { observer.frame_ack(&observation, &event.params["frameId"]).await.unwrap(); }
                if let Some((next, _, next_observation)) = refresh_target(&observer, &event, None, &current, &observation).await.unwrap() { current = next; observation = next_observation; }
            }
        }).await.expect("follower switches target");
        assert_eq!(&current.target_id, target);
        frames(&observer, &mut events, &observation).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn suspended_observation_resumes_when_agent_returns() {
    let (_state, mut agent, url) = setup().await;
    let (a, sa) = create(&mut agent, 1).await;
    call(&mut agent, 2, "Page.navigate", json!({"url":fixture()}), Some(&sa)).await;
    let (observer, mut events) = ObserverClient::connect(&url).await.unwrap(); observer.authorize("observer-secret").await.unwrap();
    let target = observer.list_targets().await.unwrap().into_iter().find(|t| t.target_id == a).unwrap();
    let observation = observer.observe(&target.connection_id, &a).await.unwrap();
    frames(&observer, &mut events, &observation).await;
    let (_b, sb) = create(&mut agent, 3).await;
    animate(&mut agent, 4, &sb).await;
    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(200), events.recv()).await {
        if event.method == "Glass.frame" { observer.frame_ack(&observation, &event.params["frameId"]).await.unwrap(); }
    }
    call(&mut agent, 5, "Runtime.evaluate", json!({"expression":"1"}), Some(&sa)).await;
    frames(&observer, &mut events, &observation).await;
}

#[tokio::test(flavor = "current_thread")]
async fn observing_suspended_page_does_not_wake_it_and_human_can_navigate() {
    let (_state, mut agent, url) = setup().await;
    let page_url = fixture();
    let (a, sa) = create(&mut agent, 1).await;
    call(&mut agent, 2, "Page.navigate", json!({"url":page_url}), Some(&sa)).await;
    let (b, sb) = create(&mut agent, 3).await;
    call(&mut agent, 4, "Page.navigate", json!({"url":page_url}), Some(&sb)).await;
    call(&mut agent, 5, "Runtime.evaluate", json!({"expression":"window.__keep='b-state'"}), Some(&sb)).await;
    let (watch_b, mut events_b) = ObserverClient::connect(&url).await.unwrap(); watch_b.authorize("observer-secret").await.unwrap();
    let target_b = watch_b.list_targets().await.unwrap().into_iter().find(|t| t.target_id == b).unwrap();
    let obs_b = watch_b.observe(&target_b.connection_id, &b).await.unwrap();
    let (watch_a, mut events_a) = ObserverClient::connect(&url).await.unwrap(); watch_a.authorize("observer-secret").await.unwrap();
    let obs_a = watch_a.observe(&target_b.connection_id, &a).await.unwrap();
    let result = call(&mut agent, 6, "Runtime.evaluate", json!({"expression":"window.__keep","returnByValue":true}), Some(&sb)).await;
    assert_eq!(result["result"]["result"]["value"], "b-state", "observing a suspended page changed the live page");
    frames(&watch_b, &mut events_b, &obs_b).await;
    while let Ok(event) = events_a.try_recv() { assert_ne!(event.method, "Glass.frame", "suspended page produced a frame"); }
    assert_eq!(watch_a.take_control().await.unwrap().mode, "human");
    watch_a.human_command("Page.navigate", json!({"url":page_url})).await.unwrap();
    frames(&watch_a, &mut events_a, &obs_a).await;
}

#[tokio::test(flavor = "current_thread")]
async fn first_blank_and_remaining_page_can_be_observed() {
    let (_state, mut agent, url) = setup().await;
    let (a, sa) = create(&mut agent, 1).await;
    let (observer, mut events) = ObserverClient::connect(&url).await.unwrap(); observer.authorize("observer-secret").await.unwrap();
    let target = observer.list_targets().await.unwrap().into_iter().find(|t| t.target_id == a).unwrap();
    let observation = observer.observe(&target.connection_id, &a).await.unwrap();
    tokio::time::timeout(Duration::from_secs(if cfg!(debug_assertions) { 6 } else { 2 }), async {
        loop { let event = events.recv().await.unwrap(); if event.method == "Glass.frame" { observer.frame_ack(&observation, &event.params["frameId"]).await.unwrap(); break; } }
    }).await.expect("first blank target produces a frame");
    observer.stop_observing().await.unwrap();
    call(&mut agent, 2, "Page.navigate", json!({"url":fixture()}), Some(&sa)).await;
    let (b, sb) = create(&mut agent, 3).await;
    call(&mut agent, 4, "Page.reload", json!({}), Some(&sb)).await;
    call(&mut agent, 5, "Target.closeTarget", json!({"targetId":b}), None).await;
    let observation = observer.observe(&target.connection_id, &a).await.unwrap();
    frames(&observer, &mut events, &observation).await;
}

#[tokio::test(flavor = "current_thread")]
async fn agent_reload_and_history_keep_mirror_live() {
    let (_state, mut agent, url) = setup().await;
    let base = fixture(); let first = format!("{base}one"); let second = format!("{base}two");
    let (target_id, session) = create(&mut agent, 1).await;
    call(&mut agent, 2, "Page.navigate", json!({"url":first}), Some(&session)).await;
    let (observer, mut events) = ObserverClient::connect(&url).await.unwrap(); observer.authorize("observer-secret").await.unwrap();
    let target = observer.list_targets().await.unwrap().into_iter().find(|t| t.target_id == target_id).unwrap();
    let observation = observer.observe(&target.connection_id, &target_id).await.unwrap();
    frames(&observer, &mut events, &observation).await;
    call(&mut agent, 3, "Page.navigate", json!({"url":second}), Some(&session)).await;
    let history = call(&mut agent, 4, "Page.getNavigationHistory", json!({}), Some(&session)).await;
    let entry = history["result"]["entries"].as_array().unwrap().iter().find(|entry| entry["url"] == first).unwrap()["id"].clone();
    for (id, method, params) in [
        (5, "Page.reload", json!({})), (6, "Page.reload", json!({})),
        (7, "Page.navigateToHistoryEntry", json!({"entryId":entry})), (8, "Page.reload", json!({})),
    ] {
        call(&mut agent, id, method, params, Some(&session)).await;
        while let Ok(event) = events.try_recv() {
            if event.method == "Glass.frame" && event.params["observationId"] == observation { observer.frame_ack(&observation, &event.params["frameId"]).await.unwrap(); }
        }
        if id == 7 { assert_eq!(observer.list_targets().await.unwrap().into_iter().find(|t| t.target_id == target_id).unwrap().url, first); }
        frames(&observer, &mut events, &observation).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn attached_rail_rejects_other_state_directory() {
    use glass_pane_bridge::receipt_tail::{actions_ledger_path_hash, ReceiptTailer, DIFFERENT_STATE_MESSAGE};
    let (server_state, _agent, url) = setup().await;
    let other_state = tempfile::tempdir().unwrap();
    let (client, _) = ObserverClient::connect(&url).await.unwrap();
    let server_hash = client.authorize("observer-secret").await.unwrap();
    assert_eq!(server_hash, actions_ledger_path_hash(server_state.path()));
    #[cfg(unix)] {
        let aliases = tempfile::tempdir().unwrap(); let link = aliases.path().join("linked");
        std::os::unix::fs::symlink(server_state.path(), &link).unwrap();
        assert_eq!(server_hash, actions_ledger_path_hash(&link));
    }
    let expected_path = std::fs::canonicalize(server_state.path()).unwrap().join("browser.actions.jsonl");
    assert_eq!(server_hash.as_deref(), Some(glass_ledger::digest_sha256(expected_path.to_str().unwrap()).as_str()));
    assert_eq!(server_hash.as_ref().unwrap().len(), 64);
    let receipt = "{\"action\":\"navigate\",\"verdict\":\"ALLOW\",\"url\":\"https://example.test/\"}\n";
    std::fs::write(server_state.path().join("browser.actions.jsonl"), receipt).unwrap();
    std::fs::write(other_state.path().join("browser.actions.jsonl"), receipt).unwrap();
    let mut wrong = ReceiptTailer::for_attach(other_state.path(), server_hash.as_deref());
    assert_eq!(wrong.warning(), Some(DIFFERENT_STATE_MESSAGE));
    assert!(wrong.poll_actions().unwrap().is_empty(), "different folder must not send a receipt to the UI");
    let missing = other_state.path().join("not-yet-created").join("nested");
    let missing_hash = actions_ledger_path_hash(&missing).unwrap();
    assert!(!missing.exists()); std::fs::create_dir_all(&missing).unwrap();
    assert_eq!(Some(missing_hash), actions_ledger_path_hash(&missing));
    let mut old = ReceiptTailer::for_attach(server_state.path(), None);
    assert_eq!(old.warning(), Some(DIFFERENT_STATE_MESSAGE)); assert!(old.poll_actions().unwrap().is_empty());
    let mut same = ReceiptTailer::for_attach(server_state.path(), server_hash.as_deref());
    assert_eq!(same.warning(), None);
    let entries = same.poll_actions().unwrap(); assert_eq!(entries.len(), 1); assert_eq!(entries[0].action, "navigate");
}
