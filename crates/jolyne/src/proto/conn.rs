//! Sans-IO connection codec: framing, compression, encryption and queues.
//!
//! [`Connection`] never touches a socket. Datagrams go in through
//! [`Connection::handle_datagram`]; decoded packets come out of
//! [`Connection::next_packet`]. Outbound packets are framed straight into the
//! pending batch buffer as they are queued and sealed (compressed, then
//! encrypted) into [`Transmit`]s that the driver drains with
//! [`Connection::poll_transmit`].
//!
//! Ordering guarantees the vanilla handshake relies on fall out of the queue:
//! enabling compression or encryption first seals whatever is pending, so a
//! `NetworkSettings` or `ServerToClientHandshake` queued before the switch is
//! always transmitted in the old (plain) format.

use std::collections::VecDeque;
use std::time::Instant;

use bytes::{Bytes, BytesMut};

use super::batch::{BatchCodec, BatchCompression, BatchEncoding};
use super::crypto::BatchCipher;
use super::custom::{CustomPacket, PacketFilter};
use super::observe::{
    ConnectionMetrics, Direction, HexPreview, PacketEvent, WIRE_TARGET, trace_packet,
    wire_trace_enabled,
};
use super::raw::{PacketHeader, RawPacket};
use crate::config::{ConnectionOptions, FlushMode};
use crate::error::{JolyneError, PeerDisconnect, ProtocolError};
use crate::valentine::{
    DisconnectPacket, DisconnectPacketMessages, DisconnectPacketMessagesUnion,
    EnumsConnectionDisconnectFailReason, McpePacket, McpePacketData, McpePacketName,
};

/// One sealed batch to hand to the transport.
#[derive(Debug, Clone)]
pub struct Transmit {
    pub payload: Bytes,
    /// `true` = reliable ordered delivery (mandatory while encrypted).
    pub reliable: bool,
}

/// One inbound transport message.
#[derive(Debug, Clone)]
pub enum Datagram {
    Contiguous(Bytes),
    /// RakNet surfaces the message ID byte separately from the payload.
    SplitFirst {
        first: u8,
        rest: Bytes,
    },
}

impl Datagram {
    pub fn len(&self) -> usize {
        match self {
            Self::Contiguous(bytes) => bytes.len(),
            Self::SplitFirst { rest, .. } => 1 + rest.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Joins a split datagram into one buffer (copies only for `SplitFirst`).
    pub fn into_bytes(self) -> Bytes {
        match self {
            Self::Contiguous(bytes) => bytes,
            Self::SplitFirst { first, rest } => {
                let mut out = BytesMut::with_capacity(1 + rest.len());
                out.extend_from_slice(&[first]);
                out.extend_from_slice(&rest);
                out.freeze()
            }
        }
    }
}

/// Which framing the transport uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    /// `[0xFE][batch]` (RakNet).
    RakNet,
    /// `[batch]` (NetherNet and other message transports).
    Unprefixed,
}

#[derive(Debug, Clone, Copy)]
struct Compression {
    algorithm: BatchCompression,
    level: u32,
    threshold: u16,
}

/// Sans-IO Bedrock connection state for one peer.
pub struct Connection {
    framing: Framing,
    compression: Option<Compression>,
    // Boxed: the AES key schedules are large and connections move through
    // every phase transition (and async state machines) by value.
    send_cipher: Option<Box<BatchCipher>>,
    recv_cipher: Option<Box<BatchCipher>>,
    codec: Box<BatchCodec>,
    options: ConnectionOptions,
    filter: PacketFilter,
    inbound: VecDeque<RawPacket>,
    /// The batch being built: marker + algorithm placeholder + framed packets.
    pending: BytesMut,
    pending_header: usize,
    pending_packets: usize,
    pending_since: Option<Instant>,
    outbound: VecDeque<Transmit>,
    outbound_bytes: usize,
    metrics: ConnectionMetrics,
    peer_closed: bool,
    /// Reliability of auto-sealed batches (false only inside `send_batch`).
    batch_reliable: bool,
}

impl Connection {
    pub fn new(framing: Framing, options: ConnectionOptions) -> Self {
        let filter = PacketFilter {
            unknown: options.unknown_packets,
            custom: options.custom_packets,
        };
        Self {
            framing,
            compression: None,
            send_cipher: None,
            recv_cipher: None,
            codec: Box::default(),
            options,
            filter,
            inbound: VecDeque::new(),
            pending: BytesMut::new(),
            pending_header: 0,
            pending_packets: 0,
            pending_since: None,
            outbound: VecDeque::new(),
            outbound_bytes: 0,
            metrics: ConnectionMetrics::default(),
            peer_closed: false,
            batch_reliable: true,
        }
    }

