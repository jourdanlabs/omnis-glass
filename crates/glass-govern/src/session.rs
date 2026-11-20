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

#[derive(Debug, Clone)]
pub struct CommandContext {
    pub driver: Driver,
    pub lane: Option<String>,
    pub allow_private_network: bool,
    pub session_namespace: Option<String>,
    approved_navigation: std::sync::Arc<Mutex<Option<String>>>,
}

impl CommandContext {
    pub fn agent() -> Self { Self { driver: Driver::Agent, lane: None, allow_private_network: false, session_namespace: None, approved_navigation: Default::default() } }
    pub fn human() -> Self {
        Self { driver: Driver::Human, lane: Some(std::env::var("OMNIS_GLASS_LANE_HUMAN").unwrap_or_else(|_| "browse:human".into())), allow_private_network: false, session_namespace: None, approved_navigation: Default::default() }
    }
    pub fn with_namespace(mut self, namespace: &str) -> Self { self.session_namespace = Some(namespace.into()); self }
    pub fn with_private_network(mut self, allow: bool) -> Self { self.allow_private_network = allow; self }
}

// Context is installed only for the duration of each future poll, then
// restored even on unwind. This scopes interleaved tasks independently, unlike
// a thread-local set across an await. Spawned tasks still need an explicit scope.
thread_local! {
    static COMMAND_CONTEXT: std::cell::RefCell<Option<CommandContext>> = const { std::cell::RefCell::new(None) };
}

struct ContextGuard(Option<CommandContext>);
impl Drop for ContextGuard {
    fn drop(&mut self) {
        COMMAND_CONTEXT.with(|slot| { slot.replace(self.0.take()); });
    }
}

struct CommandFuture<F> {
    context: CommandContext,
    future: std::pin::Pin<Box<F>>,
}

impl<F: std::future::Future> std::future::Future for CommandFuture<F> {
    type Output = F::Output;
    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        let this = self.get_mut();
        let previous = COMMAND_CONTEXT.with(|slot| slot.replace(Some(this.context.clone())));
        let _guard = ContextGuard(previous);
        this.future.as_mut().poll(cx)
    }
}

pub async fn with_command_context<F: std::future::Future>(context: CommandContext, future: F) -> F::Output {
    CommandFuture { context, future: Box::pin(future) }.await
}

struct DynamicCommandFuture<F, P> {
    provider: P,
    future: std::pin::Pin<Box<F>>,
}

impl<F: std::future::Future, P: Fn() -> CommandContext + Unpin> std::future::Future for DynamicCommandFuture<F, P> {
    type Output = F::Output;
    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        let this = self.get_mut();
        let previous = COMMAND_CONTEXT.with(|slot| slot.replace(Some((this.provider)())));
        let _guard = ContextGuard(previous);
        this.future.as_mut().poll(cx)
    }
}

/// Resolve a page's driver on every poll, so a handoff cannot leave a parked
/// autonomous page task carrying the previous driver's authority.
pub async fn with_dynamic_command_context<F: std::future::Future, P: Fn() -> CommandContext + Unpin>(provider: P, future: F) -> F::Output {
    DynamicCommandFuture { provider, future: Box::pin(future) }.await
}

pub fn command_context() -> Option<CommandContext> {
    COMMAND_CONTEXT.with(|slot| slot.borrow().clone())
}

// A CDP-approved GET may consume the matching initial HTTP gate once. Redirects,
// script fetches and later requests still pass their own outbound gates.
pub fn approve_http_navigation(url: &str) {
    let Ok(url) = url::Url::parse(url) else { return; };
    if let Some(context) = command_context() {
        if let Ok(mut slot) = context.approved_navigation.lock() { *slot = Some(url.to_string()); }
    }
}

pub fn consume_approved_http_navigation(url: &url::Url) -> bool {
    let Some(context) = command_context() else { return false; };
    let Ok(mut slot) = context.approved_navigation.lock() else { return false; };
    if slot.as_deref() == Some(url.as_str()) { *slot = None; true } else { false }
}

struct SessionState {
    mode: PaneMode,
    driver: Driver,
    inflight_action: Option<String>,
    observer_owner: Option<String>,
    agent_waiting: bool,
    actions: std::collections::HashMap<u64, (String, std::sync::Arc<std::sync::atomic::AtomicBool>)>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            mode: PaneMode::Mirror,
            driver: Driver::Agent,
            inflight_action: None,
            observer_owner: None,
            agent_waiting: false,
            actions: std::collections::HashMap::new(),
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
        s.observer_owner = None;
        s.agent_waiting = false;
    }
}

