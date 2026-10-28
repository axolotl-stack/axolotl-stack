//! Seeded Java End-islands density, sampled in block coordinates.
//!
//! This is the horizontal density primitive only, not the End terrain router.

use crate::features::random::LegacyRandom;
use crate::noise::SimplexNoise;

/// The native `DensityFunctions.endIslands(seed)` horizontal density primitive.
#[derive(Debug, Clone)]
pub struct EndIslands {
    island_noise: SimplexNoise,
}

impl EndIslands {
    /// Construct the legacy random stream and island simplex permutation once per seed.
    pub fn new(seed: i64) -> Self {
        let mut random = LegacyRandom::new(seed);
        random.consume_count(17_292);
        Self {
            island_noise: SimplexNoise::new(&mut random),
        }
    }

    /// Sample at Java block coordinates. The native density is independent of Y.
    ///
    /// Both coordinate divisions truncate toward zero, including at negative block
    /// boundaries. The radial square deliberately wraps as a Java `int` before its
    /// conversion to `float`; using a wider square changes distant terrain.
    pub fn sample(&self, block_x: i32, block_z: i32) -> f64 {
        let x = block_x / 8;
        let z = block_z / 8;
        let radial_square = x.wrapping_mul(x).wrapping_add(z.wrapping_mul(z));
        if radial_square < 0 {
            // The pinned native sqrt produces a negative quiet NaN. Its clamp and
            // subsequent Math.max operations preserve it, as does widening to double.
            // Rust's NaN payload/sign are unspecified, so retain these native bits.
            return f64::from_bits(0xfff8_0000_0000_0000);
        }
        let mut height = (100.0 - sqrt_float(radial_square as f32) * 8.0).clamp(-100.0, 80.0);
        let cell_x = x / 2;
        let cell_z = z / 2;
        let remainder_x = x % 2;
        let remainder_z = z % 2;

        for offset_x in -12..=12 {
            let island_x = i64::from(cell_x + offset_x);
            for offset_z in -12..=12 {
                let island_z = i64::from(cell_z + offset_z);
                if island_x * island_x + island_z * island_z <= 4096
                    || self.island_noise.get_value_2d(island_x as f64, island_z as f64)
                        >= f64::from(-0.9_f32)
                {
                    continue;
                }
                // Each operation is float arithmetic, including the modulo. In
                // particular, converting after the integer products is not equivalent.
                let scale = ((island_x as f32).abs() * 3439.0
                    + (island_z as f32).abs() * 147.0)
                    % 13.0
                    + 9.0;
                let dx = (remainder_x - offset_x * 2) as f32;
                let dz = (remainder_z - offset_z * 2) as f32;
                let candidate = (100.0 - sqrt_float(dx * dx + dz * dz) * scale).clamp(-100.0, 80.0);
                height = height.max(candidate);
            }
        }
        f64::from((height - 8.0) / 128.0)
    }

    /// Allocation-free exact scalar lanes for density emitters using SIMD batches.
    #[inline]
    pub fn sample_array<const N: usize>(&self, x: [i32; N], z: [i32; N]) -> [f64; N] {
        std::array::from_fn(|lane| self.sample(x[lane], z[lane]))
    }
}

