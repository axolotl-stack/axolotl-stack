//! Offline connection negotiation and handshake packets.
//!
//! These packets are exchanged before a fully connected session exists.

use std::net::SocketAddr;

use bytes::{BufMut, Bytes};

use crate::protocol::{
    constants::{self, DEFAULT_UNCONNECTED_MAGIC},
    packet::{Packet, RaknetEncodable},
    types::{EoBPadding, Magic, RaknetTime},
};

/// Client's initial connection request containing protocol and MTU probe.
#[derive(Debug, Clone)]
pub struct OpenConnectionRequest1 {
    pub magic: Magic,
    pub protocol_version: u8,
    pub padding: EoBPadding,
}

impl Packet for OpenConnectionRequest1 {
    const ID: u8 = 0x05;

    fn encode_body(
        &self,
        dst: &mut impl bytes::BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.magic.encode_raknet(dst)?;
        self.protocol_version.encode_raknet(dst)?;
        self.padding.encode_raknet(dst)?;
        Ok(())
    }

    fn decode_body(src: &mut impl bytes::Buf) -> Result<Self, super::DecodeError> {
        let magic = Magic::decode_raknet(src)?;
        if magic != DEFAULT_UNCONNECTED_MAGIC {
            return Err(super::DecodeError::InvalidMagic);
        }
        Ok(Self {
            magic,
            protocol_version: u8::decode_raknet(src)?,
            padding: EoBPadding::decode_raknet(src)?,
        })
    }
}

/// Server's reply with its GUID and selected MTU (and optional security cookie).
#[derive(Debug, Clone)]
pub struct OpenConnectionReply1 {
    pub magic: Magic,
    pub server_guid: u64,
    pub cookie: Option<u32>,
    pub mtu: u16,
}

impl Packet for OpenConnectionReply1 {
    const ID: u8 = 0x06;

    fn encode_body(
        &self,
        dst: &mut impl bytes::BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.magic.encode_raknet(dst)?;
        self.server_guid.encode_raknet(dst)?;
        self.cookie.is_some().encode_raknet(dst)?; // security bool
        if self.cookie.is_some() {
            self.cookie.unwrap().encode_raknet(dst)?;
        }
        self.mtu.encode_raknet(dst)?;
        Ok(())
    }

    fn decode_body(src: &mut impl bytes::Buf) -> Result<Self, super::DecodeError> {
        let magic = Magic::decode_raknet(src)?;
        if magic != DEFAULT_UNCONNECTED_MAGIC {
            return Err(super::DecodeError::InvalidMagic);
        }
        Ok(Self {
            magic,
            server_guid: u64::decode_raknet(src)?,
            cookie: if bool::decode_raknet(src)? {
                Some(u32::decode_raknet(src)?)
            } else {
                None
            },
            mtu: u16::decode_raknet(src)?,
        })
    }
}

/// Second stage connection request including target address, MTU and GUID.
#[derive(Debug, Clone)]
pub struct OpenConnectionRequest2 {
    pub magic: Magic,
    pub cookie: Option<u32>,
    pub client_proof: bool,
    pub server_addr: SocketAddr,
    pub mtu: u16,
    pub client_guid: u64,
}

impl Packet for OpenConnectionRequest2 {
    const ID: u8 = 0x07;

    fn encode_body(
        &self,
        dst: &mut impl bytes::BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.magic.encode_raknet(dst)?;
        if let Some(cookie) = self.cookie {
            cookie.encode_raknet(dst)?;
            self.client_proof.encode_raknet(dst)?;
        }
        self.server_addr.encode_raknet(dst)?;
        self.mtu.encode_raknet(dst)?;
        self.client_guid.encode_raknet(dst)?;
        Ok(())
    }

