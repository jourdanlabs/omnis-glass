//! B10/B15 — v1.1 release checks.

use std::process::Command;

#[test]
fn b10_pane_bypass_check_passes() {
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/check-pane-bypass.mjs");
    let output = Command::new("node")
        .arg(&script)
        .output()
        .expect("run check-pane-bypass");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn b15_pane_help_and_headless() {
    let pane = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/release/glass-pane");
    if !pane.exists() {
        eprintln!("skip: build glass-pane release first");
        return;
    }
    let help = Command::new(&pane)
        .arg("--help")
        .output()
        .expect("pane --help");
    assert!(help.status.success());
    let headless = Command::new(&pane)
        .args(["--headless", "--co-launch"])
        .output()
        .expect("pane headless");
    assert!(headless.status.success());
}