    #[inline]
    fn prefix(&self) -> bool {
        self.framing == Framing::RakNet
    }

    pub fn framing(&self) -> Framing {
        self.framing
    }

    pub fn options(&self) -> &ConnectionOptions {
        &self.options
    }

    pub fn metrics(&self) -> ConnectionMetrics {
        self.metrics
    }

    /// Counts a packet that failed typed decoding outside the codec.
    pub fn record_decode_error(&mut self) {
        self.metrics.decode_errors += 1;
    }

    /// Queues a `Disconnect` whose reason is mapped from `error`, when the
    /// peer should be told (see [`JolyneError::disconnect_reason`]).
    pub fn queue_failure_disconnect(&mut self, error: &JolyneError, now: Instant) {
        if let Some(reason) = error.disconnect_reason() {
            // Best effort: the connection is failing anyway.
            let _ = self.queue_disconnect(reason, &error.to_string(), now);
        }
    }

    pub fn is_encrypted(&self) -> bool {
        self.send_cipher.is_some()
    }

    pub fn is_compressed(&self) -> bool {
        self.compression.is_some()
    }

    pub fn flush_mode(&self) -> FlushMode {
        self.options.flush
    }

    /// Changes the flush mode. Switching to `Immediate` seals pending
    /// packets; switching to `Interval` arms the timer for packets already
    /// pending.
    pub fn set_flush_mode(&mut self, mode: FlushMode, now: Instant) -> Result<(), JolyneError> {
        self.options.flush = mode;
        match mode {
            FlushMode::Immediate => self.flush()?,
            FlushMode::Interval(_) if self.pending_packets > 0 => {
                self.pending_since.get_or_insert(now);
            }
            FlushMode::Interval(_) | FlushMode::Manual => self.pending_since = None,
        }
        Ok(())
    }

    /// Installs negotiated compression. Pending packets are sealed first so
    /// they keep the pre-negotiation (uncompressed, no algorithm byte) format.
    pub fn enable_compression(
        &mut self,
        algorithm: BatchCompression,
        level: u32,
        threshold: u16,
    ) -> Result<(), JolyneError> {
        self.flush()?;
        self.compression = Some(Compression {
            algorithm,
            level,
            threshold,
        });
        Ok(())
    }

    /// Installs AES-256-CTR for both directions. Pending packets are sealed
    /// first, so a queued `ServerToClientHandshake` leaves in plaintext.
    pub fn enable_encryption(&mut self, key: [u8; 32], iv: [u8; 12]) -> Result<(), JolyneError> {
        self.flush()?;
        self.send_cipher = Some(Box::new(BatchCipher::new(key, iv)));
        self.recv_cipher = Some(Box::new(BatchCipher::new(key, iv)));
        Ok(())
    }

    fn encoding(&self) -> BatchEncoding {
        match self.compression {
            Some(c) => BatchEncoding {
                enabled: true,
                algorithm: c.algorithm,
                level: c.level,
                threshold: c.threshold,
                prefix: self.prefix(),
            },
            None => BatchEncoding {
                enabled: false,
                algorithm: BatchCompression::None,
                level: 0,
                threshold: 0,
                prefix: self.prefix(),
            },
        }
    }

    // ---------------------------------------------------------------- inbound

    /// Decodes one transport message into the inbound queue.
    ///
    /// Errors are terminal for the connection (framing, decompression,
    /// checksum, unknown packet under `Reject`, limits).
    pub fn handle_datagram(&mut self, datagram: Datagram) -> Result<(), JolyneError> {
        let result = self.decode_datagram(datagram);
        if result.is_err() {
            self.metrics.decode_errors += 1;
        }
        result
    }

