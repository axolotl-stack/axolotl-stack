//! Plant built-ins: vegetation BDS places by identifier without a JSON
//! definition (cactus, sugar cane, patches of crops, flowers, mushrooms,
//! vines, bamboo, moss/dripleaf vegetation, azalea root systems).
//!
//! Two shapes occur in the pack data:
//! - **patch built-ins** (`cactus_feature`, `reeds_feature`,
//!   `dead_bush_feature`, `melon_feature`, `pumpkin_feature`, the firefly
//!   clusters) are placed once by a rule at the heightmap position and spread
//!   their own attempts around it. Their per-chunk rule chances match the
//!   vanilla random patches both editions share (cactus 1/6 desert and 1/13
//!   badlands, sugar cane 1/6, melons 1/6 and 1/64, pumpkins 1/300, dead bush
//!   2 and 20 per chunk), so the patch parameters (tries and spreads) are the
//!   vanilla random-patch ones;
//! - **single-block built-ins** (flowers, mushrooms, berry bushes, petals,
//!   leaf litter) are wrapped by a `scatter_feature` that already does the
//!   spreading (e.g. 96 iterations, triangle +-7/+-3: exactly vanilla's
//!   default patch), so they place at most one plant at `pos`.
//!
//! Every placement checks the vanilla survival rule of the plant against
//! its support. Decoration aims at statistical parity: the draw order inside
//! each built-in follows the vanilla algorithm where it is known, but BDS's
//! exact order is not observable from data.

use super::super::generated::{self, bb, props};
use super::super::rng::BedrockRandom;
use super::super::world::{BlockPos, BlockWorld, Ctx, Direction};

// ---------------------------------------------------------------------------
// Block tables. Rows indexed by `growth` (0..=3, i.e. 1..=4 pieces), columns
// by `Direction as usize` (south, west, north, east: the order of
// `minecraft:cardinal_direction` values and of `Direction::ALL`).
// ---------------------------------------------------------------------------

/// `minecraft:pink_petals` by `[growth][direction]`.
const PINK_PETALS: [[u32; 4]; 4] = [
    [bb::PINK_PETALS_1_SOUTH, bb::PINK_PETALS_1_WEST, bb::PINK_PETALS_1_NORTH, bb::PINK_PETALS_1_EAST],
    [bb::PINK_PETALS_2_SOUTH, bb::PINK_PETALS_2_WEST, bb::PINK_PETALS_2_NORTH, bb::PINK_PETALS_2_EAST],
    [bb::PINK_PETALS_3_SOUTH, bb::PINK_PETALS_3_WEST, bb::PINK_PETALS_3_NORTH, bb::PINK_PETALS_3_EAST],
    [bb::PINK_PETALS_4_SOUTH, bb::PINK_PETALS_4_WEST, bb::PINK_PETALS_4_NORTH, bb::PINK_PETALS_4_EAST],
];

/// `minecraft:wildflowers` by `[growth][direction]`.
const WILDFLOWERS: [[u32; 4]; 4] = [
    [bb::WILDFLOWERS_1_SOUTH, bb::WILDFLOWERS_1_WEST, bb::WILDFLOWERS_1_NORTH, bb::WILDFLOWERS_1_EAST],
    [bb::WILDFLOWERS_2_SOUTH, bb::WILDFLOWERS_2_WEST, bb::WILDFLOWERS_2_NORTH, bb::WILDFLOWERS_2_EAST],
    [bb::WILDFLOWERS_3_SOUTH, bb::WILDFLOWERS_3_WEST, bb::WILDFLOWERS_3_NORTH, bb::WILDFLOWERS_3_EAST],
    [bb::WILDFLOWERS_4_SOUTH, bb::WILDFLOWERS_4_WEST, bb::WILDFLOWERS_4_NORTH, bb::WILDFLOWERS_4_EAST],
];

/// `minecraft:leaf_litter` by `[growth][direction]`.
const LEAF_LITTER: [[u32; 4]; 4] = [
    [bb::LEAF_LITTER_1_SOUTH, bb::LEAF_LITTER_1_WEST, bb::LEAF_LITTER_1_NORTH, bb::LEAF_LITTER_1_EAST],
    [bb::LEAF_LITTER_2_SOUTH, bb::LEAF_LITTER_2_WEST, bb::LEAF_LITTER_2_NORTH, bb::LEAF_LITTER_2_EAST],
    [bb::LEAF_LITTER_3_SOUTH, bb::LEAF_LITTER_3_WEST, bb::LEAF_LITTER_3_NORTH, bb::LEAF_LITTER_3_EAST],
    [bb::LEAF_LITTER_4_SOUTH, bb::LEAF_LITTER_4_WEST, bb::LEAF_LITTER_4_NORTH, bb::LEAF_LITTER_4_EAST],
];

/// `minecraft:pumpkin` by direction.
const PUMPKINS: [u32; 4] = [bb::PUMPKIN_SOUTH, bb::PUMPKIN_WEST, bb::PUMPKIN_NORTH, bb::PUMPKIN_EAST];

/// `minecraft:vine` attached on one side, by direction of the supporting
/// block (`vine_direction_bits` 1 south, 2 west, 4 north, 8 east).
const VINES: [u32; 4] = [bb::VINE_SOUTH, bb::VINE_WEST, bb::VINE_NORTH, bb::VINE_EAST];

/// Small dripleaf lower and upper halves by direction.
const SMALL_DRIPLEAF: [(u32, u32); 4] = [
    (bb::SMALL_DRIPLEAF_LOWER_SOUTH, bb::SMALL_DRIPLEAF_UPPER_SOUTH),
    (bb::SMALL_DRIPLEAF_LOWER_WEST, bb::SMALL_DRIPLEAF_UPPER_WEST),
    (bb::SMALL_DRIPLEAF_LOWER_NORTH, bb::SMALL_DRIPLEAF_UPPER_NORTH),
    (bb::SMALL_DRIPLEAF_LOWER_EAST, bb::SMALL_DRIPLEAF_UPPER_EAST),
];

