//! Demand-resolved concentric-ring placements. Each point keeps the exact fork
//! drawn by Java's sequential ring walk; biome relocation is independent per point.
//!
//! A 112-block biome search around `(chunk * 16 + 8)` visits quart offsets
//! `[-28, 28]` around `chunk * 4 + 2`. Converting the selected quart to a chunk
//! therefore moves each axis by at most seven chunks, including negative positions.
//! Only points inside that square can possibly answer a placement query.

use super::context::StructureContext;
use super::placement::PlacementKind;
use crate::features::random::LegacyRandom;
use std::sync::OnceLock;

/// Per-dimension, single-flight ring positions, resolved only where requested.
pub struct RingPositions {
    cells: Vec<OnceLock<SetRings>>,
}

impl RingPositions {
    pub fn new(set_count: usize) -> Self {
        Self {
            cells: (0..set_count).map(|_| OnceLock::new()).collect(),
        }
    }

    pub fn contains(&self, set: usize, ctx: &dyn StructureContext, x: i32, z: i32) -> bool {
        self.cells[set]
            .get_or_init(
                || match super::generated::STRUCTURE_SETS[set].placement.kind {
                    PlacementKind::ConcentricRings {
                        distance,
                        spread,
                        count,
                        preferred_biomes,
                    } => SetRings::new(ctx.seed(), distance, spread, count, preferred_biomes),
                    PlacementKind::RandomSpread { .. } => {
                        panic!("ring query for random-spread set")
                    }
                },
            )
            .contains(ctx, x, z)
    }
}

struct RingPoint {
    chunk: (i32, i32),
    random: LegacyRandom,
    relocated: OnceLock<(i32, i32)>,
}

struct SetRings {
    points: Vec<RingPoint>,
    preferred: u128,
}

impl SetRings {
    fn new(seed: i64, distance: i32, spread: i32, count: i32, preferred: u128) -> Self {
        let mut points = Vec::with_capacity(count as usize);
        let mut random = LegacyRandom::new(seed);
        let mut angle = random.next_double() * std::f64::consts::PI * 2.0;
        let mut in_ring = 0;
        let mut ring = 0;
        let mut per_ring = spread;
        for j in 0..count {
            let dist = (4 * distance + distance * ring * 6) as f64
                + (random.next_double() - 0.5) * distance as f64 * 2.5;
            // Java Math.round: floor(x + 0.5), not Rust's ties-away rounding.
            let cx = (angle.cos() * dist + 0.5).floor() as i32;
            let cz = (angle.sin() * dist + 0.5).floor() as i32;
            points.push(RingPoint {
                chunk: (cx, cz),
                random: random.fork(),
                relocated: OnceLock::new(),
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
        Self { points, preferred }
    }

    fn contains(&self, ctx: &dyn StructureContext, x: i32, z: i32) -> bool {
        self.points.iter().any(|point| {
            x.abs_diff(point.chunk.0) <= 7
                && z.abs_diff(point.chunk.1) <= 7
                && *point.relocated.get_or_init(|| {
                    relocate(ctx, point.chunk, self.preferred, &mut point.random.clone())
                }) == (x, z)
        })
    }
}

/// Java `findBiomeHorizontal`: reservoir-sample matching quarts in scan order.
fn relocate(
    ctx: &dyn StructureContext,
    chunk: (i32, i32),
    preferred: u128,
    random: &mut LegacyRandom,
) -> (i32, i32) {
    let qx = ((chunk.0 << 4) + 8) >> 2;
    let qz = ((chunk.1 << 4) + 8) >> 2;
    let mut found = None;
    let mut matches = 0;
    for dz in -28..=28 {
        for dx in -28..=28 {
            let bx = qx + dx;
            let bz = qz + dz;
            let biome = ctx.noise_biome(bx, 0, bz);
            if preferred & (1u128 << biome) != 0 {
                if found.is_none() || random.next_int_bounded(matches + 1) == 0 {
                    found = Some((bx >> 2, bz >> 2));
                }
                matches += 1;
            }
        }
    }
    found.unwrap_or(chunk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::{Heightmap, JavaState};
    use std::collections::BTreeSet;

    struct Climate(i64);
    impl StructureContext for Climate {
        fn seed(&self) -> i64 {
            self.0
        }
        fn min_y(&self) -> i32 {
            -64
        }
        fn height(&self) -> i32 {
            384
        }
        fn sea_level(&self) -> i32 {
            63
        }
        fn noise_biome(&self, x: i32, _: i32, z: i32) -> u8 {
            u8::from((x ^ z) & 3 == 0)
        }
        fn base_height(&self, _: i32, _: i32, _: Heightmap) -> i32 {
            panic!("ring relocation does not query terrain")
        }
        fn base_column(&self, _: i32, _: i32, _: &mut [JavaState]) {
            panic!("ring relocation does not query terrain")
        }
    }

    #[test]
    fn selective_rings_equal_eager_relocation_at_both_square_boundaries() {
        for seed in [0, 12345, -91827364] {
            let ctx = Climate(seed);
            for preferred in [0, 1 << 1] {
                let eager = SetRings::new(seed, 32, 3, 128, preferred);
                let expected: BTreeSet<_> = eager
                    .points
                    .iter()
                    .map(|point| relocate(&ctx, point.chunk, preferred, &mut point.random.clone()))
                    .collect();
                let mut queries = BTreeSet::new();
                for point in &eager.points {
                    // Include the +/-7 reachable edges and +/-8 rejected edges,
                    // covering both signs without relying on a particular seed layout.
                    for dx in -8..=8 {
                        for dz in -8..=8 {
                            queries.insert((point.chunk.0 + dx, point.chunk.1 + dz));
                        }
                    }
                }
                let selective = SetRings::new(seed, 32, 3, 128, preferred);
                let actual: BTreeSet<_> = queries
                    .iter()
                    .rev()
                    .copied()
                    .filter(|&(x, z)| selective.contains(&ctx, x, z))
                    .collect();
                assert_eq!(actual, expected, "seed={seed}, preferred={preferred}");
                // The same independent point forks must survive concurrent access.
                let parallel = SetRings::new(seed, 32, 3, 128, preferred);
                let queries: Vec<_> = queries.into_iter().collect();
                let actual: BTreeSet<_> = std::thread::scope(|scope| {
                    let handles: Vec<_> = queries
                        .chunks(queries.len().div_ceil(4))
                        .map(|part| {
                            let (parallel, ctx) = (&parallel, &ctx);
                            scope.spawn(move || {
                                part.iter()
                                    .copied()
                                    .filter(|&(x, z)| parallel.contains(ctx, x, z))
                                    .collect::<Vec<_>>()
                            })
                        })
                        .collect();
                    handles
                        .into_iter()
                        .flat_map(|handle| handle.join().unwrap())
                        .collect()
                });
                assert_eq!(
                    actual, expected,
                    "parallel seed={seed}, preferred={preferred}"
                );
            }
        }
    }

    #[test]
    fn empty_ring_set_never_places_even_at_coordinate_extremes() {
        let empty = SetRings::new(12345, 32, 3, 0, 1);
        for (x, z) in [(0, 0), (i32::MIN, i32::MAX), (i32::MAX, i32::MIN)] {
            assert!(!empty.contains(&Climate(12345), x, z));
        }
    }
}
