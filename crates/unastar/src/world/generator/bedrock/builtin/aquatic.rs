//! Water built-ins: kelp, seagrass, sea pickles, the warm-ocean reef features
//! (coral structures, crust, hanging fans, scattered corals), the legacy
//! underwater discs (clay, gravel, sand), the mangrove grass disc and lily
//! pads.
//!
//! Vanilla kelp/seagrass rules and nested scatters use `query.heightmap`.
//! The measured query includes fluids and returns the first air height, not
//! the water block on the floor; `query.above_top_solid` skips fluids. Scatter
//! expressions remain relative offsets, so nested seagrass scatters arrive
//! well above the water.
//!
//! **Sea-floor projection.** Every rule feeding `kelp_feature` and the
//! seagrass built-ins passes such an above-water height, yet BDS 1.26.51 grows
//! both from the sea floor (fixture chunk (-4,-4): 517 kelp, 65 seagrass, all
//! rooted on the floor). The built-ins therefore ignore the input height and
//! start above the column's highest motion-blocking block ([`sea_floor`]),
//! like the ocean-floor heightmap of the legacy features they port; existing
//! plants do not raise it. Placement density and random draw order remain
//! unmeasured.
//!
//! Plants replace the primary water block; these built-ins write no
//! secondary liquid layer.
//!
//! **unmeasured: to confirm by BDS fixture**: geometry, density, palettes and
//! random draw order of built-in aquatic placements.

use super::super::generated::{bb, props};
use super::super::rng::BedrockRandom;
use super::super::world::{BlockPos, BlockWorld, Ctx, Direction};

/// Coral blocks, colours ordered tube, brain, bubble, fire, horn (the order
/// of every coral table below).
const CORAL_BLOCKS: [u32; 5] = [
    bb::TUBE_CORAL_BLOCK,
    bb::BRAIN_CORAL_BLOCK,
    bb::BUBBLE_CORAL_BLOCK,
    bb::FIRE_CORAL_BLOCK,
    bb::HORN_CORAL_BLOCK,
];

/// Standing coral plants.
const CORAL_PLANTS: [u32; 5] = [
    bb::TUBE_CORAL,
    bb::BRAIN_CORAL,
    bb::BUBBLE_CORAL,
    bb::FIRE_CORAL,
    bb::HORN_CORAL,
];

/// Floor coral fans, both `coral_fan_direction` rotations.
const CORAL_FANS: [[u32; 2]; 5] = [
    [bb::TUBE_CORAL_FAN_0, bb::TUBE_CORAL_FAN_1],
    [bb::BRAIN_CORAL_FAN_0, bb::BRAIN_CORAL_FAN_1],
    [bb::BUBBLE_CORAL_FAN_0, bb::BUBBLE_CORAL_FAN_1],
    [bb::FIRE_CORAL_FAN_0, bb::FIRE_CORAL_FAN_1],
    [bb::HORN_CORAL_FAN_0, bb::HORN_CORAL_FAN_1],
];

/// Wall coral fans indexed `[colour][dir_index(facing)]`, i.e. in
/// [`Direction::ALL`] order (south, west, north, east). A fan facing `dir`
/// sits on the `dir` side of its supporting block.
const CORAL_WALL_FANS: [[u32; 4]; 5] = [
    [
        bb::TUBE_CORAL_WALL_FAN_SOUTH,
        bb::TUBE_CORAL_WALL_FAN_WEST,
        bb::TUBE_CORAL_WALL_FAN_NORTH,
        bb::TUBE_CORAL_WALL_FAN_EAST,
    ],
    [
        bb::BRAIN_CORAL_WALL_FAN_SOUTH,
        bb::BRAIN_CORAL_WALL_FAN_WEST,
        bb::BRAIN_CORAL_WALL_FAN_NORTH,
        bb::BRAIN_CORAL_WALL_FAN_EAST,
    ],
    [
        bb::BUBBLE_CORAL_WALL_FAN_SOUTH,
        bb::BUBBLE_CORAL_WALL_FAN_WEST,
        bb::BUBBLE_CORAL_WALL_FAN_NORTH,
        bb::BUBBLE_CORAL_WALL_FAN_EAST,
    ],
    [
        bb::FIRE_CORAL_WALL_FAN_SOUTH,
        bb::FIRE_CORAL_WALL_FAN_WEST,
        bb::FIRE_CORAL_WALL_FAN_NORTH,
        bb::FIRE_CORAL_WALL_FAN_EAST,
    ],
    [
        bb::HORN_CORAL_WALL_FAN_SOUTH,
        bb::HORN_CORAL_WALL_FAN_WEST,
        bb::HORN_CORAL_WALL_FAN_NORTH,
        bb::HORN_CORAL_WALL_FAN_EAST,
    ],
];

/// Sea pickles with 1..=4 pickles (`cluster_count` 0..=3).
const SEA_PICKLES: [u32; 4] = [
    bb::SEA_PICKLE_1,
    bb::SEA_PICKLE_2,
    bb::SEA_PICKLE_3,
    bb::SEA_PICKLE_4,
];

/// Kelp tips: vanilla worldgen kelp ends in a tip aged 20..=23, so it only
/// grows a few more blocks (growth stops at age 25).
const KELP_TIPS: [u32; 4] = [
    bb::KELP_AGE_20,
    bb::KELP_AGE_21,
    bb::KELP_AGE_22,
    bb::KELP_AGE_23,
];

/// Chance that a seagrass placement is double (tall) seagrass, per variant.
/// These are the vanilla probabilities of the four seagrass configurations;
/// BDS picks the variant per biome in its scatter documents: ocean, cold and
/// warm ocean use the shortest, rivers the shorter, swamps the default and
/// deep oceans the taller variant.
const SHORTEST_TALL_CHANCE: f32 = 0.3;
const SHORTER_TALL_CHANCE: f32 = 0.4;
const SEAGRASS_TALL_CHANCE: f32 = 0.6;
const TALLER_TALL_CHANCE: f32 = 0.8;

