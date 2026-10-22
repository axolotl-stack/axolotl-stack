//! Bedrock batch framing with a reusable per-connection codec.
use crate::compression::CompressionContext;
use crate::error::{JolyneError, ProtocolError};
use crate::raw::{RawPacket, decode_packets_raw};
use crate::valentine::mcpe::{
    GAME_PACKET_ID as GAME_FRAME_ID, McpePacket, McpePacketArgs, McpePacketData,
};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::slice;
use valentine::bedrock::codec::BedrockSized;
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

/// Negotiated framing and compression settings for an outbound batch.
#[derive(Debug, Clone, Copy)]
pub struct BatchEncoding {
    pub enabled: bool,
    pub algorithm: BatchCompression,
    pub level: u32,
    pub threshold: u16,
    pub prefix: bool,
}

fn ensure_decompressed_size(actual: usize, max: Option<usize>) -> Result<(), JolyneError> {
    if let Some(max) = max
        && actual > max
    {
        return Err(ProtocolError::BatchTooLarge { actual, max }.into());
    }
    Ok(())
}

fn decode_payload(
    mut payload: Bytes,
    _session: &BedrockSession,
) -> Result<Vec<McpePacket>, JolyneError> {
    if payload.first().copied() == Some(GAME_FRAME_ID) {
        let (header, data) = McpePacketData::decode_game_frame_bytes(&mut payload, McpePacketArgs)?;
        return Ok(vec![McpePacket { header, data }]);
    }
    let mut cursor =
        crate::valentine::bedrock::borrowed::BorrowedReader::new(&payload, payload.len());
    let mut packets = Vec::new();
    while cursor.has_remaining() {
        let (header, data) = McpePacketData::decode_inner_reader(&mut cursor, McpePacketArgs)?;
        packets.push(McpePacket { header, data });
    }
    Ok(packets)
}

// Owned packet APIs freeze output for retention; synchronous callbacks keep
// BytesMut ownership so returning scratch does not allocate a Bytes handle.
enum BatchPayload {
    Uncompressed(Bytes),
    Decompressed(BytesMut),
}

impl BatchPayload {
    fn into_bytes(self) -> Bytes {
        match self {
            Self::Uncompressed(bytes) => bytes,
            Self::Decompressed(bytes) => bytes.freeze(),
        }
    }
}

impl AsRef<[u8]> for BatchPayload {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Uncompressed(bytes) => bytes,
            Self::Decompressed(bytes) => bytes,
        }
    }
}

/// Per-connection compression engines and reusable encode scratch.
///
/// Keep this alongside the connection's output buffer to amortize compressor,
/// Snappy table and scratch allocations. Convenience free functions below
/// retain the original API but construct a temporary codec.
#[derive(Default)]
pub struct BatchCodec {
    compression: CompressionContext,
}

impl BatchCodec {
    fn payload(
        &mut self,
        mut input: Bytes,
        enabled: bool,
        max: Option<usize>,
    ) -> Result<BatchPayload, JolyneError> {
        if !enabled {
            ensure_decompressed_size(input.len(), max)?;
            return Ok(BatchPayload::Uncompressed(input));
        }
        if input.is_empty() {
            return Err(ProtocolError::UnexpectedHandshake(
                "Empty compressed batch payload".to_string(),
            )
            .into());
        }
        let byte = input.get_u8();
        let algorithm = BatchCompression::try_from_u8(byte).ok_or_else(|| {
            ProtocolError::UnexpectedHandshake(format!(
                "Unknown compression algorithm: 0x{byte:02x} ({byte})"
            ))
        })?;
        if algorithm == BatchCompression::None {
            ensure_decompressed_size(input.len(), max)?;
            Ok(BatchPayload::Uncompressed(input))
        } else {
            Ok(BatchPayload::Decompressed(
                self.compression.decompress(&input, algorithm, max)?,
            ))
        }
    }

    fn check_prefix(first: u8) -> Result<(), JolyneError> {
        if first != BATCH_PACKET_ID {
            return Err(
                ProtocolError::InvalidBatchId(format!("expected 0xFE, got 0x{first:02x}")).into(),
            );
        }
        Ok(())
    }

