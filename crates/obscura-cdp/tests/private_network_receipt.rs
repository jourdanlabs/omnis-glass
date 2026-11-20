use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{sync::{Arc, atomic::{AtomicUsize, Ordering}}, time::Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[tokio::test(flavor = "current_thread")]
async fn navigation_receipt_respects_connection_private_network_permission() {
    let state = tempfile::tempdir().unwrap();
    std::env::set_var("OMNIS_GLASS_STATE_DIR", state.path());
    std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
    std::env::remove_var("OBSCURA_ALLOW_PRIVATE_NETWORK");
    std::env::set_var("OMNIS_GLASS_CDP_AUTH", "1");
    std::env::set_var("OMNIS_GLASS_CDP_TOKEN", "private-network-test");
    let fixture = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = fixture.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0)); let seen = hits.clone();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        for stream in fixture.incoming() {
            let Ok(mut stream) = stream else { break }; let mut buffer = [0; 4096];
            if stream.read(&mut buffer).unwrap_or(0) == 0 { continue; }
            seen.fetch_add(1, Ordering::SeqCst);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    });
    for (index, allow) in [false, true].into_iter().enumerate() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port(); drop(listener);
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(obscura_cdp::start_with_pane_observer_options(port, "127.0.0.1", None, false, None, false, None, allow, 8, obscura_cdp::PaneObserverOptions::default())).unwrap();
        });
        let ws = format!("ws://127.0.0.1:{port}/devtools/browser/private-network-test");
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(10), async {
            loop { if let Ok(socket) = connect_async(&ws).await { break socket; } tokio::time::sleep(Duration::from_millis(20)).await; }
        }).await.unwrap();
        socket.send(Message::Text(json!({"id":1,"method":"Target.createTarget","params":{"url":"about:blank"}}).to_string().into())).await.unwrap();
        let created = loop { let value: Value = serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if value["id"] == 1 { break value; } };
        let sid = format!("{}-session", created["result"]["targetId"].as_str().unwrap());
        let url = format!("http://{address}/case-{index}");
        socket.send(Message::Text(json!({"id":2,"method":"Page.navigate","sessionId":sid,"params":{"url":url}}).to_string().into())).await.unwrap();
        let response = tokio::time::timeout(Duration::from_secs(5), async { loop { let value: Value = serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if value["id"] == 2 { break value; } } }).await.unwrap();
        let entries: Vec<Value> = std::fs::read_to_string(state.path().join("browser.actions.jsonl")).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).filter(|entry: &Value| entry["action"] == "navigate" && entry["url"] == url).collect();
        if allow {
            assert!(response.get("error").is_none(), "{response}");
            assert_eq!(entries.iter().filter(|entry| entry["verdict"] == "ALLOW").count(), 1, "{entries:?}");
            assert_eq!(hits.load(Ordering::SeqCst), 1);
        } else {
            assert_eq!(entries.iter().filter(|entry| entry["verdict"] == "REFUSE").count(), 1, "{entries:?}");
            assert_eq!(entries.iter().filter(|entry| entry["verdict"] == "ALLOW").count(), 0, "{entries:?}");
            assert!(response["error"]["message"].as_str().unwrap_or("").contains("REFUSE_SSRF"), "{response}");
            assert_eq!(hits.load(Ordering::SeqCst), 0);
        }
    }
}