/// Lily pad search: how many water blocks `waterlily_feature` climbs from
/// its input to reach the surface. The input is the floor height plus a
/// gaussian offset in `[-4, 4]`, so swamp water (a few blocks deep) is
/// covered while deep lakes are not scanned.
const WATERLILY_MAX_CLIMB: i32 = 8;

/// Attempts of one sea pickle cluster (vanilla sea pickle feature count).
const PICKLE_ATTEMPTS: u32 = 20;

/// Index of `dir` in [`Direction::ALL`].
#[inline]
const fn dir_index(dir: Direction) -> usize {
    match dir {
        Direction::South => 0,
        Direction::West => 1,
        Direction::North => 2,
        Direction::East => 3,
    }
}

/// `dir` turned 90° clockwise seen from above ([`Direction::ALL`] is in
/// clockwise order: south, west, north, east).
#[inline]
const fn clockwise(dir: Direction) -> Direction {
    Direction::ALL[(dir_index(dir) + 1) % 4]
}

/// `dir` turned 90° counter-clockwise seen from above.
#[inline]
const fn counter_clockwise(dir: Direction) -> Direction {
    Direction::ALL[(dir_index(dir) + 3) % 4]
}

/// Uniform random horizontal direction.
#[inline]
fn random_direction(rng: &mut BedrockRandom) -> Direction {
    Direction::ALL[rng.next_int(4) as usize]
}

/// Fisher-Yates shuffle in place.
fn shuffle<T>(rng: &mut BedrockRandom, items: &mut [T]) {
    for i in (1..items.len()).rev() {
        let j = rng.next_int(i as i32 + 1) as usize;
        items.swap(i, j);
    }
}

/// Column `(dx, dz)` lies in the disc of radius `r` (vanilla disc test
/// `dx² + dz² <= r²`).
#[inline]
const fn in_disc(dx: i32, dz: i32, r: i32) -> bool {
    dx * dx + dz * dz <= r * r
}

/// Cell `(x, y, z)` of a `[0, sx] × [0, sy] × [0, sz]` box belongs to the
/// coral mushroom: exactly one coordinate on the box boundary, so the shell
/// without its edges and corners (the rounded cap of the vanilla shape).
#[inline]
const fn mushroom_shell(x: i32, y: i32, z: i32, sx: i32, sy: i32, sz: i32) -> bool {
    let on_x = (x == 0 || x == sx) as u8;
    let on_y = (y == 0 || y == sy) as u8;
    let on_z = (z == 0 || z == sz) as u8;
    on_x + on_y + on_z == 1
}

/// Block a rooted underwater plant (kelp base, seagrass, coral, pickle) can
/// stand on: a motion-blocking block other than magma (which bubbles). The
/// palette `solid` flag also marks plants such as seagrass, so it would let
/// kelp root on seagrass; motion blocking matches the legacy sturdy-face
/// check. Sea pickles stay excluded explicitly so repeated pickle attempts in
/// one column never stack.
#[inline]
fn supports_water_plant(id: u32) -> bool {
    props::blocks_motion(id) && id != bb::MAGMA && !SEA_PICKLES.contains(&id)
}

/// Random standing coral or floor fan of a random colour (vanilla picks from
/// the ten corals of the `corals` set, fans get a random rotation).
fn random_coral_decoration(rng: &mut BedrockRandom) -> u32 {
    let pick = rng.next_int(10) as usize;
    if pick < 5 {
        CORAL_PLANTS[pick]
    } else {
        CORAL_FANS[pick - 5][rng.next_int(2) as usize]
    }
}

/// Colour index of a coral block, if `id` is one.
#[inline]
fn coral_colour(id: u32) -> Option<usize> {
    CORAL_BLOCKS.iter().position(|&b| b == id)
}

// ---------------------------------------------------------------------------
// Kelp and seagrass
// ---------------------------------------------------------------------------

/// The first position above the highest motion-blocking block of `pos`'s
/// column (the legacy ocean-floor heightmap): where kelp and seagrass start
/// regardless of the input height. Existing plants do not raise it.
fn sea_floor<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> BlockPos {
    let min_y = ctx.world.min_y();
    let mut y = ctx.heightmap(pos.x, pos.z);
    while y > min_y && !props::blocks_motion(ctx.get(BlockPos::new(pos.x, y - 1, pos.z))) {
        y -= 1;
    }
    BlockPos::new(pos.x, y, pos.z)
}

/// Shortest and tallest worldgen kelp column, in blocks.
const KELP_MIN_HEIGHT: i32 = 3;
const KELP_MAX_HEIGHT: i32 = 25;

/// `minecraft:kelp_feature`: a kelp column from the sea floor below `pos`
/// ([`sea_floor`]). Its height is uniform in
/// `KELP_MIN_HEIGHT..=min(KELP_MAX_HEIGHT, room)`, where `room` is the number
/// of segments that still leave water above the top one. Measured in a
/// natural BDS 1.26.51 world of the fixture seed (407 kelp columns across 39
/// chunks): lengths 3..25, never shorter than 3, never above the room, and
/// uniformly spread between those bounds (mean 10.26 against the model's
/// 10.10). The legacy `2..=11` heights produced 30% too little kelp. With
/// less than three segments of room, BDS behaviour is unobserved; the column
/// then fills the room. The draw order (height, then tip age) is unmeasured.
/// The top segment is a tip aged 20..=23; stems are age 0.
pub fn kelp_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let pos = sea_floor(ctx, pos);
    if !bb::water(ctx.get(pos)) {
        return None;
    }
    let mut room = 0;
    while room < KELP_MAX_HEIGHT && bb::water(ctx.get(pos.offset(0, room + 1, 0))) {
        room += 1;
    }
    if room == 0 {
        return None;
    }
    let low = KELP_MIN_HEIGHT.min(room);
    let height = low + ctx.rng.next_int(room - low + 1);
    let extra = height - 1;
    let mut placed = false;
    let mut at = pos;
    for i in 0..=extra {
        let below = ctx.get(at.below());
        let fits = bb::water(ctx.get(at))
            && bb::water(ctx.get(at.above()))
            && (bb::kelp(below) || supports_water_plant(below));
        if !fits {
            // The column is blocked: the last stem becomes the tip.
            if i > 0 {
                let tip = KELP_TIPS[ctx.rng.next_int(4) as usize];
                ctx.set(at.below(), tip);
            }
            break;
        }
        let id = if i == extra {
            KELP_TIPS[ctx.rng.next_int(4) as usize]
        } else {
            bb::KELP_AGE_0
        };
        if !ctx.set(at, id) {
            // Left the writable region: cap what was placed so far.
            if i > 0 {
                let tip = KELP_TIPS[ctx.rng.next_int(4) as usize];
                ctx.set(at.below(), tip);
            }
            break;
        }
        placed = true;
        at = at.above();
    }
    placed.then_some(pos)
}

