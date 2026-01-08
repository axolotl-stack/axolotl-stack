use bytes::{Buf, BufMut};

use crate::protocol::packet::{DecodeError, Packet};

use crate::protocol::packet::*;

/// INTERNAL
/// Macro used to generate the `RaknetPacket` enum type
/// which is used in networking loops to encode and decode
/// the high-level packet representation.
macro_rules! define_raknet_packets {
    (
        $(
            $name:ident,
        )+
    ) => {
        /// Top–level enum over all built-in RakNet control packets.
        ///
        /// This is what most networking code will work with when
        /// sending/receiving packets on a RakNet connection.
        #[derive(Debug)]
        pub enum RaknetPacket {
            $(
                $name($name),
            )+
            /// Packet with an ID >= 0x80 that is not recognised as a built-in
            /// control packet. The payload is preserved verbatim.
            UserData { id: u8, payload: bytes::Bytes },
        }

        impl RaknetPacket {
            /// Decode a single packet (ID byte + body) from the buffer.
            pub fn decode(src: &mut impl Buf) -> Result<Self, DecodeError> {
                if !src.has_remaining() {
                    return Err(DecodeError::UnexpectedEof);
                }
                let id = src.get_u8();
                Ok(match id {
                    $(
                        <$name as Packet>::ID => {
                            RaknetPacket::$name(<$name as Packet>::decode_body(src)?)
                        }
                    )+
                    other => {
                        let payload = src.copy_to_bytes(src.remaining());
                        RaknetPacket::UserData { id: other, payload }
                    }
                })
            }

            /// Return the wire ID associated with this packet.
            pub fn id(&self) -> u8 {
                match self {
                    $(
                        RaknetPacket::$name(_inner) => <$name as Packet>::ID,
                    )+
                    RaknetPacket::UserData { id, .. } => *id,
                }
            }

            /// Encode a packet into the destination buffer (ID byte + body).
            pub fn encode(&self, dst: &mut impl BufMut) -> Result<(), crate::protocol::packet::EncodeError> {
                dst.put_u8(self.id());
                match self {
                    $(
                        RaknetPacket::$name(inner) => inner.encode_body(dst)?,
                    )+
                    RaknetPacket::UserData { payload, .. } => dst.put_slice(payload),
                }
                Ok(())
            }
        }
    }
}

