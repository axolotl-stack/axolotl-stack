//! Packet types for LAN discovery.
//!
//! Packets are encrypted with AES-ECB and verified with HMAC-SHA256.

use std::io::{Cursor, Read};

use bytes::{Buf, BufMut};

use super::crypto;

/// Upstream `discovery/packet.go:Unmarshal` (`IDRequestPacket` through `IDMessagePacket`).
const ID_REQUEST: u16 = 0;
const ID_RESPONSE: u16 = 1;
const ID_MESSAGE: u16 = 2;

/// Decoded packet types.
pub enum Packet {
    Request,
    Response(Vec<u8>),
    Message(MessageData),
}

/// Message packet data.
pub struct MessageData {
    /// Network ID, not connection ID: upstream `discovery/packet_message.go:MessagePacket.Read`.
    pub recipient_id: u64,
    pub data: String,
}

/// Transport layer used for NetherNet connections.
///
/// Session-property compatibility only; upstream v7 `discovery/server_data.go:ServerData.MarshalBinary`
/// has no transport-layer field.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportLayer {
    /// RakNet over UDP.
    RakNet = 0,
    /// NetherNet (WebRTC via LAN discovery or Xbox signaling).
    #[default]
    NetherNet = 2,
}

impl From<TransportLayer> for u8 {
    fn from(t: TransportLayer) -> u8 {
        t as u8
    }
}

impl TransportLayer {
    /// Convert from raw u8 value.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::RakNet),
            2 => Some(Self::NetherNet),
            _ => None,
        }
    }
}

/// Server data for discovery responses.
/// Version 7 layout: upstream `discovery/server_data.go:ServerData.MarshalBinary`.
#[derive(Debug, Clone)]
pub struct ServerData {
    pub server_name: String,
    pub level_name: String,
    pub protocol: i32,
    pub version: String,
    pub game_type: i32,
    pub player_count: i32,
    pub max_player_count: i32,
    pub editor_world: bool,
    pub hardcore: bool,
    /// Retained for callers selecting transports; v7 discovery does not encode this field.
    pub transport_layer: TransportLayer,
    pub accepts_online_auth: bool,
    pub accepts_self_signed_auth: bool,
    pub nonce: String,
    /// LAN value 4: upstream `discovery/listener.go:Listener.PongData`.
    pub connection_type: i32,
}

impl Default for ServerData {
    fn default() -> Self {
        Self {
            server_name: String::new(),
            level_name: String::new(),
            protocol: 0,
            version: String::new(),
            game_type: 0,
            player_count: 1, // Must be >= 1 or world won't show
            max_player_count: 10,
            editor_world: false,
            hardcore: false,
            accepts_online_auth: true,
            accepts_self_signed_auth: true,
            nonce: String::new(),
            transport_layer: TransportLayer::NetherNet,
            connection_type: 4, // LAN signaling
        }
    }
}

impl ServerData {
    /// Create a new builder for ServerData.
    pub fn builder() -> ServerDataBuilder {
        ServerDataBuilder::default()
    }
}

/// Builder for ServerData with fluent API.
#[derive(Default)]
pub struct ServerDataBuilder {
    data: ServerData,
}

impl ServerDataBuilder {
    /// Set the server name displayed in the server list.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.data.server_name = name.into();
        self
    }

    /// Set the level/world name.
    pub fn level(mut self, level: impl Into<String>) -> Self {
        self.data.level_name = level.into();
        self
    }

    /// Set the transport layer.
    pub fn transport(mut self, transport: TransportLayer) -> Self {
        self.data.transport_layer = transport;
        self
    }

    /// Set player counts (current, max).
    pub fn players(mut self, current: i32, max: i32) -> Self {
        self.data.player_count = current;
        self.data.max_player_count = max;
        self
    }

    /// Set game type: 0 Survival, 1 Creative, 2 Adventure.
    /// Upstream `discovery/listener.go:parsePongGameType` and `discovery/server_data.go:GameTypeSurvival`.
    pub fn game_type(mut self, game_type: i32) -> Self {
        self.data.game_type = game_type;
        self
    }

    /// Build the ServerData.
    pub fn build(self) -> ServerData {
        self.data
    }
}

/// Upstream `discovery/server_data.go:ServerData.MarshalBinary` (`version`).
const SERVER_DATA_VERSION: u8 = 7;

