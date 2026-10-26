//! Structure starts, references and per-chunk placement
//! (Java `ChunkGenerator.createStructures`, `createReferences` and the
//! structure half of `applyBiomeDecoration`).
//!
//! Starts are a pure function of (seed, chunk): they are computed on demand
//! and cached. Decorating a chunk places, for each generation step, the
//! pieces of every start within 8 chunks whose bounding box intersects it.

use super::context::StructureContext;
use super::placement::is_structure_chunk;
use super::rings::RingPositions;
use super::start::{GenContext, StructureStart};
use super::transform::BoundingBox;
use crate::features::decorate::StepHook;
use crate::features::random::{FeatureRandom, LegacyRandom};
use crate::features::{BlockPos, FeatureWorld};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Starts that originate in one chunk, in set order.
pub type ChunkStarts = Arc<Vec<StructureStart>>;

/// Structure generation state of one dimension.
pub struct StructureManager {
    /// Bit per structure set: sets that can generate in this dimension.
    possible_sets: Vec<usize>,
    rings: RingPositions,
    cache: Mutex<HashMap<(i32, i32), ChunkStarts>>,
    cache_limit: usize,
    /// Most queried chunks have no starts; share that immutable result.
    empty_starts: ChunkStarts,
    /// Structure indices per generation step, in registry order.
    step_structures: [Vec<u16>; 11],
}

impl StructureManager {
    /// `possible_biomes`: biomes of the dimension's biome source.
    pub fn new(possible_biomes: u128) -> Self {
        let sets = &super::generated::STRUCTURE_SETS;
        let structures = &super::generated::STRUCTURES;
        let possible_sets = (0..sets.len())
            .filter(|&i| {
                sets[i]
                    .structures
                    .iter()
                    .any(|(s, _)| structures[*s as usize].biomes & possible_biomes != 0)
            })
            .collect();
        let mut step_structures: [Vec<u16>; 11] = Default::default();
        for (i, s) in structures.iter().enumerate() {
            step_structures[s.step as usize].push(i as u16);
        }
        Self {
            possible_sets,
            rings: RingPositions::new(sets.len()),
            cache: Mutex::new(HashMap::new()),
            cache_limit: 4096,
            empty_starts: Arc::new(Vec::new()),
            step_structures,
        }
    }

    /// `createStructures` for one chunk: at most one start per set.
    pub fn starts_in_chunk(&self, ctx: &dyn StructureContext, cx: i32, cz: i32) -> ChunkStarts {
        if let Some(s) = self.cache.lock().expect("structure cache").get(&(cx, cz)) {
            return s.clone();
        }
        let _stage = crate::profile::scope(crate::profile::Stage::StructureStarts);
        let mut out = Vec::new();
        for &set in &self.possible_sets {
            if !is_structure_chunk(&self.rings, set, ctx, cx, cz) {
                continue;
            }
            let def = &super::generated::STRUCTURE_SETS[set];
            if def.structures.len() == 1 {
                if let Some(s) = try_generate(ctx, def.structures[0].0, cx, cz) {
                    out.push(s);
                }
                continue;
            }
            let mut entries: Vec<(u16, i32)> = def.structures.to_vec();
            let mut random = LegacyRandom::new(0);
            random.set_large_feature_seed(ctx.seed(), cx, cz);
            let mut total: i32 = entries.iter().map(|e| e.1).sum();
            while !entries.is_empty() {
                let mut roll = random.next_int_bounded(total);
                let mut k = 0;
                for (i, e) in entries.iter().enumerate() {
                    k = i;
                    roll -= e.1;
                    if roll < 0 {
                        break;
                    }
                }
                let (structure, weight) = entries[k];
                if let Some(s) = try_generate(ctx, structure, cx, cz) {
                    out.push(s);
                    break;
                }
                entries.remove(k);
                total -= weight;
            }
        }
        let starts = if out.is_empty() {
            self.empty_starts.clone()
        } else {
            Arc::new(out)
        };
        let mut cache = self.cache.lock().expect("structure cache");
        if cache.len() >= self.cache_limit {
            cache.clear();
        }
        cache.insert((cx, cz), starts.clone());
        starts
    }

