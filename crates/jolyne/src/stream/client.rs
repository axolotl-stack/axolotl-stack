use std::marker::PhantomData;
use std::net::SocketAddr;

use aes_gcm::Aes256Gcm;
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use jsonwebtoken::decode_header;
use p384::ecdsa::{Signature, VerifyingKey, signature::Verifier};
use p384::{PublicKey, SecretKey, pkcs8::DecodePublicKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio_raknet::RaknetStream;
use tracing::{instrument, warn};
use uuid::Uuid;

use crate::error::{JolyneError, ProtocolError};
use crate::protocol::packets::{
    PacketClientToServerHandshake, PacketLogin, PacketPlayStatusStatus, PacketRequestChunkRadius,
    PacketRequestNetworkSettings, PacketResourcePackClientResponse,
    PacketResourcePackClientResponseResponseStatus, PacketServerboundLoadingScreen,
    PacketSetLocalPlayerAsInitialized,
};
use crate::protocol::{McpePacket, McpePacketData};
use crate::stream::{
    BedrockStream, Client, Handshake, Login, Play, ResourcePacks, SecurePending, StartGame,
    transport::BedrockTransport,
};

// --- Config ---

#[derive(Debug, Clone)]
pub struct ClientHandshakeConfig {
    pub server_addr: SocketAddr,
    pub identity_key: SecretKey, // Client's private key
    pub display_name: String,
    pub uuid: Uuid,
}

impl ClientHandshakeConfig {
    /// Generates a configuration with a random identity key and UUID.
    /// Useful for testing or simple bots.
    pub fn random(server_addr: SocketAddr, display_name: impl Into<String>) -> Self {
        Self {
            server_addr,
            identity_key: SecretKey::random(&mut rand::thread_rng()),
            display_name: display_name.into(),
            uuid: Uuid::new_v4(),
        }
    }
}

// --- State: Handshake (Initial) ---

impl BedrockStream<Handshake, Client> {
    /// Connects to a Bedrock server and initializes the stream in the `Handshake` state.
    #[instrument(skip_all, level = "trace", fields(addr = %addr))]
    pub async fn connect(addr: SocketAddr) -> Result<Self, JolyneError> {
        let stream = RaknetStream::connect(addr).await?;
        tracing::debug!("Connected to server");

        Ok(Self {
            transport: BedrockTransport::new(stream),
            state: Handshake { config: None },
            _role: PhantomData,
        })
    }

    /// Requests network settings from the server and enables compression.
    #[instrument(skip_all, level = "trace")]
    pub async fn request_settings(mut self) -> Result<BedrockStream<Login, Client>, JolyneError> {
        let req = PacketRequestNetworkSettings {
            client_protocol: crate::protocol::PROTOCOL_VERSION,
        };
        self.transport.send_raw(McpePacket::from(req)).await?;

        let settings_pkt = self.transport.recv_packet().await?;

        match settings_pkt.data {
            McpePacketData::PacketNetworkSettings(settings) => {
                self.transport
                    .set_compression(true, 7, settings.compression_threshold);

                tracing::debug!("Network settings received, enabled compression");

                Ok(BedrockStream {
                    transport: self.transport,
                    state: Login {
                        config: self.state.config,
                    },
                    _role: PhantomData,
                })
            }
            _ => Err(ProtocolError::UnexpectedHandshake("Expected NetworkSettings".into()).into()),
        }
    }

    /// Helper: Orchestrates the entire login sequence.
    pub async fn join(
        self,
        config: ClientHandshakeConfig,
    ) -> Result<BedrockStream<Play, Client>, JolyneError> {
        let key = config.identity_key.clone();
        
        // 1. Settings
        let login = self.request_settings().await?;
        
        // 2. Login
        let secure = login.send_login(&config).await?;
        
        // 3. Encryption
        let packs = secure.await_handshake(&key).await?;
        
        // 4. Resource Packs
        let start = packs.handle_packs().await?;
        
        // 5. Start Game
        start.await_start_game().await
    }
}

// --- State: Login ---

impl BedrockStream<Login, Client> {
    #[instrument(skip_all, level = "trace", fields(uuid = %config.uuid, display_name = %config.display_name))]
    pub async fn send_login(
        mut self,
        config: &ClientHandshakeConfig,
    ) -> Result<BedrockStream<SecurePending, Client>, JolyneError> {
        // Generate JWT Chain
        let (chain, client_token) = crate::auth::client::generate_self_signed_chain(
            &config.identity_key,
            &config.display_name,
            config.uuid,
        )?;

        let login_pkt = PacketLogin {
            protocol_version: crate::protocol::PROTOCOL_VERSION,
            tokens: crate::protocol::types::login::LoginTokens {
                identity: chain,
                client: client_token,
            },
        };
        self.transport
            .send_batch(&[McpePacket::from(login_pkt)])
            .await?;

        tracing::debug!("Login packet sent");

        Ok(BedrockStream {
            transport: self.transport,
            state: SecurePending {
                config: None, // Client doesn't store config in state for now
            },
            _role: PhantomData,
        })
    }
}

// --- State: SecurePending ---

#[derive(Debug, Deserialize)]
struct ServerHandshakeClaims {
    salt: String,
}

impl BedrockStream<SecurePending, Client> {
    #[instrument(skip_all, level = "trace")]
    pub async fn await_handshake(
        mut self,
        client_identity_key: &SecretKey,
    ) -> Result<BedrockStream<ResourcePacks, Client>, JolyneError> {
        let next_pkt = self.transport.recv_packet().await?;

        match next_pkt.data {
            McpePacketData::PacketServerToClientHandshake(hs) => {
                // 1. Decode Header to find Server Public Key (x5u)
                let header = decode_header(&hs.token).map_err(|e| {
                    ProtocolError::UnexpectedHandshake(format!("Invalid JWT Header: {}", e))
                })?;

                let x5u = header.x5u.clone().ok_or_else(|| {
                    ProtocolError::UnexpectedHandshake(
                        "Missing x5u (Server Public Key) in handshake token".into(),
                    )
                })?;

                let server_der = STANDARD.decode(&x5u).map_err(|e| {
                    ProtocolError::UnexpectedHandshake(format!("Invalid base64 key: {}", e))
                })?;

                let server_pub = PublicKey::from_public_key_der(&server_der).map_err(|e| {
                    ProtocolError::UnexpectedHandshake(format!("Invalid server public key: {}", e))
                })?;

                // 2. Verify Token (Manually using p384, as jsonwebtoken fails with these keys)
                let parts: Vec<&str> = hs.token.split('.').collect();
                if parts.len() != 3 {
                    return Err(
                        ProtocolError::UnexpectedHandshake("Invalid JWT format".into()).into(),
                    );
                }

                let signed_part = format!("{}.{}", parts[0], parts[1]);
                let signature_bytes = URL_SAFE_NO_PAD.decode(parts[2]).map_err(|e| {
                    ProtocolError::UnexpectedHandshake(format!("Invalid signature base64: {}", e))
                })?;

                let signature = Signature::try_from(signature_bytes.as_slice()).map_err(|e| {
                    ProtocolError::UnexpectedHandshake(format!("Invalid signature length: {}", e))
                })?;

                let verifying_key = VerifyingKey::from(&server_pub);

                if let Err(e) = verifying_key.verify(signed_part.as_bytes(), &signature) {
                    tracing::error!("Handshake Signature Verification Failed: {}", e);
                    return Err(ProtocolError::UnexpectedHandshake(format!(
                        "Invalid handshake token signature: {}",
                        e
                    ))
                    .into());
                }

                // Decode Payload
                let payload_json = URL_SAFE_NO_PAD.decode(parts[1]).map_err(|e| {
                    ProtocolError::UnexpectedHandshake(format!("Invalid payload base64: {}", e))
                })?;

                let token_data: ServerHandshakeClaims = serde_json::from_slice(&payload_json)
                    .map_err(|e| {
                        ProtocolError::UnexpectedHandshake(format!("Invalid payload JSON: {}", e))
                    })?;

                let salt = STANDARD_NO_PAD.decode(&token_data.salt).map_err(|e| {
                    ProtocolError::UnexpectedHandshake(format!("Invalid salt base64: {}", e))
                })?;

                // 3. ECDH Shared Secret
                let shared_secret = p384::ecdh::diffie_hellman(
                    client_identity_key.to_nonzero_scalar(),
                    server_pub.as_affine(),
                );
                let shared_bytes = shared_secret.raw_secret_bytes();

                // 4. Derive Key & IV
                let mut h = Sha256::new();
                h.update(&salt);
                h.update(shared_bytes);
                let key_bytes = h.finalize();

                let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&key_bytes);
                let mut iv = [0u8; 12];
                iv.copy_from_slice(&key_bytes[0..12]);

                // 5. Send ClientToServerHandshake (Ack)
                // Note: This must be sent BEFORE enabling encryption?
                // Bedrock: Server sends Handshake (Unencrypted) -> Client sends Handshake (Unencrypted?? or Encrypted?)
                // Usually Client enables encryption immediately after sending the packet, OR the packet itself is encrypted?
                // Standard: Server sends Handshake. Client computes key. Client sends Handshake (Encrypted? No, usually unencrypted then switches).
                // Let's check `server.rs`.
                // Server: Sends Handshake. Enables Encryption. Waits for Handshake.
                // So Server expects the Client's Ack to be ENCRYPTED.

                // Client side:
                // 1. Recv Handshake (Unencrypted).
                // 2. Compute Key.
                // 3. Enable Encryption.
                // 4. Send Handshake (Encrypted).

                // Let's verify `server.rs` flow:
                // 3. Send ServerToClientHandshake
                // 4. Enable Encryption locally
                // 5. Wait for ClientToServerHandshake

                // Yes, Server enables encryption right after sending. So it expects the NEXT packet (Ack) to be encrypted.
                // So Client must enable encryption BEFORE sending Ack.

                self.transport.enable_encryption(*key, iv);

                let ack = PacketClientToServerHandshake {};
                self.transport.send_batch(&[McpePacket::from(ack)]).await?;

                // 6. Wait for PlayStatus::LoginSuccess (Encrypted)
                let status = self.transport.recv_packet().await?;
                if !matches!(status.data, McpePacketData::PacketPlayStatus(_)) {
                    // Could be ResourcePacksInfo if server skips PlayStatus?
                    // But usually LoginSuccess is sent.
                    // server.rs sends LoginSuccess.
                    warn!("Expected PlayStatus, got {:?}", status.header.id);
                }

                if let McpePacketData::PacketPlayStatus(status) = status.data {
                    use crate::protocol::packets::PacketPlayStatusStatus;
                    if status.status != PacketPlayStatusStatus::LoginSuccess {
                        return Err(ProtocolError::UnexpectedHandshake(format!(
                            "Login failed: {:?}",
                            status.status
                        ))
                        .into());
                    }
                } else {
                    return Err(ProtocolError::UnexpectedHandshake(
                        "Expected PlayStatus after encryption".into(),
                    )
                    .into());
                }

                tracing::debug!("Handshake complete, encryption active");
            }
            McpePacketData::PacketPlayStatus(status) => {
                // Encryption skipped by server?
                use crate::protocol::packets::PacketPlayStatusStatus;
                if status.status != PacketPlayStatusStatus::LoginSuccess {
                    return Err(ProtocolError::UnexpectedHandshake(format!(
                        "Login failed: {:?}",
                        status.status
                    ))
                    .into());
                }
            }
            _ => {
                return Err(ProtocolError::UnexpectedHandshake(
                    "Expected ServerToClientHandshake or LoginSuccess".into(),
                )
                .into());
            }
        }

        Ok(BedrockStream {
            transport: self.transport,
            state: ResourcePacks,
            _role: PhantomData,
        })
    }
}