// A child process lets the resolver interposer affect the real server without
// changing the machine's DNS configuration or the test runner's resolver.
#[cfg(target_os = "macos")]
#[test]
fn lane_n_server_child() {
    let Ok(port) = std::env::var("LANE_N_SERVER_PORT") else { return; };
    let port: u16 = port.parse().unwrap();
    let allow = std::env::var("LANE_N_ALLOW_PRIVATE").unwrap() == "1";
    let proxy = std::env::var("LANE_N_PROXY").unwrap();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(obscura_cdp::start_with_pane_observer_options(
        port, "127.0.0.1", Some(proxy), false, None, false, None, allow, 8,
        obscura_cdp::PaneObserverOptions { token: Some("lane-n-observer".into()), mirror_only: false },
    )).unwrap();
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "current_thread")]
async fn key_event_rechecks_private_dns_for_agent_and_human() {
    use std::{io::{Read, Write}, process::{Command, Stdio}};
    type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
    async fn request(socket: &mut Socket, id: u64, method: &str, params: Value, session: Option<&str>) -> Value {
        let mut request = json!({"id":id, "method":method, "params":params});
        if let Some(session) = session { request["sessionId"] = json!(session); }
        socket.send(Message::Text(request.to_string().into())).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let value: Value = serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                if value["id"] == id { break value; }
            }
        }).await.expect("server response")
    }
    struct Server(std::process::Child);
    impl Drop for Server { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }

    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("resolver.c");
    let library = root.path().join("resolver.dylib");
    std::fs::write(&source, r#"
#include <netdb.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
static int resolve_test_host(const char *name, const char *service, const struct addrinfo *hints, struct addrinfo **result) {
    if (name && strcmp(name, "lane-n.test") == 0) {
        const char *path = getenv("LANE_N_DNS_SWITCH");
        name = path && access(path, F_OK) == 0 ? "127.0.0.1" : "93.184.216.34";
    }
    return getaddrinfo(name, service, hints, result);
}
__attribute__((used)) static struct { const void *replacement; const void *original; } interpose
    __attribute__((section("__DATA,__interpose"))) = { (const void *)resolve_test_host, (const void *)getaddrinfo };
"#).unwrap();
    let clang = Command::new("clang").args(["-dynamiclib", "-o"]).arg(&library).arg(&source).status()
        .expect("clang is required for the macOS private DNS key-event test");
    assert!(clang.success(), "clang failed to build the private DNS resolver fixture: {clang}");
    let fixture = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_address = fixture.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in fixture.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut request = [0; 4096];
            if stream.read(&mut request).unwrap_or(0) == 0 { continue; }
            let body = "<html><body><input id=field></body></html>";
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            let _ = stream.write_all(response.as_bytes());
        }
    });
    for allow in [false, true] {
        let dir = root.path().join(if allow { "allowed" } else { "strict" });
        std::fs::create_dir(&dir).unwrap();
        let switch = dir.join("private");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "lane_n_server_child", "--nocapture"])
            .env("LANE_N_SERVER_PORT", port.to_string())
            .env("LANE_N_ALLOW_PRIVATE", if allow { "1" } else { "0" })
            .env("LANE_N_PROXY", format!("http://{proxy_address}"))
            .env("LANE_N_DNS_SWITCH", &switch)
            .env("DYLD_INSERT_LIBRARIES", &library)
            .env("OMNIS_GLASS_STATE_DIR", &dir)
            .env("OMNIS_GLASS_ENFORCE", "local_only")
            .env("OMNIS_GLASS_CDP_AUTH", "1")
            .env("OMNIS_GLASS_CDP_TOKEN", "lane-n-test")
            .env_remove("OBSCURA_ALLOW_PRIVATE_NETWORK")
            .stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let _server = Server(child);
        let ws = format!("ws://127.0.0.1:{port}/devtools/browser/lane-n-test");
        let (mut agent, _) = tokio::time::timeout(Duration::from_secs(10), async {
            loop { if let Ok(socket) = connect_async(&ws).await { break socket; } tokio::time::sleep(Duration::from_millis(20)).await; }
        }).await.expect("server startup");
        let (mut observer, _) = connect_async(&ws).await.unwrap();
        let created = request(&mut agent, 1, "Target.createTarget", json!({"url":"about:blank"}), None).await;
        let target = created["result"]["targetId"].as_str().unwrap();
        let session = format!("{target}-session");
        let url = "http://lane-n.test/";
        let navigation = request(&mut agent, 2, "Page.navigate", json!({"url":url}), Some(&session)).await;
        assert!(navigation.get("error").is_none(), "{navigation}");
        std::fs::write(&switch, b"private").unwrap();
        let key = json!({"type":"keyDown", "key":"x", "text":"x"});
        let agent_result = request(&mut agent, 3, "Input.dispatchKeyEvent", key.clone(), Some(&session)).await;
        assert_eq!(request(&mut observer, 4, "Glass.authorizeObserver", json!({"token":"lane-n-observer"}), None).await["result"]["ok"], true);
        let targets = request(&mut observer, 5, "Glass.listTargets", json!({}), None).await;
        let entry = targets["result"]["targets"].as_array().unwrap().iter().find(|v| v["targetId"] == target && v["url"] == url).unwrap();
        let observed = request(&mut observer, 6, "Glass.observe", json!({"connectionId":entry["connectionId"], "targetId":target}), None).await;
        assert!(observed["result"]["observationId"].is_string(), "{observed}");
        assert_eq!(request(&mut observer, 7, "Glass.takeControl", json!({}), None).await["result"]["mode"], "human");
        let human_result = request(&mut observer, 8, "Glass.humanCommand", json!({"method":"Input.dispatchKeyEvent", "params":key}), None).await;
        let mut failures = Vec::new();
        for (driver, result) in [("agent", &agent_result), ("human", &human_result)] {
            let correct = if allow { result.get("error").is_none() }
                else { result["error"]["message"].as_str().unwrap_or("").contains("REFUSE_SSRF") };
            if !correct { failures.push(format!("{driver} response: {result}")); }
        }
        let receipts: Vec<Value> = std::fs::read_to_string(dir.join("browser.actions.jsonl")).unwrap().lines()
            .map(|line| serde_json::from_str(line).unwrap()).filter(|entry: &Value| entry["action"] == "type" && entry["url"] == url).collect();
        for driver in ["agent", "human"] {
            let matching: Vec<_> = receipts.iter().filter(|entry| entry["driver"] == driver).collect();
            assert_eq!(matching.len(), 1, "{receipts:?}");
            let expected = if allow { "ALLOW" } else { "REFUSE" };
            if matching[0]["verdict"] != expected { failures.push(format!("{driver} receipt: {}", matching[0])); }
        }
        assert!(failures.is_empty(), "{}", failures.join("; "));
    }
}