impl ServerData {
    /// Encode the v7 layout from `discovery/server_data.go:ServerData.MarshalBinary`.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.put_u8(SERVER_DATA_VERSION);
        write_string(&mut buf, &self.server_name);
        write_varint(&mut buf, self.protocol);
        write_string(&mut buf, &self.version);
        write_string(&mut buf, &self.level_name);
        write_varint(&mut buf, self.player_count);
        write_varint(&mut buf, self.max_player_count);
        write_varint(&mut buf, self.game_type);
        buf.extend_from_slice(&[
            self.editor_world as u8,
            self.hardcore as u8,
            self.accepts_online_auth as u8,
            self.accepts_self_signed_auth as u8,
        ]);
        write_string(&mut buf, &self.nonce);
        write_varint(&mut buf, self.connection_type);
        buf
    }

    /// Decode v7, rejecting truncated fields and trailing bytes like upstream.
    pub fn decode(mut data: &[u8]) -> Option<Self> {
        if take_byte(&mut data)? != SERVER_DATA_VERSION {
            return None;
        }
        let result = Self {
            server_name: read_string(&mut data)?,
            protocol: read_varint(&mut data)?,
            version: read_string(&mut data)?,
            level_name: read_string(&mut data)?,
            player_count: read_varint(&mut data)?,
            max_player_count: read_varint(&mut data)?,
            game_type: read_varint(&mut data)?,
            editor_world: take_byte(&mut data)? != 0,
            hardcore: take_byte(&mut data)? != 0,
            accepts_online_auth: take_byte(&mut data)? != 0,
            accepts_self_signed_auth: take_byte(&mut data)? != 0,
            nonce: read_string(&mut data)?,
            connection_type: read_varint(&mut data)?,
            transport_layer: TransportLayer::NetherNet,
        };
        data.is_empty().then_some(result)
    }
}

/// Upstream `discovery/binary.go:writeVaruint32` uses 7-bit groups and continuation bit 0x80.
fn write_varuint(buf: &mut Vec<u8>, mut value: u32) {
    while value >= 0x80 {
        buf.push(value as u8 | 0x80);
        value >>= 7;
    }
    buf.push(value as u8);
}

fn take_byte(data: &mut &[u8]) -> Option<u8> {
    let (&byte, rest) = data.split_first()?;
    *data = rest;
    Some(byte)
}

