use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use glass_capture::{SsrfCheck, SsrfVerdict};
use glass_ledger::BrowserActionWriter;
use serde_json::Value;

use crate::envelope::{BrowserAction, OutboundEnvelope};
use crate::lanes::{LanePolicy, LaneRegistry};

static POLICY: OnceLock<LanePolicy> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforceMode {
    Enforce,
    LocalOnly,
    DenyAll,
}

impl EnforceMode {
    pub fn from_env() -> Self {
        match std::env::var("OMNIS_GLASS_ENFORCE")
            .unwrap_or_else(|_| "enforce".into())
            .to_lowercase()
            .as_str()
        {
            "local_only" => Self::LocalOnly,
            "deny_all" => Self::DenyAll,
            _ => Self::Enforce,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GateVerdict {
    pub verdict: String,
    pub decision: String,
    pub reason: String,
}

pub struct TerminusGate {
    pub policy: LanePolicy,
    pub lane: String,
    pub enforce: EnforceMode,
    pub ssrf: SsrfCheck,
    state_dir: PathBuf,
    authorize_script: PathBuf,
}

impl TerminusGate {
    pub fn new(state_dir: &Path) -> Self {
        let policy = POLICY.get_or_init(|| {
            let path = std::env::var("OMNIS_GLASS_LANES")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../browser-lanes.yaml")
                });
            LaneRegistry::load_path(&path)
        });

        let authorize_script = std::env::var("TERMINUS_AUTHORIZE")
            .map(PathBuf::from)
            .or_else(|_| {
                std::env::var("CADUCEUS_ROOT").map(|r| {
                    PathBuf::from(r).join("scripts/terminus-authorize.mjs")
                })
            })
            .unwrap_or_else(|_| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/terminus-authorize.mjs")
            });

        let lane = std::env::var("OMNIS_GLASS_LANE").unwrap_or_else(|_| "browse:research".into());

        let allow_private = std::env::var("OBSCURA_ALLOW_PRIVATE_NETWORK")
            .ok()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));

        Self {
            policy: policy.clone(),
            lane,
            enforce: EnforceMode::from_env(),
            ssrf: SsrfCheck {
                allow_private,
            },
            state_dir: state_dir.to_path_buf(),
            authorize_script,
        }
    }

    pub fn check_ssrf(&self, url: &str) -> Option<GateVerdict> {
        self.check_ssrf_with_allow(url, self.ssrf.allow_private)
    }

    /// Per-request SSRF check — honors the HTTP client's `allow_private_network`
    /// opt-in in addition to process-global `OBSCURA_ALLOW_PRIVATE_NETWORK`.
    pub fn check_ssrf_with_allow(&self, url: &str, allow_private_network: bool) -> Option<GateVerdict> {
        let allow_private = allow_private_network || self.ssrf.allow_private;
        let check = SsrfCheck { allow_private };
        match check.check_url(url) {
            SsrfVerdict::Allow => None,
            SsrfVerdict::Refuse(r) => Some(GateVerdict {
                verdict: "REFUSE".into(),
                decision: "REFUSE_SSRF".into(),
                reason: r,
            }),
        }
    }

    pub fn authorize(&self, envelope: &OutboundEnvelope) -> GateVerdict {
        self.authorize_with_network(envelope, false, None)
    }

    pub fn authorize_with_network(
        &self,
        envelope: &OutboundEnvelope,
        allow_private_network: bool,
        enforce_override: Option<EnforceMode>,
    ) -> GateVerdict {
        let enforce = if self.enforce == EnforceMode::DenyAll {
            EnforceMode::DenyAll
        } else {
            enforce_override.unwrap_or(self.enforce)
        };

        if enforce == EnforceMode::DenyAll {
            return GateVerdict {
                verdict: "REFUSE".into(),
                decision: "REFUSE_DENY_ALL".into(),
                reason: "OMNIS_GLASS_ENFORCE=deny_all".into(),
            };
        }

        // SSRF before lane/host — literal and DNS-resolved private addresses
        if let Some(ssrf) = self.check_ssrf_with_allow(&envelope.target.url, allow_private_network) {
            return ssrf;
        }

        if enforce == EnforceMode::LocalOnly {
            return GateVerdict {
                verdict: "ALLOW".into(),
                decision: "PERMIT".into(),
                reason: "local_only".into(),
            };
        }

        if !self.policy.valid {
            return GateVerdict {
                verdict: "REFUSE".into(),
                decision: "REFUSE_POLICY_INVALID".into(),
                reason: self
                    .policy
                    .invalid_reason
                    .clone()
                    .unwrap_or_else(|| "invalid browser-lanes.yaml".into()),
            };
        }

        if !self.policy.host_allowed(&envelope.lane, &envelope.target.host) {
            return GateVerdict {
                verdict: "REFUSE".into(),
                decision: "REFUSE_HOST".into(),
                reason: format!(
                    "host '{}' not on lane '{}' allowlist",
                    envelope.target.host, envelope.lane
                ),
            };
        }

        if !self
            .policy
            .action_allowed(&envelope.lane, &envelope.action)
        {
            return GateVerdict {
                verdict: "REFUSE".into(),
                decision: "REFUSE_LANE_ACTION".into(),
                reason: format!(
                    "action '{}' not permitted on lane '{}'",
                    envelope.action, envelope.lane
                ),
            };
        }

        self.call_authorize_script(envelope)
    }

    fn call_authorize_script(&self, envelope: &OutboundEnvelope) -> GateVerdict {
        if !self.authorize_script.exists() {
            return GateVerdict {
                verdict: "REFUSE".into(),
                decision: "REFUSE_SCANNER_FAILURE".into(),
                reason: "terminus_unavailable".into(),
            };
        }

        let input = envelope.to_authorize_action().to_string();
        let node = std::env::var("NODE").unwrap_or_else(|_| "node".into());

        let output = Command::new(node)
            .arg(&self.authorize_script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        let mut child = match output {
            Ok(c) => c,
            Err(e) => {
                return GateVerdict {
                    verdict: "REFUSE".into(),
                    decision: "REFUSE_SCANNER_FAILURE".into(),
                    reason: format!("terminus spawn failed: {e}"),
                };
            }
        };

        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(input.as_bytes());
        }

        let result = child.wait_with_output();
        let out = match result {
            Ok(o) => o,
            Err(e) => {
                return GateVerdict {
                    verdict: "REFUSE".into(),
                    decision: "REFUSE_SCANNER_FAILURE".into(),
                    reason: format!("terminus wait failed: {e}"),
                };
            }
        };

        let parsed: Value = match serde_json::from_slice(&out.stdout) {
            Ok(v) => v,
            Err(_) => {
                return GateVerdict {
                    verdict: "REFUSE".into(),
                    decision: "REFUSE_SCANNER_FAILURE".into(),
                    reason: "terminus_unavailable".into(),
                };
            }
        };

        GateVerdict {
            verdict: parsed
                .get("verdict")
                .and_then(|v| v.as_str())
                .unwrap_or("REFUSE")
                .into(),
            decision: parsed
                .get("decision")
                .and_then(|v| v.as_str())
                .unwrap_or("REFUSE_SCANNER_FAILURE")
                .into(),
            reason: parsed
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("terminus")
                .into(),
        }
    }

    pub fn record_action(
        &self,
        action: BrowserAction,
        verdict: &GateVerdict,
        fields: serde_json::Map<String, Value>,
    ) {
        if let Ok(writer) = BrowserActionWriter::new(&self.state_dir) {
            let _ = writer.write_action(
                action.as_str(),
                &self.lane,
                &verdict.verdict,
                fields,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{target_from_url, OutboundEnvelope};
    use tempfile::tempdir;

    #[test]
    fn disallowed_host_refused() {
        let dir = tempdir().unwrap();
        std::env::set_var("OMNIS_GLASS_STATE_DIR", dir.path());
        let gate = TerminusGate::new(dir.path());
        let target = target_from_url("https://evil.com/form").unwrap();
        let env = OutboundEnvelope::browser(
            BrowserAction::Type,
            target,
            Some(serde_json::json!({"selector":"#x","field_value":"hello"})),
            &gate.lane,
            &gate.policy.version,
            &gate.policy.coverage_class,
            "test",
        );
        let v = gate.authorize(&env);
        assert_eq!(v.verdict, "REFUSE");
    }
}
