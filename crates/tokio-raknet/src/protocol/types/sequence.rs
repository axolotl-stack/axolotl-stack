use std::ops::Add;

use crate::protocol::{
    packet::{EncodeError, RaknetEncodable},
    types::U24LE,
};

const MODULO: u32 = 1 << 24;
const MASK: u32 = MODULO - 1;
const HALF: u32 = MODULO / 2;

/// Sequence type for a U24.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct Sequence24(u32);

impl Sequence24 {
    pub fn new(v: u32) -> Sequence24 {
        Sequence24(v & MASK)
    }

    pub fn value(&self) -> u32 {
        self.0 & MASK
    }

    // clone mutations.

    pub fn next(&self) -> Sequence24 {
        Sequence24::new(self.0 + 1)
    }

    pub fn prev(&self) -> Sequence24 {
        Sequence24(if self.0 == 0 { MASK } else { self.0 - 1 })
    }

    pub fn distance_to(&self, newer: Sequence24) -> u32 {
        let current = self.value();
        let target = newer.value();
        if target >= current {
            target - current
        } else {
            (MODULO - current) + target
        }
    }
}

impl Ord for Sequence24 {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let a = self.value() as i32;
        let b = other.value() as i32;
        // This is the delta: a - b
        let d = a.wrapping_sub(b);

        if d == 0 {
            std::cmp::Ordering::Equal
        } else if (d > 0 && d < (HALF as i32)) || (d < 0 && d < -(HALF as i32)) {
            // `a` is newer (greater)
            std::cmp::Ordering::Greater
        } else {
            // `a` is older (less)
            std::cmp::Ordering::Less
        }
    }
}

impl PartialOrd for Sequence24 {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Add for Sequence24 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        let mut value = self.value() + rhs.value();
        value %= MODULO;

        Sequence24::new(value)
    }
}

impl Add<i32> for Sequence24 {
    type Output = Self;

    fn add(self, rhs: i32) -> Self::Output {
        let mut value = self.value() as i32 + rhs;
        value %= MODULO as i32;

        if value < 0 {
            value += MODULO as i32;
        }

        Sequence24::new(value as u32)
    }
}

impl From<&Sequence24> for U24LE {
    fn from(value: &Sequence24) -> Self {
        U24LE(value.value())
    }
}

impl From<Sequence24> for U24LE {
    fn from(seq: Sequence24) -> Self {
        U24LE(seq.value())
    }
}

impl From<U24LE> for Sequence24 {
    fn from(raw: U24LE) -> Self {
        Sequence24::new(raw.0)
    }
}

impl RaknetEncodable for Sequence24 {
    fn encode_raknet(&self, dst: &mut impl bytes::BufMut) -> Result<(), EncodeError> {
        U24LE::from(self).encode_raknet(dst)
    }