    fn decode_datagram(&mut self, datagram: Datagram) -> Result<(), JolyneError> {
        let wire_len = datagram.len();
        self.metrics.bytes_in += wire_len as u64;
        self.metrics.batches_in += 1;
        // `skip` cleartext marker bytes stay at the front until after
        // decryption; advancing (not splitting) them never allocates.
        let (body, skip) = match (self.framing, datagram) {
            (_, datagram) if datagram.is_empty() => return Ok(()),
            (Framing::RakNet, Datagram::SplitFirst { first, rest }) => {
                BatchCodec::check_prefix(first)?;
                (rest, 0)
            }
            (Framing::RakNet, Datagram::Contiguous(bytes)) => {
                BatchCodec::check_prefix(bytes[0])?;
                (bytes, 1)
            }
            (Framing::Unprefixed, datagram) => (datagram.into_bytes(), 0),
        };
        let body = match self.recv_cipher.as_mut() {
            Some(cipher) => {
                // Decrypt in place when the transport handed us the only
                // reference; otherwise copy once.
                let mut buf = body
                    .try_into_mut()
                    .unwrap_or_else(|shared| BytesMut::from(&shared[..]));
                cipher.decrypt(&mut buf, skip)?;
                bytes::Buf::advance(&mut buf, skip);
                buf.freeze()
            }
            None => {
                let mut body = body;
                bytes::Buf::advance(&mut body, skip);
                body
            }
        };
        let algorithm = self
            .compression
            .is_some()
            .then(|| body.first().copied())
            .flatten();
        let start = self.inbound.len();
        let stats = self.codec.decode_raw_into(
            body,
            self.compression.is_some(),
            self.options.limits,
            &self.filter,
            &mut self.inbound,
        )?;
        self.metrics.packets_in += stats.packets as u64;
        self.metrics.unknown_packets += (stats.skipped_unknown + stats.passed_unknown) as u64;
        if wire_trace_enabled() {
            tracing::trace!(
                target: WIRE_TARGET,
                direction = ?Direction::Inbound,
                wire_len,
                encrypted = self.recv_cipher.is_some(),
                algorithm = ?algorithm,
                packets = stats.packets,
                "batch"
            );
            for packet in self.inbound.range(start..) {
                trace_packet(Direction::Inbound, packet.header(), packet.body());
            }
        }
        for packet in self.inbound.range(start..) {
            if packet.is(McpePacketName::PacketViolationWarningPacket) {
                // The peer rejected one of our packets; the connection
                // usually closes next without a reason.
                tracing::warn!(
                    report = %HexPreview(packet.body()),
                    decoded = ?packet.clone().decode().map(|packet| packet.data),
                    "peer reported a packet violation"
                );
            }
        }
        if let Some(observer) = &self.options.observer {
            for packet in self.inbound.range(start..) {
                observer.observe(PacketEvent {
                    direction: Direction::Inbound,
                    header: packet.header(),
                    payload: packet.body(),
                });
            }
        }
        Ok(())
    }

    /// The next decoded packet, if any.
    ///
    /// An inbound `Disconnect` is surfaced as [`JolyneError::Disconnected`]
    /// (carrying the raw packet for proxies); afterwards the connection
    /// reports [`JolyneError::ConnectionClosed`].
    pub fn next_packet(&mut self) -> Result<Option<RawPacket>, JolyneError> {
        if self.peer_closed {
            return Err(JolyneError::ConnectionClosed);
        }
        let Some(packet) = self.inbound.pop_front() else {
            return Ok(None);
        };
        if packet.is(McpePacketName::DisconnectPacket) {
            self.peer_closed = true;
            return Err(disconnect_error(packet));
        }
        Ok(Some(packet))
    }

    /// Number of decoded packets waiting in the inbound queue.
    pub fn inbound_len(&self) -> usize {
        self.inbound.len()
    }

    /// Re-queues packets consumed by a handshake phase ahead of unread ones,
    /// preserving their order.
    pub fn prepend_inbound(&mut self, packets: impl DoubleEndedIterator<Item = RawPacket>) {
        for packet in packets.rev() {
            self.inbound.push_front(packet);
        }
    }

    // --------------------------------------------------------------- outbound

    fn begin_pending(&mut self, now: Instant) {
        if self.pending_packets == 0 {
            let encoding = self.encoding();
            self.pending_header =
                BatchCodec::start(&mut self.pending, encoding.enabled, encoding.prefix, 256);
            if let FlushMode::Interval(_) = self.options.flush {
                self.pending_since = Some(now);
            }
        }
    }

    /// Drops a partially written frame after a failed encode so the packets
    /// already queued in this batch stay intact.
    fn abandon_frame(&mut self, frame_start: usize) {
        self.pending.truncate(frame_start);
        if self.pending_packets == 0 {
            self.pending_since = None;
        }
    }

    fn after_queue(&mut self, frame_start: usize) -> Result<(), JolyneError> {
        self.pending_packets += 1;
        let trace = wire_trace_enabled();
        if trace || self.options.observer.is_some() {
            let frame = &self.pending[frame_start..];
            let mut cursor = frame;
            let _len = valentine::protocol::wire::read_var_u32(&mut cursor)?;
            let header_raw = valentine::protocol::wire::read_var_u32(&mut cursor)?;
            let header = PacketHeader {
                id: (header_raw & 0x3FF) as u16,
                from_subclient: ((header_raw >> 10) & 3) as u8,
                to_subclient: ((header_raw >> 12) & 3) as u8,
            };
            if trace {
                trace_packet(Direction::Outbound, header, cursor);
            }
            if let Some(observer) = &self.options.observer {
                observer.observe(PacketEvent {
                    direction: Direction::Outbound,
                    header,
                    payload: cursor,
                });
            }
        }
        if self.options.flush == FlushMode::Immediate
            || self.pending_packets >= self.options.max_packets_per_outbound_batch
            || self.pending.len() >= self.options.max_pending_bytes
        {
            self.seal(self.batch_reliable)?;
        }
        Ok(())
    }