/// One seagrass placement on the sea floor below `pos` ([`sea_floor`]):
/// double seagrass with probability `tall_chance` (needs water above too),
/// else short seagrass. Only in water on a solid non-magma floor. The tall
/// roll comes before the floor check, as in vanilla.
fn place_seagrass<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
    tall_chance: f32,
) -> Option<BlockPos> {
    let pos = sea_floor(ctx, pos);
    if !bb::water(ctx.get(pos)) {
        return None;
    }
    let tall = ctx.rng.probability(tall_chance);
    if !supports_water_plant(ctx.get(pos.below())) {
        return None;
    }
    if !tall {
        return ctx.set(pos, bb::SEAGRASS).then_some(pos);
    }
    let top = pos.above();
    if !bb::water(ctx.get(top)) {
        return None;
    }
    // Place the upper half first so a write refused at the region edge never
    // leaves a lone bottom half.
    if !ctx.set(top, bb::SEAGRASS_DOUBLE_TOP) {
        return None;
    }
    ctx.set(pos, bb::SEAGRASS_DOUBLE_BOT);
    Some(pos)
}

/// `minecraft:seagrass_feature`: seagrass, 60 % double (swamp variant).
pub fn seagrass_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    place_seagrass(ctx, pos, SEAGRASS_TALL_CHANCE)
}

/// `minecraft:shortest_seagrass_feature`: seagrass, 30 % double (ocean,
/// cold and warm ocean variant).
pub fn shortest_seagrass_feature<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
) -> Option<BlockPos> {
    place_seagrass(ctx, pos, SHORTEST_TALL_CHANCE)
}

/// `minecraft:shorter_seagrass_feature`: seagrass, 40 % double (river
/// variant).
pub fn shorter_seagrass_feature<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
) -> Option<BlockPos> {
    place_seagrass(ctx, pos, SHORTER_TALL_CHANCE)
}

/// `minecraft:taller_seagrass_feature`: seagrass, 80 % double (deep ocean
/// variant).
pub fn taller_seagrass_feature<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
) -> Option<BlockPos> {
    place_seagrass(ctx, pos, TALLER_TALL_CHANCE)
}

// ---------------------------------------------------------------------------
// Sea pickles and lily pads
// ---------------------------------------------------------------------------

/// `minecraft:pickle_feature`: a cluster of sea pickles. 20 attempts at
/// columns offset by `rand(8) - rand(8)` on both axes (vanilla sea pickle
/// feature), each on that column's floor, in water on a solid floor, with
/// 1..=4 pickles. Returns the first placed position.
pub fn pickle_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let mut first = None;
    for _ in 0..PICKLE_ATTEMPTS {
        let x = pos.x + ctx.rng.next_int(8) - ctx.rng.next_int(8);
        let z = pos.z + ctx.rng.next_int(8) - ctx.rng.next_int(8);
        let pickle = SEA_PICKLES[ctx.rng.next_int(4) as usize];
        let y = ctx.above_top_solid(x, z);
        let at = BlockPos::new(x, y, z);
        let placed = bb::water(ctx.get(at))
            && supports_water_plant(ctx.get(at.below()))
            && ctx.set(at, pickle);
        if placed && first.is_none() {
            first = Some(at);
        }
    }
    first
}

/// `minecraft:waterlily_feature`: place a lily pad above the input's water column.
///
/// The current model climbs through at most [`WATERLILY_MAX_CLIMB`] water
/// blocks and accepts air only when the block immediately below is water.
/// Native input projection and the climb limit remain unmeasured; the
/// fluid-inclusive heightmap query does not imply an input near the pond floor.
pub fn waterlily_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let mut at = pos;
    let mut climbed = 0;
    while bb::water(ctx.get(at)) {
        if climbed == WATERLILY_MAX_CLIMB {
            return None;
        }
        at = at.above();
        climbed += 1;
    }
    if !bb::air(ctx.get(at)) || !bb::water(ctx.get(at.below())) {
        return None;
    }
    ctx.set(at, bb::WATERLILY).then_some(at)
}

// ---------------------------------------------------------------------------
// Discs
// ---------------------------------------------------------------------------

/// Legacy underwater disc: only when `pos` is water; every block matching
/// `target` within the vertical cylinder of radius `radius` and half height
/// `half_height` around `pos` becomes `block`.
fn underwater_disc<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
    radius: i32,
    half_height: i32,
    block: u32,
    target: fn(u32) -> bool,
) -> Option<BlockPos> {
    if !bb::water(ctx.get(pos)) {
        return None;
    }
    let mut placed = false;
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if !in_disc(dx, dz, radius) {
                continue;
            }
            for dy in -half_height..=half_height {
                let at = pos.offset(dx, dy, dz);
                if target(ctx.get(at)) {
                    placed |= ctx.set(at, block);
                }
            }
        }
    }
    placed.then_some(pos)
}

/// `minecraft:clay_feature`: underwater clay disc, radius 2..=3, half height
/// 1, replacing dirt and clay (vanilla legacy clay disc).
pub fn clay_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let radius = 2 + ctx.rng.next_int(2);
    underwater_disc(ctx, pos, radius, 1, bb::CLAY, |id| {
        id == bb::DIRT || id == bb::CLAY
    })
}

