//! Per-launch pane capability token (Fix 1).

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use subtle::ConstantTimeEq;

#[derive(Clone, Debug)]
pub struct PaneAuth {
    token: String,
}

impl PaneAuth {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        Self {
            token: URL_SAFE_NO_PAD.encode(bytes),
        }
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn check(&self, presented: Option<&str>) -> bool {
        match presented {
            Some(t) => {
                let a = t.as_bytes();
                let b = self.token.as_bytes();
                if a.len() != b.len() {
                    return false;
                }
                a.ct_eq(b).into()
            }
            None => false,
        }
    }
}

/// Wire format for wry IPC: `auth:<token>|<command>`.
pub fn format_ipc_message(token: &str, command: &str) -> String {
    format!("auth:{token}|{command}")
}

pub fn parse_ipc_message(body: &str) -> Option<(&str, &str)> {
    let rest = body.strip_prefix("auth:")?;
    let (token, command) = rest.split_once('|')?;
    if command.is_empty() {
        return None;
    }
    Some((token, command))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_required() {
        let auth = PaneAuth::generate();
        assert!(auth.check(Some(auth.token())));
        assert!(!auth.check(None));
        assert!(!auth.check(Some("wrong")));
        let original = auth.token().as_bytes();
        for index in [0, original.len() - 1] {
            let mut changed = original.to_vec();
            changed[index] = if changed[index] == b'A' { b'B' } else { b'A' };
            assert!(!auth.check(Some(std::str::from_utf8(&changed).unwrap())));
        }
    }
}

/// Only the embedded UI origin may enter the webview or its IPC channel.
pub fn is_pane_ui_url(input: &str) -> bool {
    url::Url::parse(input).map(|u| {
        u.scheme() == "glass-pane" && u.host_str() == Some("localhost")
            && u.port().is_none() && u.username().is_empty() && u.password().is_none()
    }).unwrap_or(false)
}
