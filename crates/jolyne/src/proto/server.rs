//! Server-side join sequence as consuming typestates.
//!
//! ```text
//! ServerHandshake ─RequestNetworkSettings→ ServerLogin ─Login→ LoginRequest
//!   ─(caller verifies)→ Authenticated ─start_encryption→ [ServerEncryption ─C2S→]
//!   PacksReady ─start_packs→ ServerPacks ─Completed→ WorldReady
//!   ─start_game→ ServerSpawn ─SetLocalPlayerAsInitialized→ ServerSession
//! ```
//!
//! The login stage (settings through `ClientToServerHandshake`) shares one
//! deadline (`login_timeout`); resource packs and spawn each get their own.
//! Packets that arrive before the phase that handles them are deferred
//! (bounded) and replayed into the session.

use std::sync::Arc;
use std::time::Instant;

use p384::SecretKey;
use rand::RngCore;

use super::conn::{Connection, Framing};
use super::handshake;
use super::packs::{PackProgress, ServerPackExchange, ServerPacks};
use super::phase::{Deferred, Failure, Owner, Phase, Step, unexpected};
use super::raw::RawPacket;
use super::session::Session;
use crate::auth::{AuthenticatedLogin, Trust};
use crate::config::BedrockListenerConfig;
use crate::error::{AuthError, HandshakePhase, JolyneError, LoginRefusal, ProtocolError};
use crate::valentine::{
    BorrowedMcpePacketData, ChunkRadiusUpdatedPacket, EnumsConnectionDisconnectFailReason,
    McpePacket, McpePacketData, McpePacketName, NetworkChunkPublisherUpdatePacket,
    NetworkSettingsPacket, NetworkSettingsPacketCompressionAlgorithm, PROTOCOL_VERSION,
    PlayStatusPacket, PlayStatusPacketStatus, ServerToClientHandshakePacket,
};
use crate::world::WorldJoinParams;

macro_rules! owner {
    ($ty:ty) => {
        impl Owner for $ty {
            fn connection(&mut self) -> &mut Connection {
                &mut self.conn
            }
            fn connection_ref(&self) -> &Connection {
                &self.conn
            }
            fn into_connection(self) -> Connection {
                self.conn
            }
        }
    };
}

/// Fields every server owner carries.
struct Shared {
    config: Arc<BedrockListenerConfig>,
    deadline: Instant,
}

fn status_packet(status: PlayStatusPacketStatus) -> McpePacket {
    McpePacket::from(PlayStatusPacket { status })
}

/// Phase 1: waiting for `RequestNetworkSettings`.
pub struct ServerHandshake {
    conn: Connection,
    shared: Shared,
}
owner!(ServerHandshake);

impl ServerHandshake {
    pub fn new(conn: Connection, config: Arc<BedrockListenerConfig>, now: Instant) -> Self {
        let deadline = now + config.login_timeout;
        Self {
            conn,
            shared: Shared { config, deadline },
        }
    }

    pub fn config(&self) -> &Arc<BedrockListenerConfig> {
        &self.shared.config
    }

    fn protocol_mismatch(mut self, client_protocol: i32, now: Instant) -> Failure<Self> {
        let status = if client_protocol < PROTOCOL_VERSION {
            PlayStatusPacketStatus::LoginfailedClientold
        } else {
            PlayStatusPacketStatus::LoginfailedServerold
        };
        let queued = self
            .conn
            .queue_packet(&status_packet(status), now)
            .and_then(|()| self.conn.flush());
        let error = queued.err().unwrap_or(
            ProtocolError::IncompatibleProtocol {
                client_protocol,
                server_protocol: PROTOCOL_VERSION,
            }
            .into(),
        );
        Failure::new(error, self)
    }
}

impl Phase for ServerHandshake {
    type Next = ServerLogin;
    const PHASE: HandshakePhase = HandshakePhase::NetworkSettings;

    fn deadline(&self) -> Instant {
        self.shared.deadline
    }

