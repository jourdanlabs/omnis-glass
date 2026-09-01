//! B13 — Action rail redaction.

use glass_ledger::BrowserActionWriter;
use glass_pane_bridge::ReceiptTailer;
use serde_json::Map;
use tempfile::tempdir;

#[test]
fn b13_rail_no_raw_secret() {
    let dir = tempdir().unwrap();
    std::env::set_var("OMNIS_GLASS_STATE_DIR", dir.path());
    let writer = BrowserActionWriter::new(dir.path()).unwrap();
    let mut fields = Map::new();
    fields.insert(
        "field_value".into(),
        serde_json::json!("[REDACTED:sha256:abc123]"),
    );
    writer
        .write_action("type", "browse:research", "REFUSE", fields)
        .unwrap();
    let mut tailer = ReceiptTailer::new(dir.path());
    let entries = tailer.poll_actions().unwrap();
    let line = std::fs::read_to_string(dir.path().join("browser.actions.jsonl")).unwrap();
    assert!(!line.contains("sk-"));
    assert!(entries[0].verdict == "REFUSE");
}