    pub fn decode_batch(
        &mut self,
        buf: &mut Bytes,
        session: &BedrockSession,
        enabled: bool,
        max: Option<usize>,
    ) -> Result<Vec<McpePacket>, JolyneError> {
        if buf.is_empty() {
            return Ok(Vec::new());
        }
        Self::check_prefix(buf.get_u8())?;
        decode_payload(
            self.payload(buf.clone(), enabled, max)?.into_bytes(),
            session,
        )
    }

    pub fn decode_batch_no_prefix(
        &mut self,
        buf: &mut Bytes,
        session: &BedrockSession,
        max: Option<usize>,
    ) -> Result<Vec<McpePacket>, JolyneError> {
        if buf.is_empty() {
            return Ok(Vec::new());
        }
        let input = buf.clone();
        // Preserve consumption of the algorithm byte, including on failure.
        buf.advance(1);
        decode_payload(self.payload(input, true, max)?.into_bytes(), session)
    }

    pub fn decode_batch_raw(
        &mut self,
        buf: &mut Bytes,
        enabled: bool,
        max: Option<usize>,
    ) -> Result<Vec<RawPacket>, JolyneError> {
        if buf.is_empty() {
            return Ok(Vec::new());
        }
        let first = buf.get_u8();
        self.decode_batch_raw_split(first, buf.clone(), enabled, max)
    }

    pub fn decode_batch_raw_split(
        &mut self,
        first: u8,
        input: Bytes,
        enabled: bool,
        max: Option<usize>,
    ) -> Result<Vec<RawPacket>, JolyneError> {
        Self::check_prefix(first)?;
        decode_packets_raw(self.payload(input, enabled, max)?.into_bytes())
    }

    pub fn decode_batch_no_prefix_raw(
        &mut self,
        buf: &mut Bytes,
        max: Option<usize>,
    ) -> Result<Vec<RawPacket>, JolyneError> {
        if buf.is_empty() {
            return Ok(Vec::new());
        }
        decode_packets_raw(self.payload(buf.clone(), true, max)?.into_bytes())
    }

    /// Decode length-delimited packet bytes without parsing their headers.
    ///
    /// The synchronous callback borrows each body directly from the batch,
    /// avoiding reference-count updates per packet. Use the raw packet decoder
    /// when packets must outlive the callback. Earlier callbacks have already
    /// run if a later frame is invalid.
    pub fn decode_with(
        &mut self,
        mut input: Bytes,
        enabled: bool,
        max: Option<usize>,
        prefix: bool,
        mut handle: impl FnMut(&[u8]) -> Result<(), JolyneError>,
    ) -> Result<(), JolyneError> {
        if input.is_empty() {
            return Ok(());
        }
        if prefix {
            Self::check_prefix(input.get_u8())?;
        }
        let allocation = self.payload(input, enabled, max)?;
        let mut payload = allocation.as_ref();
        let result = (|| {
            let mut count = 0;
            while !payload.is_empty() {
                if count == crate::raw::MAX_RAW_BATCH_PACKETS {
                    return Err(ProtocolError::TooManyPackets {
                        max: crate::raw::MAX_RAW_BATCH_PACKETS,
                    }
                    .into());
                }
                let len = bedrock_wire::read_var_u32(&mut payload)? as usize;
                if len > payload.len() {
                    return Err(ProtocolError::UnexpectedHandshake(format!(
                        "declared packet length {len} exceeds available {}",
                        payload.len()
                    ))
                    .into());
                }
                handle(&payload[..len])?;
                payload = &payload[len..];
                count += 1;
            }
            Ok(())
        })();
        if let BatchPayload::Decompressed(output) = allocation {
            self.compression.reclaim(output);
        }
        result
    }

    fn start(out: &mut BytesMut, enabled: bool, prefix: bool, estimated: usize) -> usize {
        out.clear();
        let prefix_len = usize::from(enabled) + usize::from(prefix);
        out.reserve(prefix_len + estimated);
        if prefix {
            out.put_u8(BATCH_PACKET_ID);
        }
        if enabled {
            out.put_u8(BatchCompression::None as u8);
        }
        prefix_len
    }

