#[instrument(skip(cursor, session), level = "debug")]
fn decode_packets(
    mut cursor: Bytes,
    session: &BedrockSession,
) -> Result<Vec<McpePacket>, JolyneError> {
    let mut packets = Vec::new();
    while cursor.has_remaining() {
        let (header, data) = McpePacketData::decode_inner(&mut cursor, session.into())?;
        packets.push(McpePacket { header, data });
    }
    Ok(packets)
}

use crate::error::{JolyneError, ProtocolError};
use crate::protocol::types::mcpe::{GAME_PACKET_ID as GAME_FRAME_ID, McpePacket, McpePacketData};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use std::io::{ErrorKind, Read};
use std::slice;
use tracing::{debug, instrument, warn};
use valentine::bedrock::context::BedrockSession;
use valentine::protocol::wire as bedrock_wire;

pub const BATCH_PACKET_ID: u8 = 0xFE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BatchCompression {
    Deflate = 0x00,
    Snappy = 0x01,
    None = 0xFF,
}

impl BatchCompression {
    pub fn try_from_u8(v: u8) -> Option<Self> {
        match v {
            0x00 => Some(Self::Deflate),
            0x01 => Some(Self::Snappy),
            0xFF => Some(Self::None),
            _ => None,
        }
    }
}

/// Helper to stream decompress while enforcing an optional maximum output size.
fn decompress_with_guard<R: Read>(
    mut reader: R,
    max_decompressed_size: Option<usize>,
) -> Result<Vec<u8>, std::io::Error> {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let new_len = out.len() + n;
        if let Some(max) = max_decompressed_size {
            if new_len > max {
                return Err(std::io::Error::new(
                    ErrorKind::InvalidData,
                    format!("decompressed data exceeds max size of {max} bytes"),
                ));
            }
        }
        out.extend_from_slice(&buf[..n]);
    }
    Ok(out)
}

fn log_payload_probe(compressed_len: Option<usize>, payload: &Bytes) {
    let preview_len = payload.len().min(16);
    let preview: Vec<u8> = payload.iter().take(preview_len).copied().collect();
    let first_declared_len = {
        let mut tmp = payload.clone();
        bedrock_wire::read_var_u32(&mut tmp).ok()
    };
    debug!(
        compressed_len,
        decompressed_len = payload.len(),
        first_bytes = ?preview,
        first_declared_packet_len = first_declared_len,
        "decode_batch payload probe"
    );
}

fn decode_payload(
    payload: Bytes,
    session: &BedrockSession,
) -> Result<Vec<McpePacket>, JolyneError> {
    if payload.first().copied() == Some(GAME_FRAME_ID) {
        let mut buf = payload.clone();
        let (header, data) =
            McpePacketData::decode_game_frame(&mut buf, session.into()).map_err(JolyneError::Io)?;
        return Ok(vec![McpePacket { header, data }]);
    }
    decode_packets(payload, session)
}

/// Decodes a Batch Packet (0xFE) payload into a list of McpePackets.
#[instrument(skip(buf, session), level = "debug")]
pub fn decode_batch(
    buf: &mut Bytes,
    session: &BedrockSession,
    compression_enabled: bool,
    max_decompressed_size: Option<usize>,
) -> Result<Vec<McpePacket>, JolyneError> {
    if buf.is_empty() {
        return Ok(vec![]);
    }

    let packet_id = buf.get_u8();
    if packet_id != BATCH_PACKET_ID {
        return Err(ProtocolError::InvalidBatchId(format!(
            "expected 0xFE, got 0x{:02x}",
            packet_id
        ))
        .into());
    }

    let payload_raw = buf.clone();

    // Strict Mode: If compression is enabled, we EXPECT a valid algorithm byte.
    if compression_enabled {
        if payload_raw.is_empty() {
            return Err(JolyneError::Protocol(ProtocolError::UnexpectedHandshake(
                "Empty compressed batch payload".to_string(),
            )));
        }

        let alg_byte = payload_raw[0];
        let alg = BatchCompression::try_from_u8(alg_byte).ok_or_else(|| {
            JolyneError::Protocol(ProtocolError::UnexpectedHandshake(format!(
                "Unknown compression algorithm: 0x{:02x} ({})",
                alg_byte, alg_byte
            )))
        })?;

        let compressed = payload_raw.slice(1..);

        // Size Check
        if let Some(max) = max_decompressed_size {
            // Rough check: compressed size shouldn't exceed max (it usually shrinks, but overhead exists)
            // Ideally we check *decompressed* size during stream.
            if compressed.len() > max {
                warn!("Compressed payload large: {}", compressed.len());
            }
        }

        match alg {
            BatchCompression::Deflate => {
                let decompressed = decompress_with_guard(
                    DeflateDecoder::new(compressed.as_ref()),
                    max_decompressed_size,
                )
                .map_err(|e| ProtocolError::DecompressionFailed(e.to_string()))?;

                let payload = Bytes::from(decompressed);
                log_payload_probe(Some(compressed.len()), &payload);
                decode_payload(payload, session)
            }
            BatchCompression::None => {
                log_payload_probe(Some(compressed.len()), &compressed);
                decode_payload(compressed, session)
            }
            BatchCompression::Snappy => Err(ProtocolError::UnexpectedHandshake(
                "Snappy compression not implemented".into(),
            )
            .into()),
        }
    } else {
        // raw packets (before NetworkSettings) are just [0xFE] [Payload].

        if let Some(max) = max_decompressed_size
            && payload_raw.len() > max
        {
            return Err(JolyneError::Protocol(ProtocolError::UnexpectedHandshake(
                format!(
                    "Batch payload exceeds max decompressed size ({} > {})",
                    payload_raw.len(),
                    max
                ),
            )));
        }
        log_payload_probe(None, &payload_raw);
        decode_payload(payload_raw, session)
    }
}

