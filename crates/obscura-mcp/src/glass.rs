//! OMNIS GLASS MCP tool governance.

use glass_govern::{emit_live_capture, gate_browser_action};
use glass_ledger::digest_sha256;
use glass_terminus::BrowserAction;
use serde_json::{json, Value};

const MCP_SESSION: &str = "mcp";

pub fn gate_mcp_navigate(url: &str) -> Result<(), String> {
    gate_browser_action(BrowserAction::Navigate, url, None, MCP_SESSION)
}

pub fn gate_mcp_type(value: &str, page_url: &str) -> Result<(), String> {
    gate_browser_action(
        BrowserAction::Type,
        page_url,
        Some(json!({"field_value": value})),
        MCP_SESSION,
    )
}

pub fn gate_mcp_eval(script: &str, page_url: &str) -> Result<(), String> {
    gate_browser_action(
        BrowserAction::Eval,
        page_url,
        Some(json!({"script": script})),
        MCP_SESSION,
    )
}

pub fn gate_mcp_storage(page_url: &str) -> Result<(), String> {
    gate_browser_action(
        BrowserAction::StorageWrite,
        page_url,
        Some(json!({"storage_kind": "mcp_set"})),
        MCP_SESSION,
    )
}

pub fn gate_mcp_click(page_url: &str) -> Result<(), String> {
    gate_browser_action(BrowserAction::ClickSubmit, page_url, None, MCP_SESSION)
}

pub fn wrap_text_result(text: &str, page_url: &str) -> Value {
    let content_sha256 = digest_sha256(text);
    emit_live_capture(page_url, page_url, text.as_bytes(), 200, None);
    json!({
        "content": [{ "type": "text", "text": text }],
        "provenance": {
            "url": page_url,
            "content_sha256": content_sha256,
            "captured_at": chrono_like_now(),
            "capture_kind": "live"
        }
    })
}

pub fn wrap_error(text: String) -> Value {
    json!({
        "content": [{ "type": "text", "text": format!("Error: {text}") }],
        "isError": true
    })
}

fn chrono_like_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{ms}")
}
