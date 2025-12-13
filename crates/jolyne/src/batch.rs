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
use tracing::{debug, instrument};
use valentine::bedrock::context::BedrockSession;
use valentine::protocol::wire as bedrock_wire;

pub const BATCH_PACKET_ID: u8 = 0xFE;
// 0 == "ZLib" per Bedrock naming, but payloads are raw DEFLATE streams.
const COMPRESSION_ALG_DEFLATE: u8 = 0x00; // vanilla expectation
const COMPRESSION_ALG_NONE: u8 = 0xFF;

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

fn log_decompressed_probe(alg_marker: Option<u8>, compressed_len: usize, payload: &Bytes) {
    let preview_len = payload.len().min(16);
    let preview: Vec<u8> = payload.iter().take(preview_len).copied().collect();
    let first_declared_len = {
        let mut tmp = payload.clone();
        bedrock_wire::read_var_u32(&mut tmp).ok()
    };
    debug!(
        alg_marker,
        compressed_len,
        decompressed_len = payload.len(),
        first_bytes = ?preview,
        first_declared_packet_len = first_declared_len,
        "decode_batch decompressed probe"
    );
}

fn log_first_packet_meta(payload: &Bytes) {
    let mut tmp = payload.clone();
    if let Ok(len) = bedrock_wire::read_var_u32(&mut tmp) {
        let mut pkt = tmp.clone();
        if pkt.remaining() >= len as usize {
            let mut first = pkt.split_to(len as usize);
            if let Ok(header_raw) = bedrock_wire::read_var_u32(&mut first) {
                let id_raw = header_raw & 0x3FF;
                debug!(
                    first_packet_declared_len = len,
                    first_packet_header_raw = header_raw,
                    first_packet_id = id_raw,
                    "decode_batch first packet meta"
                );
            }
        }
    }
}

fn log_first_packet_preview(payload: &Bytes) {
    let mut tmp = payload.clone();
    if let Ok(len) = bedrock_wire::read_var_u32(&mut tmp) {
        let mut pkt = tmp.clone();
        if pkt.remaining() >= len as usize {
            let first = pkt.split_to(len as usize);
            let preview_len = first.len().min(64);
            let preview: Vec<u8> = first.iter().take(preview_len).copied().collect();
            debug!(first_packet_preview = ?preview, "decode_batch first packet preview");
        }
    }
}

