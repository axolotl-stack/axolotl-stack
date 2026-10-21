use crate::auth::{XblToken, sign_websocket_request, update_server_time_from_header};
use crate::error::{XblError, XblResult};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{debug, error, info, trace};

// df-mc/go-xsapi rta/dial.go:connectURL, subprotocol.
const RTA_URL: &str = crate::constants::endpoints::RTA_WEBSOCKET;

/// Interval between keepalive pings (30 seconds).
/// Xbox RTA typically expects activity within 60 seconds.
const KEEPALIVE_INTERVAL_SECS: u64 = 30;
/// Proactively rotate long-lived RTA sockets.
///
/// In practice, long-lived RTA connections can degrade over time even when
/// keepalives are healthy. Rotating before server-side expiry windows improves
/// stability for multi-hour runs.
const MAX_CONNECTION_AGE_SECS: u64 = 75 * 60;

fn format_connect_error(err: tokio_tungstenite::tungstenite::Error) -> XblError {
    match err {
        tokio_tungstenite::tungstenite::Error::Http(response) => XblError::HttpStatus {
            status: response.status(),
            operation: "connect RTA",
        },
        _ => XblError::XboxLive("RTA connection failed".into()),
    }
}

pub type EventHandler = Box<dyn Fn(&Value) + Send + Sync>;

pub struct RtaClient {
    /// The current token - can be updated for reconnection with fresh token.
    token: Arc<RwLock<XblToken>>,
    connection_id: Arc<RwLock<Option<String>>>,
    shutdown: Arc<RwLock<bool>>,
    event_handlers: Arc<Mutex<Vec<EventHandler>>>,
    subscriptions: Mutex<HashSet<u64>>,
    resync_handlers: Mutex<Vec<Box<dyn Fn() + Send + Sync>>>,
    shutdown_notify: tokio::sync::Notify,
    pending: Mutex<HashSet<u64>>,
}

impl RtaClient {
    pub fn new(token: XblToken) -> Self {
        Self {
            token: Arc::new(RwLock::new(token)),
            connection_id: Arc::new(RwLock::new(None)),
            shutdown: Arc::new(RwLock::new(false)),
            event_handlers: Arc::new(Mutex::new(Vec::new())),
            subscriptions: Mutex::new(HashSet::new()),
            resync_handlers: Mutex::new(Vec::new()),
            shutdown_notify: tokio::sync::Notify::new(),
            pending: Mutex::new(HashSet::new()),
        }
    }

    /// Update the token for reconnection.
    /// Call this before reconnecting if the token may have expired.
    pub async fn update_token(&self, token: XblToken) {
        *self.token.write().await = token;
    }

    /// Get a clone of the current token.
    pub async fn get_token(&self) -> XblToken {
        self.token.read().await.clone()
    }

    /// Get the latest known RTA connection ID.
    pub async fn connection_id(&self) -> Option<String> {
        self.connection_id.read().await.clone()
    }

    pub async fn connect_and_run(&self) -> XblResult<()> {
        *self.connection_id.write().await = None;
        self.subscriptions.lock().await.clear();
        *self.pending.lock().await = HashSet::from([1]);
        let result = self.run_connection().await;
        *self.connection_id.write().await = None;
        self.subscriptions.lock().await.clear();
        self.pending.lock().await.clear();
        result
    }

