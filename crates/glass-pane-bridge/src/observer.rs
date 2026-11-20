//! Browser-level observer protocol. A single reader owns the socket stream.
use std::{collections::HashMap, fmt, sync::{Arc, atomic::{AtomicU64, Ordering}}};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::Message};

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, ObserverError>>>>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObserverError { pub code: String, pub message: String }
impl ObserverError {
    fn transport(message: impl Into<String>) -> Self { Self { code: "TRANSPORT".into(), message: message.into() } }
    fn refusal(message: String) -> Self {
        let code = message.split_whitespace().next().unwrap_or("").to_string();
        Self { code, message }
    }
}
impl fmt::Display for ObserverError { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.message) } }
impl std::error::Error for ObserverError {}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Target { pub connection_id: String, pub target_id: String, pub url: String, pub title: String, pub last_activity_ms: u64, pub scripts_stopped: bool }
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mode { pub mode: String, pub agent_waiting: bool }
impl Mode { pub fn badge(&self) -> &'static str { if self.mode == "human" { if self.agent_waiting { "LOCKED" } else { "HUMAN" } } else { "MIRROR" } } }
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TakeControlResult { pub mode: String, pub cancelled_action: Option<String> }
#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseResult { pub mode: String }
#[derive(Debug, Clone)]
pub struct ObserverEvent { pub method: String, pub params: Value }

pub struct ObserverClient {
    write: Mutex<futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, Message>>,
    pending: Pending,
    next_id: AtomicU64,
}
impl ObserverClient {
    pub async fn connect(url: &str) -> Result<(Arc<Self>, mpsc::UnboundedReceiver<ObserverEvent>), ObserverError> {
        let (socket, _) = connect_async(url).await.map_err(|e| ObserverError::transport(format!("CDP connect failed: {e}")))?;
        let (write, mut read) = socket.split();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let reader_pending = pending.clone();
        tokio::spawn(async move {
            while let Some(Ok(message)) = read.next().await {
                let Message::Text(text) = message else { continue };
                let Ok(value) = serde_json::from_str::<Value>(&text) else { continue };
                if let Some(id) = value.get("id").and_then(Value::as_u64) {
                    if let Some(tx) = reader_pending.lock().await.remove(&id) {
                        let result = if let Some(error) = value.get("error") {
                            Err(ObserverError::refusal(error.get("message").and_then(Value::as_str).unwrap_or("unknown CDP error").to_string()))
                        } else { Ok(value.get("result").cloned().unwrap_or(Value::Null)) };
                        let _ = tx.send(result);
                    }
                } else if let Some(method) = value.get("method").and_then(Value::as_str) {
                    if method.starts_with("Glass.") {
                        let _ = events_tx.send(ObserverEvent { method: method.into(), params: value.get("params").cloned().unwrap_or_else(|| json!({})) });
                    }
                }
            }
            for (_, tx) in reader_pending.lock().await.drain() { let _ = tx.send(Err(ObserverError::transport("CDP stream closed"))); }
        });
        Ok((Arc::new(Self { write: Mutex::new(write), pending, next_id: AtomicU64::new(1) }), events_rx))
    }
    async fn call(&self, method: &str, params: Value) -> Result<Value, ObserverError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        if let Err(e) = self.write.lock().await.send(Message::Text(json!({"id":id,"method":method,"params":params}).to_string().into())).await {
            self.pending.lock().await.remove(&id);
            return Err(ObserverError::transport(format!("CDP send failed: {e}")));
        }
        rx.await.map_err(|_| ObserverError::transport("CDP stream closed"))?
    }
    pub async fn authorize(&self, token: &str) -> Result<Option<String>, ObserverError> { let result = self.call("Glass.authorizeObserver", json!({"token":token})).await?; Ok(result.get("actionsLedgerPathSha256").and_then(Value::as_str).map(str::to_owned)) }
    pub async fn list_targets(&self) -> Result<Vec<Target>, ObserverError> { let v = self.call("Glass.listTargets", json!({})).await?; serde_json::from_value(v.get("targets").cloned().unwrap_or(Value::Null)).map_err(|e| ObserverError::transport(e.to_string())) }
    pub async fn observe(&self, connection_id: &str, target_id: &str) -> Result<String, ObserverError> { let v = self.call("Glass.observe", json!({"connectionId":connection_id,"targetId":target_id,"format":"jpeg","quality":80})).await?; v.get("observationId").and_then(Value::as_str).map(str::to_string).ok_or_else(|| ObserverError::transport("missing observationId")) }
    pub async fn stop_observing(&self) -> Result<(), ObserverError> { self.call("Glass.stopObserving", json!({})).await?; Ok(()) }
    pub async fn frame_ack(&self, observation_id: &str, frame_id: &Value) -> Result<(), ObserverError> { self.call("Glass.frameAck", json!({"observationId":observation_id,"frameId":frame_id})).await?; Ok(()) }
    pub async fn get_mode(&self) -> Result<Mode, ObserverError> { let v = self.call("Glass.getMode", json!({})).await?; serde_json::from_value(v).map_err(|e| ObserverError::transport(e.to_string())) }
    pub async fn take_control(&self) -> Result<TakeControlResult, ObserverError> { let v = self.call("Glass.takeControl", json!({})).await?; serde_json::from_value(v).map_err(|e| ObserverError::transport(e.to_string())) }
    pub async fn release(&self) -> Result<ReleaseResult, ObserverError> { let v = self.call("Glass.release", json!({})).await?; serde_json::from_value(v).map_err(|e| ObserverError::transport(e.to_string())) }
    pub async fn human_command(&self, method: &str, params: Value) -> Result<Value, ObserverError> { self.call("Glass.humanCommand", json!({"method":method,"params":params})).await }
    pub async fn create_target(&self) -> Result<String, ObserverError> { let v = self.call("Target.createTarget", json!({"url":"about:blank"})).await?; v.get("targetId").and_then(Value::as_str).map(str::to_string).ok_or_else(|| ObserverError::transport("missing targetId")) }
}

