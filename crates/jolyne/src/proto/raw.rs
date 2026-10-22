//! Raw packets: header parsed, body kept as the original wire bytes.
//!
//! [`RawPacket`] is the unit the core queues and forwards. It carries the
//! numeric ten-bit ID, so packets valentine does not know (custom or newer
//! packets) can be inspected, forwarded and re-encoded without loss.

use std::collections::VecDeque;
use std::fmt;

use bytes::{Buf, Bytes, BytesMut};
use valentine::protocol::wire;

use super::custom::{CustomPacket, PacketFilter, UnknownPacketPolicy, packet_name};
use crate::error::{JolyneError, ProtocolError};
use crate::valentine::BorrowedMcpePacket;
use crate::valentine::mcpe::{McpePacket, McpePacketArgs, McpePacketData, McpePacketName};

/// Default cap on packets per batch (valentine raw and typed decode paths).
///
/// gophertunnel's listener uses 812; jolyne keeps 1,600 so it never rejects
/// batches a vanilla server emits (BDS spawn batches exceed 812 packets).
pub const DEFAULT_MAX_PACKETS_PER_BATCH: usize = 1_600;

/// The decoded game packet header: ten-bit ID plus sub-client routing bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PacketHeader {
    pub id: u16,
    pub from_subclient: u8,
    pub to_subclient: u8,
}

impl PacketHeader {
    #[inline]
    pub const fn new(id: u16) -> Self {
        Self {
            id,
            from_subclient: 0,
            to_subclient: 0,
        }
    }

    #[inline]
    fn encode(self) -> u32 {
        u32::from(self.id & 0x3FF)
            | (u32::from(self.from_subclient & 0x3) << 10)
            | (u32::from(self.to_subclient & 0x3) << 12)
    }
}

/// A packet with only the header parsed, body kept as raw bytes.
///
/// The packet owns a slice of the batch it arrived in (`[len][header][body]`),
/// so forwarding it copies nothing until it is written into an outbound batch.
#[derive(Clone)]
pub struct RawPacket {
    header: PacketHeader,
    name: Option<McpePacketName>,
    /// Complete inner frame: `[Length varint][Header varint][Body]`.
    frame: Bytes,
    body_start: u32,
    /// The frame owns its own allocation (see [`RawPacket::into_compact`]).
    compact: bool,
}

impl fmt::Debug for RawPacket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawPacket")
            .field("id", &self.header.id)
            .field("name", &self.name)
            .field("from_subclient", &self.header.from_subclient)
            .field("to_subclient", &self.header.to_subclient)
            .field("body_len", &self.body().len())
            .finish()
    }
}

impl RawPacket {
    /// Builds a packet from a header and body, framing it once. Subclient
    /// fields are truncated to their two wire bits.
    pub fn new(header: PacketHeader, body: &[u8]) -> Result<Self, JolyneError> {
        if header.id > super::custom::MAX_PACKET_ID {
            return Err(ProtocolError::InvalidPacketId(header.id).into());
        }
        let header_raw = header.encode();
        let payload_len = wire::var_u32_len(header_raw) + body.len();
        let len =
            u32::try_from(payload_len).map_err(|_| ProtocolError::PacketTooLarge(payload_len))?;
        let mut frame = BytesMut::with_capacity(5 + payload_len);
        wire::write_var_u32(&mut frame, len);
        wire::write_var_u32(&mut frame, header_raw);
        let body_start = frame.len() as u32;
        frame.extend_from_slice(body);
        Ok(Self {
            header,
            name: packet_name(header.id),
            frame: frame.freeze(),
            body_start,
            compact: true,
        })
    }

    /// Encodes a valentine packet into a standalone raw packet.
    pub fn from_packet(packet: &McpePacket) -> Result<Self, JolyneError> {
        let mut frame = BytesMut::new();
        packet.data.encode_inner_bytes_mut(
            &mut frame,
            packet.header.from_subclient,
            packet.header.to_subclient,
        )?;
        let mut cursor = frame.freeze();
        decode_packet_raw(&mut cursor)
    }

