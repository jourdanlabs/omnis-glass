use std::time::Duration;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn setup(token: Option<&str>) -> (Socket, Socket) {
    setup_with_mirror(token, false).await
}

async fn setup_with_mirror(token: Option<&str>, mirror_only: bool) -> (Socket, Socket) {
    std::env::set_var("OMNIS_GLASS_CDP_AUTH", "1");
    std::env::set_var("OMNIS_GLASS_CDP_TOKEN", "observer-test-cdp-token");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let token = token.map(str::to_string);
    tokio::task::spawn_local(async move {
        obscura_cdp::start_with_pane_observer_options(
            port, "127.0.0.1", None, false, None, false, None, true, 8,
            obscura_cdp::PaneObserverOptions { token, mirror_only },
        ).await.unwrap();
    });
    let url = format!("ws://127.0.0.1:{port}/devtools/browser/observer-test-cdp-token");
    let first = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match connect_async(&url).await {
                Ok((ws, _)) => break ws,
                Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        }
    }).await.expect("server startup");
    let (second, _) = connect_async(&url).await.unwrap();
    (first, second)
}

async fn request(socket: &mut Socket, id: u64, method: &str, params: Value) -> Value {
    socket.send(Message::Text(json!({"id": id, "method": method, "params": params}).to_string().into())).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = socket.next().await.unwrap().unwrap();
            if let Message::Text(text) = message {
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["id"] == id { break value; }
            }
        }
    }).await.expect("CDP response")
}

fn state_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() { files.extend(state_files(&path)); }
        else if path.is_file() { files.push(path); }
    }
    files
}

fn run(test: impl std::future::Future<Output = ()>) {
    let state = tempfile::tempdir().unwrap();
    std::env::set_var("OMNIS_GLASS_STATE_DIR", state.path());
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    tokio::task::LocalSet::new().block_on(&rt, test);
}

#[test]
fn disabled_refuses_every_glass_method() {
    run(async {
        let (_agent, mut observer) = setup(None).await;
        for (id, method) in ["Glass.authorizeObserver", "Glass.listTargets", "Glass.observe", "Glass.stopObserving", "Glass.frameAck", "Glass.getMode", "Glass.takeControl", "Glass.release", "Glass.humanCommand"].iter().enumerate() {
            let response = request(&mut observer, id as u64 + 1, method, json!({})).await;
            assert_eq!(response["error"]["message"], "OBSERVER_DISABLED", "{method}");
        }
    });
}

#[test]
fn empty_observer_token_disables_observer() {
    run(async {
        let (_agent, mut observer) = setup(Some("")).await;
        for (id, method, params) in [(1, "Glass.authorizeObserver", json!({"token":""})), (2, "Glass.authorizeObserver", json!({})), (3, "Glass.listTargets", json!({}))] {
            let response = request(&mut observer, id, method, params).await;
            assert_eq!(response["error"]["message"], "OBSERVER_DISABLED", "{response}");
        }
    });
}

#[test]
fn authorization_and_cross_connection_targets() {
    run(async {
        let (mut agent, mut observer) = setup(Some("observer-secret")).await;
        for (id, method) in [(1, "Glass.listTargets"), (2, "Glass.getMode")] {
            let response = request(&mut observer, id, method, json!({})).await;
            assert_eq!(response["error"]["message"], "OBSERVER_NOT_AUTHORIZED");
        }
        let wrong = request(&mut observer, 3, "Glass.authorizeObserver", json!({"token": "observer-secreu"})).await;
        assert_eq!(wrong["error"]["message"], "OBSERVER_TOKEN_INVALID");
        let response = request(&mut agent, 4, "Target.createTarget", json!({"url": "about:blank"})).await;
        let target = response["result"]["targetId"].as_str().unwrap().to_owned();
        let authorized = request(&mut observer, 5, "Glass.authorizeObserver", json!({"token": "observer-secret"})).await;
        assert_eq!(authorized["result"]["ok"], true);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let list = request(&mut observer, 6, "Glass.listTargets", json!({})).await;
            if let Some(entry) = list["result"]["targets"].as_array().unwrap().iter().find(|entry| entry["targetId"] == target) {
                assert!(entry["connectionId"].as_str().is_some());
                assert_eq!(entry["url"], "about:blank");
                assert!(entry["title"].is_string());
                assert!(entry["lastActivityMs"].as_u64().unwrap() > 0);
                break;
            }
            assert!(tokio::time::Instant::now() < deadline, "target snapshot did not appear");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
}

#[cfg(feature = "render")]
#[test]
fn scripts_stopped_events_follow_page_switch_and_human_reload() {
    run(async {
        std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
        std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
        let url = fixture_url().await;
        let (mut agent, mut observer) = setup(Some("observer-secret")).await;
        let first = request(&mut agent, 1, "Target.createTarget", json!({"url":"about:blank"})).await["result"]["targetId"].as_str().unwrap().to_owned();
        agent.send(Message::Text(json!({"id":12,"method":"Page.navigate","sessionId":format!("{first}-session"),"params":{"url":url}}).to_string().into())).await.unwrap();
        loop {
            let v: Value = serde_json::from_str(agent.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            if v["id"] == 12 { assert!(v.get("error").is_none(), "{v}"); break; }
        }
        assert_eq!(request(&mut observer, 2, "Glass.authorizeObserver", json!({"token":"observer-secret"})).await["result"]["ok"], true);
        let list = request(&mut observer, 3, "Glass.listTargets", json!({})).await;
        let entry = list["result"]["targets"].as_array().unwrap().iter().find(|v| v["targetId"] == first).unwrap();
        assert_eq!(entry["scriptsStopped"], false);
        let owner = entry["connectionId"].as_str().unwrap().to_owned();
        let second = request(&mut agent, 4, "Target.createTarget", json!({"url":"about:blank"})).await["result"]["targetId"].as_str().unwrap().to_owned();
        agent.send(Message::Text(json!({"id":5,"method":"Page.navigate","sessionId":format!("{second}-session"),"params":{"url":url}}).to_string().into())).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let v: Value = serde_json::from_str(observer.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                if v["method"] == "Glass.scriptsStoppedChanged" && v["params"]["targetId"] == first { assert_eq!(v["params"]["scriptsStopped"], true); break; }
            }
        }).await.expect("stopped event without listing");
        let list = request(&mut observer, 6, "Glass.listTargets", json!({})).await;
        assert_eq!(list["result"]["targets"].as_array().unwrap().iter().find(|v| v["targetId"] == first).unwrap()["scriptsStopped"], true);
        assert!(request(&mut observer, 7, "Glass.observe", json!({"connectionId":owner,"targetId":first})).await["result"]["observationId"].is_string());
        assert_eq!(request(&mut observer, 8, "Glass.takeControl", json!({})).await["result"]["mode"], "human");
        observer.send(Message::Text(json!({"id":9,"method":"Glass.humanCommand","params":{"method":"Page.reload","params":{}}}).to_string().into())).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let v: Value = serde_json::from_str(observer.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                if v["method"] == "Glass.scriptsStoppedChanged" && v["params"]["targetId"] == first && v["params"]["scriptsStopped"] == false { break; }
            }
        }).await.expect("reload event");
        let list = request(&mut observer, 10, "Glass.listTargets", json!({})).await;
        assert_eq!(list["result"]["targets"].as_array().unwrap().iter().find(|v| v["targetId"] == first).unwrap()["scriptsStopped"], false);
        agent.send(Message::Text(json!({"id":11,"method":"Glass.listTargets","params":{}}).to_string().into())).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let v: Value = serde_json::from_str(agent.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                assert_ne!(v["method"], "Glass.scriptsStoppedChanged", "unauthorized event");
                if v["id"] == 11 { assert_eq!(v["error"]["message"], "OBSERVER_NOT_AUTHORIZED"); break; }
            }
        }).await.unwrap();
    });
}