    async fn run_connection(&self) -> XblResult<()> {
        let token = self.token.read().await.clone();
        let key = tokio_tungstenite::tungstenite::handshake::client::generate_key();

        let mut request = tokio_tungstenite::tungstenite::handshake::client::Request::builder()
            .uri(RTA_URL)
            .header("Sec-WebSocket-Key", key)
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Protocol", "rta.xboxlive.com.V2")
            .header("Host", "rta.xboxlive.com")
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .body(())
            .map_err(|e| XblError::Auth(e.to_string()))?;
        sign_websocket_request(&mut request, &token).await?;

        info!("Connecting to RTA WebSocket...");
        let (ws_stream, response) = connect_async(request).await.map_err(format_connect_error)?;
        if let Some(date) = response.headers().get("Date").and_then(|v| v.to_str().ok()) {
            update_server_time_from_header(date);
        }

        info!("RTA WebSocket connected");

        let (mut write, mut read) = ws_stream.split();

        // 1. Connection handshake
        // [Type, Sequence, Endpoint]
        let handshake =
            serde_json::json!([1, 1, "https://sessiondirectory.xboxlive.com/connections/"]);
        write
            .send(Message::Text(handshake.to_string()))
            .await
            .map_err(|_| XblError::XboxLive("Failed to send RTA handshake".into()))?;

        // 2. Main loop with keepalive
        let keepalive_interval = Duration::from_secs(KEEPALIVE_INTERVAL_SECS);
        let mut keepalive_timer = tokio::time::interval(keepalive_interval);
        keepalive_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        // Skip the first immediate tick
        keepalive_timer.tick().await;
        let rotate_after = tokio::time::sleep(Duration::from_secs(MAX_CONNECTION_AGE_SECS));
        tokio::pin!(rotate_after);

        loop {
            if *self.shutdown.read().await {
                debug!("RTA shutdown flag set, exiting loop");
                break;
            }

            tokio::select! {
                biased;
                _ = self.shutdown_notify.notified() => {
                    let _ = write.close().await;
                    break;
                }

                // Proactive reconnect for long-running stability.
                _ = &mut rotate_after => {
                    info!(
                        max_age_mins = MAX_CONNECTION_AGE_SECS / 60,
                        "Rotating RTA WebSocket before long-lived timeout window"
                    );
                    break;
                }

                // Keepalive ping
                _ = keepalive_timer.tick() => {
                    trace!("Sending RTA keepalive ping");
                    if write.send(Message::Ping(vec![])).await.is_err() {
                        return Err(XblError::XboxLive("Failed to send RTA keepalive ping".into()));
                    }
                }

                // Incoming message
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            self.handle_message(&text, &mut write).await?;
                        }
                        Some(Ok(Message::Pong(_))) => {
                            trace!("Received pong from RTA server");
                        }
                        Some(Ok(Message::Ping(data))) => {
                            trace!("Received ping, sending pong");
                            if write.send(Message::Pong(data)).await.is_err() {
                                error!("Failed to send RTA pong");
                                break;
                            }
                        }
                        Some(Ok(Message::Close(frame))) => {
                            if let Some(frame) = frame {
                                info!(
                                    code = ?frame.code,
                                    "RTA WebSocket closed by server"
                                );
                            } else {
                                info!("RTA WebSocket closed by server");
                            }
                            break;
                        }
                        Some(Err(_)) => {
                            return Err(XblError::XboxLive("RTA WebSocket read failed".into()));
                        }
                        None => {
                            info!("RTA WebSocket stream ended");
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn wait_for_connection_id(&self) -> XblResult<String> {
        // Simple polling for now, could be a Notify
        for _ in 0..100 {
            if let Some(id) = self.connection_id.read().await.clone() {
                return Ok(id);
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
        Err(XblError::XboxLive(
            "Timed out waiting for RTA Connection ID".into(),
        ))
    }

    async fn handle_message(
        &self,
        text: &str,
        write: &mut futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            Message,
        >,
    ) -> XblResult<()> {
        // df-mc/go-xsapi rta/conn.go:readHeader, readResponse, handleMessage.
        let parts = parse_message(text)?;
        let msg_type = parts[0].as_u64().unwrap_or_default();
        if msg_type == 1 {
            let sequence = parts[1].as_u64().unwrap_or_default();
            let status = parts[2].as_i64().unwrap_or_default();
            if status != 0 {
                return Err(XblError::XboxLive(format!(
                    "RTA subscription failed with code {status}"
                )));
            }
            if !self.pending.lock().await.remove(&sequence) {
                return Err(XblError::XboxLive(
                    "Unexpected RTA subscription sequence".into(),
                ));
            }
            let subscription_id = parts
                .get(3)
                .and_then(Value::as_u64)
                .ok_or_else(|| XblError::XboxLive("Missing RTA subscription ID".into()))?;
            self.subscriptions.lock().await.insert(subscription_id);
            if sequence == 2 {
                return Ok(());
            }
            let conn_id = parts
                .get(4)
                .and_then(|v| v.get("ConnectionId"))
                .and_then(Value::as_str)
                .and_then(|v| uuid::Uuid::parse_str(v).ok())
                .filter(|v| !v.is_nil())
                .ok_or_else(|| XblError::XboxLive("Missing or invalid RTA connection ID".into()))?
                .to_string();
            let mut lock = self.connection_id.write().await;
            let previous = lock.clone();
            *lock = Some(conn_id.clone());
            drop(lock);

            if previous.as_deref() != Some(conn_id.as_str()) {
                info!(previous = ?previous, current = conn_id, "RTA Connection ID updated");
            } else {
                debug!(current = conn_id, "RTA Connection ID received");
            }

            // Resource URI: df-mc/go-xsapi social/client.go:New.
            let xuid = self.token.read().await.xuid.clone();
            let sub_msg = serde_json::json!([
                1,
                2,
                format!("https://social.xboxlive.com/users/xuid({})/friends", xuid)
            ]);
            self.pending.lock().await.insert(2);

            write
                .send(Message::Text(sub_msg.to_string()))
                .await
                .map_err(|_| XblError::XboxLive("Failed to subscribe to friend feed".into()))?;

            info!(xuid = %xuid, connection_id = conn_id, "Subscribed to RTA friend feed");
        }

        if msg_type == 4 {
            for handler in self.resync_handlers.lock().await.iter() {
                handler();
            }
        }
        // Only dispatch events for subscriptions acknowledged on this socket.
        if msg_type == 3
            && let Some(id) = parts.get(1).and_then(Value::as_u64)
            && self.subscriptions.lock().await.contains(&id)
            && let Some(data) = parts.get(2)
        {
            let handlers = self.event_handlers.lock().await;
            for handler in handlers.iter() {
                handler(data);
            }
        }

        Ok(())
    }

    pub async fn on_event<F>(&self, handler: F)
    where
        F: Fn(&Value) + Send + Sync + 'static,
    {
        let mut handlers = self.event_handlers.lock().await;
        handlers.push(Box::new(handler));
    }

    /// Called when RTA requests a refresh of the subscribed resources.
    pub async fn on_resync<F>(&self, handler: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.resync_handlers.lock().await.push(Box::new(handler));
    }

    pub async fn shutdown(&self) {
        *self.shutdown.write().await = true;
        self.shutdown_notify.notify_one();
    }
}

fn parse_message(text: &str) -> XblResult<Vec<Value>> {
    let parts: Vec<Value> = serde_json::from_str(text)
        .map_err(|_| XblError::XboxLive("Malformed RTA message".into()))?;
    let kind = parts
        .first()
        .and_then(Value::as_u64)
        .ok_or_else(|| XblError::XboxLive("Missing RTA message type".into()))?;
    let valid = match kind {
        1 | 2 => {
            parts.get(1).and_then(Value::as_u64).is_some()
                && parts.get(2).and_then(Value::as_i64).is_some()
        }
        3 => parts.get(1).and_then(Value::as_u64).is_some() && parts.len() >= 3,
        4 => true,
        _ => false,
    };
    if !valid {
        return Err(XblError::XboxLive("Invalid RTA message header".into()));
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_rta_frames_are_safe() {
        for frame in [
            "[]",
            "null",
            "\"ECHOED_SECRET\"",
            "[1]",
            "[1,1,\"ECHOED_SECRET\"]",
            "[3,1]",
            "[99]",
        ] {
            let error = parse_message(frame).unwrap_err();
            assert!(!format!("{error} {error:?}").contains("ECHOED_SECRET"));
        }
        assert!(parse_message("[4]").is_ok());
        assert!(parse_message("[1,1,0,42,{}]").is_ok());
        assert!(parse_message("[3,42,{}]").is_ok());
    }
}
