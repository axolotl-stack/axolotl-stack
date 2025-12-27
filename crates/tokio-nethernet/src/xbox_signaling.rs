//! Xbox Live WebRTC signaling implementation.
//!
//! This module implements the [`Signaling`] trait using Xbox Live's
//! signaling WebSocket (`signal.franchise.minecraft-services.net`).
//!
//! # Overview
//!
//! When friends connect via the "Join Game" button in Minecraft,
//! signaling messages are routed through Xbox Live's WebSocket instead
//! of LAN broadcast. The protocol is identical to LAN signaling:
//! - `CONNECTREQUEST` with SDP offer
//! - `CONNECTRESPONSE` with SDP answer
//! - `CANDIDATEADD` for ICE candidates
//!
//! # Usage
//!
//! ```ignore
//! use tokio_nethernet::xbox_signaling::XboxSignaling;
//!
//! let signaling = XboxSignaling::connect(nethernet_id, mc_token).await?;
//! let listener = NetherNetListener::new(Arc::new(signaling), config);
//! ```

use crate::signaling::{Credentials, IceServer, Signal, Signaling, SignalingChannel};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
use tracing::{debug, error, info, warn};

/// Xbox Live signaling WebSocket URL format.
/// Format with nethernet_id to get full URL.
pub const RTC_WEBSOCKET_URL: &str =
    "wss://signal.franchise.minecraft-services.net/ws/v1.0/signaling/";

/// Message types for the signaling WebSocket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WsMessageType {
    /// Heartbeat/keepalive.
    Heartbeat = 0,
    /// Signal message (CONNECTREQUEST, etc.).
    Signal = 1,
    /// Initialization with TURN credentials.
    Init = 2,
}

/// Outgoing WebSocket message.
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct WsOutgoing {
    #[serde(rename = "Type")]
    msg_type: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

/// Incoming WebSocket message.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WsIncoming {
    #[serde(rename = "Type")]
    msg_type: u8,
    from: Option<String>,
    message: Option<String>,
}

/// TURN auth server credentials from init message.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TurnAuthMessage {
    turn_auth_servers: Option<Vec<TurnAuthServer>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TurnAuthServer {
    username: String,
    password: String,
    urls: Vec<String>,
}

/// Xbox Live signaling client.
///
/// Implements [`Signaling`] using Xbox's signaling WebSocket.
pub struct XboxSignaling {
    /// Our nethernet ID.
    nethernet_id: u64,
    /// Channel to send outgoing signals.
    tx: mpsc::UnboundedSender<WsOutgoing>,
    /// Channel to receive incoming signals (takeable for SignalingChannel).
    rx: RwLock<Option<mpsc::Receiver<Signal>>>,
    /// Cached TURN credentials.
    credentials: RwLock<Option<Credentials>>,
}

impl XboxSignaling {
    /// Connect to Xbox Live signaling WebSocket.
    ///
    /// # Arguments
    /// - `nethernet_id`: Your NetherNet network ID
    /// - `mc_token`: Minecraft authorization token from PlayFab session start
    pub async fn connect(nethernet_id: u64, mc_token: &str) -> anyhow::Result<Arc<Self>> {
        let url = format!("{}{}", RTC_WEBSOCKET_URL, nethernet_id);

        // Create request with auth headers
        let request = http::Request::builder()
            .uri(&url)
            .header("Authorization", mc_token)
            .header("Session-Id", uuid::Uuid::new_v4().to_string())
            .header("Request-Id", uuid::Uuid::new_v4().to_string())
            .header(
                "Sec-WebSocket-Key",
                tungstenite::handshake::client::generate_key(),
            )
            .header("Sec-WebSocket-Version", "13")
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Host", "signal.franchise.minecraft-services.net")
            .body(())?;

        info!(nethernet_id, "Connecting to Xbox signaling WebSocket");
        let (ws_stream, _) = connect_async(request).await?;
        let (mut write, mut read) = ws_stream.split();

        // Channels for signal routing
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<WsOutgoing>();
        // Use bounded channel for SignalingChannel compatibility
        let (in_tx, in_rx) = mpsc::channel::<Signal>(256);
        let credentials: Arc<RwLock<Option<Credentials>>> = Arc::new(RwLock::new(None));
        let credentials_clone = Arc::clone(&credentials);

        // Spawn writer task
        let out_tx_clone = out_tx.clone();
        tokio::spawn(async move {
            // Start heartbeat
            let heartbeat_tx = out_tx_clone;
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(40)).await;
                    if heartbeat_tx
                        .send(WsOutgoing {
                            msg_type: WsMessageType::Heartbeat as u8,
                            to: None,
                            message: None,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });

            while let Some(msg) = out_rx.recv().await {
                let json = match serde_json::to_string(&msg) {
                    Ok(j) => j,
                    Err(e) => {
                        error!("Failed to serialize outgoing message: {}", e);
                        continue;
                    }
                };
                debug!(msg = %json, "Sending WebSocket message");
                if let Err(e) = write.send(WsMessage::Text(json.into())).await {
                    error!("WebSocket send error: {}", e);
                    break;
                }
            }
        });

