//! `minecraft:geode_feature`: the vanilla amethyst geode, a layered shell
//! around a hollow core.
//!
//! Draws, in order, from the feature's own random:
//! 1. the number `n` of distribution points;
//! 2. the shell noise, a legacy normal noise (first octave -4, one
//!    amplitude) built from this random ([`NormalNoise`], 2,608 draws);
//! 3. the crack jitter, then whether a crack is generated (two floats);
//! 4. per point its `outer_wall_distance` offsets x, y, z (from `pos`) and
//!    its `point_offset`. Every point in air or a [`bb::geode_invalid`]
//!    block counts as invalid; more than `invalid_blocks_threshold` invalid
//!    points abort the geode;
//! 5. with a crack, its corner (`next_int(4)`): three points on a vertical
//!    line at heights 7, 5, 1, `2n + 1` from `pos` in x and/or z.
//!
//! Every block within `max_radius` of `pos` (a cube) gets, in `f32`,
//! `s = sum(inv_sqrt(dist^2 + offset)) + n * noise * noise_multiplier`, with
//! the fast inverse square root (one Newton step). Thresholds
//! `1 / sqrt(layer + d)`, `d = n / max_outer_wall_distance` (the pack's
//! exclusive maximum), layers 2.2 / 3.2 / 4.2 and a fixed filling threshold
//! `1 / sqrt(1.7)`, pick `filler`, the inner layer, `middle_layer` or
//! `outer_layer`. Shell blocks below the filling threshold become air where
//! the crack value `sum(inv_sqrt(dist^2 + crack_point_offset)) + 3 * noise`
//! reaches `1 / sqrt((n > 3 ? d : 0) + jitter / 2 + base_crack_size)`.
//!
//! Blocks are visited x from `pos.x` up to `+max_radius`, then from
//! `pos.x - 1` down; within x the same for y; z ascending. Each inner-layer
//! block draws whether it is `alternate_inner_layer`, then (for alternate
//! blocks, or all when `placements_require_layer0_alternate` is off) whether
//! it is a growth candidate. Each candidate then draws one `inner_placements`
//! block and grows it into the first air or water neighbour in the order
//! down, up, north, south, west, east, facing away from the shell.
//!
//! Measured: both natural seed-12345 BDS 1.26.51 geodes are reproduced
//! block for block, shell and budding blocks (see the codegen README).
//! **Unmeasured:** BDS stops scanning past an empty slice of the cube, which
//! only differs from the full scan for shells split by noise, and skips
//! geodes that overlap a structure.
//!
//! Every write skips [`bb::geode_cannot_replace`] blocks (bedrock, spawners,
//! chests, ...) and clears the position's liquid layer. Vanilla schedules
//! fluid updates next to the crack; decoration has no tick queue, so it does
//! not.

use super::super::generated::bb;
use super::super::legacy_noise::NormalNoise;
use super::super::world::{BlockPos, BlockWorld, Ctx, OUT_OF_BOUNDS};
use super::IntRange;
use smallvec::SmallVec;

/// Filling threshold `1 / sqrt(1.7)`, as BDS stores it.
const FILLING: f32 = f32::from_bits(0x3f44_57d1);
/// Layer constants of the inner, middle and outer thresholds.
const INNER_LAYER: f32 = 2.2;
const MIDDLE_LAYER: f32 = 3.2;
const OUTER_LAYER: f32 = 4.2;
/// Shell noise: first octave and amplitudes (not part of the pack data).
const NOISE_FIRST_OCTAVE: i32 = -4;
const NOISE_AMPLITUDES: [f32; 1] = [1.0];
/// Points of a crack.
const CRACK_POINTS: usize = 3;

/// Growth directions of `inner_placements`, in the order of
/// [`Params::inner_placements`]' state columns (Bedrock `minecraft:block_face`
/// order): down, up, north, south, west, east.
const FACES: [(i32, i32, i32); 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];

