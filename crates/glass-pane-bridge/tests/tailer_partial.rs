use std::io::Write;
use glass_pane_bridge::ReceiptTailer;

#[test]
fn tailer_keeps_offset_and_waits_for_complete_lines() {
    let dir = tempfile::tempdir().unwrap();
    let mut file = std::fs::File::create(dir.path().join("browser.actions.jsonl")).unwrap();
    write!(file, "{{\"action\":\"one\"}}\n{{\"action\":\"tw").unwrap();
    let mut tailer = ReceiptTailer::new(dir.path());
    let first = tailer.poll_actions().unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].action, "one");
    assert!(tailer.poll_actions().unwrap().is_empty());
    writeln!(file, "o\"}}").unwrap();
    let second = tailer.poll_actions().unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].action, "two");
    assert!(tailer.poll_actions().unwrap().is_empty());
}
