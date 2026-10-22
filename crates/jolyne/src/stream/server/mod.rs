//! Server join sequence over a transport.
//!
//! Each method drives one core phase from [`crate::proto::server`] and
//! returns the next typestate, or [`Failed`] still owning the connection.
//! [`BedrockStream::accept_join_sequence`] runs all of them and sends the
//! reasoned `Disconnect` on failure.

use std::sync::Arc;
use std::time::Instant;

use p384::SecretKey;

use super::driver;
use super::slots::{PlayerSlots, Seat};
use super::transport::Transport;
use super::{
    BedrockStream, BedrockTransport, Failed, Handshake, Login, PhaseOf, Play, ResourcePacks,
    SecurePending, Server, StartGame,
};
use crate::auth::AuthenticatedLogin;
use crate::auth::{ClientData, ValidatedIdentity};
use crate::config::BedrockListenerConfig;
use crate::error::{HandshakePhase, JolyneError, ProtocolError};
use crate::proto::conn::Connection;
use crate::proto::packs::ServerPacks;
use crate::proto::phase::{Failure, Owner, Step};
use crate::proto::server as core;
use crate::world::{WorldJoinParams, WorldTemplate};

impl PhaseOf<Server> for Handshake {
    type Owner = core::ServerHandshake;
}
impl PhaseOf<Server> for Login {
    type Owner = core::ServerLogin;
}
impl PhaseOf<Server> for SecurePending {
    type Owner = core::Authenticated;
}
impl PhaseOf<Server> for ResourcePacks {
    type Owner = core::PacksReady;
}
impl PhaseOf<Server> for StartGame {
    type Owner = core::WorldReady;
}
impl PhaseOf<Server> for Play {
    type Owner = core::ServerSession;
}

/// Decodes and verifies a `LoginPacket.connection_request`.
pub(crate) async fn verify_login(
    connection_request: &[u8],
    online_mode: bool,
    allow_legacy: bool,
) -> Result<AuthenticatedLogin, JolyneError> {
    let (auth_info, client_data) = crate::auth::decode_login_request(connection_request)?;
    Ok(
        crate::auth::authenticate_login(&auth_info, &client_data, online_mode, allow_legacy)
            .await?,
    )
}

/// A verified player and its stream, as returned by `authenticate`.
pub type AuthenticatedStream<T> = (BedrockStream<SecurePending, Server, T>, AuthenticatedLogin);

/// A spawned player and its login, as returned by `accept_join_sequence`.
pub type JoinedStream<T> = (BedrockStream<Play, Server, T>, AuthenticatedLogin);

/// Key material for the encryption handshake.
#[derive(Debug, Clone)]
pub struct ServerHandshakeConfig {
    /// The server's P-384 key; reuse one per listener.
    pub server_key: SecretKey,
}

fn failed<T: Transport, O: Owner>(failure: Failure<O>, transport: T, seat: Seat) -> Failed<T> {
    let (error, owner) = failure.into_parts();
    Failed::new(
        error,
        BedrockTransport::from_parts(transport, owner.into_connection(), seat),
    )
}

/// Flushes the new owner's queued output and wraps it in the next typestate.
async fn advance_to<S, T>(
    mut transport: T,
    mut owner: S::Owner,
    seat: Seat,
) -> Result<BedrockStream<S, Server, T>, Failed<T>>
where
    S: PhaseOf<Server>,
    T: Transport,
{
    if let Err(error) = driver::flush_out(&mut transport, owner.connection()).await {
        return Err(failed(Failure::new(error, owner), transport, seat));
    }
    Ok(BedrockStream::from_parts(transport, owner, seat))
}

impl<T: Transport> BedrockStream<Handshake, Server, T> {
    /// Starts the server handshake on a connected transport. Without a
    /// listener, players are not counted against `max_players`.
    pub fn from_transport(transport: T, config: Arc<BedrockListenerConfig>) -> Self {
        Self::accepted(transport, config, Seat::Uncounted)
    }

    pub(crate) fn accepted(transport: T, config: Arc<BedrockListenerConfig>, seat: Seat) -> Self {
        let conn = Connection::new(T::FRAMING, config.connection.clone());
        let owner = core::ServerHandshake::new(conn, config, Instant::now());
        Self::from_parts(transport, owner, seat)
    }

