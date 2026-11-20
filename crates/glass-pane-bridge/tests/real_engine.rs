#![cfg(feature = "render")]
use futures_util::{SinkExt, StreamExt};
use glass_pane_bridge::observer::ObserverClient;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[test]
fn ledger_lock_child() {
    let Some(dir) = std::env::var_os("GLASS_TEST_HELD_LEDGER") else { return; };
    let dir = std::path::Path::new(&dir);
    let file = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("browser.actions.jsonl")).unwrap();
    file.lock().unwrap();
    std::fs::write(dir.join("lock-ready"), "").unwrap();
    while !dir.join("lock-release").exists() { std::thread::sleep(Duration::from_millis(5)); }
}

#[tokio::test(flavor = "current_thread")]
async fn held_ledger_lock_refuses_navigation_without_blocking_other_connections() {
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
    struct Holder(std::process::Child);
    impl Drop for Holder { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }
    async fn call(ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, id: u64, method: &str, params: Value, session: Option<&str>) -> Value {
        let mut req = json!({"id":id,"method":method,"params":params});
        if let Some(session) = session { req["sessionId"] = json!(session); }
        ws.send(Message::Text(req.to_string().into())).await.unwrap();
        loop { let value: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if value["id"] == id { return value; } }
    }
    let state = tempfile::tempdir().unwrap();
    std::env::set_var("OMNIS_GLASS_STATE_DIR", state.path());
    std::env::set_var("OMNIS_GLASS_LEDGER_LOCK_TIMEOUT_MS", "400");
    std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    std::env::set_var("OMNIS_GLASS_CDP_TOKEN", "ledger-lock-token");
    std::env::set_var("OMNIS_GLASS_CDP_AUTH", "1");
    let fixture = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/page", fixture.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0)); let seen = requests.clone();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        for stream in fixture.incoming() {
            let Ok(mut stream) = stream else { break; }; let mut buf = [0; 2048]; let _ = stream.read(&mut buf);
            seen.fetch_add(1, Ordering::SeqCst);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    });
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap(); let port = socket.local_addr().unwrap().port(); drop(socket);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(obscura_cdp::start_with_serve_options_and_limit(port, "127.0.0.1", None, false, None, false, None, true, 8)).unwrap();
    });
    let ws_url = format!("ws://127.0.0.1:{port}/devtools/browser/ledger-lock-token");
    let (mut first, _) = tokio::time::timeout(Duration::from_secs(10), async {
        loop { if let Ok(ws) = connect_async(&ws_url).await { break ws; } tokio::time::sleep(Duration::from_millis(25)).await; }
    }).await.unwrap();
    let (mut second, _) = connect_async(&ws_url).await.unwrap();
    let a = call(&mut first, 1, "Target.createTarget", json!({"url":"about:blank"}), None).await;
    let b = call(&mut second, 1, "Target.createTarget", json!({"url":"about:blank"}), None).await;
    let sa = format!("{}-session", a["result"]["targetId"].as_str().unwrap());
    let sb = format!("{}-session", b["result"]["targetId"].as_str().unwrap());
    let mut holder = Holder(std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", "ledger_lock_child"])
        .env("GLASS_TEST_HELD_LEDGER", state.path()).spawn().unwrap());
    tokio::time::timeout(Duration::from_secs(30), async {
        while !state.path().join("lock-ready").exists() { tokio::time::sleep(Duration::from_millis(5)).await; }
    }).await.unwrap();
    let start = std::time::Instant::now();
    let (a, b) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(call(&mut first, 2, "Page.navigate", json!({"url":url}), Some(&sa)), call(&mut second, 2, "Page.navigate", json!({"url":url}), Some(&sb)))
    }).await.expect("both ledger refusals must be bounded");
    assert!(start.elapsed() < Duration::from_millis(650), "second connection waited too long: {:?}", start.elapsed());
    for response in [a, b] { assert!(response["error"]["message"].as_str().unwrap_or("").contains("REFUSE_LEDGER_UNAVAILABLE"), "{response}"); }
    let frame = call(&mut first, 3, "Page.getFrameTree", json!({}), Some(&sa)).await;
    assert_eq!(frame["result"]["frameTree"]["frame"]["url"], "about:blank");
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    std::fs::write(state.path().join("lock-release"), "").unwrap(); assert!(holder.0.wait().unwrap().success());
    let result = call(&mut first, 4, "Page.navigate", json!({"url":url}), Some(&sa)).await;
    assert!(result.get("error").is_none(), "{result}");
    assert_eq!(requests.load(Ordering::SeqCst), 1);
}