#[cfg(feature = "render")]
#[test]
fn observation_frames_ack_backpressure_and_navigation() {
    run(async {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
        std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
        let fixture = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = fixture.local_addr().unwrap();
        tokio::task::spawn_local(async move {
            while let Ok((mut stream, _)) = fixture.accept().await {
                tokio::task::spawn_local(async move {
                    let mut buf = [0u8; 4096];
                    let _ = stream.read(&mut buf).await;
                    let body = "<html><head><title>Animated</title><style>@keyframes move{from{opacity:1}to{opacity:0}}#box{width:80px;height:60px;background:red;animation:move 2s linear infinite alternate}</style></head><body><div id=box></div></body></html>";
                    let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                    let _ = stream.write_all(reply.as_bytes()).await;
                });
            }
        });
        let (mut agent, mut observer) = setup(Some("observer-secret")).await;
        let created = request(&mut agent, 1, "Target.createTarget", json!({"url":"about:blank"})).await;
        let target = created["result"]["targetId"].as_str().unwrap().to_owned();
        let sid = format!("{target}-session");
        let url = format!("http://{address}/one");
        agent.send(Message::Text(json!({"id":2,"method":"Page.navigate","sessionId":sid,"params":{"url":url}}).to_string().into())).await.unwrap();
        loop { let message = agent.next().await.unwrap().unwrap(); let v: Value = serde_json::from_str(message.to_text().unwrap()).unwrap(); if v["id"] == 2 { assert!(v.get("error").is_none(), "{v}"); break; } }
        assert_eq!(request(&mut observer, 3, "Glass.authorizeObserver", json!({"token":"observer-secret"})).await["result"]["ok"], true);
        let entry = loop {
            let response = request(&mut observer, 4, "Glass.listTargets", json!({})).await;
            if let Some(entry) = response["result"]["targets"].as_array().unwrap().iter().find(|v| v["targetId"] == target && v["url"] == url) { break entry.clone(); }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        let owner = entry["connectionId"].as_str().unwrap();
        let response = request(&mut observer, 5, "Glass.observe", json!({"connectionId":owner,"targetId":target})).await;
        let observation = response["result"]["observationId"].as_str().unwrap().to_owned();
        let mut frames = 0;
        tokio::time::timeout(Duration::from_secs(8), async {
            while frames < 5 {
                let message = observer.next().await.unwrap().unwrap();
                let v: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
                if v["method"] == "Glass.frame" {
                    assert_eq!(v["params"]["observationId"], observation);
                    assert!(v["params"]["data"].as_str().unwrap().len() > 10);
                    assert!(v["params"]["metadata"]["deviceWidth"].as_f64().unwrap() > 0.0);
                    frames += 1;
                    observer.send(Message::Text(json!({"id":100+frames,"method":"Glass.frameAck","params":{"observationId":observation,"frameId":v["params"]["frameId"]}}).to_string().into())).await.unwrap();
                }
            }
        }).await.expect("five acknowledged frames");
        let new_url = format!("http://{address}/two");
        agent.send(Message::Text(json!({"id":6,"method":"Page.navigate","sessionId":sid,"params":{"url":new_url}}).to_string().into())).await.unwrap();
        tokio::time::timeout(Duration::from_secs(8), async {
            let mut changed = false;
            loop {
                let v: Value = serde_json::from_str(observer.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                if v["method"] == "Glass.targetsChanged" { changed = true; }
                if v["method"] == "Glass.targetNavigated" && v["params"]["url"] == new_url { assert!(changed); break; }
            }
        }).await.expect("target navigation event");
        observer.send(Message::Text(json!({"id":7,"method":"Glass.observe","params":{"connectionId":owner,"targetId":target}}).to_string().into())).await.unwrap();
        let replacement = tokio::time::timeout(Duration::from_secs(5), async {
            let mut replaced = false;
            loop {
                let v: Value = serde_json::from_str(observer.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                if v["method"] == "Glass.observationEnded" && v["params"]["observationId"] == observation && v["params"]["reason"] == "replaced" { replaced = true; }
                if v["id"] == 7 { assert!(replaced, "replacement event must precede reply"); break v["result"]["observationId"].as_str().unwrap().to_owned(); }
            }
        }).await.expect("replacement response");
        let mut unacked = 0;
        let deadline = tokio::time::Instant::now() + Duration::from_millis(800);
        while let Ok(Some(Ok(message))) = tokio::time::timeout_at(deadline, observer.next()).await {
            let v: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
            if v["method"] == "Glass.frame" && v["params"]["observationId"] == replacement { unacked += 1; }
        }
        assert!(unacked > 0 && unacked <= 2, "received {unacked} unacknowledged frames");
        assert!(request(&mut observer, 8, "Glass.stopObserving", json!({})).await["result"].is_object());
    });
}

#[cfg(feature = "render")]
#[test]
fn observation_is_scoped_to_owner_and_ends_on_disconnect() {
    run(async {
        use base64::Engine as _;
        use image::GenericImageView;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
        std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
        let fixture = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = fixture.local_addr().unwrap();
        tokio::task::spawn_local(async move {
            while let Ok((mut stream, _)) = fixture.accept().await {
                tokio::task::spawn_local(async move {
                    let mut buf = [0u8; 4096];
                    let n = stream.read(&mut buf).await.unwrap();
                    let color = if String::from_utf8_lossy(&buf[..n]).contains("/red") { "red" } else { "blue" };
                    let body = format!("<html><body style='margin:0;background:{color}'></body></html>");
                    let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                    let _ = stream.write_all(reply.as_bytes()).await;
                });
            }
        });
        let (mut red_agent, mut observer) = setup(Some("observer-secret")).await;
        let peer = match observer.get_ref() { MaybeTlsStream::Plain(stream) => stream.peer_addr().unwrap(), _ => unreachable!() };
        let url = format!("ws://{peer}/devtools/browser/observer-test-cdp-token");
        let (mut blue_agent, _) = connect_async(&url).await.unwrap();
        let red_url = format!("http://{address}/red");
        let blue_url = format!("http://{address}/blue");
        let red = request(&mut red_agent, 1, "Target.createTarget", json!({"url":red_url})).await;
        let blue = request(&mut blue_agent, 1, "Target.createTarget", json!({"url":blue_url})).await;
        assert!(red["result"]["targetId"].is_string());
        assert!(blue["result"]["targetId"].is_string());
        assert_eq!(request(&mut observer, 2, "Glass.authorizeObserver", json!({"token":"observer-secret"})).await["result"]["ok"], true);
        let (owner, target) = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let response = request(&mut observer, 3, "Glass.listTargets", json!({})).await;
                let entries = response["result"]["targets"].as_array().unwrap();
                let red = entries.iter().find(|entry| entry["url"] == red_url);
                let blue = entries.iter().find(|entry| entry["url"] == blue_url);
                if let (Some(red), Some(blue)) = (red, blue) {
                    assert_ne!(red["connectionId"], blue["connectionId"]);
                    break (red["connectionId"].as_str().unwrap().to_string(), red["targetId"].as_str().unwrap().to_string());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("both targets listed");
        let response = request(&mut observer, 4, "Glass.observe", json!({"connectionId":owner,"targetId":target})).await;
        let id = response["result"]["observationId"].as_str().unwrap().to_string();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let v: Value = serde_json::from_str(observer.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                if v["method"] == "Glass.frame" {
                    assert_eq!(v["params"]["observationId"], id);
                    let bytes = base64::engine::general_purpose::STANDARD.decode(v["params"]["data"].as_str().unwrap()).unwrap();
                    let image = image::load_from_memory(&bytes).unwrap();
                    let pixel = image.get_pixel(10, 10).0;
                    assert!(pixel[0] > 200 && pixel[2] < 50, "frame must belong to red target: {pixel:?}");
                    break;
                }
            }
        }).await.expect("red frame");
        assert!(request(&mut red_agent, 8, "Target.closeTarget", json!({"targetId":target})).await["result"].is_object());
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let v: Value = serde_json::from_str(observer.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                if v["method"] == "Glass.observationEnded" {
                    assert_eq!(v["params"]["observationId"], id);
                    assert_eq!(v["params"]["reason"], "target_closed");
                    break;
                }
            }
        }).await.expect("observation ended on target close");
        let list = request(&mut observer, 9, "Glass.listTargets", json!({})).await;
        let blue = list["result"]["targets"].as_array().unwrap().iter().find(|entry| entry["url"] == blue_url).unwrap();
        let response = request(&mut observer, 10, "Glass.observe", json!({"connectionId":blue["connectionId"],"targetId":blue["targetId"]})).await;
        let blue_id = response["result"]["observationId"].as_str().unwrap().to_string();
        blue_agent.close(None).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let v: Value = serde_json::from_str(observer.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                if v["method"] == "Glass.observationEnded" {
                    assert_eq!(v["params"]["observationId"], blue_id);
                    assert_eq!(v["params"]["reason"], "connection_closed");
                    break;
                }
            }
        }).await.expect("observation ended on owner disconnect");
    });
}

