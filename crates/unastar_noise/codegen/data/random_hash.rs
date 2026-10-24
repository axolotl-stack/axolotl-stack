//! `fromHashOf` seeds: Java derives named random streams from the MD5 of a name.
//!
//! `PositionalRandomFactory::from_hash(hash)` XORs the two big-endian halves of
//! `md5(name)` into the factory's seed. Computing the digests here keeps MD5 out of the
//! runtime and turns every named stream into a constant.

use md5::{Digest, Md5};

/// `(high 64 bits, low 64 bits)` of `md5(name)`, big-endian, as Java reads them.
pub fn hash_of(name: &str) -> (u64, u64) {
    let digest = Md5::digest(name.as_bytes());
    let mut lo = [0u8; 8];
    let mut hi = [0u8; 8];
    lo.copy_from_slice(&digest[..8]);
    hi.copy_from_slice(&digest[8..]);
    (u64::from_be_bytes(lo), u64::from_be_bytes(hi))
}
