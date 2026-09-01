//! Hash-chained `browser.action` receipts — redacted at source, Ed25519-signed.

mod chain;
mod redact;
mod sign;

pub use chain::{canon, verify_chain, BrowserActionWriter, ChainVerifyResult};
pub use redact::{digest_sha256, redact_action_fields};
pub use sign::verify_signature;
pub use sign::{load_or_create_signer, Signer};