/// Meadow flower mix: the flowers vanilla meadows show (dandelion, poppy,
/// allium, azure bluet, oxeye daisy, cornflower), equally likely. Vanilla's
/// meadow patch also mixes short and tall grass in, which BDS places through
/// its own grass rules, so only the flowers are drawn here.
const MEADOW_FLOWERS: [u32; 6] = [bb::DANDELION, bb::POPPY, bb::ALLIUM, bb::AZURE_BLUET, bb::OXEYE_DAISY, bb::CORNFLOWER];

/// A vegetation block: one block, or a lower/upper pair that needs the block
/// above free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Plant {
    Single(u32),
    Double(u32, u32),
}

/// Vanilla moss vegetation weights (sum 96).
const MOSS_VEGETATION: [(Plant, i32); 5] = [
    (Plant::Single(bb::FLOWERING_AZALEA), 4),
    (Plant::Single(bb::AZALEA), 7),
    (Plant::Single(bb::MOSS_CARPET), 25),
    (Plant::Single(bb::SHORT_GRASS), 50),
    (Plant::Double(bb::TALL_GRASS_LOWER, bb::TALL_GRASS_UPPER), 10),
];

/// Vanilla pale moss vegetation weights (sum 60): a flat pale moss carpet
/// (no side growth), short grass, tall grass. Pale hanging moss is not part
/// of it; it hangs from pale oak leaves (tree decoration).
const PALE_MOSS_VEGETATION: [(Plant, i32); 3] = [
    (Plant::Single(bb::PALE_MOSS_CARPET), 25),
    (Plant::Single(bb::SHORT_GRASS), 25),
    (Plant::Double(bb::TALL_GRASS_LOWER, bb::TALL_GRASS_UPPER), 10),
];

// ---------------------------------------------------------------------------
// Patch parameters (vanilla random patches; see the module docs).
// ---------------------------------------------------------------------------

/// Attempts / horizontal spread / vertical spread of the cactus patch.
const CACTUS_PATCH: (i32, i32, i32) = (10, 7, 3);
/// Sugar cane patch: on the origin's level only.
const REEDS_PATCH: (i32, i32, i32) = (20, 4, 0);
const DEAD_BUSH_PATCH: (i32, i32, i32) = (4, 7, 3);
const MELON_PATCH: (i32, i32, i32) = (64, 7, 3);
const PUMPKIN_PATCH: (i32, i32, i32) = (96, 7, 3);
/// Firefly bush cluster (vanilla firefly bush patch).
const FIREFLY_PATCH: (i32, i32, i32) = (20, 4, 3);
/// Random surface columns the near-water firefly cluster tries before giving
/// up. The rule fires 1 in 8 chunks at the chunk corner, so the built-in has
/// to look for a shore itself (a single random column would make the bushes
/// an order of magnitude rarer than observed); statistical parity, BDS's
/// search is not observable from data.
const FIREFLY_WATER_SAMPLES: i32 = 16;

/// Longest walk of `vines_feature` down from the canopy top (it stops at
/// the ground well before this).
const VINE_MAX_DESCENT: i32 = 128;
/// Search order of `vines_single_face_feature` (vanilla direction order
/// without up/down).
const SINGLE_FACE_ORDER: [Direction; 4] = [Direction::North, Direction::South, Direction::West, Direction::East];

/// Azalea root system (vanilla lush caves parameters).
const ROOT_COLUMN_MAX_HEIGHT: i32 = 100;
const ROOT_RADIUS: i32 = 3;
const ROOT_PLACEMENT_ATTEMPTS: i32 = 20;
const HANGING_ROOT_RADIUS: i32 = 3;
const HANGING_ROOTS_VERTICAL_SPAN: i32 = 2;
const HANGING_ROOT_PLACEMENT_ATTEMPTS: i32 = 20;
const TREE_REQUIRED_VERTICAL_SPACE: i32 = 3;
const TREE_ALLOWED_VERTICAL_WATER: i32 = 2;

// ---------------------------------------------------------------------------
// Pure helpers.
// ---------------------------------------------------------------------------

/// Vanilla random-patch offset: each axis is `next(s + 1) - next(s + 1)`, a
/// triangle distribution over `[-s, s]`; drawn x, y, z. A zero vertical
/// spread draws nothing for y.
fn patch_offset(rng: &mut BedrockRandom, xz_spread: i32, y_spread: i32) -> (i32, i32, i32) {
    let dx = rng.next_int(xz_spread + 1) - rng.next_int(xz_spread + 1);
    let dy = if y_spread > 0 { rng.next_int(y_spread + 1) - rng.next_int(y_spread + 1) } else { 0 };
    let dz = rng.next_int(xz_spread + 1) - rng.next_int(xz_spread + 1);
    (dx, dy, dz)
}

/// Vanilla "biased to bottom" integer in `[min, max]`:
/// `min + next(next(max - min + 1) + 1)`; low values are more likely.
fn biased_to_bottom(rng: &mut BedrockRandom, min: i32, max: i32) -> i32 {
    let span = rng.next_int(max - min + 1) + 1;
    min + rng.next_int(span)
}

/// Entry of a weighted table selected by `roll` in `[0, total weight)`.
fn weighted<T: Copy>(table: &[(T, i32)], mut roll: i32) -> T {
    for &(value, weight) in table {
        if roll < weight {
            return value;
        }
        roll -= weight;
    }
    table[table.len() - 1].0
}

/// Uniform weighted draw from `table`.
fn pick<T: Copy>(rng: &mut BedrockRandom, table: &[(T, i32)]) -> T {
    let total = table.iter().map(|&(_, w)| w).sum::<i32>();
    weighted(table, rng.next_int(total))
}

