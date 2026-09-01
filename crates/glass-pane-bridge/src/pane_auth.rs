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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_required() {
        let auth = PaneAuth::generate();
        assert!(auth.check(Some(auth.token())));
        assert!(!auth.check(None));
        assert!(!auth.check(Some("wrong")));
    }
}