async fn frames_after_navigation(observer: &ObserverClient, events: &mut tokio::sync::mpsc::UnboundedReceiver<glass_pane_bridge::observer::ObserverEvent>, observation: &str, step: &str) {
    while let Ok(event) = events.try_recv() {
        if event.method == "Glass.frame" { observer.frame_ack(observation, &event.params["frameId"]).await.unwrap(); }
    }
    let mut frames = 0;
    tokio::time::timeout(Duration::from_secs(if cfg!(debug_assertions) { 6 } else { 2 }), async {
        while frames < 5 {
            let event = events.recv().await.unwrap();
            if event.method == "Glass.frame" {
                observer.frame_ack(observation, &event.params["frameId"]).await.unwrap();
                frames += 1;
            }
        }
    }).await.unwrap_or_else(|_| panic!("five acknowledged frames after {step}, received {frames}"));
}

#[tokio::test(flavor = "current_thread")]
async fn real_engine_observer_control_and_receipts() {
    let state = tempfile::tempdir().unwrap();
    std::env::set_var("OMNIS_GLASS_STATE_DIR", state.path());
    std::env::set_var("OMNIS_GLASS_ENFORCE", "enforce");
    std::env::set_var("OMNIS_GLASS_LANE", "browse:human");
    std::env::set_var("OMNIS_GLASS_LANE_HUMAN", "browse:human");
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    std::env::set_var("OMNIS_GLASS_CDP_AUTH", "1");
    std::env::set_var("OMNIS_GLASS_CDP_TOKEN", "cdp-test-token");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let fixture = format!("http://{}/", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break; };
            let mut request = [0; 2048]; let n = stream.read(&mut request).unwrap_or(0);
            let request = String::from_utf8_lossy(&request[..n]);
            let body = if request.starts_with("GET /refused-link ") {
                "<style>@keyframes a{from{opacity:1}to{opacity:.2}}body{animation:a .2s infinite alternate}</style><body><a href='http://127.0.0.2/blocked'>follow link</a></body>"
            } else {
                "<style>@keyframes a{from{opacity:1}to{opacity:.2}}body{animation:a .2s infinite alternate}</style><body><a href='/clicked'>follow link</a></body>"
            };
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
        }
    });
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port(); drop(socket);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(obscura_cdp::start_with_pane_observer_options(port, "127.0.0.1", None, false, None, false, None, true, 8,
            obscura_cdp::PaneObserverOptions { token: Some("observer-secret".into()), mirror_only: false })).unwrap();
    });
    let ws_url = format!("ws://127.0.0.1:{port}/devtools/browser/cdp-test-token");
    let (mut agent, _) = tokio::time::timeout(Duration::from_secs(10), async {
        loop { if let Ok(ws) = connect_async(&ws_url).await { break ws; } tokio::time::sleep(Duration::from_millis(25)).await; }
    }).await.unwrap();
    agent.send(Message::Text(json!({"id":1,"method":"Target.createTarget","params":{"url":"about:blank"}}).to_string().into())).await.unwrap();
    let created = loop { let v: Value = serde_json::from_str(agent.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if v["id"] == 1 { break v; } };
    let target_id = created["result"]["targetId"].as_str().unwrap();
    let session = format!("{target_id}-session");
    let (observer, mut events) = ObserverClient::connect(&ws_url).await.unwrap();
    observer.authorize("observer-secret").await.unwrap();
    let target = observer.list_targets().await.unwrap().into_iter().find(|t| t.target_id == target_id).unwrap();
    let observation = observer.observe(&target.connection_id, target_id).await.unwrap();
    let agent_url = format!("{fixture}agent");
    agent.send(Message::Text(json!({"id":2,"method":"Page.navigate","sessionId":session,"params":{"url":agent_url}}).to_string().into())).await.unwrap();
    loop { let v: Value = serde_json::from_str(agent.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if v["id"] == 2 { assert!(v.get("error").is_none(), "{v}"); break; } }
    let mut frames = 0; let mut navigated = false;
    tokio::time::timeout(Duration::from_secs(8), async {
        while frames < 1 || !navigated {
            let event = events.recv().await.unwrap();
            if event.method == "Glass.frame" { observer.frame_ack(&observation, &event.params["frameId"]).await.unwrap(); frames += 1; }
            if event.method == "Glass.targetNavigated" && event.params["url"] == agent_url { navigated = true; }
        }
    }).await.expect("frame and navigation event");
    frames_after_navigation(&observer, &mut events, &observation, "first agent navigation").await;
    for id in 10..13 {
        let url = format!("{fixture}agent-{id}");
        agent.send(Message::Text(json!({"id":id,"method":"Page.navigate","sessionId":session,"params":{"url":url}}).to_string().into())).await.unwrap();
        loop { let v: Value = serde_json::from_str(agent.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if v["id"] == id { assert!(v.get("error").is_none(), "{v}"); break; } }
        frames_after_navigation(&observer, &mut events, &observation, "repeated agent navigation").await;
    }
    let pending_frame = tokio::time::timeout(Duration::from_secs(2), async {
        loop { let event = events.recv().await.unwrap(); if event.method == "Glass.frame" { break event.params["frameId"].clone(); } }
    }).await.expect("frame to acknowledge during navigation");
    let navigation = json!({"id":13,"method":"Page.navigate","sessionId":session,"params":{"url":format!("{fixture}ack-in-flight")}}).to_string();
    let (ack, sent) = tokio::join!(observer.frame_ack(&observation, &pending_frame), agent.send(Message::Text(navigation.into())));
    ack.unwrap(); sent.unwrap();
    loop { let v: Value = serde_json::from_str(agent.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if v["id"] == 13 { assert!(v.get("error").is_none(), "{v}"); break; } }
    frames_after_navigation(&observer, &mut events, &observation, "ack in flight").await;
    let mut objects = Vec::new();
    for (id, expression) in [
        (20, "new Proxy({a:1},{ownKeys(t){document.title='ownKeys';document.body.setAttribute('data-agent','wrote');return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){document.title='descriptor';document.body.setAttribute('data-agent','wrote');return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k){document.title='get';document.body.setAttribute('data-agent','wrote');return Reflect.get(t,k)}})"),
        (21, "({get boom(){document.title='getter';document.body.setAttribute('data-agent','wrote');return 1}})"),
    ] {
        agent.send(Message::Text(json!({"id":id,"method":"Runtime.evaluate","sessionId":session,"params":{"expression":expression}}).to_string().into())).await.unwrap();
        let result = loop { let v: Value = serde_json::from_str(agent.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if v["id"] == id { break v; } };
        objects.push(result["result"]["result"]["objectId"].as_str().expect("remote object").to_string());
    }
    agent.send(Message::Text(json!({"id":22,"method":"Runtime.evaluate","sessionId":session,"params":{"expression":"(()=>{document.title='lock-baseline';document.body.removeAttribute('data-agent')})()"}}).to_string().into())).await.unwrap();
    loop { let v: Value = serde_json::from_str(agent.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if v["id"] == 22 { assert!(v.get("error").is_none(), "{v}"); break; } }
    assert_eq!(observer.take_control().await.unwrap().mode, "human");
    let mut request_id = 30;
    for object in &objects {
        for method in ["Browser.getVersion", "Target.getTargets", "Target.getTargetInfo", "Target.attachToTarget", "Target.detachFromTarget", "Page.getNavigationHistory", "Page.getFrameTree", "DOM.getDocument", "DOM.getAttributes", "Runtime.getProperties", "DOM.describeNode"] {
            request_id += 1;
            agent.send(Message::Text(json!({"id":request_id,"method":method,"sessionId":session,"params":{"objectId":object,"targetId":target_id,"sessionId":"unrelated"}}).to_string().into())).await.unwrap();
            let response = loop { let v: Value = serde_json::from_str(agent.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if v["id"] == request_id { break v; } };
            let locked = response["error"]["message"].as_str().unwrap_or("").contains("REFUSE_SESSION_LOCKED");
            assert_eq!(locked, matches!(method, "Runtime.getProperties" | "DOM.describeNode"), "{method}: {response}");
        }
    }
    observer.release().await.unwrap();
    agent.send(Message::Text(json!({"id":23,"method":"Runtime.evaluate","sessionId":session,"params":{"expression":"[document.title,document.body.getAttribute('data-agent')]","returnByValue":true}}).to_string().into())).await.unwrap();
    let result = loop { let v: Value = serde_json::from_str(agent.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if v["id"] == 23 { break v; } };
    assert_eq!(result["result"]["result"]["value"], json!(["lock-baseline", null]), "{result}");
    assert_eq!(observer.take_control().await.unwrap().mode, "human");
    let ledger = state.path().join("browser.actions.jsonl");
    let read = || -> Vec<Value> { std::fs::read_to_string(&ledger).unwrap().lines().map(|s| serde_json::from_str(s).unwrap()).collect() };
    let before = read().len();
    let human_url = format!("{fixture}human");
    observer.human_command("Page.navigate", json!({"url":human_url})).await.unwrap();
    frames_after_navigation(&observer, &mut events, &observation, "human navigation").await;
    let actions = read(); assert_eq!(actions.len(), before + 1, "{actions:?}");
    assert_eq!(actions.last().unwrap()["driver"], "human");
    let before_reload = read().len();
    observer.human_command("Page.reload", json!({})).await.unwrap();
    frames_after_navigation(&observer, &mut events, &observation, "human reload").await;
    assert_eq!(read().len(), before_reload + 1, "reload must write one receipt");
    let history = observer.human_command("Page.getNavigationHistory", json!({})).await.unwrap();
    let entry = history["entries"].as_array().unwrap().iter().find(|entry| entry["url"] == agent_url).unwrap()["id"].clone();
    let before_history = read().len();
    observer.human_command("Page.navigateToHistoryEntry", json!({"entryId":entry})).await.unwrap();
    frames_after_navigation(&observer, &mut events, &observation, "human history").await;
    assert_eq!(read().len(), before_history + 1, "history navigation must write one receipt");
    observer.human_command("Page.navigate", json!({"url":human_url})).await.unwrap();
    let refused = observer.human_command("Page.navigate", json!({"url":"http://127.0.0.2/blocked"})).await.unwrap_err();
    assert!(refused.message.contains("REFUSE_HOST"), "{refused}");
    assert_eq!(observer.list_targets().await.unwrap().into_iter().find(|t| t.target_id == target_id).unwrap().url, human_url);
    frames_after_navigation(&observer, &mut events, &observation, "before human click").await;
    let held = tokio::time::timeout(Duration::from_secs(2), async {
        let mut held = Vec::new();
        while held.len() < 2 { let event = events.recv().await.unwrap(); if event.method == "Glass.frame" { held.push(event.params["frameId"].clone()); } }
        held
    }).await.expect("fill the acknowledgement window before the click");
    let before_click = read().len();
    for kind in ["mousePressed", "mouseReleased"] {
        observer.human_command("Input.dispatchMouseEvent", json!({"type":kind,"x":12,"y":12,"button":"left","clickCount":1})).await.unwrap();
    }
    let clicked = format!("{fixture}clicked");
    tokio::time::timeout(Duration::from_secs(8), async {
        loop { if observer.list_targets().await.unwrap().into_iter().any(|t| t.target_id == target_id && t.url == clicked) { break; } tokio::time::sleep(Duration::from_millis(25)).await; }
    }).await.expect("human click navigates");
    for frame in held { observer.frame_ack(&observation, &frame).await.unwrap(); }
    frames_after_navigation(&observer, &mut events, &observation, "human click").await;
    let click_receipts: Vec<_> = read()[before_click..].iter().filter(|v| v["action"] == "navigate" && v["driver"] == "human" && v["url"] == clicked).cloned().collect();
    assert_eq!(click_receipts.len(), 1, "{click_receipts:?}"); assert_eq!(click_receipts[0]["verdict"], "ALLOW");
    let refused_page = format!("{fixture}refused-link");
    observer.human_command("Page.navigate", json!({"url":refused_page})).await.unwrap();
    let before_refused_click = read().len();
    observer.human_command("Input.dispatchMouseEvent", json!({"type":"mousePressed","x":12,"y":12,"button":"left","clickCount":1})).await.unwrap();
    let refusal = observer.human_command("Input.dispatchMouseEvent", json!({"type":"mouseReleased","x":12,"y":12,"button":"left","clickCount":1})).await.unwrap_err();
    assert!(refusal.message.contains("REFUSE_HOST"), "{refusal}");
    let refusal_receipts: Vec<_> = read()[before_refused_click..].iter().filter(|v| v["action"] == "navigate" && v["driver"] == "human" && v["url"] == "http://127.0.0.2/blocked").cloned().collect();
    assert_eq!(refusal_receipts.len(), 1, "{refusal_receipts:?}"); assert_eq!(refusal_receipts[0]["verdict"], "REFUSE");
    assert_eq!(observer.list_targets().await.unwrap().into_iter().find(|t| t.target_id == target_id).unwrap().url, refused_page);
    agent.send(Message::Text(json!({"id":3,"method":"Page.navigate","sessionId":session,"params":{"url":agent_url}}).to_string().into())).await.unwrap();
    loop { let v: Value = serde_json::from_str(agent.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if v["id"] == 3 { assert!(v["error"]["message"].as_str().unwrap().contains("REFUSE_SESSION_LOCKED")); break; } }
    tokio::time::timeout(Duration::from_secs(5), async { loop { let e = events.recv().await.unwrap(); if e.method == "Glass.modeChanged" && e.params["agentWaiting"] == true { break; } } }).await.expect("agent waiting event");
    observer.release().await.unwrap();
    agent.send(Message::Text(json!({"id":4,"method":"Page.navigate","sessionId":session,"params":{"url":agent_url}}).to_string().into())).await.unwrap();
    loop { let v: Value = serde_json::from_str(agent.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if v["id"] == 4 { assert!(v.get("error").is_none(), "{v}"); break; } }
}