async fn session_request(socket: &mut Socket, id: u64, method: &str, session: &str, params: Value) -> Value {
    socket.send(Message::Text(json!({"id":id, "method":method, "sessionId":session, "params":params}).to_string().into())).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let value: Value = serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            if value["id"] == id { break value; }
        }
    }).await.expect("session response")
}

async fn fixture_url() -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::task::spawn_local(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::task::spawn_local(async move {
                let mut buf = [0u8; 1024]; let _ = stream.read(&mut buf).await;
                let body = "<html><title>lock fixture</title><body><button id=button onclick=\"this.setAttribute('clicked','yes')\">click</button></body></html>";
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    format!("http://{address}/")
}

#[test]
fn damaged_ledger_refuses_navigation_until_restored() {
    run(async {
        let state = tempfile::tempdir().unwrap();
        std::env::set_var("OMNIS_GLASS_STATE_DIR", state.path());
        std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
        std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
        let url = fixture_url().await;
        let (mut agent, _observer) = setup(Some("observer-secret")).await;
        let target = request(&mut agent, 1, "Target.createTarget", json!({"url":"about:blank"})).await["result"]["targetId"].as_str().unwrap().to_string();
        let session = format!("{target}-session");
        let initial = session_request(&mut agent, 10, "Page.navigate", &session, json!({"url":url})).await;
        assert!(initial.get("error").is_none(), "{initial}");
        let destination = format!("{url}?second");
        let ledger = state.path().join("browser.actions.jsonl");
        let valid = std::fs::read(&ledger).unwrap();
        assert!(valid.ends_with(b"\n"));
        std::fs::write(&ledger, &valid[..valid.len()-1]).unwrap();
        let denied = session_request(&mut agent, 2, "Page.navigate", &session, json!({"url":destination})).await;
        assert!(denied["error"]["message"].as_str().unwrap().contains("REFUSE_LEDGER_UNAVAILABLE"), "{denied}");
        let before = request(&mut agent, 3, "Target.getTargetInfo", json!({"targetId":target})).await;
        assert_eq!(before["result"]["targetInfo"]["url"], url);
        std::fs::write(&ledger, &valid).unwrap();
        let allowed = session_request(&mut agent, 4, "Page.navigate", &session, json!({"url":destination})).await;
        assert!(allowed.get("error").is_none(), "{allowed}");
        let after = request(&mut agent, 5, "Target.getTargetInfo", json!({"targetId":target})).await;
        assert_eq!(after["result"]["targetInfo"]["url"], destination);
    });
}

#[test]
fn control_lock_refuses_agent_and_release_restores_it() {
    run(async {
        std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
        std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
        let url = fixture_url().await;
        let (mut agent, mut observer) = setup(Some("observer-secret")).await;
        let created = request(&mut agent, 1, "Target.createTarget", json!({"url":"about:blank"})).await;
        let target = created["result"]["targetId"].as_str().unwrap();
        let session = format!("{target}-session");
        request(&mut observer, 2, "Glass.authorizeObserver", json!({"token":"observer-secret"})).await;
        let initial = request(&mut observer, 3, "Glass.getMode", json!({})).await;
        assert_eq!(initial["result"], json!({"mode":"mirror","agentWaiting":false}));
        let control = request(&mut observer, 4, "Glass.takeControl", json!({})).await;
        assert_eq!(control["result"]["mode"], "human");
        let refused = session_request(&mut agent, 5, "Page.navigate", &session, json!({"url":url})).await;
        assert!(refused["error"]["message"].as_str().unwrap().contains("REFUSE_SESSION_LOCKED"), "{refused}");
        let create_refused = request(&mut agent, 50, "Target.createTarget", json!({"url":url})).await;
        assert!(create_refused["error"]["message"].as_str().unwrap().contains("REFUSE_SESSION_LOCKED"), "{create_refused}");
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let value: Value = serde_json::from_str(observer.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                if value["method"] == "Glass.modeChanged" && value["params"]["agentWaiting"] == true { assert_eq!(value["params"]["mode"], "human"); break; }
            }
        }).await.expect("waiting event");
        let targets = request(&mut agent, 6, "Target.getTargets", json!({})).await;
        assert_eq!(targets["result"]["targetInfos"].as_array().unwrap().len(), 1);
        assert_eq!(targets["result"]["targetInfos"][0]["url"], "about:blank");
        let released = request(&mut observer, 7, "Glass.release", json!({})).await;
        assert_eq!(released["result"]["mode"], "mirror");
        let mode = request(&mut observer, 8, "Glass.getMode", json!({})).await;
        assert_eq!(mode["result"], json!({"mode":"mirror","agentWaiting":false}));
        let navigate = session_request(&mut agent, 9, "Page.navigate", &session, json!({"url":url})).await;
        assert!(navigate.get("error").is_none(), "{navigate}");
    });
}

