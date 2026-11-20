#![cfg(feature = "render")]
use futures_util::{SinkExt, StreamExt};
use glass_pane_bridge::observer::ObserverClient;
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
            let body = r#"<html><body><form id="form"><input id="one" style="position:absolute;left:20px;top:30px;width:240px;height:35px;font:28px sans-serif"><input id="two" style="position:absolute;left:20px;top:90px;width:240px;height:35px;font:28px sans-serif"><input id="disabled" disabled style="position:absolute;left:20px;top:150px;width:240px;height:35px"><textarea id="area" style="position:absolute;left:20px;top:330px;width:240px;height:70px;font:20px sans-serif">seed</textarea></form><div id="empty" style="position:absolute;left:20px;top:210px;width:240px;height:35px;background:#ccc"></div><p id="echo" style="position:absolute;top:280px"></p><script>window.focusLog=[];for(let n of ['one','two'])for(let t of ['blur','focusout','focus','focusin'])document.getElementById(n).addEventListener(t,()=>focusLog.push(n+':'+t));one.oninput=()=>echo.textContent=one.value</script></body></html>"#;
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
        }
    }); url
}
async fn human_click(observer: &ObserverClient, y: u64) {
    for (kind, buttons) in [("mousePressed", 1), ("mouseReleased", 0)] {
        observer.human_command("Input.dispatchMouseEvent", json!({"type":kind,"x":60,"y":y,"button":"left","buttons":buttons,"clickCount":1,"modifiers":0})).await.unwrap();
    }
}
async fn human_key(observer: &ObserverClient, key: &str) {
    let code = if key == "Backspace" { "Backspace".to_string() } else { format!("Key{}", key.to_uppercase()) };
    let mut down = json!({"type":"keyDown","key":key,"code":code,"windowsVirtualKeyCode":key.to_uppercase().as_bytes()[0],"modifiers":0});
    if key != "Backspace" { down["text"] = json!(key); }
    observer.human_command("Input.dispatchKeyEvent", down).await.unwrap();
    observer.human_command("Input.dispatchKeyEvent", json!({"type":"keyUp","key":key,"code":code,"windowsVirtualKeyCode":key.to_uppercase().as_bytes()[0],"modifiers":0})).await.unwrap();
}
async fn get_state(agent: &mut Socket, session: &str) -> Value {
    let result = call(agent, 90, "Runtime.evaluate", json!({"expression":"JSON.stringify({one:one.value,two:two.value,echo:echo.textContent,active:document.activeElement.id||document.activeElement.tagName,log:focusLog})","returnByValue":true}), Some(session)).await;
    serde_json::from_str(result["result"]["result"]["value"].as_str().unwrap()).unwrap()
}
async fn run_case(case: &str) {
    let (state, mut agent, url) = setup().await;
    let target = call(&mut agent, 1, "Target.createTarget", json!({"url":fixture()}), None).await["result"]["targetId"].as_str().unwrap().to_string();
    let session = format!("{target}-session");
    if case == "cancel" { call(&mut agent, 2, "Runtime.evaluate", json!({"expression":"one.addEventListener('mousedown',e=>e.preventDefault())"}), Some(&session)).await; }
    let (observer, mut events) = ObserverClient::connect(&url).await.unwrap(); observer.authorize("observer-secret").await.unwrap();
    let t = observer.list_targets().await.unwrap().into_iter().find(|t| t.target_id == target).unwrap();
    let observation = observer.observe(&t.connection_id, &target).await.unwrap();
    if case != "agent" { observer.take_control().await.unwrap(); }
    if case == "agent" {
        for (id, kind, buttons) in [(10,"mousePressed",1),(11,"mouseReleased",0)] { call(&mut agent,id,"Input.dispatchMouseEvent",json!({"type":kind,"x":60,"y":45,"button":"left","buttons":buttons,"clickCount":1,"modifiers":0}),Some(&session)).await; }
        for (id, key) in [(12,"h"),(14,"i")] {
            let code = format!("Key{}", key.to_uppercase());
            call(&mut agent,id,"Input.dispatchKeyEvent",json!({"type":"keyDown","key":key,"code":code,"windowsVirtualKeyCode":key.to_uppercase().as_bytes()[0],"modifiers":0,"text":key}),Some(&session)).await;
            call(&mut agent,id+1,"Input.dispatchKeyEvent",json!({"type":"keyUp","key":key,"code":code,"windowsVirtualKeyCode":key.to_uppercase().as_bytes()[0],"modifiers":0}),Some(&session)).await;
        }
    } else {
        human_click(&observer, 45).await;
        if case != "cancel" { human_key(&observer,"h").await; human_key(&observer,"i").await; }
        match case {
            "same" => human_click(&observer,45).await,
            "backspace" => human_key(&observer,"Backspace").await,
            "second" => { human_click(&observer,105).await; human_key(&observer,"x").await; },
            "empty" => { human_click(&observer,225).await; human_key(&observer,"x").await; },
            "disabled" => { human_click(&observer,165).await; human_key(&observer,"x").await; },
            _ => (),
        }
        observer.release().await.unwrap();
    }
    let v = get_state(&mut agent,&session).await;
    match case {
        "typing" | "agent" => { assert_eq!(v["one"], "hi", "{v}"); assert_eq!(v["echo"], "hi"); assert_eq!(v["active"], "one"); },
        "same" => { assert_eq!(v["one"], "hi"); assert_eq!(v["log"], json!(["one:focus","one:focusin"])); },
        "backspace" => assert_eq!(v["one"], "h", "{v}"),
        "second" => { assert_eq!(v["one"], "hi"); assert_eq!(v["two"], "x"); assert_eq!(v["active"], "two"); assert_eq!(v["log"], json!(["one:focus","one:focusin","one:blur","one:focusout","two:focus","two:focusin"])); },
        "empty" | "disabled" => { assert_eq!(v["one"], "hi"); assert_eq!(v["two"], ""); assert_eq!(v["active"], "BODY", "{v}"); },
        "cancel" => assert_eq!(v["active"], "BODY", "{v}"), _ => unreachable!(),
    }
    if case == "typing" || case == "agent" {
        let records = std::fs::read_to_string(state.path().join("browser.actions.jsonl")).unwrap();
        let driver = if case == "agent" { "agent" } else { "human" };
        let count = records.lines().map(|line| serde_json::from_str::<Value>(line).unwrap()).filter(|v| v["action"] == "type" && v["driver"] == driver && v["verdict"] == "ALLOW").count();
        assert_eq!(count,2,"{records}");
    }
    while let Ok(event) = events.try_recv() { if event.method == "Glass.frame" { let _ = observer.frame_ack(&observation,&event.params["frameId"]).await; } }
}
#[tokio::test(flavor="current_thread")] async fn typing() { run_case("typing").await; }
#[tokio::test(flavor="current_thread")] async fn same() { run_case("same").await; }
#[tokio::test(flavor="current_thread")] async fn backspace() { run_case("backspace").await; }
#[tokio::test(flavor="current_thread")] async fn second() { run_case("second").await; }
#[tokio::test(flavor="current_thread")] async fn empty() { run_case("empty").await; }
#[tokio::test(flavor="current_thread")] async fn disabled() { run_case("disabled").await; }
#[tokio::test(flavor="current_thread")] async fn cancel() { run_case("cancel").await; }
#[tokio::test(flavor="current_thread")] async fn agent() { run_case("agent").await; }