fn inspect_login_lengths(payload: &Bytes) {
    let mut tmp = payload.clone();
    if let Ok(len) = bedrock_wire::read_var_u32(&mut tmp) {
        let mut pkt = tmp.clone();
        if pkt.remaining() < len as usize {
            debug!(
                declared_len = len,
                remaining = pkt.remaining(),
                "inspect_login_lengths: declared exceeds remaining"
            );
            return;
        }
        let mut first = pkt.split_to(len as usize);
        match bedrock_wire::read_var_u32(&mut first) {
            Ok(header_raw) => {
                let packet_id = header_raw & 0x3FF;
                if packet_id != 1 {
                    return;
                }
                if first.remaining() < 4 {
                    debug!(
                        remaining = first.remaining(),
                        "inspect_login_lengths: missing protocol_version"
                    );
                    return;
                }
                let proto = first.get_i32();
                if first.remaining() < 4 {
                    debug!(
                        protocol_version = proto,
                        remaining = first.remaining(),
                        "inspect_login_lengths: missing identity len"
                    );
                    return;
                }
                let id_len = first.get_u32_le() as usize;
                let remaining_after_id_len = first.remaining();
                debug!(
                    protocol_version = proto,
                    identity_len = id_len,
                    remaining_after_id_len,
                    "inspect_login_lengths: identity length header"
                );
                if remaining_after_id_len < id_len + 4 {
                    debug!(
                        identity_len = id_len,
                        remaining_after_id_len,
                        "inspect_login_lengths: not enough for identity + client len"
                    );
                    return;
                }
                // Skip identity bytes.
                let _ = first.split_to(id_len.min(remaining_after_id_len));
                if first.remaining() < 4 {
                    debug!(
                        identity_len = id_len,
                        remaining_after_identity = first.remaining(),
                        "inspect_login_lengths: missing client len"
                    );
                    return;
                }
                let client_len = first.get_u32_le() as usize;
                let remaining_after_client_len = first.remaining();
                debug!(
                    protocol_version = proto,
                    identity_len = id_len,
                    client_len,
                    remaining_after_client_len,
                    "inspect_login_lengths: client length header"
                );
                if remaining_after_client_len < client_len {
                    debug!(
                        client_len,
                        remaining_after_client_len,
                        "inspect_login_lengths: not enough client bytes"
                    );
                }
            }
            Err(e) => {
                debug!(error = %e, "inspect_login_lengths: header read failed");
            }
        }
    }
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
/// Handles decompression if necessary (heuristic: if payload looks compressed).
///
/// Bedrock's batch packet is: [0xFE] [Payload]
/// Payload is usually raw Deflate compressed (vanilla/gophertunnel).
/// Decompressed payload is a sequence of: [VarU32 Length] [Packet Buffer]
/// Packet Buffer is: [Header] [Body] (which McpePacket::decode_inner handles, but decode_inner expects length prefix too?)
///
/// Wait, McpePacketData::decode_inner reads [Len] then [Header] then [Body].
/// So the sequence in batch is exactly what decode_inner expects?
/// "Decompressed payload is a sequence of: [VarU32 Length] [Packet Buffer]"
///
/// `decode_inner` reads a varu32 length, checks buffer, then reads that many bytes.
/// So `decode_inner` works perfectly on the stream of the batch payload!
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
    if compression_enabled {
        if payload_raw.is_empty() {
            return Err(JolyneError::Protocol(ProtocolError::UnexpectedHandshake(
                "Empty compressed batch payload".to_string(),
            )));
        }

        // Detect algorithm byte when present. Only 0x00/0xFF are valid markers; anything else
        // is treated as part of the compressed stream (no stripping) to avoid corrupting data.
        let (alg_marker, compressed) = match payload_raw.first().copied() {
            Some(a) if a == COMPRESSION_ALG_DEFLATE || a == COMPRESSION_ALG_NONE => {
                (Some(a), payload_raw.slice(1..))
            }
            _ => (None, payload_raw.clone()),
        };

        if let Some(max) = max_decompressed_size {
            if compressed.len() > max {
                return Err(JolyneError::Protocol(ProtocolError::UnexpectedHandshake(
                    format!(
                        "Compressed batch payload exceeds max size ({} > {})",
                        compressed.len(),
                        max
                    ),
                )));
            }
        }

        let compressed_len = compressed.len();
        let mut errors = Vec::new();

        let mut try_decode = |label: &str,
                              decompressed: Result<Vec<u8>, std::io::Error>|
         -> Option<Vec<McpePacket>> {
            match decompressed {
                Ok(bytes) => {
                    let payload = Bytes::from(bytes);
                    log_decompressed_probe(alg_marker, compressed_len, &payload);
                    match decode_payload(payload.clone(), session) {
                        Ok(pkts) => Some(pkts),
                        Err(e) => {
                            log_first_packet_meta(&payload);
                            log_first_packet_preview(&payload);
                            inspect_login_lengths(&payload);
                            errors.push(format!("{label} decode failed: {e}"));
                            None
                        }
                    }
                }
                Err(e) => {
                    errors.push(format!("{label} decompress failed: {e}"));
                    None
                }
            }
        };

        match alg_marker {
            Some(COMPRESSION_ALG_DEFLATE) => {
                if let Some(pkts) = try_decode(
                    "deflate",
                    decompress_with_guard(
                        DeflateDecoder::new(compressed.as_ref()),
                        max_decompressed_size,
                    ),
                ) {
                    return Ok(pkts);
                }
            }
            Some(COMPRESSION_ALG_NONE) => {
                log_payload_probe(Some(compressed_len), &compressed);
                return decode_payload(compressed, session);
            }
            None => {
                if let Some(pkts) = try_decode(
                    "deflate",
                    decompress_with_guard(
                        DeflateDecoder::new(compressed.as_ref()),
                        max_decompressed_size,
                    ),
                ) {
                    return Ok(pkts);
                }
                log_payload_probe(Some(compressed_len), &compressed);
                match decode_payload(compressed.clone(), session) {
                    Ok(pkts) => return Ok(pkts),
                    Err(e) => errors.push(format!("raw payload decode failed: {e}")),
                }
            }
            Some(_) => {
                // Unknown marker: treat as no marker and attempt normal fallbacks.
                if let Some(pkts) = try_decode(
                    "deflate",
                    decompress_with_guard(
                        DeflateDecoder::new(compressed.as_ref()),
                        max_decompressed_size,
                    ),
                ) {
                    return Ok(pkts);
                }
                log_payload_probe(Some(compressed_len), &compressed);
                match decode_payload(compressed.clone(), session) {
                    Ok(pkts) => return Ok(pkts),
                    Err(e) => errors.push(format!("raw payload decode failed: {e}")),
                }
            }
        }

        log_payload_probe(Some(compressed_len), &compressed);
        let detail = if errors.is_empty() {
            "failed to decode compressed batch".to_string()
        } else {
            errors.join("; ")
        };
        Err(ProtocolError::DecompressionFailed(detail).into())
    } else {
        if let Some(max) = max_decompressed_size {
            if payload_raw.len() > max {
                return Err(JolyneError::Protocol(ProtocolError::UnexpectedHandshake(
                    format!(
                        "Batch payload exceeds max decompressed size ({} > {})",
                        payload_raw.len(),
                        max
                    ),
                )));
            }
        }
        log_payload_probe(None, &payload_raw);
        return decode_payload(payload_raw, session);
    }
}

