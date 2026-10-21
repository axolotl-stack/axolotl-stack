//! Retained franchise WebSocket signaling for existing Xbox friend joins.
//!
//! This is the repository's legacy ConnectionType 3 transport, not a port of
//! current df-mc/go-nethernet or go-xsapi: neither currently implements this
//! service. Current upstream HTTP signaling lives separately in `endpoint`.
//! The retained envelope uses a string `To`, message types 0/1/2 and a 40-second
//! heartbeat. No service acknowledgement, connection-ID field, or credential
//! refresh request has been invented. Server credential updates replace a
//! shared cache; expired credentials are not reused.

use crate::signal_monitor::{SignalMonitor, SignalMonitorConfig};
use crate::signaling::{Credentials, IceServer, Signal, Signaling, SignalingChannel};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tokio::sync::{RwLock, mpsc, watch};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Message as WsMessage, client::IntoClientRequest, protocol::WebSocketConfig},
};

/// Retained repository franchise endpoint; prefer the regional discovery URL.
/// There is no current go-nethernet/go-xsapi function defining this endpoint.
pub const RTC_WEBSOCKET_URL: &str =
    "wss://signal.franchise.minecraft-services.net/ws/v1.0/signaling/";

/// Retained repository service message types, not current upstream constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WsMessageType {
    Heartbeat = 0,
    Signal = 1,
    Init = 2,
}