/// Parameters of one geode feature.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// Core (`filler`).
    pub filler: u32,
    /// Layer 0 (`inner_layer`).
    pub inner_layer: u32,
    /// Layer 0 variant (`alternate_inner_layer`, budding amethyst).
    pub alternate_inner_layer: u32,
    /// `middle_layer`.
    pub middle_layer: u32,
    /// `outer_layer`.
    pub outer_layer: u32,
    /// `inner_placements`: per block, its states facing down, up, north,
    /// south, west, east (all equal for blocks without a facing).
    pub inner_placements: &'static [[u32; 6]],
    /// Per-axis offset of distribution points (inclusive; the pack's
    /// `max_outer_wall_distance` is exclusive).
    pub outer_wall_distance: IntRange,
    /// Number of distribution points (inclusive; pack max exclusive).
    pub distribution_points: IntRange,
    /// Distance offset added per point (inclusive; pack max exclusive).
    pub point_offset: IntRange,
    /// Half-size of the cube the shell is evaluated in.
    pub max_radius: i32,
    pub crack_point_offset: f32,
    pub generate_crack_chance: f32,
    pub base_crack_size: f32,
    pub noise_multiplier: f32,
    pub use_potential_placements_chance: f32,
    pub use_alternate_layer0_chance: f32,
    pub placements_require_layer0_alternate: bool,
    pub invalid_blocks_threshold: i32,
}

/// Place one geode with its distribution points offset from `pos`;
/// `Some(pos)` when any block changed.
pub fn place<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
    params: &Params,
) -> Option<BlockPos> {
    let points = params.distribution_points.sample(ctx.rng).max(1);
    let noise = NormalNoise::new(ctx.rng, NOISE_FIRST_OCTAVE, &NOISE_AMPLITUDES);
    let crack_jitter = ctx.rng.next_float();
    let t = Thresholds::new(
        points,
        params.outer_wall_distance.max + 1,
        params.base_crack_size,
        crack_jitter,
    );
    let generate_crack = ctx.rng.next_float() < params.generate_crack_chance;

    // Vanilla draws 3..=4 centres; larger custom counts retain growable storage.
    let mut centres: SmallVec<[(BlockPos, f32); 4]> = SmallVec::with_capacity(points as usize);
    let mut invalid = 0;
    for _ in 0..points {
        let dx = params.outer_wall_distance.sample(ctx.rng);
        let dy = params.outer_wall_distance.sample(ctx.rng);
        let dz = params.outer_wall_distance.sample(ctx.rng);
        let at = pos.offset(dx, dy, dz);
        let b = ctx.get(at);
        if b == OUT_OF_BOUNDS || bb::air(b) || bb::geode_invalid(b) {
            invalid += 1;
            if invalid > params.invalid_blocks_threshold {
                return None;
            }
        }
        // Invalid points still shape the geode (vanilla keeps them).
        centres.push((at, params.point_offset.sample(ctx.rng) as f32));
    }

    let cracks = generate_crack
        .then(|| crack_offsets(ctx.rng.next_int(4), points).map(|(x, y, z)| pos.offset(x, y, z)));
    let crack_point_offset = params.crack_point_offset;
    let r = params.max_radius;
    let n_points = points as f32;
    // Keep common growth-candidate scratch inline; larger geodes still spill.
    let mut potential: SmallVec<[BlockPos; 32]> = SmallVec::new();
    let mut placed = false;
    for x in scan(pos.x, r) {
        for y in scan(pos.y, r) {
            for z in pos.z - r..=pos.z + r {
                let at = BlockPos::new(x, y, z);
                let n = noise.sample(x as f32, y as f32, z as f32) * params.noise_multiplier;
                let mut s = 0.0f32;
                for &(c, offset) in &centres {
                    s += fast_inv_sqrt(dist_sq(at, c) + offset);
                }
                let s = n_points * n + s;
                if s < t.outer {
                    continue;
                }
                let cracked = s < t.filling
                    && cracks.is_some_and(|cracks| {
                        let mut v = 0.0f32;
                        for c in cracks {
                            v += fast_inv_sqrt(dist_sq(at, c) + crack_point_offset);
                        }
                        v + n * CRACK_POINTS as f32 >= t.crack
                    });
                let block = if cracked {
                    bb::AIR
                } else if s >= t.filling {
                    params.filler
                } else if s >= t.inner {
                    let alternate = ctx.rng.probability(params.use_alternate_layer0_chance);
                    if (!params.placements_require_layer0_alternate || alternate)
                        && ctx.rng.probability(params.use_potential_placements_chance)
                    {
                        potential.push(at);
                    }
                    if alternate {
                        params.alternate_inner_layer
                    } else {
                        params.inner_layer
                    }
                } else if s >= t.middle {
                    params.middle_layer
                } else {
                    params.outer_layer
                };
                placed |= safe_set(ctx, at, block);
            }
        }
    }

    let kinds = params.inner_placements;
    if !kinds.is_empty() {
        for at in potential {
            let faces = &kinds[ctx.rng.next_int(kinds.len() as i32) as usize];
            for (face, &(dx, dy, dz)) in FACES.iter().enumerate() {
                let q = at.offset(dx, dy, dz);
                let b = ctx.get(q);
                if bb::air(b) || bb::water(b) {
                    placed |= safe_set(ctx, q, faces[face]);
                    break;
                }
            }
        }
    }
    placed.then_some(pos)
}

