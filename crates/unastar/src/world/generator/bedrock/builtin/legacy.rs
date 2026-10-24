//! `minecraft:legacy:*` built-ins: Bedrock's per-biome decoration from before
//! decoration became data driven. BDS still places them through feature rules
//! that run once per chunk (`iterations: 1`, x/z offset 0, y at the chunk
//! origin's heightmap), so each built-in decides by itself how often and where
//! it decorates the chunk, exactly like the classic per-biome decorators:
//!
//! - **tree selectors** roll a per-chunk tree count, scatter that many
//!   positions uniformly over the chunk (at the column's heightmap) and grow a
//!   tree type picked with the biome's vanilla weights. The trees themselves
//!   are the JSON tree features BDS ships for this purpose (`select_*`,
//!   `mega_*`, `jungle_bush_feature`, `swamp_tree_feature`, ...: no feature
//!   rule references them, only these selectors); they carry the
//!   fallen/vine/beehive/leaf-litter variants as data;
//! - **flower selectors** are placed by the `scatter_*_flower_feature` patches
//!   (64 or 96 tries each), so one call places one flower;
//! - **foliage** places the forest double flowers (lilac, rose bush, peony);
//!   swamp foliage is the swamp tree pass (see [`legacy_swamp_foliage_feature`]);
//! - **forest rock**, **springs** and **ice** are the classic small decorators.
//!
//! Counts and weights are the vanilla values shared by both editions where
//! BDS ships no number; BDS's exact random draw order is not observable from
//! the data, so these reproduce the statistics (statistical parity), not the
//! byte-exact layout.

use crate::world::generator::bedrock::generated::{self, bb, props};
use crate::world::generator::bedrock::rng::BedrockRandom;
use crate::world::generator::bedrock::surface::{BiomeData, Builder};
use crate::world::generator::bedrock::world::{BlockPos, BlockWorld, Ctx, Direction};

/// Minimum x/z corner of the chunk containing `pos` (rules pass the chunk
/// origin already; masking keeps the built-ins correct for any input).
#[inline]
fn chunk_origin(pos: BlockPos) -> BlockPos {
    BlockPos::new(pos.x & !15, pos.y, pos.z & !15)
}

/// Uniformly random column of the chunk at `origin`, at its heightmap (the
/// first air block above the ground, where trees and plants are rooted).
#[inline]
fn random_surface<W: BlockWorld>(ctx: &mut Ctx<'_, W>, origin: BlockPos) -> BlockPos {
    let x = origin.x + ctx.rng.next_int(16);
    let z = origin.z + ctx.rng.next_int(16);
    let y = ctx.heightmap(x, z);
    BlockPos::new(x, y, z)
}

// ---------------------------------------------------------------------------
// Trees
// ---------------------------------------------------------------------------

/// Tree shapes the selectors grow; each maps to one shipped JSON feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tree {
    /// `select_oak_tree_feature`: oak, rarely fallen or vine-covered, beehive
    /// chance by biome (all in data).
    Oak,
    /// `oak_tree_feature`: plain oak (savanna's minority tree).
    OakPlain,
    /// `select_oak_tree_with_leaf_litter_feature`.
    OakLitter,
    /// `fancy_oak_tree_with_optional_beehive_feature`.
    FancyOak,
    /// `fancy_oak_tree_feature`: no beehive (jungles, windswept hills).
    FancyOakPlain,
    /// `fancy_oak_tree_with_leaf_litter_and_optional_beehive`.
    FancyOakLitter,
    /// `fancy_oak_tree_with_beehive_feature`: always a beehive (meadow).
    FancyOakBees,
    /// `select_birch_tree_feature`.
    Birch,
    /// `select_birch_tree_with_leaf_litter_feature`.
    BirchLitter,
    /// `select_super_birch_tree_feature`: tall birch.
    SuperBirch,
    /// `super_birch_tree_with_beehive_feature`: always a beehive (meadow).
    SuperBirchBees,
    /// `select_spruce_tree_feature`.
    Spruce,
    /// `pine_tree_feature`.
    Pine,
    /// `mega_spruce_tree_feature`: 2x2 trunk, tall crown.
    MegaSpruce,
    /// `mega_pine_tree_feature`: 2x2 trunk, short crown.
    MegaPine,
    /// `select_jungle_tree_feature`: jungle tree with vines and cocoa.
    Jungle,
    /// `jungle_bush_feature`: one jungle log under an oak-leaf mound.
    JungleBush,
    /// `mega_jungle_tree_feature`.
    MegaJungle,
    /// `savanna_tree_feature`: acacia.
    Acacia,
    /// `swamp_tree_feature`: oak with hanging vines, may stand in shallow water.
    Swamp,
}

/// Grow `tree` rooted at `pos` (the air block above the ground).
fn grow<W: BlockWorld>(ctx: &mut Ctx<'_, W>, tree: Tree, pos: BlockPos) -> Option<BlockPos> {
    match tree {
        Tree::Oak => generated::f_select_oak_tree_feature(ctx, pos),
        Tree::OakPlain => generated::f_oak_tree_feature(ctx, pos),
        Tree::OakLitter => generated::f_select_oak_tree_with_leaf_litter_feature(ctx, pos),
        Tree::FancyOak => generated::f_fancy_oak_tree_with_optional_beehive_feature(ctx, pos),
        Tree::FancyOakPlain => generated::f_fancy_oak_tree_feature(ctx, pos),
        Tree::FancyOakLitter => generated::f_fancy_oak_tree_with_leaf_litter_and_optional_beehive(ctx, pos),
        Tree::FancyOakBees => generated::f_fancy_oak_tree_with_beehive_feature(ctx, pos),
        Tree::Birch => generated::f_select_birch_tree_feature(ctx, pos),
        Tree::BirchLitter => generated::f_select_birch_tree_with_leaf_litter_feature(ctx, pos),
        Tree::SuperBirch => generated::f_select_super_birch_tree_feature(ctx, pos),
        Tree::SuperBirchBees => generated::f_super_birch_tree_with_beehive_feature(ctx, pos),
        Tree::Spruce => generated::f_select_spruce_tree_feature(ctx, pos),
        Tree::Pine => generated::f_pine_tree_feature(ctx, pos),
        Tree::MegaSpruce => generated::f_mega_spruce_tree_feature(ctx, pos),
        Tree::MegaPine => generated::f_mega_pine_tree_feature(ctx, pos),
        Tree::Jungle => generated::f_select_jungle_tree_feature(ctx, pos),
        Tree::JungleBush => generated::f_jungle_bush_feature(ctx, pos),
        Tree::MegaJungle => generated::f_mega_jungle_tree_feature(ctx, pos),
        Tree::Acacia => generated::f_savanna_tree_feature(ctx, pos),
        Tree::Swamp => generated::f_swamp_tree_feature(ctx, pos),
    }
}