async fn next_picture(observer: &ObserverClient, events: &mut tokio::sync::mpsc::UnboundedReceiver<glass_pane_bridge::observer::ObserverEvent>, observation: &str) -> Vec<u8> {
    use base64::Engine;
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let e = events.recv().await.unwrap();
            if e.method == "Glass.frame" && e.params["observationId"] == observation {
                observer.frame_ack(observation, &e.params["frameId"]).await.unwrap();
                return base64::engine::general_purpose::STANDARD.decode(e.params["data"].as_str().unwrap()).unwrap();
            }
        }
    }).await.expect("observed frame")
}
#[tokio::test(flavor="current_thread")]
async fn picture() {
    let (_state, mut agent, url) = setup().await;
    let target = call(&mut agent, 1, "Target.createTarget", json!({"url":fixture()}), None).await["result"]["targetId"].as_str().unwrap().to_string();
    let (observer, mut events) = ObserverClient::connect(&url).await.unwrap(); observer.authorize("observer-secret").await.unwrap();
    let t = observer.list_targets().await.unwrap().into_iter().find(|t| t.target_id == target).unwrap();
    let observation = observer.observe(&t.connection_id, &target).await.unwrap();
    observer.take_control().await.unwrap(); human_click(&observer,45).await;
    let before = next_picture(&observer, &mut events, &observation).await;
    std::fs::write("/tmp/lane-i-field-before.jpg", &before).unwrap();
    human_key(&observer,"h").await; human_key(&observer,"i").await;
    while let Ok(e) = events.try_recv() { if e.method == "Glass.frame" && e.params["observationId"] == observation { observer.frame_ack(&observation,&e.params["frameId"]).await.unwrap(); } }
    let after = next_picture(&observer, &mut events, &observation).await;
    std::fs::write("/tmp/lane-i-field-after.jpg", &after).unwrap();
    observer.release().await.unwrap();
    let session = format!("{target}-session"); let state = get_state(&mut agent, &session).await; eprintln!("state after frame: {state}");
    let before = image::load_from_memory(&before).unwrap().to_rgb8(); let after = image::load_from_memory(&after).unwrap().to_rgb8();
    let total = before.pixels().zip(after.pixels()).filter(|(a,b)| a != b).count(); eprintln!("changed pixels entire frame: {total}");
    let mut changed = 0;
    for y in 35..60 { for x in 25..250 { if before.get_pixel(x,y) != after.get_pixel(x,y) { changed += 1; } } }
    assert!(changed > 0, "field interior has no changed pixels after typing: {changed}");
}

