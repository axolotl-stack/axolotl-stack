//! LAN discovery packet codec.
//!
//! Wire layout (upstream `discovery/packet.go:Marshal/Unmarshal`):
//! `HMAC-SHA256(plain) [32] | AES-256-ECB-PKCS7(plain)` where
//! `plain = u16 LE inclusive length | u16 LE packet ID | u64 LE sender | 8 padding bytes | body`.
//!
//! Encoding writes the plaintext once, directly into the output buffer, then
//! checksums and encrypts it in place: one exact-size allocation per packet.
//! Decoding decrypts one copy of the ciphertext in place, verifies the HMAC in
//! constant time before parsing, and reuses that buffer for message text.

use bytes::BufMut;

use super::crypto::{self, CHECKSUM_LEN};

/// Upstream `discovery/packet.go:Unmarshal` (`IDRequestPacket` through `IDMessagePacket`).
const ID_REQUEST: u16 = 0;
const ID_RESPONSE: u16 = 1;
const ID_MESSAGE: u16 = 2;

/// Inclusive length prefix preceding the header.
const LENGTH_LEN: usize = 2;
/// u16 packet ID, u64 sender and eight padding bytes: upstream `discovery/packet.go:Header.Write`.
const HEADER_LEN: usize = 18;
/// Plaintext offset of the packet body.
const BODY_OFFSET: usize = LENGTH_LEN + HEADER_LEN;
/// LE u64 recipient and u32 declared length preceding message text.
const MESSAGE_PREFIX_LEN: usize = 12;

/// Decoded packet types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    Request,
    /// Decoded application data (the wire carries it hex-encoded).
    Response(Vec<u8>),
    Message(MessageData),
}

/// Message packet data.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Starts a packet buffer with capacity for the whole sealed packet: a zeroed
/// checksum slot, the length prefix slot and the header
/// (upstream `discovery/packet.go:Header.Write`).
fn begin(packet_id: u16, sender_id: u64, body_len: usize) -> Vec<u8> {
    let plain_len = BODY_OFFSET + body_len;
    let mut out = Vec::with_capacity(CHECKSUM_LEN + crypto::padded_len(plain_len));
    out.extend_from_slice(&[0; CHECKSUM_LEN + LENGTH_LEN]);
    out.put_u16_le(packet_id);
    out.put_u64_le(sender_id);
    out.put_u64_le(0); // 8 bytes padding
    out
}

/// Writes the inclusive length prefix, then checksums and encrypts the
/// plaintext after the checksum slot in place: upstream `discovery/packet.go:Marshal`.
fn seal(mut out: Vec<u8>) -> Vec<u8> {
    let plain = CHECKSUM_LEN..out.len();
    // Upstream `uint16(buf.Len())+2` counts the prefix itself and wraps like Go.
    let inclusive_len = plain.len() as u16;
    out[CHECKSUM_LEN..CHECKSUM_LEN + LENGTH_LEN].copy_from_slice(&inclusive_len.to_le_bytes());
    let checksum = crypto::hmac_checksum(&out[plain]);
    crypto::encrypt_in_place(&mut out, CHECKSUM_LEN);
    out[..CHECKSUM_LEN].copy_from_slice(&checksum);
    out
}

/// Encode a request packet.
pub fn encode_request(sender_id: u64) -> Vec<u8> {
    seal(begin(ID_REQUEST, sender_id, 0))
}

/// Encode a response packet.
/// ApplicationData is written hex-encoded with a u32 LE length, per go-nethernet
/// `discovery/packet_response.go:ResponsePacket.Write`.
pub fn encode_response(sender_id: u64, application_data: &[u8]) -> Vec<u8> {
    let hex_len = application_data.len() * 2;
    let mut out = begin(ID_RESPONSE, sender_id, 4 + hex_len);
    out.put_u32_le(hex_len as u32);
    let start = out.len();
    out.resize(start + hex_len, 0);
    hex::encode_to_slice(application_data, &mut out[start..])
        .expect("hex output sized to twice the input");
    seal(out)
}

/// LE u64 recipient and u32 byte length: upstream `discovery/packet_message.go:MessagePacket.Write`.
pub fn encode_message(sender_id: u64, recipient_id: u64, data: &str) -> Vec<u8> {
    let mut out = begin(ID_MESSAGE, sender_id, MESSAGE_PREFIX_LEN + data.len());
    out.put_u64_le(recipient_id);
    // Use u32 prefix for large SDP data (SDPs are typically 2000+ bytes)
    write_string_u32(&mut out, data);
    seal(out)
}

#[inline]
fn u16_le(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

#[inline]
fn u32_le(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4-byte slice"))
}

#[inline]
fn u64_le(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("8-byte slice"))
}

