//! Structure set placement (`StructurePlacement`): which chunks may hold a
//! start of a set, with frequency reduction and exclusion zones.

use super::context::StructureContext;
use crate::features::random::LegacyRandom;
use std::sync::OnceLock;

/// `StructurePlacement.FrequencyReductionMethod`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrequencyReduction {
    Default,
    LegacyType1,
    LegacyType2,
    LegacyType3,
}

impl FrequencyReduction {
    /// `shouldGenerate(seed, salt, x, z, probability)`.
    pub fn should_generate(self, seed: i64, salt: i32, x: i32, z: i32, probability: f32) -> bool {
        let mut r = LegacyRandom::new(0);
        match self {
            // Java passes (seed, salt, x, z) to setLargeFeatureWithSalt(seed,
            // regionX, regionZ, salt): the salt lands in the region-x slot.
            FrequencyReduction::Default => {
                r.set_large_feature_with_salt(seed, salt, x, z);
                r.next_float() < probability
            }
            FrequencyReduction::LegacyType1 => {
                let i = x >> 4;
                let j = z >> 4;
                r.set_seed(((i ^ (j << 4)) as i64) ^ seed);
                r.next_int();
                r.next_int_bounded((1.0 / probability) as i32) == 0
            }
            FrequencyReduction::LegacyType2 => {
                r.set_large_feature_with_salt(seed, x, z, 10_387_320);
                r.next_float() < probability
            }
            FrequencyReduction::LegacyType3 => {
                r.set_large_feature_seed(seed, x, z);
                r.next_double() < probability as f64
            }
        }
    }
}

/// `RandomSpreadType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpreadType {
    Linear,
    Triangular,
}

/// Set placement kind.
#[derive(Debug, Clone, Copy)]
pub enum PlacementKind {
    RandomSpread {
        spacing: i32,
        separation: i32,
        spread: SpreadType,
    },
    ConcentricRings {
        distance: i32,
        spread: i32,
        count: i32,
        /// Biome indices of `preferred_biomes`.
        preferred_biomes: u128,
    },
}

/// `StructurePlacement` common fields.
#[derive(Debug)]
pub struct Placement {
    pub kind: PlacementKind,
    pub locate_offset: [i32; 3],
    pub frequency_reduction: FrequencyReduction,
    pub frequency: f32,
    pub salt: i32,
    /// `exclusion_zone`: (other set index, chunk count).
    pub exclusion_zone: Option<(u16, i32)>,
}

/// A structure set (`worldgen/structure_set`).
#[derive(Debug)]
pub struct StructureSetDef {
    pub name: &'static str,
    /// (structure index, weight).
    pub structures: &'static [(u16, i32)],
    pub placement: Placement,
}

/// Lazily computed concentric ring positions per set (strongholds).
pub struct RingPositions {
    cells: Vec<OnceLock<Vec<(i32, i32)>>>,
}

impl RingPositions {
    pub fn new(set_count: usize) -> Self {
        Self {
            cells: (0..set_count).map(|_| OnceLock::new()).collect(),
        }
    }

    /// Ring chunk positions of set `set` (computed on first use).
    pub fn get(&self, set: usize, ctx: &dyn StructureContext) -> &[(i32, i32)] {
        self.cells[set].get_or_init(|| {
            let def = &super::generated::STRUCTURE_SETS[set];
            match def.placement.kind {
                PlacementKind::ConcentricRings {
                    distance,
                    spread,
                    count,
                    preferred_biomes,
                } => ring_positions(ctx, distance, spread, count, preferred_biomes),
                PlacementKind::RandomSpread { .. } => Vec::new(),
            }
        })
    }
}