/// Scan order along one axis: `centre ..= centre + r`, then
/// `centre - 1` down to `centre - r`.
#[inline]
fn scan(centre: i32, r: i32) -> impl Iterator<Item = i32> {
    (centre..=centre + r).chain((centre - r..centre).rev())
}

/// Shell value thresholds of one geode.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Thresholds {
    filling: f32,
    inner: f32,
    middle: f32,
    outer: f32,
    crack: f32,
}

impl Thresholds {
    /// `d = points / max_wall_distance`, with the pack's exclusive
    /// `max_outer_wall_distance`; `crack_jitter` is a `[0, 1)` draw.
    fn new(points: i32, max_wall_distance: i32, base_crack_size: f32, crack_jitter: f32) -> Self {
        let d = points as f32 / max_wall_distance.max(1) as f32;
        let crack_extra = if points > 3 { d } else { 0.0 };
        Self {
            filling: FILLING,
            inner: 1.0 / (INNER_LAYER + d).sqrt(),
            middle: 1.0 / (MIDDLE_LAYER + d).sqrt(),
            outer: 1.0 / (OUTER_LAYER + d).sqrt(),
            crack: 1.0 / (crack_extra + crack_jitter * 0.5 + base_crack_size).sqrt(),
        }
    }
}

/// The three crack points (relative to the geode origin) for corner `index`
/// (0..4): a vertical line at heights 7, 5, 1 at `x` and/or `z` offset
/// `2 * points + 1`.
fn crack_offsets(index: i32, points: i32) -> [(i32, i32, i32); 3] {
    let c = points * 2 + 1;
    let (x, z) = match index {
        0 => (c, 0),
        1 => (0, c),
        2 => (c, c),
        _ => (0, 0),
    };
    [(x, 7, z), (x, 5, z), (x, 1, z)]
}

/// Fast inverse square root with one Newton step, as BDS evaluates shells.
#[inline]
fn fast_inv_sqrt(v: f32) -> f32 {
    let y = f32::from_bits(0x5f37_59df - (v.to_bits() >> 1));
    (v * -0.5 * y * y + 1.5) * y
}

#[inline]
fn dist_sq(a: BlockPos, b: BlockPos) -> f32 {
    let (dx, dy, dz) = (
        i64::from(a.x - b.x),
        i64::from(a.y - b.y),
        i64::from(a.z - b.z),
    );
    (dx * dx + dy * dy + dz * dz) as f32
}

/// Set unless the current block is protected; `true` when it changed.
#[inline]
fn safe_set<W: BlockWorld>(ctx: &mut Ctx<'_, W>, at: BlockPos, id: u32) -> bool {
    !bb::geode_cannot_replace(ctx.get(at)) && ctx.set(at, id)
}

#[cfg(test)]
mod tests {
    use super::super::super::rng::BedrockRandom;
    use super::super::super::world::NoiseSource;
    use super::*;

    struct NoNoise;
    impl NoiseSource for NoNoise {
        fn noise(&self, _x: f32, _z: f32) -> f32 {
            0.0
        }
    }