/// Decode upstream `discovery/packet.go:Unmarshal`, including the u16::MAX
/// nested-byte limit from `discovery/packet.go:readBytes`.
///
/// The checksum is verified (constant time) before any field is parsed.
/// Returns the packet and the sender's network ID.
pub fn decode(data: &[u8]) -> Result<(Packet, u64), &'static str> {
    let Some((checksum, encrypted)) = data.split_first_chunk::<CHECKSUM_LEN>() else {
        return Err("packet too short for HMAC");
    };

    let mut plain = encrypted.to_vec();
    let plain_len = crypto::decrypt_in_place(&mut plain)?;
    plain.truncate(plain_len);

    if !crypto::verify_hmac(&plain, checksum) {
        return Err("HMAC verification failed");
    }

    // Upstream `discovery/packet.go:Unmarshal` ignores the legacy/inclusive length.
    if plain.len() < LENGTH_LEN {
        return Err("decrypted data too short");
    }
    if plain.len() < BODY_OFFSET {
        return Err("invalid header");
    }
    let packet_id = u16_le(&plain, LENGTH_LEN);
    let sender_id = u64_le(&plain, LENGTH_LEN + 2);
    let body = &plain[BODY_OFFSET..];

    let packet = match packet_id {
        ID_REQUEST if body.is_empty() => Packet::Request,
        ID_RESPONSE => {
            if body.len() < 4 {
                return Err("response too short");
            }
            let app_len = u32_le(body, 0) as usize;
            let app_data = &body[4..];
            if app_len > u16::MAX as usize || app_data.len() != app_len {
                return Err("response application data truncated");
            }
            Packet::Response(hex::decode(app_data).map_err(|_| "invalid response hex")?)
        }
        ID_MESSAGE => {
            if body.len() < MESSAGE_PREFIX_LEN {
                return Err("message too short");
            }
            let recipient_id = u64_le(body, 0);
            let declared_len = u32_le(body, 8) as usize;
            if declared_len > u16::MAX as usize || body.len() - MESSAGE_PREFIX_LEN < declared_len {
                return Err("message data truncated");
            }
            // Vanilla appends signal bytes past its declared range.
            // Upstream `discovery/packet_message.go:MessagePacket.Read`.
            // The text is moved to the front of the decrypted buffer, which then
            // becomes the String without another allocation.
            plain.drain(..BODY_OFFSET + MESSAGE_PREFIX_LEN);
            let data = String::from_utf8(plain).map_err(|_| "invalid message data")?;
            Packet::Message(MessageData { recipient_id, data })
        }
        _ => return Err("unknown packet ID"),
    };

    Ok((packet, sender_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Raw header writer for hand-built (including malformed) payloads.
    struct Header {
        packet_id: u16,
        sender_id: u64,
    }

    impl Header {
        fn encode(&self) -> Vec<u8> {
            let mut buf = Vec::with_capacity(HEADER_LEN);
            buf.put_u16_le(self.packet_id);
            buf.put_u64_le(self.sender_id);
            buf.put_u64_le(0);
            buf
        }
    }

    /// Seals an arbitrary header+body payload exactly as the encoders do.
    fn wrap_packet(payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0; CHECKSUM_LEN + LENGTH_LEN];
        out.extend_from_slice(payload);
        seal(out)
    }

    /// Byte-exact vectors from df-mc/go-nethernet 215e464 `discovery.Marshal`
    /// (AES-ECB and HMAC are deterministic): encoders must reproduce them and
    /// the decoder must accept them, pinning both to the reference wire format.
    #[test]
    fn codec_matches_go_nethernet_marshal() {
        let cases = [
            (
                encode_message(1, 2, "CONNECTERROR 42 2"),
                "b3925f19dfcc0825cf5035b24f01b34205fa384729639bdf09de4c82c70ab46c7c81bebf50752f9b925421d27e8b8799e9441a820351e758c460a8b32124131d26fc5afa9db75f169bed1c7d1635e4ae1aed5fee47207c062512004edb5d55c8",
                Packet::Message(MessageData {
                    recipient_id: 2,
                    data: "CONNECTERROR 42 2".into(),
                }),
                1,
            ),
            (
                encode_request(0x0102030405060708),
                "a87c6e38b3ff0ef3514ada5d0d433d56709091080306399a2bd75d4c1fbd06562546e9f4ff4485881fd5cb97a655a701ff0f8709e13bf6858e2051c321db615e",
                Packet::Request,
                0x0102030405060708,
            ),
            (
                encode_response(7, b"raw"),
                "572814bbc4fe28cb59d0eff1e0450725e53c938d658d7eea5b99005b777b1b69f54042b07f2a1a52e3406974d8efda1368eaaa22ee025aae072489262d6e3c2c",
                Packet::Response(b"raw".to_vec()),
                7,
            ),
        ];
        for (encoded, go, packet, sender) in cases {
            assert_eq!(hex::encode(&encoded), go);
            assert_eq!(decode(&hex::decode(go).unwrap()).unwrap(), (packet, sender));
        }
    }

    #[test]
    fn upstream_wire_regressions() {
        let request = encode_request(u64::MAX);
        let mut plain = request[32..].to_vec();
        let plain_len = crypto::decrypt_in_place(&mut plain).unwrap();
        plain.truncate(plain_len);
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
