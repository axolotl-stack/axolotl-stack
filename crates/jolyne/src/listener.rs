//! Bedrock protocol listener with builder API.
//!
//! Use [`BedrockListener::raknet()`] or [`BedrockListener::nethernet()`] to start
//! building a listener, then call `.bind()` to actually bind the socket.
//!
//! # Examples
//!
//! ## RakNet (Traditional UDP)
//! ```ignore
//! let mut listener = BedrockListener::raknet()
//!     .addr("0.0.0.0:19132")
//!     .bind()
//!     .await?;
//! ```
//!
//! ## NetherNet with LAN Discovery
//! ```ignore
//! let mut listener = BedrockListener::nethernet()
//!     .lan("0.0.0.0:7551")
//!     .bind()
//!     .await?;
//! ```
//!
//! ## NetherNet with Xbox Live
//! ```ignore
//! let mut listener = BedrockListener::nethernet()
//!     .xbox(nethernet_id, mc_token)
//!     .bind()
//!     .await?;
//! ```

use std::future::poll_fn;
use std::marker::PhantomData;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use crate::config::BedrockListenerConfig;
use crate::error::JolyneError;
use crate::stream::transport::{BedrockTransport, Transport};
use crate::stream::{BedrockStream, Handshake, Server};

// ============================================================================
// RawListener Trait
// ============================================================================

/// Trait for raw connection listeners that can be wrapped by [`BedrockListener`].
///
/// This abstracts over different transport mechanisms:
/// - `tokio_raknet::transport::RaknetListener` (UDP/RakNet)
/// - `tokio_nethernet::NetherNetListener` (WebRTC/NetherNet)
///
/// Uses poll-based API for consistency with [`Transport`] trait.
pub trait RawListener: Unpin + Send {
    /// The transport type this listener yields.
    type Transport: Transport;

    /// Poll for the next incoming connection.
    ///
    /// Returns `Poll::Ready(Some(transport))` when a connection is available,
    /// `Poll::Ready(None)` when the listener has shut down,
    /// or `Poll::Pending` if no connection is ready yet.
    fn poll_accept(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Transport>>;

    /// Returns the local bind address, if applicable.
    fn local_addr(&self) -> Option<SocketAddr>;
}

// ============================================================================
// BedrockListener<L>
// ============================================================================

/// A Bedrock protocol server listener, generic over the transport.
///
/// Created via the builder pattern:
/// - [`BedrockListener::raknet()`] for RakNet (UDP)
/// - [`BedrockListener::nethernet()`] for NetherNet (WebRTC)
pub struct BedrockListener<L: RawListener> {
    inner: L,
    config: Arc<BedrockListenerConfig>,
}

impl<L: RawListener> BedrockListener<L> {
    /// Creates a `BedrockListener` from an existing raw listener.
    ///
    /// This is the escape hatch for advanced usage, such as:
    /// - Pre-configured `NetherNetListener` with custom signaling
    /// - Custom listener implementations
    ///
    /// # Example
    /// ```ignore
    /// let (nn_listener, signal_tx) = NetherNetListener::new(xbox_signaling, config);
    /// // ... spawn signal pump ...
    /// let listener = BedrockListener::from_listener(nn_listener, bedrock_config);
    /// ```
    pub fn from_listener(inner: L, config: BedrockListenerConfig) -> Self {
        Self {
            inner,
            config: Arc::new(config),
        }
    }

    /// Accepts the next connection in the initial Handshake state.
    ///
    /// The returned stream has not yet negotiated network settings or authenticated.
    /// Call `accept_network_settings()` or `accept_join_sequence()` to proceed.
    pub async fn accept(
        &mut self,
    ) -> Result<BedrockStream<Handshake, Server, L::Transport>, JolyneError> {
        let transport = poll_fn(|cx| Pin::new(&mut self.inner).poll_accept(cx))
            .await
            .ok_or(JolyneError::ConnectionClosed)?;

        Ok(BedrockStream {
            transport: BedrockTransport::new(transport),
            state: Handshake {
                config: Some(self.config.clone()),
            },
            _role: PhantomData,
        })
    }

    /// Returns the local address if applicable.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.inner.local_addr()
    }

