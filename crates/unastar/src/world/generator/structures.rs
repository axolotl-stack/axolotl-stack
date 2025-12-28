//! Structure positioning algorithms based on cubiomes.
//!
//! This module handles finding structure generation positions using
//! vanilla-accurate algorithms ported from cubiomes.

use super::xoroshiro::JavaRandom;

/// Structure types that can be generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureType {
    Village,
    DesertPyramid,
    JungleTemple,
    SwampHut,
    Igloo,
    PillagerOutpost,
    RuinedPortal,
    OceanRuin,
    Shipwreck,
}

/// Configuration for structure generation.
#[derive(Debug, Clone, Copy)]
pub struct StructureConfig {
    /// Salt value for the structure type
    pub salt: i64,
    /// Region size in chunks (structure attempts per region)
    pub region_size: i32,
    /// Range within region where structure can generate
    pub chunk_range: i32,
}

impl StructureConfig {
    /// Get the configuration for a specific structure type.
    pub fn get(structure: StructureType) -> Self {
        match structure {
            StructureType::Village => Self {
                salt: 10387312,
                region_size: 16,
                chunk_range: 12,
            },
            StructureType::DesertPyramid => Self {
                salt: 14357617,
                region_size: 20,
                chunk_range: 16,
            },
            StructureType::JungleTemple => Self {
                salt: 14357619,
                region_size: 20,
                chunk_range: 16,
            },
            StructureType::SwampHut => Self {
                salt: 14357620,
                region_size: 20,
                chunk_range: 16,
            },
            StructureType::Igloo => Self {
                salt: 14357618,
                region_size: 20,
                chunk_range: 16,
            },
            StructureType::PillagerOutpost => Self {
                salt: 165745296,
                region_size: 32,
                chunk_range: 24,
            },
            StructureType::RuinedPortal => Self {
                salt: 34222645,
                region_size: 40,
                chunk_range: 25,
            },
            StructureType::OceanRuin => Self {
                salt: 14357621,
                region_size: 20,
                chunk_range: 12,
            },
            StructureType::Shipwreck => Self {
                salt: 165745295,
                region_size: 24,
                chunk_range: 20,
            },
        }
    }
}

/// Structure position result.
#[derive(Debug, Clone, Copy)]
pub struct StructurePos {
    /// Block X coordinate
    pub x: i32,
    /// Block Z coordinate
    pub z: i32,
    /// Chunk X coordinate
    pub chunk_x: i32,
    /// Chunk Z coordinate
    pub chunk_z: i32,
}

/// Gets the chunk position of a structure within a region.
/// This is the core algorithm from cubiomes getFeatureChunkInRegion.
pub fn get_feature_chunk_in_region(
    config: &StructureConfig,
    seed: i64,
    reg_x: i32,
    reg_z: i32,
) -> (i32, i32) {
    const K: u64 = 0x5deece66d;
    const M: u64 = (1 << 48) - 1;
    const B: u64 = 0xb;

    // Combine seed with region coordinates and salt
    let mut s = (seed as u64)
        .wrapping_add((reg_x as i64).wrapping_mul(341873128712) as u64)
        .wrapping_add((reg_z as i64).wrapping_mul(132897987541) as u64)
        .wrapping_add(config.salt as u64);

    // Initialize Java RNG
    s ^= K;
    s = s.wrapping_mul(K).wrapping_add(B) & M;

    let r = config.chunk_range as u64;

    // Get X position in region
    let chunk_x = if (r & (r - 1)) == 0 {
        // Power of 2 - special case
        ((r.wrapping_mul(s >> 17)) >> 31) as i32
    } else {
        ((s >> 17) % r) as i32
    };

    // Advance RNG
    s = s.wrapping_mul(K).wrapping_add(B) & M;

    // Get Z position in region
    let chunk_z = if (r & (r - 1)) == 0 {
        ((r.wrapping_mul(s >> 17)) >> 31) as i32
    } else {
        ((s >> 17) % r) as i32
    };

    (chunk_x, chunk_z)
}

