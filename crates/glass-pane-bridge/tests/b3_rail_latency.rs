//! B3 — action rail reflects gated navigate within 500ms.

use std::time::{Duration, Instant};

use glass_govern::{gate_browser_action, unlock_to_agent};
use glass_pane_bridge::ReceiptTailer;
use glass_terminus::BrowserAction;
use tempfile::tempdir;

fn env(dir: &tempfile::TempDir) {
    std::env::set_var("OMNIS_GLASS_STATE_DIR", dir.path());
    std::env::set_var(
        "TERMINUS_AUTHORIZE",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts/terminus-authorize.mjs"
        ),
    );
    std::env::set_var("OMNIS_GLASS_LANE", "browse:research");
    std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
    unlock_to_agent();
}

#[test]
fn b3_rail_shows_navigate_within_500ms() {
    let dir = tempdir().unwrap();
    env(&dir);
    let start = Instant::now();
    gate_browser_action(
        BrowserAction::Navigate,
        "https://example.com",
        None,
        "b3",
    )
    .expect("gate navigate");

    let mut tailer = ReceiptTailer::new(dir.path());
    let mut found = false;
    while start.elapsed() < Duration::from_millis(500) {
        let entries = tailer.poll_actions().unwrap();
        if entries.iter().any(|e| e.action == "navigate" && e.verdict == "ALLOW") {
            found = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        found,
        "action rail must show ALLOW navigate within 500ms (elapsed {:?})",
        start.elapsed()
    );
}

#[test]
#[ignore = "can-fail mirror — must fail when run with --ignored"]
fn b3_WEAKEN_would_miss_rail_deadline() {
    assert!(
        Duration::from_secs(10) < Duration::from_millis(500),
        "WEAKEN: impossible deadline"
    );
}
