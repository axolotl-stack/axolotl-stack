//! Online (connected) RakNet control packets.
//!
//! These packets are only used once a session is established.

use bytes::{Buf, BufMut, Bytes};

use crate::protocol::{
    ack::AckNackPayload,
    packet::{Packet, RaknetEncodable},
    state::DisconnectReason,
    types::RaknetTime,
};

/// Ping sent over an established connection to measure round-trip time.
#[derive(Debug, Clone)]
pub struct ConnectedPing {
    pub ping_time: RaknetTime,
}

impl Packet for ConnectedPing {
    const ID: u8 = 0x00;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.ping_time.encode_raknet(dst)
    }

    fn decode_body(src: &mut impl Buf) -> Result<Self, super::DecodeError> {
        Ok(Self {
            ping_time: RaknetTime::decode_raknet(src)?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ConnectedPong {
    pub ping_time: RaknetTime,
    pub pong_time: RaknetTime,
}

impl Packet for ConnectedPong {
    const ID: u8 = 0x03;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.ping_time.encode_raknet(dst)?;
        self.pong_time.encode_raknet(dst)?;
        Ok(())
    }

    fn decode_body(src: &mut impl Buf) -> Result<Self, super::DecodeError> {
        Ok(Self {
            ping_time: RaknetTime::decode_raknet(src)?,
            pong_time: RaknetTime::decode_raknet(src)?,
        })
    }
}

/// Notification that the connection is being closed, with an optional reason.
#[derive(Debug, Clone)]
pub struct DisconnectionNotification {
    pub reason: DisconnectReason,
}

impl Packet for DisconnectionNotification {
    const ID: u8 = 0x15;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.reason.encode_raknet(dst)
    }

    fn decode_body(src: &mut impl Buf) -> Result<Self, super::DecodeError> {
        if !src.has_remaining() {
            Ok(Self {
                reason: DisconnectReason::ClosedByRemotePeer,
            })
        } else {
            Ok(Self {
                reason: DisconnectReason::decode_raknet(src)?,
            })
        }
    }
}

/// ID-only marker used to detect lost connections.
#[derive(Debug, Clone)]
pub struct DetectLostConnection;

impl Packet for DetectLostConnection {
    const ID: u8 = 0x04;

    // Its an ID only marker.
    // so just noop basically.

    fn encode_body(
        &self,
        _dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        Ok(())
    }

    fn decode_body(_src: &mut impl Buf) -> Result<Self, super::DecodeError> {
        Ok(Self {})
    }
}

/// ID-only marker indicating that the server has no free incoming slots.
#[derive(Debug, Clone)]
pub struct NoFreeIncomingConnections;

impl Packet for NoFreeIncomingConnections {
    const ID: u8 = 0x14;

    // Its an ID only marker.
    // so just noop basically.

    fn encode_body(
        &self,
        _dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        Ok(())
    }

    fn decode_body(_src: &mut impl Buf) -> Result<Self, super::DecodeError> {
        Ok(Self {})
    }
}

/// Legacy packet representing a connection loss with an opaque payload.
#[derive(Debug, Clone)]
pub struct ConnectionLost {
    pub payload: Bytes,
}

impl Packet for ConnectionLost {
    const ID: u8 = 0x16;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        dst.put_slice(&self.payload);
        Ok(())
    }

    fn decode_body(src: &mut impl Buf) -> Result<Self, super::DecodeError> {
        let remaining = src.remaining();
        let payload = src.copy_to_bytes(remaining);

        Err(super::DecodeError::UnimplementedPacket {
            id: Self::ID,
            payload,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionBanned {
    pub payload: Bytes,
}

impl Packet for ConnectionBanned {
    const ID: u8 = 0x17;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        dst.put_slice(&self.payload);
        Ok(())
    }

    fn decode_body(src: &mut impl Buf) -> Result<Self, super::DecodeError> {
        let remaining = src.remaining();
        let payload = src.copy_to_bytes(remaining);

        Err(super::DecodeError::UnimplementedPacket {
            id: Self::ID,
            payload,
        })
    }
}

/// ID-only marker indicating that this IP has recently connected.
#[derive(Debug, Clone)]
pub struct IpRecentlyConnected;

impl Packet for IpRecentlyConnected {
    const ID: u8 = 0x1a;

    // Its an ID only marker.
    // so just noop basically.

    fn encode_body(
        &self,
        _dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        Ok(())
    }

    fn decode_body(_src: &mut impl Buf) -> Result<Self, super::DecodeError> {
        Ok(Self {})
    }
}

/// Legacy timestamp packet with an opaque payload.
#[derive(Debug, Clone)]
pub struct Timestamp {
    payload: Bytes,
}

impl Packet for Timestamp {
    const ID: u8 = 0x1b;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        dst.put_slice(&self.payload);
        Ok(())
    }

    fn decode_body(src: &mut impl Buf) -> Result<Self, super::DecodeError> {
        let remaining = src.remaining();
        let payload = src.copy_to_bytes(remaining);

        Err(super::DecodeError::UnimplementedPacket {
            id: Self::ID,
            payload,
        })
    }
}

#[derive(Debug, Clone)]
pub struct EncapsulatedNak(pub AckNackPayload);

impl Packet for EncapsulatedNak {
    const ID: u8 = 0xa0;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.0.encode_raknet(dst)
    }

    fn decode_body(src: &mut impl Buf) -> Result<Self, super::DecodeError> {
        Ok(Self(AckNackPayload::decode_raknet(src)?))
    }
}

#[derive(Debug, Clone)]
pub struct EncapsulatedAck(pub AckNackPayload);

impl Packet for EncapsulatedAck {
    const ID: u8 = 0xc0;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.0.encode_raknet(dst)
    }

    fn decode_body(src: &mut impl Buf) -> Result<Self, super::DecodeError> {
        Ok(Self(AckNackPayload::decode_raknet(src)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn connected_ping_roundtrip() {
        let pkt = ConnectedPing {
            ping_time: RaknetTime(999),
        };
        let mut buf = BytesMut::new();
        pkt.encode_body(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = ConnectedPing::decode_body(&mut slice).unwrap();
        assert_eq!(decoded.ping_time.0, pkt.ping_time.0);
    }
}