    fn on_packet(
        mut self,
        packet: RawPacket,
        now: Instant,
    ) -> Result<Step<Self, ServerLogin>, Failure<Self>> {
        if !packet.is(McpePacketName::RequestNetworkSettingsPacket) {
            return Err(Failure::new(unexpected(Self::PHASE, &packet), self));
        }
        let client_protocol = match packet.decode_borrowed() {
            Ok(decoded) => match decoded.data {
                BorrowedMcpePacketData::RequestNetworkSettingsPacket(request) => {
                    request.client_network_version
                }
                _ => unreachable!("packet name and decoded variant agree"),
            },
            Err(error) => return Err(Failure::new(error, self)),
        };
        if client_protocol != PROTOCOL_VERSION {
            return Err(self.protocol_mismatch(client_protocol, now));
        }
        let compression = self.shared.config.compression;
        let settings = NetworkSettingsPacket {
            compression_threshold: compression.threshold,
            compression_algorithm: match compression.algorithm {
                crate::proto::batch::BatchCompression::Deflate => {
                    NetworkSettingsPacketCompressionAlgorithm::Zlib
                }
                crate::proto::batch::BatchCompression::Snappy => {
                    NetworkSettingsPacketCompressionAlgorithm::Snappy
                }
                crate::proto::batch::BatchCompression::None => {
                    NetworkSettingsPacketCompressionAlgorithm::None
                }
            },
            client_throttle_enabled: false,
            client_throttle_threshold: 0,
            client_throttle_scalar: 0.0,
        };
        // NetworkSettings leaves uncompressed: enable_compression seals it first.
        let result = self
            .conn
            .queue_packet(&McpePacket::from(settings), now)
            .and_then(|()| {
                self.conn.enable_compression(
                    compression.algorithm,
                    compression.level,
                    compression.threshold,
                )
            });
        if let Err(error) = result {
            return Err(Failure::new(error, self));
        }
        tracing::debug!(client_protocol, "network settings negotiated");
        Ok(Step::Next(ServerLogin {
            conn: self.conn,
            shared: self.shared,
        }))
    }
}

/// Phase 2: waiting for `Login`.
pub struct ServerLogin {
    conn: Connection,
    shared: Shared,
}
owner!(ServerLogin);

impl Phase for ServerLogin {
    type Next = LoginRequest;
    const PHASE: HandshakePhase = HandshakePhase::Login;

    fn deadline(&self) -> Instant {
        self.shared.deadline
    }

    fn on_packet(
        mut self,
        packet: RawPacket,
        now: Instant,
    ) -> Result<Step<Self, LoginRequest>, Failure<Self>> {
        if !packet.is(McpePacketName::LoginPacket) {
            return Err(Failure::new(unexpected(Self::PHASE, &packet), self));
        }
        let (protocol, request) = match packet.decode() {
            Ok(McpePacket {
                data: McpePacketData::LoginPacket(login),
                ..
            }) => (login.client_network_version, login.connection_request),
            Ok(_) => unreachable!("packet name and decoded variant agree"),
            Err(error) => return Err(Failure::new(error, self)),
        };
        if protocol != PROTOCOL_VERSION {
            let status = if protocol < PROTOCOL_VERSION {
                PlayStatusPacketStatus::LoginfailedClientold
            } else {
                PlayStatusPacketStatus::LoginfailedServerold
            };
            let _ = self
                .conn
                .queue_packet(&status_packet(status), now)
                .and_then(|()| self.conn.flush());
            return Err(Failure::new(
                ProtocolError::IncompatibleProtocol {
                    client_protocol: protocol,
                    server_protocol: PROTOCOL_VERSION,
                },
                self,
            ));
        }
        Ok(Step::Next(LoginRequest {
            conn: self.conn,
            shared: self.shared,
            request,
        }))
    }
}

/// A received `Login` awaiting verification by the caller.
pub struct LoginRequest {
    conn: Connection,
    shared: Shared,
    request: bytes::Bytes,
}
owner!(LoginRequest);

impl LoginRequest {
    /// The `connection_request` payload (authentication info + client data JWT).
    pub fn connection_request(&self) -> &[u8] {
        &self.request
    }

    pub fn deadline(&self) -> Instant {
        self.shared.deadline
    }

    pub fn config(&self) -> &Arc<BedrockListenerConfig> {
        &self.shared.config
    }