/// Column offset inside the disc of `radius` (vanilla `dx² + dz² <= r²`).
fn in_disc(dx: i32, dz: i32, radius: i32) -> bool {
    dx * dx + dz * dz <= radius * radius
}

/// Bamboo state of segment `k` (0 = base) of a stalk whose highest segment
/// is `top`: the top carries large leaves and the mature `age_bit`, the two
/// below it large then small leaves, the rest is bare thick stalk.
fn bamboo_segment(k: i32, top: i32) -> u32 {
    match top - k {
        0 => bb::BAMBOO_FINAL_LARGE,
        1 => bb::BAMBOO_TOP_LARGE,
        2 => bb::BAMBOO_TOP_SMALL,
        _ => bb::BAMBOO_TRUNK,
    }
}

// ---------------------------------------------------------------------------
// Survival rules.
// ---------------------------------------------------------------------------

/// Cactus: on sand or cactus, and no solid block on any horizontal side.
fn cactus_survives<W: BlockWorld>(ctx: &Ctx<'_, W>, at: BlockPos) -> bool {
    let below = ctx.get(at.below());
    (bb::cactus(below) || bb::sand_like(below))
        && Direction::ALL.into_iter().all(|d| !props::is_solid(ctx.get(at.side(d))))
}

/// Sugar cane ground: a dirt-like or sand block with water (or frosted ice)
/// next to it horizontally.
fn reeds_ground<W: BlockWorld>(ctx: &Ctx<'_, W>, ground: BlockPos) -> bool {
    let id = ctx.get(ground);
    (bb::dirt_like(id) || bb::sand_like(id))
        && Direction::ALL.into_iter().any(|d| bb::reeds_water(ctx.get(ground.side(d))))
}

/// Dead bush ground: sand, terracotta or dirt-like.
fn dead_bush_ground(id: u32) -> bool {
    bb::sand_like(id) || bb::terracotta(id) || bb::dirt_like(id)
}

/// Mushroom ground. Mycelium, podzol and nylium always; otherwise any opaque
/// full block. The vanilla low-light condition is not applied: decoration
/// runs without a light engine, and vanilla worlds show mushrooms on the
/// open surface of taiga and swamp, so worldgen effectively ignores it.
fn mushroom_ground(id: u32) -> bool {
    bb::mushroom_grow_block(id) || props::is_opaque(id)
}

/// Bamboo ground (vanilla "bamboo plantable on": dirt, sand, gravel, bamboo).
fn bamboo_ground(id: u32) -> bool {
    bb::dirt_like(id) || bb::sand_like(id) || bb::bamboo_plantable_extra(id)
}

/// A face vines can cling to: opaque full blocks and leaves.
fn supports_vine(id: u32) -> bool {
    props::is_opaque(id) || bb::leaves(id)
}

/// Ground an azalea tree grows on (vanilla "azalea grows on").
fn azalea_grows_on(id: u32) -> bool {
    bb::dirt_like(id) || bb::sand_like(id) || bb::terracotta(id) || id == bb::SNOW_BLOCK || id == bb::POWDER_SNOW
}

/// Blocks the root system turns into rooted dirt.
fn azalea_root_replaceable(id: u32) -> bool {
    bb::azalea_root_replaceable(id) || bb::dirt_like(id) || bb::terracotta(id)
}

// ---------------------------------------------------------------------------
// Shared placement.
// ---------------------------------------------------------------------------

/// Place `plant` at `pos`; a double plant needs air above. Returns whether it
/// was placed (a half-placed double plant is reverted).
fn place_plant<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, plant: Plant) -> bool {
    match plant {
        Plant::Single(id) => ctx.set(pos, id),
        Plant::Double(lower, upper) => {
            let top = pos.above();
            if !bb::air(ctx.get(top)) || !ctx.set(pos, lower) {
                return false;
            }
            if ctx.set(top, upper) {
                true
            } else {
                ctx.set(pos, bb::AIR);
                false
            }
        }
    }
}

/// One plant on air above ground matching `ground`.
fn place_single<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, block: u32, ground: fn(u32) -> bool) -> Option<BlockPos> {
    if !bb::air(ctx.get(pos)) || !ground(ctx.get(pos.below())) {
        return None;
    }
    ctx.set(pos, block).then_some(pos)
}

/// A segmented ground cover (petals, wildflowers, leaf litter): random piece
/// count 1..=4 (`growth` 0..=3; higher growth values render as four pieces
/// and are not generated), then a random `minecraft:cardinal_direction`.
fn place_segmented<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
    table: &[[u32; 4]; 4],
    ground: fn(u32) -> bool,
) -> Option<BlockPos> {
    if !bb::air(ctx.get(pos)) || !ground(ctx.get(pos.below())) {
        return None;
    }
    let growth = ctx.rng.next_int(4) as usize;
    let dir = ctx.rng.next_int(4) as usize;
    ctx.set(pos, table[growth][dir]).then_some(pos)
}

/// Floor vegetation drawn from a weighted table, on air above dirt-like ground.
fn place_vegetation<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, table: &[(Plant, i32)]) -> Option<BlockPos> {
    if !bb::air(ctx.get(pos)) || !bb::dirt_like(ctx.get(pos.below())) {
        return None;
    }
    let plant = pick(ctx.rng, table);
    place_plant(ctx, pos, plant).then_some(pos)
}

/// Air position directly above the surface of a random column of the chunk
/// whose minimum corner is `origin`.
fn random_surface_in_chunk<W: BlockWorld>(ctx: &mut Ctx<'_, W>, origin: BlockPos) -> BlockPos {
    let x = origin.x + ctx.rng.next_int(16);
    let z = origin.z + ctx.rng.next_int(16);
    let y = ctx.heightmap(x, z);
    BlockPos::new(x, y, z)
}

// ---------------------------------------------------------------------------
// Patch built-ins.
// ---------------------------------------------------------------------------