define_raknet_packets! {
    ConnectedPing,
    ConnectedPong,
    UnconnectedPing,
    UnconnectedPong,
    UnconnectedPingOpenConnections,
    OpenConnectionRequest1,
    OpenConnectionReply1,
    OpenConnectionRequest2,
    OpenConnectionReply2,
    ConnectionRequest,
    ConnectionRequestAccepted,
    ConnectionRequestFailed,
    AlreadyConnected,
    NewIncomingConnection,
    DetectLostConnection,
    NoFreeIncomingConnections,
    DisconnectionNotification,
    ConnectionLost,
    ConnectionBanned,
    IncompatibleProtocolVersion,
    IpRecentlyConnected,
    Timestamp,
    AdvertiseSystem,
    // These might not be needed
    // Idk if mc raknet does this behavior or only uses datagram level acks and naks.
    EncapsulatedNak,
    EncapsulatedAck,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ack::{AckNackPayload, SequenceRange};
    use crate::protocol::constants::DEFAULT_UNCONNECTED_MAGIC;
    use crate::protocol::state::DisconnectReason;
    use crate::protocol::types::{Advertisement, EoBPadding, RaknetTime, Sequence24};
    use bytes::BytesMut;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

    // Helper to create test socket address
    fn test_addr() -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 19132))
    }

    // Helper to create array of 10 socket addresses
    fn test_system_addresses() -> [SocketAddr; 10] {
        [test_addr(); 10]
    }

    #[test]
    fn connected_ping_roundtrip_via_enum() {
        let pkt = RaknetPacket::ConnectedPing(ConnectedPing {
            ping_time: RaknetTime(42),
        });

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::ConnectedPing(inner) => {
                assert_eq!(inner.ping_time.0, 42);
            }
            other => panic!("unexpected packet variant: {}", other.id()),
        }
    }

    #[test]
    fn unknown_user_data_id_is_preserved() {
        let id: u8 = 0x80;
        let payload = [1u8, 2, 3];
        let mut buf = BytesMut::new();
        buf.put_u8(id);
        buf.extend_from_slice(&payload);

        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::UserData {
                id: got_id,
                payload: body,
            } => {
                assert_eq!(got_id, id);
                assert_eq!(&body[..], &payload[..]);
            }
            other => panic!("expected UserData, got id {}", other.id()),
        }
    }

    // ============================================================================
    // Comprehensive Packet Roundtrip Tests
    // ============================================================================

    #[test]
    fn connected_pong_roundtrip() {
        let pkt = RaknetPacket::ConnectedPong(ConnectedPong {
            ping_time: RaknetTime(100),
            pong_time: RaknetTime(200),
        });

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::ConnectedPong(inner) => {
                assert_eq!(inner.ping_time.0, 100);
                assert_eq!(inner.pong_time.0, 200);
            }
            other => panic!("unexpected packet variant: {}", other.id()),
        }
    }

    #[test]
    fn unconnected_ping_roundtrip() {
        let pkt = RaknetPacket::UnconnectedPing(UnconnectedPing {
            ping_time: RaknetTime(12345),
            magic: DEFAULT_UNCONNECTED_MAGIC,
        });

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::UnconnectedPing(inner) => {
                assert_eq!(inner.ping_time.0, 12345);
                assert_eq!(inner.magic, DEFAULT_UNCONNECTED_MAGIC);
            }
            other => panic!("unexpected packet variant: {}", other.id()),
        }
    }

    #[test]
    fn unconnected_pong_roundtrip() {
        let pkt = RaknetPacket::UnconnectedPong(UnconnectedPong {
            ping_time: RaknetTime(111),
            server_guid: 0xDEADBEEF,
            magic: DEFAULT_UNCONNECTED_MAGIC,
            advertisement: Advertisement(None),
        });

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::UnconnectedPong(inner) => {
                assert_eq!(inner.ping_time.0, 111);
                assert_eq!(inner.server_guid, 0xDEADBEEF);
            }
            other => panic!("unexpected packet variant: {}", other.id()),
        }
    }

    #[test]
    fn unconnected_pong_with_advertisement_roundtrip() {
        let pkt = RaknetPacket::UnconnectedPong(UnconnectedPong {
            ping_time: RaknetTime(222),
            server_guid: 0x12345678,
            magic: DEFAULT_UNCONNECTED_MAGIC,
            advertisement: Advertisement(Some(bytes::Bytes::from_static(b"MCPE;Test Server;1;2"))),
        });

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::UnconnectedPong(inner) => {
                assert_eq!(inner.ping_time.0, 222);
                assert!(inner.advertisement.0.is_some());
            }
            other => panic!("unexpected packet variant: {}", other.id()),
        }
    }

    #[test]
    fn unconnected_ping_open_connections_roundtrip() {
        let pkt = RaknetPacket::UnconnectedPingOpenConnections(UnconnectedPingOpenConnections {
            ping_time: RaknetTime(333),
            magic: DEFAULT_UNCONNECTED_MAGIC,
        });

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::UnconnectedPingOpenConnections(inner) => {
                assert_eq!(inner.ping_time.0, 333);
            }
            other => panic!("unexpected packet variant: {}", other.id()),
        }
    }

    #[test]
    fn open_connection_request1_roundtrip() {
        let pkt = RaknetPacket::OpenConnectionRequest1(OpenConnectionRequest1 {
            magic: DEFAULT_UNCONNECTED_MAGIC,
            protocol_version: 11,
            padding: EoBPadding(100),
        });

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::OpenConnectionRequest1(inner) => {
                assert_eq!(inner.protocol_version, 11);
                // Padding length consumed as remaining
            }
            other => panic!("unexpected packet variant: {}", other.id()),
        }
    }

    #[test]
    fn open_connection_reply1_roundtrip_no_cookie() {
        let pkt = RaknetPacket::OpenConnectionReply1(OpenConnectionReply1 {
            magic: DEFAULT_UNCONNECTED_MAGIC,
            server_guid: 0xCAFEBABE,
            cookie: None,
            mtu: 1400,
        });

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::OpenConnectionReply1(inner) => {
                assert_eq!(inner.server_guid, 0xCAFEBABE);
                assert_eq!(inner.cookie, None);
                assert_eq!(inner.mtu, 1400);
            }
            other => panic!("unexpected packet variant: {}", other.id()),
        }
    }

    #[test]
    fn open_connection_reply1_roundtrip_with_cookie() {
        let pkt = RaknetPacket::OpenConnectionReply1(OpenConnectionReply1 {
            magic: DEFAULT_UNCONNECTED_MAGIC,
            server_guid: 0x12345678,
            cookie: Some(0xDEADC0DE),
            mtu: 1200,
        });

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::OpenConnectionReply1(inner) => {
                assert_eq!(inner.server_guid, 0x12345678);
                assert_eq!(inner.cookie, Some(0xDEADC0DE));
                assert_eq!(inner.mtu, 1200);
            }
            other => panic!("unexpected packet variant: {}", other.id()),
        }
    }

    #[test]
    fn open_connection_request2_roundtrip_no_cookie() {
        let pkt = RaknetPacket::OpenConnectionRequest2(OpenConnectionRequest2 {
            magic: DEFAULT_UNCONNECTED_MAGIC,
            cookie: None,
            client_proof: false,
            server_addr: test_addr(),
            mtu: 1400,
            client_guid: 0xABCDEF01,
        });

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::OpenConnectionRequest2(inner) => {
                assert_eq!(inner.mtu, 1400);
                assert_eq!(inner.client_guid, 0xABCDEF01);
            }
            other => panic!("unexpected packet variant: {}", other.id()),
        }
    }

    #[test]
    fn open_connection_reply2_roundtrip() {
        let pkt = RaknetPacket::OpenConnectionReply2(OpenConnectionReply2 {
            magic: DEFAULT_UNCONNECTED_MAGIC,
            server_guid: 0x99887766,
            server_addr: test_addr(),
            mtu: 1400,
            security: false,
        });

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::OpenConnectionReply2(inner) => {
                assert_eq!(inner.server_guid, 0x99887766);
                assert_eq!(inner.mtu, 1400);
                assert!(!inner.security);
            }
            other => panic!("unexpected packet variant: {}", other.id()),
        }
    }

    #[test]
    fn connection_request_roundtrip() {
        let pkt = RaknetPacket::ConnectionRequest(ConnectionRequest {
            client_guid: 0x11223344,
            timestamp: RaknetTime(555),
            secure: true,
        });

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::ConnectionRequest(inner) => {
                assert_eq!(inner.client_guid, 0x11223344);
                assert_eq!(inner.timestamp.0, 555);
                assert!(inner.secure);
            }
            other => panic!("unexpected packet variant: {}", other.id()),
        }
    }

    #[test]
    fn connection_request_accepted_roundtrip() {
        let pkt = RaknetPacket::ConnectionRequestAccepted(ConnectionRequestAccepted {
            address: test_addr(),
            system_index: 0,
            system_addresses: test_system_addresses(),
            request_timestamp: RaknetTime(1000),
            accepted_timestamp: RaknetTime(2000),
        });

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::ConnectionRequestAccepted(inner) => {
                assert_eq!(inner.system_index, 0);
                assert_eq!(inner.request_timestamp.0, 1000);
                assert_eq!(inner.accepted_timestamp.0, 2000);
            }
            other => panic!("unexpected packet variant: {}", other.id()),
        }
    }

    #[test]
    fn connection_request_failed_roundtrip() {
        let pkt = RaknetPacket::ConnectionRequestFailed(ConnectionRequestFailed {
            magic: DEFAULT_UNCONNECTED_MAGIC,
            server_guid: 0xFFEEDDCC,
        });

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::ConnectionRequestFailed(inner) => {
                assert_eq!(inner.server_guid, 0xFFEEDDCC);
            }
            other => panic!("unexpected packet variant: {}", other.id()),
        }
    }

    #[test]
    fn already_connected_roundtrip() {
        let pkt = RaknetPacket::AlreadyConnected(AlreadyConnected {
            magic: DEFAULT_UNCONNECTED_MAGIC,
            server_guid: 0xAABBCCDD,
        });

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::AlreadyConnected(inner) => {
                assert_eq!(inner.server_guid, 0xAABBCCDD);
            }
            other => panic!("unexpected packet variant: {}", other.id()),
        }
    }

    #[test]
    fn new_incoming_connection_roundtrip() {
        let pkt = RaknetPacket::NewIncomingConnection(NewIncomingConnection {
            server_address: test_addr(),
            system_addresses: test_system_addresses(),
            request_timestamp: RaknetTime(3000),
            accepted_timestamp: RaknetTime(4000),
        });

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::NewIncomingConnection(inner) => {
                assert_eq!(inner.request_timestamp.0, 3000);
                assert_eq!(inner.accepted_timestamp.0, 4000);
            }
            other => panic!("unexpected packet variant: {}", other.id()),
        }
    }

    #[test]
    fn detect_lost_connection_roundtrip() {
        let pkt = RaknetPacket::DetectLostConnection(DetectLostConnection);

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        assert!(matches!(decoded, RaknetPacket::DetectLostConnection(_)));
    }

    #[test]
    fn no_free_incoming_connections_roundtrip() {
        let pkt = RaknetPacket::NoFreeIncomingConnections(NoFreeIncomingConnections);

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        assert!(matches!(
            decoded,
            RaknetPacket::NoFreeIncomingConnections(_)
        ));
    }

    #[test]
    fn disconnection_notification_roundtrip() {
        for reason in [
            DisconnectReason::ClosedByRemotePeer,
            DisconnectReason::ShuttingDown,
            DisconnectReason::Disconnected,
            DisconnectReason::TimedOut,
        ] {
            let pkt = RaknetPacket::DisconnectionNotification(DisconnectionNotification { reason });

            let mut buf = BytesMut::new();
            pkt.encode(&mut buf).unwrap();
            let mut slice = buf.freeze();
            let decoded = RaknetPacket::decode(&mut slice).unwrap();

            match decoded {
                RaknetPacket::DisconnectionNotification(inner) => {
                    // Compare discriminants since DisconnectReason doesn't derive PartialEq
                    assert_eq!(inner.reason as u8, reason as u8);
                }
                other => panic!("unexpected packet variant: {}", other.id()),
            }
        }
    }

    #[test]
    fn incompatible_protocol_version_roundtrip() {
        let pkt = RaknetPacket::IncompatibleProtocolVersion(IncompatibleProtocolVersion {
            protocol: 11,
            magic: DEFAULT_UNCONNECTED_MAGIC,
            server_guid: 0x55667788,
        });

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::IncompatibleProtocolVersion(inner) => {
                assert_eq!(inner.protocol, 11);
                assert_eq!(inner.server_guid, 0x55667788);
            }
            other => panic!("unexpected packet variant: {}", other.id()),
        }
    }

    #[test]
    fn ip_recently_connected_roundtrip() {
        let pkt = RaknetPacket::IpRecentlyConnected(IpRecentlyConnected);

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        assert!(matches!(decoded, RaknetPacket::IpRecentlyConnected(_)));
    }

    #[test]
    fn advertise_system_roundtrip() {
        let pkt = RaknetPacket::AdvertiseSystem(AdvertiseSystem {
            ping_time: RaknetTime(5000),
            server_guid: 0x11111111,
            magic: DEFAULT_UNCONNECTED_MAGIC,
            advertisement: Advertisement(None),
        });

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::AdvertiseSystem(inner) => {
                assert_eq!(inner.ping_time.0, 5000);
                assert_eq!(inner.server_guid, 0x11111111);
            }
            other => panic!("unexpected packet variant: {}", other.id()),
        }
    }

    #[test]
    fn encapsulated_ack_roundtrip() {
        let pkt = RaknetPacket::EncapsulatedAck(EncapsulatedAck(AckNackPayload {
            ranges: vec![
                SequenceRange {
                    start: Sequence24::new(1),
                    end: Sequence24::new(5),
                },
                SequenceRange {
                    start: Sequence24::new(10),
                    end: Sequence24::new(10),
                },
            ],
        }));

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::EncapsulatedAck(inner) => {
                assert_eq!(inner.0.ranges.len(), 2);
                assert_eq!(inner.0.ranges[0].start.value(), 1);
                assert_eq!(inner.0.ranges[0].end.value(), 5);
            }
            other => panic!("unexpected packet variant: {}", other.id()),
        }
    }

    #[test]
    fn encapsulated_nak_roundtrip() {
        let pkt = RaknetPacket::EncapsulatedNak(EncapsulatedNak(AckNackPayload {
            ranges: vec![SequenceRange {
                start: Sequence24::new(100),
                end: Sequence24::new(200),
            }],
        }));

        let mut buf = BytesMut::new();
        pkt.encode(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetPacket::decode(&mut slice).unwrap();

        match decoded {
            RaknetPacket::EncapsulatedNak(inner) => {
                assert_eq!(inner.0.ranges.len(), 1);
                assert_eq!(inner.0.ranges[0].start.value(), 100);
                assert_eq!(inner.0.ranges[0].end.value(), 200);
            }
            other => panic!("unexpected packet variant: {}", other.id()),
        }
    }

    // ============================================================================
    // Edge Case Tests
    // ============================================================================

    #[test]
    fn packet_id_values_are_correct() {
        // Verify packet IDs match expected values
        assert_eq!(ConnectedPing::ID, 0x00);
        assert_eq!(UnconnectedPing::ID, 0x01);
        assert_eq!(UnconnectedPingOpenConnections::ID, 0x02);
        assert_eq!(ConnectedPong::ID, 0x03);
        assert_eq!(DetectLostConnection::ID, 0x04);
        assert_eq!(OpenConnectionRequest1::ID, 0x05);
        assert_eq!(OpenConnectionReply1::ID, 0x06);
        assert_eq!(OpenConnectionRequest2::ID, 0x07);
        assert_eq!(OpenConnectionReply2::ID, 0x08);
        assert_eq!(ConnectionRequest::ID, 0x09);
        assert_eq!(ConnectionRequestAccepted::ID, 0x10);
        assert_eq!(ConnectionRequestFailed::ID, 0x11);
        assert_eq!(AlreadyConnected::ID, 0x12);
        assert_eq!(NewIncomingConnection::ID, 0x13);
        assert_eq!(NoFreeIncomingConnections::ID, 0x14);
        assert_eq!(DisconnectionNotification::ID, 0x15);
        assert_eq!(ConnectionLost::ID, 0x16);
        assert_eq!(ConnectionBanned::ID, 0x17);
        assert_eq!(IncompatibleProtocolVersion::ID, 0x19);
        assert_eq!(IpRecentlyConnected::ID, 0x1a);
        assert_eq!(Timestamp::ID, 0x1b);
        assert_eq!(UnconnectedPong::ID, 0x1c);
        assert_eq!(AdvertiseSystem::ID, 0x1d);
        assert_eq!(EncapsulatedNak::ID, 0xa0);
        assert_eq!(EncapsulatedAck::ID, 0xc0);
    }

    #[test]
    fn empty_buffer_returns_eof_error() {
        let mut empty = bytes::Bytes::new();
        let result = RaknetPacket::decode(&mut empty);
        assert!(matches!(result, Err(DecodeError::UnexpectedEof)));
    }

    #[test]
    fn raknet_packet_id_method() {
        let pkt = RaknetPacket::ConnectedPing(ConnectedPing {
            ping_time: RaknetTime(0),
        });
        assert_eq!(pkt.id(), 0x00);

        let pkt = RaknetPacket::UserData {
            id: 0x85,
            payload: bytes::Bytes::new(),
        };
        assert_eq!(pkt.id(), 0x85);
    }
}