/// Encodes a single packet into a Batch Packet.
/// Currently always compresses if compression_level > 0.
pub fn encode_batch(
    packet: &McpePacket,
    compression_enabled: bool,
    compression_level: u32,
    compression_threshold: u16,
) -> Result<Bytes, JolyneError> {
    let mut packet_buf = BytesMut::new();
    // encode_inner writes [Len][Header][Body]
    packet
        .data
        .encode_inner(
            &mut packet_buf,
            packet.header.from_subclient,
            packet.header.to_subclient,
        )
        .map_err(JolyneError::Io)?;

    let uncompressed = packet_buf.freeze();

    let should_compress = compression_enabled
        && compression_level > 0
        && uncompressed.len() >= compression_threshold as usize;

    let payload = if compression_enabled && should_compress {
        // Vanilla: alg marker 0x00 + raw deflate stream.
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(compression_level));
        std::io::Write::write_all(&mut encoder, &uncompressed).map_err(JolyneError::Io)?;
        let compressed = encoder.finish().map_err(JolyneError::Io)?;
        let mut out = BytesMut::with_capacity(1 + compressed.len());
        out.put_u8(COMPRESSION_ALG_DEFLATE);
        out.extend_from_slice(&compressed);
        out.freeze()
    } else if compression_enabled {
        // Compression negotiated but under threshold: alg marker 0xFF + plain payload.
        let mut out = BytesMut::with_capacity(1 + uncompressed.len());
        out.put_u8(COMPRESSION_ALG_NONE);
        out.extend_from_slice(&uncompressed);
        out.freeze()
    } else {
        uncompressed.clone()
    };

    let mut batch = BytesMut::with_capacity(1 + payload.len());
    batch.put_u8(BATCH_PACKET_ID);
    batch.extend_from_slice(&payload);
    Ok(batch.freeze())
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

    let payload = if compression_enabled && should_compress {
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(compression_level));
        std::io::Write::write_all(&mut encoder, &uncompressed).map_err(JolyneError::Io)?;
        let compressed = encoder.finish().map_err(JolyneError::Io)?;
        let mut out = BytesMut::with_capacity(1 + compressed.len());
        out.put_u8(COMPRESSION_ALG_DEFLATE);
        out.extend_from_slice(&compressed);
        out.freeze()
    } else if compression_enabled {
        let mut out = BytesMut::with_capacity(1 + uncompressed.len());
        out.put_u8(COMPRESSION_ALG_NONE);
        out.extend_from_slice(&uncompressed);
        out.freeze()
    } else {
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
        let err = decode_batch(&mut buf, &session, true, None).expect_err("should fail");
        assert!(matches!(
            err,
            JolyneError::Protocol(ProtocolError::UnexpectedHandshake(_))
        ));
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

    #[test]
    fn encode_decode_roundtrip_uncompressed() {
        let session = BedrockSession { shield_item_id: 0 };
        let packet = McpePacket::from(PacketPlayStatus {
            status: PacketPlayStatusStatus::PlayerSpawn,
        });

        let batch = encode_batch(&packet, true, 7, u16::MAX).expect("encode");
        let mut buf = batch.clone();

        let decoded = decode_batch(&mut buf, &session, true, Some(1024)).expect("decode");
        assert_eq!(decoded.len(), 1);
        assert!(matches!(
            decoded[0].data,
            McpePacketData::PacketPlayStatus(ref s) if s.status == PacketPlayStatusStatus::PlayerSpawn
        ));
    }
}
