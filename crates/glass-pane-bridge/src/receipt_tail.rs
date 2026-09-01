//! Tail browser.action + web.capture ledgers for the action rail.

use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ActionRailEntry {
    pub ts: String,
    pub action: String,
    pub verdict: String,
    pub url: String,
    pub driver: Option<String>,
    pub outcome: Option<String>,
}

pub struct ReceiptTailer {
    actions_path: PathBuf,
    captures_path: PathBuf,
    actions_offset: u64,
}

impl ReceiptTailer {
    pub fn new(state_dir: &Path) -> Self {
        Self {
            actions_path: state_dir.join("browser.actions.jsonl"),
            captures_path: state_dir.join("web.captures.jsonl"),
            actions_offset: 0,
        }
    }

    pub fn poll_actions(&mut self) -> std::io::Result<Vec<ActionRailEntry>> {
        if !self.actions_path.exists() {
            return Ok(Vec::new());
        }
        let mut file = File::open(&self.actions_path)?;
        file.seek(SeekFrom::Start(self.actions_offset))?;
        let reader = BufReader::new(file);
        let mut out = Vec::new();
        for line in reader.lines() {
            let line = line?;
            self.actions_offset += line.len() as u64 + 1;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                out.push(ActionRailEntry {
                    ts: v.get("ts").and_then(|x| x.as_str()).unwrap_or("").into(),
                    action: v.get("action").and_then(|x| x.as_str()).unwrap_or("").into(),
                    verdict: v.get("verdict").and_then(|x| x.as_str()).unwrap_or("").into(),
                    url: v.get("url").and_then(|x| x.as_str()).unwrap_or("").into(),
                    driver: v.get("driver").and_then(|x| x.as_str()).map(str::to_string),
                    outcome: v.get("outcome").and_then(|x| x.as_str()).map(str::to_string),
                });
            }
        }
        Ok(out)
    }

    pub fn last_capture_sha(&self) -> Option<String> {
        if !self.captures_path.exists() {
            return None;
        }
        let file = File::open(&self.captures_path).ok()?;
        let last = BufReader::new(file).lines().filter_map(|l| l.ok()).last()?;
        serde_json::from_str::<Value>(&last)
            .ok()
            .and_then(|v| v.get("content_sha256").and_then(|x| x.as_str()).map(str::to_string))
    }
}
