//! Packet types for LAN discovery.
//!
//! Packets are encrypted with AES-ECB and verified with HMAC-SHA256.

use std::io::{Cursor, Read};

use bytes::{Buf, BufMut};

use super::crypto;

/// Packet IDs.
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
    /// Recipient network ID (parsed from protocol, available for future use).
    #[allow(dead_code)]
    pub recipient_id: u64,
    pub data: String,
}

/// Transport layer used for NetherNet connections.
///
/// This value is encoded in discovery responses and session properties.
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
/// Format matches go-nethernet/discovery/server_data.go exactly.
#[derive(Debug, Clone)]
pub struct ServerData {
    pub server_name: String,
    pub level_name: String,
    pub game_type: u8,
    pub player_count: i32,
    pub max_player_count: i32,
    pub editor_world: bool,
    pub hardcore: bool,
    pub transport_layer: TransportLayer,
    /// Connection type: 4 = LAN discovery, 3 = WebRTC/Xbox.
    pub connection_type: u8,
}

impl Default for ServerData {
    fn default() -> Self {
        Self {
            server_name: String::new(),
            level_name: String::new(),
            game_type: 0,
            player_count: 1, // Must be >= 1 or world won't show
            max_player_count: 10,
            editor_world: false,
            hardcore: false,
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

    /// Set game type (0 = Survival, 1 = Creative, 2 = Adventure).
    pub fn game_type(mut self, game_type: u8) -> Self {
        self.data.game_type = game_type;
        self
    }

    /// Build the ServerData.
    pub fn build(self) -> ServerData {
        self.data
    }
}

/// ServerData version as per go-nethernet
const SERVER_DATA_VERSION: u8 = 4;

impl ServerData {
    /// Encode server data to binary format (per go-nethernet server_data.go).
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // Version byte (must be 4)
        buf.put_u8(SERVER_DATA_VERSION);

        // Strings with u8 length prefix
        write_string_u8(&mut buf, &self.server_name);
        write_string_u8(&mut buf, &self.level_name);

        // GameType is left-shifted by 1
        buf.put_u8(self.game_type << 1);

        // Player counts as i32 LE
        buf.put_i32_le(self.player_count);
        buf.put_i32_le(self.max_player_count);

        // Booleans
        buf.put_u8(self.editor_world as u8);
        buf.put_u8(self.hardcore as u8);

        // TransportLayer and ConnectionType are left-shifted by 1
        buf.put_u8(u8::from(self.transport_layer) << 1);
        buf.put_u8(self.connection_type << 1);

        buf
    }

    /// Decode server data from binary format.
    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.is_empty() {
            return None;
        }

        let mut cursor = Cursor::new(data);

        // Version byte
        let version = cursor.get_u8();
        if version != SERVER_DATA_VERSION {
            return None;
        }

        let server_name = read_string_u8(&mut cursor)?;
        let level_name = read_string_u8(&mut cursor)?;

        if cursor.remaining() < 12 {
            return None;
        }

        let game_type = cursor.get_u8() >> 1;
        let player_count = cursor.get_i32_le();
        let max_player_count = cursor.get_i32_le();
        let editor_world = cursor.get_u8() != 0;
        let hardcore = cursor.get_u8() != 0;
        let transport_raw = cursor.get_u8() >> 1;
        let connection_type = cursor.get_u8() >> 1;

        Some(Self {
            server_name,
            level_name,
            game_type,
            player_count,
            max_player_count,
            editor_world,
            hardcore,
            transport_layer: TransportLayer::from_u8(transport_raw).unwrap_or_default(),
            connection_type,
        })
    }
}

fn write_string_u32(buf: &mut Vec<u8>, s: &str) {
    buf.put_u32_le(s.len() as u32);
    buf.extend_from_slice(s.as_bytes());
}

fn read_string_u32(cursor: &mut Cursor<&[u8]>) -> Option<String> {
    if cursor.remaining() < 4 {
        return None;
    }
    let len = cursor.get_u32_le() as usize;
    if cursor.remaining() < len {
        return None;
    }
    let mut buf = vec![0u8; len];
    cursor.read_exact(&mut buf).ok()?;
    String::from_utf8(buf).ok()
}

fn write_string_u8(buf: &mut Vec<u8>, s: &str) {
    buf.put_u8(s.len() as u8);
    buf.extend_from_slice(s.as_bytes());
}

fn read_string_u8(cursor: &mut Cursor<&[u8]>) -> Option<String> {
    if cursor.remaining() < 1 {
        return None;
    }
    let len = cursor.get_u8() as usize;
    if cursor.remaining() < len {
        return None;
    }
    let mut buf = vec![0u8; len];
    cursor.read_exact(&mut buf).ok()?;
    String::from_utf8(buf).ok()
}

/// Header structure for all packets.
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

/// Encode a message packet.
/// Note: Uses u32 length prefix for data to support large SDP payloads.
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
    // Prepend length
    let mut with_length = Vec::with_capacity(2 + payload.len());
    with_length.put_u16_le(payload.len() as u16);
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

/// Decode a packet from raw bytes.
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
    let _length = cursor.get_u16_le();

    // Parse header
    let remaining = &decrypted[2..];
    let (header, header_len) = Header::decode(remaining).ok_or("invalid header")?;
    let body = &remaining[header_len..];

    // Parse packet body
    let packet = match header.packet_id {
        ID_REQUEST => Packet::Request,
        ID_RESPONSE => {
            if body.len() < 4 {
                return Err("response too short");
            }
            let mut cursor = Cursor::new(body);
            let app_len = cursor.get_u32_le() as usize;
            if cursor.remaining() < app_len {
                return Err("response application data truncated");
            }
            let mut app_data = vec![0u8; app_len];
            cursor.read_exact(&mut app_data).map_err(|_| "read error")?;
            Packet::Response(app_data)
        }
        ID_MESSAGE => {
            if body.len() < 12 {
                return Err("message too short");
            }
            let mut cursor = Cursor::new(body);
            let recipient_id = cursor.get_u64_le();
            let remaining = &body[8..];
            // Use u32 prefix for large SDP data
            let data =
                read_string_u32(&mut Cursor::new(remaining)).ok_or("invalid message data")?;
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
            assert_eq!(hex::decode(data).unwrap(), app_data);
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
        };
        let encoded = data.encode();
        let decoded = ServerData::decode(&encoded).unwrap();
        assert_eq!(decoded.server_name, data.server_name);
        assert_eq!(decoded.level_name, data.level_name);
        assert_eq!(decoded.player_count, data.player_count);
        assert_eq!(decoded.transport_layer, TransportLayer::NetherNet);
    }
}