    fn decode_body(src: &mut impl bytes::Buf) -> Result<Self, super::DecodeError> {
        let magic = Magic::decode_raknet(src)?;
        if magic != DEFAULT_UNCONNECTED_MAGIC {
            return Err(super::DecodeError::InvalidMagic);
        }
        let rest: Bytes = src.copy_to_bytes(src.remaining());
        let mut cursor = &rest[..];

        let mut cookie = None;
        if let Some(&first) = cursor.first() {
            if first != 4 && first != 6 {
                if cursor.len() < 4 {
                    return Err(super::DecodeError::UnexpectedEof);
                }
                cookie = Some(u32::from_be_bytes([
                    cursor[0], cursor[1], cursor[2], cursor[3],
                ]));
                cursor = &cursor[4..];
                if !cursor.is_empty() {
                    cursor = &cursor[1..]; // skip reserved/client proof byte
                }
            }
        } else {
            return Err(super::DecodeError::UnexpectedEof);
        }

        let server_addr = SocketAddr::decode_raknet(&mut cursor)?;
        let mtu = u16::decode_raknet(&mut cursor)?;
        let client_guid = u64::decode_raknet(&mut cursor)?;
        Ok(Self {
            magic,
            cookie,
            client_proof: false,
            server_addr,
            mtu,
            client_guid,
        })
    }
}

/// Second stage server reply that finalises connection parameters.
#[derive(Debug, Clone)]
pub struct OpenConnectionReply2 {
    pub magic: Magic,
    pub server_guid: u64,
    pub server_addr: SocketAddr,
    pub mtu: u16,
    pub security: bool,
}

impl Packet for OpenConnectionReply2 {
    const ID: u8 = 0x08;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.magic.encode_raknet(dst)?;
        self.server_guid.encode_raknet(dst)?;
        self.server_addr.encode_raknet(dst)?;
        self.mtu.encode_raknet(dst)?;
        self.security.encode_raknet(dst)?;
        Ok(())
    }

    fn decode_body(src: &mut impl bytes::Buf) -> Result<Self, super::DecodeError> {
        let magic = Magic::decode_raknet(src)?;
        if magic != DEFAULT_UNCONNECTED_MAGIC {
            return Err(super::DecodeError::InvalidMagic);
        }
        Ok(Self {
            magic,
            server_guid: u64::decode_raknet(src)?,
            server_addr: SocketAddr::decode_raknet(src)?,
            mtu: u16::decode_raknet(src)?,
            security: bool::decode_raknet(src)?,
        })
    }
}

/// Sent when the client and server disagree on the protocol version.
#[derive(Debug, Clone)]
pub struct IncompatibleProtocolVersion {
    pub protocol: u8,
    pub magic: Magic,
    pub server_guid: u64,
}

impl Packet for IncompatibleProtocolVersion {
    const ID: u8 = 0x19;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.protocol.encode_raknet(dst)?;
        self.magic.encode_raknet(dst)?;
        self.server_guid.encode_raknet(dst)?;
        Ok(())
    }

    fn decode_body(src: &mut impl bytes::Buf) -> Result<Self, super::DecodeError> {
        let protocol = u8::decode_raknet(src)?;
        let magic = Magic::decode_raknet(src)?;
        if magic != DEFAULT_UNCONNECTED_MAGIC {
            return Err(super::DecodeError::InvalidMagic);
        }
        Ok(Self {
            protocol,
            magic,
            server_guid: u64::decode_raknet(src)?,
        })
    }
}

/// Notification that the client is already connected to the server.
#[derive(Debug, Clone)]
pub struct AlreadyConnected {
    pub magic: Magic,
    pub server_guid: u64,
}

impl Packet for AlreadyConnected {
    const ID: u8 = 0x12;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.magic.encode_raknet(dst)?;
        self.server_guid.encode_raknet(dst)?;
        Ok(())
    }

    fn decode_body(src: &mut impl bytes::Buf) -> Result<Self, super::DecodeError> {
        let magic = Magic::decode_raknet(src)?;
        if magic != DEFAULT_UNCONNECTED_MAGIC {
            return Err(super::DecodeError::InvalidMagic);
        }
        Ok(Self {
            magic,
            server_guid: u64::decode_raknet(src)?,
        })
    }
}

/// Online connection request used once offline negotiation has succeeded.
#[derive(Debug, Clone)]
pub struct ConnectionRequest {
    pub client_guid: u64,
    pub timestamp: RaknetTime,
    pub secure: bool,
}

impl Packet for ConnectionRequest {
    const ID: u8 = 0x09;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.client_guid.encode_raknet(dst)?;
        self.timestamp.encode_raknet(dst)?;
        self.secure.encode_raknet(dst)?;
        Ok(())
    }

    fn decode_body(src: &mut impl bytes::Buf) -> Result<Self, super::DecodeError> {
        Ok(Self {
            client_guid: u64::decode_raknet(src)?,
            timestamp: RaknetTime::decode_raknet(src)?,
            secure: bool::decode_raknet(src)?,
        })
    }
}

