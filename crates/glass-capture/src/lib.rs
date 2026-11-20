//! `web.capture` ingress provenance and SSRF guard.

mod capture;
mod screencast_frames;
mod source_taint;
mod ssrf;

pub use capture::{build_capture, CaptureKind, CaptureReceipt, CaptureSource, TlsObservation, WebCaptureWriter};
pub use screencast_frames::{ScreencastFrameReceipt, ScreencastFrameWriter};
pub use source_taint::{resolve as resolve_source_taint, TaintVerdict};
pub use ssrf::{SsrfCheck, SsrfVerdict};
