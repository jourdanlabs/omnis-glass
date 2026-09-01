//! Session lock — one driver at a time (agent vs human).

use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneMode {
    Mirror,
    Human,
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Driver {
    Agent,
    Human,
}

struct SessionState {
    mode: PaneMode,
    driver: Driver,
    inflight_action: Option<String>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            mode: PaneMode::Mirror,
            driver: Driver::Agent,
            inflight_action: None,
        }
    }
}

static SESSION: OnceLock<Mutex<SessionState>> = OnceLock::new();

fn session() -> &'static Mutex<SessionState> {
    SESSION.get_or_init(|| Mutex::new(SessionState::default()))
}

pub fn current_mode() -> PaneMode {
    session()
        .lock()
        .map(|s| s.mode)
        .unwrap_or(PaneMode::Mirror)
}

pub fn current_driver() -> Driver {
    session()
        .lock()
        .map(|s| s.driver)
        .unwrap_or(Driver::Agent)
}

pub fn is_session_locked_by_human() -> bool {
    matches!(
        session().lock().map(|s| s.mode),
        Ok(PaneMode::Human)
    )
}

pub fn lock_for_human() -> Option<String> {
    let mut s = session().lock().ok()?;
    let cancelled = s.inflight_action.take();
    s.mode = PaneMode::Human;
    s.driver = Driver::Human;
    cancelled
}

pub fn unlock_to_agent() {
    if let Ok(mut s) = session().lock() {
        s.mode = PaneMode::Mirror;
        s.driver = Driver::Agent;
        s.inflight_action = None;
    }
}

pub fn set_inflight(action: &str) {
    if let Ok(mut s) = session().lock() {
        s.inflight_action = Some(action.to_string());
    }
}

pub fn clear_inflight() {
    if let Ok(mut s) = session().lock() {
        s.inflight_action = None;
    }
}

pub fn driver_label() -> &'static str {
    match current_driver() {
        Driver::Human => "human",
        Driver::Agent => "agent",
    }
}

pub fn agent_blocked() -> bool {
    is_session_locked_by_human()
}
