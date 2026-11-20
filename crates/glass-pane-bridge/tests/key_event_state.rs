#![cfg(feature = "render")]
use futures_util::{SinkExt, StreamExt};
use glass_pane_bridge::observer::ObserverClient;
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::{Path, PathBuf}, process::{Child, Command, Stdio}, time::Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message};

type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
struct Server(Child);
impl Drop for Server { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }
async fn call(ws: &mut Socket, id: u64, method: &str, params: Value, session: Option<&str>) -> Value {
    let mut req = json!({"id":id,"method":method,"params":params});
    if let Some(s) = session { req["sessionId"] = json!(s); }
    ws.send(Message::Text(req.to_string().into())).await.unwrap();
    tokio::time::timeout(Duration::from_secs(15), async { loop {
        let value: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        if value["id"] == id { assert!(value.get("error").is_none(), "{value}"); break value; }
    }}).await.unwrap()
}
fn fixture() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break; };
            let mut buf = [0; 2048]; let _ = stream.read(&mut buf);
            let body = "<html><body><input id=p type=password style='position:absolute;left:20px;top:20px;width:240px;height:35px'></body></html>";
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
        }
    });
    format!("http://{address}/")
}
fn normalize(value: &mut Value) {
    match value {
        Value::Object(map) => { for k in ["ts", "prev", "sig", "signature", "key_id", "captured_at"] { map.remove(k); } for v in map.values_mut() { normalize(v); } },
        Value::Array(a) => for v in a { normalize(v); },
        _ => (),
    }
}
fn files(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() { files(root, &p, out); }
        else { out.insert(p.strip_prefix(root).unwrap().to_path_buf(), std::fs::read(&p).unwrap()); }
    }
}
async fn run_case(root: &Path, fixture_url: &str, text: &str, human: bool) {
    let lane = |name: &str| format!("  {name}:\n    sensitive: false\n    hosts:\n      - \"127.0.0.1\"\n    allowed_origins:\n      - \"http://127.0.0.1\"\n    egress_rule: record\n    max_download_bytes: 52428800\n    screenshot_policy: allow\n    allowed_residencies:\n      - local\n");
    let policy = format!("policy_version: \"key-test\"\ncoverage_class: \"browser-v1.1\"\n\nlanes:\n{}{}", lane("browse:research"), lane("browse:human"));
    let policy_path = root.join("lanes.yaml"); std::fs::write(&policy_path, policy).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap(); let port = listener.local_addr().unwrap().port(); drop(listener);
    let release = std::env::var("GLASS_RELEASE_TARGET").map(PathBuf::from).unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/release"));
    let _server = Server(Command::new(release.join("omnis-glass"))
        .env("OMNIS_GLASS_STATE_DIR", root).env("OMNIS_GLASS_LANES", &policy_path).env("OMNIS_GLASS_ENFORCE", "enforce")
        .env("OMNIS_GLASS_CDP_TOKEN", "key-test").env("OMNIS_GLASS_PANE_OBSERVER_TOKEN", "observer-test")
        .args(["serve", "--port", &port.to_string(), "--pane-observer", "--allow-private-network"])
        .stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap());
    let url = format!("ws://127.0.0.1:{port}/devtools/browser/key-test");
    let (mut agent, _) = tokio::time::timeout(Duration::from_secs(15), async { loop {
        if let Ok(ws) = connect_async(&url).await { break ws; }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }}).await.unwrap();
    let target = call(&mut agent, 1, "Target.createTarget", json!({"url":fixture_url}), None).await["result"]["targetId"].as_str().unwrap().to_string();
    let session = format!("{target}-session");
    if human {
        let (observer, _events) = ObserverClient::connect(&url).await.unwrap();
        observer.authorize("observer-test").await.unwrap();
        let entry = observer.list_targets().await.unwrap().into_iter().find(|t| t.target_id == target).unwrap();
        observer.observe(&entry.connection_id, &target).await.unwrap(); observer.take_control().await.unwrap();
        for (kind, buttons) in [("mousePressed",1), ("mouseReleased",0)] { observer.human_command("Input.dispatchMouseEvent", json!({"type":kind,"x":40,"y":35,"button":"left","buttons":buttons,"clickCount":1})).await.unwrap(); }
        for c in text.chars() { let s = c.to_string(); observer.human_command("Input.dispatchKeyEvent", json!({"type":"keyDown","key":s,"text":s})).await.unwrap(); }
        observer.release().await.unwrap();
    } else {
        for (id, kind, buttons) in [(2,"mousePressed",1), (3,"mouseReleased",0)] { call(&mut agent,id,"Input.dispatchMouseEvent",json!({"type":kind,"x":40,"y":35,"button":"left","buttons":buttons,"clickCount":1}),Some(&session)).await; }
        for (i,c) in text.chars().enumerate() { let s = c.to_string(); call(&mut agent, 10+i as u64,"Input.dispatchKeyEvent",json!({"type":"keyDown","key":s,"text":s}),Some(&session)).await; }
    }
    let value = call(&mut agent, 40, "Runtime.evaluate", json!({"expression":"document.getElementById('p').value","returnByValue":true}), Some(&session)).await;
    assert_eq!(value["result"]["result"]["value"], text);
    if human && text == "aB3!zQ7?" { if let Ok(path) = std::env::var("GLASS_TEST_TERMINUS_COPY") { std::fs::copy(root.join("terminus.actions.jsonl"), path).unwrap(); } }
    agent.send(Message::Text(json!({"id":41,"method":"Input.insertText","sessionId":session,"params":{"text":"whole-value"}}).to_string().into())).await.unwrap();
    loop { let v: Value = serde_json::from_str(agent.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if v["id"] == 41 { assert_eq!(v["error"]["message"], "Unknown Input method: insertText"); break; } }
}
async fn compare(human: bool) {
    let a = tempfile::tempdir().unwrap(); let b = tempfile::tempdir().unwrap(); let url = fixture();
    run_case(a.path(), &url, "aB3!zQ7?", human).await;
    run_case(b.path(), &url, "xY4#kM8@", human).await;
    let mut af = BTreeMap::new(); let mut bf = BTreeMap::new(); files(a.path(), a.path(), &mut af); files(b.path(), b.path(), &mut bf);
    assert_eq!(af.keys().collect::<Vec<_>>(), bf.keys().collect::<Vec<_>>());
    for name in ["browser.actions.jsonl", "terminus.actions.jsonl"] { assert!(af.contains_key(Path::new(name)), "missing {name}"); }
    for (name, left) in af {
        let right = &bf[&name];
        if name.extension().and_then(|v| v.to_str()) == Some("jsonl") {
            let parse = |bytes: &[u8]| -> Vec<Value> { std::str::from_utf8(bytes).unwrap().lines().map(|l| {
                let mut v: Value = serde_json::from_str(l).unwrap();
                if v["action"] == "type" && v.get("chars").is_none() {
                    let mut keys: Vec<_> = v.as_object().unwrap().keys().map(String::as_str).collect(); keys.sort();
                    if v["receipt"] == "terminus.action" {
                        assert_eq!(keys, vec!["action", "canon_version", "decision", "host", "kind", "lane", "payload_digest", "prev", "reason", "receipt", "refused", "sig", "ts", "verdict"]);
                    } else {
                        assert_eq!(keys, vec!["action", "canon_version", "driver", "key_id", "lane", "prev", "receipt", "sig", "signature", "ts", "url", "value_sha256", "verdict"]);
                    }
                }
                normalize(&mut v); v
            }).collect() };
            let al = parse(&left); let bl = parse(right);
            assert_eq!(al, bl, "different normalized records in {}", name.display());
            if name == Path::new("browser.actions.jsonl") || name == Path::new("terminus.actions.jsonl") {
                assert_eq!(al.iter().filter(|r| r["action"] == "type" && r.get("chars").is_some()).count(), 8);
            }
            for r in al.iter().filter(|r| r["action"] == "type") {
                if r.get("chars").is_some() {
                    assert_eq!(r["chars"], 1);
                    assert!(r.get("value_sha256").is_none());
                    if r["receipt"] == "terminus.action" { assert!(r["payload_digest"].is_null()); }
                } else if r["receipt"] == "terminus.action" {
                    assert_eq!(r["payload_digest"], glass_ledger::digest_sha256(r#"{"field_value":"whole-value"}"#));
                    let mut keys: Vec<_> = r.as_object().unwrap().keys().map(String::as_str).collect(); keys.sort();
                    assert_eq!(keys, vec!["action", "canon_version", "decision", "host", "kind", "lane", "payload_digest", "reason", "receipt", "refused", "verdict"]);
                } else {
                    assert_eq!(r["value_sha256"], glass_ledger::digest_sha256("whole-value"));
                    let mut keys: Vec<_> = r.as_object().unwrap().keys().map(String::as_str).collect(); keys.sort();
                    assert_eq!(keys, vec!["action", "canon_version", "driver", "lane", "receipt", "url", "value_sha256", "verdict"]);
                }
            }
        } else if name != Path::new("writer.key") { assert_eq!(&left, right, "different file {}", name.display()); }
    }
}
#[tokio::test(flavor="current_thread")] async fn human_key_events_do_not_change_state() { compare(true).await; }
#[tokio::test(flavor="current_thread")] async fn agent_key_events_do_not_change_state() { compare(false).await; }