        // Spawn reader task
        tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                let msg = match msg {
                    Ok(WsMessage::Text(text)) => text.to_string(),
                    Ok(WsMessage::Close(_)) => {
                        info!("WebSocket closed");
                        break;
                    }
                    Ok(_) => continue,
                    Err(e) => {
                        error!("WebSocket receive error: {}", e);
                        break;
                    }
                };

                debug!(msg = %msg, "Received WebSocket message");

                let incoming: WsIncoming = match serde_json::from_str(&msg) {
                    Ok(m) => m,
                    Err(e) => {
                        warn!("Failed to parse incoming message: {}", e);
                        continue;
                    }
                };

                match incoming.msg_type {
                    0 => {
                        // Heartbeat response, ignore
                    }
                    1 => {
                        // Signal message
                        if let (Some(from), Some(message)) = (incoming.from, incoming.message) {
                            match Signal::parse(&message, from.clone()) {
                                Ok(signal) => {
                                    if in_tx.send(signal).await.is_err() {
                                        break;
                                    }
                                }
                                Err(e) => {
                                    warn!("Failed to parse signal: {}", e);
                                }
                            }
                        }
                    }
                    2 => {
                        // Init message with TURN credentials
                        if let Some(message) = incoming.message {
                            if let Ok(turn_msg) = serde_json::from_str::<TurnAuthMessage>(&message)
                            {
                                if let Some(servers) = turn_msg.turn_auth_servers {
                                    let ice_servers: Vec<IceServer> = servers
                                        .into_iter()
                                        .map(|s| IceServer {
                                            username: s.username,
                                            password: s.password,
                                            urls: s.urls,
                                        })
                                        .collect();

                                    let mut creds = credentials_clone.write().await;
                                    *creds = Some(Credentials {
                                        expiration_seconds: 3600,
                                        ice_servers,
                                    });
                                    info!("Received TURN credentials");
                                }
                            }
                        }
                    }
                    _ => {
                        debug!(msg_type = incoming.msg_type, "Unknown message type");
                    }
                }
            }
        });

        info!("Xbox signaling WebSocket connected");

        // Wait briefly for init message with TURN credentials
        // The reader task should receive it quickly
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Get the credentials from the shared Arc - it's fine if empty at first
        let cached_credentials = credentials.read().await.clone();

        Ok(Arc::new(Self {
            nethernet_id,
            tx: out_tx,
            rx: RwLock::new(Some(in_rx)),
            credentials: RwLock::new(cached_credentials),
        }))
    }

    /// Receive the next incoming signal.
    ///
    /// Note: This will return `None` if `take_signal_receiver()` was called.
    pub async fn recv(&self) -> Option<Signal> {
        if let Some(rx) = self.rx.write().await.as_mut() {
            rx.recv().await
        } else {
            None
        }
    }
}

#[async_trait]
impl Signaling for XboxSignaling {
    async fn signal(&self, signal: Signal) -> anyhow::Result<()> {
        let msg = WsOutgoing {
            msg_type: WsMessageType::Signal as u8,
            to: Some(signal.network_id.clone()),
            message: Some(signal.to_string()),
        };

        self.tx
            .send(msg)
            .map_err(|e| anyhow::anyhow!("Send failed: {}", e))
    }

    async fn credentials(&self) -> Option<Credentials> {
        self.credentials.read().await.clone()
    }

    fn network_id(&self) -> String {
        self.nethernet_id.to_string()
    }
}

#[async_trait]
impl SignalingChannel for XboxSignaling {
    async fn take_signal_receiver(&self) -> Option<mpsc::Receiver<Signal>> {
        self.rx.write().await.take()
    }
}

impl std::fmt::Debug for XboxSignaling {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XboxSignaling")
            .field("nethernet_id", &self.nethernet_id)
            .finish()
    }
}
