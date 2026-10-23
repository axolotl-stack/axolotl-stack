//! Bedrock batch framing with a reusable per-connection codec.
//!
//! Wire layout: `[0xFE if RakNet][algorithm byte once compression is
//! negotiated][compressed or plain (varuint len + header + body)*]`.

use std::collections::VecDeque;

use bytes::{Buf, BufMut, Bytes, BytesMut};
#[cfg(any(test, feature = "bench"))]
use valentine::protocol::wire as bedrock_wire;

use super::compression::Compression;
use super::custom::PacketFilter;
use super::raw::{BatchStats, DEFAULT_MAX_PACKETS_PER_BATCH, RawPacket, decode_packets_into};
use crate::error::{JolyneError, ProtocolError};
#[cfg(any(test, feature = "bench"))]
use crate::valentine::mcpe::{McpePacket, McpePacketArgs, McpePacketData};

pub const BATCH_PACKET_ID: u8 = 0xFE;

/// Default cap on a decompressed batch (gophertunnel `DefaultMaxDecompressedLen`).
pub const DEFAULT_MAX_DECOMPRESSED_BATCH: usize = 16 * 1024 * 1024;

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

/// Settings a [`BatchCodec`] is built from ([`BatchCodec::new`]).
#[derive(Debug, Clone, Copy)]
pub struct BatchEncoding {
    /// Whether compression was negotiated (the algorithm byte is present).
    pub enabled: bool,
    pub algorithm: BatchCompression,
    pub level: u32,
    /// Payloads shorter than this are sent with the `None` marker.
    pub threshold: u16,
    /// Whether the RakNet `0xFE` marker is written.
    pub prefix: bool,
}

/// Inbound batch limits. There is no unbounded mode: the decompressed-size
/// bound is always enforced while streaming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchLimits {
    pub max_decompressed: usize,
    pub max_packets: usize,
}

impl Default for BatchLimits {
    fn default() -> Self {
        Self {
            max_decompressed: DEFAULT_MAX_DECOMPRESSED_BATCH,
            max_packets: DEFAULT_MAX_PACKETS_PER_BATCH,
        }
    }
}