/// Retained repository heartbeat period (formerly connect_with_url's heartbeat task).
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(40);
/// Retained repository lifetime fallback when the init omits expiration.
const LEGACY_CREDENTIAL_LIFETIME: u32 = 3600;
/// Local resource bounds; these are not asserted to be wire protocol constants.
const MAX_FRAME_BYTES: usize = 64 * 1024;
const QUEUE_CAPACITY: usize = 256;
const MAX_NETWORK_ID_BYTES: usize = 256;
const MAX_ICE_SERVERS: usize = 64;
const MAX_URLS_PER_SERVER: usize = 64;
/// Local stalled-write bound, not an acknowledgement deadline.
const WRITE_TIMEOUT: Duration = Duration::from_secs(15);

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

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WsIncoming {
    #[serde(rename = "Type")]
    msg_type: u8,
    from: Option<String>,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TurnAuthMessage {
    expiration_in_seconds: Option<u32>,
    turn_auth_servers: Vec<TurnAuthServer>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TurnAuthServer {
    username: String,
    password: String,
    urls: Vec<String>,
}

#[derive(Clone)]
struct CachedCredentials {
    credentials: Credentials,
    received_at: tokio::time::Instant,
}

impl CachedCredentials {
    fn is_valid(&self) -> bool {
        self.received_at.elapsed()
            < Duration::from_secs(u64::from(self.credentials.expiration_seconds))
    }
}

fn decode_credentials(message: &str) -> anyhow::Result<CachedCredentials> {
    anyhow::ensure!(
        message.len() <= MAX_FRAME_BYTES,
        "credentials exceed frame limit"
    );
    let message: TurnAuthMessage = serde_json::from_str(message)?;
    anyhow::ensure!(
        message.turn_auth_servers.len() <= MAX_ICE_SERVERS,
        "too many ICE servers"
    );
    let ice_servers = message
        .turn_auth_servers
        .into_iter()
        .map(|server| {
            anyhow::ensure!(
                server.urls.len() <= MAX_URLS_PER_SERVER,
                "too many ICE URLs"
            );
            Ok(IceServer {
                username: server.username,
                password: server.password,
                urls: server.urls,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(CachedCredentials {
        credentials: Credentials {
            expiration_seconds: message
                .expiration_in_seconds
                .unwrap_or(LEGACY_CREDENTIAL_LIFETIME),
            ice_servers,
        },
        received_at: tokio::time::Instant::now(),
    })
}

fn outgoing_signal(signal: &Signal) -> anyhow::Result<WsOutgoing> {
    signal.validate()?;
    anyhow::ensure!(
        signal.data.len() <= MAX_FRAME_BYTES,
        "signal exceeds frame limit"
    );
    anyhow::ensure!(
        !signal.network_id.is_empty() && signal.network_id.len() <= MAX_NETWORK_ID_BYTES,
        "invalid recipient network ID"
    );
    let message = signal.to_string();
    Ok(WsOutgoing {
        msg_type: WsMessageType::Signal as u8,
        to: Some(signal.network_id.clone()),
        message: Some(message),
    })
}

async fn wait_credentials(
    mut receiver: watch::Receiver<Option<CachedCredentials>>,
) -> anyhow::Result<Option<Credentials>> {
    loop {
        // Reject even a still-unexpired cached snapshot when the transport died.
        anyhow::ensure!(
            receiver.has_changed().is_ok(),
            "signaling connection closed"
        );
        let current = {
            let snapshot = receiver.borrow_and_update();
            snapshot
                .as_ref()
                .filter(|value| value.is_valid())
                .map(|value| value.credentials.clone())
        };
        if let Some(credentials) = current {
            return Ok(Some(credentials));
        }
        // Cancellation is owned by the caller's negotiation deadline. Only a real
        // server update can refresh this cache; no speculative request is sent.
        receiver
            .changed()
            .await
            .map_err(|_| anyhow::anyhow!("signaling connection closed before fresh credentials"))?;
    }
}

/// Retained Xbox friend signaling with bounded queues and a shared TURN cache.
pub struct XboxSignaling {
    nethernet_id: u64,
    tx: mpsc::Sender<String>,
    rx: RwLock<Option<mpsc::Receiver<Signal>>>,
    credentials: watch::Receiver<Option<CachedCredentials>>,
    monitor: Arc<SignalMonitor>,
    task: tokio::task::JoinHandle<()>,
}

impl XboxSignaling {
    #[deprecated(
        since = "0.2.0",
        note = "Use connect_with_url() with a URL from axolotl_xbl::discovery for regional routing"
    )]
    pub async fn connect(nethernet_id: u64, mc_token: &str) -> anyhow::Result<Arc<Self>> {
        Self::connect_with_url(
            format!("{RTC_WEBSOCKET_URL}{nethernet_id}"),
            nethernet_id,
            mc_token,
            SignalMonitorConfig::default(),
        )
        .await
    }

    #[deprecated(
        since = "0.2.0",
        note = "Use connect_with_url() with a URL from axolotl_xbl::discovery for regional routing"
    )]
    pub async fn connect_with_monitor(
        nethernet_id: u64,
        mc_token: &str,
        monitor_config: SignalMonitorConfig,
    ) -> anyhow::Result<Arc<Self>> {
        Self::connect_with_url(
            format!("{RTC_WEBSOCKET_URL}{nethernet_id}"),
            nethernet_id,
            mc_token,
            monitor_config,
        )
        .await
    }

    /// Connect using the retained franchise handshake and a regional discovery URL.
    pub async fn connect_with_url(
        url: String,
        nethernet_id: u64,
        mc_token: &str,
        monitor_config: SignalMonitorConfig,
    ) -> anyhow::Result<Arc<Self>> {
        let mut request = url.into_client_request()?;
        request
            .headers_mut()
            .insert("Authorization", mc_token.parse()?);
        request
            .headers_mut()
            .insert("Session-Id", uuid::Uuid::new_v4().to_string().parse()?);
        request
            .headers_mut()
            .insert("Request-Id", uuid::Uuid::new_v4().to_string().parse()?);
        let config = WebSocketConfig {
            max_message_size: Some(MAX_FRAME_BYTES),
            max_frame_size: Some(MAX_FRAME_BYTES),
            ..Default::default()
        };
        let (mut socket, _) = connect_async_with_config(request, Some(config), false).await?;
        let (out_tx, mut out_rx) = mpsc::channel::<String>(QUEUE_CAPACITY);
        let (in_tx, in_rx) = mpsc::channel(QUEUE_CAPACITY);
        let (credentials_tx, credentials) = watch::channel(None);
        let monitor = SignalMonitor::new_with_cleanup(monitor_config);
        let task_monitor = Arc::clone(&monitor);
        let task = tokio::spawn(async move {
            let mut heartbeat = tokio::time::interval_at(
                tokio::time::Instant::now() + HEARTBEAT_INTERVAL,
                HEARTBEAT_INTERVAL,
            );
            heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    outgoing = out_rx.recv() => {
                        let Some(text) = outgoing else { break };
                        if !matches!(tokio::time::timeout(WRITE_TIMEOUT, socket.send(WsMessage::Text(text))).await, Ok(Ok(()))) { break; }
                    }
                    _ = heartbeat.tick() => {
                        if !matches!(tokio::time::timeout(WRITE_TIMEOUT, socket.send(WsMessage::Text("{\"Type\":0}".into()))).await, Ok(Ok(()))) { break; }
                    }
                    incoming = socket.next() => {
                        let text = match incoming {
                            Some(Ok(WsMessage::Text(text))) => text,
                            Some(Ok(WsMessage::Ping(_))) => {
                                // Flush tungstenite's queued protocol Pong.
                                if !matches!(tokio::time::timeout(WRITE_TIMEOUT, socket.flush()).await, Ok(Ok(()))) { break; }
                                continue;
                            }
                            Some(Ok(WsMessage::Pong(_))) => continue,
                            _ => break,
                        };
                        let Ok(incoming) = serde_json::from_str::<WsIncoming>(&text) else { continue };
                        match incoming.msg_type {
                            0 => {},
                            1 => {
                                let (Some(from), Some(message)) = (incoming.from, incoming.message) else { continue };
                                if from.is_empty() || from.len() > MAX_NETWORK_ID_BYTES { continue; }
                                let Ok(signal) = Signal::parse(&message, from) else { continue };
                                task_monitor.process_incoming(&signal).await;
                                match in_tx.try_send(signal) {
                                    Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {},
                                    Err(mpsc::error::TrySendError::Closed(_)) => break,
                                }
                            }
                            2 => {
                                // Retain init frames without From; never accept peer-attributed credentials.
                                if incoming.from.as_deref().is_some_and(|from| from != "Server") { continue; }
                                let Some(message) = incoming.message else { continue };
                                if let Ok(credentials) = decode_credentials(&message)
                                    && credentials_tx.send(Some(credentials)).is_err() { break; }
                            }
                            _ => {},
                        }
                    }
                }
            }
            // Both channels close on every terminal path, waking waiters.
        });
        Ok(Arc::new(Self {
            nethernet_id,
            tx: out_tx,
            rx: RwLock::new(Some(in_rx)),
            credentials,
            monitor,
            task,
        }))
    }

    /// Returns None after transport closure or after take_signal_receiver().
    pub async fn recv(&self) -> Option<Signal> {
        self.rx.write().await.as_mut()?.recv().await
    }

    pub fn monitor(&self) -> &Arc<SignalMonitor> {
        &self.monitor
    }
    pub async fn monitor_summary(&self) -> String {
        self.monitor.summary().await
    }
}