    /// Answers `PlayStatus(LoginFailedServerFull)` (gophertunnel
    /// `MaximumPlayers`); the client shows "server full".
    pub fn refuse_server_full(mut self, now: Instant) -> Failure<Self> {
        let queued = self
            .conn
            .queue_packet(
                &status_packet(PlayStatusPacketStatus::LoginfailedServerfullsubclient),
                now,
            )
            .and_then(|()| self.conn.flush());
        let error = queued
            .err()
            .unwrap_or(JolyneError::Refused(LoginRefusal::ServerFull));
        Failure::new(error, self)
    }

    /// Accepts a verified login, running the configured admission hook.
    /// A refusal queues `Disconnect{Kicked, message}`.
    pub fn authenticated(
        mut self,
        login: AuthenticatedLogin,
        peer: std::net::SocketAddr,
        now: Instant,
    ) -> Result<Authenticated, Failure<Self>> {
        if now >= self.shared.deadline {
            return Err(Failure::new(
                ProtocolError::Timeout(HandshakePhase::Authentication),
                self,
            ));
        }
        if let Some(hook) = &self.shared.config.admission
            && let Err(message) = hook.admit(peer, &login)
        {
            let _ = self.conn.queue_disconnect(
                EnumsConnectionDisconnectFailReason::Kicked,
                &message,
                now,
            );
            return Err(Failure::new(
                JolyneError::Refused(LoginRefusal::Denied(message)),
                self,
            ));
        }
        tracing::debug!(name = login.identity.display_name(), "login accepted");
        Ok(Authenticated {
            conn: self.conn,
            shared: self.shared,
            login,
        })
    }
}

/// A verified, admitted player before the encryption handshake.
pub struct Authenticated {
    conn: Connection,
    shared: Shared,
    login: AuthenticatedLogin,
}
owner!(Authenticated);

impl Authenticated {
    pub fn login(&self) -> &AuthenticatedLogin {
        &self.login
    }

    /// The login deadline, which also bounds the encryption handshake.
    pub fn deadline(&self) -> Instant {
        self.shared.deadline
    }

    /// Sends `ServerToClientHandshake` (plaintext) and enables encryption, or
    /// skips straight to `LoginSuccess` when encryption is disabled.
    ///
    /// A provider-verified identity becomes authenticated only once the
    /// client proves it holds the token's key: by completing the encryption
    /// handshake, or, with encryption off, through `transport_key` (the key
    /// the transport proved, e.g. NetherNet's DTLS-bound identity). Without
    /// either it is downgraded to unauthenticated; on RakNet, which cannot
    /// prove keys, online mode without encryption is refused.
    pub fn start_encryption(
        mut self,
        server_key: &SecretKey,
        transport_key: Option<&p384::PublicKey>,
        now: Instant,
    ) -> Result<Step<ServerEncryption, PacksReady>, Failure<Self>> {
        if !self.shared.config.encryption_enabled {
            let identity = &mut self.login.identity;
            if transport_key.is_some_and(|key| identity.key_matches(key)) {
                identity.confirm_key_possession();
            } else if identity.trust() == Trust::AwaitingKeyProof {
                if self.conn.framing() == Framing::RakNet {
                    return Err(Failure::new(AuthError::OnlineModeRequiresEncryption, self));
                }
                identity.forfeit_key_proof();
            }
            return match login_success(&mut self.conn, now) {
                Ok(()) => Ok(Step::Next(PacksReady {
                    deferred: Deferred::new(&self.conn),
                    conn: self.conn,
                    config: self.shared.config,
                    login: self.login,
                })),
                Err(error) => Err(Failure::new(error, self)),
            };
        }
        let result = (|| {
            let client_key =
                handshake::parse_public_key(self.login.identity.identity_public_key())?;
            let mut salt = [0u8; 16];
            rand::thread_rng().fill_bytes(&mut salt);
            let token = handshake::server_token(server_key, &salt)?;
            let (key, iv) = handshake::derive_key(server_key, &client_key, &salt);
            self.conn.queue_packet(
                &McpePacket::from(ServerToClientHandshakePacket {
                    handshake_web_token: token.into(),
                }),
                now,
            )?;
            // Seals the handshake in plaintext, then switches both directions.
            self.conn.enable_encryption(key, iv)
        })();
        match result {
            Ok(()) => Ok(Step::Wait(ServerEncryption {
                deferred: Deferred::new(&self.conn),
                conn: self.conn,
                shared: self.shared,
                login: self.login,
            })),
            Err(error) => Err(Failure::new(error, self)),
        }
    }
}

