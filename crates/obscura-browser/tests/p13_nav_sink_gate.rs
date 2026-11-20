// P1-3 (nav sink gate) regression pair: the gated sink AND the JS-chain
// second drain point. Both must refuse ungated navigations.
use std::sync::Arc;

use obscura_browser::{BrowserContext, Page};

fn setup_glass_env(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("p13-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("OMNIS_GLASS_STATE_DIR", &dir);
    std::env::set_var(
        "TERMINUS_AUTHORIZE",
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/terminus-authorize.mjs"),
    );
    std::env::set_var("OMNIS_GLASS_LANE", "browse:research");
    std::env::set_var("OMNIS_GLASS_ENFORCE", "enforce");
    dir
}

fn dump_ledger_lines(dir: &std::path::Path, needle: &str) {
    fn walk(p: &std::path::Path, needle: &str) {
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                let path = e.path();
                if path.is_dir() {
                    walk(&path, needle);
                } else if let Ok(s) = std::fs::read_to_string(&path) {
                    for line in s.lines() {
                        if line.contains(needle) {
                            eprintln!("LEDGER[{}]: {}", path.display(), line);
                        }
                    }
                }
            }
        }
    }
    walk(dir, needle);
}

#[tokio::test(flavor = "current_thread")]
async fn p13_sink_gate_refuses_js_queued_nav() {
    let dir = setup_glass_env("sink");
    let ctx = Arc::new(BrowserContext::with_storage_full(
        "p13".into(),
        None,
        false,
        None,
        None,
    ));
    let mut page = Page::new("p13-page".into(), ctx);
    page.navigate("data:text/html,<body id=b></body>")
        .await
        .expect("data: nav");
    page.evaluate("location.href = 'https://google.com/'");
    let r = page.process_pending_navigation().await;
    eprintln!("SINK RESULT: {:?}", r.as_ref().map(|b| *b).map_err(|e| e.to_string()));
    eprintln!("SINK URL AFTER: {}", page.url_string());
    dump_ledger_lines(&dir, "google.com");
    let err = r.expect_err("sink must refuse");
    let msg = err.to_string();
    assert!(
        msg.contains("Request blocked") && msg.contains("TERMINUS"),
        "unexpected error: {msg}"
    );
    assert!(
        !page.url_string().contains("google.com"),
        "url moved: {}",
        page.url_string()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn p13_chain_hop_which_layer() {
    let dir = setup_glass_env("chain");
    let ctx = Arc::new(BrowserContext::with_storage_full(
        "p13c".into(),
        None,
        false,
        None,
        None,
    ));
    let mut page = Page::new("p13c-page".into(), ctx);
    let r = page
        .navigate("data:text/html,<script>location.href='https://google.com/'</script>")
        .await;
    eprintln!("CHAIN RESULT: {:?}", r.as_ref().map(|_| ()).map_err(|e| e.to_string()));
    eprintln!("CHAIN URL AFTER: {}", page.url_string());
    dump_ledger_lines(&dir, "google.com");
    let err = r.expect_err("chain must refuse");
    let msg = err.to_string();
    assert!(
        msg.contains("Request blocked") && msg.contains("TERMINUS"),
        "unexpected error: {msg}"
    );
    // Page url embeds the script text (which mentions google.com), so only a
    // data: prefix proves no navigation happened.
    assert!(
        page.url_string().starts_with("data:"),
        "url moved: {}",
        page.url_string()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn form_post_leaves_no_navigation_approval_and_get_approval_is_one_use() {
    use std::io::{Read, Write};
    let dir = setup_glass_env("approval");
    std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || for stream in listener.incoming() {
        let Ok(mut stream) = stream else { break; };
        let mut buf = [0; 4096]; let n = stream.read(&mut buf).unwrap_or(0);
        let line = String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("").to_owned(); let _ = tx.send(line);
        let body = "<html><body>ok</body></html>";
        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
    });
    let ctx = Arc::new(BrowserContext::with_storage_and_network("approval".into(), None, false, None, None, true));
    let mut page = Page::new("approval-page".into(), ctx);
    let count = |url: &str| -> usize {
        std::fs::read_to_string(dir.join("browser.actions.jsonl")).unwrap_or_default().lines()
            .filter(|line| { let v: serde_json::Value = serde_json::from_str(line).unwrap(); v["action"] == "navigate" && v["url"] == url }).count()
    };
    glass_govern::with_command_context(glass_govern::CommandContext::agent().with_private_network(true), async {
        page.navigate(&format!("{origin}/start")).await.unwrap();
        let post = format!("{origin}/post");
        assert_eq!(page.evaluate(&format!("(()=>{{try{{document.body.innerHTML='<form id=f method=post action=\"{post}\"><input name=x value=y></form>';document.getElementById('f').submit();return 'submitted'}}catch(e){{return String(e)}}}})()")), "submitted");
        assert!(page.process_pending_navigation().await.unwrap());
        assert!(rx.try_iter().any(|line| line.starts_with("POST /post ")), "form POST reached fixture");
        let before = count(&post);
        glass_govern::gate_http_document_get(&url::Url::parse(&post).unwrap(), "approval-page", true).unwrap();
        assert_eq!(count(&post), before + 1, "POST must not leave a reusable GET approval");
        let get = format!("{origin}/get");
        page.evaluate(&format!("location.href='{get}'"));
        assert!(page.process_pending_navigation().await.unwrap());
        assert!(rx.try_iter().any(|line| line.starts_with("GET /get ")), "document GET reached fixture");
        let before = count(&get);
        glass_govern::gate_http_document_get(&url::Url::parse(&get).unwrap(), "approval-page", true).unwrap();
        assert_eq!(count(&get), before + 1, "GET approval must already have been consumed");
    }).await;
}