    /// A cube of blocks `centre + [-half, half)` on every axis.
    #[derive(Clone, PartialEq)]
    struct Cube {
        centre: BlockPos,
        half: i32,
        blocks: Vec<u32>,
        secondary: std::collections::HashMap<BlockPos, u32>,
        block_entities: Vec<crate::world::block_entity::BlockEntity>,
    }

    impl Cube {
        fn filled(half: i32, id: u32) -> Self {
            Self::filled_at(BlockPos::new(0, 0, 0), half, id)
        }

        fn filled_at(centre: BlockPos, half: i32, id: u32) -> Self {
            let side = (2 * half) as usize;
            Self {
                centre,
                half,
                blocks: vec![id; side * side * side],
                secondary: Default::default(),
                block_entities: Vec::new(),
            }
        }

        fn index(&self, p: BlockPos) -> Option<usize> {
            let side = 2 * self.half;
            let (x, y, z) = (
                p.x - self.centre.x + self.half,
                p.y - self.centre.y + self.half,
                p.z - self.centre.z + self.half,
            );
            let inside = (0..side).contains(&x) && (0..side).contains(&y) && (0..side).contains(&z);
            inside.then(|| ((y * side + z) * side + x) as usize)
        }

        fn count(&self, id: u32) -> usize {
            self.blocks.iter().filter(|&&b| b == id).count()
        }

        /// Blocks equal to `id` whose position satisfies `keep`.
        fn count_where(&self, id: u32, keep: impl Fn(BlockPos) -> bool) -> usize {
            let side = 2 * self.half;
            (0..self.blocks.len())
                .filter(|&i| self.blocks[i] == id)
                .map(|i| {
                    let i = i as i32;
                    BlockPos::new(
                        i % side - self.half + self.centre.x,
                        i / (side * side) - self.half + self.centre.y,
                        i / side % side - self.half + self.centre.z,
                    )
                })
                .filter(|&p| keep(p))
                .count()
        }
    }

    impl BlockWorld for Cube {
        fn min_y(&self) -> i32 {
            self.centre.y - self.half
        }
        fn max_y(&self) -> i32 {
            self.centre.y + self.half
        }
        fn get(&self, pos: BlockPos) -> u32 {
            self.index(pos).map_or(OUT_OF_BOUNDS, |i| self.blocks[i])
        }
        fn get_layer(&self, pos: BlockPos, layer: u8) -> u32 {
            match (self.index(pos), layer) {
                (Some(i), 0) => self.blocks[i],
                (Some(_), 1) => self.secondary.get(&pos).copied().unwrap_or(bb::AIR),
                _ => OUT_OF_BOUNDS,
            }
        }
        fn set(&mut self, pos: BlockPos, id: u32) -> bool {
            match self.index(pos) {
                Some(i) => {
                    self.blocks[i] = id;
                    self.secondary.remove(&pos);
                    true
                }
                None => false,
            }
        }
        fn set_layers(&mut self, pos: BlockPos, primary: u32, secondary: Option<u32>) -> bool {
            if !self.set(pos, primary) {
                return false;
            }
            if let Some(id) = secondary.filter(|&id| id != bb::AIR) {
                self.secondary.insert(pos, id);
            }
            true
        }
        fn set_block_entity(&mut self, entity: crate::world::block_entity::BlockEntity) -> bool {
            let position = entity.position();
            if self
                .index(BlockPos::new(position[0], position[1], position[2]))
                .is_none()
            {
                return false;
            }
            if let Some(existing) = self
                .block_entities
                .iter_mut()
                .find(|existing| existing.position() == position)
            {
                *existing = entity;
            } else {
                self.block_entities.push(entity);
            }
            true
        }
        fn biome(&self, _pos: BlockPos) -> u16 {
            0
        }
        fn heightmap(&mut self, _x: i32, _z: i32) -> i32 {
            self.half
        }
        fn above_top_solid(&mut self, _x: i32, _z: i32) -> i32 {
            self.half
        }
    }

