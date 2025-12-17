//! Bedrock RakNet listener.
//!
//! Accepts incoming RakNet connections and wraps them as `BedrockStream`.

use std::marker::PhantomData;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio_raknet::transport::RaknetListener;

use crate::config::BedrockListenerConfig;
use crate::error::JolyneError;
use crate::stream::transport::{BedrockTransport, RakNetTransport};
use crate::stream::{BedrockStream, Handshake, ServerLogin};

/// A Bedrock server listener that accepts RakNet connections.
pub struct BedrockListener {
    inner: RaknetListener,
    config: Arc<BedrockListenerConfig>,
}

impl BedrockListener {
    /// Bind to an address and start listening for RakNet connections.
    pub async fn bind(addr: &str, config: BedrockListenerConfig) -> Result<Self, JolyneError> {
        let socket_addr: SocketAddr = addr.parse().map_err(|e| {
            JolyneError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
        })?;

        let inner = RaknetListener::bind(socket_addr).await?;

        Ok(Self {
            inner,
            config: Arc::new(config),
        })
    }

    /// Accepts a connection in the initial Handshake state.
    /// This connection has not yet negotiated network settings or authenticated.
    pub async fn accept(&mut self) -> Result<ServerLogin, JolyneError> {
        let stream = self.inner.accept().await;

        let stream = stream.ok_or(JolyneError::ConnectionClosed)?;

        Ok(BedrockStream {
            transport: BedrockTransport::new(RakNetTransport::new(stream)),
            state: Handshake {
                config: Some(self.config.clone()),
            },
            _role: PhantomData,
        })
    }

    /// Returns the local address the listener is bound to.
    pub fn local_addr(&self) -> Result<std::net::SocketAddr, JolyneError> {
        Ok(self.inner.local_addr())
    }
}
