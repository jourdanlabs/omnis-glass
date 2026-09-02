use glass_terminus::BrowserAction;
use serde_json::json;
use url::Url;

use crate::gate::{gate_browser_action_with_context, GateNetworkContext};

#[derive(Debug, Clone, Copy)]
pub enum HttpGateAction {
    ScriptFetch,
    DumpOriginal,
    Navigate,
}

impl HttpGateAction {
    fn browser_action(self) -> BrowserAction {
        match self {
            Self::ScriptFetch => BrowserAction::ScriptFetch,
            Self::DumpOriginal => BrowserAction::Navigate,
            Self::Navigate => BrowserAction::Navigate,
        }
    }

    fn action_label(self) -> &'static str {
        match self {
            Self::ScriptFetch => "script-fetch",
            Self::DumpOriginal => "dump-original",
            Self::Navigate => "navigate",
        }
    }
}

/// Gate outbound HTTP before socket opens. Returns Err if REFUSE/HOLD.
pub fn gate_http_outbound(
    url: &Url,
    body: Option<&[u8]>,
    kind: HttpGateAction,
    session_id: &str,
    allow_private_network: bool,
) -> Result<(), String> {
    let body_text = body.map(|b| String::from_utf8_lossy(b).to_string());
    gate_browser_action_with_context(
        kind.browser_action(),
        url.as_str(),
        body_text.map(|b| json!({"payload": b, "http_action": kind.action_label()})),
        session_id,
        GateNetworkContext::from_allow_private(allow_private_network),
    )
}

/// Re-gate redirect destination; returns Err to block follow.
pub fn gate_redirect_destination(
    from: &str,
    to: &str,
    session_id: &str,
    allow_private_network: bool,
) -> Result<(), String> {
    gate_browser_action_with_context(
        BrowserAction::Navigate,
        to,
        None,
        session_id,
        GateNetworkContext::from_allow_private(allow_private_network),
    )
    .map_err(|e| format!("TERMINUS REFUSED (redirect): {from} -> {to}: {e}"))
}

/// Strip Authorization-like headers on cross-origin redirect (architecture §3a).
pub fn strip_cross_origin_auth(
    extra_headers: &mut std::collections::HashMap<String, String>,
    from: &Url,
    to: &Url,
) {
    if from.origin() == to.origin() {
        return;
    }
    extra_headers.retain(|k, _| {
        let lower = k.to_ascii_lowercase();
        lower != "authorization" && lower != "proxy-authorization"
    });
}