/// `minecraft:cactus_feature`: a patch of 10 cactus attempts (spread +-7
/// horizontally, +-3 vertically). Each attempt on air grows a column of
/// 1..=3 cactus (biased to 1), stopping where cactus would not survive (no
/// sand/cactus below or a solid horizontal neighbour); a complete column
/// gets a cactus flower on top with chance 1/4, like vanilla's flowering
/// desert cacti.
pub fn cactus_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let (tries, xz, y) = CACTUS_PATCH;
    let mut placed = false;
    for _ in 0..tries {
        let (dx, dy, dz) = patch_offset(ctx.rng, xz, y);
        let base = pos.offset(dx, dy, dz);
        if !bb::air(ctx.get(base)) {
            continue;
        }
        let height = biased_to_bottom(ctx.rng, 1, 3);
        let flower = ctx.rng.next_int(4) == 0;
        let mut grown = 0;
        while grown < height {
            let at = base.offset(0, grown, 0);
            if !bb::air(ctx.get(at)) || !cactus_survives(ctx, at) || !ctx.set(at, bb::CACTUS) {
                break;
            }
            grown += 1;
        }
        placed |= grown > 0;
        let top = base.offset(0, grown, 0);
        if grown == height && flower && bb::air(ctx.get(top)) {
            ctx.set(top, bb::CACTUS_FLOWER);
        }
    }
    placed.then_some(pos)
}

/// `minecraft:reeds_feature`: a patch of 20 sugar cane attempts on the
/// origin's level (spread +-4). Each attempt on air above a dirt-like or
/// sand block touching water grows 2..=4 sugar cane (biased to 2).
pub fn reeds_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let (tries, xz, y) = REEDS_PATCH;
    let mut placed = false;
    for _ in 0..tries {
        let (dx, dy, dz) = patch_offset(ctx.rng, xz, y);
        let base = pos.offset(dx, dy, dz);
        if !bb::air(ctx.get(base)) || !reeds_ground(ctx, base.below()) {
            continue;
        }
        let height = biased_to_bottom(ctx.rng, 2, 4);
        for k in 0..height {
            let at = base.offset(0, k, 0);
            if !bb::air(ctx.get(at)) || !ctx.set(at, bb::REEDS) {
                break;
            }
            placed = true;
        }
    }
    placed.then_some(pos)
}

/// `minecraft:dead_bush_feature`: 4 dead bush attempts around `pos` (spread
/// +-7/+-3) on air above sand, terracotta or dirt-like ground.
pub fn dead_bush_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let (tries, xz, y) = DEAD_BUSH_PATCH;
    let mut placed = false;
    for _ in 0..tries {
        let (dx, dy, dz) = patch_offset(ctx.rng, xz, y);
        placed |= place_single(ctx, pos.offset(dx, dy, dz), bb::DEADBUSH, dead_bush_ground).is_some();
    }
    placed.then_some(pos)
}

/// `minecraft:melon_feature`: 64 melon attempts (spread +-7/+-3) on grass
/// blocks. Like vanilla, a melon may replace a short replaceable plant
/// (short grass, fern, dead bush), not only air.
pub fn melon_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let (tries, xz, y) = MELON_PATCH;
    let mut placed = false;
    for _ in 0..tries {
        let (dx, dy, dz) = patch_offset(ctx.rng, xz, y);
        let at = pos.offset(dx, dy, dz);
        if bb::melon_replaceable(ctx.get(at)) && ctx.get(at.below()) == bb::GRASS_BLOCK {
            placed |= ctx.set(at, bb::MELON_BLOCK);
        }
    }
    placed.then_some(pos)
}

/// `minecraft:pumpkin_feature`: 96 pumpkin attempts (spread +-7/+-3) on air
/// above grass blocks, each pumpkin facing a random direction.
pub fn pumpkin_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let (tries, xz, y) = PUMPKIN_PATCH;
    let mut placed = false;
    for _ in 0..tries {
        let (dx, dy, dz) = patch_offset(ctx.rng, xz, y);
        let at = pos.offset(dx, dy, dz);
        if bb::air(ctx.get(at)) && ctx.get(at.below()) == bb::GRASS_BLOCK {
            let facing = PUMPKINS[ctx.rng.next_int(4) as usize];
            placed |= ctx.set(at, facing);
        }
    }
    placed.then_some(pos)
}

/// A firefly bush cluster around `center`: 20 attempts (spread +-4/+-3) on
/// air above dirt-like ground.
fn firefly_patch<W: BlockWorld>(ctx: &mut Ctx<'_, W>, center: BlockPos) -> Option<BlockPos> {
    let (tries, xz, y) = FIREFLY_PATCH;
    let mut placed = false;
    for _ in 0..tries {
        let (dx, dy, dz) = patch_offset(ctx.rng, xz, y);
        placed |= place_single(ctx, center.offset(dx, dy, dz), bb::FIREFLY_BUSH, bb::dirt_like).is_some();
    }
    placed.then_some(center)
}

/// `minecraft:firefly_bush_cluster_feature` (swamps): a firefly bush cluster
/// on the surface of a random column of the chunk. The rule passes the chunk
/// corner, so the column is chosen here (clusters would otherwise line up on
/// the chunk grid).
pub fn firefly_bush_cluster_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let center = random_surface_in_chunk(ctx, pos);
    firefly_patch(ctx, center)
}

