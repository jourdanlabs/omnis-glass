//! Minimal CDP WebSocket client for Glass Pane.

use std::sync::atomic::{AtomicU64, Ordering};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::{connect_async, tungstenite::Message};

static MSG_ID: AtomicU64 = AtomicU64::new(1);

pub struct CdpClient {
    write: futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        Message,
    >,
    read: futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    >,
}

impl CdpClient {
    pub async fn connect(url: &str) -> Result<Self, String> {
        let (ws, _) = connect_async(url)
            .await
            .map_err(|e| format!("CDP connect failed: {e}"))?;
        let (write, read) = ws.split();
        Ok(Self { write, read })
    }

    fn next_id() -> u64 {
        MSG_ID.fetch_add(1, Ordering::Relaxed)
    }

    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = Self::next_id();
        let req = json!({ "id": id, "method": method, "params": params });
        self.write
            .send(Message::Text(req.to_string().into()))
            .await
            .map_err(|e| format!("CDP send failed: {e}"))?;
        loop {
            let msg = self
                .read
                .next()
                .await
                .ok_or("CDP stream closed")?
                .map_err(|e| format!("CDP read failed: {e}"))?;
            if let Message::Text(text) = msg {
                let v: Value = serde_json::from_str(&text).map_err(|e| format!("CDP JSON: {e}"))?;
                if v.get("id").and_then(|x| x.as_u64()) == Some(id) {
                    if let Some(err) = v.get("error") {
                        return Err(err.to_string());
                    }
                    return Ok(v.get("result").cloned().unwrap_or(Value::Null));
                }
            }
        }
    }

    pub async fn navigate(&mut self, url: &str) -> Result<(), String> {
        self.call("Page.navigate", json!({ "url": url })).await?;
        Ok(())
    }

    pub async fn start_screencast(&mut self) -> Result<(), String> {
        self.call(
            "Page.startScreencast",
            json!({ "format": "jpeg", "quality": 80, "everyNthFrame": 1 }),
        )
        .await?;
        Ok(())
    }

    pub async fn ack_screencast(&mut self, session_id: i64) -> Result<(), String> {
        let _ = self
            .call("Page.screencastFrameAck", json!({ "sessionId": session_id }))
            .await?;
        Ok(())
    }

    /// Read next screencast frame event, if any (non-blocking-ish single poll).
    pub async fn next_screencast_frame(&mut self) -> Option<(String, Value)> {
        loop {
            let msg = self.read.next().await?;
            let Message::Text(text) = msg.ok()? else {
                continue;
            };
            let v: Value = serde_json::from_str(&text).ok()?;
            if v.get("method").and_then(|m| m.as_str()) == Some("Page.screencastFrame") {
                let params = v.get("params")?.clone();
                let data = params.get("data")?.as_str()?.to_string();
                return Some((data, params));
            }
        }
    }
}

/// Mirror mode must not send navigate — enforced at session layer.
pub fn mirror_blocks_navigate(mirror_mode: bool) -> bool {
    mirror_mode
}
