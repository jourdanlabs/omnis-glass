use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use glass_capture::{SsrfCheck, SsrfVerdict};
use glass_ledger::{digest_sha256, BrowserActionWriter};
use glass_terminus::{target_from_url, BrowserAction, OutboundEnvelope, TerminusGate};
use serde_json::{json, Map, Value};

use crate::session::{self, Driver};

static GATE: OnceLock<Mutex<TerminusGate>> = OnceLock::new();
static SESSION_URL: OnceLock<Mutex<std::collections::HashMap<String, String>>> = OnceLock::new();

pub fn state_dir() -> PathBuf {
    std::env::var("OMNIS_GLASS_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".omnis-glass")
        })
}

fn gate() -> &'static Mutex<TerminusGate> {
    GATE.get_or_init(|| Mutex::new(TerminusGate::new(&state_dir())))
}

fn session_urls() -> &'static Mutex<std::collections::HashMap<String, String>> {
    SESSION_URL.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

pub fn session_id_key(session_id: &str) -> String {
    if session_id.is_empty() {
        "default".into()
    } else {
        session_id.into()
    }
}

fn current_page_url(session_id: &str) -> String {
    session_urls()
        .lock()
        .ok()
        .and_then(|m| m.get(session_id).cloned())
        .unwrap_or_else(|| "about:blank".into())
}

pub fn record_navigation(session_id: &str, url: &str) {
    if let Ok(mut m) = session_urls().lock() {
        m.insert(session_id_key(session_id), url.to_string());
    }
}

pub fn governance_enabled() -> bool {
    true
}

/// Core gate — returns `Err(message)` when action must not proceed.
pub fn gate_browser_action(
    action: BrowserAction,
    url: &str,
    payload: Option<Value>,
    session_id: &str,
) -> Result<(), String> {
    if session_id != "pane-human" && session::agent_blocked() {
        return Err("TERMINUS REFUSE (session_locked): REFUSE_SESSION_LOCKED".into());
    }

    let driver = if session_id == "pane-human" {
        Driver::Human
    } else {
        session::current_driver()
    };

    let target = target_from_url(url).map_err(|e| format!("TERMINUS REFUSED (invalid_target): {e}"))?;

    let gate = gate().lock().map_err(|_| "TERMINUS REFUSED (gate_lock)".to_string())?;
    let lane = if session_id == "pane-human" {
        std::env::var("OMNIS_GLASS_LANE_HUMAN").unwrap_or_else(|_| "browse:human".into())
    } else {
        gate.lane.clone()
    };
    let envelope = OutboundEnvelope::browser(
        action,
        target,
        payload.clone(),
        &lane,
        &gate.policy.version,
        &gate.policy.coverage_class,
        &session_id_key(session_id),
    );

    session::set_inflight(action.as_str());
    let verdict = gate.authorize(&envelope);
    let mut fields = Map::new();
    fields.insert("url".into(), json!(url));
    if let Some(p) = payload {
        if let Some(v) = p.get("field_value").and_then(|x| x.as_str()) {
            fields.insert("field_value".into(), json!(v));
        }
        if let Some(v) = p.get("post_body").and_then(|x| x.as_str()) {
            fields.insert("post_body".into(), json!(v));
        }
        if let Some(v) = p.get("script").and_then(|x| x.as_str()) {
            fields.insert("script".into(), json!(v));
        }
        if let Some(v) = p.get("payload").and_then(|x| x.as_str()) {
            fields.insert("field_value".into(), json!(v));
        }
        if let Some(v) = p.get("uploads") {
            fields.insert("uploads".into(), v.clone());
        }
    }
    fields.insert("action".into(), json!(action.as_str()));
    fields.insert("lane".into(), json!(lane));
    fields.insert("verdict".into(), json!(verdict.verdict));
    let driver_str = match driver {
        Driver::Human => "human",
        Driver::Agent => "agent",
    };
    if let Ok(writer) = BrowserActionWriter::new(&state_dir()) {
        let _ = writer.write_action_with_meta(
            action.as_str(),
            &lane,
            &verdict.verdict,
            fields,
            Some(driver_str),
            None,
        );
    }
    session::clear_inflight();

    if verdict.verdict != "ALLOW" {
        return Err(format!(
            "TERMINUS {} ({}): {}",
            verdict.verdict, verdict.reason, verdict.decision
        ));
    }
    Ok(())
}

/// Human pane navigate — always uses pane-human session + human lane.
pub fn gate_pane_navigate(url: &str) -> Result<(), String> {
    if !matches!(session::current_mode(), session::PaneMode::Human) {
        return Err("TERMINUS REFUSE (mirror_readonly): pane is in mirror mode".into());
    }
    gate_browser_action(BrowserAction::Navigate, url, None, "pane-human")
}