    /// Starts whose box intersects chunk (`cx`, `cz`), from chunks within 8
    /// (`createReferences`), as (structure, start) pairs grouped by structure.
    pub fn references(
        &self,
        ctx: &dyn StructureContext,
        cx: i32,
        cz: i32,
    ) -> Vec<(ChunkStarts, usize)> {
        let _stage = crate::profile::scope(crate::profile::Stage::StructureReferences);
        let (x0, z0) = (cx * 16, cz * 16);
        let mut out = Vec::new();
        for i in cx - 8..=cx + 8 {
            for j in cz - 8..=cz + 8 {
                let starts = self.starts_in_chunk(ctx, i, j);
                for (k, s) in starts.iter().enumerate() {
                    if s.bbox.intersects_xz(x0, z0, x0 + 15, z0 + 15) {
                        out.push((starts.clone(), k));
                    }
                }
            }
        }
        out
    }

    /// Decoration hook placing the structures of each step in chunk (`cx`, `cz`).
    pub fn decoration_hook<'a>(
        &'a self,
        ctx: &'a dyn StructureContext,
        cx: i32,
        cz: i32,
    ) -> StructureStepHook<'a> {
        StructureStepHook {
            manager: self,
            ctx,
            chunk: (cx, cz),
            refs: self.references(ctx, cx, cz),
        }
    }
}

/// `tryGenerateStructure`: run the structure's generator for this chunk.
fn try_generate(
    ctx: &dyn StructureContext,
    structure: u16,
    cx: i32,
    cz: i32,
) -> Option<StructureStart> {
    let def = &super::generated::STRUCTURES[structure as usize];
    let mut g = GenContext::new(ctx, def, cx, cz);
    let stub = super::kinds::find_generation_point(&mut g)?;
    let p = stub.position;
    let biome = ctx.noise_biome(p.x >> 2, p.y >> 2, p.z >> 2);
    if def.biomes & (1u128 << biome) == 0 {
        return None;
    }
    let pieces = (stub.build)(&mut g);
    StructureStart::new(structure, cx, cz, pieces)
}

/// [`StepHook`] placing structure pieces during decoration.
pub struct StructureStepHook<'a> {
    manager: &'a StructureManager,
    ctx: &'a dyn StructureContext,
    chunk: (i32, i32),
    refs: Vec<(ChunkStarts, usize)>,
}

impl<W: FeatureWorld> StepHook<W> for StructureStepHook<'_> {
    fn place_structures(
        &mut self,
        world: &mut W,
        rng: &mut FeatureRandom,
        decoration_seed: i64,
        step: usize,
    ) {
        let _stage = crate::profile::scope(crate::profile::Stage::StructurePieces);
        let (cx, cz) = self.chunk;
        // `getWritableArea`: the chunk column from min_y + 1.
        let chunk_box = BoundingBox::new(
            cx * 16,
            world.min_y() + 1,
            cz * 16,
            cx * 16 + 15,
            world.max_y() - 1,
            cz * 16 + 15,
        );
        for (m, &structure) in self.manager.step_structures[step].iter().enumerate() {
            rng.set_feature_seed(decoration_seed, m as i32, step as i32);
            for (starts, k) in &self.refs {
                let start = &starts[*k];
                if start.structure != structure {
                    continue;
                }
                place_in_chunk(start, world, self.ctx, rng, &chunk_box, self.chunk);
            }
        }
    }
}

