use bytes::{Buf, BufMut};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use crate::protocol::packet::{DecodeError, EncodeError, RaknetEncodable};

impl RaknetEncodable for SocketAddr {
    fn encode_raknet(&self, dst: &mut impl BufMut) -> Result<(), EncodeError> {
        match self {
            SocketAddr::V4(addr) => {
                dst.put_u8(4); // Version 4

                // Get the raw IP bytes
                let ip_bytes = addr.ip().octets();

                let flipped_ip: [u8; 4] = [!ip_bytes[0], !ip_bytes[1], !ip_bytes[2], !ip_bytes[3]];

                dst.put_slice(&flipped_ip);
                dst.put_u16(addr.port());
            }
            SocketAddr::V6(addr) => {
                dst.put_u8(6); // Version 6

                // This manually serializes the C-style `sockaddr_in6` struct
                // Cloudburst uses 23, so we will too.
                dst.put_u16_le(23); // sin6_family (AF_INET6)
                dst.put_u16(addr.port()); // sin6_port
                dst.put_u32(addr.flowinfo()); // sin6_flowinfo
                dst.put_slice(&addr.ip().octets()); // sin6_addr (16 bytes)
                dst.put_u32(addr.scope_id()); // sin6_scope_id
            }
        }
        Ok(())
    }

    fn decode_raknet(src: &mut impl Buf) -> Result<Self, DecodeError> {
        if src.remaining() < 1 {
            return Err(DecodeError::UnexpectedEof);
        }
        let version = src.get_u8();

        match version {
            4 => {
                // IPv4
                if src.remaining() < 4 + 2 {
                    // 4 IP bytes + 2 port bytes
                    return Err(DecodeError::UnexpectedEof);
                }
                let mut ip_bytes = [0u8; 4];
                src.copy_to_slice(&mut ip_bytes);

                // Un-flip the bytes
                let unflipped_ip: [u8; 4] =
                    [!ip_bytes[0], !ip_bytes[1], !ip_bytes[2], !ip_bytes[3]];

                let port = src.get_u16();
                Ok(SocketAddr::V4(SocketAddrV4::new(
                    Ipv4Addr::from(unflipped_ip),
                    port,
                )))
            }
            6 => {
                // IPv6
                // Check for all fields: family(2) + port(2) + flow(4) + ip(16) + scope(4)
                if src.remaining() < 2 + 2 + 4 + 16 + 4 {
                    return Err(DecodeError::UnexpectedEof);
                }

                let _family = src.get_u16_le(); // Read and discard
                let port = src.get_u16();
                let flowinfo = src.get_u32();
                let mut ip_bytes = [0u8; 16];
                src.copy_to_slice(&mut ip_bytes);
                let scope_id = src.get_u32();

                Ok(SocketAddr::V6(SocketAddrV6::new(
                    Ipv6Addr::from(ip_bytes),
                    port,
                    flowinfo,
                    scope_id,
                )))
            }
            _ => Err(DecodeError::InvalidAddrVersion(version)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn ipv4_roundtrip() -> Result<(), DecodeError> {
        let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 19132));
        let mut buf = BytesMut::new();
        addr.encode_raknet(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = SocketAddr::decode_raknet(&mut slice)?;
        assert_eq!(decoded, addr);
        Ok(())
    }

    #[test]
    fn invalid_version_yields_error() {
        let buf = BytesMut::from(&b"\x07"[..]); // unsupported version
        let mut slice = buf.freeze();
        let err = SocketAddr::decode_raknet(&mut slice).unwrap_err();
        match err {
            DecodeError::InvalidAddrVersion(7) => {}
            other => panic!("unexpected error: {:?}", other),
        }
    }
}
