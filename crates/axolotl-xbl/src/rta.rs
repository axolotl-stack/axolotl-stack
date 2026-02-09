use crate::auth::{XblToken, sign_request_for_url, update_server_time_from_header};
use crate::error::{XblError, XblResult};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{debug, error, info, trace};

const RTA_URL: &str = "wss://rta.xboxlive.com/connect";

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
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            XblError::XboxLive(format!("RTA connection failed: HTTP {}", response.status()))
        }
        other => XblError::XboxLive(format!("RTA connection failed: {}", other)),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtaMessage {
    pub message_type: i32,
    pub sequence_id: i32,
    pub payload: Value,
}

pub type EventHandler = Box<dyn Fn(&Value) + Send + Sync>;

pub struct RtaClient {
    /// The current token - can be updated for reconnection with fresh token.
    token: Arc<RwLock<XblToken>>,
    connection_id: Arc<RwLock<Option<String>>>,
    shutdown: Arc<RwLock<bool>>,
    event_handlers: Arc<Mutex<Vec<EventHandler>>>,
}

impl RtaClient {
    pub fn new(token: XblToken) -> Self {
        Self {
            token: Arc::new(RwLock::new(token)),
            connection_id: Arc::new(RwLock::new(None)),
            shutdown: Arc::new(RwLock::new(false)),
            event_handlers: Arc::new(Mutex::new(Vec::new())),
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
        let token = self.token.read().await.clone();
        let authorization = token.auth_header();
        let signature =
            sign_request_for_url("GET", RTA_URL, &authorization, b"").map_err(XblError::Auth)?;
        let key = tokio_tungstenite::tungstenite::handshake::client::generate_key();

        let request = tokio_tungstenite::tungstenite::handshake::client::Request::builder()
            .uri(RTA_URL)
            .header("Authorization", authorization)
            .header("Signature", signature)
            .header("Sec-WebSocket-Key", key)
            .header("Sec-WebSocket-Version", "13")
            .header("Host", "rta.xboxlive.com")
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .body(())
            .map_err(|e| XblError::Auth(e.to_string()))?;

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
            .map_err(|e| XblError::XboxLive(format!("Failed to send handshake: {}", e)))?;

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
                    if let Err(e) = write.send(Message::Ping(vec![])).await {
                        return Err(XblError::XboxLive(format!(
                            "Failed to send RTA keepalive ping: {}",
                            e
                        )));
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
                            if let Err(e) = write.send(Message::Pong(data)).await {
                                error!("Failed to send pong: {}", e);
                                break;
                            }
                        }
                        Some(Ok(Message::Close(frame))) => {
                            if let Some(frame) = frame {
                                info!(
                                    code = ?frame.code,
                                    reason = %frame.reason,
                                    "RTA WebSocket closed by server"
                                );
                            } else {
                                info!("RTA WebSocket closed by server");
                            }
                            break;
                        }
                        Some(Err(e)) => {
                            return Err(XblError::XboxLive(format!("RTA WebSocket error: {}", e)));
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
        // Broadcaster format: [Type, SequenceId, ...data]
        // We parse as array first
        let parts: Vec<Value> = serde_json::from_str(text)
            .map_err(|e| XblError::XboxLive(format!("Failed to parse RTA message: {}", e)))?;

        if parts.is_empty() {
            return Ok(());
        }

        let msg_type = parts[0].as_i64().unwrap_or(0);

        // Check for ConnectionId response
        if text.contains("ConnectionId")
            && let Some(data) = parts.get(4)
            && let Some(conn_id) = data.get("ConnectionId").and_then(|v| v.as_str())
        {
            let mut lock = self.connection_id.write().await;
            let previous = lock.clone();
            *lock = Some(conn_id.to_string());
            drop(lock);

            if previous.as_deref() != Some(conn_id) {
                info!(previous = ?previous, current = conn_id, "RTA Connection ID updated");
            } else {
                debug!(current = conn_id, "RTA Connection ID received");
            }

            // Always resubscribe on ConnectionId messages (includes reconnects).
            let xuid = self.token.read().await.xuid.clone();
            let sub_msg = serde_json::json!([
                1,
                2,
                format!("https://social.xboxlive.com/users/xuid({})/friends", xuid)
            ]);

            write
                .send(Message::Text(sub_msg.to_string()))
                .await
                .map_err(|e| {
                    XblError::XboxLive(format!("Failed to subscribe to friend feed: {}", e))
                })?;

            info!(xuid = %xuid, connection_id = conn_id, "Subscribed to RTA friend feed");
        }

        // Event handling
        if msg_type == 3
            && let Some(data) = parts.get(2)
        {
            debug!("RTA Event: {:?}", data);

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

    pub async fn shutdown(&self) {
        *self.shutdown.write().await = true;
    }
}
