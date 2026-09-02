//! Shared OMNIS GLASS governance — used by CDP, MCP, CLI, and HTTP client paths.

mod gate;
mod capture;
mod http;
mod session;

pub use gate::{
    check_post_navigate_redirect, gate_browser_action, gate_browser_action_with_context,
    gate_cdp_command, gate_pane_navigate, gate_storage_cookies, governance_enabled,
    record_handoff_cancel, record_navigation, session_id_key, state_dir, GateNetworkContext,
};
pub use capture::emit_live_capture;
pub use http::{gate_http_outbound, gate_redirect_destination, HttpGateAction, strip_cross_origin_auth};
pub use session::{
    agent_blocked, clear_inflight, current_driver, current_mode, lock_for_human, unlock_to_agent,
    Driver, PaneMode,
};