/// Trees per chunk: `base`, plus one more with probability `extra` (the
/// classic decorator's "extra tree chance").
#[derive(Debug, Clone, Copy, PartialEq)]
struct Density {
    base: i32,
    extra: f32,
}

impl Density {
    const fn new(base: i32, extra: f32) -> Self {
        Self { base, extra }
    }

    fn roll(self, rng: &mut BedrockRandom) -> i32 {
        self.base + i32::from(rng.probability(self.extra))
    }
}

// Vanilla per-chunk tree densities. Every biome gets the classic 10% extra
// tree except plains (5%) and meadow (a single tree once every 100 chunks).
const FOREST: Density = Density::new(10, 0.1);
const FLOWER_FOREST: Density = Density::new(6, 0.1);
const BIRCH_FOREST: Density = Density::new(10, 0.1);
const TAIGA: Density = Density::new(10, 0.1);
const WINDSWEPT_FOREST: Density = Density::new(3, 0.1);
const JUNGLE: Density = Density::new(50, 0.1);
const JUNGLE_EDGE: Density = Density::new(2, 0.1);
const BAMBOO_JUNGLE: Density = Density::new(30, 0.1);
const SAVANNA: Density = Density::new(1, 0.1);
const SAVANNA_MUTATED: Density = Density::new(2, 0.1);
const PLAINS: Density = Density::new(0, 0.05);
const SNOWY: Density = Density::new(0, 0.1);
const WOODED_BADLANDS: Density = Density::new(5, 0.1);
const MEADOW: Density = Density::new(0, 0.01);
const SWAMP: Density = Density::new(2, 0.1);

/// Plant `density` trees picked by `select` at random columns of the chunk
/// containing `pos`; the position of the last tree grown, if any.
fn plant_trees<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
    density: Density,
    select: fn(&mut BedrockRandom) -> Tree,
) -> Option<BlockPos> {
    let origin = chunk_origin(pos);
    let count = density.roll(ctx.rng);
    let mut last = None;
    for _ in 0..count {
        let at = random_surface(ctx, origin);
        let tree = select(ctx.rng);
        if let Some(grown) = grow(ctx, tree, at) {
            last = Some(grown);
        }
    }
    last
}

/// Forest: birch 1/5, otherwise fancy oak 1/10 else oak; all with leaf litter
/// (forests carry leaf litter, birch forests do not).
fn forest_tree(rng: &mut BedrockRandom) -> Tree {
    if rng.next_int(5) == 0 {
        Tree::BirchLitter
    } else if rng.next_int(10) == 0 {
        Tree::FancyOakLitter
    } else {
        Tree::OakLitter
    }
}

/// Flower forest: the forest weights without leaf litter.
fn flower_forest_tree(rng: &mut BedrockRandom) -> Tree {
    if rng.next_int(5) == 0 {
        Tree::Birch
    } else if rng.next_int(10) == 0 {
        Tree::FancyOak
    } else {
        Tree::Oak
    }
}

/// Birch forest: only birch.
fn birch_tree(_: &mut BedrockRandom) -> Tree {
    Tree::Birch
}

/// Tall birch forest: half super birch, half birch.
fn birch_mutated_tree(rng: &mut BedrockRandom) -> Tree {
    if rng.next_int(2) == 0 { Tree::SuperBirch } else { Tree::Birch }
}

/// Windswept (extreme hills) forest: spruce 2/3, otherwise fancy oak 1/10 else oak.
fn windswept_tree(rng: &mut BedrockRandom) -> Tree {
    if rng.next_int(3) > 0 {
        Tree::Spruce
    } else if rng.next_int(10) == 0 {
        Tree::FancyOakPlain
    } else {
        Tree::Oak
    }
}

/// Taiga: pine 1/3 else spruce.
fn taiga_tree(rng: &mut BedrockRandom) -> Tree {
    if rng.next_int(3) == 0 { Tree::Pine } else { Tree::Spruce }
}

/// Old growth pine taiga (`mega_taiga`): a giant tree 1/3 of the time, of
/// which 12/13 giant pines and 1/13 giant spruces; otherwise the taiga mix.
fn mega_taiga_tree(rng: &mut BedrockRandom) -> Tree {
    if rng.next_int(3) == 0 {
        if rng.next_int(13) == 0 { Tree::MegaSpruce } else { Tree::MegaPine }
    } else {
        taiga_tree(rng)
    }
}

/// Old growth spruce taiga (`redwood_taiga_mutated`): giant spruce 1/3,
/// otherwise the taiga mix.
fn redwood_mutated_tree(rng: &mut BedrockRandom) -> Tree {
    if rng.next_int(3) == 0 { Tree::MegaSpruce } else { taiga_tree(rng) }
}

