use std::collections::VecDeque;

use aes::Aes256;
use aes_gcm::{Aes256Gcm, Key};
use bytes::BytesMut;
use ctr::cipher::{KeyIvInit, StreamCipher};
use sha2::{Digest, Sha256};
use tokio_raknet::protocol::reliability::Reliability;
use tokio_raknet::transport::{Message, RaknetStream};
use tracing::{debug, instrument};

use crate::batch::{decode_batch, encode_batch_multi};
use crate::error::{JolyneError, ProtocolError};
use crate::protocol::McpePacket;
use valentine::bedrock::context::BedrockSession;

type Aes256Ctr = ctr::Ctr32BE<Aes256>;

const CHECKSUM_LEN: usize = 8;

/// Raw transport layer for the Bedrock protocol.
///
/// Handles:
/// - Framing (RakNet)
/// - Encryption (AES-256-CTR + SHA256 checksum)
/// - Compression (Zlib/Deflate)
/// - Batching
///
/// This struct does NOT handle protocol state (Login, Handshake).
/// It merely reads and writes batches of packets.
pub struct BedrockTransport {
    inner: RaknetStream,
    // We keep the session for codec context (shield ID, etc.)
    pub(crate) session: BedrockSession,

    // Packet Buffering
    recv_queue: VecDeque<McpePacket>,
    write_buffer: Vec<McpePacket>,
    auto_flush: bool,

    // Encryption State (Bedrock: AES-256-CTR + SHA256 checksum)
    pub(crate) encryption_enabled: bool,
    send_cipher: Option<Aes256Ctr>,
    recv_cipher: Option<Aes256Ctr>,
    key_bytes: Option<Vec<u8>>,
    send_counter: u64,
    recv_counter: u64,

    // Compression State
    pub(crate) compression_enabled: bool,
    pub(crate) compression_level: u32,
    pub(crate) compression_threshold: u16,
    max_decompressed_batch_size: Option<usize>,
}

impl BedrockTransport {
    pub fn new(inner: RaknetStream) -> Self {
        Self {
            inner,
            session: BedrockSession { shield_item_id: 0 },
            recv_queue: VecDeque::new(),
            write_buffer: Vec::new(),
            auto_flush: true, // Default to immediate send (like TCP/Proxy)
            encryption_enabled: false,
            send_cipher: None,
            recv_cipher: None,
            key_bytes: None,
            send_counter: 0,
            recv_counter: 0,
            compression_enabled: false,
            compression_level: 7,     // Default Zlib level
            compression_threshold: 0, // Compress everything by default if enabled
            max_decompressed_batch_size: Some(1024 * 1024 * 4), // 4MB default limit
        }
    }

    /// Enable encryption with the derived keys.
    /// This should be called immediately after the handshake key derivation.
    #[instrument(skip_all, level = "debug", fields(peer_addr = %self.peer_addr()))]
    pub fn enable_encryption(&mut self, key: Key<Aes256Gcm>, iv: [u8; 12]) {
        let key_bytes = key.to_vec();

        // Bedrock uses AES-256-CTR with a 16-byte IV, where the last 4 bytes are a BE counter.
        // Implementations typically initialise it to 2.
        let mut iv16 = [0u8; 16];
        iv16[..12].copy_from_slice(&iv);
        iv16[12..].copy_from_slice(&[0, 0, 0, 2]);

        let base_cipher = Aes256Ctr::new_from_slices(&key_bytes, &iv16)
            .expect("AES-256-CTR key/iv lengths are fixed (32/16 bytes)");
        self.send_cipher = Some(base_cipher.clone());
        self.recv_cipher = Some(base_cipher);
        self.key_bytes = Some(key_bytes);
        self.send_counter = 0;
        self.recv_counter = 0;
        self.encryption_enabled = true;
        debug!("encryption enabled");
    }

    /// Sets compression parameters.
    pub fn set_compression(&mut self, enabled: bool, level: u32, threshold: u16) {
        self.compression_enabled = enabled;
        self.compression_level = level;
        self.compression_threshold = threshold;
    }

    /// Configures the flushing strategy.
    ///
    /// - `true` (Default): `send()` sends packets immediately (low latency, high overhead).
    /// - `false`: `send()` queues packets. You MUST call `flush()` to send them (high throughput).
    pub fn set_auto_flush(&mut self, auto: bool) {
        self.auto_flush = auto;
    }

    /// Sends a packet. Behavior depends on `set_auto_flush`.
    pub async fn send(&mut self, packet: McpePacket) -> Result<(), JolyneError> {
        if self.auto_flush {
            self.send_batch(&[packet]).await
        } else {
            self.write_buffer.push(packet);
            Ok(())
        }
    }

