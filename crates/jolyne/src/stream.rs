use crate::auth::{ValidatedIdentity, authenticate_login};
use crate::batch::{decode_batch, encode_batch, encode_batch_multi}; // Import batch helpers
use crate::config::BedrockListenerConfig;
use crate::error::{JolyneError, ProtocolError};
use aes_gcm::{AeadInPlace, Aes256Gcm, Key, KeyInit, Nonce};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use bytes::BytesMut;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use p384::{
    PublicKey, SecretKey,
    pkcs8::{DecodePublicKey, EncodePrivateKey, EncodePublicKey},
};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::net::SocketAddr;
use tokio_raknet::transport::RaknetStream;
use tracing::{info, instrument, trace};

#[derive(serde::Serialize)]
struct SaltClaims {
    salt: String,
}
use crate::protocol::{
    ItemstatesItemVersion,
    packets::{
        PacketChunkRadiusUpdate, PacketCreativeContent, PacketDisconnect, PacketItemRegistry,
        PacketNetworkChunkPublisherUpdate, PacketNetworkSettings,
        PacketNetworkSettingsCompressionAlgorithm, PacketPlayStatus, PacketPlayStatusStatus,
        PacketResourcePackStack, PacketResourcePacksInfo, PacketServerToClientHandshake,
        PacketStartGame, PacketUpdateBlockProperties,
    },
    types::{
        block::BlockCoordinates,
        disconnect::DisconnectFailReason,
        education::EducationSharedResourceUri,
        experiments::Experiments,
        game::{GameMode, GameRuleVarint},
        itemstates::ItemstatesItem,
        mcpe::{McpePacket, McpePacketData},
        permission::PermissionLevel,
        resource::ResourcePackIdVersions,
        vec::{Vec2F, Vec3F},
    },
};
use uuid::Uuid;
use valentine::bedrock::{
    codec::BedrockCodec,
    context::BedrockSession as ValentineBedrockSession, // Alias to avoid conflict
};

#[derive(Debug, Clone)]
pub struct StartGameConfig {
    pub entity_id: i64,
    pub runtime_entity_id: i64,
    pub spawn_position: BlockCoordinates,
    pub player_position: Vec3F,
    pub rotation: Vec2F,
    pub world_name: String,
    pub level_id: String,
    pub world_identifier: String,
    pub game_version: String,
    pub seed: u64,
    pub generator: i32,
    pub dimension: crate::protocol::packets::start::PacketStartGameDimension,
    pub player_gamemode: GameMode,
    pub world_gamemode: GameMode,
    pub difficulty: i32,
    pub server_authoritative_inventory: bool,
    pub server_authoritative_block_breaking: bool,
    pub block_network_ids_are_hashes: bool,
    pub block_palette_checksum: u64,
}

