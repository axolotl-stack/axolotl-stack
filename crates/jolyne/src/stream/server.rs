use std::marker::PhantomData;
use std::sync::Arc;

use crate::config::BedrockListenerConfig;

use crate::protocol::{
    PacketChunkRadiusUpdate, PacketPlayStatus, PacketPlayStatusStatus, PacketResourcePackStack,
    PacketResourcePacksInfo, PacketServerToClientHandshake,
};
use aes_gcm::Aes256Gcm;
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use p384::{
    PublicKey, SecretKey,
    pkcs8::{DecodePublicKey, EncodePrivateKey, EncodePublicKey},
};
use rand::{RngCore, thread_rng};
use sha2::{Digest, Sha256};
use tracing::instrument;
use uuid::Uuid;

use crate::auth::{ValidatedIdentity, authenticate_login};
use crate::error::{JolyneError, ProtocolError};
use crate::protocol::packets::{PacketNetworkSettings, PacketNetworkSettingsCompressionAlgorithm};
use crate::protocol::types::experiments::Experiments;
use crate::protocol::types::resource::ResourcePackIdVersions;
use crate::protocol::{McpePacket, McpePacketData};
use crate::stream::{
    BedrockStream, Handshake, Login, Play, ResourcePacks, SecurePending, Server, StartGame,
    transport::{BedrockTransport, Transport},
};
use crate::world::{WorldJoinParams, WorldTemplate};

/// Configuration for the Server Handshake.
#[derive(Debug, Clone)]
pub struct ServerHandshakeConfig {
    pub server_key: SecretKey, // Persistent key for the session
}

// --- State: Handshake (Initial) ---

impl<T: Transport> BedrockStream<Handshake, Server, T> {
    /// Creates a server handshake stream from a transport.
    ///
    /// Used for NetherNet and other non-listener transports where you have
    /// the raw stream and want to start the Bedrock handshake.
    pub fn from_transport(
        transport: BedrockTransport<T>,
        config: Arc<BedrockListenerConfig>,
    ) -> Self {
        Self {
            transport,
            state: Handshake {
                config: Some(config),
            },
            _role: PhantomData,
        }
    }

    /// Accepts a new connection and negotiates network settings.

    #[instrument(skip_all, level = "trace")]

    pub async fn accept_network_settings(
        mut self,
    ) -> Result<BedrockStream<Login, Server, T>, JolyneError> {
        let packet = self.transport.recv_packet().await?;

        match packet.data {
            McpePacketData::PacketRequestNetworkSettings(req) => {
                let server_protocol = crate::protocol::PROTOCOL_VERSION;

                let client_protocol = req.client_protocol;

                // Add protocol version context to the current span

                tracing::Span::current().record("client_protocol", client_protocol);

                if client_protocol != server_protocol {
                    let status = if client_protocol < server_protocol {
                        PacketPlayStatusStatus::FailedClient
                    } else {
                        PacketPlayStatusStatus::FailedSpawn
                    };

                    self.transport
                        .send_raw(McpePacket::from(PacketPlayStatus { status }))
                        .await?;

                    tracing::warn!(client_protocol, server_protocol, "Protocol mismatch");

                    return Err(ProtocolError::IncompatibleProtocol {
                        client_protocol,
                        server_protocol,
                    }
                    .into());
                }

                let listener_config = self.state.config.as_ref().expect("config");

                let settings = PacketNetworkSettings {
                    compression_threshold: listener_config.compression_threshold,
                    compression_algorithm: PacketNetworkSettingsCompressionAlgorithm::Deflate,
                    client_throttle: false,
                    client_throttle_threshold: 0,
                    client_throttle_scalar: 0.0,
                };

                self.transport.send_raw(McpePacket::from(settings)).await?;

                self.transport.set_compression(
                    true,
                    listener_config.compression_level,
                    listener_config.compression_threshold,
                );

                tracing::debug!("Network settings negotiated");

                Ok(BedrockStream {
                    transport: self.transport,
                    state: Login {
                        config: self.state.config,
                    },
                    _role: PhantomData,
                })
            }
            _ => Err(
                ProtocolError::UnexpectedHandshake("Expected RequestNetworkSettings".into()).into(),
            ),
        }
    }

