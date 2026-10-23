//! Client join sequence over a transport.
//!
//! Each method drives one core phase from [`crate::proto::client`]. One
//! deadline ([`ClientConfig::join_timeout`]) spans the whole join.

use std::net::SocketAddr;
use std::time::Instant;

use p384::SecretKey;
use uuid::Uuid;

use super::driver;
use super::slots::Seat;
use super::transport::Transport;
use super::{
    BedrockStream, BedrockTransport, Client, Failed, Handshake, Login, PhaseOf, Play,
    ResourcePacks, SecurePending, StartGame,
};
use crate::config::ClientConfig;
use crate::error::{HandshakePhase, JolyneError, ProtocolError};
use crate::gamedata::GameData;
use crate::proto::client as core;
use crate::proto::conn::Connection;
use crate::proto::packs::DownloadedPack;
use crate::proto::phase::{Failure, Owner};

impl PhaseOf<Client> for Handshake {
    type Owner = core::ClientSettings;
}
impl PhaseOf<Client> for Login {
    type Owner = core::ClientLogin;
}
impl PhaseOf<Client> for SecurePending {
    type Owner = core::ClientHandshake;
}
impl PhaseOf<Client> for ResourcePacks {
    type Owner = core::ClientPacks;
}
impl PhaseOf<Client> for StartGame {
    type Owner = core::ClientSpawn;
}
impl PhaseOf<Client> for Play {
    type Owner = core::ClientSession;
}

/// A spawned client stream and the game data captured while joining.
pub type JoinedStream<T> = (BedrockStream<Play, Client, T>, GameData);

pub use crate::auth::client::{LoginCredentials, MultiplayerTokenSource};

/// Who the client logs in as.
#[derive(Debug, Clone)]
pub struct ClientHandshakeConfig {
    pub server_addr: SocketAddr,
    /// Signs the login tokens and drives the ECDH handshake.
    pub identity_key: SecretKey,
    pub display_name: String,
    pub uuid: Uuid,
    /// Login authentication; `Offline` unless built with [`Self::with_xbox_live`].
    pub credentials: LoginCredentials,
}

impl ClientHandshakeConfig {
    /// An offline identity with a random key and UUID.
    pub fn random(server_addr: SocketAddr, display_name: impl Into<String>) -> Self {
        Self {
            server_addr,
            identity_key: SecretKey::random(&mut rand::thread_rng()),
            display_name: display_name.into(),
            uuid: Uuid::new_v4(),
            credentials: LoginCredentials::Offline,
        }
    }

    /// An identity for online-mode servers: Login sends
    /// `AuthenticationType: 0` with a multiplayer token issued by `tokens`
    /// for `identity_key`.
    pub fn with_xbox_live(
        server_addr: SocketAddr,
        identity_key: SecretKey,
        display_name: impl Into<String>,
        uuid: Uuid,
        tokens: std::sync::Arc<dyn MultiplayerTokenSource>,
    ) -> Self {
        Self {
            server_addr,
            identity_key,
            display_name: display_name.into(),
            uuid,
            credentials: LoginCredentials::Online(tokens),
        }
    }
}

fn failed<T: Transport, O: Owner>(failure: Failure<O>, transport: T) -> Failed<T> {
    let (error, owner) = failure.into_parts();
    Failed::new(
        error,
        BedrockTransport::from_parts(transport, owner.into_connection(), Seat::Uncounted),
    )
}

async fn advance_to<S, T>(
    mut transport: T,
    mut owner: S::Owner,
) -> Result<BedrockStream<S, Client, T>, Failed<T>>
where
    S: PhaseOf<Client>,
    T: Transport,
{
    if let Err(error) = driver::flush_out(&mut transport, owner.connection()).await {
        return Err(failed(Failure::new(error, owner), transport));
    }
    Ok(BedrockStream::from_parts(transport, owner, Seat::Uncounted))
}

#[cfg(feature = "raknet")]
impl BedrockStream<Handshake, Client, super::RakNetTransport> {
    /// Connects over RakNet with default options.
    pub async fn connect(addr: SocketAddr) -> Result<Self, JolyneError> {
        Self::connect_with(addr, ClientConfig::default()).await
    }

    /// Connects over RakNet.
    pub async fn connect_with(addr: SocketAddr, config: ClientConfig) -> Result<Self, JolyneError> {
        let stream = tokio_raknet::RaknetStream::connect(addr)
            .await
            .map_err(JolyneError::transport)?;
        Self::from_transport(super::RakNetTransport::new(stream), config).await
    }
}

impl<T: Transport> BedrockStream<Handshake, Client, T> {
    /// Starts the client handshake on a connected transport by sending
    /// `RequestNetworkSettings`.
    pub async fn from_transport(
        mut transport: T,
        config: ClientConfig,
    ) -> Result<Self, JolyneError> {
        let conn = Connection::new(T::FRAMING, config.connection.clone());
        let mut owner = core::ClientSettings::new(conn, &config, Instant::now())?;
        driver::flush_out(&mut transport, owner.connection()).await?;
        Ok(Self::from_parts(transport, owner, Seat::Uncounted))
    }

    /// Waits for `NetworkSettings` and enables compression. A `PlayStatus`
    /// refusal surfaces as [`JolyneError::LoginRefused`].
    #[tracing::instrument(skip_all, level = "debug", fields(peer = %self.peer_addr()))]
    pub async fn request_settings(self) -> Result<BedrockStream<Login, Client, T>, Failed<T>> {
        let parts = self.take();
        let mut transport = parts.transport;
        match driver::drive(&mut transport, parts.owner).await {
            Ok(login) => advance_to(transport, login).await,
            Err(failure) => Err(failed(failure, transport)),
        }
    }

