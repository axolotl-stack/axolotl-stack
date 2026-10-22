//! Offline connection negotiation and handshake packets.
//!
//! These packets are exchanged before a fully connected session exists.

use std::net::SocketAddr;

use bytes::{BufMut, Bytes};

use crate::protocol::{
    constants::DEFAULT_UNCONNECTED_MAGIC,
    packet::{Packet, RaknetEncodable},
    types::{AddressLayout, EoBPadding, Magic, RaknetTime, encode_address},
};

/// Maximum system addresses carried by `ConnectionRequestAccepted` and
/// `NewIncomingConnection` (go-raknet `internal/message/addr.go:systemAddresses`).
pub const MAX_SYSTEM_ADDRESSES: usize = 20;

/// go-raknet `connection_request_accepted.go:UnmarshalBinary`: read up to 20
/// system addresses, stopping as soon as exactly the two trailing timestamps
/// remain. Peers send 10 (RakNet) or 20 (Bedrock, go-raknet) addresses.
fn decode_system_addresses(
    src: &mut impl bytes::Buf,
) -> Result<Vec<SocketAddr>, super::DecodeError> {
    let mut addresses = Vec::with_capacity(MAX_SYSTEM_ADDRESSES);
    while addresses.len() < MAX_SYSTEM_ADDRESSES && src.remaining() != 16 {
        addresses.push(SocketAddr::decode_raknet(src)?);
    }
    Ok(addresses)
}

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
        if let Some(cookie) = &self.cookie {
            cookie.encode_raknet(dst)?;
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

impl OpenConnectionRequest2 {
    /// Decode using the security setting negotiated in reply 1, as required by
    /// go-raknet `internal/message/open_connection_request_2.go:OpenConnectionRequest2.UnmarshalBinary`.
    /// Cookies may begin with an IPv4/IPv6 tag, so live listeners must use this
    /// contextual decoder rather than infer cookie presence from packet bytes.
    pub fn decode_body_with_security(
        src: &mut impl bytes::Buf,
        security: bool,
    ) -> Result<Self, super::DecodeError> {
        let magic = Magic::decode_raknet(src)?;
        if magic != DEFAULT_UNCONNECTED_MAGIC {
            return Err(super::DecodeError::InvalidMagic);
        }
        Self::decode_fields(src, magic, security)
    }

    fn decode_fields(
        src: &mut impl bytes::Buf,
        magic: Magic,
        security: bool,
    ) -> Result<Self, super::DecodeError> {
        let (cookie, client_proof) = if security {
            (Some(u32::decode_raknet(src)?), bool::decode_raknet(src)?)
        } else {
            (None, false)
        };
        Ok(Self {
            magic,
            cookie,
            client_proof,
            server_addr: SocketAddr::decode_raknet(src)?,
            mtu: u16::decode_raknet(src)?,
            client_guid: u64::decode_raknet(src)?,
        })
    }
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

        // The context-free trait implementation retains its historical
        // inference for callers that do not have the negotiated security mode.
        // Network listeners use decode_body_with_security instead.
        let first = cursor.first().ok_or(super::DecodeError::UnexpectedEof)?;
        let security = *first != 4 && *first != 6;
        Self::decode_fields(&mut cursor, magic, security)
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
    pub system_addresses: Vec<SocketAddr>,
    pub request_timestamp: RaknetTime,
    pub accepted_timestamp: RaknetTime,
}

impl ConnectionRequestAccepted {
    /// Encode the body with an explicit address layout (lunar's Vanilla
    /// listener uses [`AddressLayout::Lunar`] for IPv6 clients).
    pub fn encode_body_with_layout(
        &self,
        dst: &mut impl BufMut,
        layout: AddressLayout,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        encode_address(&self.address, layout, dst)?;
        self.system_index.encode_raknet(dst)?;
        for address in &self.system_addresses {
            encode_address(address, layout, dst)?;
        }
        self.request_timestamp.encode_raknet(dst)?;
        self.accepted_timestamp.encode_raknet(dst)?;
        Ok(())
    }
}

impl Packet for ConnectionRequestAccepted {
    const ID: u8 = 0x10;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.encode_body_with_layout(dst, AddressLayout::Standard)
    }

    fn decode_body(src: &mut impl bytes::Buf) -> Result<Self, super::DecodeError> {
        let address = SocketAddr::decode_raknet(src)?;
        let system_index = u16::decode_raknet(src)?;
        let system_addresses = decode_system_addresses(src)?;

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
    pub system_addresses: Vec<SocketAddr>,
    pub request_timestamp: RaknetTime,
    pub accepted_timestamp: RaknetTime,
}

impl NewIncomingConnection {
    /// Encode the body with an explicit layout for the server address.
    pub fn encode_body_with_layout(
        &self,
        dst: &mut impl BufMut,
        layout: AddressLayout,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        encode_address(&self.server_address, layout, dst)?;
        for address in &self.system_addresses {
            encode_address(address, layout, dst)?;
        }
        self.request_timestamp.encode_raknet(dst)?;
        self.accepted_timestamp.encode_raknet(dst)?;
        Ok(())
    }
}

impl Packet for NewIncomingConnection {
    const ID: u8 = 0x13;

    fn encode_body(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.encode_body_with_layout(dst, AddressLayout::Standard)
    }

    fn decode_body(src: &mut impl bytes::Buf) -> Result<Self, super::DecodeError> {
        let server_address = SocketAddr::decode_raknet(src)?;
        let system_addresses = decode_system_addresses(src)?;

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
    fn secured_request2_cookie_prefix_is_not_an_address_tag() {
        for address in ["127.0.0.1:19132", "[::1]:19132"] {
            for cookie in [0x0401_0203, 0x0601_0203, 0xffff_ffff] {
                let request = OpenConnectionRequest2 {
                    magic: DEFAULT_UNCONNECTED_MAGIC,
                    cookie: Some(cookie),
                    client_proof: true,
                    server_addr: address.parse().unwrap(),
                    mtu: 1492,
                    client_guid: 123,
                };
                let mut encoded = BytesMut::new();
                request.encode_body(&mut encoded).unwrap();
                for len in 0..encoded.len() {
                    assert!(
                        OpenConnectionRequest2::decode_body_with_security(
                            &mut &encoded[..len],
                            true
                        )
                        .is_err()
                    );
                }
                let decoded =
                    OpenConnectionRequest2::decode_body_with_security(&mut encoded.freeze(), true)
                        .unwrap();
                assert_eq!(decoded.cookie, Some(cookie));
                assert!(decoded.client_proof);
                assert_eq!(decoded.server_addr, request.server_addr);
                assert_eq!(decoded.mtu, request.mtu);
                assert_eq!(decoded.client_guid, request.client_guid);
            }
        }
    }

    #[test]
    fn unsecured_request2_context_reads_address_directly() {
        let request = OpenConnectionRequest2 {
            magic: DEFAULT_UNCONNECTED_MAGIC,
            cookie: None,
            client_proof: false,
            server_addr: "127.0.0.1:19132".parse().unwrap(),
            mtu: 400,
            client_guid: 123,
        };
        let mut encoded = BytesMut::new();
        request.encode_body(&mut encoded).unwrap();
        let decoded =
            OpenConnectionRequest2::decode_body_with_security(&mut encoded.freeze(), false)
                .unwrap();
        assert_eq!(decoded.cookie, None);
        assert_eq!(decoded.server_addr, request.server_addr);
        assert_eq!(decoded.mtu, request.mtu);
        assert_eq!(decoded.client_guid, request.client_guid);
    }

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

    /// go-raknet `ConnectionRequestAccepted.MarshalBinary` with its 20 zero
    /// system addresses. Before the fix the decoder consumed exactly ten
    /// addresses and read both timestamps out of the eleventh/twelfth address.
    #[test]
    fn connection_request_accepted_with_twenty_addresses_keeps_timestamps() {
        for count in [10usize, 20] {
            let mut bytes = vec![0x04, 0x80, 0xff, 0xff, 0xfe, 0x4a, 0xbc, 0x00, 0x00];
            for _ in 0..count {
                bytes.extend_from_slice(&[0x04, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00]);
            }
            bytes.extend_from_slice(&0x0102u64.to_be_bytes());
            bytes.extend_from_slice(&0x0304u64.to_be_bytes());
            let decoded = ConnectionRequestAccepted::decode_body(&mut &bytes[..]).unwrap();
            assert_eq!(decoded.system_addresses.len(), count);
            assert_eq!(decoded.request_timestamp.0, 0x0102);
            assert_eq!(decoded.accepted_timestamp.0, 0x0304);
            assert_eq!(decoded.address, "127.0.0.1:19132".parse().unwrap());

            let mut nic = vec![0x04, 0x80, 0xff, 0xff, 0xfe, 0x4a, 0xbc];
            nic.extend_from_slice(&bytes[9..]);
            let decoded = NewIncomingConnection::decode_body(&mut &nic[..]).unwrap();
            assert_eq!(decoded.system_addresses.len(), count);
            assert_eq!(decoded.request_timestamp.0, 0x0102);
            assert_eq!(decoded.accepted_timestamp.0, 0x0304);
        }
    }
}
