use bytes::{Buf, Bytes};
use std::borrow::Cow;

use crate::bedrock::codec::{BedrockCodec, BedrockSized, U32LE, VarInt, VarUInt};
use crate::bedrock::error::DecodeError;
use crate::protocol::wire;

pub const GAME_PACKET_ID: u8 = 0xfe;

/// Zero-copy decode for `Bytes`-backed protocol views.
pub trait BedrockBorrowDecode: Sized {
    type Args;

    fn borrow_decode<B: Buf>(buf: &mut B, args: Self::Args) -> Result<Self, DecodeError>;
}

/// A bounded, non-owning cursor over shared packet input.
///
/// Numeric fields only advance a slice. `copy_to_bytes` shares the source only
/// when a decoded field actually retains bytes; framing itself never clones it.
pub struct BorrowedReader<'a> {
    source: &'a Bytes,
    data: &'a [u8],
}

impl<'a> BorrowedReader<'a> {
    /// The framing layer must validate the declared length before construction.
    #[inline]
    pub fn new(source: &'a Bytes, len: usize) -> Self {
        Self {
            source,
            data: &source[..len],
        }
    }

    /// Advances this cursor and returns an independently bounded frame cursor.
    /// The framing layer must validate the declared length before calling this.
    #[inline]
    pub fn take_frame(&mut self, len: usize) -> BorrowedReader<'a> {
        let (data, remaining) = self.data.split_at(len);
        self.data = remaining;
        BorrowedReader {
            source: self.source,
            data,
        }
    }
}

impl Buf for BorrowedReader<'_> {
    #[inline]
    fn remaining(&self) -> usize {
        self.data.len()
    }

    #[inline]
    fn chunk(&self) -> &[u8] {
        self.data
    }

    #[inline]
    fn advance(&mut self, count: usize) {
        self.data = &self.data[count..];
    }

    #[inline]
    fn copy_to_bytes(&mut self, len: usize) -> Bytes {
        let bytes = self.source.slice_ref(&self.data[..len]);
        self.data = &self.data[len..];
        bytes
    }
}

/// Borrowed UTF-8-ish string storage backed by `Bytes`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct BorrowedStr(Bytes);

impl BorrowedStr {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn as_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.0)
    }

    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.0)
    }

    pub fn into_bytes(self) -> Bytes {
        self.0
    }
}

impl From<Bytes> for BorrowedStr {
    fn from(value: Bytes) -> Self {
        Self(value)
    }
}

impl AsRef<[u8]> for BorrowedStr {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl BedrockSized for BorrowedStr {
    fn encoded_size(&self) -> usize {
        self.0.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawMcpeHeader {
    pub id_raw: u32,
    pub from_subclient: u32,
    pub to_subclient: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawMcpeFrame {
    pub header: RawMcpeHeader,
    pub payload: Bytes,
}

impl RawMcpeFrame {
    /// Decodes `[0xFE] [Length] [Header] [Body]` directly from a `Bytes` buffer.
    pub fn decode(buf: &mut Bytes) -> Result<Self, DecodeError> {
        if !buf.has_remaining() {
            return Err(DecodeError::UnexpectedEof {
                needed: 1,
                available: 0,
            });
        }

        let leading = buf.get_u8();
        if leading != GAME_PACKET_ID {
            return Err(DecodeError::InvalidMagicByte {
                expected: GAME_PACKET_ID,
                actual: leading,
            });
        }

        let declared_len = wire::read_var_u32(buf)? as usize;
        let mut frame = take_exact_packet_bytes(buf, declared_len)?;
        let header_raw = wire::read_var_u32(&mut frame)?;
        let payload = frame.split_to(frame.remaining());

        Ok(Self {
            header: RawMcpeHeader {
                id_raw: header_raw & 0x3ff,
                from_subclient: (header_raw >> 10) & 0x3,
                to_subclient: (header_raw >> 12) & 0x3,
            },
            payload,
        })
    }
}

pub fn take_var_u32_prefixed_bytes<B: Buf>(buf: &mut B) -> Result<Bytes, DecodeError> {
    let len = VarUInt::decode(buf, ())?.0 as usize;
    take_exact_array_bytes(buf, len)
}

pub fn take_varint_prefixed_bytes<B: Buf>(buf: &mut B) -> Result<Bytes, DecodeError> {
    let len_raw = VarInt::decode(buf, ())?.0 as i64;
    if len_raw < 0 {
        return Err(DecodeError::NegativeLength { value: len_raw });
    }
    take_exact_array_bytes(buf, len_raw as usize)
}

pub fn take_u32le_prefixed_bytes<B: Buf>(buf: &mut B) -> Result<Bytes, DecodeError> {
    let len = U32LE::decode(buf, ())?.0 as usize;
    take_exact_string_bytes(buf, len)
}

pub fn take_var_u32_prefixed_string<B: Buf>(buf: &mut B) -> Result<BorrowedStr, DecodeError> {
    Ok(BorrowedStr(take_var_u32_prefixed_bytes(buf)?))
}

pub fn take_varint_prefixed_string<B: Buf>(buf: &mut B) -> Result<BorrowedStr, DecodeError> {
    Ok(BorrowedStr(take_varint_prefixed_bytes(buf)?))
}

pub fn take_u32le_prefixed_string<B: Buf>(buf: &mut B) -> Result<BorrowedStr, DecodeError> {
    Ok(BorrowedStr(take_u32le_prefixed_bytes(buf)?))
}

fn take_exact_array_bytes<B: Buf>(buf: &mut B, len: usize) -> Result<Bytes, DecodeError> {
    if buf.remaining() < len {
        return Err(DecodeError::ArrayLengthExceeded {
            declared: len,
            available: buf.remaining(),
        });
    }
    Ok(buf.copy_to_bytes(len))
}

fn take_exact_packet_bytes(buf: &mut Bytes, len: usize) -> Result<Bytes, DecodeError> {
    if buf.remaining() < len {
        return Err(DecodeError::PacketLengthExceeded {
            declared: len,
            available: buf.remaining(),
        });
    }
    Ok(buf.split_to(len))
}

fn take_exact_string_bytes<B: Buf>(buf: &mut B, len: usize) -> Result<Bytes, DecodeError> {
    if buf.remaining() < len {
        return Err(DecodeError::StringLengthExceeded {
            declared: len,
            available: buf.remaining(),
        });
    }
    Ok(buf.copy_to_bytes(len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_reader_shares_fields_without_exposing_the_next_frame() {
        let source = Bytes::from(vec![1, 2, 3, 4]);
        let mut reader = BorrowedReader::new(&source, source.len());
        let mut frame = reader.take_frame(2);
        assert_eq!(frame.get_u8(), 1);
        let retained = frame.copy_to_bytes(1);
        assert_eq!(retained.as_ptr(), source[1..].as_ptr());
        assert_eq!(frame.remaining(), 0);
        assert!(frame.chunk().is_empty());
        assert_eq!(reader.remaining(), 2);
        assert_eq!(reader.get_u8(), 3);
        assert_eq!(reader.get_u8(), 4);
        drop(source);
        assert_eq!(retained.as_ref(), &[2]);
    }
}
