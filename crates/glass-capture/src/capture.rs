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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TlsObservation { Observed, NotObserved, NotApplicable }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureReceipt {
    pub url: String,
    pub final_url: String,
    pub http_status: Option<u16>,
    pub content_sha256: String,
    pub byte_len: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls_observation: Option<TlsObservation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
            if receipt.http_status.is_none() || !valid_tls(&receipt.final_url, receipt.tls_observation, receipt.tls_cert_sha256.as_deref()) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "capture_kind live requires http_status and consistent TLS observation",
                ));
            }
        }
        let line = serde_json::to_string(receipt)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.capture_path)?;
        glass_ledger::lock_ledger_file(&file, &self.capture_path)?;
        let line = format!("{line}\n");
        if file.write(line.as_bytes())? != line.len() {
            return Err(std::io::Error::new(std::io::ErrorKind::WriteZero, "short record write"));
        }
        file.flush()?;
        Ok(())
    }
}

fn valid_tls(url: &str, observation: Option<TlsObservation>, hash: Option<&str>) -> bool {
    let scheme = url::Url::parse(url).ok().map(|url| url.scheme().to_owned());
    match (scheme.as_deref(), observation, hash) {
        (Some("https"), Some(TlsObservation::Observed), Some(hash)) =>
            hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
        (Some("https"), Some(TlsObservation::NotObserved), None) => true,
        (Some(scheme), Some(TlsObservation::NotApplicable), None) if scheme != "https" => true,
        _ => false,
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
    let tls_observation = match url::Url::parse(final_url).map(|url| url.scheme().to_owned()) {
        Ok(scheme) if scheme == "https" => if tls_cert_sha256.is_some() { TlsObservation::Observed } else { TlsObservation::NotObserved },
        Ok(_) => TlsObservation::NotApplicable,
        _ => return Err("unsupported capture URL scheme".into()),
    };
    if kind == CaptureKind::Live {
        if http_status.is_none() || !valid_tls(final_url, Some(tls_observation), tls_cert_sha256.as_deref()) {
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
        tls_observation: Some(tls_observation),
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
    fn live_receipts_record_tls_observation() {
        let dir = std::env::temp_dir().join(format!("glass-capture-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap()));
        let writer = WebCaptureWriter::new(&dir).unwrap();
        for url in ["http://example.test/", "https://example.test/", "blob:https://example.test/id", "custom:resource"] {
            let receipt = build_capture(url, url, b"page", CaptureSource::PageContent, CaptureKind::Live, Some(200), None).unwrap();
            writer.write(&receipt).unwrap();
        }
        let data = std::fs::read_to_string(dir.join("web.captures.jsonl")).unwrap();
        let lines: Vec<serde_json::Value> = data.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(lines[0]["tls_observation"], "not_applicable");
        assert_eq!(lines[1]["tls_observation"], "not_observed");
        assert_eq!(lines[2]["tls_observation"], "not_applicable");
        assert_eq!(lines[3]["tls_observation"], "not_applicable");
        assert!(lines.iter().all(|line| line.get("tls_cert_sha256").is_none()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_inconsistent_tls_observations() {
        assert!(!valid_tls("http://example.test", Some(TlsObservation::Observed), Some(&"a".repeat(64))));
        assert!(!valid_tls("https://example.test", Some(TlsObservation::Observed), Some("not-a-hash")));
        assert!(valid_tls("https://example.test", Some(TlsObservation::Observed), Some(&"a".repeat(64))));
        assert!(!valid_tls("https://example.test", Some(TlsObservation::NotApplicable), None));
        assert!(valid_tls("http://example.test", Some(TlsObservation::NotApplicable), None));
    }

    #[test]
    fn live_https_cannot_claim_tls_not_applicable() {
        let dir = std::env::temp_dir().join(format!("glass-capture-s8-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap()));
        let writer = WebCaptureWriter::new(&dir).unwrap();
        let mut receipt = build_capture("https://example.test", "https://example.test", b"page", CaptureSource::PageContent, CaptureKind::Live, Some(200), None).unwrap();
        receipt.tls_observation = Some(TlsObservation::NotApplicable);
        assert!(writer.write(&receipt).is_err());
        let path = dir.join("web.captures.jsonl");
        assert!(!path.exists() || std::fs::read(path).unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
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
