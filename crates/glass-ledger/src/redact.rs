use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub fn digest_sha256(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    hex::encode(hasher.finalize())
}

/// Strip raw values; keep digests and metadata only.
pub fn redact_action_fields(
    action: &str,
    lane: &str,
    verdict: &str,
    mut fields: Map<String, Value>,
) -> Map<String, Value> {
    // Never allow these keys through — fail-closed redaction
    for forbidden in [
        "value",
        "raw_value",
        "password",
        "cookie",
        "authorization",
        "set_cookie",
        "session_token",
        "file_bytes",
        "body",
        "post_body",
    ] {
        fields.remove(forbidden);
    }

    // Hash any remaining string payload fields named *_value or script/result
    for key in ["script", "result", "payload"].iter() {
        if let Some(Value::String(s)) = fields.get(*key).cloned() {
            fields.remove(*key);
            fields.insert(format!("{key}_sha256"), Value::String(digest_sha256(&s)));
        }
    }

    if let Some(Value::String(v)) = fields.remove("field_value") {
        fields.insert("value_sha256".into(), Value::String(digest_sha256(&v)));
    }

    fields.insert("action".into(), Value::String(action.into()));
    fields.insert("lane".into(), Value::String(lane.into()));
    fields.insert("verdict".into(), Value::String(verdict.into()));
    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strips_raw_secrets() {
        let mut fields = Map::new();
        fields.insert("field_value".into(), json!("sk-abcdefghijklmnopqrstuvwxyz1234"));
        fields.insert("cookie".into(), json!("session=abc"));
        let out = redact_action_fields("type", "browse:research", "ALLOW", fields);
        assert!(out.get("field_value").is_none());
        assert!(out.get("cookie").is_none());
        assert!(out.get("value_sha256").is_some());
    }
}
