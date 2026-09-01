//! B20 — screencast excluded from source taint.

use glass_capture::{resolve_source_taint, CaptureKind, TaintVerdict};

#[test]
fn b20_screencast_not_provenance() {
    assert!(matches!(
        resolve_source_taint(CaptureKind::Screencast),
        TaintVerdict::Rejected(_)
    ));
}

#[test]
fn b8_screencast_not_live() {
    use glass_capture::{build_capture, CaptureSource};
    let err = build_capture(
        "https://example.com",
        "https://example.com",
        b"frame",
        CaptureSource::PageContent,
        CaptureKind::Screencast,
        None,
        None,
    );
    assert!(err.is_err());
    let ok = build_capture(
        "https://example.com",
        "https://example.com",
        b"frame",
        CaptureSource::Screenshot,
        CaptureKind::Screencast,
        None,
        None,
    );
    assert!(ok.is_ok());
}