// --- State: ResourcePacks ---

impl BedrockStream<ResourcePacks, Client> {
    #[instrument(skip_all, level = "trace")]
    pub async fn handle_packs(mut self) -> Result<BedrockStream<StartGame, Client>, JolyneError> {
        let info_pkt = self.transport.recv_packet().await?;
        if !matches!(info_pkt.data, McpePacketData::PacketResourcePacksInfo(_)) {
            return Err(
                ProtocolError::UnexpectedHandshake("Expected ResourcePacksInfo".into()).into(),
            );
        }

        tracing::debug!("Received ResourcePacksInfo");

        let resp = PacketResourcePackClientResponse {
            response_status: PacketResourcePackClientResponseResponseStatus::HaveAllPacks,
            resourcepackids: vec![],
        };
        self.transport.send_batch(&[McpePacket::from(resp)]).await?;

        let _stack_pkt = self.transport.recv_packet().await?;

        let complete = PacketResourcePackClientResponse {
            response_status: PacketResourcePackClientResponseResponseStatus::Completed,
            resourcepackids: vec![],
        };
        self.transport
            .send_batch(&[McpePacket::from(complete)])
            .await?;

        tracing::debug!("Resource packs negotiated");

        Ok(BedrockStream {
            transport: self.transport,
            state: StartGame,
            _role: PhantomData,
        })
    }
}