    /// Helper: Orchestrates the entire join sequence using a WorldTemplate.
    /// This replaces the old `simple_login`.
    pub async fn accept_join_sequence(
        self,
        template: &WorldTemplate,
        server_key: &SecretKey,
    ) -> Result<(BedrockStream<Play, Server, T>, ValidatedIdentity), JolyneError> {
        // 1. Network Settings
        let login = self.accept_network_settings().await?;

        // 2. Auth
        let (secure, identity) = login.authenticate().await?;

        // 3. Encryption
        let packs = secure
            .finish_handshake(
                &ServerHandshakeConfig {
                    server_key: server_key.clone(),
                },
                &identity.identity_public_key,
            )
            .await?;

        // 4. Resource Packs (None/Default)
        let start_game_state = packs.negotiate_packs(false).await?;

        // 5. Personalize Template
        let join_params = template.to_join_params(rand::random());

        // 6. Join
        let play = start_game_state.start_game(join_params).await?;
        Ok((play, identity))
    }
}

// --- State: Login ---

impl<T: Transport> BedrockStream<Login, Server, T> {
    #[instrument(skip_all, level = "trace")]

    pub async fn authenticate(
        mut self,
    ) -> Result<(BedrockStream<SecurePending, Server, T>, ValidatedIdentity), JolyneError> {
        let packet = self.recv_expect_login().await?;

        let login_data = match packet.data {
            McpePacketData::PacketLogin(l) => l,

            _ => unreachable!(),
        };

        let listener_config = self.state.config.as_ref().expect("config");

        let identity = authenticate_login(
            &login_data.tokens.identity,
            &login_data.tokens.client,
            listener_config.online_mode,
            listener_config.allow_legacy_auth,
        )
        .await?;

        tracing::debug!(display_name = ?identity.display_name, uuid = ?identity.uuid, "Client authenticated");

        Ok((
            BedrockStream {
                transport: self.transport,
                state: SecurePending {
                    config: self.state.config,
                },
                _role: PhantomData,
            },
            identity,
        ))
    }

    async fn recv_expect_login(&mut self) -> Result<McpePacket, JolyneError> {
        let packet = self.transport.recv_packet().await?;

        if matches!(packet.data, McpePacketData::PacketLogin(_)) {
            Ok(packet)
        } else {
            Err(ProtocolError::MissingLoginPacket.into())
        }
    }
}

// --- State: SecurePending ---

impl<T: Transport> BedrockStream<SecurePending, Server, T> {
    #[instrument(skip_all, level = "trace")]

    pub async fn finish_handshake(
        mut self,
        config: &ServerHandshakeConfig,
        client_pub_b64: &str,
    ) -> Result<BedrockStream<ResourcePacks, Server, T>, JolyneError> {
        let listener_config = self.state.config.as_ref().expect("config");

        if listener_config.encryption_enabled {
            self.perform_encryption_handshake(&config.server_key, client_pub_b64)
                .await?;
        } else {
            tracing::debug!("Encryption disabled by config");
        }

        self.transport
            .send_batch(&[McpePacket::from(PacketPlayStatus {
                status: PacketPlayStatusStatus::LoginSuccess,
            })])
            .await?;

        Ok(BedrockStream {
            transport: self.transport,
            state: ResourcePacks,
            _role: PhantomData,
        })
    }

    #[instrument(skip_all, level = "trace")]

