//! B10/B15 — v1.1 release checks.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn release_dir() -> PathBuf {
    std::env::var("GLASS_RELEASE_TARGET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root().join("target/release"))
}

#[path = "../../../tests/release_pin.rs"]
mod release_pin;

#[test]
fn b10_pane_bypass_check_passes() {
    let script = repo_root().join("scripts/check-pane-bypass.mjs");
    let output = Command::new("node")
        .arg(&script)
        .output()
        .expect("run check-pane-bypass");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn b15_pane_help_and_headless() {
    let pane = release_dir().join("glass-pane");
    assert!(
        pane.exists(),
        "release glass-pane missing at {} — cargo build --release -p glass-pane --features render",
        pane.display()
    );
    release_pin::check(&repo_root(), &release_dir(), "glass-pane", "glass_pane");
    let help = Command::new(&pane)
        .arg("--help")
        .output()
        .expect("pane --help");
    assert!(help.status.success());
    let state = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port().to_string();
    drop(listener);
    let headless = Command::new(&pane)
        .env("OMNIS_GLASS_STATE_DIR", state.path())
        .args(["--headless", "--co-launch", "--port", &port])
        .output()
        .expect("pane headless");
    assert!(headless.status.success(), "stdout: {}\nstderr: {}", String::from_utf8_lossy(&headless.stdout), String::from_utf8_lossy(&headless.stderr));
    let report: serde_json::Value = serde_json::from_slice(&headless.stdout).unwrap();
    assert_eq!(report["connected"], true);
    assert_eq!(report["authorized"], true);
    assert!(report["frames"].as_u64().unwrap() >= 1);
}

#[cfg(feature = "render")]
#[tokio::test(flavor = "current_thread")]
async fn release_headless_frames_after_navigation() {
    use futures_util::{SinkExt, StreamExt};
    use serde_json::{json, Value};
    use std::time::Duration;
    use tokio_tungstenite::{connect_async, tungstenite::Message};
    struct Server(std::process::Child);
    impl Drop for Server { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }
    let state = tempfile::tempdir().unwrap();
    let fixture = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = fixture.local_addr().unwrap();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        for stream in fixture.incoming() {
            let Ok(mut stream) = stream else { break; };
            let mut request = [0; 2048]; let _ = stream.read(&mut request);
            let body = "<style>@keyframes a{from{opacity:1}to{opacity:.2}}body{animation:a .2s infinite alternate}</style><body>animated</body>";
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
        }
    });
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port(); drop(socket);
    let _server = Server(Command::new(release_dir().join("omnis-glass"))
        .env("OMNIS_GLASS_STATE_DIR", state.path()).env("OMNIS_GLASS_ENFORCE", "local_only")
        .env("OMNIS_GLASS_CDP_TOKEN", "cdp-release-test").env("OMNIS_GLASS_PANE_OBSERVER_TOKEN", "observer-release-test")
        .args(["serve", "--port", &port.to_string(), "--pane-observer", "--allow-private-network"])
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap());
    let ws = format!("ws://127.0.0.1:{port}/devtools/browser/cdp-release-test");
    let (mut agent, _) = tokio::time::timeout(Duration::from_secs(10), async {
        loop { if let Ok(pair) = connect_async(&ws).await { break pair; } tokio::time::sleep(Duration::from_millis(25)).await; }
    }).await.expect("release server starts");
    agent.send(Message::Text(json!({"id":1,"method":"Target.createTarget","params":{"url":format!("http://{address}/")}}).to_string().into())).await.unwrap();
    let created = loop { let v: Value = serde_json::from_str(agent.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if v["id"] == 1 { break v; } };
    let session = format!("{}-session", created["result"]["targetId"].as_str().expect("created target"));
    let mut pane = Command::new(release_dir().join("glass-pane"))
        .env("OMNIS_GLASS_STATE_DIR", state.path())
        .args(["--headless", "--attach", &ws, "--observer-token", "observer-release-test", "--frames", "5", "--after-navigation", "--timeout-secs", "8"])
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
    agent.send(Message::Text(json!({"id":2,"method":"Page.navigate","sessionId":session,"params":{"url":format!("http://{address}/next")}}).to_string().into())).await.unwrap();
    loop { let v: Value = serde_json::from_str(agent.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if v["id"] == 2 { assert!(v.get("error").is_none(), "{v}"); break; } }
    let output = tokio::task::spawn_blocking(move || pane.wait_with_output().unwrap()).await.unwrap();
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(output.status.success(), "{} {report}", String::from_utf8_lossy(&output.stderr));
    assert!(report["frames"].as_u64().unwrap() >= 5, "{report}");
}