#[test]
fn control_refuses_mutations_and_enforces_observer_owner() {
    run(async {
        std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
        std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
        let url = fixture_url().await;
        let (mut agent, mut observer) = setup(Some("observer-secret")).await;
        let address = match agent.get_ref() { MaybeTlsStream::Plain(stream) => stream.peer_addr().unwrap(), _ => panic!("expected plain socket") };
        let (mut other, _) = connect_async(format!("ws://{address}/devtools/browser/observer-test-cdp-token")).await.unwrap();
        let target = request(&mut agent, 1, "Target.createTarget", json!({"url":url})).await["result"]["targetId"].as_str().unwrap().to_string();
        let session = format!("{target}-session");
        for kind in ["mousePressed", "mouseReleased"] {
            let sent = session_request(&mut agent, 70, "Input.dispatchMouseEvent", &session,
                json!({"type":kind,"x":12,"y":12,"button":"left","clickCount":1})).await;
            assert!(sent.get("error").is_none(), "{sent}");
        }
        let clicked = session_request(&mut agent, 71, "Runtime.evaluate", &session,
            json!({"expression":"document.getElementById('button').getAttribute('clicked')","returnByValue":true})).await;
        assert_eq!(clicked["result"]["result"]["value"], "yes", "fixture click must change the page: {clicked}");
        let reset = session_request(&mut agent, 72, "Page.navigate", &session, json!({"url":url})).await;
        assert!(reset.get("error").is_none(), "{reset}");
        request(&mut observer, 2, "Glass.authorizeObserver", json!({"token":"observer-secret"})).await;
        request(&mut other, 3, "Glass.authorizeObserver", json!({"token":"observer-secret"})).await;
        assert_eq!(request(&mut observer, 4, "Glass.takeControl", json!({})).await["result"]["mode"], "human");
        for (id, method, params) in [
            (10, "Input.dispatchMouseEvent", json!({"type":"mousePressed","x":12,"y":12,"button":"left","clickCount":1})),
            (15, "Input.dispatchMouseEvent", json!({"type":"mouseReleased","x":12,"y":12,"button":"left","clickCount":1})),
            (11, "Runtime.evaluate", json!({"expression":"document.body.innerHTML='changed'"})),
            (12, "DOM.setAttributeValue", json!({"nodeId":1,"name":"changed","value":"yes"})),
            (13, "Target.createTarget", json!({"url":"about:blank"})),
            (14, "Target.closeTarget", json!({"targetId":target})),
        ] {
            let before = session_request(&mut agent, id + 100, "DOM.getDocument", &session, json!({"depth":-1})).await;
            let before_targets = request(&mut agent, id + 200, "Target.getTargets", json!({})).await;
            assert!(before.get("error").is_none(), "{before}");
            let denied = session_request(&mut agent, id, method, &session, params).await;
            assert!(denied["error"]["message"].as_str().unwrap().contains("REFUSE_SESSION_LOCKED"), "{denied}");
            let after = session_request(&mut agent, id + 300, "DOM.getDocument", &session, json!({"depth":-1})).await;
            let after_targets = request(&mut agent, id + 400, "Target.getTargets", json!({})).await;
            assert_eq!(before["result"], after["result"], "{method}");
            assert_eq!(before_targets["result"], after_targets["result"], "{method}");
        }
        for (id, method, params, code) in [
            (20, "Glass.takeControl", json!({}), "CONTROL_HELD"),
            (21, "Glass.release", json!({}), "NOT_CONTROL_OWNER"),
            (22, "Glass.humanCommand", json!({"method":"Page.navigate","params":{"url":"about:blank"}}), "NOT_CONTROL_OWNER"),
        ] {
            let denied = request(&mut other, id, method, params).await;
            assert_eq!(denied["error"]["message"], code, "{denied}");
        }
        assert_eq!(request(&mut observer, 30, "Glass.release", json!({})).await["result"]["mode"], "mirror");
    });
}

