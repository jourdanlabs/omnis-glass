//! OMNIS GLASS Glass Pane — native oversight window.

use std::borrow::Cow;
use std::path::PathBuf;
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::time::Instant;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use clap::Parser;
use glass_govern::state_dir;
use glass_pane_bridge::{parse_ipc_message, is_pane_ui_url, PaneAuth, ReceiptTailer};
use glass_pane_ui::UI;
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    window::WindowBuilder,
};
use wry::http::{Request, Response};
use wry::WebViewBuilder;

#[derive(Parser)]
#[command(name = "glass-pane", about = "OMNIS GLASS — Glass Pane oversight window")]
struct Args {
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    #[arg(long, default_value_t = 9222)]
    port: u16,
    #[arg(long)]
    attach: Option<String>,
    #[arg(long, default_value_t = true)]
    co_launch: bool,
    #[arg(long)]
    state_dir: Option<PathBuf>,
    #[arg(long, default_value = "browse:human")]
    lane_human: String,
    #[arg(long, default_value = "browse:research")]
    lane_agent: String,
    #[arg(long)]
    cdp_token: Option<String>,
    #[arg(long)]
    mirror_only: bool,
    #[arg(long)]
    headless: bool,
    #[arg(long)]
    observer_token: Option<String>,
    #[arg(long)]
    target: Option<String>,
    #[arg(long)]
    frames: Option<usize>,
    /// Count frames only after the observed target navigates.
    #[arg(long, requires = "headless")]
    after_navigation: bool,
    #[arg(long, default_value_t = 10)]
    timeout_secs: u64,
}