    fn finish(
        &mut self,
        out: &mut BytesMut,
        prefix_len: usize,
        enabled: bool,
        algorithm: BatchCompression,
        level: u32,
        threshold: u16,
    ) -> Result<(), JolyneError> {
        if enabled
            && level > 0
            && algorithm != BatchCompression::None
            && out.len() - prefix_len >= usize::from(threshold)
        {
            out[prefix_len - 1] = algorithm as u8;
            self.compression
                .compress(out, prefix_len, algorithm, level)?;
        }
        Ok(())
    }

    /// Frame typed packets directly in `out`, compressing to reusable scratch.
    pub fn encode_batch_multi_into(
        &mut self,
        packets: &[McpePacket],
        settings: BatchEncoding,
        out: &mut BytesMut,
    ) -> Result<(), JolyneError> {
        let BatchEncoding {
            enabled,
            algorithm,
            level,
            threshold,
            prefix,
        } = settings;
        let estimated = packets.iter().map(|p| 10 + p.data.encoded_size()).sum();
        let prefix_len = Self::start(out, enabled, prefix, estimated);
        for packet in packets {
            packet.data.encode_inner_bytes_mut(
                out,
                packet.header.from_subclient,
                packet.header.to_subclient,
            )?;
        }
        self.finish(out, prefix_len, enabled, algorithm, level, threshold)
    }

    /// Copy retained raw frames once, directly into the final framing buffer.
    pub fn encode_batch_raw_into(
        &mut self,
        packets: &[RawPacket],
        settings: BatchEncoding,
        out: &mut BytesMut,
    ) -> Result<(), JolyneError> {
        let BatchEncoding {
            enabled,
            algorithm,
            level,
            threshold,
            prefix,
        } = settings;
        let estimated = packets.iter().map(|p| p.inner_frame().len()).sum();
        let prefix_len = Self::start(out, enabled, prefix, estimated);
        for packet in packets {
            out.extend_from_slice(packet.inner_frame());
        }
        self.finish(out, prefix_len, enabled, algorithm, level, threshold)
    }

    /// Frame packet bytes (including the packet header, excluding length).
    pub fn encode_frames_into(
        &mut self,
        packets: &[Bytes],
        settings: BatchEncoding,
        out: &mut BytesMut,
    ) -> Result<(), JolyneError> {
        let BatchEncoding {
            enabled,
            algorithm,
            level,
            threshold,
            prefix,
        } = settings;
        let estimated = packets.iter().map(|p| 5 + p.len()).sum();
        let prefix_len = Self::start(out, enabled, prefix, estimated);
        for packet in packets {
            let len = u32::try_from(packet.len()).map_err(|_| {
                ProtocolError::UnexpectedHandshake("packet length exceeds u32".to_string())
            })?;
            bedrock_wire::write_var_u32(out, len);
            out.extend_from_slice(packet);
        }
        self.finish(out, prefix_len, enabled, algorithm, level, threshold)
    }
}

pub fn decode_batch(
    buf: &mut Bytes,
    session: &BedrockSession,
    compression_enabled: bool,
    max_decompressed_size: Option<usize>,
) -> Result<Vec<McpePacket>, JolyneError> {
    BatchCodec::default().decode_batch(buf, session, compression_enabled, max_decompressed_size)
}

pub fn decode_batch_no_prefix(
    buf: &mut Bytes,
    session: &BedrockSession,
    max_decompressed_size: Option<usize>,
) -> Result<Vec<McpePacket>, JolyneError> {
    BatchCodec::default().decode_batch_no_prefix(buf, session, max_decompressed_size)
}

pub fn decode_batch_raw(
    buf: &mut Bytes,
    compression_enabled: bool,
    max_decompressed_size: Option<usize>,
) -> Result<Vec<RawPacket>, JolyneError> {
    BatchCodec::default().decode_batch_raw(buf, compression_enabled, max_decompressed_size)
}