#[test]
fn control_lock_released_when_observer_disconnects() {
    run(async {
        std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
        std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
        let url = fixture_url().await;
        let (mut agent, mut observer) = setup(Some("observer-secret")).await;
        let target = request(&mut agent, 1, "Target.createTarget", json!({"url":"about:blank"})).await["result"]["targetId"].as_str().unwrap().to_string();
        request(&mut observer, 2, "Glass.authorizeObserver", json!({"token":"observer-secret"})).await;
        assert_eq!(request(&mut observer, 3, "Glass.takeControl", json!({})).await["result"]["mode"], "human");
        observer.close(None).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if !glass_govern::agent_blocked() { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("disconnect releases lock");
        let result = session_request(&mut agent, 4, "Page.navigate", &format!("{target}-session"), json!({"url":url})).await;
        assert!(result.get("error").is_none(), "{result}");
    });
}

#[test]
fn mirror_only_refuses_control() {
    run(async {
        let (_agent, mut observer) = setup_with_mirror(Some("observer-secret"), true).await;
        request(&mut observer, 1, "Glass.authorizeObserver", json!({"token":"observer-secret"})).await;
        let result = request(&mut observer, 2, "Glass.takeControl", json!({})).await;
        assert_eq!(result["error"]["message"], "MIRROR_ONLY");
        let mode = request(&mut observer, 3, "Glass.getMode", json!({})).await;
        assert_eq!(mode["result"], json!({"mode":"mirror","agentWaiting":false}));
    });
}

#[cfg(feature = "render")]
#[test]
fn human_commands_use_human_lane_and_preserve_refused_page() {
    run(async {
        let state = tempfile::tempdir().unwrap();
        std::env::set_var("OMNIS_GLASS_STATE_DIR", state.path());
        std::env::set_var("OMNIS_GLASS_ENFORCE", "enforce");
        std::env::set_var("OMNIS_GLASS_LANE", "browse:local");
        std::env::set_var("OMNIS_GLASS_LANE_HUMAN", "browse:human");
        std::env::remove_var("OBSCURA_ALLOW_PRIVATE_NETWORK");
        let fixture = fixture_url().await;
        let (mut agent, mut observer) = setup(Some("observer-secret")).await;
        let target = request(&mut agent, 1, "Target.createTarget", json!({"url":"about:blank"})).await["result"]["targetId"].as_str().unwrap().to_string();
        let sid = format!("{target}-session");
        let initial_url = format!("{fixture}initial");
        assert!(session_request(&mut agent, 2, "Page.navigate", &sid, json!({"url":initial_url})).await.get("error").is_none());
        request(&mut observer, 3, "Glass.authorizeObserver", json!({"token":"observer-secret"})).await;
        let owner = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let list = request(&mut observer, 4, "Glass.listTargets", json!({})).await;
                if let Some(target) = list["result"]["targets"].as_array().unwrap().iter().find(|entry| entry["url"] == initial_url) { break target["connectionId"].as_str().unwrap().to_string(); }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        let observed = request(&mut observer, 5, "Glass.observe", json!({"connectionId":owner,"targetId":target})).await;
        assert!(observed["result"]["observationId"].is_string(), "{observed}");
        let allowed_url = format!("{fixture}human");
        let command = |url: &str| json!({"method":"Page.navigate","params":{"url":url}});
        let before = request(&mut observer, 6, "Glass.humanCommand", command(&allowed_url)).await;
        assert_eq!(before["error"]["message"], "NOT_HUMAN_MODE");
        let targets = request(&mut agent, 7, "Target.getTargets", json!({})).await;
        assert_eq!(targets["result"]["targetInfos"][0]["url"], initial_url);
        request(&mut observer, 8, "Glass.takeControl", json!({})).await;
        let disallowed = request(&mut observer, 9, "Glass.humanCommand", json!({"method":"Runtime.evaluate", "params":{"expression":"1"}})).await;
        assert_eq!(disallowed["error"]["message"], "METHOD_NOT_ALLOWED");
        let allowed = request(&mut observer, 10, "Glass.humanCommand", command(&allowed_url)).await;
        assert!(allowed.get("error").is_none(), "{allowed}");
        let targets = request(&mut agent, 11, "Target.getTargets", json!({})).await;
        assert_eq!(targets["result"]["targetInfos"][0]["url"], allowed_url);
        let key = request(&mut observer, 110, "Glass.humanCommand", json!({"method":"Input.dispatchKeyEvent", "params":{"type":"keyDown", "key":"x", "text":"x"}})).await;
        assert!(key.get("error").is_none(), "{key}");
        let history = request(&mut observer, 111, "Glass.humanCommand", json!({"method":"Page.getNavigationHistory", "params":{}})).await;
        assert!(history["result"]["entries"].is_array(), "{history}");
        let refused = request(&mut observer, 12, "Glass.humanCommand", command("http://127.0.0.2/blocked")).await;
        let refusal = refused["error"]["message"].as_str().unwrap();
        assert!(refusal.starts_with("TERMINUS REFUSE") && refusal.contains("REFUSE_HOST"), "{refused}");
        let targets = request(&mut agent, 13, "Target.getTargets", json!({})).await;
        assert_eq!(targets["result"]["targetInfos"][0]["url"], allowed_url);
        let receipts = std::fs::read_to_string(state.path().join("browser.actions.jsonl")).unwrap();
        let lines: Vec<Value> = receipts.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert!(lines.iter().any(|line| line["driver"] == "human" && line["lane"] == "browse:human" && line["verdict"] == "ALLOW"), "{receipts}");
        assert!(lines.iter().any(|line| line["driver"] == "human" && line["lane"] == "browse:human" && line["verdict"] == "REFUSE"), "{receipts}");
        assert!(lines.iter().any(|line| line["driver"] == "human" && line["action"] == "type" && line["url"] == allowed_url), "{receipts}");
        request(&mut observer, 14, "Glass.release", json!({})).await;
        let after = request(&mut observer, 15, "Glass.humanCommand", command(&allowed_url)).await;
        assert_eq!(after["error"]["message"], "NOT_HUMAN_MODE");
        assert!(session_request(&mut agent, 16, "Page.navigate", &sid, json!({"url":initial_url})).await.get("error").is_none());
    });
}

#[test]
fn handoff_cancels_inflight_navigation_and_records_receipt() {
    run(async {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let state = tempfile::tempdir().unwrap();
        std::env::set_var("OMNIS_GLASS_STATE_DIR", state.path());
        std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
        std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (arrived_tx, arrived_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_local(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024]; stream.read(&mut buf).await.unwrap();
            let _ = arrived_tx.send(());
            let _ = finish_rx.await;
            let body = "<html><body>slow response</body></html>";
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            let _ = stream.write_all(response.as_bytes()).await;
        });
        let (mut agent, mut observer) = setup(Some("observer-secret")).await;
        let target = request(&mut agent, 1, "Target.createTarget", json!({"url":"about:blank"})).await["result"]["targetId"].as_str().unwrap().to_string();
        request(&mut observer, 2, "Glass.authorizeObserver", json!({"token":"observer-secret"})).await;
        agent.send(Message::Text(json!({"id":3,"method":"Page.navigate","sessionId":format!("{target}-session"),"params":{"url":format!("http://{address}/slow")}}).to_string().into())).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), arrived_rx).await.expect("request reaches fixture").unwrap();
        let control = request(&mut observer, 4, "Glass.takeControl", json!({})).await;
        assert_eq!(control["result"]["cancelledAction"], "navigate");
        finish_tx.send(()).unwrap();
        let response = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let value: Value = serde_json::from_str(agent.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                if value["id"] == 3 { break value; }
            }
        }).await.expect("navigation response");
        assert_eq!(response["error"]["message"], "cancelled_by_handoff", "{response}");
        let receipts = std::fs::read_to_string(state.path().join("browser.actions.jsonl")).unwrap();
        assert!(receipts.lines().map(|line| serde_json::from_str::<Value>(line).unwrap()).any(|value| value["outcome"] == "cancelled_by_handoff" && value["driver"] == "agent"), "{receipts}");
    });
}