    /// Runs the whole join and returns the play stream plus the
    /// `StartGame`/`ItemRegistry` data. On failure the transport is closed.
    #[tracing::instrument(skip_all, level = "debug", fields(peer = %self.peer_addr()))]
    pub async fn join(self, config: ClientHandshakeConfig) -> Result<JoinedStream<T>, JolyneError> {
        async fn run<T: Transport>(
            stream: BedrockStream<Handshake, Client, T>,
            config: &ClientHandshakeConfig,
        ) -> Result<JoinedStream<T>, Failed<T>> {
            let login = stream.request_settings().await?;
            let secure = login.send_login(config).await?;
            let packs = secure.await_handshake().await?;
            let start = packs.handle_packs().await?;
            start.await_start_game().await
        }
        match run(self, &config).await {
            Ok(joined) => Ok(joined),
            Err(failed) => {
                let (error, link) = failed.into_parts();
                let _ = link.close().await;
                Err(error)
            }
        }
    }
}

impl<T: Transport> BedrockStream<Login, Client, T> {
    /// Builds the login tokens for `config` (a provider multiplayer token for
    /// online credentials, self-signed otherwise) and sends `Login`.
    #[tracing::instrument(skip_all, level = "debug", fields(peer = %self.peer_addr()))]
    pub async fn send_login(
        self,
        config: &ClientHandshakeConfig,
    ) -> Result<BedrockStream<SecurePending, Client, T>, Failed<T>> {
        let deadline = tokio::time::Instant::from_std(self.owner().deadline());
        let tokens = tokio::time::timeout_at(
            deadline,
            crate::auth::client::build_login(
                &config.identity_key,
                &config.display_name,
                config.uuid,
                &config.credentials,
            ),
        )
        .await;
        match tokens {
            Ok(Ok(tokens)) => {
                self.send_login_with_tokens(
                    &config.identity_key,
                    &tokens.auth_info_json,
                    &tokens.client_data_jwt,
                )
                .await
            }
            Ok(Err(error)) => Err(self.fail(error.into())),
            Err(_) => Err(self.fail(ProtocolError::Timeout(HandshakePhase::Login).into())),
        }
    }

    fn fail(self, error: JolyneError) -> Failed<T> {
        let parts = self.take();
        failed(Failure::new(error, parts.owner), parts.transport)
    }

    /// Sends `Login` with caller-built tokens. `identity_key` must be the key
    /// the tokens were signed with.
    #[tracing::instrument(skip_all, level = "debug", fields(peer = %self.peer_addr()))]
    pub async fn send_login_with_tokens(
        self,
        identity_key: &SecretKey,
        chain: &str,
        client_token: &str,
    ) -> Result<BedrockStream<SecurePending, Client, T>, Failed<T>> {
        let parts = self.take();
        let request = crate::auth::client::encode_login_request(chain, client_token);
        match parts
            .owner
            .login(identity_key.clone(), request.into(), Instant::now())
        {
            Ok(handshake) => advance_to(parts.transport, handshake).await,
            Err(failure) => Err(failed(failure, parts.transport)),
        }
    }
}

impl<T: Transport> BedrockStream<SecurePending, Client, T> {
    /// Completes the encryption handshake (server token pinned to ES384) and
    /// waits for `PlayStatus(LoginSuccess)`.
    #[tracing::instrument(skip_all, level = "debug", fields(peer = %self.peer_addr()))]
    pub async fn await_handshake(
        self,
    ) -> Result<BedrockStream<ResourcePacks, Client, T>, Failed<T>> {
        let parts = self.take();
        let mut transport = parts.transport;
        match driver::drive(&mut transport, parts.owner).await {
            Ok(packs) => advance_to(transport, packs).await,
            Err(failure) => Err(failed(failure, transport)),
        }
    }
}

impl<T: Transport> BedrockStream<ResourcePacks, Client, T> {
    /// Negotiates and downloads resource packs per the client's pack policy.
    #[tracing::instrument(skip_all, level = "debug", fields(peer = %self.peer_addr()))]
    pub async fn handle_packs(self) -> Result<BedrockStream<StartGame, Client, T>, Failed<T>> {
        let parts = self.take();
        let mut transport = parts.transport;
        match driver::drive(&mut transport, parts.owner).await {
            Ok(spawn) => advance_to(transport, spawn).await,
            Err(failure) => Err(failed(failure, transport)),
        }
    }
}

impl<T: Transport> BedrockStream<StartGame, Client, T> {
    /// Requests the chunk radius after `StartGame` and finishes spawning.
    /// Definition packets and anything else received meanwhile stay queued,
    /// in order, for the play session.
    #[tracing::instrument(skip_all, level = "debug", fields(peer = %self.peer_addr()))]
    pub async fn await_start_game(self) -> Result<JoinedStream<T>, Failed<T>> {
        let parts = self.take();
        let mut transport = parts.transport;
        match driver::drive(&mut transport, parts.owner).await {
            Ok(joined) => {
                let play = advance_to(transport, joined.session).await?;
                Ok((play, joined.game_data))
            }
            Err(failure) => Err(failed(failure, transport)),
        }
    }
}

impl<T: Transport> BedrockStream<Play, Client, T> {
    pub fn runtime_id(&self) -> u64 {
        self.owner().session.runtime_id()
    }

    /// The radius from the server's `ChunkRadiusUpdated`.
    pub fn chunk_radius(&self) -> Option<i32> {
        self.owner().session.chunk_radius()
    }

    /// Packs downloaded or loaded from cache during the join.
    pub fn resource_packs(&self) -> &[DownloadedPack] {
        &self.owner().packs
    }

    /// Whether the server marked its resource packs as required.
    pub fn resource_packs_required(&self) -> bool {
        self.owner().packs_required
    }
}
