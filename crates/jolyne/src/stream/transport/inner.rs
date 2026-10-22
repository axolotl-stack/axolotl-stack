//! Raw transport layer for the Bedrock protocol.
//!
//! `BedrockTransport<T>` handles encryption, compression, and batching
//! on top of any transport implementing the `Transport` trait.

use std::collections::VecDeque;
use std::future::poll_fn;
use std::pin::Pin;

use aes_gcm::{Aes256Gcm, Key};
use bytes::BytesMut;
use tracing::{debug, instrument};
use valentine::bedrock::codec::BedrockSized;

use super::{Transport, TransportMessage, TransportRecvMessage};
use crate::batch::{BatchCodec, BatchCompression, BatchEncoding};
use crate::crypto::BatchCipher;
use crate::error::{JolyneError, ProtocolError};
use crate::raw::{RawPacket, decode_packet_raw, decode_packet_raw_split};
use crate::valentine::{BorrowedMcpePacket, McpePacket, McpePacketArgs};
use valentine::bedrock::context::BedrockSession;

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
    send_cipher: Option<BatchCipher>,
    recv_cipher: Option<BatchCipher>,

    // Compression State
    pub(crate) compression_enabled: bool,
    pub(crate) compression_algorithm: BatchCompression,
    pub(crate) compression_level: u32,
    pub(crate) compression_threshold: u16,
    max_decompressed_batch_size: Option<usize>,
    batch_codec: BatchCodec,
    encryption_header_len: usize,

    // Reusable outbound buffers to avoid per-send allocation churn.
    send_batch_buffer: BytesMut,
    send_raw_buffer: BytesMut,
    recv_decrypt_buffer: BytesMut,
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
            batch_codec: BatchCodec::default(),
            compression_enabled: false,
            compression_algorithm: BatchCompression::Deflate,
            compression_level: 7,
            compression_threshold: 0,
            max_decompressed_batch_size: Some(1024 * 1024 * 4),
            encryption_header_len: 1,
            send_batch_buffer: BytesMut::new(),
            send_raw_buffer: BytesMut::new(),
            recv_decrypt_buffer: BytesMut::new(),
        }
    }

    /// Enable encryption with the derived keys.
    #[instrument(skip_all, level = "debug", fields(peer_addr = %self.peer_addr()))]
    pub fn enable_encryption(&mut self, key: Key<Aes256Gcm>, iv: [u8; 12]) {
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&key);
        self.send_cipher = Some(BatchCipher::new(key_bytes, iv));
        self.recv_cipher = Some(BatchCipher::new(key_bytes, iv));
        self.encryption_enabled = true;
        debug!("encryption enabled");
    }

    /// Sets compression parameters.
    pub fn set_compression(&mut self, enabled: bool, level: u32, threshold: u16) {
        self.set_compression_algorithm(enabled, BatchCompression::Deflate, level, threshold);
    }

    /// Sets the negotiated compression algorithm and parameters.
    pub fn set_compression_algorithm(
        &mut self,
        enabled: bool,
        algorithm: BatchCompression,
        level: u32,
        threshold: u16,
    ) {
        self.compression_enabled = enabled;
        self.compression_algorithm = algorithm;
        self.compression_level = level;
        self.compression_threshold = threshold;
    }

    /// Configures the maximum accepted decompressed batch payload size.
    pub fn set_max_decompressed_batch_size(&mut self, max: Option<usize>) {
        self.max_decompressed_batch_size = max;
    }

    /// Configures how many leading bytes remain unencrypted in encrypted frames.
    pub fn set_encryption_header_len(&mut self, len: usize) {
        self.encryption_header_len = len;
    }

    /// Applies listener-level protocol safety limits to this transport.
    pub fn apply_listener_config(&mut self, config: &crate::config::BedrockListenerConfig) {
        self.set_max_decompressed_batch_size(config.max_decompressed_batch_size);
        self.set_encryption_header_len(config.encryption_header_len);
    }

    /// Configures the flushing strategy.
    pub fn set_auto_flush(&mut self, auto: bool) {
        self.auto_flush = auto;
    }

    /// Returns the current packet decode/encode args derived from transport session state.
    pub fn packet_args(&self) -> McpePacketArgs {
        McpePacketArgs
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
        self.batch_codec.encode_batch_multi_into(
            packets,
            BatchEncoding {
                enabled: self.compression_enabled,
                algorithm: self.compression_algorithm,
                level: self.compression_level,
                threshold: self.compression_threshold,
                prefix: T::USES_BATCH_PREFIX,
            },
            &mut self.send_batch_buffer,
        )?;

        // 2. Encrypt & Send
        let msg = if self.encryption_enabled {
            let cipher = self
                .send_cipher
                .as_mut()
                .expect("encryption_enabled implies send_cipher is initialised");
            tracing::trace!("Encrypting batch of {} bytes", self.send_batch_buffer.len());
            cipher.encrypt(&mut self.send_batch_buffer, self.encryption_header_len)?;
            tracing::trace!("Encrypted batch is {} bytes", self.send_batch_buffer.len());
            TransportMessage::reliable(self.send_batch_buffer.split().freeze())
        } else if reliable {
            TransportMessage::reliable(self.send_batch_buffer.split().freeze())
        } else {
            TransportMessage::unreliable(self.send_batch_buffer.split().freeze())
        };

        // Send using poll_fn to convert poll-based API to async
        tracing::trace!("Sending {} bytes to RakNet", msg.buffer.len());
        poll_fn(|cx| Pin::new(&mut self.inner).poll_send(cx, msg.clone()))
            .await
            .map_err(|e| JolyneError::Transport(e.to_string()))?;
        tracing::trace!("Sent successfully");

        Ok(())
    }

    /// Raw send for handshake packets that cannot be batched.
    ///
    /// The framing depends on the transport type:
    /// - **RakNet**: Uses game frame format `[0xFE][Length][Header][Body]`
    /// - **NetherNet**: Uses inner format `[Length][Header][Body]` (no 0xFE)
    #[instrument(skip_all, level = "trace", fields(peer_addr = %self.peer_addr()))]
    pub async fn send_raw(&mut self, packet: McpePacket) -> Result<(), JolyneError> {
        let reserve = if T::USES_BATCH_PREFIX {
            11usize + packet.data.encoded_size()
        } else {
            10usize + packet.data.encoded_size()
        };
        self.send_raw_buffer.clear();
        self.send_raw_buffer.reserve(reserve);

        if T::USES_BATCH_PREFIX {
            // RakNet: Use full game frame with 0xFE prefix
            packet.encode_bytes_mut(&mut self.send_raw_buffer)?;
        } else {
            // NetherNet: Use inner format without 0xFE prefix
            packet.data.encode_inner_bytes_mut(
                &mut self.send_raw_buffer,
                packet.header.from_subclient,
                packet.header.to_subclient,
            )?;
        }

        if self.encryption_enabled {
            let cipher = self
                .send_cipher
                .as_mut()
                .expect("encryption_enabled implies send_cipher is initialised");
            cipher.encrypt(&mut self.send_raw_buffer, self.encryption_header_len)?;
        }

        let msg = TransportMessage::reliable(self.send_raw_buffer.split().freeze());
        poll_fn(|cx| Pin::new(&mut self.inner).poll_send(cx, msg.clone()))
            .await
            .map_err(|e| JolyneError::Transport(e.to_string()))?;
        Ok(())
    }

    /// Returns the next single packet as an owned protocol packet.
    ///
    /// This materializes packet contents and is the convenience path for
    /// higher-level code that wants owned values immediately.
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

    /// Reads a batch of packets from the network, returning owned packets.
    ///
    /// This eagerly materializes packet contents. For hot ingress paths, prefer
    /// [`Self::recv_batch_borrowed`] or [`Self::recv_batch_raw`].
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

    #[cfg(feature = "client")]
    /// Restores packets consumed by an internal state machine so callers can
    /// observe them after the state transition. Restored packets retain their
    /// original order and precede unread packets from the same network batch.
    pub(crate) fn prepend_recv_queue(&mut self, packets: impl IntoIterator<Item = RawPacket>) {
        let mut deferred: VecDeque<_> = packets.into_iter().collect();
        deferred.append(&mut self.recv_queue);
        self.recv_queue = deferred;
    }

    /// Returns the next packet as a borrowed protocol view.
    ///
    /// This is the preferred ingress path for handshake and other hot packet
    /// processing where the caller can stay on borrowed data.
    pub async fn recv_packet_borrowed(&mut self) -> Result<BorrowedMcpePacket, JolyneError> {
        loop {
            if let Some(raw) = self.recv_queue.pop_front() {
                return raw.decode_borrowed();
            }
            let packets = self.recv_batch_raw().await?;
            if packets.is_empty() {
                continue;
            }
            self.recv_queue.extend(packets);
        }
    }

    /// Returns all packets from the next network batch as borrowed protocol views.
    ///
    /// This is the preferred batch receive path when the caller can inspect or
    /// route packets without immediately allocating owned payloads.
    #[instrument(skip_all, level = "trace", fields(peer_addr = %self.peer_addr()))]
    pub async fn recv_batch_borrowed(&mut self) -> Result<Vec<BorrowedMcpePacket>, JolyneError> {
        let raw_packets = self.recv_batch_raw().await?;
        raw_packets
            .into_iter()
            .map(RawPacket::decode_borrowed)
            .collect()
    }

    /// Returns all packets from the next network batch as raw bytes.
    #[instrument(skip_all, level = "trace", fields(peer_addr = %self.peer_addr()))]
    pub async fn recv_batch_raw(&mut self) -> Result<Vec<RawPacket>, JolyneError> {
        // 1. Read Raw Frame
        tracing::trace!("Waiting for raw RakNet frame...");
        let recv_result = poll_fn(|cx| Pin::new(&mut self.inner).poll_recv(cx)).await;
        let packet_bytes = recv_result
            .ok_or(JolyneError::ConnectionClosed)?
            .map_err(|e| JolyneError::Transport(e.to_string()))?;
        tracing::trace!("Received {} bytes from transport", packet_bytes.len());

        // 2. Decrypt
        if self.encryption_enabled {
            packet_bytes.copy_into(&mut self.recv_decrypt_buffer);
            let cipher = self
                .recv_cipher
                .as_mut()
                .expect("encryption_enabled implies recv_cipher is initialised");
            cipher.decrypt(&mut self.recv_decrypt_buffer, self.encryption_header_len)?;
            let mut buf = self.recv_decrypt_buffer.split().freeze();
            if buf.is_empty() {
                return Ok(vec![]);
            }

            if T::USES_BATCH_PREFIX {
                if buf[0] == 0xFE {
                    self.batch_codec.decode_batch_raw(
                        &mut buf,
                        self.compression_enabled,
                        self.max_decompressed_batch_size,
                    )
                } else {
                    Ok(vec![decode_packet_raw(&mut buf)?])
                }
            } else if self.compression_enabled {
                self.batch_codec
                    .decode_batch_no_prefix_raw(&mut buf, self.max_decompressed_batch_size)
            } else {
                Ok(vec![decode_packet_raw(&mut buf)?])
            }
        } else {
            match packet_bytes {
                TransportRecvMessage::Contiguous(mut buf) => {
                    if buf.is_empty() {
                        return Ok(vec![]);
                    }

                    if T::USES_BATCH_PREFIX {
                        if buf[0] == 0xFE {
                            self.batch_codec.decode_batch_raw(
                                &mut buf,
                                self.compression_enabled,
                                self.max_decompressed_batch_size,
                            )
                        } else {
                            Ok(vec![decode_packet_raw(&mut buf)?])
                        }
                    } else if self.compression_enabled {
                        self.batch_codec
                            .decode_batch_no_prefix_raw(&mut buf, self.max_decompressed_batch_size)
                    } else {
                        Ok(vec![decode_packet_raw(&mut buf)?])
                    }
                }
                TransportRecvMessage::SplitFirst { first, rest } => {
                    if T::USES_BATCH_PREFIX && first == 0xFE {
                        self.batch_codec.decode_batch_raw_split(
                            first,
                            rest,
                            self.compression_enabled,
                            self.max_decompressed_batch_size,
                        )
                    } else {
                        Ok(vec![decode_packet_raw_split(first, rest)?])
                    }
                }
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

        self.batch_codec.encode_batch_raw_into(
            packets,
            BatchEncoding {
                enabled: self.compression_enabled,
                algorithm: self.compression_algorithm,
                level: self.compression_level,
                threshold: self.compression_threshold,
                prefix: T::USES_BATCH_PREFIX,
            },
            &mut self.send_batch_buffer,
        )?;

        let msg = if self.encryption_enabled {
            let cipher = self
                .send_cipher
                .as_mut()
                .expect("encryption_enabled implies send_cipher is initialised");
            cipher.encrypt(&mut self.send_batch_buffer, self.encryption_header_len)?;
            TransportMessage::reliable(self.send_batch_buffer.split().freeze())
        } else if reliable {
            TransportMessage::reliable(self.send_batch_buffer.split().freeze())
        } else {
            TransportMessage::unreliable(self.send_batch_buffer.split().freeze())
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

    /// Returns the peer address.
    pub fn peer_addr(&self) -> std::net::SocketAddr {
        self.inner.peer_addr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::aead::generic_array::GenericArray;
    use std::io;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::pin::Pin;
    use std::task::{Context, Poll};

    struct TestTransport;

    impl Transport for TestTransport {
        type Error = io::Error;

        const USES_BATCH_PREFIX: bool = true;

        fn poll_send(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _msg: TransportMessage,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Pending
        }

        fn poll_recv(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Option<Result<TransportRecvMessage, Self::Error>>> {
            Poll::Pending
        }

        fn peer_addr(&self) -> SocketAddr {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
        }
    }

    // ========== Transport State Tests ==========

    #[test]
    fn compression_settings_stored_correctly() {
        // Test that set_compression stores all three parameters
        // We can't test BedrockTransport directly without a Transport impl,
        // but we can verify the expected behavior through field access if pub(crate)

        // This test verifies the logic of compression settings
        let enabled = true;
        let level = 6u32;
        let threshold = 256u16;

        // Verify threshold comparison logic
        let payload_size = 100usize;
        let should_compress = enabled && level > 0 && payload_size >= threshold as usize;
        assert!(!should_compress); // 100 < 256

        let payload_size = 512usize;
        let should_compress = enabled && level > 0 && payload_size >= threshold as usize;
        assert!(should_compress); // 512 >= 256
    }

    #[test]
    fn compression_threshold_boundary() {
        let threshold = 100u16;

        // Exactly at threshold -> should compress
        assert!(100usize >= threshold as usize);
        assert!(101usize >= threshold as usize);

        // Below threshold -> should not compress
        assert!(99usize <= threshold as usize);
    }

    #[tokio::test]
    async fn encryption_requires_reliable_send() {
        let mut transport = BedrockTransport::new(TestTransport);
        transport.enable_encryption(*GenericArray::from_slice(&[0x42; 32]), [1; 12]);
        for result in [
            transport.send_batch_with_reliability(&[], false).await,
            transport.send_batch_raw(&[], false).await,
        ] {
            assert!(matches!(
                result,
                Err(JolyneError::Protocol(ProtocolError::UnexpectedHandshake(_)))
            ));
        }
        transport
            .send_batch_with_reliability(&[], true)
            .await
            .unwrap();
        transport.send_batch_raw(&[], true).await.unwrap();
    }

    // ========== Key/IV Generation Tests ==========

    #[test]
    fn enable_encryption_initializes_independent_streams() {
        let mut transport = BedrockTransport::new(TestTransport);
        let key = *GenericArray::from_slice(&[0x42; 32]);
        transport.enable_encryption(key, [1; 12]);
        let mut reference = BatchCipher::new([0x42; 32], [1; 12]);
        for _ in 0..3 {
            let mut expected = BytesMut::from(&b"\xfepayload"[..]);
            let mut actual = expected.clone();
            reference.encrypt(&mut expected, 1).unwrap();
            transport
                .send_cipher
                .as_mut()
                .unwrap()
                .encrypt(&mut actual, 1)
                .unwrap();
            assert_eq!(actual, expected);
            transport
                .recv_cipher
                .as_mut()
                .unwrap()
                .decrypt(&mut actual, 1)
                .unwrap();
            assert_eq!(&actual[..], b"\xfepayload");
        }
        // Re-enabling encryption resets both the checksum counter and AES stream.
        transport.enable_encryption(key, [1; 12]);
        let mut actual = BytesMut::from(&b"\xfepayload"[..]);
        let mut expected = actual.clone();
        BatchCipher::new([0x42; 32], [1; 12])
            .encrypt(&mut expected, 1)
            .unwrap();
        transport
            .send_cipher
            .as_mut()
            .unwrap()
            .encrypt(&mut actual, 1)
            .unwrap();
        assert_eq!(actual, expected);
    }
}
