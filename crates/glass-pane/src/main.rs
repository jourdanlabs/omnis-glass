//! OMNIS GLASS Glass Pane — native oversight window.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use clap::Parser;
use glass_govern::state_dir;
use glass_pane_bridge::{CdpClient, PaneAuth, PaneSession, ReceiptTailer, SessionCommand};
use glass_pane_ui::UI;
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
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
}

fn main() -> anyhow::Result<()> {
    if std::env::var("OMNIS_GLASS_PANE").ok().as_deref() == Some("0") {
        anyhow::bail!("OMNIS GLASS pane disabled (OMNIS_GLASS_PANE=0)");
    }

    let args = Args::parse();
    if let Some(dir) = &args.state_dir {
        std::env::set_var("OMNIS_GLASS_STATE_DIR", dir);
    }
    std::env::set_var("OMNIS_GLASS_LANE_HUMAN", &args.lane_human);
    std::env::set_var("OMNIS_GLASS_LANE", &args.lane_agent);

    let pane_auth = PaneAuth::generate();
    let session = Arc::new(Mutex::new(PaneSession::new(args.mirror_only)));

    if args.co_launch && args.attach.is_none() {
        let host = args.host.clone();
        let port = args.port;
        thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("tokio");
            let _ = rt.block_on(obscura_cdp::start_with_serve_options_and_limit(
                port,
                &host,
                None,
                false,
                None,
                false,
                None,
                false,
                obscura_cdp::DEFAULT_MAX_CONNECTIONS,
            ));
        });
        thread::sleep(Duration::from_millis(800));
    }

    let cdp_token = args
        .cdp_token
        .or_else(|| std::env::var("OMNIS_GLASS_CDP_TOKEN").ok())
        .unwrap_or_default();
    let cdp_url = if let Some(url) = args.attach {
        url
    } else if cdp_token.is_empty() {
        format!("ws://{}:{}/devtools/browser", args.host, args.port)
    } else {
        format!(
            "ws://{}:{}/devtools/browser/{}",
            args.host, args.port, cdp_token
        )
    };

    if args.headless {
        println!("glass-pane headless: CDP {}", cdp_url);
        return Ok(());
    }

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("OMNIS GLASS — Glass Pane")
        .with_inner_size(tao::dpi::LogicalSize::new(1280.0, 800.0))
        .build(&event_loop)?;

    let session_ipc = session.clone();
    let _auth_token = pane_auth.token().to_string();
    let webview = WebViewBuilder::new()
        .with_custom_protocol("glass-pane".into(), |_id, request| {
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
                Response::builder()
                    .header("Content-Type", mime)
                    .body(Cow::Owned(f.contents().to_vec()))
                    .unwrap()
            } else {
                Response::builder()
                    .status(404)
                    .body(Cow::Borrowed(b"not found" as &[u8]))
                    .unwrap()
            }
        })
        .with_ipc_handler(move |req: Request<String>| {
            let body = req.body();
            let mut s = session_ipc.lock().unwrap();
            match body.as_str() {
                "take_control" => {
                    let _ = s.handle_command(SessionCommand::TakeControl);
                }
                "release" => {
                    let _ = s.handle_command(SessionCommand::Release);
                }
                nav if nav.starts_with("navigate:") => {
                    let url = nav.trim_start_matches("navigate:");
                    let _ = s.handle_command(SessionCommand::Navigate(url.to_string()));
                }
                _ => {}
            }
        })
        .with_url("glass-pane://localhost/index.html")
        .build(&window)?;

    let cdp_url_bg = cdp_url.clone();
    thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio");
        rt.block_on(async {
            if let Ok(mut client) = CdpClient::connect(&cdp_url_bg).await {
                let _ = client.start_screencast().await;
            }
            loop {
                let mut tailer = ReceiptTailer::new(&state_dir());
                let _ = tailer.poll_actions();
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });
    });

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
        let _ = &webview;
    });
}