fn login_success(conn: &mut Connection, now: Instant) -> Result<(), JolyneError> {
    conn.queue_packet(&status_packet(PlayStatusPacketStatus::Loginsuccess), now)?;
    conn.flush()
}

/// Phase 3: encryption enabled, waiting for `ClientToServerHandshake`.
pub struct ServerEncryption {
    conn: Connection,
    shared: Shared,
    login: AuthenticatedLogin,
    deferred: Deferred,
}
owner!(ServerEncryption);

impl ServerEncryption {
    /// The login; its identity is authenticated only after this phase.
    pub fn login(&self) -> &AuthenticatedLogin {
        &self.login
    }
}

impl Phase for ServerEncryption {
    type Next = PacksReady;
    const PHASE: HandshakePhase = HandshakePhase::Encryption;

    fn deadline(&self) -> Instant {
        self.shared.deadline
    }

    fn on_packet(
        mut self,
        packet: RawPacket,
        now: Instant,
    ) -> Result<Step<Self, PacksReady>, Failure<Self>> {
        if !packet.is(McpePacketName::ClientToServerHandshakePacket) {
            if let Err(error) = self.deferred.push(packet) {
                return Err(Failure::new(error, self));
            }
            return Ok(Step::Wait(self));
        }
        // The batch decrypted and its checksum verified, so the client
        // derived the ECDH secret from the identity key: it holds that key.
        self.login.identity.confirm_key_possession();
        if let Err(error) = login_success(&mut self.conn, now) {
            return Err(Failure::new(error, self));
        }
        tracing::debug!("encryption established");
        Ok(Step::Next(PacksReady {
            conn: self.conn,
            config: self.shared.config,
            login: self.login,
            deferred: self.deferred,
        }))
    }
}

/// Logged in (`LoginSuccess` sent); resource packs not yet offered.
pub struct PacksReady {
    conn: Connection,
    config: Arc<BedrockListenerConfig>,
    login: AuthenticatedLogin,
    deferred: Deferred,
}
owner!(PacksReady);

impl PacksReady {
    pub fn login(&self) -> &AuthenticatedLogin {
        &self.login
    }

    /// The listener's configured pack set.
    pub fn config_packs(&self) -> Arc<ServerPacks> {
        self.config.resource_packs.clone()
    }

    /// Offers the listener's configured packs.
    pub fn start_packs(self, now: Instant) -> Result<ServerPacksPhase, Failure<Self>> {
        let packs = self.config.resource_packs.clone();
        self.start_packs_with(packs, now)
    }

    /// Offers a per-player pack set (gophertunnel `FetchResourcePacks`).
    pub fn start_packs_with(
        mut self,
        packs: Arc<ServerPacks>,
        now: Instant,
    ) -> Result<ServerPacksPhase, Failure<Self>> {
        match ServerPackExchange::start(packs, &mut self.conn, now) {
            Ok(exchange) => Ok(ServerPacksPhase {
                deadline: now + self.config.resource_pack_timeout,
                conn: self.conn,
                config: self.config,
                login: self.login,
                deferred: self.deferred,
                exchange,
            }),
            Err(error) => Err(Failure::new(error, self)),
        }
    }
}

/// Phase 4: resource pack negotiation and transfer.
pub struct ServerPacksPhase {
    conn: Connection,
    config: Arc<BedrockListenerConfig>,
    login: AuthenticatedLogin,
    deferred: Deferred,
    exchange: ServerPackExchange,
    deadline: Instant,
}
owner!(ServerPacksPhase);

impl Phase for ServerPacksPhase {
    type Next = WorldReady;
    const PHASE: HandshakePhase = HandshakePhase::ResourcePacks;

    fn deadline(&self) -> Instant {
        self.deadline
    }

