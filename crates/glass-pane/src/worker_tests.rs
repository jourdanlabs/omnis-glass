use super::pane_worker;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::{net::TcpListener, sync::mpsc, time::{timeout, Duration}};
use tokio_tungstenite::{accept_async, tungstenite::Message};

fn page(id: &str, stopped: bool) -> Value {
    json!({"connectionId":"agent", "targetId":id, "url":format!("https://{id}.test"), "title":"", "lastActivityMs":10, "scriptsStopped":stopped})
}

struct Harness {
    commands: mpsc::UnboundedSender<Value>,
    events: mpsc::UnboundedSender<(Value, Value)>,
    ui: mpsc::UnboundedReceiver<(&'static str, Value)>,
    worker: tokio::task::JoinHandle<()>,
    server: tokio::task::JoinHandle<()>,
    _state: tempfile::TempDir,
}

impl Harness {
    async fn start(stopped: bool) -> Self {
        let state = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (events, mut event_rx) = mpsc::unbounded_channel::<(Value, Value)>();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            let mut targets = json!([page("first", stopped)]);
            loop {
                tokio::select! {
                    incoming = ws.next() => {
                        let Some(Ok(Message::Text(text))) = incoming else { break };
                        let req: Value = serde_json::from_str(&text).unwrap();
                        let result = match req["method"].as_str().unwrap() {
                            "Glass.authorizeObserver" => json!({"ok":true}),
                            "Glass.listTargets" => json!({"targets":targets}),
                            "Glass.observe" => json!({"observationId":"obs"}),
                            "Glass.getMode" => json!({"mode":"mirror","agentWaiting":false}),
                            "Glass.frameAck" => json!({}),
                            method => panic!("unexpected {method}"),
                        };
                        ws.send(Message::Text(json!({"id":req["id"],"result":result}).to_string().into())).await.unwrap();
                    }
                    event = event_rx.recv() => {
                        let Some((new_targets, event)) = event else { break };
                        targets = new_targets;
                        ws.send(Message::Text(event.to_string().into())).await.unwrap();
                    }
                }
            }
        });
        let (commands, command_rx) = mpsc::unbounded_channel();
        let (ui_tx, ui) = mpsc::unbounded_channel();
        let state_path = state.path().to_owned();
        let worker = tokio::spawn(async move {
            pane_worker(format!("ws://{addr}"), "secret".into(), None, false, false, command_rx, state_path,
                move |name, value| { ui_tx.send((name, value)).unwrap(); }).await;
        });
        commands.send(json!({"type":"ready"})).unwrap();
        Self { commands, events, ui, worker, server, _state: state }
    }

    async fn expect(&mut self, name: &'static str, value: Value) {
        let actual = timeout(Duration::from_secs(5), self.ui.recv()).await.expect("timed out waiting for UI call").expect("worker exited");
        assert_eq!(actual, (name, value));
    }

    async fn initial(&mut self, stopped: bool) {
        self.expect("setMirrorOnly", json!(false)).await;
        self.expect("setUrl", json!("https://first.test")).await;
        self.expect("setScriptsStopped", json!(stopped)).await;
        self.expect("setStatus", json!("observing agent page")).await;
        self.expect("setMode", json!("MIRROR")).await;
    }

    fn event(&self, targets: Value, method: &str, params: Value) {
        self.events.send((targets, json!({"method":method,"params":params}))).unwrap();
    }

    async fn finish(self) {
        drop(self.commands);
        timeout(Duration::from_secs(5), self.worker).await.unwrap().unwrap();
        self.server.abort();
    }
}

#[tokio::test]
async fn initial_notice_follows_url_for_both_values() {
    for stopped in [true, false] {
        let mut h = Harness::start(stopped).await;
        h.initial(stopped).await;
        h.finish().await;
    }
}

#[tokio::test]
async fn notice_refreshes_only_for_observed_page_and_status_is_independent() {
    let mut h = Harness::start(false).await;
    h.initial(false).await;
    h.commands.send(json!({"type":"navigate", "url":"https://example.test"})).unwrap();
    h.expect("setStatus", json!("take control first")).await;
    let targets = json!([page("first", true)]);
    h.event(targets.clone(), "Glass.scriptsStoppedChanged", json!({"connectionId":"agent","targetId":"other"}));
    // A frame is an ordered barrier: an unwanted notice must precede it.
    let frame = json!({"observationId":"obs","frameId":1});
    h.event(targets.clone(), "Glass.frame", frame.clone());
    h.expect("drawFrame", frame).await;
    h.event(targets, "Glass.scriptsStoppedChanged", json!({"connectionId":"agent","targetId":"first"}));
    h.expect("setScriptsStopped", json!(true)).await;
    h.event(json!([page("second", false)]), "Glass.targetsChanged", json!({}));
    h.expect("setUrl", json!("https://second.test")).await;
    h.expect("setScriptsStopped", json!(false)).await;
    h.expect("setStatus", json!("observing agent page")).await;
    let frame = json!({"observationId":"obs","frameId":2});
    h.event(json!([page("second", false)]), "Glass.frame", frame.clone());
    h.expect("drawFrame", frame).await;
    h.finish().await;
}

#[tokio::test]
async fn notice_follows_switched_target() {
    let mut h = Harness::start(true).await;
    h.initial(true).await;
    h.event(json!([page("second", false)]), "Glass.targetsChanged", json!({}));
    h.expect("setUrl", json!("https://second.test")).await;
    h.expect("setScriptsStopped", json!(false)).await;
    h.expect("setStatus", json!("observing agent page")).await;
    h.finish().await;
}
