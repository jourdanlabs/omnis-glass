//! B18/B19 — Handoff cancel + driver tag.

use glass_govern::{lock_for_human, unlock_to_agent};
use glass_ledger::BrowserActionWriter;
use glass_pane_bridge::ReceiptTailer;
use serde_json::Map;
use tempfile::tempdir;

#[test]
fn b18_handoff_cancel_receipt() {
    let dir = tempdir().unwrap();
    std::env::set_var("OMNIS_GLASS_STATE_DIR", dir.path());
    unlock_to_agent();
    glass_govern::record_handoff_cancel("navigate");
    let mut tailer = ReceiptTailer::new(dir.path());
    let entries = tailer.poll_actions().unwrap();
    assert!(entries.iter().any(|e| e.outcome.as_deref() == Some("cancelled_by_handoff")));
}

#[test]
fn b19_driver_tag_on_human_action() {
    let dir = tempdir().unwrap();
    std::env::set_var("OMNIS_GLASS_STATE_DIR", dir.path());
    std::env::set_var(
        "TERMINUS_AUTHORIZE",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts/terminus-authorize.mjs"
        ),
    );
    std::env::set_var("OMNIS_GLASS_LANE_HUMAN", "browse:human");
    std::env::set_var("OMNIS_GLASS_ENFORCE", "local_only");
    unlock_to_agent();
    lock_for_human();
    let writer = BrowserActionWriter::new(dir.path()).unwrap();
    writer
        .write_action_with_meta(
            "navigate",
            "browse:human",
            "ALLOW",
            Map::new(),
            Some("human"),
            None,
        )
        .unwrap();
    let mut tailer = ReceiptTailer::new(dir.path());
    let entries = tailer.poll_actions().unwrap();
    assert_eq!(entries.last().and_then(|e| e.driver.as_deref()), Some("human"));
}
