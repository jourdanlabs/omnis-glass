//! CDP WebSocket capability token (v1.1 pane security).

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use subtle::ConstantTimeEq;

pub fn generate_cdp_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Shipped default: auth required (fail-closed). Opt-out only via `OMNIS_GLASS_CDP_AUTH=0`.
pub fn cdp_auth_required() -> bool {
    std::env::var("OMNIS_GLASS_CDP_AUTH")
        .map(|v| v != "0" && v.to_lowercase() != "false")
        .unwrap_or(true)
}

fn constant_time_eq_str(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

/// Extract token from `GET /devtools/browser/{token}` or legacy path.
pub fn token_from_request_head(head: &str, expected: &str) -> bool {
    if !cdp_auth_required() {
        return true;
    }
    for line in head.lines() {
        let line = line.trim();
        if !line.starts_with("GET ") {
            continue;
        }
        let path = line.split_whitespace().nth(1).unwrap_or("");
        if path == "/devtools/browser" || path.starts_with("/devtools/browser?") {
            return false;
        }
        let prefix = "/devtools/browser/";
        if let Some(rest) = path.strip_prefix(prefix) {
            let tok = rest.split(['?', '/']).next().unwrap_or("");
            return !tok.is_empty() && constant_time_eq_str(tok, expected);
        }
        return false;
    }
    false
}

pub fn debugger_url(host: &str, port: u16, token: &str) -> String {
    if cdp_auth_required() {
        format!("ws://{host}:{port}/devtools/browser/{token}")
    } else {
        format!("ws://{host}:{port}/devtools/browser")
    }
}

/// WebSocket URL for tests/clients — uses `OMNIS_GLASS_CDP_TOKEN` when auth is on.
pub fn browser_ws_url(host: &str, port: u16) -> String {
    if cdp_auth_required() {
        let token = std::env::var("OMNIS_GLASS_CDP_TOKEN").unwrap_or_default();
        debugger_url(host, port, &token)
    } else {
        format!("ws://{host}:{port}/devtools/browser")
    }
}