impl Default for StartGameConfig {
    fn default() -> Self {
        Self {
            entity_id: 1,
            runtime_entity_id: 1,
            spawn_position: BlockCoordinates { x: 0, y: 64, z: 0 },
            player_position: Vec3F {
                x: 0.5,
                y: 65.0,
                z: 0.5,
            },
            rotation: Vec2F { x: 0.0, z: 0.0 },
            world_name: "world".to_string(),
            level_id: "world".to_string(),
            world_identifier: "world".to_string(),
            game_version: crate::protocol::GAME_VERSION.to_string(),
            seed: 0,
            generator: 1,
            dimension: crate::protocol::packets::start::PacketStartGameDimension::Overworld,
            player_gamemode: GameMode::Survival,
            world_gamemode: GameMode::Survival,
            difficulty: 1,
            server_authoritative_inventory: false,
            server_authoritative_block_breaking: false,
            block_network_ids_are_hashes: false,
            block_palette_checksum: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionSide {
    Client,
    Server,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum HandshakeState {
    Initial,
    NetworkSettingsSent,     // Server sent NetworkSettings, waiting for Login
    NetworkSettingsReceived, // Client received settings, ready to login
    EncryptionHandshake,     // Waiting for ClientToServerHandshake
    ResourcePacksInfoSent,
    ResourcePackStackSent,
    ResourcePackComplete,
    ReadyToSpawn,
    WaitingForLocalPlayerInit,
    InGame,
}

pub struct BedrockStream {
    inner: RaknetStream,
    session: ValentineBedrockSession, // Using the alias here
    recv_queue: VecDeque<McpePacket>,
    compression_level: u32,
    compression_threshold: u16,
    compression_enabled: bool,
    max_decompressed_batch_size: Option<usize>,
    encryption_enabled: bool,
    encryption_active: bool,
    handshake_salt: Option<Vec<u8>>,
    server_key: SecretKey,
    send_key: Option<Aes256Gcm>,
    recv_key: Option<Aes256Gcm>,
    send_iv: Option<[u8; 12]>,
    recv_iv: Option<[u8; 12]>,
    send_counter: u32,
    recv_counter: u32,
    require_resource_packs: bool,
    resource_stack_sent: bool,
    side: ConnectionSide,
    handshake_state: HandshakeState,
    listener_config: BedrockListenerConfig, // Used by Server side
    peer_identity: Option<ValidatedIdentity>,
    start_config: StartGameConfig,
    initial_chunk_radius: i32,
    sent_spawn_chunk: bool,
}

impl BedrockStream {
    pub fn new(
        inner: RaknetStream,
        side: ConnectionSide,
        listener_config: BedrockListenerConfig,
    ) -> Self {
        let mut rng = rand::thread_rng();
        let server_key = SecretKey::random(&mut rng);

        Self {
            inner,
            session: ValentineBedrockSession { shield_item_id: 0 }, // Default session
            recv_queue: VecDeque::new(),
            compression_level: listener_config.compression_level, // Use configured compression
            compression_threshold: listener_config.compression_threshold,
            compression_enabled: false, // Starts uncompressed
            max_decompressed_batch_size: listener_config.max_decompressed_batch_size,
            encryption_enabled: listener_config.encryption_enabled,
            encryption_active: false,
            handshake_salt: None,
            server_key,
            send_key: None,
            recv_key: None,
            send_iv: None,
            recv_iv: None,
            send_counter: 0,
            recv_counter: 0,
            require_resource_packs: listener_config.require_resource_packs,
            resource_stack_sent: false,
            side,
            handshake_state: HandshakeState::Initial,
            listener_config,
            peer_identity: None,
            start_config: StartGameConfig::default(),
            initial_chunk_radius: 6,
            sent_spawn_chunk: false,
        }
    }

    pub fn set_compression_level(&mut self, level: u32) {
        self.compression_level = level;
    }

    pub fn set_compression_threshold(&mut self, threshold: u16) {
        self.compression_threshold = threshold;
    }

    pub fn start_config_mut(&mut self) -> &mut StartGameConfig {
        &mut self.start_config
    }

    fn build_resource_packs_info(&self) -> PacketResourcePacksInfo {
        PacketResourcePacksInfo {
            must_accept: self.require_resource_packs,
            has_addons: false,
            has_scripts: false,
            disable_vibrant_visuals: false,
            world_template:
                crate::protocol::packets::resource::PacketResourcePacksInfoWorldTemplate {
                    uuid: Uuid::nil(),
                    version: "1.0.0".to_string(),
                },
            texture_packs: vec![],
        }
    }

    fn build_resource_pack_stack(&self) -> PacketResourcePackStack {
        PacketResourcePackStack {
            must_accept: self.require_resource_packs,
            resource_packs: ResourcePackIdVersions::new(),
            game_version: crate::protocol::GAME_VERSION.to_string(),
            experiments: Experiments::new(),
            experiments_previously_used: false,
            has_editor_packs: false,
        }
    }

    fn build_start_game(&self) -> PacketStartGame {
        PacketStartGame {
            entity_id: self.start_config.entity_id,
            runtime_entity_id: self.start_config.runtime_entity_id,
            player_gamemode: self.start_config.player_gamemode,
            player_position: self.start_config.player_position.clone(),
            rotation: self.start_config.rotation.clone(),
            seed: self.start_config.seed,
            biome_type: 0,
            biome_name: "minecraft:plains".to_string(),
            dimension: self.start_config.dimension,
            generator: self.start_config.generator,
            world_gamemode: self.start_config.world_gamemode,
            hardcore: false,
            difficulty: self.start_config.difficulty,
            spawn_position: self.start_config.spawn_position.clone(),
            achievements_disabled: true,
            editor_world_type:
                crate::protocol::packets::start::PacketStartGameEditorWorldType::NotEditor,
            created_in_editor: false,
            exported_from_editor: false,
            day_cycle_stop_time: 0,
            edu_offer: 0,
            edu_features_enabled: false,
            edu_product_uuid: "".to_string(),
            rain_level: 0.0,
            lightning_level: 0.0,
            has_confirmed_platform_locked_content: false,
            is_multiplayer: true,
            broadcast_to_lan: false,
            xbox_live_broadcast_mode: 0,
            platform_broadcast_mode: 0,
            enable_commands: true,
            is_texturepacks_required: self.require_resource_packs,
            gamerules: Vec::<GameRuleVarint>::new(),
            experiments: Experiments::new(),
            experiments_previously_used: false,
            bonus_chest: false,
            map_enabled: false,
            permission_level: PermissionLevel::Member,
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
            game_version: self.start_config.game_version.clone(),
            limited_world_width: 0,
            limited_world_length: 0,
            is_new_nether: true,
            edu_resource_uri: EducationSharedResourceUri {
                button_name: "".to_string(),
                link_uri: "".to_string(),
            },
            experimental_gameplay_override: false,
            chat_restriction_level:
                crate::protocol::packets::start::PacketStartGameChatRestrictionLevel::None,
            disable_player_interactions: false,
            server_identifier: "".to_string(),
            world_identifier: self.start_config.world_identifier.clone(),
            scenario_identifier: "".to_string(),
            owner_identifier: "".to_string(),
            level_id: self.start_config.level_id.clone(),
            world_name: self.start_config.world_name.clone(),
            premium_world_template_id: "".to_string(),
            is_trial: false,
            rewind_history_size: 0,
            server_authoritative_block_breaking: self
                .start_config
                .server_authoritative_block_breaking,
            current_tick: 0,
            enchantment_seed: 0,
            block_properties: Vec::new(),
            multiplayer_correlation_id: "".to_string(),
            server_authoritative_inventory: self.start_config.server_authoritative_inventory,
            engine: "Jolyne".to_string(),
            property_data: vec![],
            block_pallette_checksum: self.start_config.block_palette_checksum,
            world_template_id: Uuid::nil(),
            client_side_generation: false,
            block_network_ids_are_hashes: self.start_config.block_network_ids_are_hashes,
            server_controlled_sound: false,
        }
    }

    fn build_item_registry(&self) -> PacketItemRegistry {
        // Minimal item registry with one dummy item to satisfy the protocol.
        // In a real server, this would be populated from a data source.
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

    fn build_biome_definitions(
        &self,
    ) -> crate::protocol::packets::biome::PacketBiomeDefinitionList {
        use crate::protocol::types::biome::BiomeDefinition;
        crate::protocol::packets::biome::PacketBiomeDefinitionList {
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

    fn build_available_entities(
        &self,
    ) -> crate::protocol::packets::available::PacketAvailableEntityIdentifiers {
        // Minimal NBT containing only TAG_End.
        crate::protocol::packets::available::PacketAvailableEntityIdentifiers { nbt: vec![0] }
    }

    fn build_creative_content(&self) -> PacketCreativeContent {
        PacketCreativeContent {
            groups: Vec::new(),
            items: Vec::new(),
        }
    }

    async fn complete_resource_pack_phase(&mut self) -> Result<(), JolyneError> {
        // If we've already moved past this gate, no-op.
        if matches!(
            self.handshake_state,
            HandshakeState::ReadyToSpawn
                | HandshakeState::WaitingForLocalPlayerInit
                | HandshakeState::InGame
        ) {
            return Ok(());
        }
        self.handshake_state = HandshakeState::ResourcePackComplete;
        self.enter_ready_to_spawn().await?;
        Ok(())
    }

    fn build_empty_chunk(
        &self,
        x: i32,
        z: i32,
    ) -> crate::protocol::packets::level::PacketLevelChunk {
        crate::protocol::packets::level::PacketLevelChunk {
            x,
            z,
            dimension: self.start_config.dimension as i32,
            sub_chunk_count: 0,
            highest_subchunk_count: None,
            blobs: None,
            payload: vec![0],
        }
    }

    async fn send_disconnect(
        &mut self,
        reason: DisconnectFailReason,
        message: &str,
    ) -> Result<(), JolyneError> {
        let packet = PacketDisconnect {
            reason,
            content: Some(
                crate::protocol::packets::disconnect::PacketDisconnectContent {
                    message: message.to_string(),
                    filtered_message: "".to_string(),
                },
            ),
        };
        // best-effort send (encrypted if active)
        let mut buf = BytesMut::new();
        McpePacket::from(packet).encode(&mut buf)?;
        if self.encryption_active {
            let _ = self.encrypt_outgoing(&mut buf);
        }
        let _ = self.inner.send(buf.freeze()).await;
        Ok(())
    }

    fn derive_encryption_keys(
        &mut self,
        client_pub: &PublicKey,
        salt: &[u8],
    ) -> Result<(), JolyneError> {
        // ECDH
        let shared_secret =
            p384::ecdh::diffie_hellman(self.server_key.to_nonzero_scalar(), client_pub.as_affine());
        let shared_bytes = shared_secret.raw_secret_bytes(); // 48 bytes for P-384

        // SHA256 KDF (Same Key for both directions)
        let mut h = Sha256::new();
        h.update(salt);
        h.update(shared_bytes);
        let key_bytes = h.finalize(); // 32 bytes

        // AES-256-GCM uses 32-byte key.
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);

        trace!(

            shared_secret = ?STANDARD.encode(shared_bytes),

            salt = ?STANDARD.encode(salt),

            derived_key = ?STANDARD.encode(key_bytes),

            "Derived Encryption Keys"

        );

        // IV is the first 12 bytes of the key (according to gophertunnel behavior inference)

        let mut iv = [0u8; 12];
        iv.copy_from_slice(&key_bytes[0..12]);

        self.send_key = Some(Aes256Gcm::new(key));
        self.recv_key = Some(Aes256Gcm::new(key));
        self.send_iv = Some(iv);
        self.recv_iv = Some(iv);
        self.send_counter = 0;
        self.recv_counter = 0;

        Ok(())
    }

    async fn initiate_encryption_handshake(&mut self) -> Result<(), JolyneError> {
        let client_pub_key = self
            .peer_identity
            .as_ref()
            .map(|id| id.identity_public_key.clone())
            .ok_or_else(|| {
                ProtocolError::UnexpectedHandshake("Missing client identity".to_string())
            })?;

        // Decode client public key
        let client_der = STANDARD.decode(&client_pub_key).map_err(|e| {
            ProtocolError::UnexpectedHandshake(format!("Invalid client pubkey b64: {e}"))
        })?;
        let client_pub = PublicKey::from_public_key_der(&client_der).map_err(|e| {
            ProtocolError::UnexpectedHandshake(format!("Invalid client pubkey DER: {e}"))
        })?;

        // 1. Generate Salt
        let mut salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut salt);
        self.handshake_salt = Some(salt.to_vec());

        // 2. Derive Shared Secret & Keys
        self.derive_encryption_keys(&client_pub, &salt)?;

        // 3. Create JWT Token
        let public_key_der = self
            .server_key
            .public_key()
            .to_public_key_der()
            .map_err(|e| {
                ProtocolError::UnexpectedHandshake(format!("Failed to export server pubkey: {e}"))
            })?;

        let public_key_b64 = STANDARD.encode(public_key_der.as_bytes());
        let mut header = Header::new(Algorithm::ES384);
        header.x5u = Some(public_key_b64.clone());

        let claims = SaltClaims {
            salt: STANDARD.encode(salt),
        };

        let server_key_der = self.server_key.to_pkcs8_der().map_err(|e| {
            ProtocolError::UnexpectedHandshake(format!("Failed to export server privkey: {e}"))
        })?;
        let encoding_key = EncodingKey::from_ec_der(server_key_der.as_bytes());
        let token = encode(&header, &claims, &encoding_key).map_err(|e| {
            ProtocolError::UnexpectedHandshake(format!(
                "Failed to sign server handshake token: {e}"
            ))
        })?;
        trace!(?token, "Generated Handshake Token");
        trace!(
            salt_b64 = ?claims.salt,
            x5u_b64 = ?public_key_b64,
            "Handshake Token Details"
        );

        // 4. Send ServerToClientHandshake
        let packet = PacketServerToClientHandshake { token };
        trace!("Sending ServerToClientHandshake");

        // Manually encode and send to force no compression (threshold MAX) for debugging
        let mcpe_packet = McpePacket::from(packet);
        let batch_buffer = encode_batch(
            &mcpe_packet,
            self.compression_enabled,
            self.compression_level,
            self.compression_threshold,
        )?;

        trace!(
            len = batch_buffer.len(),
            hex = ?batch_buffer.as_ref(),
            "Sending uncompressed ServerToClientHandshake"
        );

        if self.encryption_active {
            let mut bm = BytesMut::from(batch_buffer.as_ref());
            self.encrypt_outgoing(&mut bm)?;
            self.inner.send(bm.freeze()).await?;
        } else {
            self.inner.send(batch_buffer).await?;
        }
        // self.send(McpePacket::from(packet)).await?;

        // 5. Enable Encryption immediately
        self.encryption_active = true;
        self.handshake_state = HandshakeState::EncryptionHandshake;
        trace!("Encryption active, waiting for ClientToServerHandshake");

        Ok(())
    }
    fn encrypt_outgoing(&mut self, buf: &mut BytesMut) -> Result<(), JolyneError> {
        if !self.encryption_active {
            return Ok(());
        }
        let cipher = self
            .send_key
            .as_ref()
            .ok_or_else(|| ProtocolError::UnexpectedHandshake("Send cipher missing".to_string()))?;
        let iv_base = self
            .send_iv
            .ok_or_else(|| ProtocolError::UnexpectedHandshake("Send IV missing".to_string()))?;
        let counter = self.send_counter;
        self.send_counter = self.send_counter.wrapping_add(1);

        let mut iv = iv_base;
        let ctr = counter as u64; // expand to u64
        let ctr_bytes = ctr.to_le_bytes();
        iv[4..12].copy_from_slice(&ctr_bytes);

        let nonce = Nonce::from(<[u8; 12]>::try_from(iv).map_err(|_| {
            ProtocolError::UnexpectedHandshake("Nonce derivation failed".to_string())
        })?);

        let mut data = buf.to_vec();
        cipher
            .encrypt_in_place(&nonce, b"", &mut data)
            .map_err(|e| {
                JolyneError::Protocol(ProtocolError::UnexpectedHandshake(format!(
                    "Encrypt failed: {e}"
                )))
            })?;
        buf.clear();
        buf.extend_from_slice(&data);
        Ok(())
    }

    fn decrypt_incoming(&mut self, buf: &mut BytesMut) -> Result<(), JolyneError> {
        if !self.encryption_active {
            return Ok(());
        }
        let cipher = self
            .recv_key
            .as_ref()
            .ok_or_else(|| ProtocolError::UnexpectedHandshake("Recv cipher missing".to_string()))?;
        let iv_base = self
            .recv_iv
            .ok_or_else(|| ProtocolError::UnexpectedHandshake("Recv IV missing".to_string()))?;
        let counter = self.recv_counter;
        self.recv_counter = self.recv_counter.wrapping_add(1);

        let mut iv = iv_base;
        let ctr = counter as u64;
        let ctr_bytes = ctr.to_le_bytes();
        iv[4..12].copy_from_slice(&ctr_bytes);

        let nonce = Nonce::from(<[u8; 12]>::try_from(iv).map_err(|_| {
            ProtocolError::UnexpectedHandshake("Nonce derivation failed".to_string())
        })?);

        let mut data = buf.to_vec();
        cipher
            .decrypt_in_place(&nonce, b"", &mut data)
            .map_err(|e| {
                JolyneError::Protocol(ProtocolError::UnexpectedHandshake(format!(
                    "Decrypt failed: {e}"
                )))
            })?;
        buf.clear();
        buf.extend_from_slice(&data);
        Ok(())
    }

    #[instrument(skip(self, packet), level = "debug")]
    pub async fn send(&mut self, packet: McpePacket) -> Result<(), JolyneError> {
        // Send initial handshake packets without batching/compression.
        let handshake_raw = matches!(
            packet.data,
            McpePacketData::PacketRequestNetworkSettings(_)
                | McpePacketData::PacketNetworkSettings(_)
        );

        if handshake_raw {
            let mut buf = BytesMut::new();
            packet.encode(&mut buf)?; // This uses McpePacketData::encode_game_frame (0xFE [Len][Header][Body])
            if self.encryption_active {
                self.encrypt_outgoing(&mut buf)?;
            }
            self.inner.send(buf.freeze()).await?;
            // If it's a NetworkSettings packet being sent, compression should be enabled afterwards.
            if matches!(packet.data, McpePacketData::PacketNetworkSettings(_)) {
                self.compression_enabled = true;
                self.handshake_state = HandshakeState::NetworkSettingsSent;
            }
            Ok(())
        } else {
            // Wrap in batch; compression decision is based on negotiated settings.
            let batch_buffer = encode_batch(
                &packet,
                self.compression_enabled,
                self.compression_level,
                self.compression_threshold,
            )?;
            trace!(
                len = batch_buffer.len(),
                hex = ?batch_buffer.as_ref(),
                "Sending batched packet"
            );
            if self.encryption_active {
                let mut bm = BytesMut::from(batch_buffer.as_ref());
                self.encrypt_outgoing(&mut bm)?;
                self.inner.send(bm.freeze()).await?;
            } else {
                self.inner.send(batch_buffer).await?;
            }
            Ok(())
        }
    }

    /// Sends multiple packets in a single batch frame when possible.
    #[instrument(skip(self, packets), level = "debug")]
    pub async fn send_many(&mut self, packets: &[McpePacket]) -> Result<(), JolyneError> {
        if packets.is_empty() {
            return Ok(());
        }
        // Fallback to per-packet send if any handshake-raw packets are present.
        let contains_handshake_raw = packets.iter().any(|p| {
            matches!(
                p.data,
                McpePacketData::PacketRequestNetworkSettings(_)
                    | McpePacketData::PacketNetworkSettings(_)
            )
        });
        if contains_handshake_raw {
            for p in packets {
                self.send(p.clone()).await?;
            }
            return Ok(());
        }

        let batch_buffer = encode_batch_multi(
            packets,
            self.compression_enabled,
            self.compression_level,
            self.compression_threshold,
        )?;
        if self.encryption_active {
            let mut bm = BytesMut::from(batch_buffer.as_ref());
            self.encrypt_outgoing(&mut bm)?;
            self.inner.send(bm.freeze()).await?;
        } else {
            self.inner.send(batch_buffer).await?;
        }
        Ok(())
    }

    #[instrument(skip(self), level = "debug")]
    pub async fn recv(&mut self) -> Result<McpePacket, JolyneError> {
        loop {
            // If there are packets in the queue, return them first
            if let Some(packet) = self.recv_queue.pop_front() {
                return Ok(packet);
            }

            // Read from RakNet
            let mut packet_bytes = self
                .inner
                .recv()
                .await
                .ok_or(JolyneError::ConnectionClosed)??;

            trace!(
                len = packet_bytes.len(),
                "Received raw packet bytes from RakNet"
            );

            // Decrypt if needed
            if self.encryption_active {
                let mut bm = BytesMut::from(packet_bytes.as_ref());
                if let Err(e) = self.decrypt_incoming(&mut bm) {
                    let _ = self
                        .send_disconnect(DisconnectFailReason::BadPacket, "Decryption failed")
                        .await;
                    return Err(e);
                }
                packet_bytes = bm.freeze();
                trace!("Decrypted packet bytes");
            }

            let mut buf = packet_bytes;

            // Handshake State Machine Logic
            trace!(state = ?self.handshake_state, "Processing handshake state");
            match self.handshake_state {
                HandshakeState::Initial => {
                    // Expect a raw packet (RequestNetworkSettings from client, or NetworkSettings from server)
                    if !buf.is_empty() {
                        let packet = McpePacket::decode(&mut buf, (&self.session).into())?;

                        if self.side == ConnectionSide::Server {
                            if let McpePacketData::PacketRequestNetworkSettings(_) = &packet.data {
                                // Server: Auto-reply with NetworkSettings
                                let network_settings_packet =
                                    McpePacket::from(PacketNetworkSettings {
                                        compression_threshold: self.compression_threshold,
                                        compression_algorithm:
                                            PacketNetworkSettingsCompressionAlgorithm::Deflate,
                                        client_throttle: false,
                                        client_throttle_threshold: 0,
                                        client_throttle_scalar: 0.0,
                                    });
                                // Enable compression immediately after sending settings.
                                self.send(network_settings_packet).await?; // raw path
                                self.compression_enabled = true;
                                self.handshake_state = HandshakeState::NetworkSettingsSent;
                                return Ok(packet); // Return the received RequestNetworkSettings
                            } else {
                                let _ = self
                                    .send_disconnect(
                                        DisconnectFailReason::UnexpectedPacket,
                                        "Unexpected packet during handshake",
                                    )
                                    .await;
                                return Err(ProtocolError::UnexpectedHandshake(
                                    "Unexpected raw packet during initial server handshake"
                                        .to_string(),
                                )
                                .into());
                            }
                        } else {
                            // Client logic omitted for brevity
                            return Err(ProtocolError::UnexpectedHandshake(
                                "Client logic not updated".to_string(),
                            )
                            .into());
                        }
                    } else {
                        // Initial state, but received a batch? Error.
                        return Err(ProtocolError::EmptyHandshakePacket.into());
                    }
                }
                HandshakeState::NetworkSettingsSent => {
                    // Server: Expect LoginPacket (batched, may be compressed based on settings sent)
                    let first_byte = buf.first().copied();
                    let len = buf.len();
                    if len == 0 {
                        let _ = self
                            .send_disconnect(
                                DisconnectFailReason::BadPacket,
                                "Empty packet after NetworkSettings",
                            )
                            .await;
                        return Err(ProtocolError::UnexpectedHandshake(
                            "Empty buffer after NetworkSettings".to_string(),
                        )
                        .into());
                    }
                    if first_byte != Some(crate::batch::BATCH_PACKET_ID) {
                        let _ = self
                            .send_disconnect(
                                DisconnectFailReason::BadPacket,
                                "Expected batch after NetworkSettings",
                            )
                            .await;
                        return Err(ProtocolError::UnexpectedHandshake(format!(
                            "Expected batch (0xFE) after NetworkSettings, got {:02x?}",
                            first_byte
                        ))
                        .into());
                    }
                    let packets = decode_batch(
                        &mut buf,
                        &self.session,
                        self.compression_enabled,
                        self.max_decompressed_batch_size,
                    )?;
                    self.recv_queue.extend(packets);

                    if let Some(pos) = self
                        .recv_queue
                        .iter()
                        .position(|p| matches!(p.data, McpePacketData::PacketLogin(_)))
                    {
                        let login_packet = self
                            .recv_queue
                            .remove(pos)
                            .expect("position found but remove failed");

                        if let McpePacketData::PacketLogin(ref login_payload) = login_packet.data {
                            // Validate login
                            let validated_identity = authenticate_login(
                                &login_payload.tokens.identity,
                                &login_payload.tokens.client,
                                self.listener_config.online_mode,
                                self.listener_config.allow_legacy_auth,
                            )
                            .await?;
                            self.peer_identity = Some(validated_identity);

                            if self.encryption_enabled {
                                self.initiate_encryption_handshake().await?;
                            } else {
                                // Skip encryption, go straight to ResourcePack phase
                                // Auto-send PlayStatus::LoginSuccess
                                let play_status_packet = McpePacket::from(PacketPlayStatus {
                                    status: PacketPlayStatusStatus::LoginSuccess,
                                });
                                self.send(play_status_packet).await?;

                                // Send ResourcePacksInfo
                                let packs_info = McpePacket::from(self.build_resource_packs_info());
                                self.send(packs_info).await?;
                                self.handshake_state = HandshakeState::ResourcePacksInfoSent;
                            }
                            return Ok(login_packet);
                        }
                    } else {
                        return Err(ProtocolError::MissingLoginPacket.into());
                    }
                }
                HandshakeState::EncryptionHandshake => {
                    // Expect ClientToServerHandshake (batched, encrypted)
                    // The packet_bytes were already decrypted above if encryption_active is true.
                    let packets = decode_batch(
                        &mut buf,
                        &self.session,
                        self.compression_enabled,
                        self.max_decompressed_batch_size,
                    )?;
                    self.recv_queue.extend(packets);

                    if let Some(pos) = self.recv_queue.iter().position(|p| {
                        matches!(p.data, McpePacketData::PacketClientToServerHandshake(_))
                    }) {
                        let _hs = self.recv_queue.remove(pos).expect("remove failed");

                        // Handshake complete. Send LoginSuccess
                        let play_status_packet = McpePacket::from(PacketPlayStatus {
                            status: PacketPlayStatusStatus::LoginSuccess,
                        });
                        self.send(play_status_packet).await?;

                        // Send ResourcePacks
                        if self.require_resource_packs {
                            let packs_info = McpePacket::from(self.build_resource_packs_info());
                            self.send(packs_info).await?;
                            self.handshake_state = HandshakeState::ResourcePacksInfoSent;
                        } else {
                            let packs_info = McpePacket::from(self.build_resource_packs_info());
                            // Send only Info first, wait for response before sending Stack
                            self.send(packs_info).await?;
                            self.handshake_state = HandshakeState::ResourcePacksInfoSent;
                        }

                        // We return the Handshake packet just so the caller sees it
                        return Ok(_hs);
                    }
                }
                HandshakeState::ResourcePacksInfoSent | HandshakeState::ResourcePackStackSent => {
                    // Expect ResourcePackClientResponse
                    let packets = decode_batch(
                        &mut buf,
                        &self.session,
                        self.compression_enabled,
                        self.max_decompressed_batch_size,
                    )?;
                    self.recv_queue.extend(packets);

                    // Handle ClientCacheStatus if present (it can come during this phase)
                    if let Some(pos) = self
                        .recv_queue
                        .iter()
                        .position(|p| matches!(p.data, McpePacketData::PacketClientCacheStatus(_)))
                    {
                        if self.listener_config.handle_client_cache_status {
                            let _ = self.recv_queue.remove(pos);
                            // Ignore it
                        }
                    }

                    if let Some(pos) = self.recv_queue.iter().position(|p| {
                        matches!(p.data, McpePacketData::PacketResourcePackClientResponse(_))
                    }) {
                        let resp_packet = self
                            .recv_queue
                            .remove(pos)
                            .expect("position found but remove failed");
                        if let McpePacketData::PacketResourcePackClientResponse(resp) =
                            resp_packet.data
                        {
                            use crate::protocol::packets::resource::PacketResourcePackClientResponseResponseStatus as RespStatus;
                            match resp.response_status {
                                RespStatus::SendPacks | RespStatus::HaveAllPacks => {
                                    let stack = self.build_resource_pack_stack();
                                    self.send(McpePacket::from(stack)).await?;
                                    self.resource_stack_sent = true;
                                    self.handshake_state = HandshakeState::ResourcePackStackSent;
                                }
                                RespStatus::Completed => {
                                    if self.require_resource_packs && !self.resource_stack_sent {
                                        return Err(ProtocolError::UnexpectedHandshake(
                                            "Completed without stack exchange".to_string(),
                                        )
                                        .into());
                                    }
                                    self.complete_resource_pack_phase().await?;
                                }
                                RespStatus::Refused => {
                                    if self.require_resource_packs {
                                        let _ = self
                                            .send_disconnect(
                                                DisconnectFailReason::ResourcePackProblem,
                                                "Required",
                                            )
                                            .await;
                                        return Err(ProtocolError::UnexpectedHandshake(
                                            "Refused".to_string(),
                                        )
                                        .into());
                                    } else {
                                        self.complete_resource_pack_phase().await?;
                                    }
                                }
                                RespStatus::None => { /* ignore */ }
                            }
                        }
                    }
                }
                HandshakeState::ResourcePackComplete => {
                    self.complete_resource_pack_phase().await?;
                    let packets = decode_batch(
                        &mut buf,
                        &self.session,
                        self.compression_enabled,
                        self.max_decompressed_batch_size,
                    )?;
                    self.recv_queue.extend(packets);
                }
                HandshakeState::WaitingForLocalPlayerInit => {
                    let packets = decode_batch(
                        &mut buf,
                        &self.session,
                        self.compression_enabled,
                        self.max_decompressed_batch_size,
                    )?;
                    self.recv_queue.extend(packets);

                    if let Some(pos) = self
                        .recv_queue
                        .iter()
                        .position(|p| matches!(p.data, McpePacketData::PacketClientCacheStatus(_)))
                    {
                        if self.listener_config.handle_client_cache_status {
                            let _ = self.recv_queue.remove(pos);
                        }
                    }

                    if let Some(pos) = self.recv_queue.iter().position(|p| {
                        matches!(p.data, McpePacketData::PacketSetLocalPlayerAsInitialized(_))
                    }) {
                        let _pkt = self.recv_queue.remove(pos).expect("remove failed");
                        info!("Received SetLocalPlayerAsInitialized, client is in-game");
                        self.handshake_state = HandshakeState::InGame;
                        // Return it so the user can see it if they want
                        return Ok(_pkt);
                    }
                }
                HandshakeState::ReadyToSpawn
                | HandshakeState::InGame
                | HandshakeState::NetworkSettingsReceived => {
                    // Normal play traffic
                    let packets = decode_batch(
                        &mut buf,
                        &self.session,
                        self.compression_enabled,
                        self.max_decompressed_batch_size,
                    )?;
                    self.recv_queue.extend(packets);

                    if let Some(packet) = self.recv_queue.pop_front() {
                        if let McpePacketData::PacketRequestChunkRadius(req) = &packet.data {
                            let radius = req.chunk_radius;
                            let radius_update = PacketChunkRadiusUpdate {
                                chunk_radius: radius,
                            };

                            let publisher = PacketNetworkChunkPublisherUpdate {
                                coordinates: self.start_config.spawn_position.clone(),
                                radius,
                                saved_chunks: Vec::new(),
                            };

                            self.send_many(&[
                                McpePacket::from(radius_update),
                                McpePacket::from(publisher),
                            ])
                            .await?;

                            if !self.sent_spawn_chunk {
                                let chunk = self.build_empty_chunk(
                                    self.start_config.spawn_position.x.div_euclid(16),
                                    self.start_config.spawn_position.z.div_euclid(16),
                                );
                                self.send(McpePacket::from(chunk)).await?;
                                self.sent_spawn_chunk = true;
                            }
                            continue;
                        }
                        if matches!(packet.data, McpePacketData::PacketClientCacheStatus(_))
                            && self.listener_config.handle_client_cache_status
                        {
                            // Ignore and continue
                            continue;
                        }
                        return Ok(packet);
                    }
                }
            }
        }
    }

    pub fn peer_addr(&self) -> Result<SocketAddr, JolyneError> {
        Ok(self.inner.peer_addr())
    }

    pub fn session(&self) -> &ValentineBedrockSession {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut ValentineBedrockSession {
        &mut self.session
    }

    pub fn peer_identity(&self) -> Option<&ValidatedIdentity> {
        self.peer_identity.as_ref()
    }

    async fn enter_ready_to_spawn(&mut self) -> Result<(), JolyneError> {
        if matches!(
            self.handshake_state,
            HandshakeState::ReadyToSpawn
                | HandshakeState::WaitingForLocalPlayerInit
                | HandshakeState::InGame
        ) {
            return Ok(());
        }
        info!("entering ready-to-spawn; building spawn sequence");
        self.handshake_state = HandshakeState::ReadyToSpawn;
        self.send_spawn_sequence().await
    }

    async fn send_spawn_sequence(&mut self) -> Result<(), JolyneError> {
        info!(
            runtime_entity_id = self.start_config.runtime_entity_id,
            "sending spawn sequence (StartGame, registries, radius, initialized, PlayerSpawn)"
        );
        let mut packets = Vec::new();
        let start = self.build_start_game();
        packets.push(McpePacket::from(start));
        if self.listener_config.send_block_palette {
            let update = PacketUpdateBlockProperties { nbt: Vec::new() };
            packets.push(McpePacket::from(update));
        }

        // Minimal registries
        let biomes = self.build_biome_definitions();
        packets.push(McpePacket::from(biomes));

        let items = self.build_item_registry();
        packets.push(McpePacket::from(items));

        let actors = self.build_available_entities();
        packets.push(McpePacket::from(actors));
        let creative = self.build_creative_content();
        packets.push(McpePacket::from(creative));

        // Chunk radius / publisher bootstrap
        let radius_update = PacketChunkRadiusUpdate {
            chunk_radius: self.initial_chunk_radius,
        };
        packets.push(McpePacket::from(radius_update));
        let publisher = PacketNetworkChunkPublisherUpdate {
            coordinates: self.start_config.spawn_position.clone(),
            radius: self.initial_chunk_radius,
            saved_chunks: Vec::new(),
        };
        packets.push(McpePacket::from(publisher));

        // PlayerSpawn status to stop loading screen
        let spawn_status = McpePacket::from(PacketPlayStatus {
            status: PacketPlayStatusStatus::PlayerSpawn,
        });
        packets.push(spawn_status);

        self.send_many(&packets).await?;
        info!("spawn sequence completed; waiting for SetLocalPlayerAsInitialized");
        self.handshake_state = HandshakeState::WaitingForLocalPlayerInit;
        Ok(())
    }
}
