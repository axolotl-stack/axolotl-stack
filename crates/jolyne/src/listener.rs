//! Bedrock server listeners.
//!
//! ```ignore
//! let mut listener = BedrockListener::raknet().addr("0.0.0.0:19132").bind().await?;
//! let mut listener = BedrockListener::nethernet().lan("0.0.0.0:7551").bind().await?;
//! ```
//!
//! A listener counts joined players against `max_players` and publishes the
//! [`StatusProvider`](crate::status::StatusProvider) status (RakNet pong,
//! NetherNet LAN server data) at bind, every `status_refresh`, and whenever
//! a player joins or leaves. Refreshes run while `accept` is awaited.

use std::future::poll_fn;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use crate::config::BedrockListenerConfig;
use crate::error::JolyneError;
use crate::status::{ServerStatus, StaticStatus, StatusProvider};
use crate::stream::slots::PlayerSlots;
use crate::stream::transport::Transport;
use crate::stream::{BedrockStream, Handshake, Server};

/// A source of connected transports.
pub trait RawListener: Unpin + Send {
    type Transport: Transport;

    /// `Ready(None)` once the listener has shut down.
    fn poll_accept(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Transport>>;

    fn local_addr(&self) -> Option<SocketAddr>;

    /// Publishes `status` wherever this listener answers pings.
    fn publish_status(&self, _status: &ServerStatus, _server_guid: u64) -> Result<(), JolyneError> {
        Ok(())
    }
}

/// A server listener yielding connections in the [`Handshake`] phase.
pub struct BedrockListener<L: RawListener> {
    inner: L,
    config: Arc<BedrockListenerConfig>,
    slots: PlayerSlots,
    status: Arc<dyn StatusProvider>,
    /// GUID written into the RakNet pong string.
    server_guid: u64,
    next_refresh: tokio::time::Instant,
    #[cfg(feature = "discovery")]
    lan: Option<Arc<tokio_nethernet::discovery::DiscoveryListener>>,
}

impl<L: RawListener> BedrockListener<L> {
    /// Wraps a pre-built raw listener (custom signaling, custom transports).
    pub fn from_listener(inner: L, config: BedrockListenerConfig) -> Self {
        let status = config
            .status
            .clone()
            .unwrap_or_else(|| Arc::new(StaticStatus::new("Jolyne Server", "Jolyne")));
        Self {
            inner,
            slots: PlayerSlots::new(config.max_players),
            config: Arc::new(config),
            status,
            server_guid: rand::random(),
            next_refresh: tokio::time::Instant::now(),
            #[cfg(feature = "discovery")]
            lan: None,
        }
    }

    /// The status the listener currently advertises.
    pub fn status(&self) -> ServerStatus {
        self.status.status(self.slots.count(), self.slots.max())
    }

    /// Recomputes and publishes the status now.
    pub async fn refresh_status(&self) -> Result<(), JolyneError> {
        let status = self.status();
        self.inner.publish_status(&status, self.server_guid)?;
        #[cfg(feature = "discovery")]
        if let Some(lan) = &self.lan {
            lan.set_server_data(status.lan_server_data()).await;
        }
        Ok(())
    }

    /// Players currently counted (from `Login` until the connection drops).
    pub fn player_count(&self) -> usize {
        self.slots.count()
    }

    pub fn config(&self) -> &Arc<BedrockListenerConfig> {
        &self.config
    }

    /// Accepts the next connection. Status refreshes run while waiting;
    /// a failed refresh is logged, never surfaced as an accept error.
    pub async fn accept(
        &mut self,
    ) -> Result<BedrockStream<Handshake, Server, L::Transport>, JolyneError> {
        let refresh = self
            .config
            .status_refresh
            .max(std::time::Duration::from_millis(100));
        loop {
            let slots = self.slots.clone();
            tokio::select! {
                transport = poll_fn(|cx| Pin::new(&mut self.inner).poll_accept(cx)) => {
                    let transport = transport.ok_or(JolyneError::ConnectionClosed)?;
                    return Ok(BedrockStream::listener_accepted(
                        transport,
                        self.config.clone(),
                        self.slots.clone(),
                    ));
                }
                _ = tokio::time::sleep_until(self.next_refresh) => {}
                _ = slots.changed() => {}
            }
            self.next_refresh = tokio::time::Instant::now() + refresh;
            if let Err(error) = self.refresh_status().await {
                tracing::warn!(%error, "publishing server status failed");
            }
        }
    }

    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.inner.local_addr()
    }