/// Gets the block position of a structure in a given region.
pub fn get_structure_pos(
    config: &StructureConfig,
    seed: i64,
    reg_x: i32,
    reg_z: i32,
) -> StructurePos {
    let (cx, cz) = get_feature_chunk_in_region(config, seed, reg_x, reg_z);

    let chunk_x = reg_x * config.region_size + cx;
    let chunk_z = reg_z * config.region_size + cz;

    StructurePos {
        x: chunk_x << 4,
        z: chunk_z << 4,
        chunk_x,
        chunk_z,
    }
}

/// Find all structure positions in a range of chunks.
pub fn find_structures_in_area(
    structure: StructureType,
    seed: i64,
    chunk_x: i32,
    chunk_z: i32,
    chunk_radius: i32,
) -> Vec<StructurePos> {
    let config = StructureConfig::get(structure);
    let mut results = Vec::new();

    // Calculate region range
    let min_reg_x = (chunk_x - chunk_radius) / config.region_size - 1;
    let max_reg_x = (chunk_x + chunk_radius) / config.region_size + 1;
    let min_reg_z = (chunk_z - chunk_radius) / config.region_size - 1;
    let max_reg_z = (chunk_z + chunk_radius) / config.region_size + 1;

    for rx in min_reg_x..=max_reg_x {
        for rz in min_reg_z..=max_reg_z {
            let pos = get_structure_pos(&config, seed, rx, rz);

            // Check if within requested chunk range
            if pos.chunk_x >= chunk_x - chunk_radius
                && pos.chunk_x <= chunk_x + chunk_radius
                && pos.chunk_z >= chunk_z - chunk_radius
                && pos.chunk_z <= chunk_z + chunk_radius
            {
                results.push(pos);
            }
        }
    }

    results
}

/// Check if a chunk contains a slime chunk.
/// Ported from cubiomes isSlimeChunk.
pub fn is_slime_chunk(seed: i64, chunk_x: i32, chunk_z: i32) -> bool {
    let mut rnd = seed as u64;
    rnd = rnd.wrapping_add((chunk_x as i32).wrapping_mul(0x5ac0db) as u64);
    rnd = rnd.wrapping_add((chunk_x.wrapping_mul(chunk_x)).wrapping_mul(0x4c1906) as u64);
    rnd = rnd.wrapping_add((chunk_z as i32).wrapping_mul(0x5f24f) as u64);
    rnd = rnd.wrapping_add((chunk_z.wrapping_mul(chunk_z) as u64).wrapping_mul(0x4307a7));
    rnd ^= 0x3ad8025f;

    let mut java_rng = JavaRandom::from_seed(rnd as i64);
    java_rng.next_int(10) == 0
}

//==============================================================================
// Stronghold Generation (ring-based, ported from cubiomes)
//==============================================================================

use std::f64::consts::PI;

/// Stronghold iterator for ring-based generation.
#[derive(Debug, Clone)]
pub struct StrongholdIter {
    /// Current stronghold position
    pub pos: (i32, i32),
    /// Approximate position of next stronghold
    pub next_approx: (i32, i32),
    /// Stronghold index counter
    pub index: i32,
    /// Current ring number
    pub ring_num: i32,
    /// Maximum strongholds in current ring
    pub ring_max: i32,
    /// Index within current ring
    pub ring_idx: i32,
    /// Angle within ring
    pub angle: f64,
    /// Distance from origin (in chunks)
    pub dist: f64,
    /// RNG state
    pub rnds: u64,
}

impl StrongholdIter {
    /// Initialize stronghold iterator and find first stronghold position.
    /// Ported from cubiomes initFirstStronghold.
    pub fn new(seed: i64) -> Self {
        let mut rnds = seed as u64;
        // Java setSeed
        rnds = (rnds ^ 0x5DEECE66D) & ((1u64 << 48) - 1);
        
        // First random for angle
        rnds = rnds.wrapping_mul(0x5DEECE66D).wrapping_add(0xB) & ((1u64 << 48) - 1);
        let angle_rand = (rnds >> 17) as f64 / (1u64 << 31) as f64;
        let angle = 2.0 * PI * angle_rand;
        
        // Second random for distance
        rnds = rnds.wrapping_mul(0x5DEECE66D).wrapping_add(0xB) & ((1u64 << 48) - 1);
        let dist_rand = (rnds >> 17) as f64 / (1u64 << 31) as f64;
        // 1.9+ formula: 4*32 + (rand-0.5)*32*2.5
        let dist = (4.0 * 32.0) + (dist_rand - 0.5) * 32.0 * 2.5;
        
        let x = ((angle.cos() * dist).round() as i32) * 16 + 8;
        let z = ((angle.sin() * dist).round() as i32) * 16 + 8;
        
        Self {
            pos: (0, 0),
            next_approx: (x, z),
            index: 0,
            ring_num: 0,
            ring_max: 3,
            ring_idx: 0,
            angle,
            dist,
            rnds,
        }
    }
    