/// Jungle: fancy oak 1/10, otherwise bush 1/2, otherwise (not at the edge)
/// mega jungle 1/3, else a jungle tree.
fn jungle_tree_kind(rng: &mut BedrockRandom, edge: bool) -> Tree {
    if rng.next_int(10) == 0 {
        Tree::FancyOakPlain
    } else if rng.next_int(2) == 0 {
        Tree::JungleBush
    } else if !edge && rng.next_int(3) == 0 {
        Tree::MegaJungle
    } else {
        Tree::Jungle
    }
}

fn jungle_tree(rng: &mut BedrockRandom) -> Tree {
    jungle_tree_kind(rng, false)
}

fn jungle_edge_tree(rng: &mut BedrockRandom) -> Tree {
    jungle_tree_kind(rng, true)
}

/// Bamboo jungle: sequential chances fancy oak 5%, bush 15%, mega jungle 70%,
/// else a jungle tree. (The other edition's selector falls through to a grass
/// patch instead; BDS places bamboo-jungle grass through its own rule, so the
/// fall-through here is a tree. Exact BDS split unknown; statistical parity.)
fn bamboo_jungle_tree(rng: &mut BedrockRandom) -> Tree {
    if rng.probability(0.05) {
        Tree::FancyOakPlain
    } else if rng.probability(0.15) {
        Tree::JungleBush
    } else if rng.probability(0.7) {
        Tree::MegaJungle
    } else {
        Tree::Jungle
    }
}

/// Savanna: acacia 4/5 else a plain oak (savannas have no fallen trees or bees).
fn savanna_tree(rng: &mut BedrockRandom) -> Tree {
    if rng.next_int(5) > 0 { Tree::Acacia } else { Tree::OakPlain }
}

/// Plains: fancy oak 1/3 else oak (beehive chance from the data's
/// `optional_beehive_feature`).
fn plains_tree(rng: &mut BedrockRandom) -> Tree {
    if rng.next_int(3) == 0 { Tree::FancyOak } else { Tree::Oak }
}

/// Meadow: the rare meadow tree always carries a beehive (the data's optional
/// beehive chance is 0 in meadows, so the `*_with_beehive_feature` variants
/// are used); half fancy oak, half super birch.
fn meadow_tree(rng: &mut BedrockRandom) -> Tree {
    if rng.next_int(2) == 0 { Tree::FancyOakBees } else { Tree::SuperBirchBees }
}

fn spruce_tree(_: &mut BedrockRandom) -> Tree {
    Tree::Spruce
}

fn badlands_tree(_: &mut BedrockRandom) -> Tree {
    Tree::OakLitter
}

fn swamp_tree(_: &mut BedrockRandom) -> Tree {
    Tree::Swamp
}

/// Bamboo jungle trees (see [`bamboo_jungle_tree`]), 30 per chunk.
pub fn legacy_bamboo_jungle_tree_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    plant_trees(ctx, pos, BAMBOO_JUNGLE, bamboo_jungle_tree)
}

/// Tall birch forest trees: 10 per chunk, half super birch.
pub fn legacy_birch_forest_mutated_tree_feature<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
) -> Option<BlockPos> {
    plant_trees(ctx, pos, BIRCH_FOREST, birch_mutated_tree)
}

/// Birch forest trees: 10 birches per chunk.
pub fn legacy_birch_forest_tree_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    plant_trees(ctx, pos, BIRCH_FOREST, birch_tree)
}

/// Windswept forest (extreme hills with trees and its edge): 3 per chunk,
/// mostly spruce.
pub fn legacy_extreme_hills_plus_trees_tree_feature<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
) -> Option<BlockPos> {
    plant_trees(ctx, pos, WINDSWEPT_FOREST, windswept_tree)
}

/// Flower forest trees: 6 per chunk, forest weights.
pub fn legacy_flower_forest_tree_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    plant_trees(ctx, pos, FLOWER_FOREST, flower_forest_tree)
}

/// Forest trees: 10 per chunk, birch 1/5, fancy oak 1/10 of the rest, with
/// leaf litter.
pub fn legacy_forest_tree_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    plant_trees(ctx, pos, FOREST, forest_tree)
}

/// Snowy plains / ice mountains: one spruce in one chunk out of ten.
pub fn legacy_ice_tree_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    plant_trees(ctx, pos, SNOWY, spruce_tree)
}

/// Sparse jungle (jungle edge): 2 per chunk, no mega jungle trees.
pub fn legacy_jungle_edge_tree_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    plant_trees(ctx, pos, JUNGLE_EDGE, jungle_edge_tree)
}

/// Jungle: 50 per chunk (fancy oak, bushes, mega and regular jungle trees).
pub fn legacy_jungle_tree_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    plant_trees(ctx, pos, JUNGLE, jungle_tree)
}

/// Meadow: one tree with a beehive in one chunk out of a hundred.
pub fn legacy_meadow_trees_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    plant_trees(ctx, pos, MEADOW, meadow_tree)
}

/// Old growth pine taiga (also its mutated hills, per the BDS rules): 10 per
/// chunk, a third giant.
pub fn legacy_mega_taiga_tree_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    plant_trees(ctx, pos, TAIGA, mega_taiga_tree)
}

/// Wooded badlands (stone mesa plateau): 5 oaks per chunk with leaf litter.
pub fn legacy_mesa_plateau_stone_tree_feature<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
) -> Option<BlockPos> {
    plant_trees(ctx, pos, WOODED_BADLANDS, badlands_tree)
}

/// Plains: a tree in one chunk out of twenty, fancy oak 1/3.
pub fn legacy_plains_tree_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    plant_trees(ctx, pos, PLAINS, plains_tree)
}

/// Old growth spruce taiga: 10 per chunk, giant spruce 1/3.
pub fn legacy_redwood_taiga_mutated_tree_feature<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
) -> Option<BlockPos> {
    plant_trees(ctx, pos, TAIGA, redwood_mutated_tree)
}

/// Windswept savanna: 2 per chunk, acacia 4/5.
pub fn legacy_savanna_mutated_tree_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    plant_trees(ctx, pos, SAVANNA_MUTATED, savanna_tree)
}