    /// Flushes all buffered packets as a single batch (ReliableOrdered).
    /// Does nothing if the buffer is empty.
    pub async fn flush(&mut self) -> Result<(), JolyneError> {
        if self.write_buffer.is_empty() {
            return Ok(());
        }
        let packets: Vec<McpePacket> = self.write_buffer.drain(..).collect();
        self.send_batch(&packets).await
    }

    /// Sends a list of packets as a single batch using `ReliableOrdered` reliability.
    #[instrument(skip_all, level = "trace", fields(peer_addr = %self.peer_addr()))]
    pub async fn send_batch(&mut self, packets: &[McpePacket]) -> Result<(), JolyneError> {
        self.send_batch_with_reliability(packets, Reliability::ReliableOrdered)
            .await
    }

    /// Sends a list of packets as a single batch with specified reliability.
    ///
    /// This bypasses the internal `write_buffer` and sends immediately.
    /// Useful for streaming data (e.g. video/maps) that should use `Unreliable` or `ReliableSequenced`.
    ///
    /// # Safety
    ///
    /// If encryption is enabled, you **MUST** use `Reliability::ReliableOrdered`.
    /// Bedrock uses a stream cipher (AES-CTR) which requires strict ordering and no gaps.
    /// Using any other reliability will desynchronize the cipher and break the connection.
    /// This method returns an error if you attempt to violate this rule.
    #[instrument(skip_all, level = "trace", fields(peer_addr = %self.peer_addr(), reliability = ?reliability))]
    pub async fn send_batch_with_reliability(
        &mut self,
        packets: &[McpePacket],
        reliability: Reliability,
    ) -> Result<(), JolyneError> {
        if self.encryption_enabled && reliability != Reliability::ReliableOrdered {
            return Err(JolyneError::Protocol(ProtocolError::UnexpectedHandshake(
                "Cannot use unreliable/unordered networking when encryption is active".into(),
            )));
        }

        if packets.is_empty() {
            return Ok(());
        }

        // 1. Encode Batch (Handles Compression)
        let batch_buffer = encode_batch_multi(
            packets,
            self.compression_enabled,
            self.compression_level,
            self.compression_threshold,
        )?;

        // 2. Encrypt & Send
        if self.encryption_enabled {
            let mut bm = BytesMut::from(batch_buffer.as_ref());
            self.encrypt_outgoing(&mut bm)?;
            self.inner
                .send(Message::new(bm.freeze()).reliability(reliability))
                .await?;
        } else {
            self.inner
                .send(Message::new(batch_buffer).reliability(reliability))
                .await?;
        }

        Ok(())
    }

    /// Raw send for handshake packets that cannot be batched (e.g., NetworkSettings).
    #[instrument(skip_all, level = "trace", fields(peer_addr = %self.peer_addr()))]
    pub async fn send_raw(&mut self, packet: McpePacket) -> Result<(), JolyneError> {
        let mut buf = BytesMut::new();
        // Manual framing: [0xFE] [Len] [Header] [Body]
        // McpePacket::encode writes [Len] [Header] [Body].
        // send_raw is mostly for uncompressed/unencrypted initial handshake packets.
        // Let's rely on the crate::protocol::packets::McpePacket implementation.

        // However, raw packets in Bedrock (like RequestNetworkSettings) are still framed
        // but just NOT batched/compressed.
        use valentine::bedrock::codec::BedrockCodec;
        packet.encode(&mut buf)?;

        if self.encryption_enabled {
            self.encrypt_outgoing(&mut buf)?;
        }

        self.inner
            .send(Message::new(buf.freeze()).reliability(Reliability::ReliableOrdered))
            .await?;
        Ok(())
    }

    /// Returns the next single packet, reading a new batch if necessary.
    pub async fn recv_packet(&mut self) -> Result<McpePacket, JolyneError> {
        loop {
            if let Some(pkt) = self.recv_queue.pop_front() {
                return Ok(pkt);
            }
            // Queue is empty, fetch more
            let packets = self.recv_batch().await?;
            if packets.is_empty() {
                // Keep trying if we got an empty batch (e.g. padding?)
                continue;
            }
            self.recv_queue.extend(packets);
        }
    }

