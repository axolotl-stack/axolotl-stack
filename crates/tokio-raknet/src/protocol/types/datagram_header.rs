use bytes::{Buf, BufMut};

use crate::protocol::{
    constants::DatagramFlags,
    packet::{DecodeError, RaknetEncodable},
    types::Sequence24,
};

#[derive(Debug, Clone)]
pub struct DatagramHeader {
    pub flags: DatagramFlags,
    pub sequence: Sequence24,
}

impl RaknetEncodable for DatagramHeader {
    fn encode_raknet(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        dst.put_u8(self.flags.bits());
        self.sequence.encode_raknet(dst)
    }

    fn decode_raknet(src: &mut impl Buf) -> Result<Self, DecodeError> {
        if src.remaining() < 4 {
            return Err(DecodeError::UnexpectedEof);
        }
        let raw_flags = src.get_u8();
        let flags = DatagramFlags::from_bits_truncate(raw_flags);
        let sequence = Sequence24::decode_raknet(src)?;
        Ok(DatagramHeader { flags, sequence })
    }
}

impl Default for DatagramHeader {
    fn default() -> Self {
        Self {
            flags: DatagramFlags::empty(),
            sequence: Sequence24::new(0),
        }
    }
}