/// Record cancellation receipt on handoff (Fix 3).
pub fn record_handoff_cancel(action: &str) {
    let lane = std::env::var("OMNIS_GLASS_LANE").unwrap_or_else(|_| "browse:research".into());
    let mut fields = Map::new();
    fields.insert("action".into(), json!(action));
    fields.insert("verdict".into(), json!("CANCELLED"));
    if let Ok(writer) = BrowserActionWriter::new(&state_dir()) {
        let _ = writer.write_action_with_meta(
            action,
            &lane,
            "CANCELLED",
            fields,
            Some("agent"),
            Some("cancelled_by_handoff"),
        );
    }
}

pub fn gate_cdp_command(method: &str, params: &Value, session_id: &Option<String>) -> Option<String> {
    let sid = session_id
        .as_ref()
        .map(|s| s.as_str())
        .unwrap_or("default");

    let err = match method {
        "Page.navigate" => {
            let url = params.get("url")?.as_str()?;
            gate_browser_action(BrowserAction::Navigate, url, None, sid).err()
        }
        "Input.insertText" => {
            let text = params.get("text")?.as_str()?;
            gate_browser_action(
                BrowserAction::Type,
                &current_page_url(sid),
                Some(json!({"field_value": text})),
                sid,
            )
            .err()
        }
        "Input.dispatchKeyEvent" => {
            let text = params.get("text").and_then(|t| t.as_str()).unwrap_or("");
            if text.is_empty() {
                return None;
            }
            gate_browser_action(
                BrowserAction::Type,
                &current_page_url(sid),
                Some(json!({"field_value": text})),
                sid,
            )
            .err()
        }
        "Runtime.evaluate" => {
            let expr = params
                .get("expression")
                .or_else(|| params.get("script"))
                .and_then(|e| e.as_str())
                .unwrap_or("");
            gate_browser_action(
                BrowserAction::Eval,
                &current_page_url(sid),
                Some(json!({"script": expr})),
                sid,
            )
            .err()
        }
        "Runtime.callFunctionOn" => {
            let func = params
                .get("functionDeclaration")
                .and_then(|e| e.as_str())
                .unwrap_or("");
            gate_browser_action(
                BrowserAction::Eval,
                &current_page_url(sid),
                Some(json!({"script": func})),
                sid,
            )
            .err()
        }
        "DOM.setFileInputFiles" => gate_upload(params, sid).err(),
        "Fetch.continueRequest" => {
            if let Some(body) = params.get("postData").and_then(|p| p.as_str()) {
                let url_owned = params
                    .get("url")
                    .and_then(|u| u.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| current_page_url(sid));
                gate_browser_action(
                    BrowserAction::Submit,
                    &url_owned,
                    Some(json!({"post_body": body})),
                    sid,
                )
                .err()
            } else {
                None
            }
        }
        "Storage.setCookies" | "Network.setCookies" => {
            gate_storage_cookies(params, sid).err()
        }
        "Storage.clearCookies" | "Storage.deleteCookies" => gate_browser_action(
            BrowserAction::StorageWrite,
            &current_page_url(sid),
            Some(json!({"kind": "clear_cookies"})),
            sid,
        )
        .err(),
        _ => None,
    };
    err
}

fn gate_upload(params: &Value, session_id: &str) -> Result<(), String> {
    let paths: Vec<String> = params
        .get("files")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let mut digests = Vec::new();
    let mut scan_parts = Vec::new();
    for p in &paths {
        let bytes = std::fs::read(p).map_err(|e| format!("TERMINUS REFUSED (upload_read): {e}"))?;
        let text = String::from_utf8_lossy(&bytes);
        digests.push(json!({"path": p, "content_sha256": digest_sha256(&text)}));
        scan_parts.push(text.to_string());
    }
    gate_browser_action(
        BrowserAction::Upload,
        &current_page_url(session_id),
        Some(json!({"uploads": digests, "payload": scan_parts.join("\n")})),
        session_id,
    )
}

pub fn gate_storage_cookies(params: &Value, session_id: &str) -> Result<(), String> {
    let digest = digest_sha256(&params.to_string());
    gate_browser_action(
        BrowserAction::StorageWrite,
        &current_page_url(session_id),
        Some(json!({"storage_kind": "cookies", "params_sha256": digest})),
        session_id,
    )
}

pub fn check_post_navigate_redirect(from: &str, to: &str) -> Option<String> {
    if from == to {
        return None;
    }
    gate_browser_action(BrowserAction::Navigate, to, None, "cdp").err().or_else(|| {
        let ssrf = SsrfCheck::default();
        match ssrf.check_redirect(from, to) {
            SsrfVerdict::Allow => None,
            SsrfVerdict::Refuse(r) => Some(format!("TERMINUS REFUSED (redirect_ssrf): {r}")),
        }
    })
}
