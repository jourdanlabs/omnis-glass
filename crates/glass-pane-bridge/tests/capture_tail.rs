use std::io::Write;
use glass_pane_bridge::ReceiptTailer;

#[test]
fn capture_hash_survives_partial_next_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("web.captures.jsonl");
    std::fs::write(&path, "{\"content_sha256\":\"first\"}\n").unwrap();
    let tailer = ReceiptTailer::new(dir.path());
    assert_eq!(tailer.last_capture_sha().as_deref(), Some("first"));
    let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
    write!(file, "{{\"content_sha256\":\"second").unwrap();
    assert_eq!(tailer.last_capture_sha().as_deref(), Some("first"));
    writeln!(file, "\"}}").unwrap();
    assert_eq!(tailer.last_capture_sha().as_deref(), Some("second"));
}