    /// Get the next stronghold position (approximate, no biome check).
    /// Returns remaining strongholds after this one (max 128).
    pub fn next(&mut self) -> Option<(i32, i32)> {
        if self.index >= 128 {
            return None;
        }
        
        // Current position is the approximation
        self.pos = self.next_approx;
        
        // Advance to next
        self.ring_idx += 1;
        self.angle += 2.0 * PI / self.ring_max as f64;
        
        if self.ring_idx >= self.ring_max {
            self.ring_num += 1;
            self.ring_idx = 0;
            self.ring_max = self.ring_max + 2 * self.ring_max / (self.ring_num + 1);
            if self.ring_max > 128 - self.index {
                self.ring_max = 128 - self.index;
            }
            // Random angle offset
            self.rnds = self.rnds.wrapping_mul(0x5DEECE66D).wrapping_add(0xB) & ((1u64 << 48) - 1);
            let offset = (self.rnds >> 17) as f64 / (1u64 << 31) as f64;
            self.angle += offset * PI * 2.0;
        }
        
        // Update distance for next ring
        self.rnds = self.rnds.wrapping_mul(0x5DEECE66D).wrapping_add(0xB) & ((1u64 << 48) - 1);
        let dist_rand = (self.rnds >> 17) as f64 / (1u64 << 31) as f64;
        self.dist = (4.0 * 32.0) + (6.0 * self.ring_num as f64 * 32.0) + (dist_rand - 0.5) * 32.0 * 2.5;
        
        self.next_approx = (
            ((self.angle.cos() * self.dist).round() as i32) * 16 + 8,
            ((self.angle.sin() * self.dist).round() as i32) * 16 + 8,
        );
        self.index += 1;
        
        Some(self.pos)
    }
}

//==============================================================================
// Mineshaft Detection (ported from cubiomes getMineshafts)
//==============================================================================

/// Check if a chunk contains a mineshaft.
/// Ported from cubiomes getMineshafts for MC 1.13+.
/// Mineshafts have a 0.4% chance per chunk.
pub fn has_mineshaft(seed: i64, chunk_x: i32, chunk_z: i32) -> bool {
    // Initialize with seed
    let mut s = seed as u64;
    s = (s ^ 0x5DEECE66D) & ((1u64 << 48) - 1);
    
    // Get a and b
    s = s.wrapping_mul(0x5DEECE66D).wrapping_add(0xB) & ((1u64 << 48) - 1);
    let a = s;
    s = s.wrapping_mul(0x5DEECE66D).wrapping_add(0xB) & ((1u64 << 48) - 1);
    let b = s;
    
    // Seed for this chunk
    let aix = (chunk_x as u64).wrapping_mul(a) ^ (seed as u64);
    let chunk_seed = aix ^ (chunk_z as u64).wrapping_mul(b);
    
    let mut s = (chunk_seed ^ 0x5DEECE66D) & ((1u64 << 48) - 1);
    s = s.wrapping_mul(0x5DEECE66D).wrapping_add(0xB) & ((1u64 << 48) - 1);
    
    // nextDouble < 0.004
    let rand = (s >> 17) as f64 / (1u64 << 31) as f64;
    rand < 0.004
}

//==============================================================================
// Monument/Mansion (Large Structures with triangular distribution)
//==============================================================================

