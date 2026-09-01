use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Deserialize)]
pub struct LanePolicyFile {
    pub policy_version: String,
    pub coverage_class: String,
    pub lanes: std::collections::BTreeMap<String, LaneConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaneConfig {
    pub sensitive: bool,
    pub hosts: Vec<String>,
    pub allowed_origins: Vec<String>,
    pub egress_rule: String,
    pub max_download_bytes: u64,
    pub screenshot_policy: String,
    pub allowed_residencies: Vec<String>,
    #[serde(default)]
    pub sensitive_hosts: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LanePolicy {
    pub version: String,
    pub coverage_class: String,
    pub digest: String,
    pub lanes: std::collections::BTreeMap<String, LaneConfig>,
    pub valid: bool,
    pub invalid_reason: Option<String>,
}

pub struct LaneRegistry;

impl LaneRegistry {
    pub fn load_yaml(content: &str) -> LanePolicy {
        match serde_yaml::from_str::<LanePolicyFile>(content) {
            Ok(file) => {
                let digest = policy_digest(content);
                LanePolicy {
                    version: file.policy_version,
                    coverage_class: file.coverage_class,
                    digest,
                    lanes: file.lanes,
                    valid: true,
                    invalid_reason: None,
                }
            }
            Err(e) => LanePolicy {
                version: "invalid".into(),
                coverage_class: "invalid".into(),
                digest: String::new(),
                lanes: Default::default(),
                valid: false,
                invalid_reason: Some(e.to_string()),
            },
        }
    }

    pub fn load_path(path: &std::path::Path) -> LanePolicy {
        match std::fs::read_to_string(path) {
            Ok(s) => Self::load_yaml(&s),
            Err(e) => LanePolicy {
                version: "invalid".into(),
                coverage_class: "invalid".into(),
                digest: String::new(),
                lanes: Default::default(),
                valid: false,
                invalid_reason: Some(e.to_string()),
            },
        }
    }
}

impl LanePolicy {
    pub fn lane(&self, name: &str) -> Option<&LaneConfig> {
        if !self.valid {
            return None;
        }
        self.lanes.get(name)
    }

    pub fn host_allowed(&self, lane: &str, host: &str) -> bool {
        let Some(cfg) = self.lane(lane) else {
            return false;
        };
        let host = host.to_lowercase();
        cfg.hosts.iter().any(|h| h.to_lowercase() == host)
    }

    pub fn action_allowed(&self, lane: &str, action: &str) -> bool {
        let Some(cfg) = self.lane(lane) else {
            return false;
        };
        match cfg.egress_rule.as_str() {
            "read_only" => !matches!(
                action,
                "type" | "fill" | "submit" | "click-submit" | "upload" | "storage-write"
                    | "script-fetch" | "dump-original"
            ),
            "deny_external" => action == "navigate", // only local navigations pre-checked
            _ => true,
        }
    }
}

pub fn policy_digest(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrape_lane_blocks_type() {
        let yaml = include_str!("../../../browser-lanes.yaml");
        let policy = LaneRegistry::load_yaml(yaml);
        assert!(policy.valid);
        assert!(!policy.action_allowed("browse:scrape", "type"));
        assert!(policy.action_allowed("browse:scrape", "navigate"));
    }
}