    fn phase_timer(&self) -> Option<Instant> {
        self.exchange.timer()
    }

    fn on_timer(mut self, now: Instant) -> Result<Step<Self, WorldReady>, Failure<Self>> {
        match self.exchange.on_timer(&mut self.conn, now) {
            Ok(()) => Ok(Step::Wait(self)),
            Err(error) => Err(Failure::new(error, self)),
        }
    }

    fn on_packet(
        mut self,
        packet: RawPacket,
        now: Instant,
    ) -> Result<Step<Self, WorldReady>, Failure<Self>> {
        let is_pack_packet = matches!(
            packet.name(),
            Some(
                McpePacketName::ResourcePackClientResponsePacket
                    | McpePacketName::ResourcePackChunkRequestPacket
            )
        );
        if !is_pack_packet {
            return match self.deferred.push(packet) {
                Ok(()) => Ok(Step::Wait(self)),
                Err(error) => Err(Failure::new(error, self)),
            };
        }
        let decoded = match packet.decode() {
            Ok(decoded) => decoded,
            Err(error) => return Err(Failure::new(error, self)),
        };
        match self.exchange.on_packet(&decoded, &mut self.conn, now) {
            Ok(PackProgress::Done) => {
                tracing::debug!("resource packs completed");
                Ok(Step::Next(WorldReady {
                    conn: self.conn,
                    config: self.config,
                    login: self.login,
                    deferred: self.deferred,
                }))
            }
            Ok(_) => Ok(Step::Wait(self)),
            Err(error) => Err(Failure::new(error, self)),
        }
    }
}

/// Packs completed; waiting for the caller's world parameters.
pub struct WorldReady {
    conn: Connection,
    config: Arc<BedrockListenerConfig>,
    login: AuthenticatedLogin,
    deferred: Deferred,
}
owner!(WorldReady);

impl WorldReady {
    pub fn login(&self) -> &AuthenticatedLogin {
        &self.login
    }

    /// Sends `DimensionData` (when set), `JigsawStructureData`, `VoxelShapes`,
    /// `StartGame`, `ItemRegistry` and the definition lists as one batch
    /// whatever the flush mode (BDS order and batching; definitions precede
    /// the client's `RequestChunkRadius`).
    pub fn start_game(
        mut self,
        params: WorldJoinParams,
        now: Instant,
    ) -> Result<ServerSpawn, Failure<Self>> {
        let result = self.conn.queue_group(|conn| {
            if let Some(dimensions) = &params.dimension_data {
                conn.queue_body(
                    McpePacketName::DimensionDataPacket,
                    dimensions.as_ref(),
                    now,
                )?;
            }
            conn.queue_body(
                McpePacketName::JigsawStructureDataPacket,
                params.jigsaw_structures.as_ref(),
                now,
            )?;
            conn.queue_body(
                McpePacketName::VoxelShapesPacket,
                params.voxel_shapes.as_ref(),
                now,
            )?;
            conn.queue_body(McpePacketName::StartGamePacket, &params.start_game, now)?;
            conn.queue_body(
                McpePacketName::ItemRegistryPacket,
                params.item_registry.as_ref(),
                now,
            )?;
            if let Some(biomes) = &params.biome_definitions {
                conn.queue_body(
                    McpePacketName::BiomeDefinitionListPacket,
                    biomes.as_ref(),
                    now,
                )?;
            }
            if let Some(entities) = &params.available_entities {
                conn.queue_body(
                    McpePacketName::AvailableActorIdentifiersPacket,
                    entities.as_ref(),
                    now,
                )?;
            }
            if let Some(creative) = &params.creative_content {
                conn.queue_body(
                    McpePacketName::CreativeContentPacket,
                    creative.as_ref(),
                    now,
                )?;
            }
            Ok(())
        });
        if let Err(error) = result {
            return Err(Failure::new(error, self));
        }
        tracing::debug!("start game sent");
        Ok(ServerSpawn {
            deadline: now + self.config.spawn_timeout,
            runtime_id: params.start_game.runtime_id.actor_runtime_id,
            publisher_position: params
                .start_game
                .settings
                .default_spawn_block_position
                .clone(),
            radius_override: params.chunk_radius,
            chunk_radius: None,
            conn: self.conn,
            login: self.login,
            deferred: self.deferred,
        })
    }
}