fn ensure_decompressed_size(actual: usize, max: usize) -> Result<(), JolyneError> {
    if actual > max {
        return Err(ProtocolError::BatchTooLarge { actual, max }.into());
    }
    Ok(())
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

/// How one connection frames batches: the RakNet marker, and the
/// negotiated compression once there is one.
///
/// Build it from settings with [`BatchCodec::new`]; a connection starts
/// [`BatchCodec::plain`] and calls [`BatchCodec::enable_compression`] when
/// `NetworkSettings` is negotiated. Keep it alongside the connection's output
/// buffer to amortize compressor, Snappy table and scratch allocations.
pub struct BatchCodec {
    /// Whether the RakNet `0xFE` marker is written and expected.
    prefix: bool,
    mode: Mode,
}

enum Mode {
    /// Before `NetworkSettings`: no algorithm byte.
    Plain,
    /// After `NetworkSettings`: an algorithm byte precedes every payload.
    Compressed {
        /// Payloads shorter than this are sent with the `None` marker.
        threshold: u16,
        // Boxed: the engines are large and the codec moves with its connection.
        compression: Box<Compression>,
    },
}

impl BatchCodec {
    /// A codec for `encoding`.
    pub fn new(encoding: BatchEncoding) -> Result<Self, JolyneError> {
        let mut codec = Self::plain(encoding.prefix);
        if encoding.enabled {
            codec.enable_compression(encoding.algorithm, encoding.level, encoding.threshold)?;
        }
        Ok(codec)
    }

    /// A codec before compression is negotiated.
    pub fn plain(prefix: bool) -> Self {
        Self {
            prefix,
            mode: Mode::Plain,
        }
    }

    /// Switches to the negotiated compression: from now on every batch
    /// carries an algorithm byte.
    pub fn enable_compression(
        &mut self,
        algorithm: BatchCompression,
        level: u32,
        threshold: u16,
    ) -> Result<(), JolyneError> {
        self.mode = Mode::Compressed {
            threshold,
            compression: Box::new(Compression::new(algorithm, level)?),
        };
        Ok(())
    }

    /// Whether compression was negotiated (the algorithm byte is present).
    pub fn is_compressed(&self) -> bool {
        matches!(self.mode, Mode::Compressed { .. })
    }

    /// Strips the algorithm byte (when compression is negotiated) and
    /// decompresses into reusable scratch.
    fn payload(&mut self, mut input: Bytes, max: usize) -> Result<BatchPayload, JolyneError> {
        let Mode::Compressed { compression, .. } = &mut self.mode else {
            ensure_decompressed_size(input.len(), max)?;
            return Ok(BatchPayload::Uncompressed(input));
        };
        if input.is_empty() {
            return Err(ProtocolError::EmptyCompressedBatch.into());
        }
        let byte = input.get_u8();
        match BatchCompression::try_from_u8(byte).ok_or(ProtocolError::UnknownCompression(byte))? {
            BatchCompression::None => {
                ensure_decompressed_size(input.len(), max)?;
                Ok(BatchPayload::Uncompressed(input))
            }
            algorithm => Ok(BatchPayload::Decompressed(
                compression.decompress(&input, algorithm, max)?,
            )),
        }
    }

    pub(crate) fn check_prefix(first: u8) -> Result<(), JolyneError> {
        if first != BATCH_PACKET_ID {
            return Err(ProtocolError::InvalidBatchId(first).into());
        }
        Ok(())
    }

    /// Decodes a batch body (marker already stripped) into raw packets
    /// appended to `out`, applying limits and the unknown-packet policy.
    pub fn decode_raw_into(
        &mut self,
        body: Bytes,
        limits: BatchLimits,
        filter: &PacketFilter,
        out: &mut VecDeque<RawPacket>,
    ) -> Result<BatchStats, JolyneError> {
        let payload = self.payload(body, limits.max_decompressed)?.into_bytes();
        decode_packets_into(payload, filter, limits.max_packets, out)
    }

    /// Strips the RakNet marker when this codec expects one.
    #[cfg(any(test, feature = "bench"))]
    fn strip_prefix(&self, buf: &mut Bytes) -> Result<(), JolyneError> {
        if self.prefix {
            Self::check_prefix(buf.get_u8())?;
        }
        Ok(())
    }

    #[cfg(any(test, feature = "bench"))]
    /// Decodes a full batch into raw packets, keeping unknown IDs.
    pub fn decode_batch_raw(
        &mut self,
        buf: &mut Bytes,
        max: usize,
    ) -> Result<Vec<RawPacket>, JolyneError> {
        if buf.is_empty() {
            return Ok(Vec::new());
        }
        self.strip_prefix(buf)?;
        let mut out = VecDeque::new();
        self.decode_raw_into(
            std::mem::take(buf),
            BatchLimits {
                max_decompressed: max,
                ..BatchLimits::default()
            },
            &PacketFilter::PASSTHROUGH,
            &mut out,
        )?;
        Ok(out.into())
    }

    #[cfg(any(test, feature = "bench"))]
    /// Decodes a full batch into owned valentine packets.
    pub fn decode_batch(
        &mut self,
        buf: &mut Bytes,
        limits: BatchLimits,
    ) -> Result<Vec<McpePacket>, JolyneError> {
        if buf.is_empty() {
            return Ok(Vec::new());
        }
        self.strip_prefix(buf)?;
        let payload = self
            .payload(std::mem::take(buf), limits.max_decompressed)?
            .into_bytes();
        let mut cursor =
            crate::valentine::bedrock::borrowed::BorrowedReader::new(&payload, payload.len());
        let mut packets = Vec::new();
        while cursor.has_remaining() {
            if packets.len() == limits.max_packets {
                return Err(ProtocolError::TooManyPackets {
                    max: limits.max_packets,
                }
                .into());
            }
            let (header, data) = McpePacketData::decode_inner_reader(&mut cursor, McpePacketArgs)?;
            packets.push(McpePacket { header, data });
        }
        Ok(packets)
    }

    #[cfg(any(test, feature = "bench"))]
    /// Decode length-delimited packet bytes without parsing their headers.
    ///
    /// The synchronous callback borrows each frame directly from the batch,
    /// avoiding reference-count updates per packet. Use the raw packet decoder
    /// when packets must outlive the callback. Earlier callbacks have already
    /// run if a later frame is invalid.
    pub fn decode_with(
        &mut self,
        mut input: Bytes,
        max: usize,
        mut handle: impl FnMut(&[u8]) -> Result<(), JolyneError>,
    ) -> Result<(), JolyneError> {
        if input.is_empty() {
            return Ok(());
        }
        self.strip_prefix(&mut input)?;
        let allocation = self.payload(input, max)?;
        let mut payload = allocation.as_ref();
        let result = (|| {
            let mut count = 0;
            while !payload.is_empty() {
                if count == DEFAULT_MAX_PACKETS_PER_BATCH {
                    return Err(ProtocolError::TooManyPackets {
                        max: DEFAULT_MAX_PACKETS_PER_BATCH,
                    }
                    .into());
                }
                let len = bedrock_wire::read_var_u32(&mut payload)? as usize;
                if len > payload.len() {
                    return Err(ProtocolError::PacketLength {
                        declared: len,
                        available: payload.len(),
                    }
                    .into());
                }
                handle(&payload[..len])?;
                payload = &payload[len..];
                count += 1;
            }
            Ok(())
        })();
        if let (BatchPayload::Decompressed(output), Mode::Compressed { compression, .. }) =
            (allocation, &mut self.mode)
        {
            compression.reclaim(output);
        }
        result
    }

    /// Clears `out` and writes the batch marker and placeholder algorithm
    /// byte. Returns the header length to pass to [`Self::finish`].
    pub(crate) fn start(&self, out: &mut BytesMut, estimated: usize) -> usize {
        out.clear();
        let compressed = self.is_compressed();
        let prefix_len = usize::from(compressed) + usize::from(self.prefix);
        out.reserve(prefix_len + estimated);
        if self.prefix {
            out.put_u8(BATCH_PACKET_ID);
        }
        if compressed {
            out.put_u8(BatchCompression::None as u8);
        }
        prefix_len
    }

    /// Compresses the framed packets in `out[prefix_len..]` in place when
    /// compression is negotiated and the payload reaches the threshold.
    pub(crate) fn finish(
        &mut self,
        out: &mut BytesMut,
        prefix_len: usize,
    ) -> Result<(), JolyneError> {
        if let Mode::Compressed {
            threshold,
            compression,
        } = &mut self.mode
            && out.len() - prefix_len >= usize::from(*threshold)
        {
            out[prefix_len - 1] = compression.algorithm() as u8;
            compression.compress(out, prefix_len)?;
        }
        Ok(())
    }

    #[cfg(any(test, feature = "bench"))]
    /// Frame typed packets directly in `out`, compressing to reusable scratch.
    pub fn encode_batch_multi_into(
        &mut self,
        packets: &[McpePacket],
        out: &mut BytesMut,
    ) -> Result<(), JolyneError> {
        use valentine::bedrock::codec::BedrockSized;
        let estimated = packets.iter().map(|p| 10 + p.data.encoded_size()).sum();
        let prefix_len = self.start(out, estimated);
        for packet in packets {
            packet.data.encode_inner_bytes_mut(
                out,
                packet.header.from_subclient,
                packet.header.to_subclient,
            )?;
        }
        self.finish(out, prefix_len)
    }

    #[cfg(any(test, feature = "bench"))]
    /// Copy retained raw frames once, directly into the final framing buffer.
    pub fn encode_batch_raw_into(
        &mut self,
        packets: &[RawPacket],
        out: &mut BytesMut,
    ) -> Result<(), JolyneError> {
        let estimated = packets.iter().map(|p| p.inner_frame().len()).sum();
        let prefix_len = self.start(out, estimated);
        for packet in packets {
            out.extend_from_slice(packet.inner_frame());
        }
        self.finish(out, prefix_len)
    }

    #[cfg(any(test, feature = "bench"))]
    /// Frame packet bytes (including the packet header, excluding length).
    pub fn encode_frames_into(
        &mut self,
        packets: &[Bytes],
        out: &mut BytesMut,
    ) -> Result<(), JolyneError> {
        let estimated = packets.iter().map(|p| 5 + p.len()).sum();
        let prefix_len = self.start(out, estimated);
        for packet in packets {
            // Most game packets are under 128 bytes: one length byte.
            if packet.len() < 0x80 {
                out.put_u8(packet.len() as u8);
            } else {
                let len = u32::try_from(packet.len())
                    .map_err(|_| ProtocolError::PacketTooLarge(packet.len()))?;
                bedrock_wire::write_var_u32(out, len);
            }
            out.extend_from_slice(packet);
        }
        self.finish(out, prefix_len)
    }
}

/// A `0xFE`-prefixed reader: plain, or expecting algorithm bytes.
#[cfg(any(test, feature = "bench"))]
fn reader(compression_enabled: bool) -> Result<BatchCodec, JolyneError> {
    BatchCodec::new(BatchEncoding {
        enabled: compression_enabled,
        algorithm: BatchCompression::None,
        level: 0,
        threshold: 0,
        prefix: true,
    })
}

#[cfg(any(test, feature = "bench"))]
/// Decodes a `0xFE`-prefixed batch into owned packets with default limits.
pub fn decode_batch(
    buf: &mut Bytes,
    compression_enabled: bool,
) -> Result<Vec<McpePacket>, JolyneError> {
    reader(compression_enabled)?.decode_batch(buf, BatchLimits::default())
}

#[cfg(any(test, feature = "bench"))]
/// Decodes a `0xFE`-prefixed batch into raw packets, keeping unknown IDs.
pub fn decode_batch_raw(
    buf: &mut Bytes,
    compression_enabled: bool,
    max_decompressed: usize,
) -> Result<Vec<RawPacket>, JolyneError> {
    reader(compression_enabled)?.decode_batch_raw(buf, max_decompressed)
}

#[cfg(any(test, feature = "bench"))]
/// Encodes owned packets into one batch.
pub fn encode_batch_multi(
    packets: &[McpePacket],
    settings: BatchEncoding,
) -> Result<Bytes, JolyneError> {
    let mut out = BytesMut::new();
    BatchCodec::new(settings)?.encode_batch_multi_into(packets, &mut out)?;
    Ok(out.freeze())
}

#[cfg(any(test, feature = "bench"))]
/// Encodes raw packets into one batch.
pub fn encode_batch_raw(
    packets: &[RawPacket],
    settings: BatchEncoding,
) -> Result<Bytes, JolyneError> {
    let mut out = BytesMut::new();
    BatchCodec::new(settings)?.encode_batch_raw_into(packets, &mut out)?;
    Ok(out.freeze())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::valentine::{McpePacketData, PlayStatusPacket, PlayStatusPacketStatus};

    pub(crate) fn settings(
        enabled: bool,
        level: u32,
        threshold: u16,
        prefix: bool,
    ) -> BatchEncoding {
        BatchEncoding {
            enabled,
            algorithm: BatchCompression::Deflate,
            level,
            threshold,
            prefix,
        }
    }

    fn status(status: PlayStatusPacketStatus) -> McpePacket {
        McpePacket::from(PlayStatusPacket { status })
    }

    #[test]
    fn frame_callback_roundtrips_reused_codec_and_markers() {
        let packets = [
            Bytes::from_static(b"\x09abc"),
            Bytes::from_static(b"\x09def"),
        ];
        let mut out = BytesMut::new();
        for algorithm in [
            BatchCompression::Deflate,
            BatchCompression::Snappy,
            BatchCompression::None,
        ] {
            for prefix in [false, true] {
                for threshold in [0, 1024] {
                    let mut codec = BatchCodec::new(BatchEncoding {
                        enabled: true,
                        algorithm,
                        level: 7,
                        threshold,
                        prefix,
                    })
                    .unwrap();
                    // Twice: the second pass reuses the engines and scratch.
                    for _ in 0..2 {
                        codec.encode_frames_into(&packets, &mut out).unwrap();
                        let expected = if threshold == 0 {
                            algorithm
                        } else {
                            BatchCompression::None
                        };
                        assert_eq!(out[usize::from(prefix)], expected as u8);
                        let mut decoded = Vec::new();
                        codec
                            .decode_with(Bytes::copy_from_slice(&out), 10, |frame| {
                                decoded.push(Bytes::copy_from_slice(frame));
                                Ok(())
                            })
                            .unwrap();
                        assert_eq!(decoded, packets);
                    }
                }
            }
        }
    }

    #[test]
    fn frame_callback_rejects_truncated_lengths_and_packet_floods() {
        let mut codec = BatchCodec::plain(false);
        assert!(
            codec
                .decode_with(Bytes::from_static(b"\x04ab"), usize::MAX, |_| Ok(()))
                .is_err()
        );
        let input = Bytes::from(vec![0; DEFAULT_MAX_PACKETS_PER_BATCH + 1]);
        let mut count = 0;
        assert!(
            codec
                .decode_with(input, usize::MAX, |_| {
                    count += 1;
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(count, DEFAULT_MAX_PACKETS_PER_BATCH);
    }

    #[test]
    fn rejects_wrong_marker_empty_payload_and_unknown_algorithm() {
        assert!(matches!(
            decode_batch(&mut Bytes::from_static(&[0x00, 0x01]), false),
            Err(JolyneError::Protocol(ProtocolError::InvalidBatchId(0x00)))
        ));
        assert!(matches!(
            decode_batch(&mut Bytes::from_static(&[BATCH_PACKET_ID]), true),
            Err(JolyneError::Protocol(ProtocolError::EmptyCompressedBatch))
        ));
        assert!(matches!(
            decode_batch(
                &mut Bytes::from_static(&[BATCH_PACKET_ID, 0xBA, 0x00]),
                true
            ),
            Err(JolyneError::Protocol(ProtocolError::UnknownCompression(
                0xBA
            )))
        ));
    }

    #[test]
    fn threshold_selects_marker_and_roundtrips_all_algorithms() {
        let packets = [
            status(PlayStatusPacketStatus::Loginsuccess),
            status(PlayStatusPacketStatus::Playerspawn),
        ];
        for algorithm in [BatchCompression::Deflate, BatchCompression::Snappy] {
            for (threshold, marker) in [(0, algorithm), (512, BatchCompression::None)] {
                let batch = encode_batch_multi(
                    &packets,
                    BatchEncoding {
                        algorithm,
                        ..settings(true, 7, threshold, true)
                    },
                )
                .unwrap();
                assert_eq!(batch[1], marker as u8);
                let decoded = decode_batch(&mut batch.clone(), true).unwrap();
                assert!(matches!(
                    decoded.as_slice(),
                    [
                        McpePacket { data: McpePacketData::PlayStatusPacket(a), .. },
                        McpePacket { data: McpePacketData::PlayStatusPacket(b), .. },
                    ] if a.status == PlayStatusPacketStatus::Loginsuccess
                        && b.status == PlayStatusPacketStatus::Playerspawn
                ));
            }
        }
        // NetherNet framing: no marker.
        let batch = encode_batch_multi(&packets[..1], settings(true, 7, 0, false)).unwrap();
        assert_eq!(batch[0], BatchCompression::Deflate as u8);
        let decoded = BatchCodec::new(settings(true, 7, 0, false))
            .unwrap()
            .decode_batch(&mut batch.clone(), BatchLimits::default())
            .unwrap();
        assert_eq!(decoded.len(), 1);
    }

    #[test]
    fn size_guard_covers_compressed_none_and_plain_payloads() {
        let packets: Vec<_> = (0..10)
            .map(|_| status(PlayStatusPacketStatus::Loginsuccess))
            .collect();
        let tiny = BatchLimits {
            max_decompressed: 1,
            ..BatchLimits::default()
        };
        for encoding in [
            settings(true, 7, 0, true),
            settings(true, 0, u16::MAX, true),
            settings(false, 0, 0, true),
        ] {
            let batch = encode_batch_multi(&packets, encoding).unwrap();
            assert!(matches!(
                BatchCodec::new(encoding)
                    .unwrap()
                    .decode_batch(&mut batch.clone(), tiny),
                Err(JolyneError::Protocol(
                    ProtocolError::BatchTooLarge { .. } | ProtocolError::DecompressionFailed(_)
                ))
            ));
            assert!(decode_batch_raw(&mut batch.clone(), encoding.enabled, 1).is_err());
        }
    }

    /// Regression: the typed decode path had no packet-count cap while the raw
    /// path enforced 1,600.
    #[test]
    fn typed_decode_enforces_packet_cap() {
        let packet = status(PlayStatusPacketStatus::Loginsuccess);
        let packets = vec![packet; 1_601];
        let batch = encode_batch_multi(&packets, settings(false, 0, 0, true)).unwrap();
        assert!(matches!(
            decode_batch(&mut batch.clone(), false),
            Err(JolyneError::Protocol(ProtocolError::TooManyPackets {
                max: 1_600
            }))
        ));
        let frame_len = encode_batch_multi(&packets[..1], settings(false, 0, 0, true))
            .unwrap()
            .len()
            - 1;
        let at_cap = batch.slice(..batch.len() - frame_len);
        assert_eq!(
            decode_batch(&mut at_cap.clone(), false).unwrap().len(),
            1_600
        );
    }

    #[test]
    fn raw_packets_reencode_byte_identically() {
        let packets = [status(PlayStatusPacketStatus::Playerspawn)];
        for encoding in [settings(true, 7, 0, true), settings(false, 0, 0, true)] {
            let batch = encode_batch_multi(&packets, encoding).unwrap();
            let raw = decode_batch_raw(&mut batch.clone(), encoding.enabled, usize::MAX).unwrap();
            assert_eq!(encode_batch_raw(&raw, encoding).unwrap(), batch);
        }
    }
}
