use std::marker::PhantomData;
use std::sync::Arc;
use std::net::SocketAddr;

use p384::SecretKey;
use tokio_raknet::transport::RaknetListener;

use crate::auth::ValidatedIdentity;
use crate::config::BedrockListenerConfig;
use crate::error::JolyneError;
use crate::stream::{BedrockStream, Handshake, ServerLogin, ServerPlay, StartGameConfig};
use crate::stream::server::ServerHandshakeConfig;
use crate::stream::transport::BedrockTransport;

pub struct BedrockListener {
    inner: RaknetListener,
    config: Arc<BedrockListenerConfig>,
    server_key: SecretKey,
    start_game_config: Arc<StartGameConfig>,
}

impl BedrockListener {
    pub async fn bind(addr: &str, config: BedrockListenerConfig, start_game_config: Option<StartGameConfig>) -> Result<Self, JolyneError> {
        let socket_addr: SocketAddr = addr.parse()
            .map_err(|e| JolyneError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))?;

        let inner = RaknetListener::bind(socket_addr)
            .await
            .map_err(|e| JolyneError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        // Generate persistent key for this listener
        let server_key = SecretKey::random(&mut rand::thread_rng());

        Ok(Self {
            inner,
            config: Arc::new(config),
            server_key,
            start_game_config: Arc::new(start_game_config.unwrap_or_default()),
        })
    }

    /// Accepts a connection and automatically performs the handshake, returning a ready-to-play stream.
    ///
    /// This method handles:
    /// 1. Handshake (Network Settings)
    /// 2. Login & Encryption
    /// 3. Resource Pack Negotiation (skips packs)
    /// 4. StartGame Sequence
    pub async fn accept(&mut self) -> Result<(ServerPlay, ValidatedIdentity), JolyneError> {
        let handshake_stream = self.accept_raw().await?;

        // 1. Network Settings
        let login_stream = handshake_stream.accept_network_settings().await?;

        // 2. Authentication
        let (secure_pending, identity) = login_stream.authenticate().await?;

        let handshake_config = ServerHandshakeConfig {
            server_key: self.server_key.clone(),
        };

        // 3. Finish Handshake (Encryption)
        let packs_stream = secure_pending.finish_handshake(&handshake_config, &identity.identity_public_key).await?;

        // 4. Resource Packs (Simple defaults for auto-accept)
        let start_stream = packs_stream.negotiate_packs(self.config.require_resource_packs).await?;

        // 5. Start Game
        let play_stream = start_stream.start_game(&self.start_game_config).await?;

        Ok((play_stream, identity))
    }

    /// Accepts a raw connection in the initial `Handshake` state.
    /// Use this if you need custom handshake logic (e.g. custom resource packs).
    pub async fn accept_raw(&mut self) -> Result<ServerLogin, JolyneError> {
        let stream = self
            .inner
            .accept()
            .await;
        
        let stream = stream.ok_or(JolyneError::ConnectionClosed)?;

        Ok(BedrockStream {
            transport: BedrockTransport::new(stream),
            state: Handshake { config: Some(self.config.clone()) },
            _role: PhantomData,
        })
    }

    pub fn local_addr(&self) -> Result<std::net::SocketAddr, JolyneError> {
        Ok(self.inner.local_addr())
    }
}
