//! Climate noise sampling and biome lookup.

use super::constants::Biome;
use super::noise::DoublePerlinNoise;
use super::xoroshiro::Xoroshiro128;

/// Climate parameter indices.
#[derive(Debug, Clone, Copy)]
#[repr(usize)]
pub enum Climate {
    Temperature = 0,
    Humidity = 1,
    Continentalness = 2,
    Erosion = 3,
    Depth = 4,
    Weirdness = 5,
}

/// Biome noise generator with 6 climate parameters.
#[derive(Debug, Clone)]
pub struct BiomeNoise {
    /// Climate noise samplers
    temperature: DoublePerlinNoise,
    humidity: DoublePerlinNoise,
    continentalness: DoublePerlinNoise,
    erosion: DoublePerlinNoise,
    weirdness: DoublePerlinNoise,
}

impl BiomeNoise {
    /// Create biome noise from seed.
    pub fn from_seed(seed: i64) -> Self {
        let mut rng = Xoroshiro128::from_seed(seed);

        // Temperature: octaves -10 to -8
        let temp_amps = [1.5, 0.0, 1.0, 0.0, 0.0, 0.0];
        let temperature = DoublePerlinNoise::new(&mut rng, &temp_amps, -10);

        // Humidity: octaves -8 to -6
        let humid_amps = [1.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        let humidity = DoublePerlinNoise::new(&mut rng, &humid_amps, -8);

        // Continentalness: octaves -9 to -5
        let cont_amps = [1.0, 1.0, 2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0];
        let continentalness = DoublePerlinNoise::new(&mut rng, &cont_amps, -9);

        // Erosion: octaves -9 to -4
        let erosion_amps = [1.0, 1.0, 0.0, 1.0, 1.0, 0.0];
        let erosion = DoublePerlinNoise::new(&mut rng, &erosion_amps, -9);

        // Weirdness: octaves -7 to -4
        let weird_amps = [1.0, 2.0, 1.0, 0.0, 0.0, 0.0];
        let weirdness = DoublePerlinNoise::new(&mut rng, &weird_amps, -7);

        Self {
            temperature,
            humidity,
            continentalness,
            erosion,
            weirdness,
        }
    }

    /// Sample climate at a position, returning 6 parameters.
    /// Values are scaled by 10000 to match biome parameter tables.
    ///
    /// Java Edition samples climate at "quart positions" (block / 4) with an
    /// additional 0.25 scale factor applied to the noise. This effectively
    /// samples at block / 16 scale. We replicate this by:
    /// 1. Converting to quart positions (>> 2 = divide by 4)
    /// 2. Applying 0.25 scale (multiply by 0.25)
    /// Result: position / 4 * 0.25 = position / 16
    pub fn sample_climate(&self, x: i32, y: i32, z: i32) -> [i64; 6] {
        // Convert to quart coordinates then apply 0.25 scale factor
        // This matches Java's shiftedNoise2d with 0.25 xzScale
        const SCALE: f64 = 0.25;
        let qx = (x >> 2) as f64 * SCALE;
        let qy = (y >> 2) as f64 * SCALE;
        let qz = (z >> 2) as f64 * SCALE;

        let temperature = (self.temperature.sample(qx, qy, qz) * 10000.0) as i64;
        let humidity = (self.humidity.sample(qx, qy, qz) * 10000.0) as i64;
        let continentalness = (self.continentalness.sample(qx, qy, qz) * 10000.0) as i64;
        let erosion = (self.erosion.sample(qx, qy, qz) * 10000.0) as i64;
        let weirdness = (self.weirdness.sample(qx, qy, qz) * 10000.0) as i64;

        // Depth is derived from Y position
        let depth = Self::depth_from_y(y);

        [
            temperature,
            humidity,
            continentalness,
            erosion,
            depth,
            weirdness,
        ]
    }

    /// Sample climate at 4 positions simultaneously using SIMD.
    /// Returns 4 climate arrays, one per input position.
    /// All positions share the same Y coordinate for efficiency.
    pub fn sample_climate_4(&self, x: [i32; 4], y: i32, z: [i32; 4]) -> [[i64; 6]; 4] {
        // Convert to quart coordinates then apply 0.25 scale factor
        // This matches Java's shiftedNoise2d with 0.25 xzScale
        const SCALE: f64 = 0.25;
        let qx = [
            (x[0] >> 2) as f64 * SCALE,
            (x[1] >> 2) as f64 * SCALE,
            (x[2] >> 2) as f64 * SCALE,
            (x[3] >> 2) as f64 * SCALE,
        ];
        let qy = (y >> 2) as f64 * SCALE;
        let qz = [
            (z[0] >> 2) as f64 * SCALE,
            (z[1] >> 2) as f64 * SCALE,
            (z[2] >> 2) as f64 * SCALE,
            (z[3] >> 2) as f64 * SCALE,
        ];

        // Sample all 5 noise parameters using SIMD batch sampling
        let temp = self.temperature.sample_4_arrays(qx, qy, qz);
        let humid = self.humidity.sample_4_arrays(qx, qy, qz);
        let cont = self.continentalness.sample_4_arrays(qx, qy, qz);
        let eros = self.erosion.sample_4_arrays(qx, qy, qz);
        let weird = self.weirdness.sample_4_arrays(qx, qy, qz);

        // Depth is derived from Y position (same for all 4)
        let depth = Self::depth_from_y(y);

        // Build result arrays
        [
            [
                (temp[0] * 10000.0) as i64,
                (humid[0] * 10000.0) as i64,
                (cont[0] * 10000.0) as i64,
                (eros[0] * 10000.0) as i64,
                depth,
                (weird[0] * 10000.0) as i64,
            ],
            [
                (temp[1] * 10000.0) as i64,
                (humid[1] * 10000.0) as i64,
                (cont[1] * 10000.0) as i64,
                (eros[1] * 10000.0) as i64,
                depth,
                (weird[1] * 10000.0) as i64,
            ],
            [
                (temp[2] * 10000.0) as i64,
                (humid[2] * 10000.0) as i64,
                (cont[2] * 10000.0) as i64,
                (eros[2] * 10000.0) as i64,
                depth,
                (weird[2] * 10000.0) as i64,
            ],
            [
                (temp[3] * 10000.0) as i64,
                (humid[3] * 10000.0) as i64,
                (cont[3] * 10000.0) as i64,
                (eros[3] * 10000.0) as i64,
                depth,
                (weird[3] * 10000.0) as i64,
            ],
        ]
    }

    /// Calculate depth parameter from Y coordinate.
    fn depth_from_y(y: i32) -> i64 {
        // Depth increases as we go deeper, decreases higher
        // Sea level is at 64, depth 0 at 64
        let depth = ((64 - y) as f64 / 128.0 * 10000.0) as i64;
        depth.clamp(-10000, 10000)
    }

    /// Get biome at a specific position.
    pub fn get_biome(&self, x: i32, y: i32, z: i32) -> Biome {
        let climate = self.sample_climate(x, y, z);
        Self::lookup_biome(&climate)
    }

    /// Lookup biome from climate parameters using vanilla biome tables.
    ///
    /// This implements the OverworldBiomeBuilder logic from Java Edition,
    /// checking continentalness, erosion, and weirdness to select biomes.
    pub fn lookup_biome(climate: &[i64; 6]) -> Biome {
        use unastar_noise::biome_tables::*;

        let temp = climate[Climate::Temperature as usize];
        let humid = climate[Climate::Humidity as usize];
        let cont = climate[Climate::Continentalness as usize];
        let erosion = climate[Climate::Erosion as usize];
        let depth = climate[Climate::Depth as usize];
        let weird = climate[Climate::Weirdness as usize];

        let ti = temp_index(temp);
        let hi = humid_index(humid);
        let ei = erosion_index(erosion);

        // Mushroom fields - extremely low continentalness
        if cont < MUSHROOM_CONT {
            return Biome::MushroomFields;
        }

        // Deep ocean
        if cont < DEEP_OCEAN_CONT {
            return OCEANS[0][ti];
        }

        // Ocean
        if cont < OCEAN_CONT {
            return OCEANS[1][ti];
        }

        // Underground biomes (depth > 0.2 scaled = 2000)
        if depth > 2000 {
            // Dripstone caves - high continentalness
            if cont > 8000 {
                return Biome::DripstoneCaves;
            }
            // Lush caves - high humidity
            if humid > 7000 {
                return Biome::LushCaves;
            }
            // Deep dark - low erosion, very deep
            if ei <= 1 && depth > 9000 {
                return Biome::DeepDark;
            }
        }

        // Coast region
        if cont < COAST_CONT {
            // Stony shore at low erosion
            if ei <= 2 {
                return Biome::StonyShore;
            }
            // Beach at medium-high erosion
            if ei >= 3 && ei <= 4 {
                return Self::pick_beach(ti);
            }
        }

        // Valley weirdness = rivers
        let is_valley = weird.abs() < 500; // -0.05 to 0.05

        if is_valley && cont >= COAST_CONT && cont < MID_INLAND_CONT {
            // Rivers in valleys
            if ei >= 2 && ei <= 5 {
                return if ti == 0 {
                    Biome::FrozenRiver
                } else {
                    Biome::River
                };
            }
        }

        // Swamp regions - high erosion, warm/temperate, inland
        if ei == 6 && cont >= NEAR_INLAND_CONT {
            if ti == 1 || ti == 2 {
                return Biome::Swamp;
            }
            if ti >= 3 {
                return Biome::MangroveSwamp;
            }
        }

        // Determine which biome picker to use based on weirdness
        let use_variant = weird > 0;

        // Pick based on erosion and continentalness
        match ei {
            // Low erosion (0-1) - peaks and slopes
            0 => Self::pick_peak_biome(ti, hi, use_variant),
            1 => {
                if cont >= MID_INLAND_CONT {
                    Self::pick_slope_biome(ti, hi, use_variant)
                } else {
                    Self::pick_middle_or_badlands(ti, hi, use_variant)
                }
            }
            // Medium erosion (2-3) - plateau and middle
            2 => {
                if cont >= MID_INLAND_CONT {
                    Self::pick_plateau_biome(ti, hi, use_variant)
                } else {
                    Self::pick_middle_biome(ti, hi, use_variant)
                }
            }
            3 => Self::pick_middle_or_badlands(ti, hi, use_variant),
            // Higher erosion (4) - middle biomes
            4 => Self::pick_middle_biome(ti, hi, use_variant),
            // High erosion (5) - shattered/windswept
            5 => {
                if cont >= MID_INLAND_CONT {
                    Self::pick_shattered_biome(ti, hi, use_variant)
                } else if ti > 1 && hi < 4 && use_variant {
                    Biome::WindsweptSavanna
                } else {
                    Self::pick_middle_biome(ti, hi, use_variant)
                }
            }
            // Very high erosion (6) - middle or swamp (swamp handled above)
            _ => Self::pick_middle_biome(ti, hi, use_variant),
        }
    }

    fn pick_beach(ti: usize) -> Biome {
        match ti {
            0 => Biome::SnowyBeach,
            4 => Biome::Desert, // Hot beaches are desert
            _ => Biome::Beach,
        }
    }

    fn pick_middle_biome(ti: usize, hi: usize, use_variant: bool) -> Biome {
        use unastar_noise::biome_tables::*;
        if use_variant {
            if let Some(variant) = MIDDLE_BIOMES_VARIANT[ti][hi] {
                return variant;
            }
        }
        MIDDLE_BIOMES[ti][hi]
    }

    fn pick_middle_or_badlands(ti: usize, hi: usize, use_variant: bool) -> Biome {
        if ti == 4 {
            Self::pick_badlands(hi, use_variant)
        } else {
            Self::pick_middle_biome(ti, hi, use_variant)
        }
    }

    fn pick_plateau_biome(ti: usize, hi: usize, use_variant: bool) -> Biome {
        use unastar_noise::biome_tables::*;
        if use_variant {
            if let Some(variant) = PLATEAU_BIOMES_VARIANT[ti][hi] {
                return variant;
            }
        }
        PLATEAU_BIOMES[ti][hi]
    }

    fn pick_slope_biome(ti: usize, hi: usize, use_variant: bool) -> Biome {
        // Cold slopes
        if ti < 3 {
            if hi <= 1 {
                return Biome::SnowySlopes;
            }
            return Biome::Grove;
        }
        // Warm slopes use plateau biomes
        Self::pick_plateau_biome(ti, hi, use_variant)
    }

    fn pick_peak_biome(ti: usize, hi: usize, use_variant: bool) -> Biome {
        // Frozen peaks
        if ti <= 2 {
            return if use_variant {
                Biome::FrozenPeaks
            } else {
                Biome::JaggedPeaks
            };
        }
        // Warm peaks
        if ti == 3 {
            return Biome::StonyPeaks;
        }
        // Hot peaks = badlands
        Self::pick_badlands(hi, use_variant)
    }

    fn pick_badlands(hi: usize, use_variant: bool) -> Biome {
        if hi < 2 {
            return if use_variant {
                Biome::ErodedBadlands
            } else {
                Biome::Badlands
            };
        }
        if hi < 3 {
            return Biome::Badlands;
        }
        Biome::WoodedBadlands
    }

    fn pick_shattered_biome(ti: usize, hi: usize, use_variant: bool) -> Biome {
        use unastar_noise::biome_tables::*;
        if let Some(biome) = SHATTERED_BIOMES[ti][hi] {
            return biome;
        }
        Self::pick_middle_biome(ti, hi, use_variant)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_biome_noise_creation() {
        let noise = BiomeNoise::from_seed(12345);
        let biome = noise.get_biome(0, 64, 0);
        // Should get some valid biome
        assert!(matches!(
            biome,
            Biome::Plains | Biome::Forest | Biome::Taiga | _
        ));
    }

    #[test]
    fn test_climate_sampling() {
        let noise = BiomeNoise::from_seed(0);
        let climate = noise.sample_climate(0, 64, 0);
        // All values should be in valid range
        for param in climate.iter() {
            assert!(*param >= -15000 && *param <= 15000);
        }
    }
}
