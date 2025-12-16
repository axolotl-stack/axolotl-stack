use bytes::{Buf, BufMut};

use crate::protocol::packet::{DecodeError, RaknetEncodable};

/// Optional server advertisement payload used by some discovery packets.
#[derive(Debug, Clone)]
pub struct Advertisement(pub Option<bytes::Bytes>);

impl RaknetEncodable for Advertisement {
    fn encode_raknet(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        if let Some(ad_bytes) = &self.0
            && !ad_bytes.is_empty()
        {
            // Ensure length fits in u16
            let len = ad_bytes.len().min(u16::MAX as usize) as u16;
            dst.put_u16(len);
            dst.put_slice(&ad_bytes[..len as usize]);
        }
        // If self.0 is None or empty, NOP
        Ok(())
    }

    fn decode_raknet(src: &mut impl Buf) -> Result<Self, DecodeError> {
        let ad = if src.has_remaining() {
            // Check for at least the length prefix
            if src.remaining() < 2 {
                return Err(DecodeError::UnexpectedEof);
            }
            let len = src.get_u16() as usize;

            // Check if we have enough data for the payload
            if src.remaining() < len {
                return Err(DecodeError::UnexpectedEof);
            }
            Some(src.copy_to_bytes(len))
        } else {
            // No data left, so the field was omitted
            None
        };
        Ok(Advertisement(ad))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn advertisement_some_roundtrip() {
        let payload = bytes::Bytes::from_static(b"hello");
        let adv = Advertisement(Some(payload.clone()));
        let mut buf = BytesMut::new();
        adv.encode_raknet(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = Advertisement::decode_raknet(&mut slice).unwrap();
        assert!(decoded.0.is_some());
        assert_eq!(&decoded.0.unwrap()[..], &payload[..]);
    }

    #[test]
    fn advertisement_none_encodes_to_nothing() {
        let adv = Advertisement(None);
        let mut buf = BytesMut::new();
        adv.encode_raknet(&mut buf).unwrap();
        assert_eq!(buf.len(), 0);
    }
}