fn main() -> anyhow::Result<()> {
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!("{}", glass_build_info::version_line("glass-pane"));
        return Ok(());
    }
    if std::env::var("OMNIS_GLASS_PANE").ok().as_deref() == Some("0") {
        anyhow::bail!("OMNIS GLASS pane disabled (OMNIS_GLASS_PANE=0)");
    }

    let args = Args::parse();
    if let Some(dir) = &args.state_dir {
        std::env::set_var("OMNIS_GLASS_STATE_DIR", dir);
    }
    std::env::set_var("OMNIS_GLASS_LANE_HUMAN", &args.lane_human);
    std::env::set_var("OMNIS_GLASS_LANE", &args.lane_agent);

    let pane_auth = Arc::new(PaneAuth::generate());
    let auth_token = pane_auth.token().to_string();
    let observer_token = args.observer_token.clone().or_else(|| std::env::var("OMNIS_GLASS_PANE_OBSERVER_TOKEN").ok()).unwrap_or_else(|| if args.attach.is_none() { PaneAuth::generate().token().to_string() } else { String::new() });

    let cdp_token = args.cdp_token.clone().or_else(|| std::env::var("OMNIS_GLASS_CDP_TOKEN").ok())
        .unwrap_or_else(|| if args.co_launch && args.attach.is_none() { PaneAuth::generate().token().to_string() } else { String::new() });
    if args.co_launch && args.attach.is_none() {
        if args.host != "127.0.0.1" { anyhow::bail!("pane co-launch must bind 127.0.0.1"); }
        std::env::set_var("OMNIS_GLASS_CDP_TOKEN", &cdp_token);
        let host = args.host.clone();
        let port = args.port;
        let server_observer_token = observer_token.clone();
        let mirror_only = args.mirror_only;
        // Fail instead of connecting to an unrelated process already using this port.
        drop(TcpListener::bind((host.as_str(), port))?);
        let (server_result_tx, server_result_rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let result = tokio::runtime::Runtime::new()
                .map_err(|e| e.to_string())
                .and_then(|rt| rt.block_on(start_pane_server(port, &host, &server_observer_token, mirror_only)).map_err(|e| e.to_string()));
            if let Err(error) = server_result_tx.send(result) {
                eprintln!("pane server result could not be reported: {error}");
            }
        });
        let address = (args.host.as_str(), args.port).to_socket_addrs()?
            .next().ok_or_else(|| anyhow::anyhow!("CDP host has no address"))?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(result) = server_result_rx.try_recv() {
                match result {
                    Ok(()) => anyhow::bail!("CDP server exited before becoming ready"),
                    Err(error) => anyhow::bail!("CDP server failed: {error}"),
                }
            }
            if TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() {
                break;
            }
            if Instant::now() >= deadline {
                anyhow::bail!("CDP server did not start on {address} before the deadline");
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    let cdp_url = if let Some(ref url) = args.attach {
        attach_url_with_token(url, &cdp_token)
    } else if cdp_token.is_empty() {
        format!("ws://{}:{}/devtools/browser", args.host, args.port)
    } else {
        format!(
            "ws://{}:{}/devtools/browser/{}",
            args.host, args.port, cdp_token
        )
    };

    if args.headless {
        let frames = args.frames.unwrap_or(if args.attach.is_none() && args.co_launch { 1 } else { 5 });
        let result = tokio::runtime::Runtime::new()?.block_on(headless_check(&cdp_url, &observer_token, args.target.as_deref(), frames, args.timeout_secs, args.after_navigation));
        println!("{}", result);
        if result["error"].is_null() { return Ok(()); }
        std::process::exit(1);
    }

    let event_loop = EventLoopBuilder::<PaneEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel::<serde_json::Value>();
    let window = WindowBuilder::new()
        .with_title("OMNIS GLASS — Glass Pane")
        .with_inner_size(tao::dpi::LogicalSize::new(1280.0, 800.0))
        .build(&event_loop)?;

    let pane_auth_ipc = pane_auth.clone();
    let auth_script = format!("window.__GLASS_PANE_AUTH_TOKEN = {};", serde_json::to_string(&auth_token)?);
    let webview = WebViewBuilder::new()
        .with_custom_protocol("glass-pane".into(), move |_id, request| {
            let path = request.uri().path();
            let file = match path {
                "/" | "/index.html" => UI.get_file("index.html"),
                "/pane.css" => UI.get_file("pane.css"),
                "/pane.js" => UI.get_file("pane.js"),
                _ => None,
            };
            if let Some(f) = file {
                let mime = if path.ends_with(".css") {
                    "text/css"
                } else if path.ends_with(".js") {
                    "application/javascript"
                } else {
                    "text/html"
                };
                let body = f.contents().to_vec();
                Response::builder()
                    .header("Content-Type", mime)
                    .body(Cow::Owned(body))
                    .unwrap()
            } else {
                Response::builder()
                    .status(404)
                    .body(Cow::Borrowed(b"not found" as &[u8]))
                    .unwrap()
            }
        })
        .with_navigation_handler(|url| is_pane_ui_url(&url))
        .with_new_window_req_handler(|_| false)
        .with_initialization_script(&auth_script)
        .with_ipc_handler(move |req: Request<String>| {
            if !is_pane_ui_url(&req.uri().to_string()) {
                eprintln!("pane IPC refused: untrusted origin");
                return;
            }
            let Some((token, command)) = parse_ipc_message(req.body()) else { eprintln!("pane IPC refused: invalid format"); return; };
            if !pane_auth_ipc.check(Some(token)) { eprintln!("pane IPC refused: invalid authentication"); return; }
            match serde_json::from_str(command) {
                Ok(command) => if command_tx.send(command).is_err() { eprintln!("pane IPC failed: observer disconnected"); },
                Err(error) => eprintln!("pane IPC refused: {error}"),
            }
        })
        .with_url("glass-pane://localhost/index.html")
        .build(&window)?;

    let token = observer_token;
    let pin = args.target;
    let mirror_only = args.mirror_only;
    let attach_mode = args.attach.is_some();
    let worker_proxy = proxy.clone();
    thread::spawn(move || {
        match tokio::runtime::Runtime::new() {
            Ok(rt) => rt.block_on(pane_worker(cdp_url, token, pin, mirror_only, attach_mode, command_rx, state_dir(), |name, value| show(&worker_proxy, name, value))),
            Err(error) => eprintln!("not connected: {error}"),
        }
    });
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *control_flow = ControlFlow::Exit,
            Event::UserEvent(PaneEvent::Ui(function, value)) => {
                match serde_json::to_string(&value) {
                    Ok(value) => if let Err(error) = webview.evaluate_script(&format!("window.__glassPane.{function}({value})")) { eprintln!("pane UI update failed: {error}"); },
                    Err(error) => eprintln!("pane UI serialization failed: {error}"),
                }
            }
            _ => ()
        }
    });
}

fn attach_url_with_token(url: &str, token: &str) -> String {
    if token.is_empty() { return url.to_string(); }
    let (path, query) = match url.split_once('?') { Some((path, query)) => (path, Some(query)), None => (url, None) };
    let path = path.trim_end_matches('/');
    if !path.ends_with("/devtools/browser") { return url.to_string(); }
    let suffix = query.map(|q| format!("?{q}")).unwrap_or_default();
    format!("{path}/{token}{suffix}")
}