/// `minecraft:gravel_feature`: underwater gravel disc, radius 2..=5, half
/// height 2, replacing dirt and grass blocks (vanilla legacy gravel disc).
pub fn gravel_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let radius = 2 + ctx.rng.next_int(4);
    underwater_disc(ctx, pos, radius, 2, bb::GRAVEL, |id| {
        id == bb::DIRT || id == bb::GRASS_BLOCK
    })
}

/// `minecraft:sand_feature`: underwater sand disc, radius 2..=5, half height
/// 2, replacing dirt and grass blocks (vanilla legacy sand disc). No 1.26.51
/// rule places it; it is kept registered as a built-in.
pub fn sand_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let radius = 2 + ctx.rng.next_int(4);
    underwater_disc(ctx, pos, radius, 2, bb::SAND, |id| {
        id == bb::DIRT || id == bb::GRASS_BLOCK
    })
}

/// `minecraft:grass_disc_feature` (mangrove swamps, input above the top
/// solid block): only where that top block is mud, a disc of radius 2..=6
/// and half height 2 around it turns dirt and mud into grass where the block
/// above is neither solid nor water, and into dirt otherwise (vanilla
/// mangrove grass disc). Columns are processed top-down, so a converted top
/// block makes everything below it dirt.
pub fn grass_disc_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let centre = pos.below();
    if ctx.get(centre) != bb::MUD {
        return None;
    }
    const HALF_HEIGHT: i32 = 2;
    let radius = 2 + ctx.rng.next_int(5);
    let mut placed = false;
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if !in_disc(dx, dz, radius) {
                continue;
            }
            for dy in (-HALF_HEIGHT..=HALF_HEIGHT).rev() {
                let at = centre.offset(dx, dy, dz);
                let id = ctx.get(at);
                if id != bb::DIRT && id != bb::MUD {
                    continue;
                }
                let above = ctx.get(at.above());
                let block = if props::is_solid(above) || bb::water(above) {
                    bb::DIRT
                } else {
                    bb::GRASS_BLOCK
                };
                placed |= ctx.set(at, block);
            }
        }
    }
    placed.then_some(centre)
}

// ---------------------------------------------------------------------------
// Warm ocean reefs
// ---------------------------------------------------------------------------

/// One coral block of a reef structure (vanilla coral piece): only into water
/// or an existing coral/fan with water above. On success the block above
/// gets a random coral or fan (25 %) or else a sea pickle (5 %), and each
/// water side gets a random wall fan facing outwards (20 % per side).
/// `false` (nothing placed) stops the growing branch.
fn place_coral_block<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, block: u32) -> bool {
    let here = ctx.get(pos);
    let above = pos.above();
    if !(bb::water(here) || bb::coral_plant_or_fan(here)) || !bb::water(ctx.get(above)) {
        return false;
    }
    if !ctx.set(pos, block) {
        return false;
    }
    if ctx.rng.next_float() < 0.25 {
        let decoration = random_coral_decoration(ctx.rng);
        ctx.set(above, decoration);
    } else if ctx.rng.next_float() < 0.05 {
        let pickle = SEA_PICKLES[ctx.rng.next_int(4) as usize];
        ctx.set(above, pickle);
    }
    for dir in Direction::ALL {
        if ctx.rng.next_float() < 0.2 {
            let side = pos.side(dir);
            if bb::water(ctx.get(side)) {
                let fan = CORAL_WALL_FANS[ctx.rng.next_int(5) as usize][dir_index(dir)];
                ctx.set(side, fan);
            }
        }
    }
    true
}

/// Coral tree: a trunk of 1..=3 blocks, then 2..=4 distinct horizontal
/// branches of 2..=6 blocks each that step outwards after their first block
/// and then, after at least two upward blocks, with 25 % per block.
fn coral_tree<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, block: u32) -> bool {
    let mut at = pos;
    let trunk = ctx.rng.next_int(3) + 1;
    for _ in 0..trunk {
        if !place_coral_block(ctx, at, block) {
            return at != pos;
        }
        at = at.above();
    }
    let top = at;
    let branches = (ctx.rng.next_int(3) + 2) as usize;
    let mut dirs = Direction::ALL;
    shuffle(ctx.rng, &mut dirs);
    for &dir in &dirs[..branches] {
        let mut at = top.side(dir);
        let length = ctx.rng.next_int(5) + 2;
        let mut straight = 0;
        for n in 0..length {
            if !place_coral_block(ctx, at, block) {
                break;
            }
            straight += 1;
            at = at.above();
            if n == 0 || (straight >= 2 && ctx.rng.next_float() < 0.25) {
                at = at.side(dir);
                straight = 0;
            }
        }
    }
    true
}

/// Coral claw: a base block with 2..=3 arms among the main direction and its
/// two perpendiculars. The main arm runs 1..=2 blocks outwards; side arms
/// start one block up and run 1..=2 blocks sideways or up. Each arm then
/// curls 2..=4 (main) or 3..=5 (side) blocks along the main direction,
/// rising with 25 % per block.
fn coral_claw<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, block: u32) -> bool {
    if !place_coral_block(ctx, pos, block) {
        return false;
    }
    let main = random_direction(ctx.rng);
    let arms = (ctx.rng.next_int(2) + 2) as usize;
    let mut candidates = [main, clockwise(main), counter_clockwise(main)];
    shuffle(ctx.rng, &mut candidates);
    for &arm in &candidates[..arms] {
        let mut at = pos.side(arm);
        let base = ctx.rng.next_int(2) + 1;
        // Step of the arm base as (dx, dy, dz).
        let (step, tip) = if arm == main {
            let (dx, dz) = arm.delta();
            ((dx, 0, dz), ctx.rng.next_int(3) + 2)
        } else {
            at = at.above();
            let step = if ctx.rng.next_int(2) == 0 {
                let (dx, dz) = arm.delta();
                (dx, 0, dz)
            } else {
                (0, 1, 0)
            };
            (step, ctx.rng.next_int(3) + 3)
        };
        for _ in 0..base {
            if !place_coral_block(ctx, at, block) {
                break;
            }
            at = at.offset(step.0, step.1, step.2);
        }
        // Back to the last base block, then one up to start the curl.
        at = at.offset(-step.0, 1 - step.1, -step.2);
        for _ in 0..tip {
            at = at.side(main);
            if !place_coral_block(ctx, at, block) {
                break;
            }
            if ctx.rng.next_float() < 0.25 {
                at = at.above();
            }
        }
    }
    true
}