    /// Encodes a custom packet into a raw packet. `P::ID` must be a ten-bit
    /// ID that valentine does not use.
    pub fn from_custom<P: CustomPacket>(packet: &P) -> Result<Self, JolyneError> {
        if P::ID > super::custom::MAX_PACKET_ID || packet_name(P::ID).is_some() {
            return Err(ProtocolError::InvalidPacketId(P::ID).into());
        }
        let mut body = BytesMut::new();
        packet.encode(&mut body)?;
        Self::new(PacketHeader::new(P::ID), &body)
    }

    /// The numeric packet ID (always present, including for unknown packets).
    #[inline]
    pub fn id(&self) -> u16 {
        self.header.id
    }

    /// The valentine name, or `None` for custom/unknown IDs.
    #[inline]
    pub fn name(&self) -> Option<McpePacketName> {
        self.name
    }

    /// Whether this packet is the given valentine packet.
    #[inline]
    pub fn is(&self, name: McpePacketName) -> bool {
        self.name == Some(name)
    }

    #[inline]
    pub fn header(&self) -> PacketHeader {
        self.header
    }

    /// The body (payload after the header varint).
    #[inline]
    pub fn body(&self) -> &[u8] {
        &self.frame[self.body_start as usize..]
    }

    /// The body as a shared `Bytes` slice of the batch allocation.
    pub fn body_bytes(&self) -> Bytes {
        self.frame.slice(self.body_start as usize..)
    }

    /// The complete inner frame `[Length][Header][Body]`, ready for batching.
    #[inline]
    pub fn inner_frame(&self) -> &Bytes {
        &self.frame
    }

    pub fn into_inner_frame(self) -> Bytes {
        self.frame
    }

    fn decode_error(&self, source: valentine::bedrock::error::DecodeError) -> JolyneError {
        let body = self.body();
        JolyneError::PacketDecode {
            packet_id: self.header.id,
            body_len: body.len(),
            body_preview: body.iter().take(32).copied().collect(),
            source,
        }
    }

    fn require_valentine(self) -> Result<Self, JolyneError> {
        if self.name.is_none() {
            return Err(JolyneError::NonValentinePacket(Box::new(self)));
        }
        Ok(self)
    }

    /// Decodes into an owned valentine packet.
    ///
    /// Custom and unknown IDs return [`JolyneError::NonValentinePacket`] with
    /// the packet handed back.
    pub fn decode(self) -> Result<McpePacket, JolyneError> {
        let this = self.require_valentine()?;
        let mut buf = this.frame.clone();
        let (header, data) = McpePacketData::decode_inner_bytes(&mut buf, McpePacketArgs)
            .map_err(|source| this.decode_error(source))?;
        if buf.has_remaining() {
            return Err(JolyneError::PacketTrailingBytes {
                packet_id: this.header.id,
                body_len: this.body().len(),
                remaining: buf.remaining(),
            });
        }
        Ok(McpePacket::new(header, data))
    }

    /// Decodes into a borrowed valentine view sharing this packet's bytes.
    pub fn decode_borrowed(self) -> Result<BorrowedMcpePacket, JolyneError> {
        let this = self.require_valentine()?;
        let mut buf = this.frame.clone();
        let (packet, payload_remaining) = BorrowedMcpePacket::decode_inner_with_remaining(&mut buf)
            .map_err(|source| this.decode_error(source))?;
        let remaining = buf.remaining() + payload_remaining;
        if remaining != 0 {
            return Err(JolyneError::PacketTrailingBytes {
                packet_id: this.header.id,
                body_len: this.body().len(),
                remaining,
            });
        }
        Ok(packet)
    }

    /// Decodes a registered custom packet with its user codec.
    pub fn decode_custom<P: CustomPacket>(&self) -> Result<P, JolyneError> {
        if self.header.id != P::ID {
            return Err(ProtocolError::NotCustomPacket(self.header.id).into());
        }
        P::decode(self.body()).map_err(|source| self.decode_error(source))
    }

