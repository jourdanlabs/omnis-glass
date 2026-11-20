use futures_util::{SinkExt, StreamExt};
use glass_pane_bridge::observer::ObserverClient;
use serde_json::{json, Value};
use tokio::{net::TcpListener, time::{timeout, Duration}};
use tokio_tungstenite::{accept_async, tungstenite::Message};

#[tokio::test]
async fn event_during_call_is_delivered_and_refusal_is_preserved() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        let request: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        ws.send(Message::Text(json!({"method":"Glass.frame","params":{"frameId":7}}).to_string().into())).await.unwrap();
        ws.send(Message::Text(json!({"id":request["id"],"error":{"code":-32000,"message":"OBSERVER_TOKEN_INVALID full refusal"}}).to_string().into())).await.unwrap();
    });
    let (client, mut events) = ObserverClient::connect(&format!("ws://{addr}")).await.unwrap();
    let error = client.authorize("wrong").await.unwrap_err();
    assert_eq!(error.code, "OBSERVER_TOKEN_INVALID");
    assert_eq!(error.message, "OBSERVER_TOKEN_INVALID full refusal");
    let event = timeout(Duration::from_secs(1), events.recv()).await.unwrap();
    assert!(event.is_some(), "frame event was dropped");
    let event = event.unwrap();
    assert_eq!(event.method, "Glass.frame");
    assert_eq!(event.params["frameId"], 7);
    server.await.unwrap();
}

#[tokio::test]
async fn creates_and_selects_own_target_when_list_is_empty() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        for (expected, result) in [
            ("Glass.listTargets", json!({"targets":[]})),
            ("Target.createTarget", json!({"targetId":"own"})),
            ("Glass.listTargets", json!({"targets":[{"connectionId":"c","targetId":"own","url":"about:blank","title":"","lastActivityMs":1,"scriptsStopped":false}]})),
            ("Glass.observe", json!({"observationId":"own-observation"})),
        ] {
            let request: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(request["method"], expected);
            if expected == "Glass.observe" { assert_eq!(request["params"]["connectionId"], "c"); assert_eq!(request["params"]["targetId"], "own"); }
            ws.send(Message::Text(json!({"id":request["id"],"result":result}).to_string().into())).await.unwrap();
        }
    });
    let (client, _) = ObserverClient::connect(&format!("ws://{addr}")).await.unwrap();
    let (target, own) = glass_pane_bridge::observer::choose_target(&client, None).await.unwrap();
    assert!(own);
    assert_eq!(target.target_id, "own");
    assert_eq!(client.observe(&target.connection_id, &target.target_id).await.unwrap(), "own-observation");
    server.await.unwrap();
}

