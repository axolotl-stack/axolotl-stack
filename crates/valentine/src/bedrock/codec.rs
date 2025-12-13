use bytes::{Buf, BufMut};
use std::mem;
use uuid::Uuid;

use crate::bedrock::context::BedrockSession;
use crate::protocol::wire;

/// Bedrock binary codec for encode/decode on the wire.
pub trait BedrockCodec: Sized {
    type Args;

    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error>;
    fn decode<B: Buf>(buf: &mut B, args: Self::Args) -> Result<Self, std::io::Error>;
}

#[derive(Clone)]
pub struct ProtocolArgs<'a> {
    pub shield_id: i32,
    pub session: &'a BedrockSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ZigZag32(pub i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ZigZag64(pub i64);

macro_rules! le_int_newtype {
    ($name:ident, $inner:ty, $put:ident, $get:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(pub $inner);

        impl BedrockCodec for $name {
            type Args = ();
            fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
                buf.$put(self.0);
                Ok(())
            }
            fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
                if buf.remaining() < mem::size_of::<$inner>() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        concat!(stringify!($name), " eof"),
                    ));
                }
                Ok(Self(buf.$get()))
            }
        }
    };
}

macro_rules! le_float_newtype {
    ($name:ident, $inner:ty, $put:ident, $get:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct $name(pub $inner);

        impl BedrockCodec for $name {
            type Args = ();
            fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
                buf.$put(self.0);
                Ok(())
            }
            fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
                if buf.remaining() < mem::size_of::<$inner>() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        concat!(stringify!($name), " eof"),
                    ));
                }
                Ok(Self(buf.$get()))
            }
        }
    };
}

le_int_newtype!(U16LE, u16, put_u16_le, get_u16_le);
le_int_newtype!(I16LE, i16, put_i16_le, get_i16_le);
le_int_newtype!(U32LE, u32, put_u32_le, get_u32_le);
le_int_newtype!(I32LE, i32, put_i32_le, get_i32_le);
le_int_newtype!(U64LE, u64, put_u64_le, get_u64_le);
le_int_newtype!(I64LE, i64, put_i64_le, get_i64_le);
le_float_newtype!(F32LE, f32, put_f32_le, get_f32_le);
le_float_newtype!(F64LE, f64, put_f64_le, get_f64_le);

impl BedrockCodec for ZigZag32 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        wire::write_zigzag32(buf, self.0);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
        Ok(ZigZag32(wire::read_zigzag32(buf)?))
    }
}

impl BedrockCodec for ZigZag64 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        wire::write_zigzag64(buf, self.0);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
        Ok(ZigZag64(wire::read_zigzag64(buf)?))
    }
}

impl BedrockCodec for bool {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_u8(u8::from(*self));
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
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
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_u8(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
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
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_i8(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
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
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_u16(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
        if buf.remaining() < 2 {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "u16 eof",
            ))
        } else {
            Ok(buf.get_u16())
        }
    }
}
impl BedrockCodec for i16 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_i16(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
        if buf.remaining() < 2 {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "i16 eof",
            ))
        } else {
            Ok(buf.get_i16())
        }
    }
}
impl BedrockCodec for u32 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_u32(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
        if buf.remaining() < 4 {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "u32 eof",
            ))
        } else {
            Ok(buf.get_u32())
        }
    }
}
impl BedrockCodec for i32 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_i32(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
        if buf.remaining() < 4 {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "i32 eof",
            ))
        } else {
            Ok(buf.get_i32())
        }
    }
}
impl BedrockCodec for u64 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_u64(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
        if buf.remaining() < 8 {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "u64 eof",
            ))
        } else {
            Ok(buf.get_u64())
        }
    }
}
impl BedrockCodec for i64 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_i64(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
        if buf.remaining() < 8 {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "i64 eof",
            ))
        } else {
            Ok(buf.get_i64())
        }
    }
}

impl BedrockCodec for f32 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_f32(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
        if buf.remaining() < 4 {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "f32 eof",
            ))
        } else {
            Ok(buf.get_f32())
        }
    }
}