    pub(crate) fn listener_accepted(
        transport: T,
        config: Arc<BedrockListenerConfig>,
        slots: PlayerSlots,
    ) -> Self {
        Self::accepted(transport, config, Seat::Pool(slots))
    }

    /// Waits for `RequestNetworkSettings`, checks the protocol and sends
    /// `NetworkSettings` (compression starts after it).
    #[tracing::instrument(skip_all, level = "debug", fields(peer = %self.peer_addr()))]
    pub async fn accept_network_settings(
        self,
    ) -> Result<BedrockStream<Login, Server, T>, Failed<T>> {
        let super::Parts {
            mut transport,
            owner,
            seat,
        } = self.take();
        match driver::drive(&mut transport, owner).await {
            Ok(login) => advance_to(transport, login, seat).await,
            Err(failure) => Err(failed(failure, transport, seat)),
        }
    }

    /// Runs the whole join with defaults: settings, login, encryption, the
    /// configured packs and `template`. On failure the client is sent a
    /// reasoned `Disconnect` before the error is returned.
    #[tracing::instrument(skip_all, level = "debug", fields(peer = %self.peer_addr()))]
    pub async fn accept_join_sequence(
        self,
        template: &WorldTemplate,
        server_key: &SecretKey,
    ) -> Result<JoinedStream<T>, JolyneError> {
        async fn run<T: Transport>(
            stream: BedrockStream<Handshake, Server, T>,
            template: &WorldTemplate,
            server_key: &SecretKey,
        ) -> Result<JoinedStream<T>, Failed<T>> {
            let login = stream.accept_network_settings().await?;
            let (secure, identity) = login.authenticate().await?;
            let packs = secure
                .finish_handshake(&ServerHandshakeConfig {
                    server_key: server_key.clone(),
                })
                .await?;
            let start = packs.negotiate_packs().await?;
            let play = start
                .start_game(template.to_join_params(rand::random()))
                .await?;
            Ok((play, identity))
        }
        match run(self, template, server_key).await {
            Ok(joined) => Ok(joined),
            Err(failed) => Err(failed.disconnect().await),
        }
    }
}

impl<T: Transport> BedrockStream<Login, Server, T> {
    /// Waits for `Login`, enforces `max_players`, verifies the login and runs
    /// the admission hook. The whole step shares the login deadline.
    #[tracing::instrument(skip_all, level = "debug", fields(peer = %self.peer_addr()))]
    pub async fn authenticate(self) -> Result<AuthenticatedStream<T>, Failed<T>> {
        let super::Parts {
            mut transport,
            owner,
            seat,
        } = self.take();
        let peer = transport.peer_addr();
        let request = match driver::drive(&mut transport, owner).await {
            Ok(request) => request,
            Err(failure) => return Err(failed(failure, transport, seat)),
        };
        let seat = match seat {
            Seat::Pool(slots) => match slots.try_acquire() {
                Some(slot) => Seat::Held { _slot: slot },
                None => {
                    let failure = request.refuse_server_full(Instant::now());
                    return Err(failed(failure, transport, Seat::Uncounted));
                }
            },
            other => other,
        };
        let config = request.config().clone();
        let deadline = tokio::time::Instant::from_std(request.deadline());
        let verified = tokio::time::timeout_at(
            deadline,
            verify_login(
                request.connection_request(),
                config.online_mode,
                config.allow_legacy_auth,
            ),
        )
        .await;
        let login = match verified {
            Ok(Ok(login)) => login,
            Ok(Err(error)) => return Err(failed(Failure::new(error, request), transport, seat)),
            Err(_) => {
                let error = ProtocolError::Timeout(HandshakePhase::Authentication);
                return Err(failed(Failure::new(error, request), transport, seat));
            }
        };
        match request.authenticated(login.clone(), peer, Instant::now()) {
            Ok(authenticated) => {
                let stream = advance_to(transport, authenticated, seat).await?;
                Ok((stream, login))
            }
            Err(failure) => Err(failed(failure, transport, seat)),
        }
    }
}

