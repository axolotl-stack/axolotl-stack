//! Client-side join sequence as consuming typestates.
//!
//! ```text
//! ClientSettings ─NetworkSettings→ ClientLogin ─login(tokens)→ ClientHandshake
//!   ─LoginSuccess→ ClientPacks ─Stack→ ClientSpawn ─PlayerSpawn→ ClientSession
//! ```
//!
//! One deadline (the join timeout) spans every phase. Packets arriving before
//! the phase that consumes them are deferred in arrival order (bounded by
//! `ConnectionOptions::max_deferred_*`) and handed to the next phase, and
//! finally to the play session.

use std::time::Instant;

use bytes::Bytes;
use p384::SecretKey;

use super::batch::BatchCompression;
use super::conn::Connection;
use super::handshake;
use super::packs::{ClientPackExchange, ClientPackPolicy, DownloadedPack, PackProgress};
use super::phase::{Deferred, Failure, Owner, Phase, Step, unexpected};
use super::raw::RawPacket;
use super::session::Session;
use crate::config::ClientConfig;
use crate::error::{HandshakePhase, JolyneError, ProtocolError};
use crate::gamedata::GameData;
use crate::valentine::{
    ActorRuntimeId, ClientCacheStatusPacket, ClientToServerHandshakePacket,
    EnumsServerboundLoadingScreenPacketType, ItemRegistryPacket, LoginPacket, McpePacket,
    McpePacketData, McpePacketName, NetworkSettingsPacketCompressionAlgorithm, PROTOCOL_VERSION,
    PlayStatusPacketStatus, RequestChunkRadiusPacket, RequestNetworkSettingsPacket,
    ServerboundLoadingScreenPacket, SetLocalPlayerAsInitializedPacket, StartGamePacket,
};

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

/// Options the client phases need after construction.
#[derive(Clone)]
struct Shared {
    deadline: Instant,
    chunk_radius: i32,
    packs: ClientPackPolicy,
}

fn decode(packet: RawPacket) -> Result<McpePacket, JolyneError> {
    packet.decode()
}

/// Phase 1: `RequestNetworkSettings` sent; waiting for `NetworkSettings`.
pub struct ClientSettings {
    conn: Connection,
    shared: Shared,
}
owner!(ClientSettings);

impl ClientSettings {
    /// Queues `RequestNetworkSettings`.
    pub fn new(
        mut conn: Connection,
        config: &ClientConfig,
        now: Instant,
    ) -> Result<Self, JolyneError> {
        conn.queue_packet(
            &McpePacket::from(RequestNetworkSettingsPacket {
                client_network_version: PROTOCOL_VERSION,
            }),
            now,
        )?;
        conn.flush()?;
        Ok(Self {
            conn,
            shared: Shared {
                deadline: now + config.join_timeout,
                chunk_radius: config.chunk_radius,
                packs: config.packs.clone(),
            },
        })
    }
}

impl Phase for ClientSettings {
    type Next = ClientLogin;
    const PHASE: HandshakePhase = HandshakePhase::NetworkSettings;

    fn deadline(&self) -> Instant {
        self.shared.deadline
    }

    fn on_packet(
        mut self,
        packet: RawPacket,
        _now: Instant,
    ) -> Result<Step<Self, ClientLogin>, Failure<Self>> {
        if !matches!(
            packet.name(),
            Some(McpePacketName::NetworkSettingsPacket | McpePacketName::PlayStatusPacket)
        ) {
            return Err(Failure::new(unexpected(Self::PHASE, &packet), self));
        }
        let settings = match decode(packet) {
            Ok(McpePacket {
                data: McpePacketData::NetworkSettingsPacket(settings),
                ..
            }) => settings,
            // A refusal (e.g. outdated client) arrives instead of the settings.
            Ok(McpePacket {
                data: McpePacketData::PlayStatusPacket(status),
                ..
            }) => return Err(Failure::new(JolyneError::LoginRefused(status.status), self)),
            Ok(_) => unreachable!("filtered by name"),
            Err(error) => return Err(Failure::new(error, self)),
        };
        let (algorithm, level) = match settings.compression_algorithm {
            NetworkSettingsPacketCompressionAlgorithm::Zlib => (BatchCompression::Deflate, 7),
            NetworkSettingsPacketCompressionAlgorithm::Snappy => (BatchCompression::Snappy, 1),
            NetworkSettingsPacketCompressionAlgorithm::None => (BatchCompression::None, 0),
            NetworkSettingsPacketCompressionAlgorithm::Unknown(value) => {
                return Err(Failure::new(
                    ProtocolError::UnsupportedNetworkCompression(value),
                    self,
                ));
            }
        };
        if let Err(error) =
            self.conn
                .enable_compression(algorithm, level, settings.compression_threshold)
        {
            return Err(Failure::new(error, self));
        }
        Ok(Step::Next(ClientLogin {
            conn: self.conn,
            shared: self.shared,
        }))
    }
}