/// `minecraft:firefly_bush_water_cluster_feature`: a firefly bush cluster at
/// a shore. Up to 16 random surface columns of the chunk are tried; the first
/// one that is air above dirt-like ground with water beside the ground block
/// (vanilla's near-water test) gets the cluster.
pub fn firefly_bush_water_cluster_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    for _ in 0..FIREFLY_WATER_SAMPLES {
        let center = random_surface_in_chunk(ctx, pos);
        let ground = center.below();
        if bb::air(ctx.get(center))
            && bb::dirt_like(ctx.get(ground))
            && Direction::ALL.into_iter().any(|d| bb::water(ctx.get(ground.side(d))))
        {
            return firefly_patch(ctx, center);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Single-block built-ins.
// ---------------------------------------------------------------------------

/// `minecraft:brown_mushroom_feature`: one brown mushroom at `pos` on
/// mushroom ground (see [`mushroom_ground`]).
pub fn brown_mushroom_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    place_single(ctx, pos, bb::BROWN_MUSHROOM, mushroom_ground)
}

/// `minecraft:red_mushroom_feature`: one red mushroom at `pos` on mushroom
/// ground (see [`mushroom_ground`]).
pub fn red_mushroom_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    place_single(ctx, pos, bb::RED_MUSHROOM, mushroom_ground)
}

/// `minecraft:sweet_berry_bush_feature`: one mature sweet berry bush
/// (`growth` 3, bearing berries) on air above a grass block (vanilla
/// restricts the berry patch to grass blocks).
pub fn sweet_berry_bush_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    place_single(ctx, pos, bb::SWEET_BERRY_BUSH_MATURE, |id| id == bb::GRASS_BLOCK)
}

/// `minecraft:eyeblossom_feature` (pale garden): one eyeblossom on dirt-like
/// ground. Generated eyeblossoms are closed: the open/closed state follows
/// the day cycle through random ticks, and generation happens outside it
/// (vanilla generates the closed variant).
pub fn eyeblossom_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    place_single(ctx, pos, bb::CLOSED_EYEBLOSSOM, bb::dirt_like)
}

/// `minecraft:meadow_flower_feature`: one meadow flower (see
/// [`MEADOW_FLOWERS`]) on dirt-like ground.
pub fn meadow_flower_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    if !bb::air(ctx.get(pos)) || !bb::dirt_like(ctx.get(pos.below())) {
        return None;
    }
    let flower = MEADOW_FLOWERS[ctx.rng.next_int(MEADOW_FLOWERS.len() as i32) as usize];
    ctx.set(pos, flower).then_some(pos)
}

/// `minecraft:pink_petals_feature`: pink petals with a random amount and
/// facing on dirt-like ground.
pub fn pink_petals_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    place_segmented(ctx, pos, &PINK_PETALS, bb::dirt_like)
}

/// `minecraft:wildflowers_feature`: wildflowers with a random amount and
/// facing on dirt-like ground.
pub fn wildflowers_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    place_segmented(ctx, pos, &WILDFLOWERS, bb::dirt_like)
}

/// `minecraft:leaf_litter_feature`: leaf litter with a random amount and
/// facing on any solid ground except leaves (vanilla litter needs a full top
/// face; the scatter around a tree reaches 2 blocks up, which must not put
/// litter on the canopy).
pub fn leaf_litter_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    place_segmented(ctx, pos, &LEAF_LITTER, |id| props::is_solid(id) && !bb::leaves(id))
}

// ---------------------------------------------------------------------------
// Vines.
// ---------------------------------------------------------------------------

/// `minecraft:vines_feature` (jungles, 50 per chunk via
/// `fixup_vines_position_feature`, which shifts the rule position by
/// (+8, +8) after the rule sampled the heightmap of the unshifted column).
///
/// Walks down from the top of the column at `pos` (its own heightmap): every
/// air block with a supporting neighbour (opaque block or leaves) gets a vine
/// on the first supporting side in `Direction::ALL` order; leaves, logs and
/// vines in the way shift the walk by a random horizontal step of up to +-3
/// (the classic jungle vine walk, run downwards from the canopy so vines
/// hang along trunks and leaf edges). The walk ends at the ground (any other
/// block) or after [`VINE_MAX_DESCENT`] steps.
pub fn vines_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let top = ctx.heightmap(pos.x, pos.z);
    let mut at = BlockPos::new(pos.x, top, pos.z);
    let mut placed = false;
    for _ in 0..VINE_MAX_DESCENT {
        let id = ctx.get(at);
        if bb::air(id) {
            let support = Direction::ALL.into_iter().find(|&d| supports_vine(ctx.get(at.side(d))));
            if let Some(dir) = support {
                placed |= ctx.set(at, VINES[dir as usize]);
            }
        } else if bb::leaves(id) || bb::logs(id) || bb::vine(id) {
            let dx = ctx.rng.next_int(4) - ctx.rng.next_int(4);
            let dz = ctx.rng.next_int(4) - ctx.rng.next_int(4);
            at = at.offset(dx, 0, dz);
        } else {
            break;
        }
        at = at.below();
    }
    placed.then_some(pos)
}

/// `minecraft:vines_single_face_feature` (lush caves): one vine at `pos` if
/// it is air, attached to the first supporting neighbour in vanilla
/// direction order (north, south, west, east).
pub fn vines_single_face_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    if !bb::air(ctx.get(pos)) {
        return None;
    }
    let dir = SINGLE_FACE_ORDER.into_iter().find(|&d| supports_vine(ctx.get(pos.side(d))))?;
    ctx.set(pos, VINES[dir as usize]).then_some(pos)
}

// ---------------------------------------------------------------------------
// Bamboo and podzol.
// ---------------------------------------------------------------------------

/// `minecraft:bamboo_feature`: one bamboo stalk of 5..=16 thick segments on
/// air above bamboo ground, stopping early at the first non-air block. A
/// stalk of at least 3 segments gets its leafy top: large leaves with the
/// mature `age_bit` on a segment above the stalk, large then small leaves
/// on the two below ([`bamboo_segment`]). When the block above the stalk is
/// taken, the top replaces the highest segment instead (vanilla would write
/// it into the obstruction). The podzol disc of vanilla bamboo is a separate
/// feature here ([`podzol_area_feature`]).
pub fn bamboo_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    if !bb::air(ctx.get(pos)) || !bamboo_ground(ctx.get(pos.below())) {
        return None;
    }
    let height = ctx.rng.next_int(12) + 5;
    let mut grown = 0;
    while grown < height {
        let at = pos.offset(0, grown, 0);
        if !bb::air(ctx.get(at)) || !ctx.set(at, bb::BAMBOO_TRUNK) {
            break;
        }
        grown += 1;
    }
    if grown == 0 {
        return None;
    }
    if grown >= 3 {
        let above = pos.offset(0, grown, 0);
        let top = if bb::air(ctx.get(above)) && ctx.set(above, bb::BAMBOO_FINAL_LARGE) { grown } else { grown - 1 };
        for k in top - 2..=top {
            ctx.set(pos.offset(0, k, 0), bamboo_segment(k, top));
        }
    }
    Some(pos)
}