    /// Reads a batch of packets from the network.
    ///
    /// This method:
    /// 1. Reads a raw frame from RakNet.
    /// 2. Decrypts it (if encryption is active).
    /// 3. Decodes the batch (decompresses and splits).
    ///
    /// If the packet is NOT a batch (does not start with 0xFE), it is treated as a raw packet.
    #[instrument(skip_all, level = "trace", fields(peer_addr = %self.peer_addr()))]
    pub async fn recv_batch(&mut self) -> Result<Vec<McpePacket>, JolyneError> {
        // 1. Read Raw Frame
        let mut packet_bytes = self
            .inner
            .recv()
            .await
            .ok_or(JolyneError::ConnectionClosed)??;

        // 2. Decrypt
        if self.encryption_enabled {
            let mut bm = BytesMut::from(packet_bytes.as_ref());
            self.decrypt_incoming(&mut bm)?;
            packet_bytes = bm.freeze();
        }

        if packet_bytes.is_empty() {
            return Ok(vec![]);
        }

        // 3. Detect & Decode
        let mut buf = packet_bytes;

        // 0xFE is the Batch Packet ID
        if buf[0] == 0xFE {
            let packets = decode_batch(
                &mut buf,
                &self.session,
                self.compression_enabled,
                self.max_decompressed_batch_size,
            )?;
            Ok(packets)
        } else {
            // Treat as Raw Packet (e.g., RequestNetworkSettings)
            use valentine::bedrock::codec::BedrockCodec;
            // McpePacket::decode expects the buffer to be advanced past the ID?
            // No, usually decode reads the ID to know which variant to create.
            // BUT, standard BedrockCodec implementation usually reads the ID.
            // Let's assume McpePacket::decode handles the full packet including ID.
            let packet = McpePacket::decode(&mut buf, (&self.session).into())?;
            Ok(vec![packet])
        }
    }

    // --- Crypto Helpers ---

    fn encrypt_outgoing(&mut self, buf: &mut BytesMut) -> Result<(), JolyneError> {
        if buf.is_empty() {
            return Ok(());
        }

        let cipher = self
            .send_cipher
            .as_mut()
            .expect("encryption_enabled implies send_cipher is initialised");
        let key_bytes = self
            .key_bytes
            .as_deref()
            .expect("encryption_enabled implies key_bytes is initialised");

        // Checksum = SHA256(counter_le || payload || key)[0..8]
        let counter = self.send_counter;
        self.send_counter = self.send_counter.wrapping_add(1);

        let counter_bytes = counter.to_le_bytes();
        let mut digest = Sha256::new();
        digest.update(counter_bytes);
        digest.update(&buf[1..]);
        digest.update(key_bytes);
        let checksum = digest.finalize();

        buf.extend_from_slice(&checksum[..CHECKSUM_LEN]);

        // Encrypt everything after the first byte (packet header), including the checksum.
        cipher.apply_keystream(&mut buf[1..]);

        Ok(())
    }

    fn decrypt_incoming(&mut self, buf: &mut BytesMut) -> Result<(), JolyneError> {
        if buf.is_empty() {
            return Ok(());
        }

        let cipher = self
            .recv_cipher
            .as_mut()
            .expect("encryption_enabled implies recv_cipher is initialised");
        let key_bytes = self
            .key_bytes
            .as_deref()
            .expect("encryption_enabled implies key_bytes is initialised");

        // Decrypt everything after the first byte (packet header).
        cipher.apply_keystream(&mut buf[1..]);

        if buf.len() < 1 + CHECKSUM_LEN {
            return Err(ProtocolError::UnexpectedHandshake(format!(
                "encrypted packet must be at least {} bytes long, got {}",
                1 + CHECKSUM_LEN,
                buf.len()
            ))
            .into());
        }

        let checksum_start = buf.len() - CHECKSUM_LEN;
        let their_checksum = &buf[checksum_start..];

        // Checksum = SHA256(counter_le || payload || key)[0..8]
        let counter = self.recv_counter;
        self.recv_counter = self.recv_counter.wrapping_add(1);

        let counter_bytes = counter.to_le_bytes();
        let mut digest = Sha256::new();
        digest.update(counter_bytes);
        digest.update(&buf[1..checksum_start]);
        digest.update(key_bytes);
        let our_checksum_full = digest.finalize();
        let our_checksum = &our_checksum_full[..CHECKSUM_LEN];

        if their_checksum != our_checksum {
            return Err(ProtocolError::UnexpectedHandshake(format!(
                "invalid checksum of packet {}: expected {:02x?}, got {:02x?}",
                counter, our_checksum, their_checksum
            ))
            .into());
        }

        // Strip checksum suffix before decoding.
        buf.truncate(checksum_start);

        Ok(())
    }

    pub fn peer_addr(&self) -> std::net::SocketAddr {
        self.inner.peer_addr()
    }
}
