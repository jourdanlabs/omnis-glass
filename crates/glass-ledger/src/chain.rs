use std::fs::{create_dir_all, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::redact::{digest_sha256, redact_action_fields};
use crate::sign::Signer;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainVerifyResult {
    pub ok: bool,
    pub count: usize,
    pub first_break_index: Option<usize>,
    pub reason: Option<String>,
}

pub struct BrowserActionWriter {
    ledger_path: PathBuf,
    signer: Signer,
}

impl BrowserActionWriter {
    pub fn new(state_dir: &Path) -> std::io::Result<Self> {
        create_dir_all(state_dir)?;
        let ledger_path = state_dir.join("browser.actions.jsonl");
        let key_path = state_dir.join("writer.key");
        // Lock the ledger during key initialization as well as appends.
        let file = OpenOptions::new().create(true).append(true).read(true).open(&ledger_path)?;
        crate::lock_ledger_file(&file, &ledger_path)?;
        let signer = crate::sign::load_or_create_signer(&key_path)?;
        Ok(Self {
            ledger_path,
            signer,
        })
    }

    fn chain_head(file: &mut std::fs::File, path: &Path) -> std::io::Result<Option<String>> {
        let mut end = file.seek(SeekFrom::End(0))?;
        if end == 0 { return Ok(None); }
        let mut byte = [0u8; 1];
        file.seek(SeekFrom::Start(end - 1))?;
        file.read_exact(&mut byte)?;
        let damaged = || std::io::Error::new(std::io::ErrorKind::InvalidData,
            format!("ledger's last record is damaged: {}", path.display()));
        if byte[0] != b'\n' { return Err(damaged()); }
        end -= 1;
        let mut start = end;
        let mut chunk = [0u8; 4096];
        while start > 0 {
            let len = start.min(chunk.len() as u64) as usize;
            start -= len as u64;
            file.seek(SeekFrom::Start(start))?;
            file.read_exact(&mut chunk[..len])?;
            if let Some(pos) = chunk[..len].iter().rposition(|&b| b == b'\n') {
                start += pos as u64 + 1;
                break;
            }
        }
        file.seek(SeekFrom::Start(start))?;
        let mut line = vec![0; (end - start) as usize];
        file.read_exact(&mut line)?;
        let value: Value = serde_json::from_slice(&line).map_err(|_| damaged())?;
        let sig = value.get("sig").and_then(|s| s.as_str()).filter(|s| !s.is_empty()).ok_or_else(damaged)?;
        Ok(Some(sig.to_string()))
    }

    /// Append a redacted `browser.action` receipt. Raw secrets must never appear in `fields`.
    pub fn write_action(
        &self,
        action: &str,
        lane: &str,
        verdict: &str,
        fields: Map<String, Value>,
    ) -> std::io::Result<Value> {
        self.write_action_with_meta(action, lane, verdict, fields, None, None)
    }

    pub fn write_action_with_meta(
        &self,
        action: &str,
        lane: &str,
        verdict: &str,
        mut fields: Map<String, Value>,
        driver: Option<&str>,
        outcome: Option<&str>,
    ) -> std::io::Result<Value> {
        if let Some(d) = driver {
            fields.insert("driver".into(), Value::String(d.to_string()));
        }
        if let Some(o) = outcome {
            fields.insert("outcome".into(), Value::String(o.to_string()));
        }
        let redacted = redact_action_fields(action, lane, verdict, fields);
        let mut file = OpenOptions::new().create(true).append(true).read(true).open(&self.ledger_path)?;
        crate::lock_ledger_file(&file, &self.ledger_path)?;
        let prev = Self::chain_head(&mut file, &self.ledger_path)?;
        let mut body = Map::new();
        body.insert("receipt".into(), Value::String("browser.action".into()));
        body.insert("canon_version".into(), Value::Number(3.into()));
        body.insert(
            "ts".into(),
            Value::String(Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        );
        for (k, v) in redacted {
            body.insert(k, v);
        }
        if let Some(p) = prev {
            body.insert("prev".into(), Value::String(p));
        }
        body.insert(
            "key_id".into(),
            Value::String(self.signer.key_id.clone()),
        );

        let canonical = crate::chain::canon(&Value::Object(body.clone()));
        let sig = digest_sha256(&canonical);
        let signature = self.signer.sign(&canonical);

        let mut rec = body;
        rec.insert("sig".into(), Value::String(sig));
        rec.insert("signature".into(), Value::String(signature));

        let line = serde_json::to_string(&Value::Object(rec))?;
        let line = format!("{line}\n");
        if file.write(line.as_bytes())? != line.len() {
            return Err(std::io::Error::new(std::io::ErrorKind::WriteZero, "short record write"));
        }
        file.flush()?;
        Ok(serde_json::from_str(&line).unwrap_or(Value::Null))
    }
}

pub fn verify_chain(ledger_path: &Path, public_pem: Option<&str>) -> ChainVerifyResult {
    if !ledger_path.exists() {
        return ChainVerifyResult {
            ok: true,
            count: 0,
            first_break_index: None,
            reason: None,
        };
    }

    let file = match std::fs::File::open(ledger_path) {
        Ok(f) => f,
        Err(e) => {
            return ChainVerifyResult {
                ok: false,
                count: 0,
                first_break_index: Some(0),
                reason: Some(format!("open: {e}")),
            }
        }
    };

    let lines: Vec<String> = BufReader::new(file)
        .lines()
        .filter_map(|l| l.ok())
        .filter(|l| !l.trim().is_empty())
        .collect();

    let mut expected_prev: Option<String> = None;
    for (i, line) in lines.iter().enumerate() {
        let mut rec: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                return ChainVerifyResult {
                    ok: false,
                    count: lines.len(),
                    first_break_index: Some(i),
                    reason: Some(format!("json: {e}")),
                };
            }
        };

        let stored_sig = rec
            .get("sig")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let stored_prev = rec.get("prev").and_then(|s| s.as_str()).map(str::to_string);
        let stored_signature = rec
            .get("signature")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();

        if stored_prev != expected_prev {
            return ChainVerifyResult {
                ok: false,
                count: lines.len(),
                first_break_index: Some(i),
                reason: Some(format!(
                    "prev mismatch at {i}: expected {:?}, got {:?}",
                    expected_prev, stored_prev
                )),
            };
        }

        rec.as_object_mut().map(|o| {
            o.remove("sig");
            o.remove("signature");
        });
        let canonical = canon(&rec);
        let computed_sig = digest_sha256(&canonical);
        if computed_sig != stored_sig {
            return ChainVerifyResult {
                ok: false,
                count: lines.len(),
                first_break_index: Some(i),
                reason: Some(format!("sig mismatch at {i}")),
            };
        }

        if let Some(pem) = public_pem {
            if !crate::sign::verify_signature(pem, &canonical, &stored_signature) {
                return ChainVerifyResult {
                    ok: false,
                    count: lines.len(),
                    first_break_index: Some(i),
                    reason: Some(format!("signature invalid at {i}")),
                };
            }
        }

        expected_prev = Some(stored_sig);
    }

    ChainVerifyResult {
        ok: true,
        count: lines.len(),
        first_break_index: None,
        reason: None,
    }
}

