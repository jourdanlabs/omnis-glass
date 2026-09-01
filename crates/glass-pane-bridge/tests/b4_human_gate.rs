//! B4/B5/B6/B7 — Human browse + session lock.

use glass_govern::{agent_blocked, gate_browser_action, unlock_to_agent};
use glass_pane_bridge::{PaneSession, SessionCommand};
use glass_terminus::BrowserAction;
use tempfile::tempdir;

fn env(dir: &tempfile::TempDir) {
    std::env::set_var("OMNIS_GLASS_STATE_DIR", dir.path());
    std::env::set_var(
        "TERMINUS_AUTHORIZE",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts/terminus-authorize.mjs"
        ),
    );
    std::env::set_var("OMNIS_GLASS_LANE", "browse:research");
    std::env::set_var("OMNIS_GLASS_LANE_HUMAN", "browse:human");
    std::env::set_var("OMNIS_GLASS_ENFORCE", "enforce");
    unlock_to_agent();
}

#[test]
fn b4_human_disallowed_host_refused() {
    let dir = tempdir().unwrap();
    env(&dir);
    let mut session = PaneSession::new(false);
    session.handle_command(SessionCommand::TakeControl).unwrap();
    let err = session.handle_command(SessionCommand::Navigate("https://google.com".into()));
    assert!(err.is_err());
}

#[test]
fn b5_human_allowed_host_permitted_at_gate() {
    let dir = tempdir().unwrap();
    env(&dir);
    let mut session = PaneSession::new(false);
    session.handle_command(SessionCommand::TakeControl).unwrap();
    session
        .handle_command(SessionCommand::Navigate("https://example.com".into()))
        .expect("allowed host");
}

#[test]
fn b6_session_lock_blocks_agent() {
    let dir = tempdir().unwrap();
    env(&dir);
    let mut session = PaneSession::new(false);
    session.handle_command(SessionCommand::TakeControl).unwrap();
    assert!(agent_blocked());
    let err = gate_browser_action(BrowserAction::Navigate, "https://example.com", None, "mcp");
    assert!(err.is_err());
    assert!(err.unwrap_err().contains("SESSION_LOCKED"));
}

#[test]
fn b7_release_unblocks_agent() {
    let dir = tempdir().unwrap();
    env(&dir);
    let mut session = PaneSession::new(false);
    session.handle_command(SessionCommand::TakeControl).unwrap();
    session.handle_command(SessionCommand::Release).unwrap();
    assert!(!agent_blocked());
    gate_browser_action(BrowserAction::Navigate, "https://example.com", None, "mcp").unwrap();
}