/// Phase 5: `StartGame` sent; waiting for `RequestChunkRadius` and
/// `SetLocalPlayerAsInitialized`.
pub struct ServerSpawn {
    conn: Connection,
    login: AuthenticatedLogin,
    deferred: Deferred,
    deadline: Instant,
    runtime_id: u64,
    publisher_position: crate::valentine::BlockPos,
    radius_override: Option<i32>,
    chunk_radius: Option<i32>,
}
owner!(ServerSpawn);

impl ServerSpawn {
    /// Answers the first `RequestChunkRadius` with `ChunkRadiusUpdated`,
    /// `NetworkChunkPublisherUpdate` and `PlayStatus(PlayerSpawn)` in one
    /// batch.
    fn on_radius(&mut self, requested: i32, now: Instant) -> Result<(), JolyneError> {
        if requested < 1 {
            return Err(ProtocolError::InvalidChunkRadius(requested).into());
        }
        let radius = self.radius_override.unwrap_or(requested);
        self.chunk_radius = Some(radius);
        let packets = [
            McpePacket::from(ChunkRadiusUpdatedPacket {
                chunk_radius: radius,
            }),
            McpePacket::from(NetworkChunkPublisherUpdatePacket {
                newpositionforview: self.publisher_position.clone(),
                newradiusforview: (radius as u32).saturating_mul(16),
                server_built_chunks_list: vec![],
            }),
            status_packet(PlayStatusPacketStatus::Playerspawn),
        ];
        self.conn.send_batch(&packets, true, now)
    }
}

impl Phase for ServerSpawn {
    type Next = ServerSession;
    const PHASE: HandshakePhase = HandshakePhase::Spawn;

    fn deadline(&self) -> Instant {
        self.deadline
    }

    fn on_packet(
        mut self,
        packet: RawPacket,
        now: Instant,
    ) -> Result<Step<Self, ServerSession>, Failure<Self>> {
        let name = packet.name();
        let first_radius =
            name == Some(McpePacketName::RequestChunkRadiusPacket) && self.chunk_radius.is_none();
        if !first_radius && name != Some(McpePacketName::SetLocalPlayerAsInitializedPacket) {
            return match self.deferred.push(packet) {
                Ok(()) => Ok(Step::Wait(self)),
                Err(error) => Err(Failure::new(error, self)),
            };
        }
        let decoded = match packet.decode() {
            Ok(decoded) => decoded,
            Err(error) => return Err(Failure::new(error, self)),
        };
        match decoded.data {
            McpePacketData::RequestChunkRadiusPacket(request) => {
                match self.on_radius(request.chunk_radius, now) {
                    Ok(()) => Ok(Step::Wait(self)),
                    Err(error) => Err(Failure::new(error, self)),
                }
            }
            McpePacketData::SetLocalPlayerAsInitializedPacket(initialized) => {
                let got = initialized.player_id.actor_runtime_id;
                if got != self.runtime_id {
                    let error = ProtocolError::RuntimeIdMismatch {
                        expected: self.runtime_id,
                        got,
                    };
                    return Err(Failure::new(error, self));
                }
                let ServerSpawn {
                    mut conn,
                    login,
                    deferred,
                    runtime_id,
                    chunk_radius,
                    ..
                } = self;
                deferred.replay_into(&mut conn);
                tracing::debug!(runtime_id, "player initialized");
                Ok(Step::Next(ServerSession {
                    session: Session::new(conn, runtime_id, chunk_radius),
                    login,
                }))
            }
            _ => unreachable!("filtered above"),
        }
    }
}

/// The play session of an accepted player.
pub struct ServerSession {
    pub session: Session,
    pub login: AuthenticatedLogin,
}

impl Owner for ServerSession {
    fn connection(&mut self) -> &mut Connection {
        self.session.connection()
    }
    fn connection_ref(&self) -> &Connection {
        self.session.connection_ref()
    }
    fn into_connection(self) -> Connection {
        self.session.into_connection()
    }
}