#[test]
fn handoff_cancels_inflight_evaluation() {
    run(async {
        let state = tempfile::tempdir().unwrap();
        std::env::set_var("OMNIS_GLASS_STATE_DIR", state.path());
        std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
        std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
        let url = fixture_url().await;
        let (mut agent, mut observer) = setup(Some("observer-secret")).await;
        let target = request(&mut agent, 1, "Target.createTarget", json!({"url":"about:blank"})).await["result"]["targetId"].as_str().unwrap().to_string();
        let sid = format!("{target}-session");
        assert!(session_request(&mut agent, 2, "Page.navigate", &sid, json!({"url":url})).await.get("error").is_none());
        request(&mut observer, 3, "Glass.authorizeObserver", json!({"token":"observer-secret"})).await;
        agent.send(Message::Text(json!({"id":4,"method":"Runtime.evaluate","sessionId":sid,"params":{"expression":"new Promise(resolve => setTimeout(() => resolve(42), 1000))", "awaitPromise":true, "timeout":4000}}).to_string().into())).await.unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;
        let control = request(&mut observer, 5, "Glass.takeControl", json!({})).await;
        assert_eq!(control["result"]["cancelledAction"], "eval", "{control}");
        let response = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let v: Value = serde_json::from_str(agent.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                if v["id"] == 4 { break v; }
            }
        }).await.expect("evaluation response");
        assert_eq!(response["error"]["message"], "cancelled_by_handoff", "{response}");
        let receipts = std::fs::read_to_string(state.path().join("browser.actions.jsonl")).unwrap();
        assert!(receipts.lines().map(|line| serde_json::from_str::<Value>(line).unwrap()).any(|v| v["action"] == "eval" && v["outcome"] == "cancelled_by_handoff"), "{receipts}");
    });
}

#[test]
fn registry_removes_target_when_its_connection_closes() {
    run(async {
        let (mut agent, mut observer) = setup(Some("observer-secret")).await;
        let target = request(&mut agent, 1, "Target.createTarget", json!({"url":"about:blank"})).await["result"]["targetId"].as_str().unwrap().to_string();
        request(&mut observer, 2, "Glass.authorizeObserver", json!({"token":"observer-secret"})).await;
        let owner = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let result = request(&mut observer, 3, "Glass.listTargets", json!({})).await;
                if let Some(target) = result["result"]["targets"].as_array().unwrap().iter().find(|entry| entry["targetId"] == target) { break target["connectionId"].as_str().unwrap().to_string(); }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("target appears in registry");
        agent.close(None).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let result = request(&mut observer, 4, "Glass.listTargets", json!({})).await;
                if !result["result"]["targets"].as_array().unwrap().iter().any(|entry| entry["connectionId"] == owner) { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("disconnected target leaves registry");
    });
}

#[cfg(feature = "render")]
#[test]
fn observer_can_observe_its_own_fallback_target() {
    run(async {
        std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
        std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
        let url = fixture_url().await;
        let (_agent, mut observer) = setup(Some("observer-secret")).await;
        request(&mut observer, 1, "Glass.authorizeObserver", json!({"token":"observer-secret"})).await;
        let target = request(&mut observer, 2, "Target.createTarget", json!({"url":url})).await["result"]["targetId"].as_str().unwrap().to_string();
        let owner = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let result = request(&mut observer, 3, "Glass.listTargets", json!({})).await;
                if let Some(target) = result["result"]["targets"].as_array().unwrap().iter().find(|entry| entry["targetId"] == target) { break target["connectionId"].as_str().unwrap().to_string(); }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("fallback target appears");
        let result = request(&mut observer, 4, "Glass.observe", json!({"connectionId":owner,"targetId":target})).await;
        let id = result["result"]["observationId"].as_str().expect("observation response").to_string();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let v: Value = serde_json::from_str(observer.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                if v["method"] == "Glass.frame" { assert_eq!(v["params"]["observationId"], id); break; }
            }
        }).await.expect("own target frame");
    });
}

#[test]
fn create_target_disallowed_host_has_no_page_side_effect() {
    run(async {
        let state = tempfile::tempdir().unwrap();
        std::env::set_var("OMNIS_GLASS_STATE_DIR", state.path());
        std::env::set_var("OMNIS_GLASS_ENFORCE", "enforce");
        std::env::set_var("OMNIS_GLASS_LANE", "browse:local");
        std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
        let (mut agent, _observer) = setup(Some("observer-secret")).await;
        let result = request(&mut agent, 1, "Target.createTarget", json!({"url":"http://127.0.0.2/refused"})).await;
        assert!(result["error"]["message"].as_str().map(|message| message.contains("REFUSE_HOST")).unwrap_or(false), "{result}");
        let list = request(&mut agent, 2, "Target.getTargets", json!({})).await;
        assert!(list["result"]["targetInfos"].as_array().unwrap().is_empty(), "{list}");
        let receipts = std::fs::read_to_string(state.path().join("browser.actions.jsonl")).unwrap();
        assert!(receipts.lines().map(|line| serde_json::from_str::<Value>(line).unwrap()).any(|v| v["action"] == "navigate" && v["verdict"] == "REFUSE"), "{receipts}");
    });
}

