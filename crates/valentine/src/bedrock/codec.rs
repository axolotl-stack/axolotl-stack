use bytes::{Buf, BufMut};
use uuid::Uuid;

use crate::protocol::wire;

/// Bedrock binary codec for encode/decode on the wire.
pub trait BedrockCodec: Sized {
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error>;
    fn decode<B: Buf>(buf: &mut B) -> Result<Self, std::io::Error>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ZigZag32(pub i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ZigZag64(pub i64);

impl BedrockCodec for ZigZag32 {
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        wire::write_zigzag32(buf, self.0);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B) -> Result<Self, std::io::Error> {
        Ok(ZigZag32(wire::read_zigzag32(buf)?))
    }
}

impl BedrockCodec for ZigZag64 {
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        wire::write_zigzag64(buf, self.0);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B) -> Result<Self, std::io::Error> {
        Ok(ZigZag64(wire::read_zigzag64(buf)?))
    }
}

impl BedrockCodec for bool {
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_u8(u8::from(*self));
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B) -> Result<Self, std::io::Error> {
        if !buf.has_remaining() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "bool eof",
            ));
        }
        Ok(buf.get_u8() != 0)
    }
}

impl BedrockCodec for u8 {
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_u8(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B) -> Result<Self, std::io::Error> {
        if !buf.has_remaining() {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "u8 eof",
            ))
        } else {
            Ok(buf.get_u8())
        }
    }
}
impl BedrockCodec for i8 {
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_i8(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B) -> Result<Self, std::io::Error> {
        if !buf.has_remaining() {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "i8 eof",
            ))
        } else {
            Ok(buf.get_i8())
        }
    }
}
impl BedrockCodec for u16 {
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_u16_le(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B) -> Result<Self, std::io::Error> {
        if buf.remaining() < 2 {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "u16 eof",
            ))
        } else {
            Ok(buf.get_u16_le())
        }
    }
}
impl BedrockCodec for i16 {
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_i16_le(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B) -> Result<Self, std::io::Error> {
        if buf.remaining() < 2 {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "i16 eof",
            ))
        } else {
            Ok(buf.get_i16_le())
        }
    }
}
impl BedrockCodec for u32 {
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_u32_le(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B) -> Result<Self, std::io::Error> {
        if buf.remaining() < 4 {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "u32 eof",
            ))
        } else {
            Ok(buf.get_u32_le())
        }
    }
}
impl BedrockCodec for i32 {
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_i32_le(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B) -> Result<Self, std::io::Error> {
        if buf.remaining() < 4 {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "i32 eof",
            ))
        } else {
            Ok(buf.get_i32_le())
        }
    }
}
impl BedrockCodec for u64 {
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_u64_le(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B) -> Result<Self, std::io::Error> {
        if buf.remaining() < 8 {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "u64 eof",
            ))
        } else {
            Ok(buf.get_u64_le())
        }
    }
}
impl BedrockCodec for i64 {
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_i64_le(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B) -> Result<Self, std::io::Error> {
        if buf.remaining() < 8 {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "i64 eof",
            ))
        } else {
            Ok(buf.get_i64_le())
        }
    }
}

impl BedrockCodec for f32 {
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_f32_le(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B) -> Result<Self, std::io::Error> {
        if buf.remaining() < 4 {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "f32 eof",
            ))
        } else {
            Ok(buf.get_f32_le())
        }
    }
}

impl BedrockCodec for f64 {
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_f64_le(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B) -> Result<Self, std::io::Error> {
        if buf.remaining() < 8 {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "f64 eof",
            ))
        } else {
            Ok(buf.get_f64_le())
        }
    }
}

impl BedrockCodec for String {
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        let bytes = self.as_bytes();
        crate::protocol::wire::write_var_u32(buf, bytes.len() as u32);
        buf.put_slice(bytes);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B) -> Result<Self, std::io::Error> {
        let len = crate::protocol::wire::read_var_u32(buf)? as usize;
        if buf.remaining() < len {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "string eof",
            ));
        }
        let mut v = vec![0u8; len];
        buf.copy_to_slice(&mut v);
        String::from_utf8(v).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

impl<T: BedrockCodec> BedrockCodec for Vec<T> {
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        crate::protocol::wire::write_var_u32(buf, self.len() as u32);
        for item in self {
            item.encode(buf)?;
        }
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B) -> Result<Self, std::io::Error> {
        let len = crate::protocol::wire::read_var_u32(buf)? as usize;
        let mut v = Vec::with_capacity(len);
        for _ in 0..len {
            v.push(T::decode(buf)?);
        }
        Ok(v)
    }
}

impl<T: BedrockCodec> BedrockCodec for Option<T> {
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        match self {
            Some(v) => {
                buf.put_u8(1);
                v.encode(buf)?;
            }
            None => {
                buf.put_u8(0);
            }
        }
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B) -> Result<Self, std::io::Error> {
        let present = u8::decode(buf)?;
        if present != 0 {
            Ok(Some(T::decode(buf)?))
        } else {
            Ok(None)
        }
    }
}

impl BedrockCodec for uuid::Uuid {
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_slice(self.as_bytes());
        Ok(())
    }

    fn decode<B: Buf>(buf: &mut B) -> Result<Self, std::io::Error> {
        if buf.remaining() < 16 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "UUID eof",
            ));
        }
        let mut bytes = [0u8; 16];
        buf.copy_to_slice(&mut bytes);
        Ok(uuid::Uuid::from_bytes(bytes))
    }
}