use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use glass_capture::{SsrfCheck, SsrfVerdict};
use glass_ledger::{digest_sha256, BrowserActionWriter};
use glass_terminus::{target_from_url, BrowserAction, EnforceMode, OutboundEnvelope, TerminusGate};
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

fn normalized_session_id(session_id: &str) -> &str {
    if session_id.is_empty() { "default" } else { session_id }
}

pub fn session_id_key(session_id: &str) -> String {
    let session_id = normalized_session_id(session_id);
    match session::command_context().and_then(|context| context.session_namespace) {
        Some(namespace) => format!("{namespace}/{session_id}"),
        None => session_id.into(),
    }
}

fn current_page_url(session_id: &str) -> String {
    let key = session_id_key(session_id);
    session_urls().lock().ok().and_then(|m| m.get(&key).cloned())
        .unwrap_or_else(|| "about:blank".into())
}

pub fn record_navigation(session_id: &str, url: &str) {
    if let Ok(mut m) = session_urls().lock() { m.insert(session_id_key(session_id), url.to_string()); }
}

/// Update a connection-owned session outside its command scope.
pub fn record_connection_navigation(namespace: &str, session_id: &str, url: &str) {
    if let Ok(mut m) = session_urls().lock() {
        m.insert(format!("{namespace}/{}", normalized_session_id(session_id)), url.to_string());
    }
}

pub fn forget_connection_session(namespace: &str, session_id: &str) {
    if let Ok(mut m) = session_urls().lock() {
        m.remove(&format!("{namespace}/{}", normalized_session_id(session_id)));
    }
}

pub fn forget_connection_navigation(namespace: &str) {
    let prefix = format!("{namespace}/");
    if let Ok(mut m) = session_urls().lock() { m.retain(|key, _| !key.starts_with(&prefix)); }
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
    gate_browser_action_with_context(
        action,
        url,
        payload,
        session_id,
        GateNetworkContext::with_allow_private(session::command_context().map(|c| c.allow_private_network).unwrap_or(false)),
    )
}

/// Per-request gate options for HTTP/JS fetch paths.
#[derive(Debug, Clone, Copy, Default)]
pub struct GateNetworkContext {
    pub allow_private_network: bool,
    pub enforce_override: Option<EnforceMode>,
}

impl GateNetworkContext {
    pub fn strict() -> Self {
        Self::default()
    }

    /// SSRF opt-in only — does not alter lane/host/TERMINUS enforcement.
    pub fn with_allow_private(allow_private_network: bool) -> Self {
        Self {
            allow_private_network,
            enforce_override: None,
        }
    }

    /// Integration-test fixture hook — not used by production call sites.
    #[doc(hidden)]
    pub fn test_fixture(allow_private_network: bool, enforce_override: EnforceMode) -> Self {
        Self {
            allow_private_network,
            enforce_override: Some(enforce_override),
        }
    }
}

/// Like [`gate_browser_action`] but honors per-request network context
/// (e.g. JS fetch via `ObscuraHttpClient.allow_private_network`).
pub fn gate_browser_action_with_network(
    action: BrowserAction,
    url: &str,
    payload: Option<Value>,
    session_id: &str,
    allow_private_network: bool,
) -> Result<(), String> {
    gate_browser_action_with_context(
        action,
        url,
        payload,
        session_id,
        GateNetworkContext::with_allow_private(allow_private_network),
    )
}

pub fn gate_browser_action_with_context(
    action: BrowserAction,
    url: &str,
    payload: Option<Value>,
    session_id: &str,
    ctx: GateNetworkContext,
) -> Result<(), String> {
    gate_browser_action_with_receipt_context(action, url, payload, session_id, ctx, false)
}