pub fn decode_batch_raw_split(
    first: u8,
    payload_raw: Bytes,
    compression_enabled: bool,
    max_decompressed_size: Option<usize>,
) -> Result<Vec<RawPacket>, JolyneError> {
    BatchCodec::default().decode_batch_raw_split(
        first,
        payload_raw,
        compression_enabled,
        max_decompressed_size,
    )
}

pub fn decode_batch_no_prefix_raw(
    buf: &mut Bytes,
    max_decompressed_size: Option<usize>,
) -> Result<Vec<RawPacket>, JolyneError> {
    BatchCodec::default().decode_batch_no_prefix_raw(buf, max_decompressed_size)
}

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
        true,
    )
}

pub fn encode_batch_multi(
    packets: &[McpePacket],
    compression_enabled: bool,
    compression_level: u32,
    compression_threshold: u16,
    use_batch_prefix: bool,
) -> Result<Bytes, JolyneError> {
    Ok(encode_batch_multi_bytes_mut(
        packets,
        compression_enabled,
        compression_level,
        compression_threshold,
        use_batch_prefix,
    )?
    .freeze())
}

pub(crate) fn encode_batch_multi_bytes_mut(
    packets: &[McpePacket],
    compression_enabled: bool,
    compression_level: u32,
    compression_threshold: u16,
    use_batch_prefix: bool,
) -> Result<BytesMut, JolyneError> {
    let mut out = BytesMut::new();
    encode_batch_multi_into(
        packets,
        compression_enabled,
        BatchCompression::Deflate,
        compression_level,
        compression_threshold,
        use_batch_prefix,
        &mut out,
    )?;
    Ok(out)
}

pub(crate) fn encode_batch_multi_into(
    packets: &[McpePacket],
    compression_enabled: bool,
    compression_algorithm: BatchCompression,
    compression_level: u32,
    compression_threshold: u16,
    use_batch_prefix: bool,
    out: &mut BytesMut,
) -> Result<(), JolyneError> {
    BatchCodec::default().encode_batch_multi_into(
        packets,
        BatchEncoding {
            enabled: compression_enabled,
            algorithm: compression_algorithm,
            level: compression_level,
            threshold: compression_threshold,
            prefix: use_batch_prefix,
        },
        out,
    )
}

pub fn encode_batch_raw(
    packets: &[RawPacket],
    compression_enabled: bool,
    compression_level: u32,
    compression_threshold: u16,
    use_batch_prefix: bool,
) -> Result<Bytes, JolyneError> {
    Ok(encode_batch_raw_bytes_mut(
        packets,
        compression_enabled,
        compression_level,
        compression_threshold,
        use_batch_prefix,
    )?
    .freeze())
}

pub(crate) fn encode_batch_raw_bytes_mut(
    packets: &[RawPacket],
    compression_enabled: bool,
    compression_level: u32,
    compression_threshold: u16,
    use_batch_prefix: bool,
) -> Result<BytesMut, JolyneError> {
    let mut out = BytesMut::new();
    encode_batch_raw_into(
        packets,
        compression_enabled,
        BatchCompression::Deflate,
        compression_level,
        compression_threshold,
        use_batch_prefix,
        &mut out,
    )?;
    Ok(out)
}