/// Select an existing target, or create an observer-owned blank page when none exist.
/// The server supplies the connection ID via the subsequent target listing.
pub async fn choose_target(client: &ObserverClient, pin: Option<(&str, &str)>) -> Result<(Target, bool), ObserverError> {
    let mut targets = client.list_targets().await?;
    if let Some((connection, target)) = pin {
        return targets.into_iter().find(|t| t.connection_id == connection && t.target_id == target)
            .map(|t| (t, false)).ok_or_else(|| ObserverError::refusal("TARGET_NOT_FOUND pinned target".into()));
    }
    if let Some(target) = targets.iter().max_by_key(|t| t.last_activity_ms).cloned() { return Ok((target, false)); }
    let created = client.create_target().await?;
    for _ in 0..20 {
        targets = client.list_targets().await?;
        if let Some(target) = targets.into_iter().find(|t| t.target_id == created) { return Ok((target, true)); }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Err(ObserverError::transport("created target did not appear in observer list"))
}

/// Dispatch authenticated UI input only when the engine reports human mode.
pub async fn dispatch_ui(client: &ObserverClient, mode: &Mode, mirror_only: bool, command: &Value) -> Result<(), ObserverError> {
    dispatch_ui_gated(client, mode, mirror_only, command).await.map(|_| ())
}

async fn dispatch_ui_gated(client: &ObserverClient, mode: &Mode, mirror_only: bool, command: &Value) -> Result<Option<String>, ObserverError> {
    let name = command["type"].as_str().unwrap_or("");
    match name {
        "take" if !mirror_only => { return Ok(Some(client.take_control().await?.mode)); }
        "release" => { return Ok(Some(client.release().await?.mode)); }
        "navigate" if mode.mode == "human" && !mirror_only => { client.human_command("Page.navigate", json!({"url":command["url"]})).await?; }
        "reload" if mode.mode == "human" && !mirror_only => {
            client.human_command("Page.reload", json!({})).await?;
        }
        "back" | "forward" if mode.mode == "human" && !mirror_only => {
            let history = client.human_command("Page.getNavigationHistory", json!({})).await?;
            let current = history["currentIndex"].as_i64().unwrap_or(-1);
            let index = if name == "back" { current - 1 } else { current + 1 };
            if index >= 0 {
                if let Some(entry) = history["entries"].as_array().and_then(|v| v.get(index as usize)) {
                    client.human_command("Page.navigateToHistoryEntry", json!({"entryId":entry["id"]})).await?;
                }
            }
        }
        "mouse" if mode.mode == "human" && !mirror_only => { client.human_command("Input.dispatchMouseEvent", command["params"].clone()).await?; }
        "key" if mode.mode == "human" && !mirror_only => { client.human_command("Input.dispatchKeyEvent", command["params"].clone()).await?; }
        "navigate" | "reload" | "back" | "forward" | "mouse" | "key" => return Err(ObserverError::refusal("take control first".into())),
        _ => ()
    }
    Ok(None)
}

#[derive(Debug, PartialEq, Eq)]
pub enum UiEvent { Status(String), Mode(String) }

/// Apply successful mode results immediately and report engine refusals unchanged.
pub async fn dispatch_ui_with_events<F: FnMut(UiEvent)>(client: &ObserverClient, mode: &mut Mode, mirror_only: bool, command: &Value, mut emit: F) {
    match dispatch_ui_gated(client, mode, mirror_only, command).await {
        Ok(Some(next)) => {
            mode.mode = next;
            if mode.mode == "mirror" { mode.agent_waiting = false; }
            emit(UiEvent::Mode(mode.badge().into()));
        }
        Ok(None) => (),
        Err(error) => emit(UiEvent::Status(error.message)),
    }
}

/// Re-list on target changes and on the end of the current observation.
pub async fn refresh_target(client: &ObserverClient, event: &ObserverEvent, pin: Option<(&str, &str)>, current: &Target, observation_id: &str) -> Result<Option<(Target, bool, String)>, ObserverError> {
    let ended = event.method == "Glass.observationEnded" && event.params["observationId"].as_str() == Some(observation_id);
    if event.method != "Glass.targetsChanged" && !ended { return Ok(None); }
    let (next, own) = choose_target(client, pin).await?;
    if !ended && next.connection_id == current.connection_id && next.target_id == current.target_id { return Ok(None); }
    let id = client.observe(&next.connection_id, &next.target_id).await?;
    Ok(Some((next, own, id)))
}
