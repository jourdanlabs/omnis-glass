//! A10: acceptance against release binary (when built).

use std::process::Command;

#[test]
fn a10_release_binary_exists_and_receipts_verify() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let root = std::path::Path::new(manifest).join("../..");
    let release_bin = root.join("target/release/omnis-glass");
    if !release_bin.exists() {
        eprintln!("skip: release binary not built yet — run cargo build --release -p obscura-cli --bin omnis-glass");
        return;
    }
    let status = Command::new(&release_bin)
        .args(["receipts", "status"])
        .status()
        .expect("spawn omnis-glass");
    assert!(status.success() || status.code() == Some(0));
}

#[test]
fn a10_cdp_bypass_check_passes() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let script = std::path::Path::new(manifest)
        .join("../../scripts/check-cdp-bypass.mjs");
    let output = Command::new("node")
        .arg(&script)
        .output()
        .expect("run check-cdp-bypass");
    assert!(
        output.status.success(),
        "check-cdp-bypass failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
