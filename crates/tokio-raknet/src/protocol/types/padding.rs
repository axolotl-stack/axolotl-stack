use bytes::{Buf, BufMut};

use crate::protocol::packet::{DecodeError, EncodeError, RaknetEncodable};

/// End-of-buffer padding – consumes or emits zero bytes up to the end.
///
/// On encode this writes `len` zero bytes. On decode it advances to the
/// end of the buffer and records how many bytes were skipped.
#[derive(Debug, Clone, Copy)]
pub struct EoBPadding(pub usize);

impl RaknetEncodable for EoBPadding {
    fn encode_raknet(&self, dst: &mut impl BufMut) -> Result<(), EncodeError> {
        dst.put_bytes(0, self.0);
        Ok(())
    }

    fn decode_raknet(src: &mut impl Buf) -> Result<Self, DecodeError> {
        let len = src.remaining();
        src.advance(len);
        Ok(EoBPadding(len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn eobpadding_consumes_remaining() {
        let buf = BytesMut::from(&b"\x01\x02\x03"[..]);
        let mut slice = buf.freeze();
        let padding = EoBPadding::decode_raknet(&mut slice).unwrap();
        assert_eq!(padding.0, 3);
        assert_eq!(slice.remaining(), 0);
    }
}