async fn setup_field() -> (tempfile::TempDir, Socket, String, String, String) {
    let (state, mut agent, url) = setup().await;
    let target = call(&mut agent, 1, "Target.createTarget", json!({"url":fixture()}), None).await["result"]["targetId"].as_str().unwrap().to_string();
    let session = format!("{target}-session"); (state, agent, url, target, session)
}
async fn eval(agent: &mut Socket, session: &str, code: &str) -> Value {
    call(agent, 30, "Runtime.evaluate", json!({"expression":code,"returnByValue":true}), Some(session)).await["result"]["result"]["value"].clone()
}
async fn crop_image(agent: &mut Socket, session: &str, top: u32, bottom: u32) -> Vec<u8> {
    use base64::Engine;
    let result = call(agent, 31, "Page.captureScreenshot", json!({"format":"png"}), Some(session)).await;
    let bytes = base64::engine::general_purpose::STANDARD.decode(result["result"]["data"].as_str().unwrap()).unwrap();
    let image = image::load_from_memory(&bytes).unwrap().to_rgb8();
    let mut pixels = Vec::new(); for y in top..bottom { for x in 20..260 { pixels.extend_from_slice(&image.get_pixel(x,y).0); } } pixels
}
async fn field_image(agent: &mut Socket, session: &str) -> Vec<u8> { crop_image(agent, session, 30, 65).await }
#[tokio::test(flavor="current_thread")]
async fn live_value_reset_restores_picture() {
    let (_state, mut agent, _url, _target, session) = setup_field().await;
    let before = field_image(&mut agent,&session).await;
    eval(&mut agent,&session,"one.value='hi'").await;
    assert_ne!(before,field_image(&mut agent,&session).await);
    eval(&mut agent,&session,"form.reset()").await;
    assert_eq!(before,field_image(&mut agent,&session).await);
}
#[tokio::test(flavor="current_thread")]
async fn live_value_reload_restores_picture() {
    let (_state, mut agent, _url, _target, session) = setup_field().await;
    let before = field_image(&mut agent,&session).await;
    eval(&mut agent,&session,"one.value='hi'").await;
    assert_ne!(before,field_image(&mut agent,&session).await);
    call(&mut agent, 32, "Page.reload", json!({}), Some(&session)).await;
    assert_eq!(before,field_image(&mut agent,&session).await);
}
#[tokio::test(flavor="current_thread")]
async fn removed_field_has_no_stale_picture() {
    let (_state, mut agent, _url, _target, session) = setup_field().await;
    let before = field_image(&mut agent,&session).await;
    eval(&mut agent,&session,"one.value='hi'").await;
    assert_ne!(before,field_image(&mut agent,&session).await);
    eval(&mut agent,&session,"(()=>{window.oldField=one;let fresh=one.cloneNode(true);one.replaceWith(fresh)})()").await;
    assert_eq!(before,field_image(&mut agent,&session).await);
    eval(&mut agent,&session,"one.replaceWith(oldField)").await;
    assert_eq!(before,field_image(&mut agent,&session).await, "reattached node must have no retained paint value");
}
#[tokio::test(flavor="current_thread")]
async fn live_value_keeps_dom_attributes_and_snapshot() {
    let (_state, mut agent, _url, _target, session) = setup_field().await;
    let before = eval(&mut agent,&session,"JSON.stringify([one.getAttribute('value'),one.outerHTML])").await;
    let snap_before = call(&mut agent, 33,"DOMSnapshot.captureSnapshot",json!({"computedStyles":[]}),Some(&session)).await["result"].clone();
    eval(&mut agent,&session,"one.value='hi'").await;
    let after = eval(&mut agent,&session,"JSON.stringify([one.getAttribute('value'),one.outerHTML])").await;
    let snap_after = call(&mut agent, 34,"DOMSnapshot.captureSnapshot",json!({"computedStyles":[]}),Some(&session)).await["result"].clone();
    assert_eq!(before,after); assert_eq!(snap_before,snap_after);
}
#[tokio::test(flavor="current_thread")]
async fn form_paint_op_ignores_invalid_inputs() {
    let (_state, mut agent, _url, _target, session) = setup_field().await;
    let before = field_image(&mut agent,&session).await;
    let result = eval(&mut agent,&session,"(()=>{try{Deno.core.ops.op_form_paint_value(4294967295,'x',false,0);Deno.core.ops.op_form_paint_value(one._nid,42,false,0);Deno.core.ops.op_form_paint_value(one._nid,undefined,false,0);let cycle={};cycle.self=cycle;Deno.core.ops.op_form_paint_value(one._nid,cycle,false,0);Deno.core.ops.op_form_paint_value(document.body._nid,'x',false,0);return 42}catch(e){return String(e)}})()").await;
    assert_eq!(result.as_f64(),Some(42.0)); assert_eq!(eval(&mut agent,&session,"6*7").await.as_f64(),Some(42.0)); assert_eq!(before,field_image(&mut agent,&session).await);
}
#[tokio::test(flavor="current_thread")]
async fn script_value_activity_wakes_frame_and_same_value_is_quiet() {
    let (_state, mut agent, url, target, session) = setup_field().await;
    let (observer, mut events) = ObserverClient::connect(&url).await.unwrap(); observer.authorize("observer-secret").await.unwrap();
    let t = observer.list_targets().await.unwrap().into_iter().find(|t| t.target_id == target).unwrap();
    let observation = observer.observe(&t.connection_id, &target).await.unwrap();
    let before = next_picture(&observer,&mut events,&observation).await;
    eval(&mut agent,&session,"setTimeout(()=>{one.value='abc'},300)").await;
    while let Ok(event) = events.try_recv() { if event.method == "Glass.frame" { observer.frame_ack(&observation,&event.params["frameId"]).await.unwrap(); } }
    let after = next_picture(&observer,&mut events,&observation).await;
    assert_ne!(before,after);
    eval(&mut agent,&session,"setTimeout(()=>{one.value='abc'},300)").await;
    while let Ok(event) = events.try_recv() { if event.method == "Glass.frame" { observer.frame_ack(&observation,&event.params["frameId"]).await.unwrap(); } }
    let extra = tokio::time::timeout(Duration::from_millis(800), async { loop { let e=events.recv().await.unwrap(); if e.method == "Glass.frame" { break e; } } }).await;
    assert!(extra.is_err(),"setting the same value generated a new frame");
}