async fn headless_check(url: &str, token: &str, pin: Option<&str>, frames: usize, timeout_secs: u64, after_navigation: bool) -> serde_json::Value {
    use glass_pane_bridge::observer::{choose_target, ObserverClient};
    use serde_json::json;
    let mut out = json!({"connected":false,"authorized":false,"target":null,"frames":0,"mode":null,"error":null});
    let run = async {
        let (client, mut events) = ObserverClient::connect(url).await.map_err(|e| e.to_string())?;
        out["connected"] = json!(true);
        client.authorize(token).await.map_err(|e| e.to_string())?;
        out["authorized"] = json!(true);
        let parsed = match pin { Some(p) => Some(p.split_once('/').ok_or("invalid target: expected connectionId/targetId")?), None => None };
        let (target, _) = choose_target(&client, parsed).await.map_err(|e| e.to_string())?;
        out["target"] = json!({"connectionId":target.connection_id,"targetId":target.target_id});
        client.observe(&target.connection_id, &target.target_id).await.map_err(|e| e.to_string())?;
        let mode = client.get_mode().await.map_err(|e| e.to_string())?;
        out["mode"] = json!(mode.badge());
        if after_navigation { eprintln!("waiting for target navigation"); }
        let mut navigated = !after_navigation;
        while out["frames"].as_u64().unwrap_or(0) < frames as u64 {
            let event = events.recv().await.ok_or("observer stream closed")?;
            if event.method == "Glass.targetNavigated" && event.params["url"].as_str().is_some_and(|url| url != target.url) { navigated = true; }
            if event.method == "Glass.frame" {
                let observation = event.params["observationId"].as_str().ok_or("frame missing observationId")?;
                let frame_id = event.params.get("frameId").ok_or("frame missing frameId")?;
                client.frame_ack(observation, frame_id).await.map_err(|e| e.to_string())?;
                if navigated { out["frames"] = json!(out["frames"].as_u64().unwrap_or(0) + 1); }
            }
        }
        Ok::<(), String>(())
    };
    let error = match tokio::time::timeout(Duration::from_secs(timeout_secs), run).await {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error),
        Err(_) => Some("timed out waiting for frames".into()),
    };
    if let Some(error) = error { eprintln!("not connected: {error}"); out["error"] = json!(error); }
    out
}

