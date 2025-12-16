//! Shared protocol-level constants and flags for RakNet.
//!
//! These values mirror the behaviour of the reference RakNet implementation
//! and should be treated as part of the wire-level contract.

use bitflags::bitflags;
use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6},
    time::Duration,
};

use crate::protocol::types::Magic;

// === Protocol / version ===

/// RakNet protocol version used by this implementation (Mojang's version).
pub const RAKNET_PROTOCOL_VERSION: u8 = 11;

// === MTU and framing sizes ===

/// Minimum supported MTU as used during negotiation.
pub const MINIMUM_MTU_SIZE: u16 = 576;
/// Maximum supported MTU as used during negotiation.
pub const MAXIMUM_MTU_SIZE: u16 = 1400;
/// Candidate MTU sizes to probe, from most used by client.
pub const MTU_SIZES: &[u16] = &[1200, MAXIMUM_MTU_SIZE, MINIMUM_MTU_SIZE];

const _: () = {
    assert!(
        MINIMUM_MTU_SIZE < MAXIMUM_MTU_SIZE,
        "MINIMUM_MTU_SIZE must be less than MAXIMUM_MTU_SIZE"
    );
};

/// Maximum amount of ordering channels as defined in vanilla RakNet.
pub const MAXIMUM_ORDERING_CHANNELS: u8 = 16;

/// Maximum size of an encapsulated packet header.
pub const MAXIMUM_ENCAPSULATED_HEADER_SIZE: usize = 28;

pub const MAX_ACK_SEQUENCES: u16 = 8192;

/// Size of a UDP header on the wire.
pub const UDP_HEADER_SIZE: usize = 8;
/// Size of an IPv4 header without options. We pessimistically subtract this
/// when packing datagrams so the full IP packet stays within the negotiated MTU
/// even if the MTU was measured at the link layer.
pub const IPV4_HEADER_SIZE: usize = 20;

pub const RAKNET_DATAGRAM_HEADER_SIZE: usize = 4;

// === Connection / session timing ===

/// Maximum number of connection attempts before giving up.
pub const MAXIMUM_CONNECTION_ATTEMPTS: usize = 10;

/// Time between sending connection attempts.
pub const TIME_BETWEEN_SEND_CONNECTION_ATTEMPTS: Duration = Duration::from_millis(1000);

/// Time after which a session is closed due to no activity.
pub const SESSION_TIMEOUT: Duration = Duration::from_millis(10000);

/// Time after which a session is considered stale due to no activity.
pub const SESSION_STALE: Duration = Duration::from_millis(5000);

// === Packet limits / congestion ===

/// Maximum number of datagram packets each address can send within one RakNet tick (10ms).
pub const DEFAULT_PACKET_LIMIT: usize = 120;

/// Maximum number of datagrams handled within one tick before dropping excess data.
pub const DEFAULT_GLOBAL_PACKET_LIMIT: usize = 100000;

bitflags! {
    /// Flags for the main RakNet UDP Datagram.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(transparent)]
    pub struct DatagramFlags: u8 {
        const VALID          = 0b1000_0000;
        const ACK            = 0b0100_0000;
        const NACK           = 0b0010_0000;
        /// Datagram is part of a packet pair (used for bandwidth probing).
        const PACKET_PAIR    = 0b0001_0000;
        /// Sender is going to transmit additional datagrams immediately after this one.
        const CONTINUOUS_SEND = 0b0000_1000;
        /// Datagram requests/needs B&AS (bandwidth and smoothness) info.
        const HAS_B_AND_AS   = 0b0000_0100;
    }
}

// Constants for the Frame (Encapsulated Packet) header
pub const FRAME_FLAG_SPLIT: u8 = 0b0001_0000;
pub const FRAME_FLAG_NEEDS_BAS: u8 = 0b0000_0100;

// === Magic and discovery ===

/// Magic used to identify unconnected RakNet packets.
pub const DEFAULT_UNCONNECTED_MAGIC: Magic = [
    0x00, 0xFF, 0xFF, 0x00, 0xFE, 0xFE, 0xFE, 0xFE, 0xFD, 0xFD, 0xFD, 0xFD, 0x12, 0x34, 0x56, 0x78,
];

/// Congestion control maximum threshold.
pub const CC_MAXIMUM_THRESHOLD: usize = 2000;
/// Congestion control additional variance.
pub const CC_ADDITIONAL_VARIANCE: usize = 30;
/// Congestion control SYN value.
pub const CC_SYN: usize = 10;

// === IP / address helpers ===

/// Size of an IPv4 address payload in RakNet messages.
pub const IPV4_MESSAGE_SIZE: usize = 7;
/// Size of an IPv6 address payload in RakNet messages.
pub const IPV6_MESSAGE_SIZE: usize = 29;

pub const LOOPBACK_V4: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
pub const ANY_V4: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);

/// Default IPv4 system-address list, mirroring the reference implementation.
pub const LOCAL_IP_ADDRESSES_V4: [SocketAddrV4; 10] = [
    LOOPBACK_V4,
    ANY_V4,
    ANY_V4,
    ANY_V4,
    ANY_V4,
    ANY_V4,
    ANY_V4,
    ANY_V4,
    ANY_V4,
    ANY_V4,
];

pub const LOOPBACK_V6: SocketAddrV6 = SocketAddrV6::new(Ipv6Addr::LOCALHOST, 0, 0, 0);
pub const ANY_V6: SocketAddrV6 = SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0);

/// Default IPv6 system-address list, mirroring the reference implementation.
pub const LOCAL_IP_ADDRESSES_V6: [SocketAddrV6; 10] = [
    LOOPBACK_V6,
    ANY_V6,
    ANY_V6,
    ANY_V6,
    ANY_V6,
    ANY_V6,
    ANY_V6,
    ANY_V6,
    ANY_V6,
    ANY_V6,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_version_matches_expected() {
        assert_eq!(RAKNET_PROTOCOL_VERSION, 11);
    }

    #[test]
    fn mtu_bounds_are_consistent() {
        assert!(MTU_SIZES.contains(&MAXIMUM_MTU_SIZE));
        assert!(MTU_SIZES.contains(&MINIMUM_MTU_SIZE));
    }
}
