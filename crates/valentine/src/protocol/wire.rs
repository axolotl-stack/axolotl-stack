use bytes::{Buf, BufMut};

#[inline]
pub fn write_var_u32<B: BufMut>(buf: &mut B, mut v: u32) {
    while v >= 0x80 {
        buf.put_u8((v as u8) | 0x80);
        v >>= 7;
    }
    buf.put_u8(v as u8);
}

#[inline]
pub fn read_var_u32<B: Buf>(buf: &mut B) -> Result<u32, std::io::Error> {
    let mut result: u32 = 0;
    let mut shift = 0u32;
    loop {
        if !buf.has_remaining() {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "varu32 eof"));
        }
        let byte = buf.get_u8();
        result |= ((byte & 0x7F) as u32) << shift;
        if (byte & 0x80) == 0 {
            break;
        }
        shift += 7;
        if shift >= 35 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "varu32 too long"));
        }
    }
    Ok(result)
}

#[inline]
pub fn write_var_u64<B: BufMut>(buf: &mut B, mut v: u64) {
    while v >= 0x80 {
        buf.put_u8((v as u8) | 0x80);
        v >>= 7;
    }
    buf.put_u8(v as u8);
}

#[inline]
pub fn read_var_u64<B: Buf>(buf: &mut B) -> Result<u64, std::io::Error> {
    let mut result: u64 = 0;
    let mut shift = 0u32;
    loop {
        if !buf.has_remaining() {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "varu64 eof"));
        }
        let byte = buf.get_u8();
        result |= ((byte & 0x7F) as u64) << shift;
        if (byte & 0x80) == 0 {
            break;
        }
        shift += 7;
        if shift >= 70 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "varu64 too long"));
        }
    }
    Ok(result)
}

#[inline]
pub fn zigzag32_encode(v: i32) -> u32 {
    ((v << 1) ^ (v >> 31)) as u32
}

#[inline]
pub fn zigzag32_decode(v: u32) -> i32 {
    ((v >> 1) as i32) ^ (-((v & 1) as i32))
}

#[inline]
pub fn zigzag64_encode(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

#[inline]
pub fn zigzag64_decode(v: u64) -> i64 {
    ((v >> 1) as i64) ^ (-((v & 1) as i64))
}

#[inline]
pub fn write_zigzag32<B: BufMut>(buf: &mut B, v: i32) {
    write_var_u32(buf, zigzag32_encode(v));
}

#[inline]
pub fn read_zigzag32<B: Buf>(buf: &mut B) -> Result<i32, std::io::Error> {
    Ok(zigzag32_decode(read_var_u32(buf)?))
}

#[inline]
pub fn write_zigzag64<B: BufMut>(buf: &mut B, v: i64) {
    write_var_u64(buf, zigzag64_encode(v));
}

#[inline]
pub fn read_zigzag64<B: Buf>(buf: &mut B) -> Result<i64, std::io::Error> {
    Ok(zigzag64_decode(read_var_u64(buf)?))
}