#[tokio::test(flavor="current_thread")]
async fn bounded_native_copy_preserves_page_value() {
    let (_state, mut agent, _url, _target, session) = setup_field().await;
    let length = eval(&mut agent,&session,"(()=>{window.longValue='x'.repeat(1000000);one.value=longValue;return one.value.length})()").await;
    assert_eq!(length.as_f64(), Some(1_000_000.0));
    let long_image = field_image(&mut agent,&session).await;
    eval(&mut agent,&session,"one.value='x'.repeat(4096)").await;
    assert_eq!(long_image, field_image(&mut agent,&session).await);
}

#[tokio::test(flavor="current_thread")]
async fn live_textarea_line_break_and_reset() {
    let (_state, mut agent, _url, _target, session) = setup_field().await;
    let before = crop_image(&mut agent,&session,330,400).await;
    eval(&mut agent,&session,"area.value='a\\nb'").await;
    let multiline = crop_image(&mut agent,&session,330,400).await;
    { use base64::Engine; let shot = call(&mut agent, 35, "Page.captureScreenshot", json!({"format":"png"}), Some(&session)).await; std::fs::write("/tmp/lane-i-textarea.png", base64::engine::general_purpose::STANDARD.decode(shot["result"]["data"].as_str().unwrap()).unwrap()).unwrap(); }
    eval(&mut agent,&session,"area.value='a b'").await;
    assert!(multiline != crop_image(&mut agent,&session,330,400).await, "line break must change the live textarea picture");
    eval(&mut agent,&session,"form.reset()").await;
    assert_eq!(eval(&mut agent,&session,"area.value").await,"seed");
    assert!(before == crop_image(&mut agent,&session,330,400).await, "textarea reset must restore the initial picture");
}