#[test]
fn colliding_cdp_session_ids_keep_connection_urls_separate() {
    run(async {
        let state = tempfile::tempdir().unwrap();
        std::env::set_var("OMNIS_GLASS_STATE_DIR", state.path());
        std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
        std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
        let base = fixture_url().await;
        let (mut first, mut observer) = setup(Some("observer-secret")).await;
        let peer = match observer.get_ref() { MaybeTlsStream::Plain(stream) => stream.peer_addr().unwrap(), _ => unreachable!() };
        let (mut second, _) = connect_async(format!("ws://{peer}/devtools/browser/observer-test-cdp-token")).await.unwrap();
        let one = request(&mut first, 1, "Target.createTarget", json!({"url":"about:blank"})).await["result"]["targetId"].as_str().unwrap().to_string();
        let two = request(&mut second, 1, "Target.createTarget", json!({"url":"about:blank"})).await["result"]["targetId"].as_str().unwrap().to_string();
        assert_eq!(one, two, "the test requires colliding target IDs");
        let session = format!("{one}-session");
        let first_url = format!("{base}first");
        let second_url = format!("{base}second");
        assert!(session_request(&mut first, 2, "Page.navigate", &session, json!({"url":first_url})).await.get("error").is_none());
        assert!(session_request(&mut second, 2, "Page.navigate", &session, json!({"url":second_url})).await.get("error").is_none());
        request(&mut observer, 3, "Glass.authorizeObserver", json!({"token":"observer-secret"})).await;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let list = request(&mut observer, 4, "Glass.listTargets", json!({})).await;
                let entries = list["result"]["targets"].as_array().unwrap();
                if entries.iter().any(|v| v["url"] == first_url) && entries.iter().any(|v| v["url"] == second_url) { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("both navigated targets");
        for (agent, id) in [(&mut first, 5), (&mut second, 6)] {
            let result = session_request(agent, id, "Runtime.evaluate", &session, json!({"expression":"42", "returnByValue":true})).await;
            assert!(result.get("error").is_none(), "{result}");
        }
        let attached = request(&mut first, 7, "Target.attachToTarget", json!({"targetId":one,"flatten":true})).await;
        let attached_session = attached["result"]["sessionId"].as_str().unwrap().to_string();
        assert_ne!(attached_session, session);
        let result = session_request(&mut first, 8, "Runtime.evaluate", &attached_session, json!({"expression":"43", "returnByValue":true})).await;
        assert!(result.get("error").is_none(), "{result}");
        let receipts = std::fs::read_to_string(state.path().join("browser.actions.jsonl")).unwrap();
        let entries: Vec<Value> = receipts.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert!(entries.iter().any(|entry| entry["action"] == "eval" && entry["url"] == first_url), "{receipts}");
        assert!(entries.iter().any(|entry| entry["action"] == "eval" && entry["url"] == second_url), "{receipts}");
        assert_eq!(entries.iter().filter(|entry| entry["action"] == "eval" && entry["url"] == first_url).count(), 2, "{receipts}");
    });
}

#[cfg(feature = "render")]
#[test]
fn key_event_receipts_do_not_hash_text() {
    use sha2::{Digest, Sha256};
    run(async {
        let state = tempfile::tempdir().unwrap();
        std::env::set_var("OMNIS_GLASS_STATE_DIR", state.path());
        std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
        std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
        let url = fixture_url().await;
        let (mut agent, mut observer) = setup(Some("observer-secret")).await;
        let target = request(&mut agent, 1, "Target.createTarget", json!({"url":"about:blank"})).await["result"]["targetId"].as_str().unwrap().to_string();
        let sid = format!("{target}-session");
        assert!(session_request(&mut agent, 2, "Page.navigate", &sid, json!({"url":url})).await.get("error").is_none());
        assert!(session_request(&mut agent, 3, "Runtime.evaluate", &sid, json!({"expression":"(()=>{document.body.innerHTML='<input type=password id=p>'; document.getElementById('p').focus()})()"})).await.get("error").is_none());
        request(&mut observer, 4, "Glass.authorizeObserver", json!({"token":"observer-secret"})).await;
        let entry = loop {
            let list = request(&mut observer, 5, "Glass.listTargets", json!({})).await;
            if let Some(entry) = list["result"]["targets"].as_array().unwrap().iter().find(|v| v["targetId"] == target) { break entry.clone(); }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        assert!(request(&mut observer, 6, "Glass.observe", json!({"connectionId":entry["connectionId"],"targetId":target})).await.get("error").is_none());
        assert!(request(&mut observer, 7, "Glass.takeControl", json!({})).await.get("error").is_none());
        for (i, c) in "aB3!zQ7?".chars().enumerate() {
            let s = c.to_string();
            let response = request(&mut observer, 10+i as u64, "Glass.humanCommand", json!({"method":"Input.dispatchKeyEvent","params":{"type":"keyDown","key":s,"text":s}})).await;
            assert!(response.get("error").is_none(), "{response}");
        }
        if let Ok(path) = std::env::var("GLASS_TEST_LEDGER_COPY") {
            std::fs::copy(state.path().join("browser.actions.jsonl"), path).unwrap();
        }
        assert!(request(&mut observer, 30, "Glass.release", json!({})).await.get("error").is_none());
        let value = session_request(&mut agent, 31, "Runtime.evaluate", &sid, json!({"expression":"document.getElementById('p').value", "returnByValue":true})).await;
        assert_eq!(value["result"]["result"]["value"], "aB3!zQ7?", "{value}");
        for (i, c) in "aB3!zQ7?".chars().enumerate() {
            let s = c.to_string();
            let response = session_request(&mut agent, 40+i as u64, "Input.dispatchKeyEvent", &sid, json!({"type":"keyDown","key":s,"text":s})).await;
            assert!(response.get("error").is_none(), "{response}");
        }
        let path = state.path().join("browser.actions.jsonl");
        let data = std::fs::read_to_string(&path).unwrap();
        let entries: Vec<Value> = data.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert!(entries.iter().all(|v| v.get("value_sha256").is_none()));
        for driver in ["human", "agent"] {
            let typing: Vec<_> = entries.iter().filter(|v| v["action"] == "type" && v["driver"] == driver).collect();
            assert_eq!(typing.len(), 8);
            assert!(typing.iter().all(|v| v.get("value_sha256").is_none() && v["chars"] == 1));
        }
        let verified = glass_ledger::verify_chain(&path, None);
        assert!(verified.ok, "{verified:?}");
        let changed = data.replacen("\"chars\":1", "\"chars\":2", 1);
        assert_ne!(changed, data);
        std::fs::write(&path, changed).unwrap();
        assert!(!glass_ledger::verify_chain(&path, None).ok);
        std::fs::write(&path, &data).unwrap();
        let mut rail = glass_pane_bridge::ReceiptTailer::new(state.path());
        assert!(rail.poll_actions().unwrap().iter().filter(|v| v.action == "type").count() >= 16);
        for c in "aB3!zQ7?".chars() {
            let hash = Sha256::digest(c.to_string().as_bytes());
            for path in state_files(state.path()) {
                {
                    let bytes = std::fs::read(path).unwrap();
                    assert!(!bytes.windows(hash.len()).any(|w| w == hash.as_slice()));
                    let hex = hash.iter().map(|b| format!("{b:02x}")).collect::<String>();
                    assert!(!bytes.windows(hex.len()).any(|w| w == hex.as_bytes()));
                }
            }
        }
        let inserted = session_request(&mut agent, 60, "Input.insertText", &sid, json!({"text":"whole value"})).await;
        assert_eq!(inserted["error"]["message"], "Unknown Input method: insertText");
        let updated = std::fs::read_to_string(&path).unwrap();
        let inserted: Value = serde_json::from_str(updated.lines().last().unwrap()).unwrap();
        assert_eq!(inserted["value_sha256"], glass_ledger::digest_sha256("whole value"));
        let mut expected: Vec<_> = entries.iter().find(|v| v["action"] == "type").unwrap().as_object().unwrap().keys().cloned().collect();
        expected.retain(|key| key != "chars");
        expected.push("value_sha256".into());
        expected.sort();
        let mut actual: Vec<_> = inserted.as_object().unwrap().keys().cloned().collect();
        actual.sort();
        assert_eq!(actual, expected);
    });
}

#[cfg(feature = "render")]
#[test]
fn human_typing_then_reload_does_not_store_cleartext() {
    run(async {
        let state = tempfile::tempdir().unwrap();
        std::env::set_var("OMNIS_GLASS_STATE_DIR", state.path());
        std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
        std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
        let url = fixture_url().await;
        let (mut agent, mut observer) = setup(Some("observer-secret")).await;
        let target = request(&mut agent, 1, "Target.createTarget", json!({"url":"about:blank"})).await["result"]["targetId"].as_str().unwrap().to_string();
        let sid = format!("{target}-session");
        assert!(session_request(&mut agent, 2, "Page.navigate", &sid, json!({"url":url})).await.get("error").is_none());
        let setup_page = session_request(&mut agent, 3, "Runtime.evaluate", &sid, json!({"expression":"(()=>{document.body.innerHTML='<input id=t style=\"position:absolute;left:10px;top:10px\"><input id=p type=password style=\"position:absolute;left:10px;top:60px\"><input id=c type=checkbox style=\"position:absolute;left:10px;top:110px\">'; document.getElementById('t').focus()})()"})).await;
        assert!(setup_page.get("error").is_none(), "{setup_page}");
        request(&mut observer, 4, "Glass.authorizeObserver", json!({"token":"observer-secret"})).await;
        let entry = loop {
            let list = request(&mut observer, 5, "Glass.listTargets", json!({})).await;
            if let Some(entry) = list["result"]["targets"].as_array().unwrap().iter().find(|v| v["targetId"] == target) { break entry.clone(); }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        assert!(request(&mut observer, 6, "Glass.observe", json!({"connectionId":entry["connectionId"],"targetId":target})).await.get("error").is_none());
        assert!(request(&mut observer, 7, "Glass.takeControl", json!({})).await.get("error").is_none());
        let ledger_path = state.path().join("browser.actions.jsonl");
        let before = std::fs::read_to_string(&ledger_path).unwrap().lines().count();
        for (id, key) in [(50, "Enter"), (51, "Backspace"), (52, "Tab")] {
            let result = request(&mut observer, id, "Glass.humanCommand", json!({"method":"Input.dispatchKeyEvent","params":{"type":"keyDown","key":key,"code":key}})).await;
            assert!(result.get("error").is_none(), "{result}");
        }
        for (id, kind) in [(53, "mousePressed"), (54, "mouseReleased")] {
            let result = request(&mut observer, id, "Glass.humanCommand", json!({"method":"Input.dispatchMouseEvent","params":{"type":kind,"x":15,"y":115,"button":"left","clickCount":1}})).await;
            assert!(result.get("error").is_none(), "{result}");
        }
        let after = std::fs::read_to_string(&ledger_path).unwrap().lines().count();
        println!("Receipts for Enter, Backspace, Tab and checkbox click: {}", after - before);
        for (id, kind) in [(55, "mousePressed"), (56, "mouseReleased")] {
            let result = request(&mut observer, id, "Glass.humanCommand", json!({"method":"Input.dispatchMouseEvent","params":{"type":kind,"x":20,"y":20,"button":"left","clickCount":1}})).await;
            assert!(result.get("error").is_none(), "{result}");
        }
        let other_files_before: std::collections::BTreeMap<_, _> = state_files(state.path()).into_iter().filter(|p| !p.ends_with("browser.actions.jsonl")).map(|p| (p.clone(), std::fs::read(p).unwrap())).collect();
        for (i, c) in "K2test89".chars().enumerate() {
            let s = c.to_string();
            assert!(request(&mut observer, 10+i as u64, "Glass.humanCommand", json!({"method":"Input.dispatchKeyEvent","params":{"type":"keyDown","key":s,"text":s}})).await.get("error").is_none());
        }
        assert!(request(&mut observer, 20, "Glass.humanCommand", json!({"method":"Input.dispatchKeyEvent","params":{"type":"keyDown","key":"Tab","code":"Tab"}})).await.get("error").is_none());
        for (id, kind) in [(21, "mousePressed"), (22, "mouseReleased")] {
            let click = request(&mut observer, id, "Glass.humanCommand", json!({"method":"Input.dispatchMouseEvent","params":{"type":kind,"x":20,"y":70,"button":"left","clickCount":1}})).await;
            assert!(click.get("error").is_none(), "{click}");
        }
        for (i, c) in "K2pass37".chars().enumerate() {
            let s = c.to_string();
            assert!(request(&mut observer, 30+i as u64, "Glass.humanCommand", json!({"method":"Input.dispatchKeyEvent","params":{"type":"keyDown","key":s,"text":s}})).await.get("error").is_none());
        }
        let other_files_after: std::collections::BTreeMap<_, _> = state_files(state.path()).into_iter().filter(|p| !p.ends_with("browser.actions.jsonl")).map(|p| (p.clone(), std::fs::read(p).unwrap())).collect();
        println!("Other state files changed during typing: {}", other_files_before != other_files_after);
        assert!(request(&mut observer, 40, "Glass.release", json!({})).await.get("error").is_none());
        let values = session_request(&mut agent, 41, "Runtime.evaluate", &sid, json!({"expression":"[document.getElementById('t').value, document.getElementById('p').value, document.getElementById('c').checked].join('|')", "returnByValue":true})).await;
        assert_eq!(values["result"]["result"]["value"], "K2test89|K2pass37|true", "{values}");
        assert!(request(&mut observer, 42, "Glass.takeControl", json!({})).await.get("error").is_none());
        let captures = state.path().join("web.captures.jsonl");
        let captures_before = std::fs::read_to_string(&captures).unwrap_or_default().lines().count();
        let reload = request(&mut observer, 43, "Glass.humanCommand", json!({"method":"Page.reload","params":{}})).await;
        assert!(reload.get("error").is_none(), "{reload}");
        assert!(std::fs::read_to_string(&captures).unwrap().lines().count() > captures_before);
        for path in state_files(state.path()) {
            {
                let bytes = std::fs::read(&path).unwrap();
                for sentinel in [b"K2test89".as_slice(), b"K2pass37".as_slice()] {
                    assert!(!bytes.windows(sentinel.len()).any(|w| w == sentinel), "cleartext in {}", path.display());
                }
            }
        }
    });
}