#[tokio::test]
async fn ui_commands_follow_engine_mode_and_preserve_refusals() {
    use glass_pane_bridge::observer::{dispatch_ui, dispatch_ui_with_events, Mode, UiEvent};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        let mut requests = Vec::new();
        while let Some(Ok(message)) = ws.next().await {
            let request: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
            let method = request["method"].as_str().unwrap();
            let inner = request["params"]["method"].as_str().unwrap_or("");
            let reply = if inner == "Page.navigate" && request["params"]["params"]["url"] == "https://refused.test" {
                json!({"id":request["id"],"error":{"message":"TERMINUS REFUSE exact reason"}})
            } else {
                let result = if inner == "Page.getNavigationHistory" { json!({"currentIndex":1,"entries":[{"id":1},{"id":2},{"id":3}]}) } else if method == "Glass.takeControl" { json!({"mode":"human","cancelledAction":null}) } else if method == "Glass.release" { json!({"mode":"mirror"}) } else { json!({}) };
                json!({"id":request["id"],"result":result})
            };
            requests.push(request.clone());
            ws.send(Message::Text(reply.to_string().into())).await.unwrap();
            if requests.len() == 11 { break; }
            assert!(method.starts_with("Glass."));
        }
        requests
    });
    let (client, _) = ObserverClient::connect(&format!("ws://{addr}")).await.unwrap();
    let mirror = Mode { mode:"mirror".into(), agent_waiting:true };
    let mut human = Mode { mode:"human".into(), agent_waiting:false };
    assert_eq!(mirror.badge(), "MIRROR");
    assert_eq!(human.badge(), "HUMAN");
    assert_eq!(Mode { agent_waiting:true, ..human.clone() }.badge(), "LOCKED");
    for command in [json!({"type":"navigate","url":"blocked"}), json!({"type":"mouse"}), json!({"type":"key"})] {
        assert_eq!(dispatch_ui(&client, &mirror, false, &command).await.unwrap_err().message, "take control first");
    }
    dispatch_ui(&client, &mirror, true, &json!({"type":"take"})).await.unwrap();
    dispatch_ui(&client, &mirror, false, &json!({"type":"take"})).await.unwrap();
    dispatch_ui(&client, &human, false, &json!({"type":"navigate","url":"https://allowed.test"})).await.unwrap();
    let mut events = Vec::new();
    dispatch_ui_with_events(&client, &mut human, false, &json!({"type":"navigate","url":"https://refused.test"}), |event| events.push(event)).await;
    assert_eq!(events, vec![UiEvent::Status("TERMINUS REFUSE exact reason".into())]);
    dispatch_ui(&client, &human, false, &json!({"type":"back"})).await.unwrap();
    dispatch_ui(&client, &human, false, &json!({"type":"forward"})).await.unwrap();
    dispatch_ui(&client, &human, false, &json!({"type":"reload"})).await.unwrap();
    dispatch_ui(&client, &human, false, &json!({"type":"mouse", "params":{"type":"mouseMoved","x":2,"y":3}})).await.unwrap();
    dispatch_ui(&client, &human, false, &json!({"type":"key", "params":{"type":"keyDown","key":"a"}})).await.unwrap();
    dispatch_ui(&client, &human, true, &json!({"type":"release"})).await.unwrap();
    let requests = server.await.unwrap();
    assert_eq!(requests.iter().map(|r| r["method"].as_str().unwrap()).collect::<Vec<_>>(), vec!["Glass.takeControl", "Glass.humanCommand", "Glass.humanCommand", "Glass.humanCommand", "Glass.humanCommand", "Glass.humanCommand", "Glass.humanCommand", "Glass.humanCommand", "Glass.humanCommand", "Glass.humanCommand", "Glass.release"]);
    assert_eq!(requests[1]["params"], json!({"method":"Page.navigate","params":{"url":"https://allowed.test"}}));
    assert_eq!(requests[3]["params"]["method"], "Page.getNavigationHistory");
    assert_eq!(requests[4]["params"], json!({"method":"Page.navigateToHistoryEntry","params":{"entryId":1}}));
    assert_eq!(requests[5]["params"]["method"], "Page.getNavigationHistory");
    assert_eq!(requests[6]["params"]["params"]["entryId"], 3);
    assert_eq!(requests[7]["params"]["method"], "Page.reload");
    assert_eq!(requests[8]["params"]["method"], "Input.dispatchMouseEvent");
    assert_eq!(requests[9]["params"]["method"], "Input.dispatchKeyEvent");
}

#[tokio::test]
async fn mode_results_and_dropped_commands_update_ui() {
    use glass_pane_bridge::observer::{dispatch_ui_with_events, Mode, UiEvent};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        for mode in ["human", "mirror"] {
            let request: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            ws.send(Message::Text(json!({"id":request["id"],"result":{"mode":mode,"cancelledAction":null}}).to_string().into())).await.unwrap();
        }
    });
    let (client, _) = ObserverClient::connect(&format!("ws://{addr}")).await.unwrap();
    let mut mode = Mode { mode:"mirror".into(), agent_waiting:false };
    let mut events = Vec::new();
    dispatch_ui_with_events(&client, &mut mode, false, &json!({"type":"navigate","url":"x"}), |e| events.push(e)).await;
    assert_eq!(events, vec![UiEvent::Status("take control first".into())]);
    events.clear();
    dispatch_ui_with_events(&client, &mut mode, false, &json!({"type":"take"}), |e| events.push(e)).await;
    assert_eq!(mode.mode, "human"); assert_eq!(events, vec![UiEvent::Mode("HUMAN".into())]);
    events.clear();
    dispatch_ui_with_events(&client, &mut mode, false, &json!({"type":"release"}), |e| events.push(e)).await;
    assert_eq!(mode.mode, "mirror"); assert_eq!(events, vec![UiEvent::Mode("MIRROR".into())]);
    server.await.unwrap();
}