/// `minecraft:podzol_area_feature` (after bamboo, 20%): turns the surface
/// block of every column in a disc of radius 1..=4 around `pos` into podzol
/// when it is dirt-like and not under liquid. Bamboo stalks (placed first by
/// `bamboo_then_podzol_feature`) are skipped down to their ground.
pub fn podzol_area_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let radius = ctx.rng.next_int(4) + 1;
    let mut placed = false;
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if !in_disc(dx, dz, radius) {
                continue;
            }
            let (x, z) = (pos.x + dx, pos.z + dz);
            let mut ground = BlockPos::new(x, ctx.heightmap(x, z) - 1, z);
            // `OUT_OF_BOUNDS` is not bamboo, so this ends below the world.
            while bb::bamboo(ctx.get(ground)) {
                ground = ground.below();
            }
            if bb::dirt_like(ctx.get(ground)) && !props::is_liquid(ctx.get(ground.above())) {
                placed |= ctx.set(ground, bb::PODZOL);
            }
        }
    }
    placed.then_some(pos)
}

// ---------------------------------------------------------------------------
// Vegetation patch callees.
// ---------------------------------------------------------------------------

/// `minecraft:moss_vegetation_feature` (vegetation of moss patches): one of
/// flowering azalea, azalea, moss carpet, short grass or tall grass
/// ([`MOSS_VEGETATION`] weights) on air above the moss.
pub fn moss_vegetation_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    place_vegetation(ctx, pos, &MOSS_VEGETATION)
}

/// `minecraft:pale_moss_vegetation_feature` (vegetation of pale moss
/// patches): pale moss carpet, short grass or tall grass
/// ([`PALE_MOSS_VEGETATION`] weights) on air above the pale moss.
pub fn pale_moss_vegetation_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    place_vegetation(ctx, pos, &PALE_MOSS_VEGETATION)
}

/// `minecraft:dripleaf_feature` (vegetation of the waterlogged clay patches
/// in lush caves): one of five equally likely choices, vanilla's dripleaf
/// selector: a small dripleaf, or a big dripleaf facing east, west, south or
/// north (BDS ships those as `big_dripleaf_<dir>_feature` growing plants).
pub fn dripleaf_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    match ctx.rng.next_int(5) {
        0 => small_dripleaf(ctx, pos),
        1 => generated::f_big_dripleaf_east_feature(ctx, pos),
        2 => generated::f_big_dripleaf_west_feature(ctx, pos),
        3 => generated::f_big_dripleaf_south_feature(ctx, pos),
        _ => generated::f_big_dripleaf_north_feature(ctx, pos),
    }
}

/// A two-block small dripleaf with a random facing, in air or water, on clay
/// or moss (or on dirt-like ground when standing in water). The decoration
/// world has no liquid layer, so a dripleaf placed in water replaces it
/// rather than being waterlogged.
fn small_dripleaf<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let (here, above, below) = (ctx.get(pos), ctx.get(pos.above()), ctx.get(pos.below()));
    let open = |id: u32| bb::air(id) || bb::water(id);
    if !open(here) || !open(above) {
        return None;
    }
    if !(bb::small_dripleaf_ground(below) || (bb::water(here) && bb::dirt_like(below))) {
        return None;
    }
    let (lower, upper) = SMALL_DRIPLEAF[ctx.rng.next_int(4) as usize];
    if !ctx.set(pos, lower) {
        return None;
    }
    if !ctx.set(pos.above(), upper) {
        ctx.set(pos, here);
        return None;
    }
    Some(pos)
}

// ---------------------------------------------------------------------------
// Azalea root system.
// ---------------------------------------------------------------------------

/// `minecraft:azalea_root_system_feature`, placed by
/// `azalea_root_system_snap_to_ceiling_feature` at the air block under a
/// lush cave ceiling (vanilla root system).
///
/// From `pos` it climbs up to 100 blocks looking for a tree spot: air or a
/// tree-replaceable plant, on azalea ground, with 3 blocks of air above
/// (the first may be water). The block under the spot must be solid and not
/// lava, else the feature fails. When `azalea_tree_feature` grows there, a
/// column of rooted dirt joins the cave ceiling to the tree (20 attempts per
/// layer within +-2 blocks, replacing stone/dirt/sand-like blocks, from `pos`
/// up to two blocks below the tree), and up to 20 hanging roots are placed
/// in air under solid blocks within +-2 horizontally and +-1 vertically of
/// `pos`. Returns the tree position.
pub fn azalea_root_system_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    if !bb::air(ctx.get(pos)) {
        return None;
    }
    for climb in 1..=ROOT_COLUMN_MAX_HEIGHT {
        let tree = pos.offset(0, climb, 0);
        if !ctx.in_height(tree.y) {
            return None;
        }
        if !azalea_tree_position(ctx, tree) {
            continue;
        }
        let below = ctx.get(tree.below());
        if bb::lava(below) || !props::is_solid(below) {
            return None;
        }
        if generated::f_azalea_tree_feature(ctx, tree).is_some() {
            for y in pos.y..tree.y - 1 {
                place_rooted_dirt_layer(ctx, BlockPos::new(pos.x, y, pos.z));
            }
            place_hanging_roots(ctx, pos);
            return Some(tree);
        }
    }
    None
}