    /// Frames a packet body by reference (no `McpePacketData` wrapper, so
    /// shared `Arc` packets are encoded without a deep clone).
    pub fn queue_body<P>(
        &mut self,
        name: McpePacketName,
        body: &P,
        now: Instant,
    ) -> Result<(), JolyneError>
    where
        P: valentine::bedrock::codec::BedrockCodec + valentine::bedrock::codec::BedrockSized,
    {
        use valentine::protocol::wire;
        self.begin_pending(now);
        let start = self.pending.len();
        let header = name as u32;
        let len = wire::var_u32_len(header) + body.encoded_size();
        let len = u32::try_from(len).map_err(|_| ProtocolError::PacketTooLarge(len))?;
        self.pending.reserve(5 + len as usize);
        wire::write_var_u32(&mut self.pending, len);
        wire::write_var_u32(&mut self.pending, header);
        if let Err(error) = body.encode(&mut self.pending) {
            self.abandon_frame(start);
            return Err(error.into());
        }
        self.after_queue(start)
    }

    /// Frames a valentine packet into the pending batch.
    pub fn queue_packet(&mut self, packet: &McpePacket, now: Instant) -> Result<(), JolyneError> {
        use valentine::bedrock::codec::BedrockSized;
        self.begin_pending(now);
        // Exact capacity up front: the encoder sizes its length prefix from the
        // spare capacity, and growing mid-encode would copy the batch.
        self.pending.reserve(10 + packet.data.encoded_size());
        self.encode_packet(packet)
    }

    /// Encodes into capacity the caller already reserved.
    fn encode_packet(&mut self, packet: &McpePacket) -> Result<(), JolyneError> {
        let start = self.pending.len();
        if let Err(error) = packet.data.encode_inner_bytes_mut(
            &mut self.pending,
            packet.header.from_subclient,
            packet.header.to_subclient,
        ) {
            self.abandon_frame(start);
            return Err(error.into());
        }
        self.after_queue(start)
    }

    /// Copies a raw packet's frame into the pending batch.
    pub fn queue_raw(&mut self, packet: &RawPacket, now: Instant) -> Result<(), JolyneError> {
        self.begin_pending(now);
        let start = self.pending.len();
        self.pending.extend_from_slice(packet.inner_frame());
        self.after_queue(start)
    }

    /// Encodes a custom packet into the pending batch.
    pub fn queue_custom<P: CustomPacket>(
        &mut self,
        packet: &P,
        now: Instant,
    ) -> Result<(), JolyneError> {
        let raw = RawPacket::from_custom(packet)?;
        self.queue_raw(&raw, now)
    }

    /// Queues `Disconnect{reason, message}` and seals it.
    pub fn queue_disconnect(
        &mut self,
        reason: EnumsConnectionDisconnectFailReason,
        message: &str,
        now: Instant,
    ) -> Result<(), JolyneError> {
        let messages = if message.is_empty() {
            DisconnectPacketMessagesUnion::Empty1
        } else {
            DisconnectPacketMessagesUnion::DisconnectPacketMessages(DisconnectPacketMessages {
                message: message.into(),
                filtered_message: message.into(),
            })
        };
        self.queue_packet(
            &McpePacket::from(DisconnectPacket { reason, messages }),
            now,
        )?;
        self.flush()
    }

    /// Seals the pending batch into a reliable transmit. No-op when empty.
    pub fn flush(&mut self) -> Result<(), JolyneError> {
        if self.pending_packets == 0 {
            return Ok(());
        }
        self.seal(true)
    }

    fn seal(&mut self, reliable: bool) -> Result<(), JolyneError> {
        let packets = self.pending_packets as u64;
        self.pending_packets = 0;
        self.pending_since = None;
        let encoding = self.encoding();
        self.codec
            .finish(&mut self.pending, self.pending_header, encoding)?;
        // Check the bound before encrypting: a refused batch must not advance
        // the cipher, or every later batch would fail the peer's checksum.
        let len = self.pending.len() + if self.send_cipher.is_some() { 8 } else { 0 };
        if self.outbound_bytes + len > self.options.max_queued_bytes {
            self.pending.clear();
            return Err(ProtocolError::WriteBufferFull {
                pending: self.outbound_bytes + len,
                max: self.options.max_queued_bytes,
            }
            .into());
        }
        let header_len = usize::from(self.prefix());
        if wire_trace_enabled() {
            let body = &self.pending[header_len..];
            tracing::trace!(
                target: WIRE_TARGET,
                direction = ?Direction::Outbound,
                wire_len = len,
                encrypted = self.send_cipher.is_some(),
                algorithm = ?self.compression.and(body.first()),
                packets,
                reliable,
                plain = %HexPreview(body),
                "batch"
            );
        }
        if let Some(cipher) = self.send_cipher.as_mut() {
            cipher.encrypt(&mut self.pending, header_len)?;
        }
        let payload = self.pending.split().freeze();
        self.outbound_bytes += len;
        self.metrics.packets_out += packets;
        self.metrics.batches_out += 1;
        self.metrics.bytes_out += len as u64;
        self.outbound.push_back(Transmit { payload, reliable });
        Ok(())
    }

