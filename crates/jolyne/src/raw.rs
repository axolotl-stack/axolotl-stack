//! Tier 3, the async low-level surface (see `docs/API.md`): packets with
//! only their header parsed, borrowed decoding, custom packet IDs and
//! per-packet instrumentation, for hot loops, proxies and protocol tools.
//!
//! A joined [`crate::BedrockStream`] already offers `recv_packet_raw`,
//! `recv_packet_borrowed`, `send_packet_raw` and `send_batch[_raw]`;
//! [`BedrockTransport`] is the same connection outside the join typestate
//! (`BedrockStream::into_transport`). For the sans-IO core, see
//! [`crate::proto`].

pub use crate::proto::custom::{CustomPacket, MAX_PACKET_ID, UnknownPacketPolicy};
pub use crate::proto::observe::{ConnectionMetrics, Direction, PacketEvent, PacketObserver};
pub use crate::proto::raw::{PacketHeader, RawPacket};
pub use crate::stream::BedrockTransport;
pub use crate::valentine::BorrowedMcpePacket;