/// Whether an azalea tree may stand at `at` (see
/// [`azalea_root_system_feature`]).
fn azalea_tree_position<W: BlockWorld>(ctx: &Ctx<'_, W>, at: BlockPos) -> bool {
    let here = ctx.get(at);
    if !(bb::air(here) || bb::replaceable_by_trees(here)) || !azalea_grows_on(ctx.get(at.below())) {
        return false;
    }
    (1..=TREE_REQUIRED_VERTICAL_SPACE).all(|k| {
        let id = ctx.get(at.offset(0, k, 0));
        // Vanilla allows water at height k while `k + 1 <= allowed` (only the first block here).
        bb::air(id) || (k < TREE_ALLOWED_VERTICAL_WATER && bb::water(id))
    })
}

/// Rooted dirt attempts in the layer of `center`.
fn place_rooted_dirt_layer<W: BlockWorld>(ctx: &mut Ctx<'_, W>, center: BlockPos) {
    for _ in 0..ROOT_PLACEMENT_ATTEMPTS {
        let dx = ctx.rng.next_int(ROOT_RADIUS) - ctx.rng.next_int(ROOT_RADIUS);
        let dz = ctx.rng.next_int(ROOT_RADIUS) - ctx.rng.next_int(ROOT_RADIUS);
        let at = center.offset(dx, 0, dz);
        if azalea_root_replaceable(ctx.get(at)) {
            ctx.set(at, bb::ROOTED_DIRT);
        }
    }
}