#[tokio::test(flavor="current_thread")]
async fn report_other_controls() {
    let (_state, mut agent, _url, _target, session) = setup_field().await;
    for (name, markup, change) in [
        ("checkbox", "<input id='control' type='checkbox' style='position:absolute;left:20px;top:30px;width:30px;height:30px'>", "control.checked=true"),
        ("radio", "<input id='control' type='radio' style='position:absolute;left:20px;top:30px;width:30px;height:30px'>", "control.checked=true"),
        ("select", "<select id='control' style='position:absolute;left:20px;top:30px;width:180px;height:30px;font:20px sans-serif'><option value='a'>alpha</option><option value='b'>beta</option></select>", "control.value='b'"),
    ] {
        eval(&mut agent,&session,&format!("document.body.innerHTML={}",serde_json::to_string(markup).unwrap())).await;
        let before = field_image(&mut agent,&session).await; eval(&mut agent,&session,change).await;
        let after = field_image(&mut agent,&session).await;
        let live = eval(&mut agent,&session,if name == "select" { "control.value" } else { "control.checked" }).await;
        let changed = before.chunks_exact(3).zip(after.chunks_exact(3)).filter(|(a,b)|a!=b).count();
        eprintln!("{name} live state {live}, changed pixels: {changed}");
    }
}

#[tokio::test(flavor="current_thread")]
async fn live_password_masks_and_follows_type_changes() {
    let (_state, mut agent, _url, _target, session) = setup_field().await;
    eval(&mut agent,&session,"(()=>{one.setAttribute('type','password');one.value='abc'})()").await;
    let masked = field_image(&mut agent,&session).await;
    eval(&mut agent,&session,"one.value='xyz'").await;
    assert!(masked == field_image(&mut agent,&session).await, "equal-length passwords must render identically");
    eval(&mut agent,&session,"one.value='abcd'").await;
    assert!(masked != field_image(&mut agent,&session).await, "extra character must add a bullet");
    eval(&mut agent,&session,"(()=>{one.setAttribute('type','text');one.value='abc'})()").await;
    assert!(masked != field_image(&mut agent,&session).await, "changing to text must show text");
}

#[tokio::test(flavor="current_thread")]
async fn hidden_input_with_tabindex_cannot_receive_focus() {
    let (_state, mut agent, url, target, session) = setup_field().await;
    let hit = eval(&mut agent,&session,"(()=>{one.setAttribute('type','hidden');one.setAttribute('tabindex','0');document.elementFromPoint=()=>one;return document.elementFromPoint(60,45).id})()").await;
    assert_eq!(hit, "one");
    let (observer, _events) = ObserverClient::connect(&url).await.unwrap(); observer.authorize("observer-secret").await.unwrap();
    let t = observer.list_targets().await.unwrap().into_iter().find(|t| t.target_id == target).unwrap();
    observer.observe(&t.connection_id,&target).await.unwrap(); observer.take_control().await.unwrap();
    human_click(&observer,45).await; observer.release().await.unwrap();
    assert_eq!(get_state(&mut agent,&session).await["active"], "BODY");
}