impl<T: Transport> BedrockStream<SecurePending, Server, T> {
    pub fn login(&self) -> &AuthenticatedLogin {
        self.owner().login()
    }

    /// Runs the ECDH handshake (when enabled) and sends `LoginSuccess`.
    /// The handshake is flushed in plaintext before encryption starts.
    #[tracing::instrument(skip_all, level = "debug", fields(peer = %self.peer_addr()))]
    pub async fn finish_handshake(
        self,
        config: &ServerHandshakeConfig,
    ) -> Result<BedrockStream<ResourcePacks, Server, T>, Failed<T>> {
        let super::Parts {
            mut transport,
            owner,
            seat,
        } = self.take();
        match owner.start_encryption(&config.server_key, Instant::now()) {
            Ok(Step::Next(ready)) => advance_to(transport, ready, seat).await,
            // `drive` flushes the queued ServerToClientHandshake first.
            Ok(Step::Wait(encryption)) => match driver::drive(&mut transport, encryption).await {
                Ok(ready) => advance_to(transport, ready, seat).await,
                Err(failure) => Err(failed(failure, transport, seat)),
            },
            Err(failure) => Err(failed(failure, transport, seat)),
        }
    }
}

impl<T: Transport> BedrockStream<ResourcePacks, Server, T> {
    pub fn login(&self) -> &AuthenticatedLogin {
        self.owner().login()
    }

    /// Offers the listener's packs and serves downloads until the client
    /// completes the stack.
    pub async fn negotiate_packs(self) -> Result<BedrockStream<StartGame, Server, T>, Failed<T>> {
        let packs = self.owner_config_packs();
        self.negotiate_packs_with(packs).await
    }

    fn owner_config_packs(&self) -> Arc<ServerPacks> {
        self.owner().config_packs()
    }

    /// Offers a per-player pack set (gophertunnel `FetchResourcePacks`).
    #[tracing::instrument(skip_all, level = "debug", fields(peer = %self.peer_addr()))]
    pub async fn negotiate_packs_with(
        self,
        packs: Arc<ServerPacks>,
    ) -> Result<BedrockStream<StartGame, Server, T>, Failed<T>> {
        let super::Parts {
            mut transport,
            owner,
            seat,
        } = self.take();
        let phase = match owner.start_packs_with(packs, Instant::now()) {
            Ok(phase) => phase,
            Err(failure) => return Err(failed(failure, transport, seat)),
        };
        match driver::drive(&mut transport, phase).await {
            Ok(ready) => advance_to(transport, ready, seat).await,
            Err(failure) => Err(failed(failure, transport, seat)),
        }
    }
}

impl<T: Transport> BedrockStream<StartGame, Server, T> {
    pub fn login(&self) -> &AuthenticatedLogin {
        self.owner().login()
    }

    /// Sends `StartGame` and the definitions, answers `RequestChunkRadius`,
    /// and enters play on a matching `SetLocalPlayerAsInitialized`.
    #[tracing::instrument(skip_all, level = "debug", fields(peer = %self.peer_addr()))]
    pub async fn start_game(
        self,
        params: WorldJoinParams,
    ) -> Result<BedrockStream<Play, Server, T>, Failed<T>> {
        let super::Parts {
            mut transport,
            owner,
            seat,
        } = self.take();
        let spawn = match owner.start_game(params, Instant::now()) {
            Ok(spawn) => spawn,
            Err(failure) => return Err(failed(failure, transport, seat)),
        };
        match driver::drive(&mut transport, spawn).await {
            Ok(session) => advance_to(transport, session, seat).await,
            Err(failure) => Err(failed(failure, transport, seat)),
        }
    }
}

impl<T: Transport> BedrockStream<Play, Server, T> {
    pub fn login(&self) -> &AuthenticatedLogin {
        &self.owner().login
    }

    pub fn identity(&self) -> &ValidatedIdentity {
        &self.owner().login.identity
    }

    /// The validated client data (skin, device, locale, platform IDs).
    pub fn client_data(&self) -> &ClientData {
        &self.owner().login.client_data
    }

    pub fn runtime_id(&self) -> u64 {
        self.owner().session.runtime_id()
    }

    pub fn chunk_radius(&self) -> Option<i32> {
        self.owner().session.chunk_radius()
    }
}