    pub fn inner(&self) -> &L {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut L {
        &mut self.inner
    }
}

/// Builder entry point type (`BedrockListener::raknet()`, `::nethernet()`).
pub struct NoListener;

#[cfg(feature = "raknet")]
impl BedrockListener<tokio_raknet::transport::RaknetListener> {
    /// Starts building a RakNet (UDP) listener.
    pub fn raknet() -> RakNetBuilder {
        RakNetBuilder {
            addr: None,
            config: BedrockListenerConfig::default(),
            raknet: tokio_raknet::transport::RaknetListenerConfig::default(),
        }
    }
}

#[cfg(feature = "nethernet")]
impl BedrockListener<tokio_nethernet::NetherNetListener> {
    /// Starts building a NetherNet (WebRTC) listener.
    pub fn nethernet() -> NetherNetBuilder {
        NetherNetBuilder {
            signaling: NetherNetSignaling::None,
            config: BedrockListenerConfig {
                // DTLS already encrypts NetherNet.
                encryption_enabled: false,
                ..Default::default()
            },
            #[cfg(any(feature = "xbox-signaling", feature = "endpoint"))]
            signal_monitor_config: None,
        }
    }
}

#[cfg(all(
    feature = "server",
    not(any(feature = "raknet", feature = "nethernet"))
))]
compile_error!(
    "The `server` feature requires at least one transport feature: `raknet` or `nethernet`"
);

/// Builder for RakNet listeners.
#[cfg(feature = "raknet")]
pub struct RakNetBuilder {
    addr: Option<String>,
    config: BedrockListenerConfig,
    raknet: tokio_raknet::transport::RaknetListenerConfig,
}

#[cfg(feature = "raknet")]
impl RakNetBuilder {
    /// Bind address, e.g. `"0.0.0.0:19132"`. Required.
    pub fn addr(mut self, addr: impl Into<String>) -> Self {
        self.addr = Some(addr.into());
        self
    }

    pub fn config(mut self, config: BedrockListenerConfig) -> Self {
        self.config = config;
        self
    }

    /// RakNet session options (timeouts, MTU, connection caps).
    pub fn raknet_config(mut self, raknet: tokio_raknet::transport::RaknetListenerConfig) -> Self {
        self.raknet = raknet;
        self
    }

    pub async fn bind(
        self,
    ) -> Result<BedrockListener<tokio_raknet::transport::RaknetListener>, JolyneError> {
        let addr = self.addr.ok_or_else(|| {
            JolyneError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "RakNet listener requires .addr() before .bind()",
            ))
        })?;
        let addr: SocketAddr = addr.parse().map_err(|error| {
            JolyneError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, error))
        })?;
        let inner =
            tokio_raknet::transport::RaknetListener::bind_with_config(addr, self.raknet).await?;
        let listener = BedrockListener::from_listener(inner, self.config);
        listener.refresh_status().await?;
        Ok(listener)
    }
}

/// Builder for NetherNet listeners.
#[cfg(feature = "nethernet")]
pub struct NetherNetBuilder {
    signaling: NetherNetSignaling,
    config: BedrockListenerConfig,
    #[cfg(any(feature = "xbox-signaling", feature = "endpoint"))]
    signal_monitor_config: Option<tokio_nethernet::SignalMonitorConfig>,
}

#[cfg(feature = "nethernet")]
enum NetherNetSignaling {
    None,
    #[cfg(feature = "discovery")]
    Lan {
        addr: String,
    },
    #[cfg(feature = "endpoint")]
    Endpoint {
        handler: Arc<tokio_nethernet::endpoint::Handler>,
        config: tokio_nethernet::NetherNetListenerConfig,
    },
    #[cfg(feature = "xbox-signaling")]
    Xbox {
        signaling_url: String,
        nethernet_id: u64,
        mc_token: String,
    },
}

#[cfg(feature = "nethernet")]
impl NetherNetBuilder {
    /// LAN discovery signaling. The listener answers discovery requests with
    /// the status provider's server data. Identityless LAN clients are
    /// allowed; Bedrock login still authenticates them.
    #[cfg(feature = "discovery")]
    pub fn lan(mut self, addr: impl Into<String>) -> Self {
        self.signaling = NetherNetSignaling::Lan { addr: addr.into() };
        self
    }

    /// HTTP `/v1/join` signaling.
    #[cfg(feature = "endpoint")]
    pub fn endpoint(
        mut self,
        handler: Arc<tokio_nethernet::endpoint::Handler>,
        config: tokio_nethernet::NetherNetListenerConfig,
    ) -> Self {
        self.signaling = NetherNetSignaling::Endpoint { handler, config };
        self
    }

    /// Franchise WebSocket signaling for friend joins.
    #[cfg(feature = "xbox-signaling")]
    pub fn xbox(
        mut self,
        signaling_url: impl Into<String>,
        nethernet_id: u64,
        mc_token: impl Into<String>,
    ) -> Self {
        self.signaling = NetherNetSignaling::Xbox {
            signaling_url: signaling_url.into(),
            nethernet_id,
            mc_token: mc_token.into(),
        };
        self
    }