    async fn perform_encryption_handshake(
        &mut self,
        server_key: &SecretKey,
        client_pub_b64: &str,
    ) -> Result<(), JolyneError> {
        let client_der = STANDARD
            .decode(client_pub_b64)
            .map_err(|e| ProtocolError::UnexpectedHandshake(e.to_string()))?;

        let client_pub = PublicKey::from_public_key_der(&client_der)
            .map_err(|e| ProtocolError::UnexpectedHandshake(e.to_string()))?;
        let mut salt = [0u8; 16];
        thread_rng().fill_bytes(&mut salt);
        let shared_secret =
            p384::ecdh::diffie_hellman(server_key.to_nonzero_scalar(), client_pub.as_affine());
        let shared_bytes = shared_secret.raw_secret_bytes();

        let mut h = Sha256::new();
        h.update(salt);
        h.update(shared_bytes);

        let key_bytes = h.finalize();
        let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&key_bytes);

        let mut iv = [0u8; 12];

        iv.copy_from_slice(&key_bytes[0..12]);
        let server_pub_der = server_key.public_key().to_public_key_der().unwrap();
        let server_pub_b64 = STANDARD.encode(server_pub_der.as_bytes());
        let mut header = Header::new(Algorithm::ES384);
        header.x5u = Some(server_pub_b64);

        #[derive(serde::Serialize)]
        struct SaltClaims {
            salt: String,
        }

        let claims = SaltClaims {
            salt: STANDARD_NO_PAD.encode(salt),
        };

        let server_priv_der = server_key.to_pkcs8_der().unwrap();

        let encoding_key = EncodingKey::from_ec_der(server_priv_der.as_bytes());

        let token = encode(&header, &claims, &encoding_key).unwrap();

        let handshake_pkt = PacketServerToClientHandshake { token };

        self.transport
            .send_batch(&[McpePacket::from(handshake_pkt)])
            .await?;

        self.transport.enable_encryption(*key, iv);

        let packet = self.transport.recv_packet().await?;

        if !matches!(
            packet.data,
            McpePacketData::PacketClientToServerHandshake(_)
        ) {
            return Err(ProtocolError::UnexpectedHandshake(
                "Expected ClientToServerHandshake".into(),
            )
            .into());
        }

        tracing::debug!("Server encryption enabled");

        Ok(())
    }
}

// --- State: ResourcePacks ---

impl<T: Transport> BedrockStream<ResourcePacks, Server, T> {
    #[instrument(skip_all, level = "trace")]

    pub async fn negotiate_packs(
        mut self,

        required: bool,
    ) -> Result<BedrockStream<StartGame, Server, T>, JolyneError> {
        let info = PacketResourcePacksInfo {
            must_accept: required,
            has_addons: false,
            has_scripts: false,
            disable_vibrant_visuals: false,
            world_template:
                crate::protocol::packets::resource::PacketResourcePacksInfoWorldTemplate {
                    uuid: Uuid::nil(),
                    version: "1.0.0".to_string(),
                },
            texture_packs: vec![],
        };

        let stack = PacketResourcePackStack {
            must_accept: required,
            resource_packs: ResourcePackIdVersions::new(),
            game_version: crate::protocol::GAME_VERSION.to_string(),
            experiments: Experiments::new(),
            experiments_previously_used: false,
            has_editor_packs: false,
        };

        self.transport
            .send_batch(&[McpePacket::from(info), McpePacket::from(stack)])
            .await?;

        loop {
            let packets = self.transport.recv_batch().await?;
            for pkt in packets {
                if let McpePacketData::PacketResourcePackClientResponse(resp) = pkt.data {
                    use crate::protocol::packets::resource::PacketResourcePackClientResponseResponseStatus as Status;
                    match resp.response_status {
                        Status::Refused if required => {
                            tracing::warn!("Client refused required resource packs");
                            return Err(ProtocolError::UnexpectedHandshake(
                                "Client refused required packs".into(),
                            )
                            .into());
                        }

                        Status::Refused => {
                            tracing::debug!("Client refused optional resource packs");

                            return Ok(BedrockStream {
                                transport: self.transport,
                                state: StartGame,
                                _role: PhantomData,
                            });
                        }

                        Status::SendPacks => {}

                        Status::HaveAllPacks => {
                            tracing::debug!("Client has all resource packs");

                            return Ok(BedrockStream {
                                transport: self.transport,
                                state: StartGame,
                                _role: PhantomData,
                            });
                        }
                        Status::Completed => {}
                        _ => {}
                    }
                }
            }
        }
    }
}