// --- State: StartGame ---

impl BedrockStream<StartGame, Client> {
    #[instrument(skip_all, level = "trace")]
    pub async fn await_start_game(mut self) -> Result<BedrockStream<Play, Client>, JolyneError> {
        let mut runtime_entity_id: Option<i64> = None;
        let mut sent_chunk_radius = false;

        tracing::debug!("Waiting for StartGame sequence...");

        // 1. Receive StartGame -> Request Radius -> Receive Spawn
        loop {
            let pkt = self.transport.recv_packet().await?;
            match pkt.data {
                McpePacketData::PacketStartGame(start) => {
                    tracing::debug!(runtime_id = %start.runtime_entity_id, "StartGame received");
                    runtime_entity_id = Some(start.runtime_entity_id);
                }
                McpePacketData::PacketItemRegistry(_) => {
                    if !sent_chunk_radius {
                        let req = PacketRequestChunkRadius {
                            chunk_radius: 4,
                            max_radius: 32,
                        };
                        self.transport.send_batch(&[McpePacket::from(req)]).await?;
                        sent_chunk_radius = true;
                    }
                }
                McpePacketData::PacketPlayStatus(status) => {
                    if status.status == PacketPlayStatusStatus::PlayerSpawn {
                        tracing::debug!("PlayerSpawn received");
                        break;
                    }
                }
                _ => {}
            }
        }

        // 2. Send Loading Screen (Start & End)
        self.transport
            .send_batch(&[
                McpePacket::from(PacketServerboundLoadingScreen {
                    type_: 1,
                    loading_screen_id: None,
                }),
                McpePacket::from(PacketServerboundLoadingScreen {
                    type_: 2,
                    loading_screen_id: None,
                }),
            ])
            .await?;

        // 3. Send Initialized
        if let Some(rid) = runtime_entity_id {
            self.transport
                .send_batch(&[McpePacket::from(PacketSetLocalPlayerAsInitialized {
                    runtime_entity_id: rid,
                })])
                .await?;
        }

        tracing::debug!("Game initialization complete, entering Play state");

        Ok(BedrockStream {
            transport: self.transport,
            state: Play,
            _role: PhantomData,
        })
    }
}

// --- State: Play ---

impl BedrockStream<Play, Client> {
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
