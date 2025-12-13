use crate::config::BedrockListenerConfig;
use crate::error::JolyneError;
use crate::stream::{BedrockStream, ConnectionSide};
use std::net::SocketAddr;
use tokio_raknet::transport::RaknetListener;

pub struct BedrockListener {
    inner: RaknetListener,
    #[allow(dead_code)]
    config: BedrockListenerConfig,
}

impl BedrockListener {
    pub async fn bind(addr: SocketAddr) -> Result<Self, JolyneError> {
        Self::bind_with_config(addr, BedrockListenerConfig::default()).await
    }

    pub async fn bind_with_config(
        addr: SocketAddr,
        config: BedrockListenerConfig,
    ) -> Result<Self, JolyneError> {
        let inner = RaknetListener::bind(addr).await?;
        Ok(Self { inner, config })
    }

    pub async fn accept(&mut self) -> Result<BedrockStream, JolyneError> {
        // RaknetListener::accept returns Option<RaknetStream>
        let stream = self
            .inner
            .accept()
            .await
            .ok_or(JolyneError::ListenerClosed)?;

        // Pass config to stream
        let bstream = BedrockStream::new(stream, ConnectionSide::Server, self.config.clone());
        Ok(bstream)
    }

    pub fn local_addr(&self) -> Result<SocketAddr, JolyneError> {
        Ok(self.inner.local_addr())
    }

    pub fn config(&self) -> &BedrockListenerConfig {
        &self.config
    }
}