/// Compression negotiated; waiting for the caller's login tokens.
pub struct ClientLogin {
    conn: Connection,
    shared: Shared,
}
owner!(ClientLogin);

impl ClientLogin {
    pub fn deadline(&self) -> Instant {
        self.shared.deadline
    }

    /// Sends `Login` with a pre-encoded `connection_request` payload.
    /// `identity_key` must be the key the tokens were signed with; it is used
    /// for the ECDH handshake.
    pub fn login(
        mut self,
        identity_key: SecretKey,
        connection_request: Bytes,
        now: Instant,
    ) -> Result<ClientHandshake, Failure<Self>> {
        let result = self
            .conn
            .queue_packet(
                &McpePacket::from(LoginPacket {
                    client_network_version: PROTOCOL_VERSION,
                    connection_request,
                }),
                now,
            )
            .and_then(|()| self.conn.flush());
        match result {
            Ok(()) => Ok(ClientHandshake {
                deferred: Deferred::new(&self.conn),
                conn: self.conn,
                shared: self.shared,
                identity_key,
            }),
            Err(error) => Err(Failure::new(error, self)),
        }
    }
}

/// Phase 2: `Login` sent; waiting for the optional `ServerToClientHandshake`
/// and then `PlayStatus(LoginSuccess)`. Anything else (some servers send
/// `ResourcePacksInfo` before `LoginSuccess`) is deferred to the pack phase.
pub struct ClientHandshake {
    conn: Connection,
    shared: Shared,
    identity_key: SecretKey,
    deferred: Deferred,
}
owner!(ClientHandshake);

impl ClientHandshake {
    fn on_server_handshake(&mut self, token: &str, now: Instant) -> Result<(), JolyneError> {
        let (server_key, salt) = handshake::verify_server_token(token)?;
        let (key, iv) = handshake::derive_key(&self.identity_key, &server_key, &salt);
        // The server already encrypts; our acknowledgement must be encrypted too.
        self.conn.enable_encryption(key, iv)?;
        self.conn
            .queue_packet(&McpePacket::from(ClientToServerHandshakePacket {}), now)?;
        self.conn.flush()
    }
}

impl Phase for ClientHandshake {
    type Next = ClientPacks;
    const PHASE: HandshakePhase = HandshakePhase::Encryption;

    fn deadline(&self) -> Instant {
        self.shared.deadline
    }

    fn on_packet(
        mut self,
        packet: RawPacket,
        now: Instant,
    ) -> Result<Step<Self, ClientPacks>, Failure<Self>> {
        match packet.name() {
            Some(
                McpePacketName::ServerToClientHandshakePacket | McpePacketName::PlayStatusPacket,
            ) => {}
            _ => {
                return match self.deferred.push(packet) {
                    Ok(()) => Ok(Step::Wait(self)),
                    Err(error) => Err(Failure::new(error, self)),
                };
            }
        }
        let result = match decode(packet) {
            Ok(McpePacket {
                data: McpePacketData::ServerToClientHandshakePacket(handshake),
                ..
            }) => {
                if self.conn.is_encrypted() {
                    Err(ProtocolError::InvalidHandshake("duplicate handshake".into()).into())
                } else {
                    self.on_server_handshake(&handshake.handshake_web_token, now)
                        .map(|()| false)
                }
            }
            Ok(McpePacket {
                data: McpePacketData::PlayStatusPacket(status),
                ..
            }) => match status.status {
                PlayStatusPacketStatus::Loginsuccess => Ok(true),
                other => Err(JolyneError::LoginRefused(other)),
            },
            Ok(_) => unreachable!("filtered by name"),
            Err(error) => Err(error),
        };
        match result {
            Ok(false) => Ok(Step::Wait(self)),
            Ok(true) => {
                // Tell the server we have no blob cache, then expect packs.
                let sent = self
                    .conn
                    .queue_packet(
                        &McpePacket::from(ClientCacheStatusPacket {
                            iscachesupported: false,
                        }),
                        now,
                    )
                    .and_then(|()| self.conn.flush());
                if let Err(error) = sent {
                    return Err(Failure::new(error, self));
                }
                let ClientHandshake {
                    mut conn,
                    shared,
                    deferred,
                    ..
                } = self;
                deferred.replay_into(&mut conn);
                Ok(Step::Next(ClientPacks {
                    exchange: ClientPackExchange::new(shared.packs.clone()),
                    deferred: Deferred::new(&conn),
                    conn,
                    shared,
                }))
            }
            Err(error) => Err(Failure::new(error, self)),
        }
    }
}

