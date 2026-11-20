use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserAction {
    Navigate,
    Type,
    Fill,
    Submit,
    ClickSubmit,
    Upload,
    ScriptFetch,
    StorageWrite,
    Eval,
    Scrape,
    Screenshot,
}

impl BrowserAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Navigate => "navigate",
            Self::Type => "type",
            Self::Fill => "fill",
            Self::Submit => "submit",
            Self::ClickSubmit => "click-submit",
            Self::Upload => "upload",
            Self::ScriptFetch => "script-fetch",
            Self::StorageWrite => "storage-write",
            Self::Eval => "eval",
            Self::Scrape => "scrape",
            Self::Screenshot => "screenshot",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundTarget {
    pub url: String,
    pub origin: String,
    pub host: String,
    pub port: u16,
    pub scheme: String,
    pub path_class: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls_cert_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundEnvelope {
    pub modality: String,
    pub action: String,
    pub target: OutboundTarget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
    pub lane: String,
    pub policy_version: String,
    pub coverage_class: String,
    pub agent_id: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_event: Option<bool>,
}

impl OutboundEnvelope {
    pub fn browser(
        action: BrowserAction,
        target: OutboundTarget,
        payload: Option<Value>,
        lane: &str,
        policy_version: &str,
        coverage_class: &str,
        session_id: &str,
    ) -> Self {
        Self {
            modality: "browser".into(),
            action: action.as_str().into(),
            target,
            payload,
            lane: lane.into(),
            policy_version: policy_version.into(),
            coverage_class: coverage_class.into(),
            agent_id: "glass".into(),
            session_id: session_id.into(),
            key_event: None,
        }
    }

    pub fn to_authorize_action(&self) -> Value {
        let mut action = serde_json::json!({
            "agent_id": self.agent_id,
            "kind": "browser",
            "modality": self.modality,
            "action": self.action,
            "target": self.target,
            "payload": self.payload,
            "lane": self.lane,
            "policy_version": self.policy_version,
            "coverage_class": self.coverage_class,
            "session_id": self.session_id,
        });
        if self.key_event == Some(true) { action["key_event"] = Value::Bool(true); }
        action
    }
}

pub fn target_from_url(url_str: &str) -> Result<OutboundTarget, String> {
    let parsed = url::Url::parse(url_str).map_err(|e| e.to_string())?;
    let host = parsed.host_str().unwrap_or("").to_string();
    let port = parsed.port_or_known_default().unwrap_or(443);
    let scheme = parsed.scheme().to_string();
    let origin = format!("{}://{}", scheme, host);
    let path = parsed.path();
    let path_class = if path.is_empty() || path == "/" {
        "root".into()
    } else if path.contains("/api/") {
        "api".into()
    } else {
        "path".into()
    };
    Ok(OutboundTarget {
        url: url_str.to_string(),
        origin,
        host,
        port,
        scheme,
        path_class,
        tls_cert_sha256: None,
    })
}
