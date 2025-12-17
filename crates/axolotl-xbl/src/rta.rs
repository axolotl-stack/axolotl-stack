use crate::auth::XblToken;
use crate::error::{XblError, XblResult};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{debug, error, info};

const RTA_URL: &str = "wss://rta.xboxlive.com/connect";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtaMessage {
    pub message_type: i32,
    pub sequence_id: i32,
    pub payload: Value,
}

pub struct RtaClient {
    token: XblToken,
    connection_id: Arc<RwLock<Option<String>>>,
    shutdown: Arc<RwLock<bool>>,
    event_handlers: Arc<Mutex<Vec<Box<dyn Fn(&Value) + Send + Sync>>>>,
}

impl RtaClient {
    pub fn new(token: XblToken) -> Self {
        Self {
            token,
            connection_id: Arc::new(RwLock::new(None)),
            shutdown: Arc::new(RwLock::new(false)),
            event_handlers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn connect_and_run(&self) -> XblResult<()> {
        let key = tokio_tungstenite::tungstenite::handshake::client::generate_key();

        let request = tokio_tungstenite::tungstenite::handshake::client::Request::builder()
            .uri(RTA_URL)
            .header("Authorization", self.token.auth_header())
            .header("Sec-WebSocket-Key", key)
            .header("Sec-WebSocket-Version", "13")
            .header("Host", "rta.xboxlive.com")
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .body(())
            .map_err(|e| XblError::Auth(e.to_string()))?;

        info!("Connecting to RTA WebSocket...");
        let (ws_stream, _) = connect_async(request)
            .await
            .map_err(|e| XblError::XboxLive(format!("RTA connection failed: {}", e)))?;

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

        // 2. Main loop
        while let Some(msg) = read.next().await {
            if *self.shutdown.read().await {
                break;
            }

            match msg {
                Ok(Message::Text(text)) => {
                    self.handle_message(&text, &mut write).await?;
                }
                Ok(Message::Close(_)) => {
                    info!("RTA WebSocket closed");
                    break;
                }
                Err(e) => {
                    error!("RTA WebSocket error: {}", e);
                    break;
                }
                _ => {}
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
        if text.contains("ConnectionId") {
            if let Some(data) = parts.get(4) {
                if let Some(conn_id) = data.get("ConnectionId").and_then(|v| v.as_str()) {
                    let mut lock = self.connection_id.write().await;
                    if lock.is_none() {
                        *lock = Some(conn_id.to_string());
                        info!("RTA Connection ID: {}", conn_id);

                        // Subscribe to friends
                        let sub_msg = serde_json::json!([
                            1,
                            2,
                            format!(
                                "https://social.xboxlive.com/users/xuid({})/friends",
                                self.token.xuid
                            )
                        ]);
                        write.send(Message::Text(sub_msg.to_string())).await.ok();
                    }
                }
            }
        }

        // Event handling
        if msg_type == 3 {
            if let Some(data) = parts.get(2) {
                debug!("RTA Event: {:?}", data);

                let handlers = self.event_handlers.lock().await;
                for handler in handlers.iter() {
                    handler(data);
                }
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