pub(crate) fn encode_batch_raw_into(
    packets: &[RawPacket],
    compression_enabled: bool,
    compression_algorithm: BatchCompression,
    compression_level: u32,
    compression_threshold: u16,
    use_batch_prefix: bool,
    out: &mut BytesMut,
) -> Result<(), JolyneError> {
    BatchCodec::default().encode_batch_raw_into(
        packets,
        BatchEncoding {
            enabled: compression_enabled,
            algorithm: compression_algorithm,
            level: compression_level,
            threshold: compression_threshold,
            prefix: use_batch_prefix,
        },
        out,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::valentine::{PlayStatusPacket, PlayStatusPacketStatus};

    #[test]
    fn frame_callback_roundtrips_reused_codec_and_markers() {
        let packets = [
            Bytes::from_static(b"\x09abc"),
            Bytes::from_static(b"\x09def"),
        ];
        let mut codec = BatchCodec::default();
        let mut out = BytesMut::new();
        for algorithm in [
            BatchCompression::Deflate,
            BatchCompression::Snappy,
            BatchCompression::None,
        ] {
            for prefix in [false, true] {
                for threshold in [0, 1024] {
                    codec
                        .encode_frames_into(
                            &packets,
                            BatchEncoding {
                                enabled: true,
                                algorithm,
                                level: 7,
                                threshold,
                                prefix,
                            },
                            &mut out,
                        )
                        .unwrap();
                    let expected = if threshold == 0 {
                        algorithm
                    } else {
                        BatchCompression::None
                    };
                    assert_eq!(out[usize::from(prefix)], expected as u8);
                    let mut decoded = Vec::new();
                    codec
                        .decode_with(
                            Bytes::copy_from_slice(&out),
                            true,
                            Some(10),
                            prefix,
                            |frame| {
                                decoded.push(Bytes::copy_from_slice(frame));
                                Ok(())
                            },
                        )
                        .unwrap();
                    assert_eq!(decoded, packets);
                }
            }
        }
    }

    #[test]
    fn frame_callback_rejects_truncated_lengths_and_packet_floods() {
        let mut codec = BatchCodec::default();
        assert!(
            codec
                .decode_with(
                    Bytes::from_static(b"\x04ab"),
                    false,
                    None,
                    false,
                    |_| Ok(())
                )
                .is_err()
        );
        let input = Bytes::from(vec![0; crate::raw::MAX_RAW_BATCH_PACKETS + 1]);
        let mut count = 0;
        assert!(
            codec
                .decode_with(input, false, None, false, |_| {
                    count += 1;
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(count, crate::raw::MAX_RAW_BATCH_PACKETS);
    }

    fn test_session() -> BedrockSession {
        BedrockSession { shield_item_id: 0 }
    }

    #[test]
    fn decode_batch_rejects_wrong_id() {
        let mut buf = Bytes::from_static(&[0x00, 0x01, 0x02]);
        let session = test_session();
        let err = decode_batch(&mut buf, &session, false, None).expect_err("should fail");
        assert!(matches!(
            err,
            JolyneError::Protocol(ProtocolError::InvalidBatchId(_))
        ));
    }

    #[test]
    fn decode_batch_rejects_empty_compressed_payload() {
        let mut buf = Bytes::from_static(&[BATCH_PACKET_ID]);
        let session = test_session();
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
        let session = test_session();
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
        let session = test_session();
        let packet = McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::Loginsuccess,
        });

        let batch = encode_batch(&packet, true, 7, 0).expect("encode");
        let mut buf = batch.clone();

        let decoded = decode_batch(&mut buf, &session, true, Some(1024)).expect("decode");
        assert_eq!(decoded.len(), 1);
        assert!(matches!(
            &decoded[0].data,
            McpePacketData::PlayStatusPacket(s) if s.status == PlayStatusPacketStatus::Loginsuccess
        ));
    }

    // ========== Compression Tests ==========

    #[test]
    fn batch_compression_enum_values() {
        assert_eq!(BatchCompression::Deflate as u8, 0x00);
        assert_eq!(BatchCompression::Snappy as u8, 0x01);
        assert_eq!(BatchCompression::None as u8, 0xFF);
    }

    #[test]
    fn batch_compression_try_from_u8() {
        assert_eq!(
            BatchCompression::try_from_u8(0x00),
            Some(BatchCompression::Deflate)
        );
        assert_eq!(
            BatchCompression::try_from_u8(0x01),
            Some(BatchCompression::Snappy)
        );
        assert_eq!(
            BatchCompression::try_from_u8(0xFF),
            Some(BatchCompression::None)
        );
        assert_eq!(BatchCompression::try_from_u8(0x02), None);
        assert_eq!(BatchCompression::try_from_u8(0xFE), None);
    }

    #[test]
    fn encode_decode_roundtrip_uncompressed() {
        let session = test_session();
        let packet = McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::Playerspawn,
        });

        // Compression disabled
        let batch = encode_batch(&packet, false, 0, 0).expect("encode");
        let mut buf = batch.clone();

        // Should start with 0xFE
        assert_eq!(buf[0], BATCH_PACKET_ID);

        let decoded = decode_batch(&mut buf, &session, false, None).expect("decode");
        assert_eq!(decoded.len(), 1);
        assert!(matches!(
            &decoded[0].data,
            McpePacketData::PlayStatusPacket(s) if s.status == PlayStatusPacketStatus::Playerspawn
        ));
    }

    #[test]
    fn encode_with_compression_none_marker() {
        let packet = McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::Loginsuccess,
        });

        // Compression enabled but level 0 or below threshold -> should use None marker
        let batch = encode_batch(&packet, true, 0, 9999).expect("encode");

        // Should be: [0xFE][0xFF][payload]
        assert_eq!(batch[0], BATCH_PACKET_ID);
        assert_eq!(batch[1], BatchCompression::None as u8);
    }

    #[test]
    fn compression_threshold_below_threshold_uses_none() {
        let packet = McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::Loginsuccess,
        });

        // Threshold = 512, packet is small -> should use None compression
        let batch = encode_batch(&packet, true, 7, 512).expect("encode");

        assert_eq!(batch[0], BATCH_PACKET_ID);
        assert_eq!(batch[1], BatchCompression::None as u8);
    }

    #[test]
    fn compression_threshold_above_threshold_uses_deflate() {
        let packet = McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::Loginsuccess,
        });

        // Threshold = 0, any packet should compress
        let batch = encode_batch(&packet, true, 7, 0).expect("encode");

        assert_eq!(batch[0], BATCH_PACKET_ID);
        assert_eq!(batch[1], BatchCompression::Deflate as u8);
    }

    #[test]
    fn decode_batch_empty_returns_empty() {
        let session = test_session();
        let mut buf = Bytes::new();

        let decoded = decode_batch(&mut buf, &session, false, None).expect("decode empty");
        assert!(decoded.is_empty());
    }

    #[test]
    fn multiple_packets_in_batch() {
        let session = test_session();
        let packets = vec![
            McpePacket::from(PlayStatusPacket {
                status: PlayStatusPacketStatus::Loginsuccess,
            }),
            McpePacket::from(PlayStatusPacket {
                status: PlayStatusPacketStatus::Playerspawn,
            }),
        ];

        let batch = encode_batch_multi(&packets, true, 7, 0, true).expect("encode multi");
        let mut buf = batch.clone();

        let decoded = decode_batch(&mut buf, &session, true, None).expect("decode");
        assert_eq!(decoded.len(), 2);

        assert!(matches!(
            &decoded[0].data,
            McpePacketData::PlayStatusPacket(s) if s.status == PlayStatusPacketStatus::Loginsuccess
        ));
        assert!(matches!(
            &decoded[1].data,
            McpePacketData::PlayStatusPacket(s) if s.status == PlayStatusPacketStatus::Playerspawn
        ));
    }

    // ========== NetherNet (No Prefix) Tests ==========

    #[test]
    fn encode_decode_nethernet_format() {
        let session = test_session();
        let packet = McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::Loginsuccess,
        });

        // NetherNet style: no 0xFE prefix
        let batch =
            encode_batch_multi(slice::from_ref(&packet), true, 7, 0, false).expect("encode");
        let mut buf = batch.clone();

        // Should NOT start with 0xFE
        assert_eq!(buf[0], BatchCompression::Deflate as u8);

        let decoded = decode_batch_no_prefix(&mut buf, &session, None).expect("decode");
        assert_eq!(decoded.len(), 1);
    }

    #[test]
    fn decode_nethernet_empty_returns_empty() {
        let session = test_session();
        let mut buf = Bytes::new();

        let decoded = decode_batch_no_prefix(&mut buf, &session, None).expect("decode empty");
        assert!(decoded.is_empty());
    }

    // ========== Decompression Guard Tests ==========

    #[test]
    fn decompression_guard_rejects_oversized() {
        let session = test_session();

        // Create packets that will decompress to more than 100 bytes
        let packets: Vec<_> = (0..10)
            .map(|_| {
                McpePacket::from(PlayStatusPacket {
                    status: PlayStatusPacketStatus::Loginsuccess,
                })
            })
            .collect();

        let batch = encode_batch_multi(&packets, true, 7, 0, true).expect("encode");
        let mut buf = batch.clone();

        // Very small max size
        let result = decode_batch(&mut buf, &session, true, Some(1));
        assert!(result.is_err());
    }

    #[test]
    fn raw_batch_size_guard_covers_none_and_pre_compression_payloads() {
        let packet = McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::Loginsuccess,
        });

        let mut none = encode_batch(&packet, true, 0, u16::MAX).expect("encode None batch");
        decode_batch_raw(&mut none, true, Some(1))
            .expect_err("None payload over the configured limit must fail");

        let clear = encode_batch(&packet, false, 0, 0).expect("encode clear batch");
        let mut contiguous = clear.clone();
        decode_batch_raw(&mut contiguous, false, Some(1))
            .expect_err("pre-compression contiguous payload over the limit must fail");

        decode_batch_raw_split(BATCH_PACKET_ID, clear.slice(1..), false, Some(1))
            .expect_err("pre-compression split payload over the limit must fail");
    }

    // ========== Snappy Tests ==========

    #[test]
    fn decode_batch_accepts_snappy() {
        let session = test_session();
        let packet = McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::Loginsuccess,
        });
        let raw_batch = encode_batch(&packet, false, 0, 0).expect("encode raw");
        let compressed = snap::raw::Encoder::new()
            .compress_vec(&raw_batch[1..])
            .expect("snappy compress");

        let mut framed = BytesMut::with_capacity(2 + compressed.len());
        framed.put_u8(BATCH_PACKET_ID);
        framed.put_u8(BatchCompression::Snappy as u8);
        framed.extend_from_slice(&compressed);

        let mut framed = framed.freeze();
        let decoded = decode_batch(&mut framed, &session, true, Some(1024)).expect("decode snappy");
        assert_eq!(decoded.len(), 1);
        assert!(matches!(
            &decoded[0].data,
            McpePacketData::PlayStatusPacket(s) if s.status == PlayStatusPacketStatus::Loginsuccess
        ));
    }

    // ========== Raw Packet Tests ==========
    // Note: RawPacket creation requires going through decode_packets_raw,
    // which means we can test the roundtrip through encode -> decode.

    #[test]
    fn raw_batch_encode_decode_via_full_packet() {
        // Create a normal packet, encode it, then decode as raw
        let session = test_session();
        let packet = McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::Loginsuccess,
        });

        // Encode as normal batch
        let batch = encode_batch(&packet, true, 7, 0).expect("encode");
        let mut buf = batch.clone();

        // Decode as raw
        let decoded_raw = decode_batch_raw(&mut buf, true, None).expect("decode raw");
        assert_eq!(decoded_raw.len(), 1);

        // Re-encode the raw packets
        let batch2 = encode_batch_raw(&decoded_raw, true, 7, 0, true).expect("encode raw");
        let mut buf2 = batch2.clone();

        // Decode as full packet to verify roundtrip
        let decoded = decode_batch(&mut buf2, &session, true, None).expect("decode full");
        assert_eq!(decoded.len(), 1);
        assert!(matches!(
            &decoded[0].data,
            McpePacketData::PlayStatusPacket(s) if s.status == PlayStatusPacketStatus::Loginsuccess
        ));
    }

    #[test]
    fn raw_batch_uncompressed_roundtrip() {
        let session = test_session();
        let packet = McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::Playerspawn,
        });

        // Encode without compression
        let batch = encode_batch(&packet, false, 0, 0).expect("encode");
        let mut buf = batch.clone();

        // Decode as raw (no compression)
        let decoded_raw = decode_batch_raw(&mut buf, false, None).expect("decode raw");
        assert_eq!(decoded_raw.len(), 1);

        // Re-encode
        let batch2 = encode_batch_raw(&decoded_raw, false, 0, 0, true).expect("encode raw");
        let mut buf2 = batch2.clone();

        // Verify
        let decoded = decode_batch(&mut buf2, &session, false, None).expect("decode full");
        assert_eq!(decoded.len(), 1);
    }
}
