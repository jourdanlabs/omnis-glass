use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Utc;
use glass_ledger::digest_sha256;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureKind {
    Live,
    Cache,
    Fixture,
    EvalOnly,
    HarReplay,
    Screencast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureSource {
    NetworkResponse,
    PageContent,
    Screenshot,
    Pdf,
    Download,
    EvalResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureReceipt {
    pub url: String,
    pub final_url: String,
    pub http_status: Option<u16>,
    pub content_sha256: String,
    pub byte_len: u64,
    pub tls_cert_sha256: Option<String>,
    pub captured_at: String,
    pub capture_kind: CaptureKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dom_extract_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshot_sha256: Option<String>,
}

pub struct WebCaptureWriter {
    capture_path: PathBuf,
}

impl WebCaptureWriter {
    pub fn new(state_dir: &Path) -> std::io::Result<Self> {
        create_dir_all(state_dir)?;
        Ok(Self {
            capture_path: state_dir.join("web.captures.jsonl"),
        })
    }

    pub fn write(&self, receipt: &CaptureReceipt) -> std::io::Result<()> {
        // Structural fail-closed: caller cannot stamp live without network evidence
        if receipt.capture_kind == CaptureKind::Live {
            if receipt.http_status.is_none() || receipt.tls_cert_sha256.is_none() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "capture_kind live requires http_status and tls_cert_sha256",
                ));
            }
        }
        let line = serde_json::to_string(receipt)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.capture_path)?;
        writeln!(file, "{line}")?;
        Ok(())
    }
}

pub fn build_capture(
    url: &str,
    final_url: &str,
    bytes: &[u8],
    source: CaptureSource,
    kind: CaptureKind,
    http_status: Option<u16>,
    tls_cert_sha256: Option<String>,
) -> Result<CaptureReceipt, String> {
    // Structural enforcement before building
    if kind == CaptureKind::Live {
        if http_status.is_none() || tls_cert_sha256.is_none() {
            return Err("cannot stamp capture_kind live without network round-trip evidence".into());
        }
        if source == CaptureSource::EvalResult {
            return Err("eval-only result cannot be capture_kind live".into());
        }
    }
    if kind == CaptureKind::Screencast && source != CaptureSource::Screenshot {
        return Err("screencast capture_kind requires screenshot source".into());
    }

    let content_sha256 = digest_sha256(&String::from_utf8_lossy(bytes));
    let mut receipt = CaptureReceipt {
        url: url.to_string(),
        final_url: final_url.to_string(),
        http_status,
        content_sha256: content_sha256.clone(),
        byte_len: bytes.len() as u64,
        tls_cert_sha256,
        captured_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        capture_kind: kind,
        dom_extract_sha256: None,
        screenshot_sha256: None,
    };

    match source {
        CaptureSource::PageContent | CaptureSource::Download => {
            receipt.dom_extract_sha256 = Some(content_sha256);
        }
        CaptureSource::Screenshot | CaptureSource::Pdf => {
            receipt.screenshot_sha256 = Some(content_sha256);
        }
        _ => {}
    }

    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_requires_network_evidence() {
        let err = build_capture(
            "https://example.com",
            "https://example.com",
            b"hello",
            CaptureSource::PageContent,
            CaptureKind::Live,
            None,
            None,
        );
        assert!(err.is_err());
    }

    #[test]
    fn eval_cannot_be_live() {
        let err = build_capture(
            "https://example.com",
            "https://example.com",
            b"42",
            CaptureSource::EvalResult,
            CaptureKind::Live,
            Some(200),
            Some("abc".into()),
        );
        assert!(err.is_err());
    }

    #[test]
    fn fixture_ok() {
        let r = build_capture(
            "https://example.com",
            "https://example.com",
            b"<html></html>",
            CaptureSource::PageContent,
            CaptureKind::Fixture,
            None,
            None,
        )
        .unwrap();
        assert_eq!(r.capture_kind, CaptureKind::Fixture);
    }
}
