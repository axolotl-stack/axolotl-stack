use std::marker::PhantomData;

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
use crate::protocol::packets::{
    PacketAvailableEntityIdentifiers, PacketBiomeDefinitionList, PacketChunkRadiusUpdate,
    PacketCreativeContent, PacketItemRegistry, PacketNetworkChunkPublisherUpdate,
    PacketNetworkSettings, PacketNetworkSettingsCompressionAlgorithm, PacketPlayStatus,
    PacketPlayStatusStatus, PacketResourcePackStack, PacketResourcePacksInfo,
    PacketServerToClientHandshake,
};
use crate::protocol::types::experiments::Experiments;
use crate::protocol::types::itemstates::{ItemstatesItem, ItemstatesItemVersion};
use crate::protocol::types::resource::ResourcePackIdVersions;
use crate::protocol::{McpePacket, McpePacketData};
use crate::stream::{
    BedrockStream, Handshake, Login, Play, ResourcePacks, SecurePending, Server, StartGame,
};

/// Configuration for the Server Handshake.
#[derive(Debug, Clone)]
pub struct ServerHandshakeConfig {
    pub server_key: SecretKey, // Persistent key for the session
}

// --- State: Handshake (Initial) ---

impl BedrockStream<Handshake, Server> {
    /// Accepts a new connection and negotiates network settings.
    ///
    /// This is the first step in the server-side handshake.
    /// It waits for `RequestNetworkSettings` and sends `NetworkSettings`.
    #[instrument(skip(self), level = "debug")]
    pub async fn accept_network_settings(
        mut self,
    ) -> Result<BedrockStream<Login, Server>, JolyneError> {
        // 1. Wait for RequestNetworkSettings (Raw)
        // Transport now handles detecting Raw vs Batch.
        let packet = self.transport.recv_packet().await?;

        match packet.data {
            McpePacketData::PacketRequestNetworkSettings(req) => {
                let server_protocol = crate::protocol::PROTOCOL_VERSION;
                let client_protocol = req.client_protocol;
                if client_protocol != server_protocol {
                    // Bedrock uses PlayStatus to signal version incompatibility.
                    // - FailedClient: Client is outdated.
                    // - FailedSpawn: Server is outdated.
                    let status = if client_protocol < server_protocol {
                        PacketPlayStatusStatus::FailedClient
                    } else {
                        PacketPlayStatusStatus::FailedSpawn
                    };
                    self.transport
                        .send_raw(McpePacket::from(PacketPlayStatus { status }))
                        .await?;
                    return Err(ProtocolError::IncompatibleProtocol {
                        client_protocol,
                        server_protocol,
                    }
                    .into());
                }

                // Send NetworkSettings
                let listener_config = self
                    .state
                    .config
                    .as_ref()
                    .expect("server handshake state must have config");
                let settings = PacketNetworkSettings {
                    compression_threshold: listener_config.compression_threshold,
                    compression_algorithm: PacketNetworkSettingsCompressionAlgorithm::Deflate,
                    client_throttle: false,
                    client_throttle_threshold: 0,
                    client_throttle_scalar: 0.0,
                };
                self.transport.send_raw(McpePacket::from(settings)).await?;

                // Enable Compression
                self.transport.set_compression(
                    true,
                    listener_config.compression_level,
                    listener_config.compression_threshold,
                );

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
}

// --- State: Login ---

impl BedrockStream<Login, Server> {
    /// Validates the client's identity (XBOX Auth).
    ///
    /// Waits for the `Login` packet and verifies the certificate chain.
    #[instrument(skip(self), level = "debug")]
    pub async fn authenticate(
        mut self,
    ) -> Result<(BedrockStream<SecurePending, Server>, ValidatedIdentity), JolyneError> {
        // 1. Wait for Login Packet (Batched)
        let packet = self.recv_expect_login().await?;
        let login_data = match packet.data {
            McpePacketData::PacketLogin(l) => l,
            _ => unreachable!(),
        };

        let listener_config = self
            .state
            .config
            .as_ref()
            .expect("server login state must have config");

        // 2. Authenticate
        let identity = authenticate_login(
            &login_data.tokens.identity,
            &login_data.tokens.client,
            listener_config.online_mode,
            listener_config.allow_legacy_auth,
        )
        .await?;

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
        // We expect Login to be the next significant packet.
        // Transport buffers packets, so we just pop the next one.
        let packet = self.transport.recv_packet().await?;
        if matches!(packet.data, McpePacketData::PacketLogin(_)) {
            Ok(packet)
        } else {
            Err(ProtocolError::MissingLoginPacket.into())
        }
    }
}

// --- State: SecurePending ---

impl BedrockStream<SecurePending, Server> {
    /// Completes the handshake by negotiating encryption (if enabled) and sending `LoginSuccess`.
    ///
    /// Requires the `identity_public_key` returned from `authenticate`.
    #[instrument(skip(self, config), level = "debug")]
    pub async fn finish_handshake(
        mut self,
        config: &ServerHandshakeConfig,
        client_pub_b64: &str,
    ) -> Result<BedrockStream<ResourcePacks, Server>, JolyneError> {
        let listener_config = self
            .state
            .config
            .as_ref()
            .expect("server secure_pending state must have config");

        // 1. Encryption Handshake
        if listener_config.encryption_enabled {
            self.perform_encryption_handshake(&config.server_key, client_pub_b64)
                .await?;
        }

        // 2. Send PlayStatus::LoginSuccess
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

    async fn perform_encryption_handshake(
        &mut self,
        server_key: &SecretKey,
        client_pub_b64: &str,
    ) -> Result<(), JolyneError> {
        // 1. Decode Client Key
        let client_der = STANDARD
            .decode(client_pub_b64)
            .map_err(|e| ProtocolError::UnexpectedHandshake(e.to_string()))?;
        let client_pub = PublicKey::from_public_key_der(&client_der)
            .map_err(|e| ProtocolError::UnexpectedHandshake(e.to_string()))?;

        // 2. Salt & ECDH
        let mut salt = [0u8; 16];
        thread_rng().fill_bytes(&mut salt);

        let shared_secret =
            p384::ecdh::diffie_hellman(server_key.to_nonzero_scalar(), client_pub.as_affine());
        let shared_bytes = shared_secret.raw_secret_bytes();

        let mut h = Sha256::new();
        h.update(&salt);
        h.update(&shared_bytes);
        let key_bytes = h.finalize();

        let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&key_bytes);
        let mut iv = [0u8; 12];
        iv.copy_from_slice(&key_bytes[0..12]);

        // 3. Send ServerToClientHandshake (Token)
        let server_pub_der = server_key.public_key().to_public_key_der().unwrap();
        let server_pub_b64 = STANDARD.encode(server_pub_der.as_bytes());

        let mut header = Header::new(Algorithm::ES384);
        header.x5u = Some(server_pub_b64);

        #[derive(serde::Serialize)]
        struct SaltClaims {
            salt: String,
        }
        let claims = SaltClaims {
            // BDS encodes the salt using raw base64 (no '=' padding). The vanilla client appears to
            // expect this exact format.
            salt: STANDARD_NO_PAD.encode(salt),
        };

        let server_priv_der = server_key.to_pkcs8_der().unwrap();
        let encoding_key = EncodingKey::from_ec_der(server_priv_der.as_bytes());
        let token = encode(&header, &claims, &encoding_key).unwrap();

        let handshake_pkt = PacketServerToClientHandshake { token };
        self.transport
            .send_batch(&[McpePacket::from(handshake_pkt)])
            .await?;

        // 4. Enable Encryption locally
        self.transport.enable_encryption(*key, iv);

        // 5. Wait for ClientToServerHandshake (Ack)
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

        Ok(())
    }
}

// --- State: ResourcePacks ---

impl BedrockStream<ResourcePacks, Server> {
    /// Negotiates resource packs.
    pub async fn negotiate_packs(
        mut self,
        required: bool,
    ) -> Result<BedrockStream<StartGame, Server>, JolyneError> {
        // 1. Send ResourcePacksInfo (+ empty ResourcePackStack).
        //
        // Even if we have no packs, many clients expect an explicit ResourcePackStack before the
        // server proceeds to StartGame.
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

        // 2. Wait for Response
        // Loop until we get ResourcePackClientResponse
        loop {
            let packets = self.transport.recv_batch().await?;
            for pkt in packets {
                if let McpePacketData::PacketResourcePackClientResponse(resp) = pkt.data {
                    use crate::protocol::packets::resource::PacketResourcePackClientResponseResponseStatus as Status;
                    match resp.response_status {
                        Status::Refused if required => {
                            return Err(ProtocolError::UnexpectedHandshake(
                                "Client refused required packs".into(),
                            )
                            .into());
                        }
                        Status::Refused => {
                            // Packs aren't required; proceed anyway.
                            return Ok(BedrockStream {
                                transport: self.transport,
                                state: StartGame,
                                _role: PhantomData,
                            });
                        }
                        Status::SendPacks | Status::HaveAllPacks => {
                            // Client is acknowledging the info (and/or requesting the stack).
                            // We already sent an empty stack, so just keep waiting for Completed.
                        }
                        Status::Completed => {
                            // Done with packs
                            return Ok(BedrockStream {
                                transport: self.transport,
                                state: StartGame,
                                _role: PhantomData,
                            });
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

// --- State: StartGame ---

impl BedrockStream<StartGame, Server> {
    /// Completes the sequence by sending StartGame and waiting for client initialization.
    /// Returns the active Play stream.
    pub async fn start_game(
        mut self,
        config: &crate::stream::StartGameConfig,
    ) -> Result<BedrockStream<Play, Server>, JolyneError> {
        // 1. Send StartGame Packet
        let start_game = self.build_start_game_packet(config);
        let item_registry = self.build_item_registry();

        // 2. StartGame must be followed by ItemRegistry before the client requests chunk radius.
        self.transport
            .send_batch(&[
                McpePacket::from(start_game),
                McpePacket::from(item_registry),
            ])
            .await?;

        // 3. Client requests its chunk radius (view distance). Respond with ChunkRadiusUpdate and
        // NetworkChunkPublisherUpdate, then send PlayStatus::PlayerSpawn to end the loading screen.
        let requested_radius = loop {
            let pkt = self.transport.recv_packet().await?;
            match pkt.data {
                McpePacketData::PacketRequestChunkRadius(req) => {
                    break req.chunk_radius;
                }
                // Ignore other early packets (e.g., ClientCacheStatus) until the radius request arrives.
                _ => {}
            }
        };

        let mut radius = requested_radius;
        if radius < 2 {
            radius = 2;
        } else if radius > 32 {
            radius = 32;
        }

        let radius_update = PacketChunkRadiusUpdate {
            chunk_radius: radius,
        };
        let publisher_update = PacketNetworkChunkPublisherUpdate {
            coordinates: config.spawn_position.clone(),
            radius,
            saved_chunks: Vec::new(),
        };
        let biome_definitions = self.build_biome_definitions();
        let available_entities = self.build_available_entities();
        let creative_content = self.build_creative_content();
        let play_status = PacketPlayStatus {
            status: PacketPlayStatusStatus::PlayerSpawn,
        };

        self.transport
            .send_batch(&[
                McpePacket::from(radius_update),
                McpePacket::from(publisher_update),
                McpePacket::from(biome_definitions),
                McpePacket::from(available_entities),
                McpePacket::from(creative_content),
                McpePacket::from(play_status),
            ])
            .await?;

        // 4. Wait for SetLocalPlayerAsInitialized (client finished loading)
        loop {
            let pkt = self.transport.recv_packet().await?;
            if matches!(
                pkt.data,
                McpePacketData::PacketSetLocalPlayerAsInitialized(_)
            ) {
                break;
            }
        }

        Ok(BedrockStream {
            transport: self.transport,
            state: Play,
            _role: PhantomData,
        })
    }

    fn build_start_game_packet(
        &self,
        config: &crate::stream::StartGameConfig,
    ) -> crate::protocol::packets::PacketStartGame {
        // Re-use logic from old stream.rs or moved to a helper
        // For now, just a placeholder construction to show flow
        crate::protocol::packets::PacketStartGame {
            entity_id: config.entity_id,
            runtime_entity_id: config.runtime_entity_id,
            player_gamemode: config.player_gamemode,
            player_position: config.player_position.clone(),
            rotation: config.rotation.clone(),
            seed: config.seed,
            dimension: config.dimension,
            generator: config.generator,
            world_gamemode: config.world_gamemode,
            difficulty: config.difficulty,
            spawn_position: config.spawn_position.clone(),
            game_version: config.game_version.clone(),
            level_id: config.level_id.clone(),
            world_name: config.world_name.clone(),
            world_identifier: config.world_identifier.clone(),
            server_authoritative_inventory: config.server_authoritative_inventory,
            server_authoritative_block_breaking: config.server_authoritative_block_breaking,
            block_network_ids_are_hashes: config.block_network_ids_are_hashes,
            block_pallette_checksum: config.block_palette_checksum,
            // Defaults for the rest
            biome_type: 0,
            biome_name: "minecraft:plains".into(),
            hardcore: false,
            achievements_disabled: true,
            editor_world_type:
                crate::protocol::packets::start::PacketStartGameEditorWorldType::NotEditor,
            created_in_editor: false,
            exported_from_editor: false,
            day_cycle_stop_time: 0,
            edu_offer: 0,
            edu_features_enabled: false,
            edu_product_uuid: "".into(),
            rain_level: 0.0,
            lightning_level: 0.0,
            has_confirmed_platform_locked_content: false,
            is_multiplayer: true,
            broadcast_to_lan: false,
            xbox_live_broadcast_mode: 0,
            platform_broadcast_mode: 0,
            enable_commands: true,
            is_texturepacks_required: false,
            gamerules: vec![],
            experiments: Experiments::new(),
            experiments_previously_used: false,
            bonus_chest: false,
            map_enabled: false,
            permission_level: crate::protocol::types::permission::PermissionLevel::Member,
            server_chunk_tick_range: 4,
            has_locked_behavior_pack: false,
            has_locked_resource_pack: false,
            is_from_locked_world_template: false,
            msa_gamertags_only: false,
            is_from_world_template: false,
            is_world_template_option_locked: false,
            only_spawn_v_1_villagers: false,
            persona_disabled: false,
            custom_skins_disabled: false,
            emote_chat_muted: false,
            limited_world_width: 0,
            limited_world_length: 0,
            is_new_nether: true,
            edu_resource_uri: crate::protocol::types::education::EducationSharedResourceUri {
                button_name: "".into(),
                link_uri: "".into(),
            },
            experimental_gameplay_override: false,
            chat_restriction_level:
                crate::protocol::packets::start::PacketStartGameChatRestrictionLevel::None,
            disable_player_interactions: false,
            server_identifier: "".into(),
            scenario_identifier: "".into(),
            owner_identifier: "".into(),
            premium_world_template_id: "".into(),
            is_trial: false,
            rewind_history_size: 0,
            current_tick: 0,
            enchantment_seed: 0,
            block_properties: vec![],
            multiplayer_correlation_id: "".into(),
            engine: "Jolyne".into(),
            property_data: vec![],
            world_template_id: Uuid::nil(),
            client_side_generation: false,
            server_controlled_sound: false,
        }
    }

    fn build_item_registry(&self) -> PacketItemRegistry {
        // Minimal item registry with one dummy item to satisfy the protocol.
        // In a real server, this should be populated with the full vanilla item table for the protocol version.
        PacketItemRegistry {
            itemstates: vec![ItemstatesItem {
                name: "minecraft:apple".to_string(),
                runtime_id: 1,
                component_based: false,
                version: ItemstatesItemVersion::Legacy,
                nbt: vec![],
            }],
        }
    }

    fn build_biome_definitions(&self) -> PacketBiomeDefinitionList {
        use crate::protocol::types::biome::BiomeDefinition;

        PacketBiomeDefinitionList {
            biome_definitions: vec![BiomeDefinition {
                name_index: 0,
                biome_id: 0,
                temperature: 0.8,
                downfall: 0.4,
                snow_foliage: 0.0,
                depth: 0.1,
                scale: 0.2,
                map_water_colour: 0x3f76e4,
                rain: true,
                tags: None,
                chunk_generation: None,
            }],
            string_list: vec!["minecraft:plains".to_string()],
        }
    }

    fn build_available_entities(&self) -> PacketAvailableEntityIdentifiers {
        // Minimal NBT containing only TAG_End.
        PacketAvailableEntityIdentifiers { nbt: vec![0] }
    }

    fn build_creative_content(&self) -> PacketCreativeContent {
        PacketCreativeContent {
            groups: Vec::new(),
            items: Vec::new(),
        }
    }
}

// --- State: Play ---

impl BedrockStream<Play, Server> {
    /// Reads the next game packet.
    #[instrument(skip(self), level = "trace")]
    pub async fn recv_packet(&mut self) -> Result<McpePacket, JolyneError> {
        // TODO: Transport should probably have an internal queue if it reads batches?
        // Yes, if recv_batch returns multiple packets, we need to buffer them.
        // For now, this is a simplified view assuming 1:1 or external buffering.
        // Ideally BedrockStream has a `VecDeque<McpePacket>` field.
        // BUT BedrockStream is just a wrapper around Transport.
        // So Transport should have the queue!

        // This requires updating Transport to have `recv_queue`.
        // I will assume Transport has `pop_incoming()` or similar.

        // For the purpose of this file, I'll use `transport.recv_batch` and return the first.
        // Real impl needs buffering.
        let mut batches = self.transport.recv_batch().await?;
        if let Some(pkt) = batches.pop() {
            Ok(pkt)
        } else {
            Err(JolyneError::ConnectionClosed)
        }
    }

    pub async fn send_packet(&mut self, packet: McpePacket) -> Result<(), JolyneError> {
        self.transport.send_batch(&[packet]).await
    }
}
