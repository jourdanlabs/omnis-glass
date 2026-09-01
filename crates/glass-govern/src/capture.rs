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
    if let Ok(receipt) = build_capture(
        requested_url,
        final_url,
        bytes,
        CaptureSource::PageContent,
        CaptureKind::Live,
        Some(http_status),
        tls_fingerprint.or_else(|| Some("pending-v1".into())),
    ) {
        if let Ok(writer) = WebCaptureWriter::new(&state_dir()) {
            let _ = writer.write(&receipt);
        }
    }
}