impl Drop for XboxSignaling {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[async_trait]
impl Signaling for XboxSignaling {
    async fn signal(&self, signal: Signal) -> anyhow::Result<()> {
        let outgoing = outgoing_signal(&signal)?;
        let text = serde_json::to_string(&outgoing)?;
        anyhow::ensure!(
            text.len() <= MAX_FRAME_BYTES,
            "encoded signal exceeds frame limit"
        );
        self.monitor.process_outgoing(&signal).await;
        self.tx
            .try_send(text)
            .map_err(|_| anyhow::anyhow!("signaling writer closed or queue full"))
    }

    async fn credentials(&self) -> anyhow::Result<Option<Credentials>> {
        wait_credentials(self.credentials.clone()).await
    }

    async fn closed(&self) {
        let mut receiver = self.credentials.clone();
        while receiver.changed().await.is_ok() {}
    }

    fn requires_identity(&self) -> bool {
        false
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn retained_socket_routes_signals_and_refreshes_shared_credentials() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (release, keep_open) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let credentials = |from: Option<&str>, expiration: u32| {
                serde_json::json!({
                "Type": 2, "From": from,
                "Message": format!("{{\"ExpirationInSeconds\":{expiration},\"TurnAuthServers\":[]}}")
            }).to_string()
            };
            socket
                .send(WsMessage::Text(credentials(None, 10)))
                .await
                .unwrap();
            let WsMessage::Text(sent) = socket.next().await.unwrap().unwrap() else {
                panic!("expected signal")
            };
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&sent).unwrap()["To"],
                "42"
            );
            socket
                .send(WsMessage::Text(credentials(Some("Server"), 20)))
                .await
                .unwrap();
            socket
                .send(WsMessage::Text(credentials(Some("42"), 999)))
                .await
                .unwrap();
            for message in [
                r#"{"Type":1,"From":"42","Message":"UNKNOWN 1 data"}"#,
                r#"{"Type":1,"From":"42","Message":"CONNECTERROR 1 +1"}"#,
                r#"{"Type":1,"From":"42","Message":"CONNECTRESPONSE 1 answer"}"#,
            ] {
                socket.send(WsMessage::Text(message.into())).await.unwrap();
            }
            let _ = keep_open.await;
        });
        let signaling = XboxSignaling::connect_with_url(
            format!("ws://{address}/"),
            7,
            "test-token",
            SignalMonitorConfig::default(),
        )
        .await
        .unwrap();
        assert!(!signaling.requires_identity());
        assert_eq!(
            signaling
                .credentials()
                .await
                .unwrap()
                .unwrap()
                .expiration_seconds,
            10
        );
        signaling
            .signal(Signal {
                typ: "CONNECTREQUEST".into(),
                connection_id: 1,
                network_id: "42".into(),
                data: "offer".into(),
            })
            .await
            .unwrap();
        assert_eq!(signaling.recv().await.unwrap().typ, "CONNECTRESPONSE");
        assert_eq!(
            signaling
                .credentials()
                .await
                .unwrap()
                .unwrap()
                .expiration_seconds,
            20
        );
        drop(release);
        server.await.unwrap();
        signaling.closed().await;
        assert!(signaling.credentials().await.is_err());
        assert!(signaling.recv().await.is_none());
    }

    /// Retained repository wire format: recipient is a string, not a JSON number.
    #[test]
    fn retained_signal_envelope_preserves_string_recipient() {
        let outgoing = outgoing_signal(&Signal {
            typ: "CONNECTREQUEST".into(),
            connection_id: 42,
            network_id: "123".into(),
            data: "v=0\r\n".into(),
        })
        .unwrap();
        assert_eq!(
            serde_json::to_value(outgoing).unwrap(),
            serde_json::json!({
                "Type": 1, "To": "123", "Message": "CONNECTREQUEST 42 v=0\r\n"
            })
        );
    }

    #[test]
    fn legacy_credentials_default_and_explicit_expiration() {
        let credentials = decode_credentials(
            r#"{"TurnAuthServers":[{"Username":"u","Password":"p","Urls":["turn:relay:3478"]}]}"#,
        )
        .unwrap();
        assert_eq!(credentials.credentials.expiration_seconds, 3600);
        let explicit =
            decode_credentials(r#"{"ExpirationInSeconds":42,"TurnAuthServers":[]}"#).unwrap();
        assert_eq!(explicit.credentials.expiration_seconds, 42);
        assert!(decode_credentials(r#"{"ExpirationInSeconds":-1,"TurnAuthServers":[]}"#).is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn credentials_expire_and_wait_for_shared_refresh() {
        use futures_util::FutureExt;
        let (sender, receiver) = watch::channel(Some(
            decode_credentials(r#"{"ExpirationInSeconds":1,"TurnAuthServers":[]}"#).unwrap(),
        ));
        assert_eq!(
            wait_credentials(receiver.clone())
                .await
                .unwrap()
                .unwrap()
                .expiration_seconds,
            1
        );
        tokio::time::advance(Duration::from_secs(2)).await;
        let mut waiting = Box::pin(wait_credentials(receiver.clone()));
        assert!(waiting.as_mut().now_or_never().is_none());
        sender
            .send(Some(
                decode_credentials(r#"{"ExpirationInSeconds":42,"TurnAuthServers":[]}"#).unwrap(),
            ))
            .unwrap();
        assert_eq!(waiting.await.unwrap().unwrap().expiration_seconds, 42);
        drop(sender);
        assert!(wait_credentials(receiver).await.is_err());
    }

    #[test]
    fn malformed_or_oversized_signals_are_rejected() {
        assert!(
            outgoing_signal(&Signal {
                typ: "UNKNOWN".into(),
                connection_id: 1,
                network_id: "1".into(),
                data: String::new()
            })
            .is_err()
        );
        assert!(
            outgoing_signal(&Signal {
                typ: "CONNECTREQUEST".into(),
                connection_id: 1,
                network_id: "1".into(),
                data: "x".repeat(MAX_FRAME_BYTES + 1)
            })
            .is_err()
        );
    }
}