    /// Bedrock configuration (defaults to encryption off; DTLS encrypts).
    pub fn config(mut self, config: BedrockListenerConfig) -> Self {
        self.config = config;
        self
    }

    /// Signal-level monitoring for endpoint/Xbox negotiations.
    #[cfg(any(feature = "xbox-signaling", feature = "endpoint"))]
    pub fn with_signal_monitoring(mut self, config: tokio_nethernet::SignalMonitorConfig) -> Self {
        self.signal_monitor_config = Some(config);
        self
    }

    pub async fn bind(
        self,
    ) -> Result<BedrockListener<tokio_nethernet::NetherNetListener>, JolyneError> {
        #[cfg(feature = "discovery")]
        let lan: Option<Arc<tokio_nethernet::discovery::DiscoveryListener>>;
        let inner = match self.signaling {
            NetherNetSignaling::None => {
                return Err(JolyneError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "NetherNet listener requires .lan(), .xbox() or .endpoint() before .bind()",
                )));
            }
            #[cfg(feature = "discovery")]
            NetherNetSignaling::Lan { addr } => {
                use tokio_nethernet::discovery::{DiscoveryListener, DiscoveryListenerConfig};
                use tokio_nethernet::{NetherNetListener, NetherNetListenerConfig};
                let discovery = DiscoveryListener::bind(&addr, DiscoveryListenerConfig::default())
                    .await
                    .map_err(JolyneError::Io)?;
                lan = Some(discovery.clone());
                NetherNetListener::bind_with_signaling(
                    discovery,
                    NetherNetListenerConfig {
                        allow_identityless_client: true,
                        ..Default::default()
                    },
                )
            }
            #[cfg(feature = "endpoint")]
            NetherNetSignaling::Endpoint {
                handler,
                mut config,
            } => {
                use tokio_nethernet::{NetherNetListener, SignalMonitor};
                #[cfg(feature = "discovery")]
                {
                    lan = None;
                }
                if let Some(monitor) = self.signal_monitor_config {
                    config.signal_monitor = Some(SignalMonitor::new_with_cleanup(monitor));
                }
                NetherNetListener::bind_with_signaling(handler, config)
            }
            #[cfg(feature = "xbox-signaling")]
            NetherNetSignaling::Xbox {
                signaling_url,
                nethernet_id,
                mc_token,
            } => {
                use tokio_nethernet::{NetherNetListener, NetherNetListenerConfig, XboxSignaling};
                #[cfg(feature = "discovery")]
                {
                    lan = None;
                }
                let xbox = XboxSignaling::connect_with_url(
                    signaling_url,
                    nethernet_id,
                    &mc_token,
                    self.signal_monitor_config.unwrap_or_default(),
                )
                .await
                .map_err(|error| JolyneError::Transport(error.into()))?;
                NetherNetListener::bind_with_signaling(xbox, NetherNetListenerConfig::default())
            }
        };
        #[allow(unused_mut)]
        let mut listener = BedrockListener::from_listener(inner, self.config);
        #[cfg(feature = "discovery")]
        {
            listener.lan = lan;
        }
        listener.refresh_status().await?;
        Ok(listener)
    }
}

#[cfg(feature = "raknet")]
mod raknet_impl {
    use super::*;
    use crate::stream::transport::RakNetTransport;
    use tokio_raknet::transport::RaknetListener;

    impl RawListener for RaknetListener {
        type Transport = RakNetTransport;

        fn poll_accept(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Self::Transport>> {
            RaknetListener::poll_accept(&mut self, cx)
                .map(|stream| stream.map(RakNetTransport::new))
        }

        fn local_addr(&self) -> Option<SocketAddr> {
            Some(RaknetListener::local_addr(self))
        }

        fn publish_status(
            &self,
            status: &ServerStatus,
            server_guid: u64,
        ) -> Result<(), JolyneError> {
            let port = RaknetListener::local_addr(self).port();
            self.set_advertisement(status.raknet_pong(server_guid, port).into_bytes())
                .map_err(JolyneError::transport)
        }
    }
}

#[cfg(feature = "nethernet")]
mod nethernet_impl {
    use super::*;
    use crate::stream::transport::NetherNetTransport;
    use tokio_nethernet::NetherNetListener;

    impl RawListener for NetherNetListener {
        type Transport = NetherNetTransport;

        fn poll_accept(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Self::Transport>> {
            NetherNetListener::poll_accept(&mut self, cx)
                .map(|stream| stream.map(NetherNetTransport::new))
        }

        fn local_addr(&self) -> Option<SocketAddr> {
            None
        }
    }
}