/// Server's acceptance of a connection request, including system addresses.
#[derive(Debug, Clone)]
pub struct ConnectionRequestAccepted {
    pub address: SocketAddr,
    pub system_index: u16,
    pub system_addresses: [SocketAddr; 10],
    pub request_timestamp: RaknetTime,
    pub accepted_timestamp: RaknetTime,
}

impl Packet for ConnectionRequestAccepted {
    const ID: u8 = 0x10;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.address.encode_raknet(dst)?;
        self.system_index.encode_raknet(dst)?;

        for address in &self.system_addresses {
            address.encode_raknet(dst)?;
        }

        self.request_timestamp.encode_raknet(dst)?;
        self.accepted_timestamp.encode_raknet(dst)?;
        Ok(())
    }

    fn decode_body(src: &mut impl bytes::Buf) -> Result<Self, super::DecodeError> {
        let address = SocketAddr::decode_raknet(src)?;
        let system_index = u16::decode_raknet(src)?;

        let mut system_addresses: [SocketAddr; 10] = [SocketAddr::V4(constants::ANY_V4); 10];

        for addr in &mut system_addresses {
            *addr = SocketAddr::decode_raknet(src)?
        }

        let request_timestamp = RaknetTime::decode_raknet(src)?;
        let accepted_timestamp = RaknetTime::decode_raknet(src)?;

        Ok(Self {
            address,
            system_index,
            system_addresses,
            request_timestamp,
            accepted_timestamp,
        })
    }
}

/// Notification that a connection request failed.
#[derive(Debug, Clone)]
pub struct ConnectionRequestFailed {
    pub magic: Magic,
    pub server_guid: u64,
}

impl Packet for ConnectionRequestFailed {
    const ID: u8 = 0x11;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.magic.encode_raknet(dst)?;
        self.server_guid.encode_raknet(dst)?;
        Ok(())
    }

    fn decode_body(src: &mut impl bytes::Buf) -> Result<Self, super::DecodeError> {
        let magic = Magic::decode_raknet(src)?;
        if magic != DEFAULT_UNCONNECTED_MAGIC {
            return Err(super::DecodeError::InvalidMagic);
        }
        Ok(Self {
            magic,
            server_guid: u64::decode_raknet(src)?,
        })
    }
}

/// Notification of a new incoming connection, including system addresses.
#[derive(Debug, Clone)]
pub struct NewIncomingConnection {
    pub server_address: SocketAddr,
    pub system_addresses: [SocketAddr; 10],
    pub request_timestamp: RaknetTime,
    pub accepted_timestamp: RaknetTime,
}

impl Packet for NewIncomingConnection {
    const ID: u8 = 0x13;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.server_address.encode_raknet(dst)?;
        for address in &self.system_addresses {
            address.encode_raknet(dst)?;
        }
        self.request_timestamp.encode_raknet(dst)?;
        self.accepted_timestamp.encode_raknet(dst)?;
        Ok(())
    }

    fn decode_body(src: &mut impl bytes::Buf) -> Result<Self, super::DecodeError> {
        let server_address = SocketAddr::decode_raknet(src)?;

        let mut system_addresses: [SocketAddr; 10] = [SocketAddr::V4(constants::ANY_V4); 10];

        for addr in &mut system_addresses {
            *addr = SocketAddr::decode_raknet(src)?
        }

        let request_timestamp = RaknetTime::decode_raknet(src)?;
        let accepted_timestamp = RaknetTime::decode_raknet(src)?;
        Ok(Self {
            server_address,
            system_addresses,
            request_timestamp,
            accepted_timestamp,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn connection_request_roundtrip() {
        let pkt = ConnectionRequest {
            client_guid: 123,
            timestamp: RaknetTime(456),
            secure: true,
        };
        let mut buf = BytesMut::new();
        pkt.encode_body(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = ConnectionRequest::decode_body(&mut slice).unwrap();
        assert_eq!(decoded.client_guid, pkt.client_guid);
        assert_eq!(decoded.timestamp.0, pkt.timestamp.0);
        assert_eq!(decoded.secure, pkt.secure);
    }
}