/// Phase 3: resource pack negotiation and download.
pub struct ClientPacks {
    conn: Connection,
    shared: Shared,
    exchange: ClientPackExchange,
    deferred: Deferred,
}
owner!(ClientPacks);

impl Phase for ClientPacks {
    type Next = ClientSpawn;
    const PHASE: HandshakePhase = HandshakePhase::ResourcePacks;

    fn deadline(&self) -> Instant {
        self.shared.deadline
    }

    fn on_packet(
        mut self,
        packet: RawPacket,
        now: Instant,
    ) -> Result<Step<Self, ClientSpawn>, Failure<Self>> {
        let is_pack_packet = matches!(
            packet.name(),
            Some(
                McpePacketName::ResourcePacksInfoPacket
                    | McpePacketName::ResourcePackDataInfoPacket
                    | McpePacketName::ResourcePackChunkDataPacket
                    | McpePacketName::ResourcePackStackPacket
            )
        );
        if !is_pack_packet {
            return match self.deferred.push(packet) {
                Ok(()) => Ok(Step::Wait(self)),
                Err(error) => Err(Failure::new(error, self)),
            };
        }
        let decoded = match decode(packet) {
            Ok(decoded) => decoded,
            Err(error) => return Err(Failure::new(error, self)),
        };
        match self.exchange.on_packet(&decoded, &mut self.conn, now) {
            Ok(PackProgress::Done) => {
                let ClientPacks {
                    mut conn,
                    shared,
                    exchange,
                    deferred,
                } = self;
                deferred.replay_into(&mut conn);
                let (packs, packs_required) = exchange.into_downloaded();
                Ok(Step::Next(ClientSpawn {
                    deferred: Deferred::new(&conn),
                    conn,
                    shared,
                    packs,
                    packs_required,
                    start_game: None,
                    item_registry: None,
                    requested_radius: false,
                    updated_radius: None,
                    spawned: false,
                }))
            }
            Ok(_) => Ok(Step::Wait(self)),
            Err(error) => Err(Failure::new(error, self)),
        }
    }
}

/// Phase 4: waiting for `StartGame`, `ItemRegistry`, `ChunkRadiusUpdated`
/// and `PlayStatus(PlayerSpawn)`.
pub struct ClientSpawn {
    conn: Connection,
    shared: Shared,
    packs: Vec<DownloadedPack>,
    packs_required: bool,
    deferred: Deferred,
    start_game: Option<Box<StartGamePacket>>,
    item_registry: Option<ItemRegistryPacket>,
    requested_radius: bool,
    updated_radius: Option<i32>,
    spawned: bool,
}
owner!(ClientSpawn);

/// The client's play session.
pub struct ClientSession {
    pub session: Session,
    /// Packs downloaded (or loaded from cache) during the join.
    pub packs: Vec<DownloadedPack>,
    /// Whether the server marked its packs as required.
    pub packs_required: bool,
}

impl Owner for ClientSession {
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

/// What the client learned while joining.
pub struct ClientJoined {
    pub session: ClientSession,
    pub game_data: GameData,
}

impl ClientSpawn {
    fn loading_screen(kind: EnumsServerboundLoadingScreenPacketType) -> McpePacket {
        McpePacket::from(ServerboundLoadingScreenPacket {
            loading_screen_packet_type: kind,
            loading_screen_id: None,
        })
    }