    /// Copies this packet into an allocation sized for this frame alone.
    ///
    /// Raw packets decoded from a batch are slices of the decompressed batch
    /// allocation. Long-lived queues must compact those slices or one small
    /// packet can keep the entire batch resident.
    pub(crate) fn into_compact(self) -> Self {
        if self.compact {
            return self;
        }
        Self {
            frame: Bytes::copy_from_slice(&self.frame),
            compact: true,
            ..self
        }
    }
}

/// Parses one `[Length][Header][Body]` entry at the start of `input`.
///
/// Returns the header, the total frame size and the body offset.
#[inline]
fn parse_frame(input: &[u8]) -> Result<(PacketHeader, usize, usize), JolyneError> {
    let mut remaining = input;
    let declared = wire::read_var_u32(&mut remaining)? as usize;
    if remaining.len() < declared {
        return Err(ProtocolError::PacketLength {
            declared,
            available: remaining.len(),
        }
        .into());
    }
    let len_size = input.len() - remaining.len();
    let frame_size = len_size + declared;
    let mut payload = &remaining[..declared];
    let header_raw = wire::read_var_u32(&mut payload)?;
    let header = PacketHeader {
        id: (header_raw & 0x3FF) as u16,
        from_subclient: ((header_raw >> 10) & 0x3) as u8,
        to_subclient: ((header_raw >> 12) & 0x3) as u8,
    };
    Ok((header, frame_size, frame_size - payload.len()))
}

/// Decodes a single packet entry and advances `cursor` past it.
///
/// Unknown IDs are returned with `name() == None`.
pub fn decode_packet_raw(cursor: &mut Bytes) -> Result<RawPacket, JolyneError> {
    let (header, frame_size, body_start) = parse_frame(cursor)?;
    Ok(RawPacket {
        name: packet_name(header.id),
        header,
        frame: cursor.split_to(frame_size),
        body_start: body_start as u32,
        compact: false,
    })
}

/// Outcome counters for one decoded batch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BatchStats {
    /// Packets appended to the output queue.
    pub packets: usize,
    /// Unknown packets dropped under [`UnknownPacketPolicy::Skip`].
    pub skipped_unknown: usize,
    /// Unknown packets kept under [`UnknownPacketPolicy::Passthrough`].
    pub passed_unknown: usize,
}

/// Decodes every packet of a decompressed batch payload into `out`.
///
/// Applies the packet cap and the unknown-packet policy. On error, packets
/// already appended stay in `out` (callers treat batch errors as fatal).
pub(crate) fn decode_packets_into(
    payload: Bytes,
    filter: &PacketFilter,
    max_packets: usize,
    out: &mut VecDeque<RawPacket>,
) -> Result<BatchStats, JolyneError> {
    let mut stats = BatchStats::default();
    let mut offset = 0usize;
    let mut seen = 0usize;
    while offset < payload.len() {
        if seen == max_packets {
            return Err(ProtocolError::TooManyPackets { max: max_packets }.into());
        }
        seen += 1;
        let (header, frame_size, body_start) = parse_frame(&payload[offset..])?;
        let name = packet_name(header.id);
        if name.is_none() && !filter.custom.contains(header.id) {
            match filter.unknown {
                UnknownPacketPolicy::Reject => {
                    return Err(ProtocolError::UnknownPacket(header.id).into());
                }
                UnknownPacketPolicy::Skip => {
                    stats.skipped_unknown += 1;
                    offset += frame_size;
                    continue;
                }
                UnknownPacketPolicy::Passthrough => stats.passed_unknown += 1,
            }
        }
        out.push_back(RawPacket {
            header,
            name,
            frame: payload.slice(offset..offset + frame_size),
            body_start: body_start as u32,
            compact: false,
        });
        stats.packets += 1;
        offset += frame_size;
    }
    Ok(stats)
}