    /// Seals pending packets, then sends `packets` as their own batch (split
    /// into several when they exceed the per-batch limits) with the given
    /// reliability. Unreliable delivery is refused while encrypted.
    pub fn send_batch(
        &mut self,
        packets: &[McpePacket],
        reliable: bool,
        now: Instant,
    ) -> Result<(), JolyneError> {
        use valentine::bedrock::codec::BedrockSized;
        let estimate = packets.iter().map(|p| 10 + p.data.encoded_size()).sum();
        self.send_batch_with(packets.len(), reliable, estimate, now, |conn, index| {
            // Re-opens the batch after an auto-seal at the per-batch limits.
            conn.begin_pending(now);
            conn.encode_packet(&packets[index])
        })
    }

    /// Like [`Self::send_batch`] for raw packets.
    pub fn send_batch_raw(
        &mut self,
        packets: &[RawPacket],
        reliable: bool,
        now: Instant,
    ) -> Result<(), JolyneError> {
        let estimate = packets.iter().map(|p| p.inner_frame().len()).sum();
        self.send_batch_with(packets.len(), reliable, estimate, now, |conn, index| {
            conn.queue_raw(&packets[index], now)
        })
    }

    fn send_batch_with(
        &mut self,
        count: usize,
        reliable: bool,
        estimate: usize,
        now: Instant,
        mut queue: impl FnMut(&mut Self, usize) -> Result<(), JolyneError>,
    ) -> Result<(), JolyneError> {
        if !reliable && self.is_encrypted() {
            return Err(ProtocolError::UnreliableWhileEncrypted.into());
        }
        if count == 0 {
            return Ok(());
        }
        self.flush()?;
        // One allocation for the whole batch instead of growing per packet.
        self.begin_pending(now);
        self.pending
            .reserve(estimate.min(self.options.max_pending_bytes));
        self.grouped(reliable, |conn| {
            (0..count).try_for_each(|index| queue(conn, index))
        })
    }

    /// Seals pending packets, then sends everything `queue` adds as one
    /// reliable batch whatever the flush mode (split only at the per-batch
    /// limits), so a group of packets reaches the peer together as BDS sends
    /// it.
    pub fn queue_group(
        &mut self,
        queue: impl FnOnce(&mut Self) -> Result<(), JolyneError>,
    ) -> Result<(), JolyneError> {
        self.flush()?;
        self.grouped(true, queue)
    }

    /// Runs `queue` in manual flush mode, then seals what it queued.
    fn grouped(
        &mut self,
        reliable: bool,
        queue: impl FnOnce(&mut Self) -> Result<(), JolyneError>,
    ) -> Result<(), JolyneError> {
        let mode = std::mem::replace(&mut self.options.flush, FlushMode::Manual);
        self.batch_reliable = reliable;
        let result = queue(self).and_then(|()| {
            if self.pending_packets > 0 {
                self.seal(reliable)
            } else {
                Ok(())
            }
        });
        self.batch_reliable = true;
        self.options.flush = mode;
        result
    }

    /// Next sealed batch for the transport.
    pub fn poll_transmit(&mut self) -> Option<Transmit> {
        let transmit = self.outbound.pop_front()?;
        self.outbound_bytes -= transmit.payload.len();
        Some(transmit)
    }

    pub fn has_transmits(&self) -> bool {
        !self.outbound.is_empty()
    }

    /// Packets queued but not yet sealed.
    pub fn pending_packets(&self) -> usize {
        self.pending_packets
    }

    /// When the interval flush timer fires, if armed.
    pub fn poll_timeout(&self) -> Option<Instant> {
        match (self.options.flush, self.pending_since) {
            (FlushMode::Interval(interval), Some(since)) => Some(since + interval),
            _ => None,
        }
    }