    fn observe(&mut self, packet: RawPacket, now: Instant) -> Result<(), JolyneError> {
        match packet.name() {
            Some(McpePacketName::StartGamePacket) => {
                let McpePacketData::StartGamePacket(start) = decode(packet)?.data else {
                    unreachable!("filtered by name")
                };
                match &self.start_game {
                    Some(first)
                        if first.runtime_id.actor_runtime_id
                            != start.runtime_id.actor_runtime_id =>
                    {
                        return Err(ProtocolError::ConflictingStartGame {
                            first: first.runtime_id.actor_runtime_id,
                            second: start.runtime_id.actor_runtime_id,
                        }
                        .into());
                    }
                    Some(_) => {}
                    None => self.start_game = Some(start),
                }
                if !self.requested_radius {
                    self.requested_radius = true;
                    self.conn.queue_packet(
                        &Self::loading_screen(
                            EnumsServerboundLoadingScreenPacketType::Startloadingscreen,
                        ),
                        now,
                    )?;
                    let radius = self.shared.chunk_radius;
                    self.conn.queue_packet(
                        &McpePacket::from(RequestChunkRadiusPacket {
                            chunk_radius: radius,
                            max_chunk_radius: radius.clamp(0, i32::from(u8::MAX)) as u8,
                        }),
                        now,
                    )?;
                    self.conn.flush()?;
                }
            }
            Some(McpePacketName::ItemRegistryPacket) => {
                let McpePacketData::ItemRegistryPacket(registry) = decode(packet)?.data else {
                    unreachable!("filtered by name")
                };
                self.item_registry = Some(registry);
            }
            Some(McpePacketName::PlayStatusPacket) => {
                let McpePacketData::PlayStatusPacket(status) = decode(packet)?.data else {
                    unreachable!("filtered by name")
                };
                match status.status {
                    PlayStatusPacketStatus::Playerspawn => self.spawned = true,
                    PlayStatusPacketStatus::Loginsuccess => {}
                    other => return Err(JolyneError::LoginRefused(other)),
                }
            }
            Some(McpePacketName::ChunkRadiusUpdatedPacket) => {
                let McpePacketData::ChunkRadiusUpdatedPacket(update) = decode(packet.clone())?.data
                else {
                    unreachable!("filtered by name")
                };
                if update.chunk_radius < 1 {
                    return Err(ProtocolError::InvalidChunkRadius(update.chunk_radius).into());
                }
                self.updated_radius = Some(update.chunk_radius);
                // The play session still sees the radius update.
                self.deferred.push(packet)?;
            }
            _ => self.deferred.push(packet)?,
        }
        Ok(())
    }
}

impl Phase for ClientSpawn {
    type Next = ClientJoined;
    const PHASE: HandshakePhase = HandshakePhase::Spawn;

    fn deadline(&self) -> Instant {
        self.shared.deadline
    }

    fn on_packet(
        mut self,
        packet: RawPacket,
        now: Instant,
    ) -> Result<Step<Self, ClientJoined>, Failure<Self>> {
        if let Err(error) = self.observe(packet, now) {
            return Err(Failure::new(error, self));
        }
        if !(self.updated_radius.is_some() && self.spawned && self.item_registry.is_some()) {
            return Ok(Step::Wait(self));
        }
        let Some(start_game) = self.start_game.take() else {
            return Ok(Step::Wait(self));
        };
        let runtime_id = start_game.runtime_id.actor_runtime_id;
        let sent = (|| {
            self.conn.queue_packet(
                &Self::loading_screen(EnumsServerboundLoadingScreenPacketType::Endloadingscreen),
                now,
            )?;
            self.conn.queue_packet(
                &McpePacket::from(SetLocalPlayerAsInitializedPacket {
                    player_id: ActorRuntimeId {
                        actor_runtime_id: runtime_id,
                    },
                }),
                now,
            )?;
            self.conn.flush()
        })();
        if let Err(error) = sent {
            self.start_game = Some(start_game);
            return Err(Failure::new(error, self));
        }
        let ClientSpawn {
            mut conn,
            packs,
            packs_required,
            deferred,
            item_registry,
            updated_radius,
            ..
        } = self;
        deferred.replay_into(&mut conn);
        Ok(Step::Next(ClientJoined {
            session: ClientSession {
                session: Session::new(conn, runtime_id, updated_radius),
                packs,
                packs_required,
            },
            game_data: GameData {
                start_game: *start_game,
                item_registry: item_registry.expect("checked above"),
                biome_definitions: None,
                entity_identifiers: None,
                creative_content: None,
            },
        }))
    }
}