/// Native float sqrt widens the argument to double, then narrows the result.
#[inline]
fn sqrt_float(value: f32) -> f32 {
    f64::from(value).sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::EndIslands;

    // Fixture values below are actual pinned native API measurements, not Rust
    // output hashes. Y alternated between i32::MIN and i32::MAX in the capture.
    // Source: Java 1.21.11 DensityFunctions.endIslands(seed).compute(
    //     new DensityFunction.SinglePointContext(x, y, z)).
    // server.jar SHA1: 64bb6d763bed0a9f1d632ec347938594144943ed
    // official server-mappings.txt SHA1: 5621e9253f05fd57872bbe7f8ddf5f9a7d525955
    // Extracted versions/1.21.11/server-1.21.11.jar SHA256, verified before loading:
    // ec47239a8de246335e1d54f6ac319bd35641778eb4b6a6da06372840d02fcebc
    // Six boundary seeds and disjoint holdouts -734287144591, 5930842301397;
    // signed /8 and /2 boundaries, island/void edges, 30M blocks, wrapping int
    // radii, float precision at distant islands, and both i32 endpoints.
    // Capture: external EndIslandsCapture.java, an access-only native API adapter;
    // java -XX:ActiveProcessorCount=1 -Xmx1G EndIslandsCapture.java

    #[test]
    fn pinned_native_scalar_bits() {
        for &(seed, expected) in NATIVE {
            let islands = EndIslands::new(seed);
            assert_eq!(expected.len(), COORDS.len());
            for (&(x, z), &bits) in COORDS.iter().zip(expected) {
                assert_eq!(
                    islands.sample(x, z).to_bits(),
                    bits,
                    "native End-islands seed={seed} block=({x},{z})"
                );
            }
        }
    }

    #[test]
    fn pinned_native_array_bits() {
        for &(seed, expected) in NATIVE {
            let islands = EndIslands::new(seed);
            for start in (0..COORDS.len()).step_by(4) {
                let indices: [usize; 4] =
                    std::array::from_fn(|lane| (start + lane) % COORDS.len());
                let x = indices.map(|index| COORDS[index].0);
                let z = indices.map(|index| COORDS[index].1);
                let actual = islands.sample_array(x, z).map(f64::to_bits);
                assert_eq!(
                    actual,
                    indices.map(|index| expected[index]),
                    "native End-islands seed={seed} lanes={indices:?}"
                );
            }
        }
    }

    const COORDS: &[(i32, i32)] = &[
        (0, 0), (7, 7), (8, 0),
        (-7, -7), (-8, 0), (15, 15),
        (16, 0), (-15, -15), (-16, 0),
        (63, 65), (-63, -65), (511, 0),
        (512, 0), (-511, 0), (-512, 0),
        (1023, 0), (1024, 0), (1031, 0),
        (1032, 0), (-1023, 0), (-1024, 0),
        (-1031, 0), (-1032, 0), (1151, 1039),
        (1152, 1040), (1159, 1047), (1160, 1048),
        (-1151, -1039), (-1152, -1040), (-1159, -1047),
        (-1160, -1048), (2000, 3000), (-2000, 3000),
        (2000, -3000), (-2000, -3000), (8192, 16384),
        (-8193, -16385), (370720, 0), (370728, 0),
        (262144, 262144), (370727, -8), (-370728, 8),
        (1000000, -1000000), (-1000001, 1000001), (29999999, -30000000),
        (-30000000, 29999999), (-2147483584, 0), (-2147483520, 0),
        (-2147483392, 0), (2147483584, 0), (2147483520, 0),
        (2147483392, 0), (0, -2147483584), (0, 2147483520),
        (1234567890, -123000000), (-1234567890, 123000000), (1073741824, 1073741824),
        (-1073741824, 1073741824), (-2147483648, 0), (2147483647, 0),
        (0, -2147483648), (0, 2147483647), (-2147483648, -2147483648),
        (2147483647, 2147483647), (-2147483648, 2147483647), (2147483647, -2147483648),
    ];

    const SEED_ZERO: &[u64] = &[
        0x3fe2000000000000, 0x3fe2000000000000, 0x3fe2000000000000,
        0x3fe2000000000000, 0x3fe2000000000000, 0x3fe2000000000000,
        0x3fe2000000000000, 0x3fe2000000000000, 0x3fe2000000000000,
        0x3fabd5d800000000, 0x3fabd5d800000000, 0xbfeb000000000000,
        0xbfeb000000000000, 0xbfeb000000000000, 0xbfeb000000000000,
        0xbfeb000000000000, 0xbfea902680000000, 0xbfea902680000000,
        0xbfe88435c0000000, 0x3fb20c5700000000, 0x3fbf43d300000000,
        0x3fbf43d300000000, 0x3fc5b52f00000000, 0xbfe6199280000000,
        0xbfe36d2900000000, 0xbfe36d2900000000, 0xbfdb1ea900000000,
        0xbfc2c9c000000000, 0xbfd6816680000000, 0xbfd6816680000000,
        0xbfd4686780000000, 0xbfe627ee40000000, 0x3fe2000000000000,
        0x3fe2000000000000, 0xbfeb000000000000, 0xbf8ea34000000000,
        0x3fc01af700000000, 0xbfc5231800000000, 0xfff8000000000000,
        0xfff8000000000000, 0xbfb0000000000000, 0xfff8000000000000,
        0x3fdd000000000000, 0x3fc66e0d80000000, 0xbfc87b5480000000,
        0xbfdb413380000000, 0x3fcc000000000000, 0x3f94755000000000,
        0x3f94755000000000, 0x3fcc000000000000, 0x3fb8000000000000,
        0x3fd7a3aa80000000, 0x3fcc000000000000, 0xbfd2000000000000,
        0x3fde000000000000, 0xbfddb77180000000, 0x3fe2000000000000,
        0x3fe2000000000000, 0x3fe2000000000000, 0xfff8000000000000,
        0x3fe2000000000000, 0xfff8000000000000, 0x3fe2000000000000,
        0xfff8000000000000, 0xfff8000000000000, 0xfff8000000000000,
    ];

    const SEED_NEG_ONE: &[u64] = &[
        0x3fe2000000000000, 0x3fe2000000000000, 0x3fe2000000000000,
        0x3fe2000000000000, 0x3fe2000000000000, 0x3fe2000000000000,
        0x3fe2000000000000, 0x3fe2000000000000, 0x3fe2000000000000,
        0x3fabd5d800000000, 0x3fabd5d800000000, 0xbfeb000000000000,
        0xbfeb000000000000, 0xbfeb000000000000, 0xbfeb000000000000,
        0xbfeb000000000000, 0xbfeb000000000000, 0xbfeb000000000000,
        0xbfeb000000000000, 0xbfeb000000000000, 0xbfeb000000000000,
        0xbfeb000000000000, 0xbfeb000000000000, 0xbfeb000000000000,
        0xbfeb000000000000, 0xbfeb000000000000, 0xbfeb000000000000,
        0xbfe54cd440000000, 0xbfe7190440000000, 0xbfe7190440000000,
        0xbfe402fa00000000, 0x3fc5fe2c00000000, 0xbfeb000000000000,
        0xbfeb000000000000, 0xbfe9671140000000, 0x3fac96b800000000,
        0xbfe3944f80000000, 0xbfde06bc80000000, 0xfff8000000000000,
        0xfff8000000000000, 0xbfd8000000000000, 0xfff8000000000000,
        0x3fda000000000000, 0x3fe0800000000000, 0x3fb3136600000000,
        0x3fb3136600000000, 0x3fcc000000000000, 0xbfd2000000000000,
        0x3fd7a3aa80000000, 0x3fcc000000000000, 0x3fb8000000000000,
        0x3fb8000000000000, 0x3fcc000000000000, 0xbfd2000000000000,
        0xbfeb000000000000, 0x3fc4722180000000, 0x3fe2000000000000,
        0x3fe2000000000000, 0x3fe2000000000000, 0xfff8000000000000,
        0x3fe2000000000000, 0xfff8000000000000, 0x3fe2000000000000,
        0xfff8000000000000, 0xfff8000000000000, 0xfff8000000000000,
    ];

    const NATIVE: &[(i64, &[u64])] = &[
        (0, SEED_ZERO),
        (1, &[
            0x3fe2000000000000, 0x3fe2000000000000, 0x3fe2000000000000,
            0x3fe2000000000000, 0x3fe2000000000000, 0x3fe2000000000000,
            0x3fe2000000000000, 0x3fe2000000000000, 0x3fe2000000000000,
            0x3fabd5d800000000, 0x3fabd5d800000000, 0xbfeb000000000000,
            0xbfeb000000000000, 0xbfeb000000000000, 0xbfeb000000000000,
            0xbfeb000000000000, 0xbfeb000000000000, 0xbfeb000000000000,
            0xbfeaf54680000000, 0xbfe0229b40000000, 0xbfe0000000000000,
            0xbfe0000000000000, 0xbfe0229b40000000, 0xbfeb000000000000,
            0xbfe9776b80000000, 0xbfe9776b80000000, 0xbfe68f1c80000000,
            0xbfeb000000000000, 0xbfeb000000000000, 0xbfeb000000000000,
            0xbfeb000000000000, 0x3fe2000000000000, 0x3fc590cd80000000,
            0xbfeb000000000000, 0xbfc28c7300000000, 0xbfb0000000000000,
            0xbfc0000000000000, 0x3f90000000000000, 0xfff8000000000000,
            0xfff8000000000000, 0xbf9d5ed800000000, 0xfff8000000000000,
            0xbfe63974c0000000, 0x3fe0800000000000, 0xbfe5cfb880000000,
            0x3fd542e900000000, 0x3fd4000000000000, 0xbfc0000000000000,
            0x3fccc12380000000, 0x3fdc000000000000, 0xbfc0000000000000,
            0xbfe646e4c0000000, 0x3fda000000000000, 0x3fcc000000000000,
            0x3fe2000000000000, 0x3fde000000000000, 0x3fe2000000000000,
            0x3fe2000000000000, 0x3fe2000000000000, 0xfff8000000000000,
            0x3fe2000000000000, 0xfff8000000000000, 0x3fe2000000000000,
            0xfff8000000000000, 0xfff8000000000000, 0xfff8000000000000,
        ]),
        (-1, SEED_NEG_ONE),
        (12345, &[
            0x3fe2000000000000, 0x3fe2000000000000, 0x3fe2000000000000,
            0x3fe2000000000000, 0x3fe2000000000000, 0x3fe2000000000000,
            0x3fe2000000000000, 0x3fe2000000000000, 0x3fe2000000000000,
            0x3fabd5d800000000, 0x3fabd5d800000000, 0xbfeb000000000000,
            0xbfeb000000000000, 0xbfeb000000000000, 0xbfeb000000000000,
            0xbfeb000000000000, 0xbfeb000000000000, 0xbfeb000000000000,
            0xbfeb000000000000, 0xbfe3cb0900000000, 0xbfe14fb080000000,
            0xbfe14fb080000000, 0xbfdda9ea00000000, 0xbfc0bf5680000000,
            0xbfc5d78d00000000, 0xbfc5d78d00000000, 0xbfcc133200000000,
            0xbfc0bf5680000000, 0xbfc5d78d00000000, 0xbfc5d78d00000000,
            0xbfcc133200000000, 0xbfeabeb680000000, 0xbfea58aac0000000,
            0x3f9a857400000000, 0x3fd542e900000000, 0x3fc1c8bb00000000,
            0x3fc4000000000000, 0xbfeb000000000000, 0xfff8000000000000,
            0xfff8000000000000, 0xbfeb000000000000, 0xfff8000000000000,
            0x3fb8000000000000, 0xbfd2e66180000000, 0xbfc87b5480000000,
            0xbfe2db8ec0000000, 0x3fd0ee5da0000000, 0xbfc5d78d00000000,
            0xbfde286100000000, 0x3fcc000000000000, 0xbfd2000000000000,
            0xbfd13edc80000000, 0x3fcc000000000000, 0xbfd2000000000000,
            0xbfe58c37c0000000, 0xbfeb000000000000, 0x3fe2000000000000,
            0x3fe2000000000000, 0x3fe2000000000000, 0xfff8000000000000,
            0x3fe2000000000000, 0xfff8000000000000, 0x3fe2000000000000,
            0xfff8000000000000, 0xfff8000000000000, 0xfff8000000000000,
        ]),
        // These two complete native captures equal the corresponding 48-bit seeds.
        (i64::MIN, SEED_ZERO),
        (i64::MAX, SEED_NEG_ONE),
        (-734287144591, &[
            0x3fe2000000000000, 0x3fe2000000000000, 0x3fe2000000000000,
            0x3fe2000000000000, 0x3fe2000000000000, 0x3fe2000000000000,
            0x3fe2000000000000, 0x3fe2000000000000, 0x3fe2000000000000,
            0x3fabd5d800000000, 0x3fabd5d800000000, 0xbfeb000000000000,
            0xbfeb000000000000, 0xbfeb000000000000, 0xbfeb000000000000,
            0x3fd9626cc0000000, 0x3fda000000000000, 0x3fda000000000000,
            0x3fd9626cc0000000, 0xbfeb000000000000, 0xbfeb000000000000,
            0xbfeb000000000000, 0xbfeb000000000000, 0xbfeb000000000000,
            0xbfea7f5a80000000, 0xbfea7f5a80000000, 0xbfe7b22180000000,
            0xbfeb000000000000, 0xbfeb000000000000, 0xbfeb000000000000,
            0xbfeb000000000000, 0xbf98000000000000, 0xbfeb000000000000,
            0xbfdc374580000000, 0xbfa5664800000000, 0xbfc760ad00000000,
            0xbfc760ad00000000, 0x3fb700cb00000000, 0xfff8000000000000,
            0xfff8000000000000, 0x3fcb199e80000000, 0xfff8000000000000,
            0x3fb700cb00000000, 0x3fc66e0d80000000, 0x3fd6000000000000,
            0xbfccef1200000000, 0x3fcc000000000000, 0xbfb0000000000000,
            0x3fd0000000000000, 0x3fcc000000000000, 0xbfd2000000000000,
            0xbfb0000000000000, 0x3fd9e01960000000, 0xbfc760ad00000000,
            0xbfddb77180000000, 0xbfcacb3f80000000, 0x3fe2000000000000,
            0x3fe2000000000000, 0x3fe2000000000000, 0xfff8000000000000,
            0x3fe2000000000000, 0xfff8000000000000, 0x3fe2000000000000,
            0xfff8000000000000, 0xfff8000000000000, 0xfff8000000000000,
        ]),
        (5930842301397, &[
            0x3fe2000000000000, 0x3fe2000000000000, 0x3fe2000000000000,
            0x3fe2000000000000, 0x3fe2000000000000, 0x3fe2000000000000,
            0x3fe2000000000000, 0x3fe2000000000000, 0x3fe2000000000000,
            0x3fabd5d800000000, 0x3fabd5d800000000, 0xbfeb000000000000,
            0xbfeb000000000000, 0xbfeb000000000000, 0xbfeb000000000000,
            0xbfcd000000000000, 0xbfc2000000000000, 0xbfc2000000000000,
            0xbfac000000000000, 0xbfe0229b40000000, 0xbfe0000000000000,
            0xbfe0000000000000, 0xbfe0229b40000000, 0xbfe0debf00000000,
            0xbfe3b0d4c0000000, 0xbfe3b0d4c0000000, 0xbfe68f1c80000000,
            0x3f738b0000000000, 0x3fc01af700000000, 0x3fc01af700000000,
            0x3fcecfd000000000, 0xbfd82da800000000, 0x3fd542e900000000,
            0x3fde58f740000000, 0xbfc87b5480000000, 0x3fc1c8bb00000000,
            0xbfd3855f80000000, 0xbfc5231800000000, 0xfff8000000000000,
            0xfff8000000000000, 0xbfcc6a7b80000000, 0xfff8000000000000,
            0xbfe7f51a00000000, 0x3fcb199e80000000, 0x3fd061b020000000,
            0xbfc516a580000000, 0x3fcc000000000000, 0xbfb3a59100000000,
            0xbfda1c6c00000000, 0x3fcc000000000000, 0xbfc5d78d00000000,
            0xbfc5231800000000, 0x3fcc000000000000, 0x3fd18a1cc0000000,
            0xbfeb000000000000, 0x3fd75f61a0000000, 0x3fe2000000000000,
            0x3fe2000000000000, 0x3fe2000000000000, 0xfff8000000000000,
            0x3fe2000000000000, 0xfff8000000000000, 0x3fe2000000000000,
            0xfff8000000000000, 0xfff8000000000000, 0xfff8000000000000,
        ]),
    ];
}
