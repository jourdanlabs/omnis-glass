//! Shared OMNIS GLASS governance — used by CDP, MCP, CLI, and HTTP client paths.

mod gate;
mod capture;
mod http;
mod session;

pub use gate::{
    check_post_navigate_redirect, gate_browser_action, gate_browser_action_with_context,
    gate_cdp_command, gate_approved_navigation, gate_storage_cookies, governance_enabled,
    record_handoff_cancel, record_session_locked_refusal, record_navigation, record_connection_navigation, forget_connection_navigation, forget_connection_session, session_id_key, state_dir, GateNetworkContext,
};
pub use capture::emit_live_capture;
pub use http::{gate_http_document_get, gate_http_outbound, gate_redirect_destination, HttpGateAction, strip_cross_origin_auth};
pub use session::{
    agent_blocked, clear_inflight, current_driver, current_mode, lock_for_human, unlock_to_agent,
    Driver, PaneMode, mark_agent_waiting, observer_mode, observer_owns_control, release_observer_control,
    take_observer_control, begin_agent_action, AgentAction, CommandContext, command_context, with_command_context, with_dynamic_command_context,
};