// --- State: StartGame ---

impl<T: Transport> BedrockStream<StartGame, Server, T> {
    /// Completes the sequence by sending StartGame and waiting for client initialization.
    #[instrument(skip_all, level = "trace")]
    pub async fn start_game(
        mut self,
        params: WorldJoinParams,
    ) -> Result<BedrockStream<Play, Server, T>, JolyneError> {
        // 1. Send StartGame Packet & ItemRegistry
        self.transport
            .send_batch(&[
                McpePacket::from(params.start_game),
                // McpePacket::from(params.item_registry.as_ref().clone()), // Clone for now
            ])
            .await?;

        tracing::debug!("StartGame packet sent");

        // 2. Wait for RequestChunkRadius
        let requested_radius = loop {
            let pkt = self.transport.recv_packet().await?;

            if let McpePacketData::PacketRequestChunkRadius(req) = pkt.data {
                break req.chunk_radius;
            }
            // ignore all other packets. Maybe add a configable logging here?
        };

        let radius = requested_radius.clamp(2, 32);

        // 3. Send World Data
        self.transport
            .send_batch(&[
                McpePacket::from(PacketChunkRadiusUpdate {
                    chunk_radius: radius,
                }),
                McpePacket::from(params.biome_definitions.as_ref().clone()),
                // McpePacket::from(params.available_entities.as_ref().clone()),
                McpePacket::from(PacketPlayStatus {
                    status: PacketPlayStatusStatus::PlayerSpawn,
                }),
                McpePacket::from(params.creative_content.as_ref().clone()),
            ])
            .await?;

        // 4. Loading Screen Handshake (Types 1 & 2)
        loop {
            let pkt = self.transport.recv_packet().await?;

            if let McpePacketData::PacketServerboundLoadingScreen(pk) = pkt.data
                && pk.type_ == 1
            {
                break;
            }
        }

        let end_loading = tokio::time::timeout(std::time::Duration::from_secs(60), async {
            loop {
                let pkt = self.transport.recv_packet().await?;

                if let McpePacketData::PacketServerboundLoadingScreen(pk) = pkt.data
                    && pk.type_ == 2
                {
                    return Ok(());
                }
            }
        })
        .await;

        match end_loading {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                return Err(ProtocolError::UnexpectedHandshake(
                    "Timeout waiting for EndLoadingScreen".into(),
                )
                .into());
            }
        }

        tracing::debug!("Client finished loading");
        // 5. Wait for SetLocalPlayerAsInitialized
        loop {
            let pkt = self.transport.recv_packet().await?;

            if matches!(
                pkt.data,
                McpePacketData::PacketSetLocalPlayerAsInitialized(_)
            ) {
                break;
            }
        }

        tracing::debug!("Client initialized, entering Play state");
        Ok(BedrockStream {
            transport: self.transport,
            state: Play,
            _role: PhantomData,
        })
    }
}

// --- State: Play ---

impl<T: Transport> BedrockStream<Play, Server, T> {
    #[instrument(skip_all, level = "trace")]
    pub async fn recv_packet(&mut self) -> Result<McpePacket, JolyneError> {
        let mut batches = self.transport.recv_batch().await?;

        if let Some(pkt) = batches.pop() {
            Ok(pkt)
        } else {
            Err(JolyneError::ConnectionClosed)
        }
    }

    #[instrument(skip_all, level = "trace")]
    pub async fn send_packet(&mut self, packet: McpePacket) -> Result<(), JolyneError> {
        self.transport.send(packet).await
    }
}