async fn prefix_activity_case(replacement: &str, expect_frame: bool) {
    let (_state, mut agent, url, target, session) = setup_field().await;
    eval(&mut agent,&session,"one.value='x'.repeat(4096)+'A'").await;
    let (observer, mut events) = ObserverClient::connect(&url).await.unwrap(); observer.authorize("observer-secret").await.unwrap();
    let t = observer.list_targets().await.unwrap().into_iter().find(|t| t.target_id == target).unwrap();
    let observation = observer.observe(&t.connection_id,&target).await.unwrap();
    let _before = next_picture(&observer,&mut events,&observation).await;
    eval(&mut agent,&session,&format!("setTimeout(()=>{{one.value={replacement}}},300)")).await;
    while let Ok(event) = events.try_recv() { if event.method == "Glass.frame" { observer.frame_ack(&observation,&event.params["frameId"]).await.unwrap(); } }
    let frame = tokio::time::timeout(Duration::from_millis(1200), async { loop { let e=events.recv().await.unwrap(); if e.method == "Glass.frame" { break e; } } }).await;
    assert_eq!(frame.is_ok(), expect_frame, "unexpected frame activity for {replacement}");
    if let Ok(frame) = frame {
        observer.frame_ack(&observation,&frame.params["frameId"]).await.unwrap();
        let extra = tokio::time::timeout(Duration::from_millis(800), async { loop { let e=events.recv().await.unwrap(); if e.method == "Glass.frame" { break e; } } }).await;
        assert!(extra.is_err(), "one changed prefix must produce only one frame");
    }
}
#[tokio::test(flavor="current_thread")]
async fn prefix_activity_identical_value_is_quiet() { prefix_activity_case("'x'.repeat(4096)+'A'", false).await; }
#[tokio::test(flavor="current_thread")]
async fn prefix_activity_tail_change_is_quiet() { prefix_activity_case("'x'.repeat(4096)+'B'", false).await; }
#[tokio::test(flavor="current_thread")]
async fn prefix_activity_prefix_change_makes_one_frame() { prefix_activity_case("'y'+'x'.repeat(4095)+'A'", true).await; }