/// Savanna and savanna plateau: 1 per chunk, acacia 4/5.
pub fn legacy_savanna_tree_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    plant_trees(ctx, pos, SAVANNA, savanna_tree)
}

/// Taiga (every non-giant taiga, snowy ones included): 10 per chunk, pine 1/3.
pub fn legacy_taiga_tree_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    plant_trees(ctx, pos, TAIGA, taiga_tree)
}

/// Swamp foliage: the swamp's trees, 2 per chunk (`swamp_tree_feature`: oak
/// with vines that may root up to 2 blocks under water). BDS has no other rule
/// that places swamp trees and nothing else references `swamp_tree_feature`;
/// swamp grass, flowers, lilies, reeds and mushrooms all have their own rules.
pub fn legacy_swamp_foliage_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    plant_trees(ctx, pos, SWAMP, swamp_tree)
}

// ---------------------------------------------------------------------------
// Flowers (one per call: the `scatter_*_flower_feature` patches iterate)
// ---------------------------------------------------------------------------

const TULIPS: [u32; 4] = [bb::ORANGE_TULIP, bb::RED_TULIP, bb::PINK_TULIP, bb::WHITE_TULIP];
const PLAINS_COMMON: [u32; 4] = [bb::POPPY, bb::AZURE_BLUET, bb::OXEYE_DAISY, bb::CORNFLOWER];
/// Flower forest flowers in noise order (blue orchid is swamp-only).
const FLOWER_FOREST_FLOWERS: [u32; 11] = [
    bb::DANDELION,
    bb::POPPY,
    bb::ALLIUM,
    bb::AZURE_BLUET,
    bb::RED_TULIP,
    bb::ORANGE_TULIP,
    bb::WHITE_TULIP,
    bb::PINK_TULIP,
    bb::OXEYE_DAISY,
    bb::CORNFLOWER,
    bb::LILY_OF_THE_VALLEY,
];

/// Whether a small plant can be put at `pos`: air on a dirt-family block
/// (grass, dirt, podzol, ... = the `dirt` tag trees and flowers root in).
#[inline]
fn plant_fits<W: BlockWorld>(ctx: &Ctx<'_, W>, pos: BlockPos) -> bool {
    bb::air(ctx.get(pos)) && bb::dirt_like(ctx.get(pos.below()))
}

/// Put `flower` at `pos` if it fits.
fn place_flower<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, flower: u32) -> Option<BlockPos> {
    ctx.set(pos, flower).then_some(pos)
}

/// Plains flower for grass-colour noise `noise`: tulips in the low-noise
/// patches (< -0.8, the same threshold the plains rules use), otherwise
/// poppy/azure bluet/oxeye daisy/cornflower 2/3 and dandelion 1/3.
fn plains_flower(noise: f32, rng: &mut BedrockRandom) -> u32 {
    if noise < -0.8 {
        TULIPS[rng.next_int(4) as usize]
    } else if rng.next_int(3) > 0 {
        PLAINS_COMMON[rng.next_int(4) as usize]
    } else {
        bb::DANDELION
    }
}

/// Flower forest flower: the noise sweeps through the flower list, giving the
/// biome its single-species bands.
fn flower_forest_flower(noise: f32) -> u32 {
    let t = ((1.0 + noise) / 2.0).clamp(0.0, 0.9999);
    FLOWER_FOREST_FLOWERS[(t * FLOWER_FOREST_FLOWERS.len() as f32) as usize]
}

/// Generic overworld flower: dandelion 2/3, poppy 1/3.
fn overworld_flower(rng: &mut BedrockRandom) -> u32 {
    if rng.next_int(3) > 0 { bb::DANDELION } else { bb::POPPY }
}

/// One flower forest flower at `pos`, chosen by `query.noise` at 1/48 scale.
///
/// `ctx.noise` is the world noise behind Molang `query.noise` (about
/// [-1, 1]); the scale is the vanilla flower forest band scale.
pub fn legacy_flower_forest_flower_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    if !plant_fits(ctx, pos) {
        return None;
    }
    let noise = ctx.noise(pos.x as f32 / 48.0, pos.z as f32 / 48.0);
    place_flower(ctx, pos, flower_forest_flower(noise))
}

/// One generic flower (dandelion or poppy) at `pos`.
pub fn legacy_overworld_flower_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    if !plant_fits(ctx, pos) {
        return None;
    }
    let flower = overworld_flower(ctx.rng);
    place_flower(ctx, pos, flower)
}

/// One plains flower at `pos`; the noise is `query.noise(x / 200, z / 200)`,
/// the same field the plains rules threshold at -0.8 for their flower and
/// grass counts, so tulip patches coincide with the flower-rich chunks.
pub fn legacy_plains_flower_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    if !plant_fits(ctx, pos) {
        return None;
    }
    let noise = ctx.noise(pos.x as f32 / 200.0, pos.z as f32 / 200.0);
    let flower = plains_flower(noise, ctx.rng);
    place_flower(ctx, pos, flower)
}

/// One blue orchid at `pos`.
pub fn legacy_swamp_flower_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    if !plant_fits(ctx, pos) {
        return None;
    }
    place_flower(ctx, pos, bb::BLUE_ORCHID)
}

// ---------------------------------------------------------------------------
// Forest foliage: double flowers
// ---------------------------------------------------------------------------

/// Lilac, rose bush, peony as (lower, upper) halves.
const DOUBLE_FLOWERS: [(u32, u32); 3] = [
    (bb::LILAC_LOWER, bb::LILAC_UPPER),
    (bb::ROSE_BUSH_LOWER, bb::ROSE_BUSH_UPPER),
    (bb::PEONY_LOWER, bb::PEONY_UPPER),
];

