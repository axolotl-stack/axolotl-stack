use bytes::{Buf, BufMut};

use crate::protocol::{
    ack::AckNackPayload,
    constants::{DatagramFlags, RAKNET_DATAGRAM_HEADER_SIZE},
    encapsulated_packet::EncapsulatedPacket,
    packet::{DecodeError, EncodeError, RaknetEncodable},
    types::{DatagramHeader, Sequence24},
};

/// The payload of a datagram.
///
/// A datagram can either be a "Data" datagram (0x80-0x8F) which contains
/// a set of encapsulated packets, or it can be a lightweight, unencapsulated
/// ACK (0xC0) or NACK (0xA0) datagram.
#[derive(Debug, Clone)]
pub enum DatagramPayload {
    EncapsulatedPackets(Vec<EncapsulatedPacket>),
    Nak(AckNackPayload),
    Ack(AckNackPayload),
}

/// Represents a single, raw RakNet UDP datagram.
#[derive(Debug, Clone)]
pub struct Datagram {
    pub header: DatagramHeader,
    /// The payload of the datagram, which varies based on the header flags.
    pub payload: DatagramPayload,
}

impl Datagram {
    /// Encodes the datagram (header + payload) into the destination buffer.
    pub fn encode(&self, dst: &mut impl BufMut) -> Result<(), EncodeError> {
        match &self.payload {
            DatagramPayload::EncapsulatedPackets(packets) => {
                self.header.encode_raknet(dst)?;
                for pkt in packets {
                    pkt.encode_raknet(dst)?;
                }
            }
            DatagramPayload::Nak(payload) | DatagramPayload::Ack(payload) => {
                dst.put_u8(self.header.flags.bits());
                payload.encode_raknet(dst)?;
            }
        }
        Ok(())
    }

    /// Decodes a datagram from the source buffer.
    ///
    /// This function reads the header flags to determine how to
    /// parse the rest of the buffer.
    pub fn decode(src: &mut impl Buf) -> Result<Self, DecodeError> {
        if !src.has_remaining() {
            return Err(DecodeError::UnexpectedEof);
        }

        let raw_flags = src.get_u8();
        let flags = DatagramFlags::from_bits_truncate(raw_flags);

        if flags.contains(DatagramFlags::ACK) {
            let header = DatagramHeader {
                flags,
                sequence: Sequence24::new(0),
            };
            return Ok(Self {
                header,
                payload: DatagramPayload::Ack(AckNackPayload::decode_raknet(src)?),
            });
        }

        if flags.contains(DatagramFlags::NACK) {
            let header = DatagramHeader {
                flags,
                sequence: Sequence24::new(0),
            };
            return Ok(Self {
                header,
                payload: DatagramPayload::Nak(AckNackPayload::decode_raknet(src)?),
            });
        }

        let sequence = Sequence24::decode_raknet(src)?;
        let header = DatagramHeader { flags, sequence };

        let mut packets = Vec::with_capacity(1);
        while src.has_remaining() {
            packets.push(EncapsulatedPacket::decode_raknet(src)?);
        }
        Ok(Self {
            header,
            payload: DatagramPayload::EncapsulatedPackets(packets),
        })
    }

    pub fn size(&self) -> usize {
        match &self.payload {
            DatagramPayload::EncapsulatedPackets(encapsulated_packets) => {
                let mut size = RAKNET_DATAGRAM_HEADER_SIZE;
                for pkt in encapsulated_packets {
                    size += pkt.size();
                }
                size
            }
            DatagramPayload::Nak(payload) | DatagramPayload::Ack(payload) => 1 + payload.size(),
        }
    }
}
