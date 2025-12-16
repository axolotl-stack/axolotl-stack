use std::marker::PhantomData;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio_raknet::transport::RaknetListener;

use crate::config::BedrockListenerConfig;
use crate::error::JolyneError;
use crate::stream::transport::BedrockTransport;
use crate::stream::{BedrockStream, Handshake, ServerLogin};

pub struct BedrockListener {
    inner: RaknetListener,
    config: Arc<BedrockListenerConfig>,
}

impl BedrockListener {
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
            transport: BedrockTransport::new(stream),
            state: Handshake {
                config: Some(self.config.clone()),
            },
            _role: PhantomData,
        })
    }

    pub fn local_addr(&self) -> Result<std::net::SocketAddr, JolyneError> {
        Ok(self.inner.local_addr())
    }
}