/// Take control for an observer, recording the owner for disconnect cleanup.
pub fn take_observer_control(owner: &str) -> Result<Vec<String>, &'static str> {
    let mut s = session().lock().map_err(|_| "REFUSE_SESSION_LOCKED")?;
    if s.mode == PaneMode::Human && s.observer_owner.as_deref() != Some(owner) { return Err("CONTROL_HELD"); }
    let mut cancelled = Vec::new();
    for (action, flag) in s.actions.values() {
        if !flag.swap(true, std::sync::atomic::Ordering::AcqRel) { cancelled.push(action.clone()); }
    }
    let legacy = s.inflight_action.take();
    if cancelled.is_empty() && s.actions.is_empty() && s.mode != PaneMode::Human {
        if let Some(action) = legacy { cancelled.push(action); }
    }
    s.mode = PaneMode::Human;
    s.driver = Driver::Human;
    s.observer_owner = Some(owner.to_string());
    Ok(cancelled)
}

/// One agent operation remains tracked across all await points until it ends.
pub struct AgentAction {
    id: u64,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl AgentAction {
    /// Retire and publish the result atomically against a concurrent handoff.
    pub fn finish_with<R>(self, publish: impl FnOnce(bool) -> R) -> R {
        let mut state = session().lock().unwrap_or_else(|e| e.into_inner());
        state.actions.remove(&self.id);
        let result = publish(self.cancelled.load(std::sync::atomic::Ordering::Acquire));
        drop(state);
        result
    }
}

impl Drop for AgentAction {
    fn drop(&mut self) {
        if let Ok(mut s) = session().lock() { s.actions.remove(&self.id); }
    }
}

pub fn begin_agent_action(action: &str) -> Result<AgentAction, &'static str> {
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let mut s = session().lock().map_err(|_| "REFUSE_SESSION_LOCKED")?;
    if s.mode == PaneMode::Human {
        s.agent_waiting = true;
        return Err("TERMINUS REFUSE (session_locked): REFUSE_SESSION_LOCKED");
    }
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    s.actions.insert(id, (action.to_string(), cancelled.clone()));
    Ok(AgentAction { id, cancelled })
}

pub fn release_observer_control(owner: &str) -> bool {
    let Ok(mut s) = session().lock() else { return false; };
    if s.observer_owner.as_deref() != Some(owner) { return false; }
    s.mode = PaneMode::Mirror;
    s.driver = Driver::Agent;
    s.observer_owner = None;
    s.agent_waiting = false;
    true
}

/// True only for the observer that owns the active human-control lock.
pub fn observer_owns_control(owner: &str) -> bool {
    session().lock().map(|s| s.mode == PaneMode::Human && s.observer_owner.as_deref() == Some(owner)).unwrap_or(false)
}

pub fn observer_mode() -> (PaneMode, bool) {
    session().lock().map(|s| (s.mode, s.agent_waiting)).unwrap_or((PaneMode::Mirror, false))
}

pub fn mark_agent_waiting() {
    if let Ok(mut s) = session().lock() {
        if s.mode == PaneMode::Human { s.agent_waiting = true; }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    #[test]
    fn agent_action_waits_for_observer_release() {
        take_observer_control("sweep-s1").unwrap();
        let error = match begin_agent_action("navigate") { Err(error) => error, Ok(_) => panic!("agent action started during human control") };
        assert!(error.contains("REFUSE_SESSION_LOCKED"), "{error}");
        assert!(observer_mode().1);
        assert!(release_observer_control("sweep-s1"));
        assert!(begin_agent_action("navigate").is_ok());
    }

    #[test]
    fn approved_navigation_is_url_matched_one_use_and_scoped() {
        let target = url::Url::parse("https://example.test/page").unwrap();
        let other = url::Url::parse("https://example.test/other").unwrap();
        let mut action = Box::pin(with_command_context(CommandContext::human(), async {
            approve_http_navigation(target.as_str());
            assert!(!consume_approved_http_navigation(&other));
            assert!(consume_approved_http_navigation(&target));
            assert!(!consume_approved_http_navigation(&target));
        }));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(action.as_mut().poll(&mut cx).is_ready());
        assert!(!consume_approved_http_navigation(&target));
    }

    #[test]
    fn command_context_is_restored_between_interleaved_polls() {
        fn action(driver: Driver) -> impl Future<Output = ()> {
            let context = CommandContext { driver, ..CommandContext::agent() };
            with_command_context(context, async move {
                assert_eq!(command_context().unwrap().driver, driver);
                let mut first = true;
                std::future::poll_fn(|cx| {
                    assert_eq!(command_context().unwrap().driver, driver);
                    if first { first = false; cx.waker().wake_by_ref(); Poll::Pending } else { Poll::Ready(()) }
                }).await;
                assert_eq!(command_context().unwrap().driver, driver);
            })
        }
        let mut human = Box::pin(action(Driver::Human));
        let mut agent = Box::pin(action(Driver::Agent));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(human.as_mut().poll(&mut cx).is_pending());
        assert!(command_context().is_none());
        assert!(agent.as_mut().poll(&mut cx).is_pending());
        assert!(command_context().is_none());
        assert!(human.as_mut().poll(&mut cx).is_ready());
        assert!(agent.as_mut().poll(&mut cx).is_ready());
        assert!(command_context().is_none());
    }
}
