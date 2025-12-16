//! Unconnected (offline) RakNet discovery and ping packets.

use bytes::{Buf, BufMut};

use crate::protocol::{
    constants::DEFAULT_UNCONNECTED_MAGIC,
    packet::{Packet, RaknetEncodable},
    types::{Advertisement, Magic, RaknetTime},
};

/// Unconnected ping used by clients to discover RakNet servers.
#[derive(Debug, Clone)]
pub struct UnconnectedPing {
    pub ping_time: RaknetTime,
    pub magic: Magic,
}

impl Packet for UnconnectedPing {
    const ID: u8 = 0x01;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.ping_time.encode_raknet(dst)?;

        self.magic.encode_raknet(dst)?;
        Ok(())
    }

    fn decode_body(src: &mut impl Buf) -> Result<Self, super::DecodeError> {
        let ping_time = RaknetTime::decode_raknet(src)?;
        let magic = Magic::decode_raknet(src)?;
        if magic != DEFAULT_UNCONNECTED_MAGIC {
            return Err(super::DecodeError::InvalidMagic);
        }
        Ok(Self { ping_time, magic })
    }
}

/// Unconnected pong sent by servers in response to `UnconnectedPing`.
#[derive(Debug, Clone)]
pub struct UnconnectedPong {
    pub ping_time: RaknetTime,
    pub server_guid: u64,
    pub magic: Magic,
    pub advertisement: Advertisement,
}

impl Packet for UnconnectedPong {
    const ID: u8 = 0x1c;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.ping_time.encode_raknet(dst)?;
        self.server_guid.encode_raknet(dst)?;
        self.magic.encode_raknet(dst)?;
        self.advertisement.encode_raknet(dst)?;
        Ok(())
    }

    fn decode_body(src: &mut impl Buf) -> Result<Self, super::DecodeError> {
        let ping_time = RaknetTime::decode_raknet(src)?;
        let server_guid = u64::decode_raknet(src)?;
        let magic = Magic::decode_raknet(src)?;
        if magic != DEFAULT_UNCONNECTED_MAGIC {
            return Err(super::DecodeError::InvalidMagic);
        }
        let advertisement = Advertisement::decode_raknet(src)?;
        Ok(Self {
            ping_time,
            server_guid,
            magic,
            advertisement,
        })
    }
}

/// Advertise system packet (legacy/alternative to UnconnectedPong).
#[derive(Debug, Clone)]
pub struct AdvertiseSystem {
    pub ping_time: RaknetTime,
    pub server_guid: u64,
    pub magic: Magic,
    pub advertisement: Advertisement,
}

impl Packet for AdvertiseSystem {
    const ID: u8 = 0x1d;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.ping_time.encode_raknet(dst)?;
        self.server_guid.encode_raknet(dst)?;
        self.magic.encode_raknet(dst)?;
        self.advertisement.encode_raknet(dst)?;
        Ok(())
    }

    fn decode_body(src: &mut impl Buf) -> Result<Self, super::DecodeError> {
        let ping_time = RaknetTime::decode_raknet(src)?;
        let server_guid = u64::decode_raknet(src)?;
        let magic = Magic::decode_raknet(src)?;
        if magic != DEFAULT_UNCONNECTED_MAGIC {
            return Err(super::DecodeError::InvalidMagic);
        }
        let advertisement = Advertisement::decode_raknet(src)?;
        Ok(Self {
            ping_time,
            server_guid,
            magic,
            advertisement,
        })
    }
}

/// Unconnected ping for open connections.
/// Same structure as UnconnectedPing but different ID.
#[derive(Debug, Clone)]
pub struct UnconnectedPingOpenConnections {
    pub ping_time: RaknetTime,
    pub magic: Magic,
}

impl Packet for UnconnectedPingOpenConnections {
    const ID: u8 = 0x02;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.ping_time.encode_raknet(dst)?;
        self.magic.encode_raknet(dst)?;
        Ok(())
    }

    fn decode_body(src: &mut impl Buf) -> Result<Self, super::DecodeError> {
        let ping_time = RaknetTime::decode_raknet(src)?;
        let magic = Magic::decode_raknet(src)?;
        if magic != DEFAULT_UNCONNECTED_MAGIC {
            return Err(super::DecodeError::InvalidMagic);
        }
        Ok(Self { ping_time, magic })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn unconnected_ping_roundtrip() {
        let pkt = UnconnectedPing {
            ping_time: RaknetTime(123),
            magic: DEFAULT_UNCONNECTED_MAGIC,
        };
        let mut buf = BytesMut::new();
        pkt.encode_body(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = UnconnectedPing::decode_body(&mut slice).unwrap();
        assert_eq!(decoded.ping_time.0, pkt.ping_time.0);
        assert_eq!(decoded.magic, pkt.magic);
    }

    #[test]
    fn unconnected_pong_roundtrip() {
        let pkt = UnconnectedPong {
            ping_time: RaknetTime(1),
            server_guid: 2,
            magic: DEFAULT_UNCONNECTED_MAGIC,
            advertisement: Advertisement(None),
        };
        let mut buf = BytesMut::new();
        pkt.encode_body(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = UnconnectedPong::decode_body(&mut slice).unwrap();
        assert_eq!(decoded.ping_time.0, pkt.ping_time.0);
        assert_eq!(decoded.server_guid, pkt.server_guid);
        assert_eq!(decoded.magic, pkt.magic);
    }
}