/// Decodes all packets of a batch payload, keeping unknown IDs.
pub fn decode_packets_raw(payload: Bytes) -> Result<Vec<RawPacket>, JolyneError> {
    let mut out = VecDeque::new();
    decode_packets_into(
        payload,
        &PacketFilter::PASSTHROUGH,
        DEFAULT_MAX_PACKETS_PER_BATCH,
        &mut out,
    )?;
    Ok(out.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::custom::PacketRegistry;

    fn frame(packet_id: u32, from_sub: u32, to_sub: u32, body: &[u8]) -> Bytes {
        let header_raw = packet_id | (from_sub << 10) | (to_sub << 12);
        let mut payload = BytesMut::new();
        wire::write_var_u32(&mut payload, header_raw);
        payload.extend_from_slice(body);
        let mut frame = BytesMut::new();
        wire::write_var_u32(&mut frame, payload.len() as u32);
        frame.extend_from_slice(&payload);
        frame.freeze()
    }

    fn concat(frames: &[Bytes]) -> Bytes {
        let mut out = BytesMut::new();
        for frame in frames {
            out.extend_from_slice(frame);
        }
        out.freeze()
    }

    #[test]
    fn parses_ids_subclients_and_multibyte_ids() {
        let mut cursor = frame(0x02, 1, 2, &[0, 0, 0, 1]);
        let raw = decode_packet_raw(&mut cursor).unwrap();
        assert_eq!(raw.name(), Some(McpePacketName::PlayStatusPacket));
        assert_eq!(raw.header().from_subclient, 1);
        assert_eq!(raw.header().to_subclient, 2);
        assert_eq!(raw.body(), &[0, 0, 0, 1]);

        let mut cursor = frame(193, 3, 3, &[]);
        let raw = decode_packet_raw(&mut cursor).unwrap();
        assert_eq!(
            raw.name(),
            Some(McpePacketName::RequestNetworkSettingsPacket)
        );
        assert_eq!(
            (raw.header().from_subclient, raw.header().to_subclient),
            (3, 3)
        );
    }

    #[test]
    fn unknown_ids_keep_numeric_identity_and_roundtrip_bytes() {
        let original = frame(1000, 1, 0, &[9, 8, 7]);
        let mut cursor = original.clone();
        let raw = decode_packet_raw(&mut cursor).unwrap();
        assert_eq!(raw.id(), 1000);
        assert_eq!(raw.name(), None);
        assert_eq!(raw.inner_frame(), &original);
        assert!(matches!(
            raw.clone().decode(),
            Err(JolyneError::NonValentinePacket(packet)) if packet.id() == 1000
        ));
        let rebuilt = RawPacket::new(raw.header(), raw.body()).unwrap();
        assert_eq!(rebuilt.inner_frame(), &original);
    }

    #[test]
    fn unknown_policy_rejects_skips_or_passes_through() {
        let batch = concat(&[
            frame(2, 0, 0, &[0; 4]),
            frame(1000, 0, 0, &[1]),
            frame(2, 0, 0, &[0; 4]),
        ]);
        let mut out = VecDeque::new();

        let reject = PacketFilter::default();
        assert!(matches!(
            decode_packets_into(batch.clone(), &reject, 16, &mut out),
            Err(JolyneError::Protocol(ProtocolError::UnknownPacket(1000)))
        ));

        out.clear();
        let skip = PacketFilter {
            unknown: UnknownPacketPolicy::Skip,
            ..Default::default()
        };
        let stats = decode_packets_into(batch.clone(), &skip, 16, &mut out).unwrap();
        assert_eq!((stats.packets, stats.skipped_unknown), (2, 1));
        assert!(out.iter().all(|packet| packet.id() == 2));

        out.clear();
        let stats =
            decode_packets_into(batch.clone(), &PacketFilter::PASSTHROUGH, 16, &mut out).unwrap();
        assert_eq!((stats.packets, stats.passed_unknown), (3, 1));
        assert_eq!(out[1].id(), 1000);

        // Registered custom IDs are known even under Reject.
        out.clear();
        let mut custom = PacketRegistry::default();
        custom.register_id(1000).unwrap();
        let registered = PacketFilter {
            unknown: UnknownPacketPolicy::Reject,
            custom,
        };
        let stats = decode_packets_into(batch, &registered, 16, &mut out).unwrap();
        assert_eq!((stats.packets, stats.passed_unknown), (3, 0));
    }

    #[test]
    fn rejects_truncated_and_overlong_frames() {
        assert!(decode_packet_raw(&mut Bytes::from_static(&[0x80])).is_err());
        let mut overlong = BytesMut::new();
        wire::write_var_u32(&mut overlong, 100);
        overlong.extend_from_slice(&[1, 2, 3]);
        assert!(matches!(
            decode_packet_raw(&mut overlong.freeze()),
            Err(JolyneError::Protocol(ProtocolError::PacketLength {
                declared: 100,
                available: 3
            }))
        ));
    }

    #[test]
    fn packet_cap_is_exact() {
        let one = frame(2, 0, 0, &[]);
        let at = concat(&vec![one.clone(); 1_600]);
        assert_eq!(decode_packets_raw(at).unwrap().len(), 1_600);
        let over = concat(&vec![one; 1_601]);
        assert!(matches!(
            decode_packets_raw(over),
            Err(JolyneError::Protocol(ProtocolError::TooManyPackets {
                max: 1_600
            }))
        ));
    }

    #[test]
    fn decode_error_reports_id_and_bounded_preview() {
        // Text/Raw declares a 127-byte message but only carries 36 bytes.
        let mut body = vec![0x00, 0x00, 0x00, 0x7f];
        body.extend(0u8..36);
        let raw = decode_packet_raw(&mut frame(0x09, 0, 0, &body)).unwrap();
        match raw.decode().expect_err("truncated Text must fail") {
            JolyneError::PacketDecode {
                packet_id,
                body_len,
                body_preview,
                ..
            } => {
                assert_eq!(packet_id, 9);
                assert_eq!(body_len, body.len());
                assert_eq!(body_preview, &body[..32]);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn decode_rejects_trailing_body_bytes_in_both_paths() {
        let raw = decode_packet_raw(&mut frame(0x02, 0, 0, &[0, 0, 0, 0, 0xaa])).unwrap();
        // The owned decoder rejects the leftover byte inside the bounded frame.
        assert!(matches!(
            raw.clone().decode(),
            Err(JolyneError::PacketDecode { packet_id: 2, .. }
                | JolyneError::PacketTrailingBytes { remaining: 1, .. })
        ));
        assert!(matches!(
            raw.decode_borrowed(),
            Err(JolyneError::PacketTrailingBytes { remaining: 1, .. })
        ));
    }

    #[test]
    fn compact_copy_detaches_from_batch_allocation() {
        let batch = concat(&[frame(2, 0, 0, &[0; 4]), frame(2, 0, 0, &[0; 4])]);
        let start = batch.as_ptr() as usize;
        let mut out = VecDeque::new();
        decode_packets_into(batch.clone(), &PacketFilter::PASSTHROUGH, 8, &mut out).unwrap();
        let compact = out.pop_front().unwrap().into_compact();
        let pointer = compact.inner_frame().as_ptr() as usize;
        assert!(pointer < start || pointer >= start + batch.len());
        assert_eq!(compact.body(), &[0; 4]);
    }

    /// Regression: custom packets were framed without an ID check, so ID
    /// 1025 went out as `Login` (1025 & 0x3FF == 1).
    #[test]
    fn custom_packets_must_use_free_ten_bit_ids() {
        struct Wide;
        impl CustomPacket for Wide {
            const ID: u16 = 1025;
            fn decode(_: &[u8]) -> Result<Self, valentine::bedrock::error::DecodeError> {
                Ok(Self)
            }
            fn encode(&self, _: &mut BytesMut) -> Result<(), std::io::Error> {
                Ok(())
            }
        }
        struct Shadow;
        impl CustomPacket for Shadow {
            const ID: u16 = 1;
            fn decode(_: &[u8]) -> Result<Self, valentine::bedrock::error::DecodeError> {
                Ok(Self)
            }
            fn encode(&self, _: &mut BytesMut) -> Result<(), std::io::Error> {
                Ok(())
            }
        }
        for result in [
            RawPacket::from_custom(&Wide),
            RawPacket::from_custom(&Shadow),
        ] {
            assert!(matches!(
                result,
                Err(JolyneError::Protocol(ProtocolError::InvalidPacketId(_)))
            ));
        }
        assert!(RawPacket::new(PacketHeader::new(1024), &[]).is_err());
    }
}
