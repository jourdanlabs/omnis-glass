#![cfg(feature = "render")]
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use std::time::Duration;

#[tokio::test(flavor = "current_thread")]
async fn rail_and_canvas_fit_real_layout_at_window_sizes() {
    async fn call(ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, id: u64, method: &str, params: Value, session: Option<&str>) -> Value {
        let mut request = json!({"id":id,"method":method,"params":params});
        if let Some(session) = session { request["sessionId"] = json!(session); }
        ws.send(Message::Text(request.to_string().into())).await.unwrap();
        loop { let value: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if value["id"] == id { assert!(value.get("error").is_none(), "{value}"); return value; } }
    }
    let state = tempfile::tempdir().unwrap();
    std::env::set_var("OMNIS_GLASS_STATE_DIR", state.path());
    std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    std::env::set_var("OMNIS_GLASS_CDP_TOKEN", "layout-test-token");
    std::env::set_var("OMNIS_GLASS_CDP_AUTH", "1");
    let fixture = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/index.html", fixture.local_addr().unwrap());
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        for stream in fixture.incoming() {
            let Ok(mut stream) = stream else { break; }; let mut request = [0; 4096];
            let size = stream.read(&mut request).unwrap_or(0);
            let request = String::from_utf8_lossy(&request[..size]);
            let path = request.split_whitespace().nth(1).unwrap_or("/").trim_start_matches('/');
            let name = if path.is_empty() { "index.html" } else { path };
            let Some(file) = glass_pane_ui::UI.get_file(name) else { let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n"); continue; };
            let mime = if name.ends_with(".css") { "text/css" } else if name.ends_with(".js") { "text/javascript" } else { "text/html" };
            let body = file.contents();
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = stream.write_all(body);
        }
    });
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap(); let port = socket.local_addr().unwrap().port(); drop(socket);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(obscura_cdp::start_with_serve_options_and_limit(port, "127.0.0.1", None, false, None, false, None, true, 8)).unwrap();
    });
    let ws_url = format!("ws://127.0.0.1:{port}/devtools/browser/layout-test-token");
    let (mut agent, _) = tokio::time::timeout(Duration::from_secs(10), async {
        loop { if let Ok(ws) = connect_async(&ws_url).await { break ws; } tokio::time::sleep(Duration::from_millis(25)).await; }
    }).await.unwrap();
    let target = call(&mut agent, 1, "Target.createTarget", json!({"url":"about:blank"}), None).await;
    let session = format!("{}-session", target["result"]["targetId"].as_str().unwrap());
    for (index, (width, height)) in [(1280, 832), (1024, 700)].into_iter().enumerate() {
        let id = 10 + index as u64 * 3;
        call(&mut agent, id, "Emulation.setDeviceMetricsOverride", json!({"width":width,"height":height,"deviceScaleFactor":1,"mobile":false}), Some(&session)).await;
        call(&mut agent, id+1, "Page.navigate", json!({"url":url}), Some(&session)).await;
        let result = call(&mut agent, id+2, "Runtime.evaluate", json!({"expression":"(()=>{const rail=document.querySelector('#rail').getBoundingClientRect(), canvas=document.querySelector('#viewport').getBoundingClientRect();return JSON.stringify({railRight:rail.right,railWidth:rail.width,canvasWidth:canvas.width,docWidth:document.documentElement.scrollWidth,viewport:window.innerWidth})})()","returnByValue":true}), Some(&session)).await;
        let boxes: Value = serde_json::from_str(result["result"]["result"]["value"].as_str().unwrap()).unwrap();
        let number = |key: &str| boxes[key].as_f64().unwrap_or_else(|| panic!("missing {key}: {boxes}"));
        assert_eq!(number("viewport"), width as f64, "{boxes}");
        assert!(number("railWidth") >= 279.0 && number("railRight") <= width as f64 + 0.5, "{boxes}");
        assert!(number("canvasWidth") >= 600.0, "{boxes}");
        assert!(number("docWidth") > 0.0 && number("docWidth") <= width as f64, "{boxes}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn real_engine_headless_attach_and_observer_auth() {
    let state = tempfile::tempdir().unwrap();
    std::env::set_var("OMNIS_GLASS_STATE_DIR", state.path());
    std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
    std::env::set_var("OBSCURA_ALLOW_PRIVATE_NETWORK", "1");
    std::env::set_var("OMNIS_GLASS_CDP_TOKEN", "cdp-test-token");
    std::env::set_var("OMNIS_GLASS_CDP_AUTH", "1");
    let fixture = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let fixture_addr = fixture.local_addr().unwrap();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        for stream in fixture.incoming() {
            let Ok(mut stream) = stream else { break; };
            let mut request = [0; 2048]; let _ = stream.read(&mut request);
            let body = "<style>@keyframes blink{from{opacity:1}to{opacity:.1}} body{animation:blink .2s infinite alternate}</style><body>animated</body>";
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
        }
    });
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port(); drop(listener);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(obscura_cdp::start_with_pane_observer_options(port, "127.0.0.1", None, false, None, false, None, true, 8,
            obscura_cdp::PaneObserverOptions { token: Some("observer-secret".into()), mirror_only: false })).unwrap();
    });
    let ws_url = format!("ws://127.0.0.1:{port}/devtools/browser/cdp-test-token");
    let (mut agent, _) = tokio::time::timeout(Duration::from_secs(10), async {
        loop { if let Ok(ws) = connect_async(&ws_url).await { break ws; } tokio::time::sleep(Duration::from_millis(25)).await; }
    }).await.unwrap();
    agent.send(Message::Text(json!({"id":1,"method":"Target.createTarget","params":{"url":format!("http://{fixture_addr}/")}}).to_string().into())).await.unwrap();
    let created = loop { let v: Value = serde_json::from_str(agent.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if v["id"] == 1 { break v; } };
    assert!(created["result"]["targetId"].is_string(), "{created}");
    for (token, success) in [("observer-secret", true), ("", false)] {
        let url = ws_url.clone();
        let output = tokio::task::spawn_blocking(move || std::process::Command::new(env!("CARGO_BIN_EXE_glass-pane"))
            .args(["--headless", "--attach", &url, "--observer-token", token, "--frames", "5", "--timeout-secs", "8"]).output().unwrap()).await.unwrap();
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output.status.success(), success, "{} {report}", String::from_utf8_lossy(&output.stderr));
        if success { assert!(report["frames"].as_u64().unwrap() >= 5, "{report}"); }
        else { assert!(report["error"].as_str().unwrap().contains("OBSERVER_TOKEN_INVALID"), "{report}"); }
    }
    let mut pane = std::process::Command::new(env!("CARGO_BIN_EXE_glass-pane"))
        .args(["--headless", "--attach", &ws_url, "--observer-token", "observer-secret", "--frames", "5", "--after-navigation", "--timeout-secs", "8"])
        .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    let stderr = pane.stderr.take().unwrap();
    let stderr = tokio::task::spawn_blocking(move || {
        use std::io::BufRead;
        let mut reader = std::io::BufReader::new(stderr);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.contains("waiting for target navigation"), "{line}");
        reader.into_inner()
    }).await.unwrap();
    pane.stderr = Some(stderr);
    let target = created["result"]["targetId"].as_str().unwrap();
    let session = format!("{target}-session");
    agent.send(Message::Text(json!({"id":2,"method":"Page.navigate","sessionId":session,"params":{"url":format!("http://{fixture_addr}/next")}}).to_string().into())).await.unwrap();
    loop { let v: Value = serde_json::from_str(agent.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if v["id"] == 2 { assert!(v.get("error").is_none(), "{v}"); break; } }
    let output = tokio::task::spawn_blocking(move || pane.wait_with_output().unwrap()).await.unwrap();
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(output.status.success(), "{} {report}", String::from_utf8_lossy(&output.stderr));
    assert!(report["frames"].as_u64().unwrap() >= 5, "{report}");
}

#[test]
fn real_engine_headless_co_launch() {
    let state = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port().to_string(); drop(listener);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_glass-pane"))
        .env("OMNIS_GLASS_STATE_DIR", state.path()).env_remove("OMNIS_GLASS_CDP_TOKEN").env_remove("OMNIS_GLASS_PANE_OBSERVER_TOKEN")
        .args(["--headless", "--port", &port]).output().unwrap();
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(output.status.success(), "{} {report}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(report["connected"], true); assert_eq!(report["authorized"], true);
    assert!(report["target"]["targetId"].is_string());
}

#[test]
fn co_launch_refuses_wildcard_bind() {
    let state = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_glass-pane"))
        .env("OMNIS_GLASS_STATE_DIR", state.path())
        .args(["--headless", "--host", "0.0.0.0", "--port", "0", "--timeout-secs", "1"]).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("pane co-launch must bind 127.0.0.1"), "{}", String::from_utf8_lossy(&output.stderr));
}
