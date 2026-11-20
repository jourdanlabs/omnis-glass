use glass_capture::{build_capture, CaptureKind, CaptureSource, WebCaptureWriter};

use crate::gate::{governance_enabled, state_dir};

pub fn emit_live_capture(
    requested_url: &str,
    final_url: &str,
    bytes: &[u8],
    http_status: u16,
    tls_fingerprint: Option<String>,
) {
    if !governance_enabled() {
        return;
    }
    match build_capture(
        requested_url, final_url, bytes, CaptureSource::PageContent, CaptureKind::Live,
        Some(http_status), tls_fingerprint,
    ) {
        Ok(receipt) => {
            if let Err(error) = WebCaptureWriter::new(&state_dir()).and_then(|writer| writer.write(&receipt)) {
                eprintln!("live capture receipt write failed: {error}");
            }
        }
        Err(error) => eprintln!("live capture receipt build failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn capture_errors_are_reported() {
        if let Ok(kind) = std::env::var("GLASS_CAPTURE_ERROR_CHILD") {
            if kind == "build" {
                super::emit_live_capture("invalid", "invalid", b"body", 200, None);
            } else {
                let dir = tempfile::tempdir().unwrap();
                let file = dir.path().join("not-a-directory");
                std::fs::write(&file, b"x").unwrap();
                std::env::set_var("OMNIS_GLASS_STATE_DIR", file);
                super::emit_live_capture("https://example.test", "https://example.test", b"body", 200, None);
            }
            return;
        }
        for (kind, expected) in [("build", "live capture receipt build failed:"), ("write", "live capture receipt write failed:")] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "capture::tests::capture_errors_are_reported", "--nocapture"])
                .env("GLASS_CAPTURE_ERROR_CHILD", kind).output().unwrap();
            assert!(output.status.success());
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains(expected), "{stderr}");
        }
    }
}