/// Coral mushroom: the edge-less shell of a box 4..=6 blocks on each axis,
/// extending towards +x/+z from `pos` and sunk 1..=3 blocks (the buried part
/// fails in the floor), each cell skipped with 10 %.
fn coral_mushroom<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, block: u32) -> bool {
    let sy = ctx.rng.next_int(3) + 3;
    let sx = ctx.rng.next_int(3) + 3;
    let sz = ctx.rng.next_int(3) + 3;
    let sink = ctx.rng.next_int(3) + 1;
    let mut placed = false;
    for x in 0..=sx {
        for y in 0..=sy {
            for z in 0..=sz {
                if mushroom_shell(x, y, z, sx, sy, sz) && ctx.rng.next_float() >= 0.1 {
                    placed |= place_coral_block(ctx, pos.offset(x, y - sink, z), block);
                }
            }
        }
    }
    placed
}

/// `minecraft:coral_feature`: one warm ocean coral structure of a random
/// colour on the floor at `pos`, uniformly a tree, claw or mushroom (vanilla
/// warm ocean vegetation shapes; BDS ships no document for the selection, so
/// the uniform pick is unmeasured: to confirm by BDS fixture).
pub fn coral_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let shape = ctx.rng.next_int(3);
    let block = CORAL_BLOCKS[ctx.rng.next_int(5) as usize];
    let placed = match shape {
        0 => coral_tree(ctx, pos, block),
        1 => coral_claw(ctx, pos, block),
        _ => coral_mushroom(ctx, pos, block),
    };
    placed.then_some(pos)
}

/// `minecraft:coral_crust_feature` (Bedrock-only reef decoration): a ragged
/// patch of one random coral colour, radius 2..=4, set flush into the floor.
/// Each column's floor is its own top solid block, accepted when it is sand,
/// gravel, clay or dirt with water above and within 2 blocks of `pos.y` (the
/// patch does not drape over ledges); edge columns are kept with 50 %. BDS
/// ships no parameters for this feature: the shape is inferred from observed
/// reefs (flat coral patches on the sand), unmeasured: to confirm by BDS fixture.
pub fn coral_crust_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    if !bb::water(ctx.get(pos)) || !props::is_solid(ctx.get(pos.below())) {
        return None;
    }
    let block = CORAL_BLOCKS[ctx.rng.next_int(5) as usize];
    let radius = 2 + ctx.rng.next_int(3);
    let inner = (radius - 1) * (radius - 1);
    let mut placed = false;
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if !in_disc(dx, dz, radius) {
                continue;
            }
            if dx * dx + dz * dz > inner && ctx.rng.next_int(2) == 0 {
                continue;
            }
            let (x, z) = (pos.x + dx, pos.z + dz);
            let y = ctx.above_top_solid(x, z);
            if (y - pos.y).abs() > 2 {
                continue;
            }
            let water = BlockPos::new(x, y, z);
            let floor = water.below();
            let floor_id = ctx.get(floor);
            let crustable = bb::sand_like(floor_id)
                || floor_id == bb::GRAVEL
                || floor_id == bb::CLAY
                || floor_id == bb::DIRT;
            if crustable && bb::water(ctx.get(water)) {
                placed |= ctx.set(floor, block);
            }
        }
    }
    placed.then_some(pos)
}

/// `minecraft:coral_hang_feature` (Bedrock-only reef decoration; Bedrock's
/// wall fans were once the `coral_fan_hang` blocks): walks down the coral
/// blocks below `pos` (the top of a reef column, at most 8) and gives each
/// water side a wall fan of the block's colour with 25 %. The fan count per
/// block is inferred, unmeasured: to confirm by BDS fixture.
pub fn coral_hang_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    const MAX_DEPTH: i32 = 8;
    let mut placed = None;
    for depth in 1..=MAX_DEPTH {
        let at = pos.offset(0, -depth, 0);
        let Some(colour) = coral_colour(ctx.get(at)) else {
            break;
        };
        for dir in Direction::ALL {
            if ctx.rng.next_int(4) != 0 {
                continue;
            }
            let side = at.side(dir);
            let attached =
                bb::water(ctx.get(side)) && ctx.set(side, CORAL_WALL_FANS[colour][dir_index(dir)]);
            if attached && placed.is_none() {
                placed = Some(side);
            }
        }
    }
    placed
}

/// `minecraft:sea_anemone_feature` (warm oceans, input at `y = 62`, the top
/// water layer at sea level): Bedrock 1.26 has no anemone block, so this is
/// the scattered floor decoration of warm oceans: one random coral or floor
/// fan on the floor of the input column, in water on a solid floor. Columns
/// whose floor reaches above the input (land) are skipped. The block choice
/// is inferred, unmeasured: to confirm by BDS fixture.
pub fn sea_anemone_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
    let y = ctx.above_top_solid(pos.x, pos.z);
    if y > pos.y {
        return None;
    }
    let at = BlockPos::new(pos.x, y, pos.z);
    if !bb::water(ctx.get(at)) || !supports_water_plant(ctx.get(at.below())) {
        return None;
    }
    let decoration = random_coral_decoration(ctx.rng);
    ctx.set(at, decoration).then_some(at)
}

#[cfg(test)]
mod tests {
    use super::super::super::world::{NoiseSource, OUT_OF_BOUNDS};
    use super::*;
    use std::collections::HashMap;

    struct NoNoise;

    impl NoiseSource for NoNoise {
        fn noise(&self, _x: f32, _z: f32) -> f32 {
            0.0
        }
    }