/// Encodes a single packet into a Batch Packet.
pub fn encode_batch(
    packet: &McpePacket,
    compression_enabled: bool,
    compression_level: u32,
    compression_threshold: u16,
) -> Result<Bytes, JolyneError> {
    encode_batch_multi(
        slice::from_ref(packet),
        compression_enabled,
        compression_level,
        compression_threshold,
    )
}

/// Encodes multiple packets into a single Batch Packet.
pub fn encode_batch_multi(
    packets: &[McpePacket],
    compression_enabled: bool,
    compression_level: u32,
    compression_threshold: u16,
) -> Result<Bytes, JolyneError> {
    let mut packet_buf = BytesMut::new();
    for packet in packets {
        packet
            .data
            .encode_inner(
                &mut packet_buf,
                packet.header.from_subclient,
                packet.header.to_subclient,
            )
            .map_err(JolyneError::Io)?;
    }

    let uncompressed = packet_buf.freeze();

    let should_compress = compression_enabled
        && compression_level > 0
        && uncompressed.len() >= compression_threshold as usize;

    let payload = if compression_enabled {
        if should_compress {
            // Deflate (0x00)
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(compression_level));
            std::io::Write::write_all(&mut encoder, &uncompressed).map_err(JolyneError::Io)?;
            let compressed = encoder.finish().map_err(JolyneError::Io)?;

            let mut out = BytesMut::with_capacity(1 + compressed.len());
            out.put_u8(BatchCompression::Deflate as u8);
            out.extend_from_slice(&compressed);
            out.freeze()
        } else {
            // None (0xFF)
            let mut out = BytesMut::with_capacity(1 + uncompressed.len());
            out.put_u8(BatchCompression::None as u8);
            out.extend_from_slice(&uncompressed);
            out.freeze()
        }
    } else {
        // No Marker
        uncompressed.clone()
    };

    let mut batch = BytesMut::with_capacity(1 + payload.len());
    batch.put_u8(BATCH_PACKET_ID);
    batch.extend_from_slice(&payload);
    Ok(batch.freeze())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::packets::{PacketPlayStatus, PacketPlayStatusStatus};

    #[test]
    fn decode_batch_rejects_wrong_id() {
        let mut buf = Bytes::from_static(&[0x00, 0x01, 0x02]);
        let session = BedrockSession { shield_item_id: 0 };
        let err = decode_batch(&mut buf, &session, false, None).expect_err("should fail");
        assert!(matches!(
            err,
            JolyneError::Protocol(ProtocolError::InvalidBatchId(_))
        ));
    }

    #[test]
    fn decode_batch_rejects_empty_compressed_payload() {
        let mut buf = Bytes::from_static(&[BATCH_PACKET_ID]);
        let session = BedrockSession { shield_item_id: 0 };
        // Valid compression enabled -> expects alg byte. Empty -> fail.
        let err = decode_batch(&mut buf, &session, true, None).expect_err("should fail");
        assert!(matches!(
            err,
            JolyneError::Protocol(ProtocolError::UnexpectedHandshake(_))
        ));
    }

    #[test]
    fn decode_batch_rejects_unknown_alg() {
        let mut buf = Bytes::from_static(&[BATCH_PACKET_ID, 0xBA, 0x00]); // 0xBA = 186
        let session = BedrockSession { shield_item_id: 0 };
        let err = decode_batch(&mut buf, &session, true, None).expect_err("should fail");
        // Should catch our new explicit check
        if let JolyneError::Protocol(ProtocolError::UnexpectedHandshake(msg)) = err {
            assert!(msg.contains("Unknown compression algorithm: 0xba"));
        } else {
            panic!("Wrong error type: {:?}", err);
        }
    }

    #[test]
    fn encode_decode_roundtrip_compressed() {
        let session = BedrockSession { shield_item_id: 0 };
        let packet = McpePacket::from(PacketPlayStatus {
            status: PacketPlayStatusStatus::LoginSuccess,
        });

        let batch = encode_batch(&packet, true, 7, 0).expect("encode");
        let mut buf = batch.clone();

        let decoded = decode_batch(&mut buf, &session, true, Some(1024)).expect("decode");
        assert_eq!(decoded.len(), 1);
        assert!(matches!(
            decoded[0].data,
            McpePacketData::PacketPlayStatus(ref s) if s.status == PacketPlayStatusStatus::LoginSuccess
        ));
    }
}
