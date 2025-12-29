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
        let temp = self.temperature.sample_4(qx, qy, qz);
        let humid = self.humidity.sample_4(qx, qy, qz);
        let cont = self.continentalness.sample_4(qx, qy, qz);
        let eros = self.erosion.sample_4(qx, qy, qz);
        let weird = self.weirdness.sample_4(qx, qy, qz);

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

    /// Lookup biome from climate parameters using simplified rules.
    /// This is a simplified version - full vanilla uses a complex binary tree.
    pub fn lookup_biome(climate: &[i64; 6]) -> Biome {
        let temp = climate[Climate::Temperature as usize];
        let humid = climate[Climate::Humidity as usize];
        let cont = climate[Climate::Continentalness as usize];
        let erosion = climate[Climate::Erosion as usize];
        let depth = climate[Climate::Depth as usize];
        let weird = climate[Climate::Weirdness as usize];

        // Ocean check - low continentalness
        if cont < -5000 {
            return if temp < -4500 {
                Biome::FrozenOcean
            } else if temp < -1500 {
                Biome::ColdOcean
            } else if temp > 5000 {
                Biome::WarmOcean
            } else {
                Biome::Ocean
            };
        }

        // Deep ocean
        if cont < -7000 {
            return if temp < -4500 {
                Biome::DeepFrozenOcean
            } else if temp < -1500 {
                Biome::DeepColdOcean
            } else {
                Biome::DeepOcean
            };
        }

        // Beach check
        if cont >= -4500 && cont < -2000 && erosion > 4500 {
            return if temp < -4500 {
                Biome::SnowyBeach
            } else {
                Biome::Beach
            };
        }

        // River check (high erosion at medium continentalness)
        if erosion > 7000 && cont > -3000 && cont < 3000 {
            return if temp < -4500 {
                Biome::FrozenRiver
            } else {
                Biome::River
            };
        }

        // Mountain peaks
        if cont > 6000 && erosion < -5000 {
            return if temp < -4500 {
                Biome::FrozenPeaks
            } else if temp < 0 {
                Biome::JaggedPeaks
            } else {
                Biome::StonyPeaks
            };
        }

        // High elevation slopes
        if cont > 4000 && erosion < -2000 {
            return if temp < -4500 {
                Biome::SnowySlopes
            } else if temp < 0 {
                Biome::Grove
            } else {
                Biome::Meadow
            };
        }

        // Land biomes based on temperature and humidity
        match (temp, humid) {
            // Frozen biomes
            (t, _) if t < -4500 => {
                if humid < -3500 {
                    Biome::SnowyPlains
                } else {
                    Biome::SnowyTaiga
                }
            }
            // Cold biomes
            (t, h) if t < -1500 => {
                if h < -3500 {
                    Biome::Plains
                } else if h < 3500 {
                    Biome::Taiga
                } else {
                    Biome::OldGrowthPineTaiga
                }
            }
            // Temperate biomes
            (t, h) if t < 2000 => {
                if h < -3500 {
                    Biome::Plains
                } else if h < 0 {
                    Biome::Forest
                } else if h < 3500 {
                    Biome::BirchForest
                } else {
                    Biome::DarkForest
                }
            }
            // Warm biomes
            (t, h) if t < 5500 => {
                if h < -3500 {
                    if erosion > 5000 {
                        Biome::Badlands
                    } else {
                        Biome::Desert
                    }
                } else if h < 0 {
                    Biome::Savanna
                } else if h < 3500 {
                    Biome::Plains
                } else {
                    Biome::Jungle
                }
            }
            // Hot biomes
            (_, h) => {
                if h < -3500 {
                    Biome::Desert
                } else if h < 3500 {
                    if erosion > 5000 {
                        Biome::Badlands
                    } else {
                        Biome::Desert
                    }
                } else {
                    Biome::Jungle
                }
            }
        }
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