#[derive(Debug)]
enum PaneEvent { Ui(&'static str, serde_json::Value) }
fn show(proxy: &EventLoopProxy<PaneEvent>, name: &'static str, value: impl Into<serde_json::Value>) {
    if proxy.send_event(PaneEvent::Ui(name, value.into())).is_err() { eprintln!("pane window closed"); }
}
async fn pane_worker(url: String, token: String, pin: Option<String>, mirror_only: bool, attach_mode: bool, mut commands: tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>, state_path: PathBuf, mut emit: impl FnMut(&'static str, serde_json::Value)) {
    use glass_pane_bridge::observer::{choose_target, dispatch_ui_with_events, refresh_target, ObserverClient, UiEvent};
    use serde_json::json;
    let result: Result<(), String> = async {
        loop {
            match commands.recv().await {
                Some(command) if command["type"] == "ready" => break,
                Some(_) => continue,
                None => return Err("pane UI closed".into()),
            }
        }
        emit("setMirrorOnly", json!(mirror_only));
        let (client, mut events) = tokio::time::timeout(Duration::from_secs(10), ObserverClient::connect(&url)).await.map_err(|_| "connection timed out".to_string())?.map_err(|e| e.to_string())?;
        let server_ledger_hash = tokio::time::timeout(Duration::from_secs(10), client.authorize(&token)).await.map_err(|_| "authorization timed out".to_string())?.map_err(|e| e.to_string())?;
        let pinned = match pin.as_deref() { Some(p) => Some(p.split_once('/').ok_or("invalid target: expected connectionId/targetId")?), None => None };
        let (mut target, own) = choose_target(&client, pinned).await.map_err(|e| e.to_string())?;
        let mut own_target = if own { Some((target.connection_id.clone(), target.target_id.clone())) } else { None };
        let mut observation_id = client.observe(&target.connection_id, &target.target_id).await.map_err(|e| e.to_string())?;
        emit("setUrl", json!(target.url));
        emit("setScriptsStopped", json!(target.scripts_stopped));
        emit("setStatus", json!(if own { "observing own page" } else { "observing agent page" }));
        let mut mode = client.get_mode().await.map_err(|e| e.to_string())?;
        emit("setMode", json!(mode.badge()));
        let mut tailer = if attach_mode { ReceiptTailer::for_attach(&state_path, server_ledger_hash.as_deref()) } else { ReceiptTailer::new(&state_path) };
        if let Some(message) = tailer.warning() { emit("appendRail", json!({"verdict":"", "action":message, "url":""})); }
        let mut timer = tokio::time::interval(Duration::from_millis(500));
        let mut last_capture = None;
        loop {
            tokio::select! {
                _ = timer.tick() => {
                    match tailer.poll_actions() {
                        Ok(entries) => for entry in entries { emit("appendRail", json!({"verdict":entry.verdict,"action":entry.action,"url":entry.url})); },
                        Err(error) => emit("setStatus", json!(format!("receipt tail: {error}"))),
                    }
                    let capture = tailer.last_capture_sha();
                    if capture != last_capture { emit("setCapture", json!(capture)); last_capture = capture; }
                }
                event = events.recv() => {
                    let event = event.ok_or("observer connection closed")?;
                    match event.method.as_str() {
                        "Glass.frame" => {
                            let observation = event.params["observationId"].as_str().ok_or("frame missing observationId")?;
                            let frame_id = event.params.get("frameId").ok_or("frame missing frameId")?;
                            emit("drawFrame", event.params.clone());
                            client.frame_ack(observation, frame_id).await.map_err(|e| e.to_string())?;
                        }
                        "Glass.targetNavigated" if event.params["observationId"].as_str() == Some(&observation_id) => emit("setUrl", event.params["url"].clone()),
                        "Glass.scriptsStoppedChanged" if event.params["connectionId"].as_str() == Some(&target.connection_id) && event.params["targetId"].as_str() == Some(&target.target_id) => {
                            // A queued event can predate the target snapshot selected above.
                            // Read the latest state so an old event cannot restore a stale notice.
                            match client.list_targets().await {
                                Ok(targets) => if let Some(current) = targets.into_iter().find(|t| t.connection_id == target.connection_id && t.target_id == target.target_id) {
                                    target.scripts_stopped = current.scripts_stopped;
                                    emit("setScriptsStopped", json!(current.scripts_stopped));
                                },
                                Err(error) => emit("setStatus", json!(error.to_string())),
                            }
                        }
                        "Glass.modeChanged" => {
                            mode = serde_json::from_value(event.params).map_err(|e| format!("invalid mode event: {e}"))?;
                            emit("setMode", json!(mode.badge()));
                        }
                        "Glass.targetsChanged" | "Glass.observationEnded" => {
                            if event.method == "Glass.observationEnded" && event.params["observationId"].as_str() != Some(&observation_id) { continue; }
                            if event.method == "Glass.observationEnded" { emit("setStatus", json!(format!("observation ended: {}", event.params["reason"].as_str().unwrap_or("unknown")))); }
                            match refresh_target(&client, &event, pinned, &target, &observation_id).await {
                                Ok(Some((next, own, id))) => {
                                    if own { own_target = Some((next.connection_id.clone(), next.target_id.clone())); }
                                    observation_id = id;
                                    emit("setUrl", json!(next.url));
                                    emit("setScriptsStopped", json!(next.scripts_stopped));
                                    let is_own = own_target.as_ref().map(|(c, t)| c == &next.connection_id && t == &next.target_id).unwrap_or(false);
                                    let selected = if is_own { "observing own page" } else { "observing agent page" };
                                    let status = if event.method == "Glass.observationEnded" { format!("observation ended: {}; {selected}", event.params["reason"].as_str().unwrap_or("unknown")) } else { selected.to_string() };
                                    emit("setStatus", json!(status));
                                    target = next;
                                }
                                Ok(None) => (),
                                Err(error) => emit("setStatus", json!(error.to_string())),
                            }
                        }
                        _ => ()
                    }
                }
                command = commands.recv() => {
                    let Some(command) = command else { break; };
                    dispatch_ui_with_events(&client, &mut mode, mirror_only, &command, |event| match event { UiEvent::Status(message) => emit("setStatus", json!(message)), UiEvent::Mode(badge) => emit("setMode", json!(badge)) }).await;
                }
            }
        }
        Ok(())
    }.await;
    if let Err(error) = result { let message = format!("not connected: {error}"); eprintln!("{message}"); emit("setStatus", json!(message)); }
}

async fn start_pane_server(port: u16, host: &str, observer_token: &str, mirror_only: bool) -> anyhow::Result<()> {
    obscura_cdp::start_with_pane_observer_options(port, host, None, false, None, false, None, false, obscura_cdp::DEFAULT_MAX_CONNECTIONS, obscura_cdp::PaneObserverOptions { token: Some(observer_token.to_string()), mirror_only }).await?;
    Ok(())
}

#[cfg(test)]
mod worker_tests;
