//! A10: acceptance against release binary (when built).

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn release_dir() -> PathBuf {
    std::env::var("GLASS_RELEASE_TARGET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root().join("target/release"))
}

#[path = "../../../tests/release_pin.rs"]
mod release_pin;

#[test]
fn a10_release_binary_exists_and_receipts_verify() {
    let release_bin = release_dir().join("omnis-glass");
    assert!(
        release_bin.exists(),
        "release binary missing at {} — cargo build --release -p obscura-cli --bin omnis-glass",
        release_bin.display()
    );
    release_pin::check(&repo_root(), &release_dir(), "omnis-glass", "omnis_glass");
    let state = tempfile::tempdir().unwrap();
    let status = Command::new(&release_bin)
        .env("OMNIS_GLASS_STATE_DIR", state.path())
        .args(["receipts", "status"])
        .status()
        .expect("spawn omnis-glass");
    assert!(status.success() || status.code() == Some(0));
}

#[test]
fn a10_cdp_bypass_check_passes() {
    let script = repo_root().join("scripts/check-cdp-bypass.mjs");
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
