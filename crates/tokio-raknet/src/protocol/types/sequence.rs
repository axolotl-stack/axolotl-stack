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
}
