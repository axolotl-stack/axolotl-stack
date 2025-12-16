//! Low-level on-the-wire primitives and helpers for the RakNet protocol.
//!
//! This module defines basic integer formats, varints, time and address
//! encodings, as well as small helper types used by control packets.

mod addr;
mod advertisement;
mod datagram_header;
mod encapsulated_packet_header;
mod ints;
mod padding;
mod primitives;
mod sequence;
mod time;
mod varint;

pub use advertisement::Advertisement;
pub use datagram_header::DatagramHeader;
pub use encapsulated_packet_header::EncapsulatedPacketHeader;
pub use ints::{U16LE, U24LE};
pub use padding::EoBPadding;
pub use sequence::Sequence24;
pub use time::{RaknetTime, raknet_start_time};
pub use varint::{VarInt, VarUInt};

/// Magic used to identify RakNet packets.
pub type Magic = [u8; 16];
