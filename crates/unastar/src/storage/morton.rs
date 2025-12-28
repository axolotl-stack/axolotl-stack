//! Morton (Z-Order) encoding for spatial locality.
//!
//! Morton codes interleave the bits of X and Z coordinates to create
//! a single value where spatially close chunks have numerically close keys.
//! This improves disk I/O by keeping nearby chunks physically close in storage.

/// Encode chunk coordinates and dimension into a Morton code.
///
/// The resulting u64 has:
/// - Bits 0-47: Interleaved X/Z coordinates
/// - Bits 48-63: Dimension ID
#[inline]
pub fn encode(x: i32, z: i32, dim: i32) -> u64 {
    // Convert signed to unsigned (shift by 2^31 to handle negatives)
    let ux = (x as u32) ^ 0x8000_0000;
    let uz = (z as u32) ^ 0x8000_0000;

    // Interleave bits
    let morton = interleave_bits(ux, uz);

    // Combine with dimension in upper bits
    morton | ((dim as u64 & 0xFFFF) << 48)
}

/// Decode a Morton code back to chunk coordinates and dimension.
#[inline]
pub fn decode(morton: u64) -> (i32, i32, i32) {
    let dim = ((morton >> 48) & 0xFFFF) as i32;
    let interleaved = morton & 0x0000_FFFF_FFFF_FFFF;

    let (ux, uz) = deinterleave_bits(interleaved);

    // Convert back to signed
    let x = (ux ^ 0x8000_0000) as i32;
    let z = (uz ^ 0x8000_0000) as i32;

    (x, z, dim)
}

/// Interleave bits of two 32-bit values into a 64-bit Morton code.
/// Result: Z31 X31 Z30 X30 ... Z0 X0
#[inline]
fn interleave_bits(x: u32, z: u32) -> u64 {
    let x = spread_bits(x as u64);
    let z = spread_bits(z as u64);
    (z << 1) | x
}

/// Deinterleave a 64-bit Morton code back to two 32-bit values.
#[inline]
fn deinterleave_bits(morton: u64) -> (u32, u32) {
    let x = compact_bits(morton);
    let z = compact_bits(morton >> 1);
    (x as u32, z as u32)
}

/// Spread bits of a 32-bit value across 64 bits.
/// Input:  .... .... .... .... FEDC BA98 7654 3210
/// Output: .F.E .D.C .B.A .9.8 .7.6 .5.4 .3.2 .1.0
#[inline]
fn spread_bits(mut x: u64) -> u64 {
    // Magic numbers for bit spreading (from Stanford bit-twiddling hacks)
    x = (x | (x << 16)) & 0x0000_FFFF_0000_FFFF;
    x = (x | (x << 8)) & 0x00FF_00FF_00FF_00FF;
    x = (x | (x << 4)) & 0x0F0F_0F0F_0F0F_0F0F;
    x = (x | (x << 2)) & 0x3333_3333_3333_3333;
    x = (x | (x << 1)) & 0x5555_5555_5555_5555;
    x
}

/// Compact bits from a 64-bit value back to 32 bits.
/// Inverse of spread_bits.
#[inline]
fn compact_bits(mut x: u64) -> u64 {
    x &= 0x5555_5555_5555_5555;
    x = (x | (x >> 1)) & 0x3333_3333_3333_3333;
    x = (x | (x >> 2)) & 0x0F0F_0F0F_0F0F_0F0F;
    x = (x | (x >> 4)) & 0x00FF_00FF_00FF_00FF;
    x = (x | (x >> 8)) & 0x0000_FFFF_0000_FFFF;
    x = (x | (x >> 16)) & 0x0000_0000_FFFF_FFFF;
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_positive() {
        let (x, z, dim) = (100, 200, 0);
        let morton = encode(x, z, dim);
        let (dx, dz, ddim) = decode(morton);
        assert_eq!((x, z, dim), (dx, dz, ddim));
    }

    #[test]
    fn test_encode_decode_negative() {
        let (x, z, dim) = (-50, -100, 1);
        let morton = encode(x, z, dim);
        let (dx, dz, ddim) = decode(morton);
        assert_eq!((x, z, dim), (dx, dz, ddim));
    }

    #[test]
    fn test_encode_decode_mixed() {
        let (x, z, dim) = (-1000, 500, 2);
        let morton = encode(x, z, dim);
        let (dx, dz, ddim) = decode(morton);
        assert_eq!((x, z, dim), (dx, dz, ddim));
    }

    #[test]
    fn test_spatial_locality() {
        // Nearby chunks should have close Morton codes
        let m1 = encode(0, 0, 0);
        let m2 = encode(1, 0, 0);
        let m3 = encode(0, 1, 0);
        let m_far = encode(1000, 1000, 0);

        // Close chunks should have smaller difference than far chunks
        assert!((m2 as i64 - m1 as i64).abs() < (m_far as i64 - m1 as i64).abs());
        assert!((m3 as i64 - m1 as i64).abs() < (m_far as i64 - m1 as i64).abs());
    }
}