/// Upstream `discovery/binary.go:readVaruint32` allows at most five bytes.
fn read_varuint(data: &mut &[u8]) -> Option<u32> {
    let mut value = 0;
    for shift in (0..35).step_by(7) {
        let byte = take_byte(data)?;
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// Zigzag encoding: upstream `discovery/binary.go:writeVarint32`.
fn write_varint(buf: &mut Vec<u8>, value: i32) {
    write_varuint(buf, ((value as u32) << 1) ^ ((value >> 31) as u32));
}

fn read_varint(data: &mut &[u8]) -> Option<i32> {
    let value = read_varuint(data)?;
    Some((value >> 1) as i32 ^ -((value & 1) as i32))
}

fn write_string(buf: &mut Vec<u8>, value: &str) {
    write_varuint(buf, value.len() as u32);
    buf.extend_from_slice(value.as_bytes());
}

fn read_string(data: &mut &[u8]) -> Option<String> {
    let len = read_varuint(data)? as usize;
    let bytes = data.get(..len)?;
    let value = String::from_utf8(bytes.to_vec()).ok()?;
    *data = &data[len..];
    Some(value)
}

fn write_string_u32(buf: &mut Vec<u8>, s: &str) {
    buf.put_u32_le(s.len() as u32);
    buf.extend_from_slice(s.as_bytes());
}

/// 18-byte header (u16 ID, u64 sender, eight padding bytes): upstream `discovery/packet.go:Header.Write`.
struct Header {
    packet_id: u16,
    sender_id: u64,
}

impl Header {
    fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(18);
        buf.put_u16_le(self.packet_id);
        buf.put_u64_le(self.sender_id);
        buf.put_u64_le(0); // 8 bytes padding
        buf
    }

    fn decode(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 18 {
            return None;
        }
        let mut cursor = Cursor::new(data);
        let header = Self {
            packet_id: cursor.get_u16_le(),
            sender_id: cursor.get_u64_le(),
        };
        cursor.advance(8); // Skip padding
        Some((header, 18))
    }
}

/// Encode a request packet.
pub fn encode_request(sender_id: u64) -> Vec<u8> {
    let header = Header {
        packet_id: ID_REQUEST,
        sender_id,
    };
    let payload = header.encode();
    wrap_packet(&payload)
}

/// Encode a response packet.
/// Note: ApplicationData is hex-encoded per go-nethernet protocol.
pub fn encode_response(sender_id: u64, application_data: &[u8]) -> Vec<u8> {
    let header = Header {
        packet_id: ID_RESPONSE,
        sender_id,
    };

    let mut payload = header.encode();

    // Hex-encode the application data (per go-nethernet packet_response.go)
    let hex_encoded = hex::encode(application_data);

    // Write hex-encoded data with u32 length prefix
    payload.put_u32_le(hex_encoded.len() as u32);
    payload.extend_from_slice(hex_encoded.as_bytes());

    wrap_packet(&payload)
}

/// LE u64 recipient and u32 byte length: upstream `discovery/packet_message.go:MessagePacket.Write`.
pub fn encode_message(sender_id: u64, recipient_id: u64, data: &str) -> Vec<u8> {
    let header = Header {
        packet_id: ID_MESSAGE,
        sender_id,
    };

    let mut payload = header.encode();
    payload.put_u64_le(recipient_id);
    // Use u32 prefix for large SDP data (SDPs are typically 2000+ bytes)
    write_string_u32(&mut payload, data);

    wrap_packet(&payload)
}

/// Wrap payload with length, encrypt, and add HMAC.
fn wrap_packet(payload: &[u8]) -> Vec<u8> {
    // Inclusive prefix: upstream `discovery/packet.go:Marshal`.
    let mut with_length = Vec::with_capacity(2 + payload.len());
    with_length.put_u16_le((payload.len() + 2) as u16);
    with_length.extend_from_slice(payload);

    // Encrypt
    let encrypted = crypto::encrypt(&with_length);

    // Prepend HMAC
    let hmac = crypto::hmac_checksum(&with_length);
    let mut result = Vec::with_capacity(32 + encrypted.len());
    result.extend_from_slice(&hmac);
    result.extend_from_slice(&encrypted);

    result
}

/// Decode upstream `discovery/packet.go:Unmarshal`, including the u16::MAX
/// nested-byte limit from `discovery/packet.go:readBytes`.
pub fn decode(data: &[u8]) -> Result<(Packet, u64), &'static str> {
    if data.len() < 32 {
        return Err("packet too short for HMAC");
    }

    // Split HMAC and encrypted data
    let hmac: [u8; 32] = data[..32].try_into().unwrap();
    let encrypted = &data[32..];

    // Decrypt
    let decrypted = crypto::decrypt(encrypted)?;

    // Verify HMAC
    if !crypto::verify_hmac(&decrypted, &hmac) {
        return Err("HMAC verification failed");
    }

    // Parse length and content
    if decrypted.len() < 2 {
        return Err("decrypted data too short");
    }
    let mut cursor = Cursor::new(decrypted.as_slice());
    // Upstream `discovery/packet.go:Unmarshal` ignores this legacy/inclusive length.
    let _length = cursor.get_u16_le();

    // Parse header
    let remaining = &decrypted[2..];
    let (header, header_len) = Header::decode(remaining).ok_or("invalid header")?;
    let body = &remaining[header_len..];

    // Parse packet body
    let packet = match header.packet_id {
        ID_REQUEST if body.is_empty() => Packet::Request,
        ID_RESPONSE => {
            if body.len() < 4 {
                return Err("response too short");
            }
            let mut cursor = Cursor::new(body);
            let app_len = cursor.get_u32_le() as usize;
            if app_len > u16::MAX as usize || cursor.remaining() != app_len {
                return Err("response application data truncated");
            }
            let mut app_data = vec![0u8; app_len];
            cursor.read_exact(&mut app_data).map_err(|_| "read error")?;
            Packet::Response(hex::decode(app_data).map_err(|_| "invalid response hex")?)
        }
        ID_MESSAGE => {
            if body.len() < 12 {
                return Err("message too short");
            }
            let mut cursor = Cursor::new(body);
            let recipient_id = cursor.get_u64_le();
            let declared_len = cursor.get_u32_le() as usize;
            if declared_len > u16::MAX as usize || cursor.remaining() < declared_len {
                return Err("message data truncated");
            }
            // Vanilla appends signal bytes past its declared range.
            // Upstream `discovery/packet_message.go:MessagePacket.Read`.
            let data =
                String::from_utf8(body[12..].to_vec()).map_err(|_| "invalid message data")?;
            Packet::Message(MessageData { recipient_id, data })
        }
        _ => return Err("unknown packet ID"),
    };

    Ok((packet, header.sender_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_wire_regressions() {
        let request = encode_request(u64::MAX);
        let plain = crypto::decrypt(&request[32..]).unwrap();
        assert_eq!(
            u16::from_le_bytes([plain[0], plain[1]]) as usize,
            plain.len()
        );
        let mut payload = Header {
            packet_id: ID_MESSAGE,
            sender_id: u64::MAX,
        }
        .encode();
        payload.put_u64_le(42);
        write_string_u32(&mut payload, "CONNECTREQUEST 18446744073709551615 ");
        payload.extend_from_slice(b"v=0\r\n");
        let (Packet::Message(message), sender) = decode(&wrap_packet(&payload)).unwrap() else {
            panic!("expected message");
        };
        assert_eq!(sender, u64::MAX);
        assert_eq!(message.recipient_id, 42);
        assert!(message.data.ends_with("v=0\r\n"));
        let (Packet::Response(data), _) = decode(&encode_response(1, b"raw")).unwrap() else {
            panic!("expected response");
        };
        assert_eq!(data, b"raw");
    }

    #[test]
    fn malformed_packets_are_rejected_without_panics() {
        let valid = encode_message(1, 2, &"x".repeat(4096));
        for len in 0..valid.len() {
            assert!(
                decode(&valid[..len]).is_err(),
                "accepted truncated length {len}"
            );
        }
        let mut corrupted = valid;
        corrupted[0] ^= 1;
        assert!(decode(&corrupted).is_err());
        let mut payload = Header {
            packet_id: ID_REQUEST,
            sender_id: 1,
        }
        .encode();
        payload.push(1);
        assert!(decode(&wrap_packet(&payload)).is_err());
        let mut payload = Header {
            packet_id: ID_RESPONSE,
            sender_id: 1,
        }
        .encode();
        write_string_u32(&mut payload, "zz");
        assert!(decode(&wrap_packet(&payload)).is_err());
        for id in [ID_RESPONSE, ID_MESSAGE] {
            let mut payload = Header {
                packet_id: id,
                sender_id: 1,
            }
            .encode();
            if id == ID_MESSAGE {
                payload.put_u64_le(2);
            }
            payload.put_u32_le(u32::MAX);
            assert!(decode(&wrap_packet(&payload)).is_err());
        }
    }

    #[test]
    fn server_data_v7_fixture_and_all_truncations() {
        // discovery/server_data.go:ServerData.MarshalBinary; binary.go:writeVarint32.
        let data = ServerData::builder()
            .name("s")
            .level("l")
            .players(1, 10)
            .build();
        let expected = [7, 1, b's', 0, 0, 1, b'l', 2, 20, 0, 0, 0, 1, 1, 0, 8];
        assert_eq!(data.encode(), expected);
        for len in 0..expected.len() {
            assert!(ServerData::decode(&expected[..len]).is_none());
        }
        let mut trailing = expected.to_vec();
        trailing.push(0);
        assert!(ServerData::decode(&trailing).is_none());
    }

    #[test]
    fn test_request_roundtrip() {
        let sender_id = 12345u64;
        let encoded = encode_request(sender_id);
        let (packet, decoded_sender) = decode(&encoded).unwrap();
        assert!(matches!(packet, Packet::Request));
        assert_eq!(decoded_sender, sender_id);
    }

    #[test]
    fn test_response_roundtrip() {
        let sender_id = 67890u64;
        let app_data = b"test server data";
        let encoded = encode_response(sender_id, app_data);
        let (packet, decoded_sender) = decode(&encoded).unwrap();
        assert_eq!(decoded_sender, sender_id);
        if let Packet::Response(data) = packet {
            assert_eq!(data, app_data);
        } else {
            panic!("expected Response packet");
        }
    }

    #[test]
    fn test_message_roundtrip() {
        let sender_id = 111u64;
        let recipient_id = 222u64;
        let data = "CONNECTREQUEST 123 sdp-data-here";
        let encoded = encode_message(sender_id, recipient_id, data);
        let (packet, decoded_sender) = decode(&encoded).unwrap();
        assert_eq!(decoded_sender, sender_id);
        if let Packet::Message(msg) = packet {
            assert_eq!(msg.recipient_id, recipient_id);
            assert_eq!(msg.data, data);
        } else {
            panic!("expected Message packet");
        }
    }

    #[test]
    fn test_server_data_roundtrip() {
        let data = ServerData {
            server_name: "Test Server".into(),
            level_name: "Test World".into(),
            game_type: 1,
            player_count: 5,
            max_player_count: 20,
            editor_world: false,
            hardcore: true,
            transport_layer: TransportLayer::NetherNet,
            connection_type: 4,
            ..Default::default()
        };
        let encoded = data.encode();
        let decoded = ServerData::decode(&encoded).unwrap();
        assert_eq!(decoded.server_name, data.server_name);
        assert_eq!(decoded.level_name, data.level_name);
        assert_eq!(decoded.player_count, data.player_count);
        assert_eq!(decoded.transport_layer, TransportLayer::NetherNet);
    }
}