fn gate_browser_action_with_receipt_context(
    action: BrowserAction,
    url: &str,
    payload: Option<Value>,
    session_id: &str,
    ctx: GateNetworkContext,
    key_event: bool,
) -> Result<(), String> {
    let command = session::command_context();
    let is_human = command.as_ref().map(|c| c.driver == Driver::Human).unwrap_or(false);
    if !is_human && session::agent_blocked() {
        return Err(record_session_locked_refusal(action.as_str(), url));
    }

    let driver = command.as_ref().map(|c| c.driver).unwrap_or(Driver::Agent);

    let target = target_from_url(url).map_err(|e| format!("TERMINUS REFUSED (invalid_target): {e}"))?;

    let gate = gate().lock().map_err(|_| "TERMINUS REFUSED (gate_lock)".to_string())?;
    let lane = command.and_then(|c| c.lane).unwrap_or_else(|| gate.lane.clone());
    let mut envelope = OutboundEnvelope::browser(
        action,
        target,
        payload.clone(),
        &lane,
        &gate.policy.version,
        &gate.policy.coverage_class,
        &session_id_key(session_id),
    );

    if key_event { envelope.key_event = Some(true); }
    session::set_inflight(action.as_str());
    let verdict = gate.authorize_with_network(
        &envelope,
        ctx.allow_private_network,
        ctx.enforce_override,
    );
    // Never hold the process-wide policy lock while waiting on a ledger file.
    drop(gate);
    let mut fields = Map::new();
    fields.insert("url".into(), json!(url));
    if let Some(p) = payload {
        if let Some(v) = p.get("field_value").and_then(|x| x.as_str()) {
            if key_event {
                fields.insert("chars".into(), json!(v.chars().count()));
            } else {
                fields.insert("field_value".into(), json!(v));
            }
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
    let receipt = BrowserActionWriter::new(&state_dir()).and_then(|writer| {
        writer.write_action_with_meta(action.as_str(), &lane, &verdict.verdict, fields, Some(driver_str), None)
    });
    if let Err(error) = &receipt { eprintln!("browser action receipt write failed: {error}"); }
    session::clear_inflight();

    if verdict.verdict != "ALLOW" {
        return Err(format!(
            "TERMINUS {} ({}): {}",
            verdict.verdict, verdict.reason, verdict.decision
        ));
    }
    if receipt.is_err() { return Err("TERMINUS REFUSE (ledger_unavailable): REFUSE_LEDGER_UNAVAILABLE".into()); }
    Ok(())
}

/// Record a lock refusal already decided atomically by the session state.
/// This must not re-authorize the action if control changes before the write.
pub fn record_session_locked_refusal(action: &str, url: &str) -> String {
    session::mark_agent_waiting();
    let lane = session::command_context().and_then(|c| c.lane).unwrap_or_else(|| std::env::var("OMNIS_GLASS_LANE").unwrap_or_else(|_| "browse:research".into()));
    let mut fields = Map::new();
    fields.insert("url".into(), json!(url));
    fields.insert("reason".into(), json!("REFUSE_SESSION_LOCKED"));
    if let Err(error) = BrowserActionWriter::new(&state_dir()).and_then(|writer| writer.write_action_with_meta(action, &lane, "REFUSE", fields, Some("agent"), None)) {
        eprintln!("browser action refusal receipt write failed: {error}");
        return "TERMINUS REFUSE (ledger_unavailable): REFUSE_LEDGER_UNAVAILABLE".into();
    }
    "TERMINUS REFUSE (session_locked): REFUSE_SESSION_LOCKED".into()
}

/// Record cancellation receipt on handoff (Fix 3).
pub fn record_handoff_cancel(action: &str) {
    let lane = std::env::var("OMNIS_GLASS_LANE").unwrap_or_else(|_| "browse:research".into());
    let mut fields = Map::new();
    fields.insert("action".into(), json!(action));
    fields.insert("verdict".into(), json!("CANCELLED"));
    if let Err(error) = BrowserActionWriter::new(&state_dir()).and_then(|writer| {
        writer.write_action_with_meta(action, &lane, "CANCELLED", fields, Some("agent"), Some("cancelled_by_handoff"))
    }) { eprintln!("handoff cancellation receipt write failed: {error}"); }

}

/// Authorize a CDP navigation and hand its initial GET to the outbound gate once.
pub fn gate_approved_navigation(url: &str, session_id: &str) -> Result<(), String> {
    gate_browser_action(BrowserAction::Navigate, url, None, session_id)?;
    session::approve_http_navigation(url);
    Ok(())
}

pub fn gate_cdp_command(method: &str, params: &Value, session_id: &Option<String>) -> Option<String> {
    let sid = session_id
        .as_ref()
        .map(|s| s.as_str())
        .unwrap_or("default");

    let err = match method {
        "Target.createTarget" => {
            let url = params.get("url").and_then(|v| v.as_str()).unwrap_or("about:blank");
            if url.is_empty() || url == "about:blank" { None }
            else { gate_approved_navigation(url, sid).err() }
        }
        "Page.navigate" => {
            let url = params.get("url")?.as_str()?;
            gate_approved_navigation(url, sid).err()
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
            gate_browser_action_with_receipt_context(
                BrowserAction::Type,
                &current_page_url(sid),
                Some(json!({"field_value": text})),
                sid,
                GateNetworkContext::with_allow_private(session::command_context().map(|c| c.allow_private_network).unwrap_or(false)),
                true,
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
