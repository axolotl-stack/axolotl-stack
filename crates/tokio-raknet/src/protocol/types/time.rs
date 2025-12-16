use bytes::{Buf, BufMut};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::protocol::packet::{DecodeError, RaknetEncodable};

pub static START_TIME: OnceLock<Instant> = OnceLock::new();

pub fn raknet_start_time() -> Instant {
    *START_TIME.get_or_init(Instant::now)
}

/// Milliseconds of a duration as used on the RakNet wire format.
///
/// This represents the elapsed time since [`raknet_start_time`].
#[derive(Debug, Clone, Copy)]
pub struct RaknetTime(pub u64); // ms on wire

impl RaknetEncodable for RaknetTime {
    fn encode_raknet(
        &self,
        dst: &mut impl BufMut,
    ) -> Result<(), crate::protocol::packet::EncodeError> {
        self.0.encode_raknet(dst)
    }

    fn decode_raknet(src: &mut impl Buf) -> Result<Self, DecodeError> {
        Ok(Self(u64::decode_raknet(src)?))
    }
}

impl From<RaknetTime> for Duration {
    fn from(value: RaknetTime) -> Self {
        Duration::from_millis(value.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn raknet_time_roundtrip_and_conversion() {
        let value = RaknetTime(1234);
        let mut buf = BytesMut::new();
        value.encode_raknet(&mut buf).unwrap();
        let mut slice = buf.freeze();
        let decoded = RaknetTime::decode_raknet(&mut slice).unwrap();
        assert_eq!(decoded.0, value.0);

        let duration: Duration = decoded.into();
        assert_eq!(duration.as_millis(), 1234);
    }
}