    /// Seals the pending batch if the interval flush deadline has passed.
    pub fn handle_timeout(&mut self, now: Instant) -> Result<(), JolyneError> {
        if self.poll_timeout().is_some_and(|deadline| now >= deadline) {
            self.flush()?;
        }
        Ok(())
    }
}

/// Builds the typed error for an inbound `Disconnect` packet.
pub(crate) fn disconnect_error(packet: RawPacket) -> JolyneError {
    let (reason, message) = match packet.clone().decode() {
        Ok(McpePacket {
            data: McpePacketData::DisconnectPacket(disconnect),
            ..
        }) => {
            let message = match disconnect.messages {
                DisconnectPacketMessagesUnion::DisconnectPacketMessages(messages) => {
                    Some(messages.message.to_string())
                }
                DisconnectPacketMessagesUnion::Empty1 => None,
            };
            (disconnect.reason, message)
        }
        _ => (EnumsConnectionDisconnectFailReason::Unknown, None),
    };
    JolyneError::Disconnected(Box::new(PeerDisconnect {
        reason,
        message,
        packet,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::batch::decode_batch;
    use crate::valentine::{PlayStatusPacket, PlayStatusPacketStatus, TextPacket};
    use parking_lot::Mutex;
    use std::sync::Arc;
    use std::time::Duration;

    fn status() -> McpePacket {
        McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::Loginsuccess,
        })
    }

    fn pair(options: ConnectionOptions) -> (Connection, Connection) {
        (
            Connection::new(Framing::RakNet, options.clone()),
            Connection::new(Framing::RakNet, options),
        )
    }

    fn deliver(from: &mut Connection, to: &mut Connection) {
        while let Some(transmit) = from.poll_transmit() {
            to.handle_datagram(Datagram::Contiguous(transmit.payload))
                .unwrap();
        }
    }

    #[test]
    fn enabling_compression_and_encryption_seals_pending_in_the_old_format() {
        let (mut a, mut b) = pair(ConnectionOptions {
            flush: FlushMode::Manual,
            ..Default::default()
        });
        let now = Instant::now();
        a.queue_packet(&status(), now).unwrap();
        a.enable_compression(BatchCompression::Deflate, 7, 0)
            .unwrap();
        let first = a
            .poll_transmit()
            .expect("pending sealed by enable_compression");
        // Plain: no algorithm byte, decodable as an uncompressed batch.
        assert_eq!(
            decode_batch(&mut first.payload.clone(), false)
                .unwrap()
                .len(),
            1
        );

        a.queue_packet(&status(), now).unwrap();
        a.enable_encryption([7; 32], [9; 12]).unwrap();
        let second = a
            .poll_transmit()
            .expect("pending sealed by enable_encryption");
        assert_eq!(
            second.payload[1],
            BatchCompression::Deflate as u8,
            "compressed, not encrypted"
        );

        b.handle_datagram(Datagram::Contiguous(first.payload))
            .unwrap();
        b.enable_compression(BatchCompression::Deflate, 7, 0)
            .unwrap();
        b.handle_datagram(Datagram::Contiguous(second.payload))
            .unwrap();
        b.enable_encryption([7; 32], [9; 12]).unwrap();
        a.queue_packet(&status(), now).unwrap();
        a.flush().unwrap();
        deliver(&mut a, &mut b);
        for _ in 0..3 {
            assert!(
                b.next_packet()
                    .unwrap()
                    .unwrap()
                    .is(McpePacketName::PlayStatusPacket)
            );
        }
        assert_eq!(b.metrics().packets_in, 3);
        assert_eq!(a.metrics().batches_out, 3);
    }

    #[test]
    fn flush_modes_immediate_manual_interval_and_auto_seal_limits() {
        let now = Instant::now();
        let mut immediate = Connection::new(Framing::RakNet, ConnectionOptions::default());
        immediate.queue_packet(&status(), now).unwrap();
        immediate.queue_packet(&status(), now).unwrap();
        assert_eq!(immediate.metrics().batches_out, 2);

        let mut manual = Connection::new(
            Framing::RakNet,
            ConnectionOptions {
                flush: FlushMode::Manual,
                max_packets_per_outbound_batch: 3,
                ..Default::default()
            },
        );
        for _ in 0..7 {
            manual.queue_packet(&status(), now).unwrap();
        }
        assert_eq!(manual.metrics().batches_out, 2, "auto-sealed at 3 packets");
        assert_eq!(manual.pending_packets(), 1);
        assert_eq!(manual.poll_timeout(), None);

        let mut interval = Connection::new(
            Framing::RakNet,
            ConnectionOptions {
                flush: FlushMode::Interval(Duration::from_millis(50)),
                ..Default::default()
            },
        );
        interval.queue_packet(&status(), now).unwrap();
        interval
            .queue_packet(&status(), now + Duration::from_millis(10))
            .unwrap();
        assert_eq!(
            interval.poll_timeout(),
            Some(now + Duration::from_millis(50))
        );
        interval
            .handle_timeout(now + Duration::from_millis(49))
            .unwrap();
        assert!(!interval.has_transmits());
        interval
            .handle_timeout(now + Duration::from_millis(50))
            .unwrap();
        assert_eq!(interval.metrics().packets_out, 2);
        assert_eq!(interval.metrics().batches_out, 1);
        assert_eq!(interval.poll_timeout(), None);
    }

    /// Regression: the bound was checked after encrypting, so a refused batch
    /// advanced the cipher and every later batch failed the peer's checksum.
    #[test]
    fn refused_batch_leaves_the_cipher_in_sync() {
        let options = ConnectionOptions {
            max_queued_bytes: 40,
            ..Default::default()
        };
        let (mut a, mut b) = pair(options);
        a.enable_encryption([5; 32], [6; 12]).unwrap();
        b.enable_encryption([5; 32], [6; 12]).unwrap();
        let now = Instant::now();
        a.queue_packet(&status(), now).unwrap();
        a.queue_packet(&status(), now).unwrap();
        assert!(matches!(
            a.queue_packet(&status(), now),
            Err(JolyneError::Protocol(ProtocolError::WriteBufferFull {
                max: 40,
                ..
            }))
        ));
        deliver(&mut a, &mut b);
        a.queue_packet(&status(), now).unwrap();
        deliver(&mut a, &mut b);
        let mut received = 0;
        while b.next_packet().unwrap().is_some() {
            received += 1;
        }
        assert_eq!(received, 3, "batches after the refusal still decrypt");
    }

    struct Unencodable;

    impl valentine::bedrock::codec::BedrockSized for Unencodable {
        fn encoded_size(&self) -> usize {
            4
        }
    }

    impl valentine::bedrock::codec::BedrockCodec for Unencodable {
        type Args = ();
        fn encode<B: bytes::BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
            buf.put_u8(0xAA);
            Err(std::io::Error::other("unencodable"))
        }
        fn decode<B: bytes::Buf>(
            _: &mut B,
            _: (),
        ) -> Result<Self, valentine::bedrock::error::DecodeError> {
            Ok(Self)
        }
    }

    /// Regression: a failed encode left a partial frame in the batch and, in
    /// interval mode, an armed timer with nothing pending (a busy loop).
    #[test]
    fn failed_encode_keeps_the_pending_batch_intact() {
        let interval = Duration::from_millis(50);
        let (mut a, mut b) = pair(ConnectionOptions {
            flush: FlushMode::Interval(interval),
            ..Default::default()
        });
        let now = Instant::now();
        assert!(
            a.queue_body(McpePacketName::TextPacket, &Unencodable, now)
                .is_err()
        );
        assert_eq!(a.poll_timeout(), None, "no timer without pending packets");
        a.queue_packet(&status(), now).unwrap();
        assert!(
            a.queue_body(McpePacketName::TextPacket, &Unencodable, now)
                .is_err()
        );
        a.queue_packet(&status(), now).unwrap();
        a.handle_timeout(now + interval).unwrap();
        deliver(&mut a, &mut b);
        for _ in 0..2 {
            assert!(
                b.next_packet()
                    .unwrap()
                    .unwrap()
                    .is(McpePacketName::PlayStatusPacket)
            );
        }
        assert!(b.next_packet().unwrap().is_none());
    }

    #[test]
    fn switching_to_interval_arms_the_timer_for_pending_packets() {
        let mut conn = Connection::new(
            Framing::RakNet,
            ConnectionOptions {
                flush: FlushMode::Manual,
                ..Default::default()
            },
        );
        let now = Instant::now();
        conn.queue_packet(&status(), now).unwrap();
        assert_eq!(conn.poll_timeout(), None);
        let interval = Duration::from_millis(20);
        conn.set_flush_mode(FlushMode::Interval(interval), now)
            .unwrap();
        assert_eq!(conn.poll_timeout(), Some(now + interval));
        conn.handle_timeout(now + interval).unwrap();
        assert!(conn.has_transmits());
    }

    #[test]
    fn send_batch_splits_at_the_limit_with_the_requested_reliability() {
        let mut conn = Connection::new(
            Framing::RakNet,
            ConnectionOptions {
                max_packets_per_outbound_batch: 2,
                ..Default::default()
            },
        );
        conn.send_batch(&[status(), status(), status()], false, Instant::now())
            .unwrap();
        let reliabilities: Vec<_> = std::iter::from_fn(|| conn.poll_transmit())
            .map(|transmit| transmit.reliable)
            .collect();
        assert_eq!(reliabilities, [false, false]);
    }

    #[test]
    fn unreliable_batches_are_refused_once_encrypted() {
        let mut conn = Connection::new(Framing::RakNet, ConnectionOptions::default());
        let now = Instant::now();
        conn.send_batch(&[status()], false, now).unwrap();
        assert!(!conn.poll_transmit().unwrap().reliable);
        conn.enable_encryption([1; 32], [2; 12]).unwrap();
        assert!(matches!(
            conn.send_batch(&[status()], false, now),
            Err(JolyneError::Protocol(
                ProtocolError::UnreliableWhileEncrypted
            ))
        ));
    }

    #[test]
    fn raknet_frames_must_carry_the_batch_marker() {
        let mut conn = Connection::new(Framing::RakNet, ConnectionOptions::default());
        assert!(matches!(
            conn.handle_datagram(Datagram::SplitFirst {
                first: 0x84,
                rest: Bytes::from_static(&[1, 2])
            }),
            Err(JolyneError::Protocol(ProtocolError::InvalidBatchId(0x84)))
        ));
        assert_eq!(conn.metrics().decode_errors, 1);
    }

    #[test]
    fn checksum_failure_is_terminal_for_the_connection() {
        let (mut a, mut b) = pair(ConnectionOptions::default());
        a.enable_encryption([3; 32], [4; 12]).unwrap();
        b.enable_encryption([3; 32], [4; 12]).unwrap();
        a.queue_packet(&status(), Instant::now()).unwrap();
        a.queue_packet(&status(), Instant::now()).unwrap();
        let mut tampered = BytesMut::from(&a.poll_transmit().unwrap().payload[..]);
        tampered[3] ^= 1;
        assert!(matches!(
            b.handle_datagram(Datagram::Contiguous(tampered.freeze())),
            Err(JolyneError::Protocol(
                ProtocolError::ChecksumMismatch { .. }
            ))
        ));
        let valid = a.poll_transmit().unwrap();
        assert!(matches!(
            b.handle_datagram(Datagram::Contiguous(valid.payload)),
            Err(JolyneError::Protocol(ProtocolError::CipherInvalidated))
        ));
    }

    #[test]
    fn inbound_disconnect_is_a_typed_error_then_closed() {
        let (mut a, mut b) = pair(ConnectionOptions::default());
        a.queue_disconnect(
            EnumsConnectionDisconnectFailReason::Kicked,
            "bye",
            Instant::now(),
        )
        .unwrap();
        deliver(&mut a, &mut b);
        match b.next_packet() {
            Err(JolyneError::Disconnected(disconnect)) => {
                assert_eq!(
                    disconnect.reason,
                    EnumsConnectionDisconnectFailReason::Kicked
                );
                assert_eq!(disconnect.message.as_deref(), Some("bye"));
                assert!(disconnect.packet.is(McpePacketName::DisconnectPacket));
            }
            other => panic!("expected Disconnected, got {other:?}"),
        }
        assert!(matches!(
            b.next_packet(),
            Err(JolyneError::ConnectionClosed)
        ));
    }

    #[test]
    fn observer_sees_both_directions_with_bodies() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let options = ConnectionOptions {
            observer: Some(Arc::new(move |event: PacketEvent<'_>| {
                sink.lock()
                    .push((event.direction, event.header.id, event.payload.to_vec()));
            })),
            ..Default::default()
        };
        let (mut a, mut b) = pair(options);
        a.queue_packet(&status(), Instant::now()).unwrap();
        deliver(&mut a, &mut b);
        let seen = seen.lock();
        assert_eq!(
            *seen,
            vec![
                (Direction::Outbound, 2, vec![0, 0, 0, 0]),
                (Direction::Inbound, 2, vec![0, 0, 0, 0]),
            ]
        );
    }

    #[test]
    fn unprefixed_framing_roundtrips_with_compression() {
        let mut a = Connection::new(Framing::Unprefixed, ConnectionOptions::default());
        let mut b = Connection::new(Framing::Unprefixed, ConnectionOptions::default());
        a.enable_compression(BatchCompression::Snappy, 1, 0)
            .unwrap();
        b.enable_compression(BatchCompression::Snappy, 1, 0)
            .unwrap();
        let text = McpePacket::from(TextPacket::default());
        a.queue_packet(&text, Instant::now()).unwrap();
        let transmit = a.poll_transmit().unwrap();
        assert_eq!(transmit.payload[0], BatchCompression::Snappy as u8);
        b.handle_datagram(Datagram::Contiguous(transmit.payload))
            .unwrap();
        assert!(
            b.next_packet()
                .unwrap()
                .unwrap()
                .is(McpePacketName::TextPacket)
        );
    }
}