    /// Returns a reference to the underlying raw listener.
    ///
    /// Useful for transport-specific operations like `set_advertisement()` on RakNet.
    pub fn inner(&self) -> &L {
        &self.inner
    }

    /// Returns a mutable reference to the underlying raw listener.
    pub fn inner_mut(&mut self) -> &mut L {
        &mut self.inner
    }
}

// ============================================================================
// Builder Entry Points
// ============================================================================

/// Dummy type for builder entry point.
/// This allows `BedrockListener::raknet()` syntax without a real generic.
pub struct NoListener;

impl BedrockListener<NoListener> {
    /// Start building a RakNet listener.
    ///
    /// RakNet is the traditional UDP-based transport for Minecraft Bedrock.
    ///
    /// # Example
    /// ```ignore
    /// let listener = BedrockListener::raknet()
    ///     .addr("0.0.0.0:19132")
    ///     .bind()
    ///     .await?;
    /// ```
    #[cfg(feature = "raknet")]
    pub fn raknet() -> RakNetBuilder {
        RakNetBuilder::new()
    }

    /// Start building a NetherNet listener.
    ///
    /// NetherNet uses WebRTC for transport, with signaling via LAN discovery or Xbox Live.
    ///
    /// # Example
    /// ```ignore
    /// let listener = BedrockListener::nethernet()
    ///     .lan("0.0.0.0:7551")
    ///     .bind()
    ///     .await?;
    /// ```
    #[cfg(feature = "nethernet")]
    pub fn nethernet() -> NetherNetBuilder {
        NetherNetBuilder::new()
    }
}

// Implement RawListener for NoListener to satisfy trait bounds (never actually used)
impl RawListener for NoListener {
    type Transport = crate::stream::transport::RakNetTransport; // Placeholder

    fn poll_accept(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Transport>> {
        Poll::Ready(None)
    }

    fn local_addr(&self) -> Option<SocketAddr> {
        None
    }
}

// ============================================================================
// RakNet Builder
// ============================================================================

/// Builder for RakNet listeners.
///
/// Created via [`BedrockListener::raknet()`].
#[cfg(feature = "raknet")]
pub struct RakNetBuilder {
    addr: Option<String>,
    config: BedrockListenerConfig,
}

#[cfg(feature = "raknet")]
impl RakNetBuilder {
    fn new() -> Self {
        Self {
            addr: None,
            config: BedrockListenerConfig::default(),
        }
    }

    /// Set the bind address (e.g., "0.0.0.0:19132").
    ///
    /// This is required before calling `.bind()`.
    pub fn addr(mut self, addr: impl Into<String>) -> Self {
        self.addr = Some(addr.into());
        self
    }

    /// Set the Bedrock protocol configuration.
    ///
    /// If not called, uses [`BedrockListenerConfig::default()`].
    pub fn config(mut self, config: BedrockListenerConfig) -> Self {
        self.config = config;
        self
    }

    /// Bind the listener.
    ///
    /// This is when the UDP socket is actually opened and starts accepting connections.
    pub async fn bind(
        self,
    ) -> Result<BedrockListener<tokio_raknet::transport::RaknetListener>, JolyneError> {
        use tokio_raknet::transport::RaknetListener;

        let addr = self.addr.ok_or_else(|| {
            JolyneError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "RakNet listener requires .addr() to be set before .bind()",
            ))
        })?;

        let socket_addr: SocketAddr = addr.parse().map_err(|e| {
            JolyneError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
        })?;

        let inner = RaknetListener::bind(socket_addr).await?;
        Ok(BedrockListener {
            inner,
            config: Arc::new(self.config),
        })
    }
}

// ============================================================================
// NetherNet Builder
// ============================================================================

/// Builder for NetherNet listeners.
///
/// Created via [`BedrockListener::nethernet()`].
#[cfg(feature = "nethernet")]
pub struct NetherNetBuilder {
    signaling: NetherNetSignaling,
    config: BedrockListenerConfig,
}

#[cfg(feature = "nethernet")]
enum NetherNetSignaling {
    None,
    #[cfg(feature = "discovery")]
    Lan {
        addr: String,
    },
    #[cfg(feature = "xbox-signaling")]
    Xbox {
        nethernet_id: u64,
        mc_token: String,
    },
}

