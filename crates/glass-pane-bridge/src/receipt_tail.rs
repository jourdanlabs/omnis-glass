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

pub const DIFFERENT_STATE_MESSAGE: &str = "Actions not shown: this pane reads a different state folder than the server. Start it with --state-dir.";

pub fn actions_ledger_path_hash(state_dir: &Path) -> Option<String> {
    let absolute_dir = if state_dir.is_absolute() { state_dir.to_path_buf() } else { std::env::current_dir().ok()?.join(state_dir) };
    let state_dir = absolute_dir.as_path();
    let ledger = state_dir.join("browser.actions.jsonl");
    let canonical = match std::fs::canonicalize(&ledger) {
        Ok(path) => path,
        Err(_) => {
            let mut parent = state_dir;
            let mut missing = Vec::new();
            let mut existing = loop {
                if let Ok(path) = std::fs::canonicalize(parent) { break path; }
                missing.push(parent.file_name()?.to_os_string());
                parent = parent.parent()?;
            };
            for component in missing.iter().rev() { existing.push(component); }
            existing.join("browser.actions.jsonl")
        }
    };
    use sha2::Digest;
    Some(hex::encode(sha2::Sha256::digest(canonical.as_os_str().as_encoded_bytes())))
}

pub struct ReceiptTailer {
    actions_path: PathBuf,
    captures_path: PathBuf,
    actions_offset: u64,
    different_state: bool,
}

impl ReceiptTailer {
    pub fn new(state_dir: &Path) -> Self {
        Self {
            actions_path: state_dir.join("browser.actions.jsonl"),
            captures_path: state_dir.join("web.captures.jsonl"),
            actions_offset: 0,
            different_state: false,
        }
    }

    pub fn for_attach(state_dir: &Path, server_hash: Option<&str>) -> Self {
        let mut tailer = Self::new(state_dir);
        let local_hash = actions_ledger_path_hash(state_dir);
        tailer.different_state = !matches!((local_hash.as_deref(), server_hash), (Some(local), Some(server)) if local == server);
        tailer
    }

    pub fn warning(&self) -> Option<&'static str> { self.different_state.then_some(DIFFERENT_STATE_MESSAGE) }

    pub fn poll_actions(&mut self) -> std::io::Result<Vec<ActionRailEntry>> {
        if self.different_state { return Ok(Vec::new()); }
        if !self.actions_path.exists() {
            return Ok(Vec::new());
        }
        let mut file = File::open(&self.actions_path)?;
        file.seek(SeekFrom::Start(self.actions_offset))?;
        let mut reader = BufReader::new(file);
        let mut out = Vec::new();
        loop {
            let mut line = String::new();
            let bytes = reader.read_line(&mut line)?;
            if bytes == 0 || !line.ends_with('\n') {
                break;
            }
            self.actions_offset += bytes as u64;
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
        if self.different_state { return None; }
        if !self.captures_path.exists() {
            return None;
        }
        let file = File::open(&self.captures_path).ok()?;
        let mut reader = BufReader::new(file);
        let mut latest = None;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).ok()? == 0 || !line.ends_with('\n') { break; }
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                if let Some(hash) = value.get("content_sha256").and_then(Value::as_str) {
                    latest = Some(hash.to_string());
                }
            }
        }
        latest
    }
}