/// Number of double-flower patches in a chunk: `next_int(5) - 3 + bonus`
/// (negative = none). Forests use bonus 0 (a patch in one chunk out of five),
/// flower forests bonus 2 (0 to 3 patches, 1.2 on average).
fn double_flower_patches(rng: &mut BedrockRandom, bonus: i32) -> i32 {
    (rng.next_int(5) - 3 + bonus).max(0)
}

/// Vanilla double plant patch: 64 tries spread +-7 horizontally and +-3
/// vertically (difference of two uniform draws, peaking at the centre); each
/// try needs two air blocks on a dirt-family block. The upper half's air test
/// also keeps the plant below the build limit (reads above it are
/// out-of-bounds, never air).
fn double_plant_patch<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    center: BlockPos,
    lower: u32,
    upper: u32,
) -> Option<BlockPos> {
    let mut placed = None;
    for _ in 0..64 {
        let dx = ctx.rng.next_int(8) - ctx.rng.next_int(8);
        let dy = ctx.rng.next_int(4) - ctx.rng.next_int(4);
        let dz = ctx.rng.next_int(8) - ctx.rng.next_int(8);
        let at = center.offset(dx, dy, dz);
        if plant_fits(ctx, at) && bb::air(ctx.get(at.above())) && ctx.set(at, lower) {
            ctx.set(at.above(), upper);
            placed = Some(at);
        }
    }
    placed
}

/// Forest double flowers: each patch picks one species, then tries up to 5
/// random columns of the chunk until a patch places something.
fn forest_double_flowers<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, bonus: i32) -> Option<BlockPos> {
    let origin = chunk_origin(pos);
    let patches = double_flower_patches(ctx.rng, bonus);
    let mut last = None;
    for _ in 0..patches {
        let (lower, upper) = DOUBLE_FLOWERS[ctx.rng.next_int(3) as usize];
        for _ in 0..5 {
            let at = random_surface(ctx, origin);
            if let Some(placed) = double_plant_patch(ctx, at, lower, upper) {
                last = Some(placed);
                break;
            }
        }
    }
    last
}

/// Forest foliage (forests and birch forests): lilac/rose bush/peony patches,
/// one in one chunk out of five.
pub fn legacy_forest_foliage_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    forest_double_flowers(ctx, pos, 0)
}

/// Flower forest foliage: lilac/rose bush/peony patches, 0 to 3 per chunk.
pub fn legacy_flower_forest_foliage_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    forest_double_flowers(ctx, pos, 2)
}

// ---------------------------------------------------------------------------
// Forest rock
// ---------------------------------------------------------------------------

/// Old growth pine taiga boulders: 0 to 2 per chunk at random columns.
pub fn legacy_forest_rock_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let origin = chunk_origin(pos);
    let rocks = ctx.rng.next_int(3);
    let mut last = None;
    for _ in 0..rocks {
        let at = random_surface(ctx, origin);
        if let Some(rock) = forest_rock(ctx, at) {
            last = Some(rock);
        }
    }
    last
}

/// Ground a forest rock may rest on.
#[inline]
fn rock_ground(id: u32) -> bool {
    bb::dirt_like(id) || bb::stone_like(id)
}

/// One mossy cobblestone boulder: sink from `pos` until the block below is
/// dirt or stone (giving up 3 blocks above the world bottom), then stamp
/// three small ellipsoid blobs (half-axes 0 or 1 each, radius
/// `(a + b + c) * 0.333 + 0.5`), each next blob shifted by -1/0 on x and z
/// and 0/-1 on y.
fn forest_rock<W: BlockWorld>(ctx: &mut Ctx<'_, W>, mut pos: BlockPos) -> Option<BlockPos> {
    let floor = ctx.world.min_y() + 3;
    loop {
        if pos.y <= floor {
            return None;
        }
        if rock_ground(ctx.get(pos.below())) {
            break;
        }
        pos = pos.below();
    }
    let start = pos;
    for _ in 0..3 {
        let ax = ctx.rng.next_int(2);
        let ay = ctx.rng.next_int(2);
        let az = ctx.rng.next_int(2);
        let radius = (ax + ay + az) as f32 * 0.333 + 0.5;
        let radius_sq = radius * radius;
        for dx in -ax..=ax {
            for dy in -ay..=ay {
                for dz in -az..=az {
                    if (dx * dx + dy * dy + dz * dz) as f32 <= radius_sq {
                        ctx.set(pos.offset(dx, dy, dz), bb::MOSSY_COBBLESTONE);
                    }
                }
            }
        }
        let sx = ctx.rng.next_int(2) - 1;
        let sy = -ctx.rng.next_int(2);
        let sz = ctx.rng.next_int(2) - 1;
        pos = pos.offset(sx, sy, sz);
    }
    Some(start)
}

// ---------------------------------------------------------------------------
// Springs
// ---------------------------------------------------------------------------

/// Water spring attempts per chunk.
const WATER_SPRING_ATTEMPTS: i32 = 25;
/// Highest y of a water spring attempt (inclusive).
const WATER_SPRING_MAX_Y: i32 = 192;
/// Lava spring attempts per chunk.
const LAVA_SPRING_ATTEMPTS: i32 = 20;
/// Lava springs stay this far below the build limit, and the bottom bias's
/// inner range.
const LAVA_SPRING_TOP_MARGIN: i32 = 8;

/// Height strongly biased toward `min`: three nested uniform draws in
/// `[min, max]` (`inner` keeps the first draw clear of the bottom).
fn very_biased_to_bottom(rng: &mut BedrockRandom, min: i32, max: i32, inner: i32) -> i32 {
    let upper = rng.next_int_inclusive(min + inner, max);
    let middle = rng.next_int_inclusive(min, upper - 1);
    rng.next_int_inclusive(min, middle - 1 + inner)
}