#[cfg(feature = "nethernet")]
impl NetherNetBuilder {
    fn new() -> Self {
        Self {
            signaling: NetherNetSignaling::None,
            config: BedrockListenerConfig {
                // NetherNet uses DTLS for transport encryption, so disable Bedrock encryption
                encryption_enabled: false,
                ..Default::default()
            },
        }
    }

    /// Use LAN discovery signaling.
    ///
    /// Binds a UDP socket for encrypted LAN discovery broadcasts.
    /// This is how players on the same local network find your server.
    ///
    /// # Arguments
    /// * `addr` - The address to bind for discovery (e.g., "0.0.0.0:7551")
    #[cfg(feature = "discovery")]
    pub fn lan(mut self, addr: impl Into<String>) -> Self {
        self.signaling = NetherNetSignaling::Lan { addr: addr.into() };
        self
    }

    /// Use Xbox Live signaling.
    ///
    /// Connects to Xbox's signaling WebSocket for friend-to-friend connections.
    /// Players join via the Xbox friends list / "Join Game" button.
    ///
    /// # Arguments
    /// * `nethernet_id` - Your NetherNet network ID
    /// * `mc_token` - Minecraft authorization token from PlayFab session
    #[cfg(feature = "xbox-signaling")]
    pub fn xbox(mut self, nethernet_id: u64, mc_token: impl Into<String>) -> Self {
        self.signaling = NetherNetSignaling::Xbox {
            nethernet_id,
            mc_token: mc_token.into(),
        };
        self
    }

    /// Set the Bedrock protocol configuration.
    ///
    /// If not called, uses a default config with encryption disabled
    /// (since NetherNet uses DTLS for transport encryption).
    pub fn config(mut self, config: BedrockListenerConfig) -> Self {
        self.config = config;
        self
    }

    /// Bind the listener.
    ///
    /// This is when the signaling channel connects and starts accepting connections.
    pub async fn bind(
        self,
    ) -> Result<BedrockListener<tokio_nethernet::NetherNetListener>, JolyneError> {
        let inner = match self.signaling {
            NetherNetSignaling::None => {
                return Err(JolyneError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "NetherNet listener requires .lan() or .xbox() signaling before .bind()",
                )));
            }
            #[cfg(feature = "discovery")]
            NetherNetSignaling::Lan { addr } => {
                use tokio_nethernet::discovery::{DiscoveryListener, DiscoveryListenerConfig};
                use tokio_nethernet::{NetherNetListener, NetherNetListenerConfig};
                let discovery = DiscoveryListener::bind(&addr, DiscoveryListenerConfig::default())
                    .await
                    .map_err(JolyneError::Io)?;
                NetherNetListener::bind_with_signaling(
                    discovery,
                    NetherNetListenerConfig::default(),
                )
            }
            #[cfg(feature = "xbox-signaling")]
            NetherNetSignaling::Xbox {
                nethernet_id,
                mc_token,
            } => {
                use tokio_nethernet::{NetherNetListener, NetherNetListenerConfig, XboxSignaling};
                let xbox = XboxSignaling::connect(nethernet_id, &mc_token)
                    .await
                    .map_err(|e| JolyneError::Transport(e.to_string()))?;
                NetherNetListener::bind_with_signaling(xbox, NetherNetListenerConfig::default())
            }
        };

        Ok(BedrockListener {
            inner,
            config: Arc::new(self.config),
        })
    }
}

// ============================================================================
// RawListener Implementations
// ============================================================================

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
            // Use the inherent method on RaknetListener, not the trait method
            match RaknetListener::poll_accept(&mut *self, cx) {
                Poll::Ready(Some(stream)) => Poll::Ready(Some(RakNetTransport::new(stream))),
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            }
        }

        fn local_addr(&self) -> Option<SocketAddr> {
            Some(RaknetListener::local_addr(self))
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
            // Use the inherent method on NetherNetListener, not the trait method
            match NetherNetListener::poll_accept(&mut *self, cx) {
                Poll::Ready(Some(stream)) => Poll::Ready(Some(NetherNetTransport::new(stream))),
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            }
        }

        fn local_addr(&self) -> Option<SocketAddr> {
            None // NetherNet uses signaling, no traditional bind address
        }
    }
}
