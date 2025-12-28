//! Raw transport layer for the Bedrock protocol.
//!
//! `BedrockTransport<T>` handles encryption, compression, and batching
//! on top of any transport implementing the `Transport` trait.

use std::collections::VecDeque;
use std::future::poll_fn;
use std::pin::Pin;

use aes::Aes256;
use aes_gcm::{Aes256Gcm, Key};
use bytes::BytesMut;
use ctr::cipher::{KeyIvInit, StreamCipher};
use sha2::{Digest, Sha256};
use tracing::{debug, instrument};

use super::{Transport, TransportMessage};
use crate::batch::{
    decode_batch_no_prefix_raw, decode_batch_raw, encode_batch_multi, encode_batch_raw,
};
use crate::error::{JolyneError, ProtocolError};
use crate::raw::RawPacket;
use crate::valentine::McpePacket;
use valentine::bedrock::context::BedrockSession;

type Aes256Ctr = ctr::Ctr32BE<Aes256>;

const CHECKSUM_LEN: usize = 8;

/// Raw transport layer for the Bedrock protocol.
///
/// Handles:
/// - Framing (via underlying Transport)
/// - Encryption (AES-256-CTR + SHA256 checksum)
/// - Compression (Zlib/Deflate)
/// - Batching
///
/// This struct does NOT handle protocol state (Login, Handshake).
/// It merely reads and writes batches of packets.
///
/// Generic over `T: Transport` to support both RakNet and NetherNet.
pub struct BedrockTransport<T: Transport> {
    inner: T,
    // We keep the session for codec context (shield ID, etc.)
    pub(crate) session: BedrockSession,

