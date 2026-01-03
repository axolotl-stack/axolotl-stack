//! Raw packet types for proxies and partial packet inspection.
//!
//! This module provides [`RawPacket`], a packet representation that parses only the
//! header (packet ID + subclient info) while keeping the body as raw bytes.
//! This is useful for:
//! - **Proxies**: Inspect packet IDs without full decode, then forward as-is
//! - **Filtering**: Match on packet type without parse overhead
//! - **Passthrough**: Forward unknown/unimplemented packets transparently

use bytes::{Buf, Bytes, BytesMut};
use valentine::bedrock::codec::BedrockCodec;
use valentine::bedrock::context::BedrockSession;
use valentine::protocol::wire;

use crate::error::{JolyneError, ProtocolError};
use crate::valentine::mcpe::{GameHeader, McpePacket, McpePacketData, McpePacketName};

/// A packet with only the header parsed, body kept as raw bytes.
///
/// Useful for proxies that need to peek at packet IDs without full decode,
/// then forward packets as raw bytes. This avoids the overhead of parsing
/// and re-serializing packet bodies.
///
/// # Example
/// ```ignore
/// match stream.recv_packet_raw().await?.id {
///     McpePacketName::PacketText => { /* snoop on chat */ },
///     _ => stream.send_packet_raw(raw).await?, // forward as-is
/// }
/// ```
#[derive(Debug, Clone)]
pub struct RawPacket {
    /// Parsed header with subclient info.
    pub header: GameHeader,
    /// Packet ID for easy pattern matching.
    pub id: McpePacketName,
    /// Raw body bytes (everything after the varint header).
    /// This does NOT include the length prefix or header varint—just the payload.
    body: Bytes,
    /// Complete inner frame bytes for re-encoding.
    /// Format: `[Length varint][Header varint][Body]`
    inner_frame: Bytes,
}

impl RawPacket {
    /// Decode the raw bytes into a full [`McpePacket`] on demand.
    ///
    /// This is useful when you decide you need the full packet contents
    /// after initially receiving it as raw bytes.
    pub fn decode(self, session: &BedrockSession) -> Result<McpePacket, JolyneError> {
        // Debug: Log raw bytes for TextPacket
        if self.id == McpePacketName::PacketText {
            let body_preview: Vec<u8> = self.body.iter().take(64).copied().collect();
            tracing::warn!(
                packet_id = ?self.id,
                body_len = self.body.len(),
                body_hex = ?body_preview,
                "TextPacket raw bytes before decode"
            );
        }

        let mut buf = self.inner_frame;
        let (header, data) = McpePacketData::decode_inner(&mut buf, session.into())?;
        Ok(McpePacket::new(header, data))
    }

    /// Returns the raw body bytes (payload after header).
    pub fn body(&self) -> &Bytes {
        &self.body
    }

    /// Consumes self and returns the complete inner frame bytes.
    ///
    /// Format: `[Length varint][Header varint][Body]`
    /// Ready for batching/encoding.
    pub fn into_inner_frame(self) -> Bytes {
        self.inner_frame
    }

    /// Returns a reference to the inner frame bytes.
    pub fn inner_frame(&self) -> &Bytes {
        &self.inner_frame
    }
}

/// Decodes a single packet entry from batch payload into a [`RawPacket`].
///
/// Format: `[Length varint][Header varint][Body]`
///
/// Returns the RawPacket and advances the cursor past this entry.
pub fn decode_packet_raw(cursor: &mut Bytes) -> Result<RawPacket, JolyneError> {
    // Remember start position to capture full frame
    let frame_start = cursor.clone();

    // Read length
    let declared_len = wire::read_var_u32(cursor)? as usize;
    if cursor.remaining() < declared_len {
        return Err(JolyneError::Protocol(ProtocolError::UnexpectedHandshake(
            format!(
                "declared packet length {} exceeds available {}",
                declared_len,
                cursor.remaining()
            ),
        )));
    }

    // Calculate frame size: length varint + declared payload
    let len_varint_size = frame_start.remaining() - cursor.remaining();
    let frame_size = len_varint_size + declared_len;
    let inner_frame = frame_start.slice(..frame_size);

    // Parse header from payload
    let mut payload = cursor.slice(..declared_len);
    let header_raw = wire::read_var_u32(&mut payload)?;

    // Decode packet ID directly using BedrockCodec
    // The header is: [10-bit packet_id][2-bit from_subclient][2-bit to_subclient]
    let id_raw = header_raw & 0x3FF;
    let id = McpePacketName::decode(&mut &id_raw.to_le_bytes()[..], ()).map_err(|e| {
        JolyneError::Protocol(ProtocolError::UnexpectedHandshake(format!(
            "unknown packet ID {}: {}",
            id_raw, e
        )))
    })?;

    let from_subclient = (header_raw >> 10) & 0x3;
    let to_subclient = (header_raw >> 12) & 0x3;

    let header = GameHeader {
        id,
        from_subclient,
        to_subclient,
    };

    // Body is the remaining payload after header varint
    let body = payload.clone();

    // Advance main cursor past this packet
    cursor.advance(declared_len);

    Ok(RawPacket {
        header,
        id,
        body,
        inner_frame,
    })
}

/// Decodes all packets from a decompressed batch payload into [`RawPacket`]s.
pub(crate) fn decode_packets_raw(mut cursor: Bytes) -> Result<Vec<RawPacket>, JolyneError> {
    let mut packets = Vec::new();
    while cursor.has_remaining() {
        packets.push(decode_packet_raw(&mut cursor)?);
    }
    Ok(packets)
}

/// Encodes a slice of [`RawPacket`]s into batch payload bytes.
///
/// This just concatenates the inner frames—caller handles compression/batching.
pub(crate) fn encode_packets_raw(packets: &[RawPacket]) -> Bytes {
    let total_len: usize = packets.iter().map(|p| p.inner_frame.len()).sum();
    let mut buf = BytesMut::with_capacity(total_len);
    for packet in packets {
        buf.extend_from_slice(&packet.inner_frame);
    }
    buf.freeze()
}
