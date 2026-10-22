//! Packet observation and connection counters.
//!
//! Both are zero-cost when unused: the observer is an `Option` checked once
//! per batch, and counters are plain integers bumped once per batch (never per
//! packet), except on the error and unknown-packet paths.

use std::sync::Arc;

use super::raw::PacketHeader;

/// Which way a packet travelled, from this endpoint's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Inbound,
    Outbound,
}

/// One observed packet.
#[derive(Debug, Clone, Copy)]
pub struct PacketEvent<'a> {
    pub direction: Direction,
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
    pub packets_in: u64,
    pub packets_out: u64,
    /// Wire bytes (after encryption/compression).
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub batches_in: u64,
    pub batches_out: u64,
    /// Batches or packets that failed to decode.
    pub decode_errors: u64,
    /// Unknown packet IDs seen (skipped or passed through).
    pub unknown_packets: u64,
}