    /// Flat test world: `floor_block` up to `floor`, water up to `surface`,
    /// air above; edits recorded separately.
    struct Flat {
        floor: i32,
        surface: i32,
        floor_block: u32,
        edits: HashMap<BlockPos, u32>,
        secondary: HashMap<BlockPos, u32>,
        block_entities: Vec<crate::world::block_entity::BlockEntity>,
    }

    impl Flat {
        fn new(floor: i32, surface: i32, floor_block: u32) -> Self {
            Self {
                floor,
                surface,
                floor_block,
                edits: HashMap::new(),
                secondary: HashMap::new(),
                block_entities: Vec::new(),
            }
        }
    }

    impl BlockWorld for Flat {
        fn min_y(&self) -> i32 {
            -64
        }
        fn max_y(&self) -> i32 {
            320
        }
        fn get(&self, pos: BlockPos) -> u32 {
            if pos.y < self.min_y() || pos.y >= self.max_y() {
                return OUT_OF_BOUNDS;
            }
            if let Some(&id) = self.edits.get(&pos) {
                id
            } else if pos.y <= self.floor {
                self.floor_block
            } else if pos.y <= self.surface {
                bb::WATER
            } else {
                bb::AIR
            }
        }
        fn get_layer(&self, pos: BlockPos, layer: u8) -> u32 {
            let primary = self.get(pos);
            match layer {
                0 => primary,
                1 if primary != OUT_OF_BOUNDS => {
                    self.secondary.get(&pos).copied().unwrap_or(bb::AIR)
                }
                _ => OUT_OF_BOUNDS,
            }
        }
        fn set(&mut self, pos: BlockPos, id: u32) -> bool {
            if pos.y < self.min_y() || pos.y >= self.max_y() {
                return false;
            }
            self.edits.insert(pos, id);
            self.secondary.remove(&pos);
            true
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
            if position[1] < self.min_y() || position[1] >= self.max_y() {
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
        fn heightmap(&mut self, x: i32, z: i32) -> i32 {
            for y in (self.min_y()..self.max_y()).rev() {
                let id = self.get(BlockPos::new(x, y, z));
                if !bb::air(id) {
                    return y + 1;
                }
            }
            self.min_y()
        }
        fn above_top_solid(&mut self, x: i32, z: i32) -> i32 {
            for y in (self.min_y()..self.max_y()).rev() {
                if props::is_solid(self.get(BlockPos::new(x, y, z))) {
                    return y + 1;
                }
            }
            self.min_y()
        }
    }

    /// Run `f` on `world` with an rng seeded by `seed`.
    fn run<R>(world: &mut Flat, seed: u32, f: impl FnOnce(&mut Ctx<'_, Flat>) -> R) -> R {
        let mut rng = BedrockRandom::new(seed);
        let mut ctx = Ctx {
            world,
            rng: &mut rng,
            noise: &NoNoise,
            seed: 0,
        };
        f(&mut ctx)
    }

    #[test]
    fn mushroom_shell_drops_edges_and_interior() {
        // 3×3×3 box: only the six face centres remain.
        let mut count = 0;
        for x in 0..=2 {
            for y in 0..=2 {
                for z in 0..=2 {
                    count += mushroom_shell(x, y, z, 2, 2, 2) as i32;
                }
            }
        }
        assert_eq!(count, 6);
        assert!(!mushroom_shell(1, 1, 1, 2, 2, 2), "interior");
        assert!(!mushroom_shell(0, 0, 1, 2, 2, 2), "edge");
        assert!(!mushroom_shell(0, 0, 0, 2, 2, 2), "corner");
        // 4×5×6 box (sizes 3, 4, 5): faces minus their rims.
        let mut count = 0;
        for x in 0..=3 {
            for y in 0..=4 {
                for z in 0..=5 {
                    count += mushroom_shell(x, y, z, 3, 4, 5) as i32;
                }
            }
        }
        assert_eq!(count, 2 * (3 * 4) + 2 * (2 * 4) + 2 * (2 * 3));
    }

    #[test]
    fn disc_matches_euclidean_radius() {
        assert!(in_disc(2, 0, 2));
        assert!(in_disc(1, 1, 2));
        assert!(!in_disc(2, 1, 2));
        assert!(in_disc(3, 4, 5));
        assert!(!in_disc(4, 4, 5));
    }

    #[test]
    fn rotations_are_inverse_and_clockwise() {
        for dir in Direction::ALL {
            assert_eq!(counter_clockwise(clockwise(dir)), dir);
            assert_eq!(clockwise(clockwise(dir)), dir.opposite());
        }
        // Facing north, the right hand points east.
        assert_eq!(clockwise(Direction::North), Direction::East);
        assert_eq!(counter_clockwise(Direction::North), Direction::West);
    }

    #[test]
    fn kelp_grows_from_the_floor_with_a_tip() {
        // Water 41..=62 leaves room for 21 segments; a 120-deep sea is capped
        // at the 25-block maximum.
        for (floor, cap) in [(40, 21), (-60, KELP_MAX_HEIGHT)] {
            let mut tallest = 0;
            for seed in 0..64 {
                let mut world = Flat::new(floor, 62, bb::SAND);
                let base = BlockPos::new(0, floor + 1, 0);
                assert_eq!(
                    run(&mut world, seed, |ctx| kelp_feature(ctx, base)),
                    Some(base)
                );
                let mut top = base.y;
                while bb::kelp(world.get(BlockPos::new(0, top + 1, 0))) {
                    top += 1;
                }
                let len = top - base.y + 1;
                assert!((KELP_MIN_HEIGHT..=cap).contains(&len), "length {len}");
                tallest = tallest.max(len);
                assert!(KELP_TIPS.contains(&world.get(BlockPos::new(0, top, 0))));
                for y in base.y..top {
                    assert_eq!(world.get(BlockPos::new(0, y, 0)), bb::KELP_AGE_0);
                }
                assert_eq!(world.edits.len() as i32, len);
            }
            assert!(tallest > 11, "legacy 2..=11 heights: tallest {tallest}");
        }
    }

    #[test]
    fn kelp_stays_below_the_surface() {
        for seed in 0..64 {
            // Water at y 59..=62: the top segment needs water above, so 61.
            let mut world = Flat::new(58, 62, bb::SAND);
            let placed = run(&mut world, seed, |ctx| {
                kelp_feature(ctx, BlockPos::new(0, 59, 0))
            });
            assert!(placed.is_some());
            assert!(!bb::kelp(world.get(BlockPos::new(0, 62, 0))));
            let top = (59..=61)
                .rev()
                .find(|&y| bb::kelp(world.get(BlockPos::new(0, y, 0))))
                .unwrap();
            assert!(KELP_TIPS.contains(&world.get(BlockPos::new(0, top, 0))));
        }
        let mut land = Flat::new(70, 62, bb::SAND);
        assert!(
            run(&mut land, 1, |ctx| kelp_feature(
                ctx,
                BlockPos::new(0, 71, 0)
            ))
            .is_none()
        );
    }

    #[test]
    fn seagrass_variants_follow_their_tall_chance() {
        let pos = BlockPos::new(0, 41, 0);
        let mut shares = [0.0f32; 4];
        let chances = [
            SHORTEST_TALL_CHANCE,
            SHORTER_TALL_CHANCE,
            SEAGRASS_TALL_CHANCE,
            TALLER_TALL_CHANCE,
        ];
        let mut rng = BedrockRandom::new(7);
        for (share, &chance) in shares.iter_mut().zip(&chances) {
            let mut tall = 0;
            for _ in 0..2000 {
                let mut world = Flat::new(40, 62, bb::SAND);
                let mut ctx = Ctx {
                    world: &mut world,
                    rng: &mut rng,
                    noise: &NoNoise,
                    seed: 0,
                };
                assert_eq!(place_seagrass(&mut ctx, pos, chance), Some(pos));
                match world.get(pos) {
                    id if id == bb::SEAGRASS_DOUBLE_BOT => {
                        assert_eq!(world.get(pos.above()), bb::SEAGRASS_DOUBLE_TOP);
                        tall += 1;
                    }
                    id => assert_eq!(id, bb::SEAGRASS),
                }
            }
            *share = tall as f32 / 2000.0;
            assert!(
                (*share - chance).abs() < 0.05,
                "share {share} for chance {chance}"
            );
        }
        assert!(shares.windows(2).all(|w| w[0] < w[1]), "{shares:?}");
    }

    #[test]
    fn seagrass_needs_water_and_a_solid_floor() {
        let mut land = Flat::new(70, 62, bb::SAND);
        assert!(
            run(&mut land, 3, |ctx| seagrass_feature(
                ctx,
                BlockPos::new(0, 71, 0)
            ))
            .is_none()
        );
        // A mid-water (or above-water) input height is projected onto the
        // floor, like the BDS rules' `query.heightmap` inputs.
        let mut deep = Flat::new(30, 62, bb::SAND);
        assert_eq!(
            run(&mut deep, 3, |ctx| seagrass_feature(
                ctx,
                BlockPos::new(0, 50, 0)
            )),
            Some(BlockPos::new(0, 31, 0))
        );
        assert!(deep.edits.keys().all(|p| p.y == 31 || p.y == 32));
    }

    #[test]
    fn clay_disc_replaces_submerged_dirt_only() {
        for seed in 0..16 {
            let mut world = Flat::new(50, 62, bb::DIRT);
            let pos = BlockPos::new(0, 51, 0);
            assert_eq!(
                run(&mut world, seed, |ctx| clay_feature(ctx, pos)),
                Some(pos)
            );
            for (at, &id) in &world.edits {
                assert_eq!(id, bb::CLAY);
                assert!(in_disc(at.x, at.z, 3), "{at:?}");
                // Half height 1 around y 51 reaches only the dirt at y 50.
                assert_eq!(at.y, 50, "{at:?}");
            }
        }
        let mut dry = Flat::new(70, 62, bb::DIRT);
        assert!(
            run(&mut dry, 1, |ctx| gravel_feature(
                ctx,
                BlockPos::new(0, 71, 0)
            ))
            .is_none()
        );
    }

    #[test]
    fn gravel_disc_reaches_two_blocks_down() {
        let mut world = Flat::new(50, 62, bb::DIRT);
        let pos = BlockPos::new(0, 51, 0);
        run(&mut world, 5, |ctx| gravel_feature(ctx, pos));
        assert_eq!(world.get(BlockPos::new(0, 50, 0)), bb::GRAVEL);
        assert_eq!(world.get(BlockPos::new(0, 49, 0)), bb::GRAVEL);
        assert_eq!(world.get(BlockPos::new(0, 48, 0)), bb::DIRT);
        assert!(world.edits.keys().all(|at| in_disc(at.x, at.z, 5)));
    }

    #[test]
    fn grass_disc_turns_dry_mud_into_grass_over_dirt() {
        // Mud up to 62, air above.
        let mut world = Flat::new(62, 62, bb::MUD);
        let placed = run(&mut world, 9, |ctx| {
            grass_disc_feature(ctx, BlockPos::new(0, 63, 0))
        });
        assert!(placed.is_some());
        assert_eq!(world.get(BlockPos::new(0, 62, 0)), bb::GRASS_BLOCK);
        assert_eq!(world.get(BlockPos::new(0, 61, 0)), bb::DIRT);
        assert_eq!(world.get(BlockPos::new(0, 60, 0)), bb::DIRT);
        assert_eq!(world.get(BlockPos::new(0, 59, 0)), bb::MUD);
        // Submerged mud becomes dirt, never grass.
        let mut wet = Flat::new(60, 62, bb::MUD);
        run(&mut wet, 9, |ctx| {
            grass_disc_feature(ctx, BlockPos::new(0, 61, 0))
        });
        assert!(wet.edits.values().all(|&id| id == bb::DIRT));
        // Not on mud: nothing.
        let mut sand = Flat::new(62, 62, bb::SAND);
        assert!(
            run(&mut sand, 9, |ctx| grass_disc_feature(
                ctx,
                BlockPos::new(0, 63, 0)
            ))
            .is_none()
        );
    }

    #[test]
    fn waterlily_floats_to_the_surface() {
        let mut world = Flat::new(58, 62, bb::DIRT);
        let placed = run(&mut world, 1, |ctx| {
            waterlily_feature(ctx, BlockPos::new(0, 59, 0))
        });
        assert_eq!(placed, Some(BlockPos::new(0, 63, 0)));
        assert_eq!(world.get(BlockPos::new(0, 63, 0)), bb::WATERLILY);
        // Directly above the surface too.
        let mut world = Flat::new(58, 62, bb::DIRT);
        assert!(
            run(&mut world, 1, |ctx| waterlily_feature(
                ctx,
                BlockPos::new(0, 63, 0)
            ))
            .is_some()
        );
        // Too deep, in the ground, or on land: nothing.
        let mut deep = Flat::new(30, 62, bb::DIRT);
        assert!(
            run(&mut deep, 1, |ctx| waterlily_feature(
                ctx,
                BlockPos::new(0, 31, 0)
            ))
            .is_none()
        );
        assert!(
            run(&mut deep, 1, |ctx| waterlily_feature(
                ctx,
                BlockPos::new(0, 20, 0)
            ))
            .is_none()
        );
        let mut land = Flat::new(70, 62, bb::DIRT);
        assert!(
            run(&mut land, 1, |ctx| waterlily_feature(
                ctx,
                BlockPos::new(0, 71, 0)
            ))
            .is_none()
        );
    }

    #[test]
    fn pickles_sit_on_the_floor_in_water() {
        let mut world = Flat::new(40, 62, bb::SAND);
        let placed = run(&mut world, 11, |ctx| {
            pickle_feature(ctx, BlockPos::new(0, 41, 0))
        });
        assert!(placed.is_some());
        for (at, id) in &world.edits {
            assert!(SEA_PICKLES.contains(id));
            assert_eq!(at.y, 41);
            assert!(at.x.abs() <= 7 && at.z.abs() <= 7);
        }
    }

    #[test]
    fn coral_structures_grow_only_in_water() {
        let reef_block = |id: u32| {
            bb::coral_block(id)
                || bb::coral_plant_or_fan(id)
                || SEA_PICKLES.contains(&id)
                || CORAL_WALL_FANS.iter().any(|fans| fans.contains(&id))
        };
        let mut any = false;
        for seed in 0..48 {
            let mut world = Flat::new(40, 62, bb::SAND);
            let placed = run(&mut world, seed, |ctx| {
                coral_feature(ctx, BlockPos::new(0, 41, 0))
            });
            any |= placed.is_some();
            let mut colours = world.edits.values().filter_map(|&id| coral_colour(id));
            if let Some(first) = colours.next() {
                assert!(
                    colours.all(|c| c == first),
                    "one colour of coral block per structure"
                );
            }
            for (at, &id) in &world.edits {
                assert!(reef_block(id), "{at:?}");
                assert!(at.y > 40 && at.y <= 62, "{at:?}");
            }
        }
        assert!(any);
    }

    #[test]
    fn coral_hang_attaches_matching_fans_to_reef_sides() {
        let mut any = false;
        for seed in 0..32 {
            let mut world = Flat::new(40, 62, bb::SAND);
            for y in 41..=43 {
                world.set(BlockPos::new(0, y, 0), bb::TUBE_CORAL_BLOCK);
            }
            let before = world.edits.len();
            let placed = run(&mut world, seed, |ctx| {
                coral_hang_feature(ctx, BlockPos::new(0, 44, 0))
            });
            any |= placed.is_some();
            assert_eq!(placed.is_some(), world.edits.len() > before);
            for (at, &id) in &world.edits {
                if at.x == 0 && at.z == 0 {
                    continue;
                }
                assert!((41..=43).contains(&at.y));
                let dir = Direction::ALL
                    .into_iter()
                    .find(|d| d.delta() == (at.x, at.z))
                    .expect("adjacent");
                assert_eq!(id, CORAL_WALL_FANS[0][dir_index(dir)]);
            }
        }
        assert!(any);
    }

    #[test]
    fn sea_anemone_decorates_the_floor_below_sea_level() {
        let mut world = Flat::new(40, 62, bb::SAND);
        let placed = run(&mut world, 2, |ctx| {
            sea_anemone_feature(ctx, BlockPos::new(3, 62, 4))
        });
        assert_eq!(placed, Some(BlockPos::new(3, 41, 4)));
        assert!(bb::coral_plant_or_fan(world.get(BlockPos::new(3, 41, 4))));
        let mut land = Flat::new(70, 62, bb::SAND);
        assert!(
            run(&mut land, 2, |ctx| sea_anemone_feature(
                ctx,
                BlockPos::new(3, 62, 4)
            ))
            .is_none()
        );
    }

    #[test]
    fn coral_crust_stays_flush_with_the_floor() {
        let mut world = Flat::new(40, 62, bb::SAND);
        let placed = run(&mut world, 4, |ctx| {
            coral_crust_feature(ctx, BlockPos::new(0, 41, 0))
        });
        assert!(placed.is_some());
        let first = *world.edits.values().next().unwrap();
        for (at, &id) in &world.edits {
            assert_eq!(id, first);
            assert!(bb::coral_block(id));
            assert_eq!(at.y, 40);
            assert!(in_disc(at.x, at.z, 4));
        }
    }

    /// `minecraft:underwater_magma_underground_feature` measures depth from
    /// the top solid block, not the water surface: native seed-12345 magma
    /// never lies on the ocean floor.
    #[test]
    fn underground_magma_skips_the_ocean_floor() {
        let mut world = Flat::new(40, 62, bb::STONE);
        for seed in 0..32 {
            let placed = run(&mut world, seed, |ctx| {
                super::super::super::generated::f_underwater_magma_underground_feature(
                    ctx,
                    BlockPos::new(3, 41, 4),
                )
            });
            assert_eq!(placed, None, "seed {seed}");
        }
        assert!(world.edits.is_empty());
    }
}
