//! Reliability-related protocol types.
//!
//! This module is currently a placeholder; reliability logic lives under
//! `reliability` in the Go reference implementation and will be ported
//! here as Rust types become necessary.

use crate::protocol::packet::{DecodeError, EncodeError, RaknetEncodable};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Reliability {
    Unreliable = 0,
    UnreliableSequenced = 1,
    Reliable = 2,
    ReliableOrdered = 3,
    ReliableSequenced = 4,
    UnreliableWithAckReceipt = 5,
    ReliableWithAckReceipt = 6,
    ReliableOrderedWithAckReceipt = 7,
}

/// Holds the properties for a given Reliability.
#[derive(Debug, Clone, Copy)]
struct ReliabilityProperties {
    is_reliable: bool,
    is_ordered: bool,
    is_sequenced: bool,
    is_with_ack_receipt: bool,
    header_size: usize,
}

const RELIABILITY_TABLE: [ReliabilityProperties; 8] = [
    // Unreliable
    ReliabilityProperties {
        is_reliable: false,
        is_ordered: false,
        is_sequenced: false,
        is_with_ack_receipt: false,
        header_size: 0,
    },
    // UnreliableSequenced
    ReliabilityProperties {
        is_reliable: false,
        is_ordered: false,
        is_sequenced: true,
        is_with_ack_receipt: false,
        header_size: 3, // sequenceIndex (3)
    },
    // Reliable
    ReliabilityProperties {
        is_reliable: true,
        is_ordered: false,
        is_sequenced: false,
        is_with_ack_receipt: false,
        header_size: 3, // reliableIndex (3)
    },
    // ReliableOrdered
    ReliabilityProperties {
        is_reliable: true,
        is_ordered: true,
        is_sequenced: false,
        is_with_ack_receipt: false,
        header_size: 7, // reliableIndex (3) + orderingIndex (3) + orderingChannel (1)
    },
    // ReliableSequenced
    ReliabilityProperties {
        is_reliable: true,
        is_ordered: false,
        is_sequenced: true,
        is_with_ack_receipt: false,
        header_size: 6, // reliableIndex (3) + sequenceIndex (3)
    },
    // UnreliableWithAckReceipt
    ReliabilityProperties {
        is_reliable: false,
        is_ordered: false,
        is_sequenced: false,
        is_with_ack_receipt: true,
        header_size: 0,
    },
    // ReliableWithAckReceipt
    ReliabilityProperties {
        is_reliable: true,
        is_ordered: false,
        is_sequenced: false,
        is_with_ack_receipt: true,
        header_size: 3, // reliableIndex (3)
    },
    // ReliableOrderedWithAckReceipt
    ReliabilityProperties {
        is_reliable: true,
        is_ordered: true,
        is_sequenced: false,
        is_with_ack_receipt: true,
        header_size: 7, // reliableIndex (3) + orderingIndex (3) + orderingChannel (1)
    },
];

impl Reliability {
    /// Gets the associated properties for this reliability from the lookup table.
    #[inline]
    fn properties(self) -> &'static ReliabilityProperties {
        &RELIABILITY_TABLE[self as usize]
    }

    #[inline]
    pub fn is_reliable(self) -> bool {
        self.properties().is_reliable
    }

    #[inline]
    pub fn is_ordered(self) -> bool {
        self.properties().is_ordered
    }

    #[inline]
    pub fn is_sequenced(self) -> bool {
        self.properties().is_sequenced
    }

    #[inline]
    pub fn is_with_ack_receipt(self) -> bool {
        self.properties().is_with_ack_receipt
    }

    /// Gets the size of the required header fields for this reliability.
    #[inline]
    pub fn header_size(self) -> usize {
        self.properties().header_size
    }
}

impl TryFrom<u8> for Reliability {
    type Error = DecodeError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Reliability::Unreliable),
            1 => Ok(Reliability::UnreliableSequenced),
            2 => Ok(Reliability::Reliable),
            3 => Ok(Reliability::ReliableOrdered),
            4 => Ok(Reliability::ReliableSequenced),
            5 => Ok(Reliability::UnreliableWithAckReceipt),
            6 => Ok(Reliability::ReliableWithAckReceipt),
            7 => Ok(Reliability::ReliableOrderedWithAckReceipt),
            _ => Err(DecodeError::UnknownReliability(value)),
        }
    }
}

impl RaknetEncodable for Reliability {
    fn encode_raknet(&self, dst: &mut impl bytes::BufMut) -> Result<(), EncodeError> {
        (*self as u8).encode_raknet(dst)
    }

    fn decode_raknet(src: &mut impl bytes::Buf) -> Result<Self, DecodeError> {
        Reliability::try_from(u8::decode_raknet(src)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reliable_ordered_header_size() {
        assert_eq!(Reliability::ReliableOrdered.header_size(), 7);
    }

    #[test]
    fn roundtrip_encode_decode() {
        for r in [
            Reliability::Unreliable,
            Reliability::Reliable,
            Reliability::ReliableOrderedWithAckReceipt,
        ] {
            let mut buf = bytes::BytesMut::new();
            r.encode_raknet(&mut buf).unwrap();
            let mut slice = buf.freeze();
            let decoded = Reliability::decode_raknet(&mut slice).unwrap();
            assert_eq!(decoded, r);
        }
    }
}
