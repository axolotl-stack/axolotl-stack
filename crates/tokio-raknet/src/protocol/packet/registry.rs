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
    use crate::protocol::types::RaknetTime;
    use bytes::BytesMut;

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
}
