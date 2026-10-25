//! Structure starts, references and per-chunk placement
//! (Java `ChunkGenerator.createStructures`, `createReferences` and the
//! structure half of `applyBiomeDecoration`).
//!
//! Starts are a pure function of (seed, chunk): they are computed on demand
//! and cached. Decorating a chunk places, for each generation step, the
//! pieces of every start within 8 chunks whose bounding box intersects it.

use super::context::StructureContext;
use super::placement::{RingPositions, is_structure_chunk};
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
            step_structures,
        }
    }

    /// `createStructures` for one chunk: at most one start per set.
    pub fn starts_in_chunk(&self, ctx: &dyn StructureContext, cx: i32, cz: i32) -> ChunkStarts {
        if let Some(s) = self.cache.lock().expect("structure cache").get(&(cx, cz)) {
            return s.clone();
        }
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
        let starts = Arc::new(out);
        let mut cache = self.cache.lock().expect("structure cache");
        if cache.len() >= self.cache_limit {
            cache.clear();
        }
        cache.insert((cx, cz), starts.clone());
        starts
    }

    /// Starts whose box intersects chunk (`cx`, `cz`), from chunks within 8
    /// (`createReferences`), as (structure, start) pairs grouped by structure.
    pub fn references(&self, ctx: &dyn StructureContext, cx: i32, cz: i32) -> Vec<(ChunkStarts, usize)> {
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
    pub fn decoration_hook<'a>(&'a self, ctx: &'a dyn StructureContext, cx: i32, cz: i32) -> StructureStepHook<'a> {
        StructureStepHook {
            manager: self,
            ctx,
            chunk: (cx, cz),
            refs: self.references(ctx, cx, cz),
        }
    }
}

/// `tryGenerateStructure`: run the structure's generator for this chunk.
fn try_generate(ctx: &dyn StructureContext, structure: u16, cx: i32, cz: i32) -> Option<StructureStart> {
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
    fn place_structures(&mut self, world: &mut W, rng: &mut FeatureRandom, decoration_seed: i64, step: usize) {
        let (cx, cz) = self.chunk;
        // `getWritableArea`: the chunk column from min_y + 1.
        let chunk_box = BoundingBox::new(cx * 16, world.min_y() + 1, cz * 16, cx * 16 + 15, world.max_y() - 1, cz * 16 + 15);
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
    super::kinds::after_place(start.def(), world, ctx, rng, chunk_box, chunk, &start.pieces);
}