/// Single-block springs in cave walls: 25 water attempts (uniform from the
/// world bottom to y 192) and 20 lava attempts (strongly biased to the bottom)
/// at random columns of the chunk. Counts and heights are the vanilla
/// post-1.18 values for the -64..320 world (BDS ships none; statistical
/// parity). Only the source block is placed; the fluid flows once the chunk
/// ticks, as in vanilla.
pub fn legacy_springs_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let origin = chunk_origin(pos);
    let (min_y, max_y) = (ctx.world.min_y(), ctx.world.max_y());
    let mut last = None;
    for _ in 0..WATER_SPRING_ATTEMPTS {
        let x = origin.x + ctx.rng.next_int(16);
        let z = origin.z + ctx.rng.next_int(16);
        let y = ctx.rng.next_int_inclusive(min_y, WATER_SPRING_MAX_Y.min(max_y - 1));
        if let Some(placed) = spring(ctx, BlockPos::new(x, y, z), bb::WATER, bb::water_spring_host) {
            last = Some(placed);
        }
    }
    for _ in 0..LAVA_SPRING_ATTEMPTS {
        let x = origin.x + ctx.rng.next_int(16);
        let z = origin.z + ctx.rng.next_int(16);
        let y = very_biased_to_bottom(ctx.rng, min_y, max_y - 1 - LAVA_SPRING_TOP_MARGIN, LAVA_SPRING_TOP_MARGIN);
        if let Some(placed) = spring(ctx, BlockPos::new(x, y, z), bb::LAVA, bb::lava_spring_host) {
            last = Some(placed);
        }
    }
    last
}

/// Place `fluid` at `pos` if it is a wall spot opening onto exactly one side:
/// host rock above and below, `pos` itself air or host rock, and of the four
/// horizontal neighbours exactly three host rock and one air.
fn spring<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, fluid: u32, host: fn(u32) -> bool) -> Option<BlockPos> {
    if !host(ctx.get(pos.above())) || !host(ctx.get(pos.below())) {
        return None;
    }
    let here = ctx.get(pos);
    if !bb::air(here) && !host(here) {
        return None;
    }
    let (mut rock, mut open) = (0, 0);
    for dir in Direction::ALL {
        let side = ctx.get(pos.side(dir));
        if host(side) {
            rock += 1;
        } else if bb::air(side) {
            open += 1;
        }
    }
    (rock == 3 && open == 1 && ctx.set(pos, fluid)).then_some(pos)
}

// ---------------------------------------------------------------------------
// Ice
// ---------------------------------------------------------------------------

/// Temperature below which water freezes and snow settles.
const FREEZE_TEMPERATURE: f32 = 0.15;
/// Height above which temperature drops with altitude.
const COOLING_START_Y: i32 = 80;

/// Frozen ocean surface: for every column of the chunk, freeze the top water
/// source to ice and lay one snow layer on an exposed top that can hold snow,
/// wherever the column is cold (see [`is_cold`]). Snow does not settle on ice,
/// so frozen sea stays bare, as in vanilla.
///
/// Columns whose biome uses the frozen ocean surface builder keep their water
/// as the surface pass left it: that builder already froze the sea with its
/// open-water patches (and leaves deep frozen oceans open except for
/// icebergs), and refreezing here would close those holes. Those columns only
/// get snow.
pub fn legacy_ice_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let origin = chunk_origin(pos);
    let mut last = None;
    for dz in 0..16 {
        for dx in 0..16 {
            if let Some(changed) = freeze_column(ctx, origin.x + dx, origin.z + dz) {
                last = Some(changed);
            }
        }
    }
    last
}

/// Freeze/snow one column; the topmost position changed, if any.
fn freeze_column<W: BlockWorld>(ctx: &mut Ctx<'_, W>, x: i32, z: i32) -> Option<BlockPos> {
    // The heightmap skips liquids: climb through the water to the first block
    // above the topmost non-air block.
    let mut top = BlockPos::new(x, ctx.heightmap(x, z), z);
    while ctx.in_height(top.y) && props::is_liquid(ctx.get(top)) {
        top = top.above();
    }
    let surface = top.below();
    let biome = generated::biome(ctx.biome(surface))?;
    let sea_frozen_by_surface = matches!(biome.surface, Some(Builder::FrozenOcean(_)));
    let mut changed = None;
    let mut ground = ctx.get(surface);
    if !sea_frozen_by_surface && ground == bb::WATER && is_cold(ctx, biome, surface) && ctx.set(surface, bb::ICE) {
        ground = bb::ICE;
        changed = Some(surface);
    }
    if ctx.in_height(top.y)
        && bb::air(ctx.get(top))
        && holds_snow(ground)
        && is_cold(ctx, biome, top)
        && ctx.set(top, bb::SNOW_LAYER)
    {
        changed = Some(top);
    }
    changed
}

/// Snow settles on full solid blocks and leaves, never on ice.
#[inline]
fn holds_snow(ground: u32) -> bool {
    !bb::ice(ground) && (props::is_solid(ground) || bb::leaves(ground))
}