    fn decode_raknet(
        src: &mut impl bytes::Buf,
    ) -> Result<Self, crate::protocol::packet::DecodeError> {
        Ok(Sequence24::new(U24LE::decode_raknet(src)?.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use crate::protocol::packet::RaknetEncodable;

    #[test]
    fn wraps_on_next() {
        let max = Sequence24::new(MASK);
        assert_eq!(max.next().value(), 0);
    }

    #[test]
    fn ordering_handles_wrap() {
        let a = Sequence24::new(MASK); // "Old" packet
        let b = a.next(); // "New" packet (0)
        let c = b.next(); // "Newer" packet (1)

        // 1 is greater (newer) than 0.
        assert!(c > b);
        // 0 is greater (newer) than MASK.
        assert!(b > a);
        // 1 is greater (newer) than MASK.
        assert!(c > a);
    }

    #[test]
    fn negative_wrapping_add() {
        let a = Sequence24::new(0);
        let b = a + (-5i32);
        assert_eq!(b.value(), MASK - 4); // Should wrap to near max
    }

    // --- Additional comprehensive tests ---

    #[test]
    fn sequence_masking() {
        // Values above 24-bit should be masked
        let seq = Sequence24::new(0xFFFFFFFF);
        assert_eq!(seq.value(), MASK);

        let seq2 = Sequence24::new(MODULO); // 2^24 should become 0
        assert_eq!(seq2.value(), 0);

        let seq3 = Sequence24::new(MODULO + 42); // Should become 42
        assert_eq!(seq3.value(), 42);
    }

    #[test]
    fn prev_wraps_at_zero() {
        let zero = Sequence24::new(0);
        let prev = zero.prev();
        assert_eq!(prev.value(), MASK);
    }

    #[test]
    fn prev_decrements_normally() {
        let seq = Sequence24::new(100);
        assert_eq!(seq.prev().value(), 99);
    }

    #[test]
    fn distance_to_no_wrap() {
        let a = Sequence24::new(10);
        let b = Sequence24::new(20);
        assert_eq!(a.distance_to(b), 10);
    }

    #[test]
    fn distance_to_with_wrap() {
        let a = Sequence24::new(MASK - 5); // Near max
        let b = Sequence24::new(5);         // Wrapped around
        // Distance should be 11 (5 to reach 0, then 5 more, plus 1 for the wrap)
        assert_eq!(a.distance_to(b), 11);
    }

    #[test]
    fn distance_to_same() {
        let a = Sequence24::new(100);
        let b = Sequence24::new(100);
        assert_eq!(a.distance_to(b), 0);
    }

    #[test]
    fn add_sequences() {
        let a = Sequence24::new(10);
        let b = Sequence24::new(20);
        let sum = a + b;
        assert_eq!(sum.value(), 30);
    }

    #[test]
    fn add_sequences_with_wrap() {
        let a = Sequence24::new(MASK);
        let b = Sequence24::new(10);
        let sum = a + b;
        assert_eq!(sum.value(), 9); // MASK + 10 = MODULO + 9 = 9 (after mod)
    }

    #[test]
    fn add_i32_positive() {
        let a = Sequence24::new(100);
        let b = a + 50i32;
        assert_eq!(b.value(), 150);
    }

    #[test]
    fn add_i32_large_wrap() {
        let a = Sequence24::new(MASK - 10);
        let b = a + 20i32;
        assert_eq!(b.value(), 9); // Wraps around
    }

    #[test]
    fn ordering_equal() {
        let a = Sequence24::new(100);
        let b = Sequence24::new(100);
        assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
    }

    #[test]
    fn ordering_near_half() {
        // Test around the half point where comparison logic changes
        let a = Sequence24::new(0);
        let half_minus_1 = Sequence24::new(HALF - 1);
        let _half = Sequence24::new(HALF); // boundary value
        let half_plus_1 = Sequence24::new(HALF + 1);

        // half_minus_1 is newer than 0
        assert!(half_minus_1 > a);
        // half_plus_1 should be considered older (wrapped around)
        assert!(half_plus_1 < a);
    }

    #[test]
    fn roundtrip_encoding() {
        let test_values = [0u32, 1, 127, 128, 255, 256, 65535, MASK - 1, MASK];

        for &v in &test_values {
            let seq = Sequence24::new(v);
            let mut buf = BytesMut::new();
            seq.encode_raknet(&mut buf).unwrap();

            assert_eq!(buf.len(), 3, "Sequence24 should encode to 3 bytes");

            let mut slice = buf.freeze();
            let decoded = Sequence24::decode_raknet(&mut slice).unwrap();
            assert_eq!(decoded.value(), v, "Roundtrip failed for value {}", v);
        }
    }

    #[test]
    fn u24le_conversion() {
        let seq = Sequence24::new(0x123456);
        let u24: U24LE = seq.into();
        assert_eq!(u24.0, 0x123456);

        let seq_back: Sequence24 = u24.into();
        assert_eq!(seq_back.value(), 0x123456);
    }
}
