use std::marker::PhantomData;
use std::net::SocketAddr;

use p384::SecretKey;
use tokio_raknet::RaknetStream;
use tracing::instrument;

use crate::error::{JolyneError, ProtocolError};
use crate::protocol::packets::{
    PacketClientToServerHandshake, PacketLogin, PacketPlayStatusStatus, PacketRequestChunkRadius,
    PacketRequestNetworkSettings, PacketResourcePackClientResponse,
    PacketResourcePackClientResponseResponseStatus, PacketSetLocalPlayerAsInitialized,
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
                                 // TODO: Add offline/online mode flags, XBOX token inputs
}

// --- State: Handshake (Initial) ---

impl BedrockStream<Handshake, Client> {
    /// Connects to a Bedrock server and initializes the stream in the `Handshake` state.
    pub async fn connect(addr: SocketAddr) -> Result<Self, JolyneError> {
        let stream = RaknetStream::connect(addr)
            .await
            .map_err(|e| JolyneError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        Ok(Self {
            transport: BedrockTransport::new(stream),
            state: Handshake { config: None },
            _role: PhantomData,
        })
    }

    /// Requests network settings from the server and enables compression.
    ///
    /// Sends `RequestNetworkSettings` and waits for `NetworkSettings`.
    #[instrument(skip(self), level = "debug")]
    pub async fn request_settings(mut self) -> Result<BedrockStream<Login, Client>, JolyneError> {
        // 1. Send RequestNetworkSettings (Raw)
        let req = PacketRequestNetworkSettings {
            client_protocol: crate::protocol::PROTOCOL_VERSION,
        };
        self.transport.send_raw(McpePacket::from(req)).await?;

        // 2. Receive NetworkSettings (Raw or Batch)
        let settings_pkt = self.transport.recv_packet().await?;

        match settings_pkt.data {
            McpePacketData::PacketNetworkSettings(settings) => {
                // Enable compression
                self.transport.set_compression(
                    true,
                    7, // Use default level
                    settings.compression_threshold,
                );

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
}

// --- State: Login ---

impl BedrockStream<Login, Client> {
    /// Sends the `Login` packet with the authentication chain.
    ///
    /// Transitions to `SecurePending` waiting for the server's handshake response.
    #[instrument(skip(self, _config), level = "debug")]
    pub async fn send_login(
        mut self,
        _config: &ClientHandshakeConfig,
    ) -> Result<BedrockStream<SecurePending, Client>, JolyneError> {
        // Generate tokens (simplified for now, assumes offline or provided keys)
        // TODO: Restore `create_client_auth_tokens` once implemented or found.
        let chain = "mock_chain".to_string();
        let client_token = "mock_token".to_string();

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

        Ok(BedrockStream {
            transport: self.transport,
            state: SecurePending {
                config: self.state.config,
            },
            _role: PhantomData,
        })
    }
}

// --- State: SecurePending ---

impl BedrockStream<SecurePending, Client> {
    /// Waits for the server's handshake (Encryption) or login success.
    ///
    /// Handles `ServerToClientHandshake` (encryption) and `PlayStatus`.
    #[instrument(skip(self), level = "debug")]
    pub async fn await_handshake(
        mut self,
    ) -> Result<BedrockStream<ResourcePacks, Client>, JolyneError> {
        // 1. Wait for ServerToClientHandshake (Encryption Request) or PlayStatus
        let next_pkt = self.transport.recv_packet().await?;

        match next_pkt.data {
            McpePacketData::PacketServerToClientHandshake(_hs) => {
                // Encryption Flow
                // STUB: Real impl would decode JWT from `hs.token`.

                // Perform Key Derivation & Enable Encryption
                // self.transport.enable_encryption(...)

                // Send ClientToServerHandshake (Ack)
                let ack = PacketClientToServerHandshake {};
                self.transport.send_batch(&[McpePacket::from(ack)]).await?;

                // Now wait for LoginSuccess
                let status = self.transport.recv_packet().await?;
                if !matches!(status.data, McpePacketData::PacketPlayStatus(_)) {
                    return Err(ProtocolError::UnexpectedHandshake(
                        "Expected PlayStatus after encryption".into(),
                    )
                    .into());
                }
            }
            McpePacketData::PacketPlayStatus(status) => {
                // Encryption skipped?
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
    /// Handles resource pack negotiation.
    pub async fn handle_packs(mut self) -> Result<BedrockStream<StartGame, Client>, JolyneError> {
        // 1. Expect ResourcePacksInfo
        let info_pkt = self.transport.recv_packet().await?;
        if !matches!(info_pkt.data, McpePacketData::PacketResourcePacksInfo(_)) {
            return Err(
                ProtocolError::UnexpectedHandshake("Expected ResourcePacksInfo".into()).into(),
            );
        }

        // 2. Respond (We accept everything for now)
        let resp = PacketResourcePackClientResponse {
            response_status: PacketResourcePackClientResponseResponseStatus::HaveAllPacks,
            resourcepackids: vec![],
        };
        self.transport.send_batch(&[McpePacket::from(resp)]).await?;

        // 3. Expect ResourcePackStack
        let stack_pkt = self.transport.recv_packet().await?;
        if !matches!(stack_pkt.data, McpePacketData::PacketResourcePackStack(_)) {
            // Sometimes server sends more info?
        }

        // 4. Send Completed
        let complete = PacketResourcePackClientResponse {
            response_status: PacketResourcePackClientResponseResponseStatus::Completed,
            resourcepackids: vec![],
        };
        self.transport
            .send_batch(&[McpePacket::from(complete)])
            .await?;

        Ok(BedrockStream {
            transport: self.transport,
            state: StartGame,
            _role: PhantomData,
        })
    }
}

// --- State: StartGame ---

impl BedrockStream<StartGame, Client> {
    pub async fn await_start_game(mut self) -> Result<BedrockStream<Play, Client>, JolyneError> {
        // Expected sequence (minimal):
        // S->C StartGame
        // S->C ItemRegistry
        // C->S RequestChunkRadius
        // S->C ChunkRadiusUpdate + NetworkChunkPublisherUpdate
        // S->C PlayStatus(PlayerSpawn)

        let mut runtime_entity_id: Option<i64> = None;
        let mut sent_chunk_radius = false;
        let mut got_chunk_radius_update = false;
        let mut got_chunk_publisher_update = false;

        loop {
            let pkt = self.transport.recv_packet().await?;
            match pkt.data {
                McpePacketData::PacketStartGame(start) => {
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
                McpePacketData::PacketChunkRadiusUpdate(_) => {
                    got_chunk_radius_update = true;
                }
                McpePacketData::PacketNetworkChunkPublisherUpdate(_) => {
                    got_chunk_publisher_update = true;
                }
                McpePacketData::PacketPlayStatus(status) => {
                    if status.status == PacketPlayStatusStatus::PlayerSpawn {
                        break;
                    }
                }
                _ => {}
            }
        }

        if sent_chunk_radius && (!got_chunk_radius_update || !got_chunk_publisher_update) {
            // Not fatal, but indicates the server didn't complete the expected bootstrap packets.
            // Continue anyway: some servers may skip these.
        }

        // Tell the server we're ready to play.
        if let Some(rid) = runtime_entity_id {
            self.transport
                .send_batch(&[McpePacket::from(PacketSetLocalPlayerAsInitialized {
                    runtime_entity_id: rid,
                })])
                .await?;
        }

        Ok(BedrockStream {
            transport: self.transport,
            state: Play,
            _role: PhantomData,
        })
    }
}

// --- State: Play ---

impl BedrockStream<Play, Client> {
    pub async fn recv_packet(&mut self) -> Result<McpePacket, JolyneError> {
        self.transport.recv_packet().await
    }

    pub async fn send_packet(&mut self, packet: McpePacket) -> Result<(), JolyneError> {
        self.transport.send_batch(&[packet]).await
    }
}