/// `StructureStart.placeInChunk`.
pub fn place_in_chunk<W: FeatureWorld>(
    start: &StructureStart,
    world: &mut W,
    ctx: &dyn StructureContext,
    rng: &mut FeatureRandom,
    chunk_box: &BoundingBox,
    chunk: (i32, i32),
) {
    let Some(first) = start.pieces.first() else {
        return;
    };
    let bb = first.bbox();
    let c = bb.center();
    let pivot = BlockPos::new(c.x, bb.min_y, c.z);
    let world: &mut dyn FeatureWorld = world;
    for piece in &start.pieces {
        if piece.bbox().intersects(chunk_box) {
            piece.post_process(world, ctx, rng, chunk_box, chunk, pivot);
        }
    }
    super::kinds::after_place(
        start.def(),
        world,
        ctx,
        rng,
        chunk_box,
        chunk,
        &start.pieces,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::decorate::{BiomeSet, decorate_chunk};
    use crate::features::generated::{OVERWORLD_BIOMES, OVERWORLD_FEATURES, biome_index};
    use crate::features::java_blocks::states;
    use crate::features::test_world::TestWorld;
    use crate::features::{Heightmap, JavaState};
    use crate::structures::template_data::TEMPLATES_EMBEDDED;

    /// Flat plains at y 64 (top block y 63).
    struct Plains {
        seed: i64,
        biome: u8,
    }

    impl StructureContext for Plains {
        fn seed(&self) -> i64 {
            self.seed
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
        fn noise_biome(&self, _: i32, _: i32, _: i32) -> u8 {
            self.biome
        }
        fn base_height(&self, _: i32, _: i32, _: Heightmap) -> i32 {
            64
        }
        fn base_column(&self, _: i32, _: i32, out: &mut [JavaState]) {
            for (i, s) in out.iter_mut().enumerate() {
                *s = if i < 128 { states::STONE } else { states::AIR };
            }
        }
    }

    fn plains(seed: i64) -> Plains {
        Plains {
            seed,
            biome: biome_index("minecraft:plains").unwrap(),
        }
    }

    fn name(s: &StructureStart) -> &'static str {
        s.def().name
    }

    /// Over a 12x12 region grid of chunks, plains villages appear about once per
    /// 34-chunk village region and every start's chunk is its set's placement chunk.
    #[test]
    fn plains_starts_follow_placement() {
        if !TEMPLATES_EMBEDDED {
            return;
        }
        let ctx = plains(12345);
        let manager = StructureManager::new(OVERWORLD_BIOMES);
        let mut villages = 0;
        for cx in -48..48 {
            for cz in -48..48 {
                for s in manager.starts_in_chunk(&ctx, cx, cz).iter() {
                    assert!(!s.pieces.is_empty());
                    if name(s) == "minecraft:village_plains" {
                        villages += 1;
                        let set = super::super::generated::STRUCTURE_SETS
                            .iter()
                            .find(|d| d.name == "minecraft:villages")
                            .unwrap();
                        assert_eq!(set.placement.potential_chunk(ctx.seed, cx, cz), (cx, cz));
                    }
                }
            }
        }
        // 96x96 chunks / 34x34 regions: up to 9 regions fully inside.
        assert!((3..=16).contains(&villages), "{villages} villages");
    }

    /// Decorating a chunk next to a village with the structure hook writes
    /// structure blocks, deterministically.
    #[test]
    fn village_chunk_decorates_with_structures() {
        if !TEMPLATES_EMBEDDED {
            return;
        }
        let ctx = plains(12345);
        let manager = StructureManager::new(OVERWORLD_BIOMES);
        let mut found = None;
        'outer: for cx in -48..48 {
            for cz in -48..48 {
                for s in manager.starts_in_chunk(&ctx, cx, cz).iter() {
                    if name(s) == "minecraft:village_plains" {
                        found = Some((cx, cz));
                        break 'outer;
                    }
                }
            }
        }
        let (cx, cz) = found.expect("a plains village in range");
        let run = || {
            let mut w = TestWorld::flat(64);
            w.seed = ctx.seed;
            w.biome = ctx.biome;
            w.writable = Some((cx * 16 - 16, cz * 16 - 16, cx * 16 + 31, cz * 16 + 31));
            let mut set = BiomeSet::default();
            set.insert(ctx.biome);
            let mut hook = manager.decoration_hook(&ctx, cx, cz);
            decorate_chunk(&mut w, &OVERWORLD_FEATURES, cx, cz, set, &mut hook);
            w
        };
        let a = run();
        let b = run();
        assert_eq!(a.blocks, b.blocks);
        let structure_blocks = a
            .blocks
            .values()
            .filter(|s| {
                s.is(crate::features::java_blocks::blocks::OAK_PLANKS)
                    || s.is(crate::features::java_blocks::blocks::DIRT_PATH)
            })
            .count();
        assert!(structure_blocks > 0, "no village blocks in the start chunk");
    }
}
