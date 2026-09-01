//! OMNIS GLASS Glass Pane — session bridge (CDP + receipts + auth).

pub mod cdp_client;
pub mod pane_auth;
pub mod receipt_tail;
pub mod screencast;
pub mod session;

pub use cdp_client::CdpClient;
pub use pane_auth::PaneAuth;
pub use receipt_tail::{ActionRailEntry, ReceiptTailer};
pub use session::{PaneSession, SessionCommand};