    // Packet Buffering (single raw buffer - decoded on demand)
    recv_queue: VecDeque<RawPacket>,
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

impl<T: Transport> BedrockTransport<T> {
    /// Create a new BedrockTransport wrapping the given transport.
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            session: BedrockSession { shield_item_id: 0 },
            recv_queue: VecDeque::new(),
            write_buffer: Vec::new(),
            auto_flush: true,
            encryption_enabled: false,
            send_cipher: None,
            recv_cipher: None,
            key_bytes: None,
            send_counter: 0,
            recv_counter: 0,
            compression_enabled: false,
            compression_level: 7,
            compression_threshold: 0,
            max_decompressed_batch_size: Some(1024 * 1024 * 4),
        }
    }

    /// Enable encryption with the derived keys.
    #[instrument(skip_all, level = "debug", fields(peer_addr = %self.peer_addr()))]
    pub fn enable_encryption(&mut self, key: Key<Aes256Gcm>, iv: [u8; 12]) {
        let key_bytes = key.to_vec();

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
        self.send_batch_with_reliability(packets, true).await
    }

    /// Sends a list of packets as a single batch with specified reliability.
    #[instrument(skip_all, level = "trace", fields(peer_addr = %self.peer_addr(), reliable = reliable))]
    pub async fn send_batch_with_reliability(
        &mut self,
        packets: &[McpePacket],
        reliable: bool,
    ) -> Result<(), JolyneError> {
        if self.encryption_enabled && !reliable {
            return Err(JolyneError::Protocol(ProtocolError::UnexpectedHandshake(
                "Cannot use unreliable networking when encryption is active".into(),
            )));
        }

        if packets.is_empty() {
            return Ok(());
        }

        // 1. Encode Batch (Handles Compression)
        // Use T::USES_BATCH_PREFIX to conditionally add 0xFE prefix
        let batch_buffer = encode_batch_multi(
            packets,
            self.compression_enabled,
            self.compression_level,
            self.compression_threshold,
            T::USES_BATCH_PREFIX,
        )?;

        // 2. Encrypt & Send
        let msg = if self.encryption_enabled {
            let mut bm = BytesMut::from(batch_buffer.as_ref());
            self.encrypt_outgoing(&mut bm)?;
            TransportMessage::reliable(bm.freeze())
        } else if reliable {
            TransportMessage::reliable(batch_buffer)
        } else {
            TransportMessage::unreliable(batch_buffer)
        };

        // Send using poll_fn to convert poll-based API to async
        poll_fn(|cx| Pin::new(&mut self.inner).poll_send(cx, msg.clone()))
            .await
            .map_err(|e| JolyneError::Transport(e.to_string()))?;

        Ok(())
    }

    /// Raw send for handshake packets that cannot be batched.
    ///
    /// The framing depends on the transport type:
    /// - **RakNet**: Uses game frame format `[0xFE][Length][Header][Body]`
    /// - **NetherNet**: Uses inner format `[Length][Header][Body]` (no 0xFE)
    #[instrument(skip_all, level = "trace", fields(peer_addr = %self.peer_addr()))]
    pub async fn send_raw(&mut self, packet: McpePacket) -> Result<(), JolyneError> {
        let mut buf = BytesMut::new();

        if T::USES_BATCH_PREFIX {
            // RakNet: Use full game frame with 0xFE prefix
            use valentine::bedrock::codec::BedrockCodec;
            packet.encode(&mut buf)?;
        } else {
            // NetherNet: Use inner format without 0xFE prefix
            packet.data.encode_inner(
                &mut buf,
                packet.header.from_subclient,
                packet.header.to_subclient,
            )?;
        }

        if self.encryption_enabled {
            self.encrypt_outgoing(&mut buf)?;
        }

        let msg = TransportMessage::reliable(buf.freeze());
        poll_fn(|cx| Pin::new(&mut self.inner).poll_send(cx, msg.clone()))
            .await
            .map_err(|e| JolyneError::Transport(e.to_string()))?;
        Ok(())
    }

    /// Returns the next single packet, decoding from raw buffer on demand.
    pub async fn recv_packet(&mut self) -> Result<McpePacket, JolyneError> {
        loop {
            if let Some(raw) = self.recv_queue.pop_front() {
                // Decode on demand
                return raw.decode(&self.session);
            }
            let packets = self.recv_batch_raw().await?;
            if packets.is_empty() {
                continue;
            }
            self.recv_queue.extend(packets);
        }
    }

    /// Reads a batch of packets from the network, returning fully decoded packets.
    ///
    /// Note: This decodes all packets in the batch immediately. For proxy use cases
    /// where you want to inspect IDs without full decode, use `recv_batch_raw()` instead.
    #[instrument(skip_all, level = "trace", fields(peer_addr = %self.peer_addr()))]
    pub async fn recv_batch(&mut self) -> Result<Vec<McpePacket>, JolyneError> {
        let raw_packets = self.recv_batch_raw().await?;
        raw_packets
            .into_iter()
            .map(|raw| raw.decode(&self.session))
            .collect()
    }

    /// Returns the next packet as raw bytes (header parsed, body undecoded).
    ///
    /// Useful for proxies that need to inspect packet IDs without full decode.
    /// Call [`RawPacket::decode`] if you later need the full packet.
    pub async fn recv_packet_raw(&mut self) -> Result<RawPacket, JolyneError> {
        loop {
            if let Some(pkt) = self.recv_queue.pop_front() {
                return Ok(pkt);
            }
            let packets = self.recv_batch_raw().await?;
            if packets.is_empty() {
                continue;
            }
            self.recv_queue.extend(packets);
        }
    }

    /// Returns all packets from the next network batch as raw bytes.
    #[instrument(skip_all, level = "trace", fields(peer_addr = %self.peer_addr()))]
    pub async fn recv_batch_raw(&mut self) -> Result<Vec<RawPacket>, JolyneError> {
        // 1. Read Raw Frame
        let recv_result = poll_fn(|cx| Pin::new(&mut self.inner).poll_recv(cx)).await;
        let mut packet_bytes = recv_result
            .ok_or(JolyneError::ConnectionClosed)?
            .map_err(|e| JolyneError::Transport(e.to_string()))?;

        // 2. Decrypt
        if self.encryption_enabled {
            let mut bm = BytesMut::from(packet_bytes.as_ref());
            self.decrypt_incoming(&mut bm)?;
            packet_bytes = bm.freeze();
        }

        if packet_bytes.is_empty() {
            return Ok(vec![]);
        }

        // 3. Decode as raw packets
        let mut buf = packet_bytes;

        if T::USES_BATCH_PREFIX {
            if buf[0] == 0xFE {
                decode_batch_raw(
                    &mut buf,
                    self.compression_enabled,
                    self.max_decompressed_batch_size,
                )
            } else {
                // Non-batch packet (before compression): parse as single raw packet
                use crate::raw::decode_packet_raw;
                Ok(vec![decode_packet_raw(&mut buf)?])
            }
        } else {
            if self.compression_enabled {
                decode_batch_no_prefix_raw(&mut buf, self.max_decompressed_batch_size)
            } else {
                // Before NetworkSettings: parse as single raw packet
                use crate::raw::decode_packet_raw;
                Ok(vec![decode_packet_raw(&mut buf)?])
            }
        }
    }

    /// Sends raw packets as a batch with specified reliability.
    ///
    /// Useful for proxies forwarding packets without decode/re-encode overhead.
    #[instrument(skip_all, level = "trace", fields(peer_addr = %self.peer_addr(), reliable = reliable))]
    pub async fn send_batch_raw(
        &mut self,
        packets: &[RawPacket],
        reliable: bool,
    ) -> Result<(), JolyneError> {
        if self.encryption_enabled && !reliable {
            return Err(JolyneError::Protocol(ProtocolError::UnexpectedHandshake(
                "Cannot use unreliable networking when encryption is active".into(),
            )));
        }

        if packets.is_empty() {
            return Ok(());
        }

        let batch_buffer = encode_batch_raw(
            packets,
            self.compression_enabled,
            self.compression_level,
            self.compression_threshold,
            T::USES_BATCH_PREFIX,
        )?;

        let msg = if self.encryption_enabled {
            let mut bm = BytesMut::from(batch_buffer.as_ref());
            self.encrypt_outgoing(&mut bm)?;
            TransportMessage::reliable(bm.freeze())
        } else if reliable {
            TransportMessage::reliable(batch_buffer)
        } else {
            TransportMessage::unreliable(batch_buffer)
        };

        poll_fn(|cx| Pin::new(&mut self.inner).poll_send(cx, msg.clone()))
            .await
            .map_err(|e| JolyneError::Transport(e.to_string()))?;

        Ok(())
    }

    /// Sends a single raw packet (convenience wrapper around `send_batch_raw`).
    pub async fn send_packet_raw(&mut self, packet: RawPacket) -> Result<(), JolyneError> {
        self.send_batch_raw(&[packet], true).await
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

        let counter = self.send_counter;
        self.send_counter = self.send_counter.wrapping_add(1);

        let counter_bytes = counter.to_le_bytes();
        let mut digest = Sha256::new();
        digest.update(counter_bytes);
        digest.update(&buf[1..]);
        digest.update(key_bytes);
        let checksum = digest.finalize();

        buf.extend_from_slice(&checksum[..CHECKSUM_LEN]);
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

        buf.truncate(checksum_start);
        Ok(())
    }

    /// Returns the peer address.
    pub fn peer_addr(&self) -> std::net::SocketAddr {
        self.inner.peer_addr()
    }
}