    /// One "placement" block per face, so the face it grew in is visible.
    static PLACEMENTS: [[u32; 6]; 1] = [[
        bb::SAND,
        bb::RED_SAND,
        bb::GRAVEL,
        bb::CLAY,
        bb::ICE,
        bb::DIRT,
    ]];

    fn params() -> Params {
        Params {
            filler: bb::AIR,
            inner_layer: bb::CALCITE,
            alternate_inner_layer: bb::MAGMA,
            middle_layer: bb::TUFF,
            outer_layer: bb::OBSIDIAN,
            inner_placements: &PLACEMENTS,
            outer_wall_distance: IntRange { min: 4, max: 6 },
            distribution_points: IntRange { min: 3, max: 4 },
            point_offset: IntRange { min: 1, max: 2 },
            max_radius: 16,
            crack_point_offset: 2.0,
            generate_crack_chance: 0.0,
            base_crack_size: 2.0,
            noise_multiplier: 0.025,
            use_potential_placements_chance: 1.0,
            use_alternate_layer0_chance: 0.083,
            placements_require_layer0_alternate: false,
            invalid_blocks_threshold: 1,
        }
    }

    fn run(world: &mut Cube, params: &Params, seed: u32) -> Option<BlockPos> {
        run_at(world, params, seed, BlockPos::new(0, 0, 0))
    }

    fn run_at(world: &mut Cube, params: &Params, seed: u32, pos: BlockPos) -> Option<BlockPos> {
        let mut rng = BedrockRandom::new(seed);
        let mut ctx = Ctx {
            world,
            rng: &mut rng,
            noise: &NoNoise,
            seed: 0,
        };
        place(&mut ctx, pos, params)
    }

    #[test]
    fn thresholds_nest_from_the_core_outwards() {
        let t = Thresholds::new(4, 7, 2.0, 0.5);
        assert!(t.filling > t.inner && t.inner > t.middle && t.middle > t.outer);
        let d = 4.0f32 / 7.0;
        assert!((t.outer - 1.0 / (4.2f32 + d).sqrt()).abs() < 1e-6);
        assert!((t.crack - 1.0 / (2.25f32 + d).sqrt()).abs() < 1e-6);
        // Three points: no `d` in the crack threshold.
        assert!((Thresholds::new(3, 7, 2.0, 0.0).crack - 1.0 / 2.0f32.sqrt()).abs() < 1e-6);
        assert!((t.filling - 1.0 / 1.7f32.sqrt()).abs() < 1e-6);
    }

    #[test]
    fn crack_points_form_a_vertical_line() {
        assert_eq!(crack_offsets(0, 3), [(7, 7, 0), (7, 5, 0), (7, 1, 0)]);
        assert_eq!(crack_offsets(1, 4), [(0, 7, 9), (0, 5, 9), (0, 1, 9)]);
        assert_eq!(crack_offsets(2, 3), [(7, 7, 7), (7, 5, 7), (7, 1, 7)]);
        assert_eq!(crack_offsets(3, 3), [(0, 7, 0), (0, 5, 0), (0, 1, 0)]);
    }

