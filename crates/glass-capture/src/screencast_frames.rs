//! Separate ledger for optional screencast frame shas (never web.captures.jsonl).

use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ScreencastFrameReceipt {
    pub frame_sha256: String,
    pub captured_at: String,
    pub session_id: String,
}

pub struct ScreencastFrameWriter {
    path: PathBuf,
}

impl ScreencastFrameWriter {
    pub fn new(state_dir: &Path) -> std::io::Result<Self> {
        create_dir_all(state_dir)?;
        Ok(Self {
            path: state_dir.join("screencast-frames.jsonl"),
        })
    }

    pub fn write(&self, frame_sha256: &str, session_id: &str) -> std::io::Result<()> {
        let rec = ScreencastFrameReceipt {
            frame_sha256: frame_sha256.to_string(),
            captured_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            session_id: session_id.to_string(),
        };
        let line = serde_json::to_string(&rec)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        glass_ledger::lock_ledger_file(&file, &self.path)?;
        let line = format!("{line}\n");
        if file.write(line.as_bytes())? != line.len() {
            return Err(std::io::Error::new(std::io::ErrorKind::WriteZero, "short record write"));
        }
        file.flush()?;
        Ok(())
    }
}
