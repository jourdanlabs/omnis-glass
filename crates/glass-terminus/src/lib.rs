//! TERMINUS browser egress gate — builds OutboundEnvelope, shells to authorize script.

mod envelope;
mod gate;
mod lanes;

pub use envelope::{target_from_url, BrowserAction, OutboundEnvelope, OutboundTarget};
pub use gate::{EnforceMode, GateVerdict, TerminusGate};
pub use lanes::{LaneConfig, LanePolicy, LaneRegistry};
