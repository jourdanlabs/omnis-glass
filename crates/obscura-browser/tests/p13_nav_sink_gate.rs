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