#[tokio::test]
async fn newest_target_wins_unless_pinned() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        for _ in 0..2 {
            let request: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            let targets = json!([
                {"connectionId":"a","targetId":"old","url":"","title":"","lastActivityMs":10,"scriptsStopped":false},
                {"connectionId":"b","targetId":"new","url":"","title":"","lastActivityMs":20,"scriptsStopped":false}
            ]);
            ws.send(Message::Text(json!({"id":request["id"],"result":{"targets":targets}}).to_string().into())).await.unwrap();
        }
    });
    let (client, _) = ObserverClient::connect(&format!("ws://{addr}")).await.unwrap();
    let (newest, own) = glass_pane_bridge::observer::choose_target(&client, None).await.unwrap();
    assert_eq!(newest.target_id, "new");
    assert!(!own);
    let (pinned, _) = glass_pane_bridge::observer::choose_target(&client, Some(("a", "old"))).await.unwrap();
    assert_eq!(pinned.target_id, "old");
    server.await.unwrap();
}

#[tokio::test]
async fn target_change_and_observation_end_relist_and_observe() {
    use glass_pane_bridge::observer::{refresh_target, Target};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        ws.send(Message::Text(json!({"method":"Glass.targetsChanged","params":{}}).to_string().into())).await.unwrap();
        for n in 0..2 {
            let list: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(list["method"], "Glass.listTargets");
            ws.send(Message::Text(json!({"id":list["id"],"result":{"targets":[{"connectionId":"b","targetId":"new","url":"https://new.test","title":"","lastActivityMs":20,"scriptsStopped":false}]}}).to_string().into())).await.unwrap();
            let observe: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(observe["method"], "Glass.observe");
            assert_eq!(observe["params"]["targetId"], "new");
            ws.send(Message::Text(json!({"id":observe["id"],"result":{"observationId":format!("obs{n}")}}).to_string().into())).await.unwrap();
            if n == 0 { ws.send(Message::Text(json!({"method":"Glass.observationEnded","params":{"observationId":"obs0","reason":"target_closed"}}).to_string().into())).await.unwrap(); }
        }
    });
    let (client, mut events) = ObserverClient::connect(&format!("ws://{addr}")).await.unwrap();
    let old: Target = serde_json::from_value(json!({"connectionId":"a","targetId":"old","url":"","title":"","lastActivityMs":1,"scriptsStopped":false})).unwrap();
    let changed = events.recv().await.unwrap();
    let selected = refresh_target(&client, &changed, None, &old, "old-observation").await.unwrap();
    assert!(selected.is_some(), "target change did not select and observe");
    let (selected, _, id) = selected.unwrap();
    assert_eq!(selected.target_id, "new");
    let ended = events.recv().await.unwrap();
    let selected_again = refresh_target(&client, &ended, None, &selected, &id).await.unwrap();
    assert!(selected_again.is_some(), "observation end did not re-observe");
    assert_eq!(selected_again.unwrap().2, "obs1");
    server.await.unwrap();
}

async fn recorded_blocked_commands(commands: &[Value], mirror_only: bool) -> Vec<String> {
    use glass_pane_bridge::observer::{dispatch_ui, Mode};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        let mut methods = Vec::new();
        while let Ok(Some(Ok(Message::Text(text)))) = timeout(Duration::from_millis(250), ws.next()).await {
            let request: Value = serde_json::from_str(&text).unwrap();
            methods.push(request["method"].as_str().unwrap().to_string());
            let result = if request["method"] == "Glass.takeControl" { json!({"mode":"human","cancelledAction":null}) } else { json!({}) };
            ws.send(Message::Text(json!({"id":request["id"],"result":result}).to_string().into())).await.unwrap();
        }
        methods
    });
    let (client, _) = ObserverClient::connect(&format!("ws://{addr}")).await.unwrap();
    let mode = Mode { mode:"mirror".into(), agent_waiting:false };
    for command in commands { let _ = dispatch_ui(&client, &mode, mirror_only, command).await; }
    server.await.unwrap()
}

#[tokio::test]
async fn mirror_ui_requests_never_send_human_commands() {
    let commands = [json!({"type":"navigate","url":"https://example.test"}), json!({"type":"mouse","params":{}}), json!({"type":"key","params":{}})];
    let sent = recorded_blocked_commands(&commands, false).await;
    assert!(sent.is_empty(), "mirror sent {sent:?}");
}

#[tokio::test]
async fn mirror_only_never_sends_take_control() {
    let sent = recorded_blocked_commands(&[json!({"type":"take"})], true).await;
    assert!(sent.is_empty(), "mirror-only sent {sent:?}");
}
