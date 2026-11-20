//! OMNIS GLASS Glass Pane — session bridge (CDP + receipts + auth).

pub mod pane_auth;
pub mod receipt_tail;
pub mod screencast;

pub use pane_auth::{format_ipc_message, is_pane_ui_url, parse_ipc_message, PaneAuth};
pub use receipt_tail::{ActionRailEntry, ReceiptTailer};

pub mod observer;