impl BedrockCodec for f64 {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_f64(*self);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
        if buf.remaining() < 8 {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "f64 eof",
            ))
        } else {
            Ok(buf.get_f64())
        }
    }
}

impl BedrockCodec for String {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        let bytes = self.as_bytes();
        crate::protocol::wire::write_var_u32(buf, bytes.len() as u32);
        buf.put_slice(bytes);
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
        let len = crate::protocol::wire::read_var_u32(buf)? as usize;
        if buf.remaining() < len {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "string eof",
            ));
        }
        let mut v = vec![0u8; len];
        buf.copy_to_slice(&mut v);
        // Bedrock strings may contain arbitrary bytes; tolerate invalid UTF-8 by lossily decoding.
        Ok(String::from_utf8_lossy(&v).into_owned())
    }
}

impl<T: BedrockCodec> BedrockCodec for Box<T> {
    type Args = T::Args;
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        (**self).encode(buf)
    }
    fn decode<B: Buf>(buf: &mut B, args: Self::Args) -> Result<Self, std::io::Error> {
        Ok(Box::new(T::decode(buf, args)?))
    }
}

impl<T: BedrockCodec> BedrockCodec for Vec<T>
where
    T::Args: Clone,
{
    type Args = T::Args;
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        crate::protocol::wire::write_var_u32(buf, self.len() as u32);
        for item in self {
            item.encode(buf)?;
        }
        Ok(())
    }
    fn decode<B: Buf>(buf: &mut B, args: Self::Args) -> Result<Self, std::io::Error> {
        let len = crate::protocol::wire::read_var_u32(buf)? as usize;
        let mut v = Vec::with_capacity(len);
        for _ in 0..len {
            v.push(T::decode(buf, args.clone())?);
        }
        Ok(v)
    }
}

impl<T: BedrockCodec> BedrockCodec for Option<T> {
    type Args = T::Args;
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
    fn decode<B: Buf>(buf: &mut B, args: Self::Args) -> Result<Self, std::io::Error> {
        let present = u8::decode(buf, ())?;
        if present != 0 {
            Ok(Some(T::decode(buf, args)?))
        } else {
            Ok(None)
        }
    }
}

impl BedrockCodec for uuid::Uuid {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        buf.put_slice(self.as_bytes());
        Ok(())
    }

    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VarInt(pub i32);

impl BedrockCodec for VarInt {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        let mut x = self.0 as u32;
        loop {
            let mut temp = (x & 0x7F) as u8;
            x >>= 7;
            if x != 0 {
                temp |= 0x80;
                buf.put_u8(temp);
            } else {
                buf.put_u8(temp);
                break;
            }
        }
        Ok(())
    }

    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
        let mut result = 0;
        let mut shift = 0;
        loop {
            if !buf.has_remaining() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "VarInt EOF",
                ));
            }
            let byte = buf.get_u8();
            result |= ((byte & 0x7F) as i32) << shift;
            if (byte & 0x80) == 0 {
                return Ok(VarInt(result));
            }
            shift += 7;
            if shift >= 35 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "VarInt too large",
                ));
            }
        }
    }
}

// --- VarLong Wrapper ---
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VarLong(pub i64);

impl BedrockCodec for VarLong {
    type Args = ();
    fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        let mut x = self.0 as u64;
        loop {
            let mut temp = (x & 0x7F) as u8;
            x >>= 7;
            if x != 0 {
                temp |= 0x80;
                buf.put_u8(temp);
            } else {
                buf.put_u8(temp);
                break;
            }
        }
        Ok(())
    }

    fn decode<B: Buf>(buf: &mut B, _args: Self::Args) -> Result<Self, std::io::Error> {
        let mut result = 0;
        let mut shift = 0;
        loop {
            if !buf.has_remaining() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "VarLong EOF",
                ));
            }
            let byte = buf.get_u8();
            result |= ((byte & 0x7F) as i64) << shift;
            if (byte & 0x80) == 0 {
                return Ok(VarLong(result));
            }
            shift += 7;
            if shift >= 70 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "VarLong too large",
                ));
            }
        }
    }
}

pub trait GamePacket: BedrockCodec {
    type PacketId;
    const PACKET_ID: Self::PacketId;
}