/// Hanging roots around the root system's ceiling point.
fn place_hanging_roots<W: BlockWorld>(ctx: &mut Ctx<'_, W>, origin: BlockPos) {
    for _ in 0..HANGING_ROOT_PLACEMENT_ATTEMPTS {
        let dx = ctx.rng.next_int(HANGING_ROOT_RADIUS) - ctx.rng.next_int(HANGING_ROOT_RADIUS);
        let dy = ctx.rng.next_int(HANGING_ROOTS_VERTICAL_SPAN) - ctx.rng.next_int(HANGING_ROOTS_VERTICAL_SPAN);
        let dz = ctx.rng.next_int(HANGING_ROOT_RADIUS) - ctx.rng.next_int(HANGING_ROOT_RADIUS);
        let at = origin.offset(dx, dy, dz);
        if bb::air(ctx.get(at)) && props::is_solid(ctx.get(at.above())) {
            ctx.set(at, bb::HANGING_ROOTS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::world::{NoiseSource, OUT_OF_BOUNDS};
    use super::*;
    use std::collections::HashMap;

    /// Sparse test world: air everywhere unless set, y in [-64, 320).
    #[derive(Default)]
    struct TestWorld {
        blocks: HashMap<BlockPos, u32>,
    }

    impl TestWorld {
        fn in_range(pos: BlockPos) -> bool {
            (-64..320).contains(&pos.y)
        }

        /// A square floor of `id` at `y`, `x, z` in `[-r, r]`.
        fn floor(r: i32, y: i32, id: u32) -> Self {
            let mut w = Self::default();
            for x in -r..=r {
                for z in -r..=r {
                    w.blocks.insert(BlockPos::new(x, y, z), id);
                }
            }
            w
        }

        fn top(&self, x: i32, z: i32, pred: fn(u32) -> bool) -> i32 {
            (-64..320).rev().find(|&y| pred(self.get(BlockPos::new(x, y, z)))).map_or(-64, |y| y + 1)
        }
    }

    impl BlockWorld for TestWorld {
        fn min_y(&self) -> i32 {
            -64
        }
        fn max_y(&self) -> i32 {
            320
        }
        fn get(&self, pos: BlockPos) -> u32 {
            if Self::in_range(pos) { self.blocks.get(&pos).copied().unwrap_or(bb::AIR) } else { OUT_OF_BOUNDS }
        }
        fn set(&mut self, pos: BlockPos, id: u32) -> bool {
            if !Self::in_range(pos) {
                return false;
            }
            self.blocks.insert(pos, id);
            true
        }
        fn biome(&self, _: BlockPos) -> u16 {
            0
        }
        fn heightmap(&mut self, x: i32, z: i32) -> i32 {
            self.top(x, z, |id| !bb::air(id) && !props::is_liquid(id))
        }
        fn above_top_solid(&mut self, x: i32, z: i32) -> i32 {
            self.top(x, z, props::is_solid)
        }
    }

    struct Flat;
    impl NoiseSource for Flat {
        fn noise(&self, _: f32, _: f32) -> f32 {
            0.0
        }
    }

    fn run(world: &mut TestWorld, seed: u32, f: fn(&mut Ctx<'_, TestWorld>, BlockPos) -> Option<BlockPos>, pos: BlockPos) -> Option<BlockPos> {
        let mut rng = BedrockRandom::new(seed);
        let mut ctx = Ctx { world, rng: &mut rng, noise: &Flat, seed: 0 };
        f(&mut ctx, pos)
    }

    #[test]
    fn weighted_selects_by_cumulative_weight() {
        let table = [('a', 1), ('b', 2), ('c', 3)];
        let picks: Vec<char> = (0..6).map(|roll| weighted(&table, roll)).collect();
        assert_eq!(picks, ['a', 'b', 'b', 'c', 'c', 'c']);
        assert_eq!(MOSS_VEGETATION.iter().map(|&(_, w)| w).sum::<i32>(), 96);
        assert_eq!(PALE_MOSS_VEGETATION.iter().map(|&(_, w)| w).sum::<i32>(), 60);
    }

    #[test]
    fn biased_to_bottom_stays_in_range_and_favours_low_values() {
        let mut rng = BedrockRandom::new(7);
        let mut counts = [0u32; 3];
        for _ in 0..30_000 {
            let v = biased_to_bottom(&mut rng, 1, 3);
            assert!((1..=3).contains(&v));
            counts[(v - 1) as usize] += 1;
        }
        // Expected 11/18, 5/18, 2/18.
        assert!(counts[0] > counts[1] && counts[1] > counts[2] && counts[2] > 0, "{counts:?}");
    }

    #[test]
    fn patch_offsets_are_bounded_and_flat_without_vertical_spread() {
        let mut rng = BedrockRandom::new(1);
        let (mut min, mut max) = (0, 0);
        for _ in 0..10_000 {
            let (dx, dy, dz) = patch_offset(&mut rng, 7, 3);
            assert!((-7..=7).contains(&dx) && (-3..=3).contains(&dy) && (-7..=7).contains(&dz));
            min = min.min(dx);
            max = max.max(dx);
            let (_, flat, _) = patch_offset(&mut rng, 4, 0);
            assert_eq!(flat, 0);
        }
        assert_eq!((min, max), (-7, 7));
    }

    #[test]
    fn disc_has_vanilla_cell_counts() {
        let cells = |r: i32| (-r..=r).flat_map(|x| (-r..=r).map(move |z| (x, z))).filter(|&(x, z)| in_disc(x, z, r)).count();
        assert_eq!([cells(1), cells(2), cells(3), cells(4)], [5, 13, 29, 49]);
    }

    #[test]
    fn bamboo_segments_put_leaves_on_top() {
        let column: Vec<u32> = (0..=6).map(|k| bamboo_segment(k, 6)).collect();
        assert_eq!(&column[..4], &[bb::BAMBOO_TRUNK; 4]);
        assert_eq!(&column[4..], &[bb::BAMBOO_TOP_SMALL, bb::BAMBOO_TOP_LARGE, bb::BAMBOO_FINAL_LARGE]);
    }

    #[test]
    fn bamboo_grows_a_leafy_stalk_on_grass() {
        let mut w = TestWorld::floor(2, 63, bb::GRASS_BLOCK);
        let base = BlockPos::new(0, 64, 0);
        assert_eq!(run(&mut w, 3, bamboo_feature, base), Some(base));
        let len = (0..).take_while(|&k| bb::bamboo(w.get(base.offset(0, k, 0)))).count() as i32;
        assert!((6..=17).contains(&len), "len {len}");
        for k in 0..len {
            assert_eq!(w.get(base.offset(0, k, 0)), bamboo_segment(k, len - 1));
        }
        // Not on stone.
        let mut stone = TestWorld::floor(2, 63, bb::STONE);
        assert_eq!(run(&mut stone, 3, bamboo_feature, base), None);
    }

    #[test]
    fn cactus_never_touches_solid_blocks() {
        let mut placed = 0;
        for seed in 0..40 {
            let mut w = TestWorld::floor(12, 63, bb::SAND);
            // Stone pillars every 3 blocks make many spots invalid.
            for x in (-12..=12).step_by(3) {
                for z in (-12..=12).step_by(3) {
                    w.blocks.insert(BlockPos::new(x, 64, z), bb::STONE);
                }
            }
            placed += run(&mut w, seed, cactus_feature, BlockPos::new(1, 64, 1)).is_some() as u32;
            for (&pos, &id) in &w.blocks {
                if id != bb::CACTUS {
                    continue;
                }
                let below = w.get(pos.below());
                assert!(below == bb::SAND || below == bb::CACTUS);
                assert!(Direction::ALL.into_iter().all(|d| !props::is_solid(w.get(pos.side(d)))));
                assert!(pos.y <= 66, "column taller than 3 at {pos:?}");
            }
        }
        assert!(placed > 0);
    }

    #[test]
    fn reeds_need_water_next_to_their_ground() {
        let mut dry = TestWorld::floor(8, 63, bb::SAND);
        assert_eq!(run(&mut dry, 5, reeds_feature, BlockPos::new(0, 64, 0)), None);

        let mut wet = TestWorld::floor(8, 63, bb::SAND);
        for z in -8..=8 {
            wet.blocks.insert(BlockPos::new(0, 63, z), bb::WATER);
        }
        let placed = (0..20).filter(|&s| run(&mut wet, s, reeds_feature, BlockPos::new(0, 64, 0)).is_some()).count();
        assert!(placed > 0);
        for (&pos, &id) in &wet.blocks {
            if id == bb::REEDS && w_is_base(&wet, pos) {
                assert_eq!(pos.x.abs(), 1, "cane base away from water at {pos:?}");
                let height = (0..).take_while(|&k| wet.get(pos.offset(0, k, 0)) == bb::REEDS).count();
                assert!((2..=4).contains(&height));
            }
        }
    }

    fn w_is_base(w: &TestWorld, pos: BlockPos) -> bool {
        w.get(pos.below()) != bb::REEDS
    }

    #[test]
    fn single_face_vine_attaches_to_its_support() {
        let mut w = TestWorld::default();
        let pos = BlockPos::new(0, 10, 0);
        assert_eq!(run(&mut w, 1, vines_single_face_feature, pos), None);
        w.blocks.insert(pos.side(Direction::East), bb::STONE);
        assert_eq!(run(&mut w, 1, vines_single_face_feature, pos), Some(pos));
        assert_eq!(w.get(pos), bb::VINE_EAST);
    }

    #[test]
    fn segmented_covers_use_one_to_four_pieces_on_dirt_only() {
        let mut w = TestWorld::floor(0, 63, bb::GRASS_BLOCK);
        let pos = BlockPos::new(0, 64, 0);
        assert_eq!(run(&mut w, 9, pink_petals_feature, pos), Some(pos));
        assert!(PINK_PETALS.iter().flatten().any(|&id| id == w.get(pos)));
        let mut stone = TestWorld::floor(0, 63, bb::STONE);
        assert_eq!(run(&mut stone, 9, wildflowers_feature, pos), None);
        assert_eq!(run(&mut stone, 9, leaf_litter_feature, pos), Some(pos));
    }
}