/// Whether water freezes / snow settles at `pos` in `biome`: the biome
/// temperature, lowered above y 80 by 0.05 per 40 blocks (the altitude is
/// jittered by `query.noise` at 1/8 scale, 8 blocks of amplitude, so the snow
/// line is ragged), must be below 0.15. BDS's exact jitter noise is not
/// observable from the data; statistical parity.
fn is_cold<W: BlockWorld>(ctx: &Ctx<'_, W>, biome: &BiomeData, pos: BlockPos) -> bool {
    let mut temperature = biome.temperature;
    if pos.y > COOLING_START_Y {
        let jitter = ctx.noise(pos.x as f32 / 8.0, pos.z as f32 / 8.0) * 8.0;
        temperature -= (jitter + (pos.y - COOLING_START_Y) as f32) * 0.05 / 40.0;
    }
    temperature < FREEZE_TEMPERATURE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::generator::bedrock::world::NoiseSource;

    const DRAWS: usize = 200_000;

    /// Share of `DRAWS` selections equal to each listed tree.
    fn shares(select: fn(&mut BedrockRandom) -> Tree, trees: &[Tree]) -> Vec<f64> {
        let mut rng = BedrockRandom::new(0x5eed);
        let mut counts = vec![0usize; trees.len()];
        for _ in 0..DRAWS {
            let tree = select(&mut rng);
            let i = trees.iter().position(|t| *t == tree).unwrap_or_else(|| panic!("unexpected {tree:?}"));
            counts[i] += 1;
        }
        counts.into_iter().map(|c| c as f64 / DRAWS as f64).collect()
    }

    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 0.006, "share {actual} != {expected}");
    }

    #[test]
    fn forest_weights() {
        let s = shares(forest_tree, &[Tree::BirchLitter, Tree::FancyOakLitter, Tree::OakLitter]);
        close(s[0], 0.2);
        close(s[1], 0.08);
        close(s[2], 0.72);
    }

    #[test]
    fn jungle_weights_and_edge_has_no_mega_trees() {
        let all = [Tree::FancyOakPlain, Tree::JungleBush, Tree::MegaJungle, Tree::Jungle];
        let s = shares(jungle_tree, &all);
        close(s[0], 0.1);
        close(s[1], 0.45);
        close(s[2], 0.15);
        close(s[3], 0.3);
        let edge = shares(jungle_edge_tree, &all);
        assert_eq!(edge[2], 0.0);
        close(edge[3], 0.45);
    }

    #[test]
    fn taiga_weights() {
        let s = shares(taiga_tree, &[Tree::Pine, Tree::Spruce]);
        close(s[0], 1.0 / 3.0);
        let mega = shares(mega_taiga_tree, &[Tree::MegaPine, Tree::MegaSpruce, Tree::Pine, Tree::Spruce]);
        close(mega[0], 12.0 / 39.0);
        close(mega[1], 1.0 / 39.0);
        close(mega[2], 2.0 / 9.0);
        let redwood = shares(redwood_mutated_tree, &[Tree::MegaSpruce, Tree::Pine, Tree::Spruce]);
        close(redwood[0], 1.0 / 3.0);
        close(redwood[1], 2.0 / 9.0);
    }

    #[test]
    fn savanna_windswept_bamboo_weights() {
        let s = shares(savanna_tree, &[Tree::Acacia, Tree::OakPlain]);
        close(s[0], 0.8);
        let w = shares(windswept_tree, &[Tree::Spruce, Tree::FancyOakPlain, Tree::Oak]);
        close(w[0], 2.0 / 3.0);
        close(w[1], 1.0 / 30.0);
        let b = shares(bamboo_jungle_tree, &[Tree::FancyOakPlain, Tree::JungleBush, Tree::MegaJungle, Tree::Jungle]);
        close(b[0], 0.05);
        close(b[1], 0.95 * 0.15);
        close(b[2], 0.95 * 0.85 * 0.7);
    }

    #[test]
    fn density_adds_at_most_one_tree() {
        let mut rng = BedrockRandom::new(7);
        let mut extra = 0;
        for _ in 0..DRAWS {
            let n = PLAINS.roll(&mut rng);
            assert!(n == 0 || n == 1);
            extra += n as usize;
        }
        close(extra as f64 / DRAWS as f64, 0.05);
        assert!((0..1000).all(|_| matches!(JUNGLE.roll(&mut rng), 50 | 51)));
    }

    #[test]
    fn plains_flowers_follow_noise() {
        let mut rng = BedrockRandom::new(1);
        for _ in 0..1000 {
            assert!(TULIPS.contains(&plains_flower(-0.9, &mut rng)));
        }
        let dandelions = (0..DRAWS).filter(|_| plains_flower(0.0, &mut rng) == bb::DANDELION).count();
        close(dandelions as f64 / DRAWS as f64, 1.0 / 3.0);
        assert!((0..1000).all(|_| {
            let f = plains_flower(0.5, &mut rng);
            f == bb::DANDELION || PLAINS_COMMON.contains(&f)
        }));
    }

    #[test]
    fn flower_forest_bands_cover_the_list_in_order() {
        assert_eq!(flower_forest_flower(-1.0), bb::DANDELION);
        assert_eq!(flower_forest_flower(-5.0), bb::DANDELION);
        assert_eq!(flower_forest_flower(1.0), bb::LILY_OF_THE_VALLEY);
        assert_eq!(flower_forest_flower(5.0), bb::LILY_OF_THE_VALLEY);
        assert_eq!(flower_forest_flower(0.0), FLOWER_FOREST_FLOWERS[5]);
        let mut previous = 0;
        for step in 0..=200 {
            let flower = flower_forest_flower(-1.0 + step as f32 / 100.0);
            let index = FLOWER_FOREST_FLOWERS.iter().position(|f| *f == flower).unwrap();
            assert!(index >= previous);
            previous = index;
        }
        assert_eq!(previous, FLOWER_FOREST_FLOWERS.len() - 1);
    }

    #[test]
    fn overworld_flower_weights() {
        let mut rng = BedrockRandom::new(3);
        let dandelions = (0..DRAWS).filter(|_| overworld_flower(&mut rng) == bb::DANDELION).count();
        close(dandelions as f64 / DRAWS as f64, 2.0 / 3.0);
    }

    #[test]
    fn double_flower_patch_counts() {
        let mut rng = BedrockRandom::new(11);
        let forest: usize = (0..DRAWS).map(|_| double_flower_patches(&mut rng, 0) as usize).sum();
        close(forest as f64 / DRAWS as f64, 0.2);
        let flower: usize = (0..DRAWS).map(|_| double_flower_patches(&mut rng, 2) as usize).sum();
        assert!(((flower as f64 / DRAWS as f64) - 1.2).abs() < 0.02);
    }

    #[test]
    fn biased_heights_stay_in_range_and_low() {
        let mut rng = BedrockRandom::new(5);
        let (min, max) = (-64, 311);
        let mut sum = 0i64;
        for _ in 0..DRAWS {
            let y = very_biased_to_bottom(&mut rng, min, max, 8);
            assert!((min..=max).contains(&y));
            sum += i64::from(y);
        }
        // Three nested draws: the mean sits far below the range midpoint.
        assert!((sum as f64 / DRAWS as f64) < f64::from(min + (max - min) / 4));
    }

    #[test]
    fn chunk_origin_masks_negative_coordinates() {
        assert_eq!(chunk_origin(BlockPos::new(-1, 70, 17)), BlockPos::new(-16, 70, 16));
        assert_eq!(chunk_origin(BlockPos::new(32, 0, -32)), BlockPos::new(32, 0, -32));
    }

    /// A 16x16 column of blocks from y 0 to 16, air by default.
    struct Grid(Vec<u32>);

    impl Grid {
        fn new() -> Self {
            Self(vec![bb::AIR; 16 * 16 * 16])
        }

        fn index(pos: BlockPos) -> Option<usize> {
            let inside = (0..16).contains(&pos.x) && (0..16).contains(&pos.y) && (0..16).contains(&pos.z);
            inside.then(|| ((pos.y * 16 + pos.z) * 16 + pos.x) as usize)
        }
    }

    impl BlockWorld for Grid {
        fn min_y(&self) -> i32 {
            0
        }
        fn max_y(&self) -> i32 {
            16
        }
        fn get(&self, pos: BlockPos) -> u32 {
            Self::index(pos).map_or(crate::world::generator::bedrock::world::OUT_OF_BOUNDS, |i| self.0[i])
        }
        fn set(&mut self, pos: BlockPos, id: u32) -> bool {
            Self::index(pos).map(|i| self.0[i] = id).is_some()
        }
        fn biome(&self, _: BlockPos) -> u16 {
            0
        }
        fn heightmap(&mut self, x: i32, z: i32) -> i32 {
            (0..16)
                .rev()
                .find(|&y| {
                    let id = self.get(BlockPos::new(x, y, z));
                    !bb::air(id) && !props::is_liquid(id)
                })
                .map_or(0, |y| y + 1)
        }
        fn above_top_solid(&mut self, x: i32, z: i32) -> i32 {
            self.heightmap(x, z)
        }
    }

    struct Flat;

    impl NoiseSource for Flat {
        fn noise(&self, _: f32, _: f32) -> f32 {
            0.0
        }
    }

    /// Stone cube with a one-block air pocket east of `(5, 5, 5)`.
    fn wall() -> Grid {
        let mut grid = Grid::new();
        for x in 0..16 {
            for y in 0..16 {
                for z in 0..16 {
                    grid.set(BlockPos::new(x, y, z), bb::STONE);
                }
            }
        }
        grid.set(BlockPos::new(6, 5, 5), bb::AIR);
        grid
    }

    #[test]
    fn spring_needs_one_opening() {
        let mut grid = wall();
        let mut rng = BedrockRandom::new(0);
        let mut ctx = Ctx { world: &mut grid, rng: &mut rng, noise: &Flat, seed: 0 };
        let at = BlockPos::new(5, 5, 5);
        assert_eq!(spring(&mut ctx, at, bb::WATER, bb::water_spring_host), Some(at));
        assert_eq!(ctx.get(at), bb::WATER);
        // Fully enclosed: no opening.
        let enclosed = BlockPos::new(10, 10, 10);
        assert_eq!(spring(&mut ctx, enclosed, bb::LAVA, bb::lava_spring_host), None);
        // Two openings: rejected.
        ctx.set(BlockPos::new(2, 5, 5), bb::AIR);
        ctx.set(BlockPos::new(2, 5, 6), bb::AIR);
        ctx.set(BlockPos::new(2, 5, 4), bb::AIR);
        assert_eq!(spring(&mut ctx, BlockPos::new(2, 5, 5), bb::LAVA, bb::lava_spring_host), None);
    }

    #[test]
    fn forest_rock_sinks_to_ground_and_stays_small() {
        let mut grid = Grid::new();
        for x in 0..16 {
            for z in 0..16 {
                grid.set(BlockPos::new(x, 4, z), bb::DIRT);
            }
        }
        let mut rng = BedrockRandom::new(9);
        let mut ctx = Ctx { world: &mut grid, rng: &mut rng, noise: &Flat, seed: 0 };
        let rock = forest_rock(&mut ctx, BlockPos::new(8, 12, 8)).expect("rock placed");
        assert_eq!(rock, BlockPos::new(8, 5, 8));
        let mut cobbles = 0;
        for x in 0..16 {
            for y in 0..16 {
                for z in 0..16 {
                    if grid.get(BlockPos::new(x, y, z)) == bb::MOSSY_COBBLESTONE {
                        cobbles += 1;
                        // Three blobs of half-axis <= 1, each shifted by <= 1.
                        assert!((5..=10).contains(&x) && (2..=6).contains(&y) && (5..=10).contains(&z));
                    }
                }
            }
        }
        // Blobs may coincide (all half-axes 0, no shift), so only the first
        // centre is guaranteed.
        assert!(cobbles >= 1);
        assert_eq!(grid.get(rock), bb::MOSSY_COBBLESTONE);
    }

    #[test]
    fn forest_rock_gives_up_near_the_bottom() {
        let mut grid = Grid::new();
        let mut rng = BedrockRandom::new(9);
        let mut ctx = Ctx { world: &mut grid, rng: &mut rng, noise: &Flat, seed: 0 };
        assert_eq!(forest_rock(&mut ctx, BlockPos::new(8, 12, 8)), None);
    }

    #[test]
    fn snow_rests_on_solid_ground_only() {
        assert!(holds_snow(bb::STONE));
        assert!(holds_snow(bb::GRASS_BLOCK));
        assert!(!holds_snow(bb::ICE));
        assert!(!holds_snow(bb::PACKED_ICE));
        assert!(!holds_snow(bb::WATER));
        assert!(!holds_snow(bb::AIR));
    }
}