/// `ChunkGeneratorStructureState.generateRingPositions` (concentric rings
/// seed = level seed). Each position is moved to a preferred biome found by
/// `findBiomeHorizontal` within 112 blocks of the ring point.
pub fn ring_positions(ctx: &dyn StructureContext, distance: i32, spread: i32, count: i32, preferred: u128) -> Vec<(i32, i32)> {
    if count == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(count as usize);
    let mut random = LegacyRandom::new(ctx.seed());
    let mut angle = random.next_double() * std::f64::consts::PI * 2.0;
    let mut in_ring = 0;
    let mut ring = 0;
    let mut per_ring = spread;
    for j in 0..count {
        let dist = (4 * distance + distance * ring * 6) as f64 + (random.next_double() - 0.5) * distance as f64 * 2.5;
        // Java Math.round: floor(x + 0.5).
        let cx = (angle.cos() * dist + 0.5).floor() as i32;
        let cz = (angle.sin() * dist + 0.5).floor() as i32;
        let mut fork = random.fork();
        let found = find_biome_horizontal(ctx, (cx << 4) + 8, 0, (cz << 4) + 8, 112, preferred, &mut fork);
        out.push(match found {
            Some((x, _, z)) => (x >> 4, z >> 4),
            None => (cx, cz),
        });
        angle += std::f64::consts::PI * 2.0 / per_ring as f64;
        in_ring += 1;
        if in_ring == per_ring {
            ring += 1;
            in_ring = 0;
            per_ring += 2 * per_ring / (ring + 1);
            per_ring = per_ring.min(count - j);
            angle += random.next_double() * std::f64::consts::PI * 2.0;
        }
    }
    out
}

/// `BiomeSource.findBiomeHorizontal(x, y, z, radius, 1, predicate, random, false)`:
/// a uniformly random matching quart on the ring at `radius` (reservoir
/// sampling over the square's outline and interior as Java iterates it).
pub fn find_biome_horizontal(
    ctx: &dyn StructureContext,
    x: i32,
    y: i32,
    z: i32,
    radius: i32,
    biomes: u128,
    random: &mut LegacyRandom,
) -> Option<(i32, i32, i32)> {
    let qx = x >> 2;
    let qz = z >> 2;
    let r = radius >> 2;
    let qy = y >> 2;
    let mut found = None;
    let mut matches = 0;
    for dz in -r..=r {
        for dx in -r..=r {
            let bx = qx + dx;
            let bz = qz + dz;
            let biome = ctx.noise_biome(bx, qy, bz);
            if biomes & (1u128 << biome) != 0 {
                if found.is_none() || random.next_int_bounded(matches + 1) == 0 {
                    found = Some((bx << 2, y, bz << 2));
                }
                matches += 1;
            }
        }
    }
    found
}

impl Placement {
    /// `RandomSpreadStructurePlacement.getPotentialStructureChunk`.
    pub fn potential_chunk(&self, seed: i64, x: i32, z: i32) -> (i32, i32) {
        let PlacementKind::RandomSpread {
            spacing,
            separation,
            spread,
        } = self.kind
        else {
            return (x, z);
        };
        let i = x.div_euclid(spacing);
        let j = z.div_euclid(spacing);
        let mut r = LegacyRandom::new(0);
        r.set_large_feature_with_salt(seed, i, j, self.salt);
        let k = spacing - separation;
        let mut pick = || match spread {
            SpreadType::Linear => r.next_int_bounded(k),
            SpreadType::Triangular => (r.next_int_bounded(k) + r.next_int_bounded(k)) / 2,
        };
        let l = pick();
        let m = pick();
        (i * spacing + l, j * spacing + m)
    }

    /// `isPlacementChunk`.
    pub fn is_placement_chunk(&self, rings: &RingPositions, set: usize, ctx: &dyn StructureContext, x: i32, z: i32) -> bool {
        match self.kind {
            PlacementKind::RandomSpread { .. } => self.potential_chunk(ctx.seed(), x, z) == (x, z),
            PlacementKind::ConcentricRings { .. } => rings.get(set, ctx).contains(&(x, z)),
        }
    }
}

/// `StructurePlacement.isStructureChunk` for set `set`.
pub fn is_structure_chunk(rings: &RingPositions, set: usize, ctx: &dyn StructureContext, x: i32, z: i32) -> bool {
    let p = &super::generated::STRUCTURE_SETS[set].placement;
    if !p.is_placement_chunk(rings, set, ctx, x, z) {
        return false;
    }
    if p.frequency < 1.0 && !p.frequency_reduction.should_generate(ctx.seed(), p.salt, x, z, p.frequency) {
        return false;
    }
    if let Some((other, range)) = p.exclusion_zone {
        for i in x - range..=x + range {
            for j in z - range..=z + range {
                if is_structure_chunk(rings, other as usize, ctx, i, j) {
                    return false;
                }
            }
        }
    }
    true
}