pub fn canon(value: &Value) -> String {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            serde_json::to_string(value).unwrap_or_default()
        }
        Value::Array(arr) => {
            let parts: Vec<String> = arr.iter().map(canon).collect();
            format!("[{}]", parts.join(","))
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .iter()
                .map(|k| format!("{}:{}", serde_json::to_string(k).unwrap(), canon(&map[*k])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn trailing_space_cannot_replace_the_final_newline() {
        let dir = tempdir().unwrap();
        let writer = BrowserActionWriter::new(dir.path()).unwrap();
        writer.write_action("navigate", "browse:research", "ALLOW", Map::new()).unwrap();
        let path = dir.path().join("browser.actions.jsonl");
        let original = std::fs::read(&path).unwrap();
        assert_eq!(original.last(), Some(&b'\n'));
        let mut damaged = original.clone(); *damaged.last_mut().unwrap() = b' ';
        std::fs::write(&path, &damaged).unwrap();
        assert!(writer.write_action("navigate", "browse:research", "ALLOW", Map::new()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), damaged);
        std::fs::write(&path, original).unwrap();
        writer.write_action("navigate", "browse:research", "ALLOW", Map::new()).unwrap();
        let verified = verify_chain(&path, None); assert!(verified.ok); assert_eq!(verified.count, 2);
    }

    #[test]
    fn chain_links_and_tamper_detected() {
        let dir = tempdir().unwrap();
        let writer = BrowserActionWriter::new(dir.path()).unwrap();
        let mut fields = Map::new();
        fields.insert("url".into(), json!("https://example.com"));
        writer
            .write_action("navigate", "browse:research", "ALLOW", fields)
            .unwrap();

        let path = dir.path().join("browser.actions.jsonl");
        let result = verify_chain(&path, None);
        assert!(result.ok);
        assert_eq!(result.count, 1);

        // Tamper
        let content = std::fs::read_to_string(&path).unwrap();
        let tampered = content.replace("example.com", "evil.com");
        std::fs::write(&path, tampered).unwrap();
        let broken = verify_chain(&path, None);
        assert!(!broken.ok);
        assert!(broken.first_break_index.is_some());
    }
}
