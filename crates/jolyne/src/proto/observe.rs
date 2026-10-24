//! Packet observation and connection counters.
//!
//! Both are near zero-cost when unused: the observer is an `Option` checked
//! once per inbound batch and once per outbound packet, and counters are
//! plain integers bumped once per batch, except on the error and
//! unknown-packet paths.
//!
//! Independently of the observer, every batch and packet is logged at
//! `TRACE` under the [`WIRE_TARGET`] target (`RUST_LOG=jolyne::wire=trace`)
//! for interop debugging against real clients.

use std::fmt;
use std::sync::Arc;

use super::raw::PacketHeader;

/// `tracing` target of the per-batch and per-packet wire log.
pub const WIRE_TARGET: &str = "jolyne::wire";

/// Whether the wire log is enabled; checked before walking a batch.
#[inline]
pub(crate) fn wire_trace_enabled() -> bool {
    tracing::enabled!(target: WIRE_TARGET, tracing::Level::TRACE)
}

/// Logs one packet of the wire trace.
pub(crate) fn trace_packet(direction: Direction, header: PacketHeader, payload: &[u8]) {
    tracing::trace!(
        target: WIRE_TARGET,
        ?direction,
        id = header.id,
        name = ?super::custom::packet_name(header.id),
        len = payload.len(),
        payload = %HexPreview(payload),
        "packet"
    );
}

/// Lowercase hex of the first [`HexPreview::LIMIT`] bytes, then the count
/// of omitted bytes.
pub(crate) struct HexPreview<'a>(pub &'a [u8]);

impl HexPreview<'_> {
    const LIMIT: usize = 256;
}

impl fmt::Display for HexPreview<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let shown = &self.0[..self.0.len().min(Self::LIMIT)];
        for byte in shown {
            write!(f, "{byte:02x}")?;
        }
        if self.0.len() > shown.len() {
            write!(f, "…(+{} bytes)", self.0.len() - shown.len())?;
        }
        Ok(())
    }
}

/// Which way a packet travelled, from this endpoint's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Received from the peer.
    Inbound,
    /// Sent to the peer.
    Outbound,
}

/// One observed packet.
#[derive(Debug, Clone, Copy)]
pub struct PacketEvent<'a> {
    /// Whether the packet was received or sent.
    pub direction: Direction,
    /// The packet's ten-bit ID and sub-client routing bits.
    pub header: PacketHeader,
    /// The packet body (after the header varint), before compression and
    /// encryption.
    pub payload: &'a [u8],
}

/// Read-only packet hook (gophertunnel `PacketFunc`).
///
/// Called synchronously from the codec for every inbound packet that survives
/// the unknown-packet policy and every outbound packet as it is framed.
pub trait PacketObserver: Send + Sync {
    /// Inspects one packet; must not block, as it runs inline in the codec.
    fn observe(&self, event: PacketEvent<'_>);
}

impl<F: Fn(PacketEvent<'_>) + Send + Sync> PacketObserver for F {
    fn observe(&self, event: PacketEvent<'_>) {
        self(event)
    }
}

/// Shared observer handle stored in connection options.
pub type Observer = Arc<dyn PacketObserver>;

/// Lightweight per-connection counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConnectionMetrics {
    /// Game packets decoded from inbound batches.
    pub packets_in: u64,
    /// Game packets framed into outbound batches.
    pub packets_out: u64,
    /// Wire bytes (after encryption/compression).
    pub bytes_in: u64,
    /// Wire bytes of outbound batches (after compression/encryption).
    pub bytes_out: u64,
    /// Inbound batches (transport messages) received.
    pub batches_in: u64,
    /// Outbound batches (transport messages) queued for sending.
    pub batches_out: u64,
    /// Batches or packets that failed to decode.
    pub decode_errors: u64,
    /// Unknown packet IDs seen (skipped or passed through).
    pub unknown_packets: u64,
}