    #[test]
    fn geode_has_layers_and_outward_facing_growth() {
        let mut world = Cube::filled(24, bb::STONE);
        assert!(run(&mut world, &params(), 7).is_some());
        for id in [bb::AIR, bb::TUFF, bb::OBSIDIAN] {
            assert!(world.count(id) > 0, "layer {id} missing");
        }
        assert!(world.count(bb::CALCITE) + world.count(bb::MAGMA) > 0);
        // Layers nest: measured from the core's centroid, the inner layer
        // lies outside the core, the middle layer outside it, the outer
        // layer outermost.
        let positions = |id: u32| {
            let mut out = Vec::new();
            for x in -24..24 {
                for y in -24..24 {
                    for z in -24..24 {
                        if world.get(BlockPos::new(x, y, z)) == id {
                            out.push((f64::from(x), f64::from(y), f64::from(z)));
                        }
                    }
                }
            }
            out
        };
        let core = positions(bb::AIR);
        let n = core.len() as f64;
        let centre = core.iter().fold((0.0, 0.0, 0.0), |a, p| {
            (a.0 + p.0 / n, a.1 + p.1 / n, a.2 + p.2 / n)
        });
        let mean_distance = |id: u32| {
            let ps = positions(id);
            let sum: f64 = ps
                .iter()
                .map(|p| {
                    ((p.0 - centre.0).powi(2) + (p.1 - centre.1).powi(2) + (p.2 - centre.2).powi(2))
                        .sqrt()
                })
                .sum();
            sum / ps.len() as f64
        };
        let (core_d, inner_d, middle_d, outer_d) = (
            mean_distance(bb::AIR),
            mean_distance(bb::CALCITE),
            mean_distance(bb::TUFF),
            mean_distance(bb::OBSIDIAN),
        );
        assert!(
            core_d < inner_d && inner_d < middle_d && middle_d < outer_d,
            "{core_d} {inner_d} {middle_d} {outer_d}"
        );
        let mut grown = 0;
        for (face, &(dx, dy, dz)) in FACES.iter().enumerate() {
            let id = PLACEMENTS[0][face];
            for x in -24..24 {
                for y in -24..24 {
                    for z in -24..24 {
                        if world.get(BlockPos::new(x, y, z)) != id {
                            continue;
                        }
                        grown += 1;
                        let support = world.get(BlockPos::new(x - dx, y - dy, z - dz));
                        assert!(
                            support == bb::CALCITE || support == bb::MAGMA,
                            "{id} at {x} {y} {z} not on the shell"
                        );
                    }
                }
            }
        }
        assert!(grown > 0);
    }

    #[test]
    fn geode_is_deterministic() {
        let mut a = Cube::filled(24, bb::STONE);
        let mut b = a.clone();
        run(&mut a, &params(), 11);
        run(&mut b, &params(), 11);
        assert!(a == b);
    }

    #[test]
    fn too_many_invalid_points_abort() {
        let mut world = Cube::filled(24, bb::AIR);
        let before = world.clone();
        assert_eq!(run(&mut world, &params(), 3), None);
        assert!(world == before);
    }

    /// Natural seed-12345 BDS 1.26.51 geodes, exported block by block: the
    /// feature random of each starts from its rule seed, and both must come
    /// out exactly. Layers here: outer obsidian (smooth basalt), middle tuff
    /// (calcite), inner calcite or magma (amethyst or budding amethyst).
    #[test]
    fn native_geodes_are_reproduced_block_for_block() {
        let vanilla = Params {
            generate_crack_chance: 0.95,
            use_potential_placements_chance: 0.35,
            placements_require_layer0_alternate: true,
            ..params()
        };
        // Chunk (-4,3), all neighbours exported; no crack.
        let pos = BlockPos::new(-64, -37, 48);
        let mut world = Cube::filled_at(pos.offset(5, 5, 5), 24, bb::STONE);
        assert!(run_at(&mut world, &vanilla, 3_502_308_776, pos).is_some());
        assert_eq!(world.count(bb::OBSIDIAN), 413);
        assert_eq!(world.count(bb::TUFF), 310);
        assert_eq!(world.count(bb::CALCITE) + world.count(bb::MAGMA), 239);
        assert_eq!(world.count(bb::MAGMA), 14);
        // Chunk (-1,1), cracked; only chunks (-2,1), (-1,1), (-1,2) exported.
        let pos = BlockPos::new(-16, -33, 16);
        let mut world = Cube::filled_at(pos.offset(5, 5, 5), 24, bb::STONE);
        assert!(run_at(&mut world, &vanilla, 4_121_427_388, pos).is_some());
        let exported = |p: BlockPos| matches!((p.x >> 4, p.z >> 4), (-2, 1) | (-1, 1) | (-1, 2));
        assert_eq!(world.count_where(bb::OBSIDIAN, exported), 294);
        assert_eq!(world.count_where(bb::TUFF, exported), 241);
        let inner =
            world.count_where(bb::CALCITE, exported) + world.count_where(bb::MAGMA, exported);
        assert_eq!(inner, 154);
        assert_eq!(world.count_where(bb::MAGMA, exported), 13);
    }
}