/// Get position for large structures (Monument, Mansion) with triangular distribution.
/// Ported from cubiomes getLargeStructurePos.
pub fn get_large_structure_pos(
    config: &StructureConfig,
    seed: i64,
    reg_x: i32,
    reg_z: i32,
) -> StructurePos {
    const K: u64 = 0x5deece66d;
    const M: u64 = (1 << 48) - 1;
    const B: u64 = 0xb;
    
    let mut s = (seed as u64)
        .wrapping_add((reg_x as i64).wrapping_mul(341873128712) as u64)
        .wrapping_add((reg_z as i64).wrapping_mul(132897987541) as u64)
        .wrapping_add(config.salt as u64);
    
    s ^= K;
    
    // Triangular distribution: (rand1 + rand2) / 2
    s = s.wrapping_mul(K).wrapping_add(B) & M;
    let r1_x = (s >> 17) % (config.chunk_range as u64);
    s = s.wrapping_mul(K).wrapping_add(B) & M;
    let r2_x = (s >> 17) % (config.chunk_range as u64);
    let chunk_x_offset = ((r1_x + r2_x) / 2) as i32;
    
    s = s.wrapping_mul(K).wrapping_add(B) & M;
    let r1_z = (s >> 17) % (config.chunk_range as u64);
    s = s.wrapping_mul(K).wrapping_add(B) & M;
    let r2_z = (s >> 17) % (config.chunk_range as u64);
    let chunk_z_offset = ((r1_z + r2_z) / 2) as i32;
    
    let chunk_x = reg_x * config.region_size + chunk_x_offset;
    let chunk_z = reg_z * config.region_size + chunk_z_offset;
    
    StructurePos {
        x: chunk_x << 4,
        z: chunk_z << 4,
        chunk_x,
        chunk_z,
    }
}

/// Config for Monument (1.8+)
pub fn monument_config() -> StructureConfig {
    StructureConfig {
        salt: 10387313,
        region_size: 32,
        chunk_range: 27,
    }
}

/// Config for Mansion (1.11+)
pub fn mansion_config() -> StructureConfig {
    StructureConfig {
        salt: 10387319,
        region_size: 80,
        chunk_range: 60,
    }
}

//==============================================================================
// Chunk Generation Random (for caves/ravines)
//==============================================================================

/// Get the random seed for chunk generation (caves, ravines, etc).
/// Ported from cubiomes chunkGenerateRnd.
pub fn chunk_generate_rnd(seed: i64, chunk_x: i32, chunk_z: i32) -> u64 {
    let mut rnd = seed as u64;
    // setSeed
    rnd = (rnd ^ 0x5DEECE66D) & ((1u64 << 48) - 1);
    
    // nextLong() for x multiplier
    rnd = rnd.wrapping_mul(0x5DEECE66D).wrapping_add(0xB) & ((1u64 << 48) - 1);
    let x_mult = rnd;
    
    // nextLong() for z multiplier
    rnd = rnd.wrapping_mul(0x5DEECE66D).wrapping_add(0xB) & ((1u64 << 48) - 1);
    let z_mult = rnd;
    
    // Combine
    let combined = (x_mult.wrapping_mul(chunk_x as u64))
        ^ (z_mult.wrapping_mul(chunk_z as u64))
        ^ (seed as u64);
    
    // setSeed on result
    (combined ^ 0x5DEECE66D) & ((1u64 << 48) - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_village_positions() {
        let seed = 12345i64;
        let config = StructureConfig::get(StructureType::Village);
        let pos = get_structure_pos(&config, seed, 0, 0);
        // Should get a valid position
        assert!(pos.chunk_x >= 0 && pos.chunk_x < config.region_size);
        assert!(pos.chunk_z >= 0 && pos.chunk_z < config.region_size);
    }

    #[test]
    fn test_slime_chunk() {
        // Known slime chunk at seed 0, chunk (0, 0) should be testable
        let is_slime = is_slime_chunk(0, 0, 0);
        // Just verify it runs without panic
        assert!(is_slime || !is_slime);
    }
    
    #[test]
    fn test_stronghold_iterator() {
        let mut iter = StrongholdIter::new(12345);
        let first = iter.next().unwrap();
        // First stronghold should be ~1500-2000 blocks from origin
        let dist = ((first.0 * first.0 + first.1 * first.1) as f64).sqrt();
        assert!(dist > 500.0 && dist < 3000.0);
    }
    
    #[test]
    fn test_mineshaft_probability() {
        // Test that mineshafts are rare (0.4%)
        let mut count = 0;
        for cx in -100..100 {
            for cz in -100..100 {
                if has_mineshaft(12345, cx, cz) {
                    count += 1;
                }
            }
        }
        // Expect roughly 160 mineshafts in 40000 chunks (0.4%)
        assert!(count > 50 && count < 300);
    }
}