async fn setup_checkables(agent: &mut Socket, session: &str) {
    eval(agent, session, r#"document.body.innerHTML='<form id="form"><input id="cb" type="checkbox" checked style="position:absolute;left:40px;top:30px;width:30px;height:30px"><input id="r1" type="radio" name="r" checked style="position:absolute;left:40px;top:90px;width:30px;height:30px"><input id="r2" type="radio" name="r" style="position:absolute;left:40px;top:150px;width:30px;height:30px"></form>'"#).await;
}
async fn checkable_crop(agent: &mut Socket, session: &str, top: u32) -> Vec<u8> {
    use base64::Engine;
    let shot = call(agent, 31, "Page.captureScreenshot", json!({"format":"png"}), Some(session)).await;
    let bytes = base64::engine::general_purpose::STANDARD.decode(shot["result"]["data"].as_str().unwrap()).unwrap();
    let image = image::load_from_memory(&bytes).unwrap().to_rgb8();
    let mut pixels = Vec::new(); for y in top..top+30 { for x in 40..70 { pixels.extend_from_slice(&image.get_pixel(x,y).0); } } pixels
}
#[tokio::test(flavor="current_thread")]
async fn checked_radio_group_updates_both_pictures() {
    let (_state, mut agent, _url, _target, session) = setup_field().await;
    setup_checkables(&mut agent,&session).await;
    let first = checkable_crop(&mut agent,&session,90).await; let second = checkable_crop(&mut agent,&session,150).await;
    assert_ne!(first,second);
    eval(&mut agent,&session,"r2.click()").await;
    assert_eq!(checkable_crop(&mut agent,&session,90).await,second);
    assert_eq!(checkable_crop(&mut agent,&session,150).await,first);
    assert_eq!(eval(&mut agent,&session,"JSON.stringify([r1.checked,r2.checked])").await,"[false,true]");
}
#[tokio::test(flavor="current_thread")]
async fn checked_controls_reset_and_removal() {
    let (_state, mut agent, _url, _target, session) = setup_field().await;
    setup_checkables(&mut agent,&session).await;
    let initial = checkable_crop(&mut agent,&session,30).await;
    eval(&mut agent,&session,"cb.click()").await;
    assert_ne!(initial,checkable_crop(&mut agent,&session,30).await);
    eval(&mut agent,&session,"form.reset()").await;
    assert_eq!(initial,checkable_crop(&mut agent,&session,30).await);
    eval(&mut agent,&session,"(()=>{cb.click();window.oldControl=cb;let fresh=cb.cloneNode(true);cb.replaceWith(fresh)})()").await;
    assert_eq!(initial,checkable_crop(&mut agent,&session,30).await);
    eval(&mut agent,&session,"cb.replaceWith(oldControl)").await;
    assert_eq!(initial,checkable_crop(&mut agent,&session,30).await);

}
#[tokio::test(flavor="current_thread")]
async fn checked_click_keeps_attributes_and_invalid_op_is_safe() {
    let (_state, mut agent, _url, _target, session) = setup_field().await;
    setup_checkables(&mut agent,&session).await;
    let before = eval(&mut agent,&session,"JSON.stringify([cb.getAttribute('checked'),cb.outerHTML])").await;
    eval(&mut agent,&session,"cb.click()").await;
    assert_eq!(before,eval(&mut agent,&session,"JSON.stringify([cb.getAttribute('checked'),cb.outerHTML])").await);
    let before_invalid = checkable_crop(&mut agent,&session,30).await;
    let result = eval(&mut agent,&session,"(()=>{let cycle={};cycle.self=cycle;Deno.core.ops.op_form_paint_checked(4294967295,true,false,0);Deno.core.ops.op_form_paint_checked(cb._nid,cycle,false,0);Deno.core.ops.op_form_paint_checked(document.body._nid,true,false,0);return 42})()").await;
    assert_eq!(result.as_f64(),Some(42.0)); assert_eq!(eval(&mut agent,&session,"6*7").await.as_f64(),Some(42.0));
    assert_eq!(before_invalid,checkable_crop(&mut agent,&session,30).await);
}
#[tokio::test(flavor="current_thread")]
async fn checked_control_reload_restores_attribute_picture() {
    let (_state, mut agent, url) = setup().await;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let fixture_url = format!("http://{}/", listener.local_addr().unwrap());
    std::thread::spawn(move || { use std::io::{Read, Write}; for stream in listener.incoming() {
        let Ok(mut stream) = stream else { break; }; let mut request = [0; 2048]; let _ = stream.read(&mut request);
        let body = r#"<html><body><input id="cb" type="checkbox" checked style="position:absolute;left:40px;top:30px;width:30px;height:30px"></body></html>"#;
        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
    }});
    let target = call(&mut agent,1,"Target.createTarget",json!({"url":fixture_url}),None).await["result"]["targetId"].as_str().unwrap().to_string();
    let session = format!("{target}-session"); let _ = url;
    let initial = checkable_crop(&mut agent,&session,30).await;
    eval(&mut agent,&session,"cb.click()").await;
    assert_ne!(initial,checkable_crop(&mut agent,&session,30).await);
    call(&mut agent,32,"Page.reload",json!({}),Some(&session)).await;
    assert_eq!(initial,checkable_crop(&mut agent,&session,30).await);
}


#[tokio::test(flavor="current_thread")]
async fn human_checkable_frames_follow_press() {
    use base64::Engine;
    let (_state, mut agent, url, target, session) = setup_field().await;
    setup_checkables(&mut agent,&session).await;
    let (mut ws, _) = connect_async(&url).await.unwrap();
    call(&mut ws,100,"Glass.authorizeObserver",json!({"token":"observer-secret"}),None).await;
    let targets = call(&mut ws,101,"Glass.listTargets",json!({}),None).await;
    let connection = targets["result"]["targets"].as_array().unwrap().iter().find(|t| t["targetId"] == target).unwrap()["connectionId"].clone();
    call(&mut ws,104,"Glass.takeControl",json!({}),None).await;
    let result = call(&mut ws,102,"Glass.observe",json!({"connectionId":connection,"targetId":target,"format":"jpeg"}),None).await;
    let observation = result["result"]["observationId"].clone();
    let first = loop { let v: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap(); if v["method"] == "Glass.frame" { break v; } };
    let mut ack_id = 200;
    let mut pending_acks = std::collections::HashSet::new();
    pending_acks.insert(ack_id);
    ws.send(Message::Text(json!({"id":ack_id,"method":"Glass.frameAck","params":{"observationId":observation,"frameId":first["params"]["frameId"]}}).to_string().into())).await.unwrap();
    let decode = |v: &Value| image::load_from_memory(&base64::engine::general_purpose::STANDARD.decode(v["params"]["data"].as_str().unwrap()).unwrap()).unwrap().to_rgb8();
    let mut baseline = decode(&first);
    // Drain and acknowledge any frames produced before the first press.
    while let Ok(Some(Ok(message))) = tokio::time::timeout(Duration::from_millis(100), ws.next()).await {
        let v: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
        if v["method"] == "Glass.frame" {
            baseline = decode(&v); ack_id += 1; pending_acks.insert(ack_id);
            ws.send(Message::Text(json!({"id":ack_id,"method":"Glass.frameAck","params":{"observationId":observation,"frameId":v["params"]["frameId"]}}).to_string().into())).await.unwrap();
        }
        if let Some(id) = v["id"].as_u64() { pending_acks.remove(&id); }
    }
    assert!(pending_acks.is_empty(), "baseline frame acknowledgement did not complete");
    let different = |a: &image::RgbImage, b: &image::RgbImage, top: u32| {
        (top..top+30).any(|y| (40..70).any(|x| a.get_pixel(x,y) != b.get_pixel(x,y)))
    };
    for (press_id, release_id, y, top) in [(301,302,45,30),(303,304,165,150)] {
        // The baseline is the last received and acknowledged frame before sending this press.
        ws.send(Message::Text(json!({"id":press_id,"method":"Glass.humanCommand","params":{"method":"Input.dispatchMouseEvent","params":{"type":"mousePressed","x":60,"y":y,"button":"left","buttons":1,"clickCount":1,"modifiers":0}}}).to_string().into())).await.unwrap();
        let mut frames = Vec::new(); let mut pressed = false; let mut released = false; let mut deadline = None;
        while !released || deadline.is_some_and(|d| tokio::time::Instant::now() < d) {
            let wait = deadline.map(|d| d.saturating_duration_since(tokio::time::Instant::now())).unwrap_or(Duration::from_secs(2));
            let message = match tokio::time::timeout(wait, ws.next()).await {
                Ok(Some(Ok(message))) => message,
                Err(_) if released => break,
                other => panic!("observer ended before response: {other:?}"),
            };
            let v: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
            if v["method"] == "Glass.frame" {
                frames.push(decode(&v)); ack_id += 1; pending_acks.insert(ack_id);
                ws.send(Message::Text(json!({"id":ack_id,"method":"Glass.frameAck","params":{"observationId":observation,"frameId":v["params"]["frameId"]}}).to_string().into())).await.unwrap();
            }
            if let Some(id) = v["id"].as_u64() { pending_acks.remove(&id); }
            if v["id"] == press_id {
                assert!(v.get("error").is_none(), "{v}"); pressed = true;
                ws.send(Message::Text(json!({"id":release_id,"method":"Glass.humanCommand","params":{"method":"Input.dispatchMouseEvent","params":{"type":"mouseReleased","x":60,"y":y,"button":"left","buttons":0,"clickCount":1,"modifiers":0}}}).to_string().into())).await.unwrap();
            }
            if v["id"] == release_id { assert!(pressed && v.get("error").is_none(), "{v}"); released = true; deadline = Some(tokio::time::Instant::now()+Duration::from_millis(500)); }
            if released && deadline.is_some_and(|d| tokio::time::Instant::now() >= d) { break; }
        }
        assert!(pending_acks.is_empty(), "last frame acknowledgement did not complete");
        assert!(frames.iter().any(|frame| different(&baseline,frame,top)), "no changed frame after press at {y}");
        let last = frames.last().expect("last frame after press");
        assert!(different(&baseline,last,top), "last frame returned to baseline at {y}");
        if y == 165 {
            // Center pixels distinguish the dot from the unfilled interior despite JPEG artifacts.
            assert!(baseline.get_pixel(59,108)[0] < 120 && baseline.get_pixel(59,168)[0] > 200);
            assert!(last.get_pixel(59,108)[0] > 200 && last.get_pixel(59,168)[0] < 120);

        }
        baseline = last.clone();
    }
}
