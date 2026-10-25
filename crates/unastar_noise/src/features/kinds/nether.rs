//! Nether features: `minecraft:huge_fungus`, `minecraft:nether_forest_vegetation`,
//! `minecraft:twisting_vines`, `minecraft:weeping_vines`,
//! `minecraft:netherrack_replace_blobs`, `minecraft:basalt_columns`,
//! `minecraft:basalt_pillar`, `minecraft:delta_feature` and
//! `minecraft:glowstone_blob`.

use crate::features::blocks::{BlockPos, Direction, JavaBlock, JavaState};
use crate::features::config::{
    ColumnConfig, DeltaConfig, HugeFungusConfig, NetherForestVegetationConfig, ReplaceBlobsConfig,
    TwistingVinesConfig,
};
use crate::features::eval::{FeatureWorld, provide_state, test_predicate};
use crate::features::java_blocks::{blocks as b, states, tags};
use crate::features::random::FeatureRandom;
use crate::features::survival::can_survive;

/// `Mth.nextInt(random, min, max)`: inclusive, and no draw when `min >= max`.
#[inline]
fn mth_next_int(rng: &mut FeatureRandom, min: i32, max: i32) -> i32 {
    if min >= max {
        min
    } else {
        rng.next_int_bounded(max - min + 1) + min
    }
}

/// Manhattan distance between two positions (`distManhattan`).
#[inline]
fn dist_manhattan(a: BlockPos, b: BlockPos) -> i32 {
    (a.x - b.x).abs() + (a.y - b.y).abs() + (a.z - b.z).abs()
}

/// Java `BlockPos.withinManhattan(origin, reachX, reachY, reachZ)`: positions
/// in the box ordered by increasing Manhattan distance, each `+z` position
/// immediately followed by its `-z` mirror.
struct WithinManhattan {
    origin: BlockPos,
    reach: (i32, i32, i32),
    max_depth: i32,
    depth: i32,
    max_x: i32,
    max_y: i32,
    x: i32,
    y: i32,
    cursor: BlockPos,
    z_mirror: bool,
}

impl WithinManhattan {
    fn new(origin: BlockPos, rx: i32, ry: i32, rz: i32) -> Self {
        Self {
            origin,
            reach: (rx, ry, rz),
            max_depth: rx + ry + rz,
            depth: 0,
            max_x: 0,
            max_y: 0,
            x: 0,
            y: 0,
            cursor: origin,
            z_mirror: false,
        }
    }
}

impl Iterator for WithinManhattan {
    type Item = BlockPos;

    fn next(&mut self) -> Option<BlockPos> {
        if self.z_mirror {
            self.z_mirror = false;
            self.cursor.z = self.origin.z - (self.cursor.z - self.origin.z);
            return Some(self.cursor);
        }
        let (rx, ry, rz) = self.reach;
        loop {
            if self.y > self.max_y {
                self.x += 1;
                if self.x > self.max_x {
                    self.depth += 1;
                    if self.depth > self.max_depth {
                        return None;
                    }
                    self.max_x = rx.min(self.depth);
                    self.x = -self.max_x;
                }
                self.max_y = ry.min(self.depth - self.x.abs());
                self.y = -self.max_y;
            }
            let (xx, yy) = (self.x, self.y);
            let zz = self.depth - xx.abs() - yy.abs();
            self.y += 1;
            if zz <= rz {
                self.z_mirror = zz != 0;
                self.cursor = self.origin.offset(xx, yy, zz);
                return Some(self.cursor);
            }
        }
    }
}

// ---- huge_fungus ----------------------------------------------------------

/// Java `HugeFungusFeature.place` (`minecraft:huge_fungus`).
///
/// Needs `valid_base_block` below the origin. Unplanted fungi fail when the
/// top would reach `gen_depth` and become 3x3 "huge" fungi 6% of the time.
/// Planted fungi (bonemeal) clear non-replaceable blocks they grow through;
/// item drops of those broken blocks are not modelled.
pub fn place_huge_fungus<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &HugeFungusConfig,
    origin: BlockPos,
) -> bool {
    if !world
        .get_state(origin.below())
        .is(cfg.valid_base_block.block())
    {
        return false;
    }
    let mut height = mth_next_int(rng, 4, 13);
    if rng.next_int_bounded(12) == 0 {
        height *= 2;
    }
    if !cfg.planted && origin.y + height + 1 >= world.gen_depth() {
        return false;
    }
    let huge = !cfg.planted && rng.next_float() < 0.06;
    world.set_state(origin, states::AIR);
    fungus_stem(world, rng, cfg, origin, height, huge);
    fungus_hat(world, rng, cfg, origin, height, huge);
    true
}

/// `HugeFungusFeature.isReplaceable`.
#[inline]
fn fungus_replaceable<W: FeatureWorld + ?Sized>(
    world: &mut W,
    pos: BlockPos,
    cfg: &HugeFungusConfig,
    check_config: bool,
) -> bool {
    world.get_state(pos).can_be_replaced()
        || (check_config && test_predicate(world, &cfg.replaceable_blocks, pos))
}

fn fungus_stem<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &HugeFungusConfig,
    origin: BlockPos,
    height: i32,
    huge: bool,
) {
    let r: i32 = if huge { 1 } else { 0 };
    for dx in -r..=r {
        for dz in -r..=r {
            let corner = huge && dx.abs() == r && dz.abs() == r;
            for dy in 0..height {
                let pos = origin.offset(dx, dy, dz);
                if !fungus_replaceable(world, pos, cfg, true) {
                    continue;
                }
                if cfg.planted {
                    if !world.get_state(pos.below()).is_air() {
                        world.set_state(pos, states::AIR);
                    }
                    world.set_state(pos, cfg.stem_state);
                } else if corner {
                    if rng.next_float() < 0.1 {
                        world.set_state(pos, cfg.stem_state);
                    }
                } else {
                    world.set_state(pos, cfg.stem_state);
                }
            }
        }
    }
}

fn fungus_hat<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &HugeFungusConfig,
    origin: BlockPos,
    height: i32,
    huge: bool,
) {
    let vines = cfg.hat_state.is(b::NETHER_WART_BLOCK);
    let hat_height = (rng.next_int_bounded(1 + height / 3) + 5).min(height);
    let hat_start = height - hat_height;
    for dy in hat_start..=height {
        let mut radius = if dy < height - rng.next_int_bounded(3) {
            2
        } else {
            1
        };
        if hat_height > 8 && dy < hat_start + 4 {
            radius = 3;
        }
        if huge {
            radius += 1;
        }
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let edge_x = dx == -radius || dx == radius;
                let edge_z = dz == -radius || dz == radius;
                let inner = !edge_x && !edge_z && dy != height;
                let corner = edge_x && edge_z;
                let low_hat = dy < hat_start + 3;
                let pos = origin.offset(dx, dy, dz);
                if !fungus_replaceable(world, pos, cfg, false) {
                    continue;
                }
                if cfg.planted && !world.get_state(pos.below()).is_air() {
                    world.set_state(pos, states::AIR);
                }
                if low_hat {
                    if !inner {
                        hat_drop_block(world, rng, pos, cfg.hat_state, vines);
                    }
                } else if inner {
                    hat_block(
                        world,
                        rng,
                        cfg,
                        pos,
                        0.1,
                        0.2,
                        if vines { 0.1 } else { 0.0 },
                    );
                } else if corner {
                    hat_block(
                        world,
                        rng,
                        cfg,
                        pos,
                        0.01,
                        0.7,
                        if vines { 0.083 } else { 0.0 },
                    );
                } else {
                    hat_block(
                        world,
                        rng,
                        cfg,
                        pos,
                        5.0e-4,
                        0.98,
                        if vines { 0.07 } else { 0.0 },
                    );
                }
            }
        }
    }
}

fn hat_block<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &HugeFungusConfig,
    pos: BlockPos,
    decor_chance: f32,
    hat_chance: f32,
    vines_chance: f32,
) {
    if rng.next_float() < decor_chance {
        world.set_state(pos, cfg.decor_state);
    } else if rng.next_float() < hat_chance {
        world.set_state(pos, cfg.hat_state);
        if rng.next_float() < vines_chance {
            fungus_weeping_vines(world, rng, pos);
        }
    }
}

fn hat_drop_block<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    pos: BlockPos,
    hat: JavaState,
    vines: bool,
) {
    if world.get_state(pos.below()).is(hat.block()) {
        world.set_state(pos, hat);
    } else if (rng.next_float() as f64) < 0.15 {
        world.set_state(pos, hat);
        if vines && rng.next_int_bounded(11) == 0 {
            fungus_weeping_vines(world, rng, pos);
        }
    }
}

/// `HugeFungusFeature.tryPlaceWeepingVines`.
fn fungus_weeping_vines<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    hat: BlockPos,
) {
    let pos = hat.below();
    if !world.is_air(pos) {
        return;
    }
    let mut max = mth_next_int(rng, 1, 5);
    if rng.next_int_bounded(7) == 0 {
        max *= 2;
    }
    weeping_vines_column(world, rng, pos, max, 23, 25);
}

// ---- weeping / twisting vines ---------------------------------------------

/// `WeepingVinesFeature.placeWeepingVinesColumn`: grows down from `pos`, up to
/// `max_height + 1` blocks, ending in a head with a random age.
fn weeping_vines_column<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    mut pos: BlockPos,
    max_height: i32,
    min_age: i32,
    max_age: i32,
) {
    for i in 0..=max_height {
        if world.is_air(pos) {
            if i == max_height || !world.is_air(pos.below()) {
                let age = mth_next_int(rng, min_age, max_age);
                world.set_state(pos, states::WEEPING_VINES.with_int("age", age));
                break;
            }
            world.set_state(pos, states::WEEPING_VINES_PLANT);
        }
        pos = pos.below();
    }
}

#[inline]
fn is_netherrack_or_wart(s: JavaState) -> bool {
    s.is(b::NETHERRACK) || s.is(b::NETHER_WART_BLOCK)
}

/// Java `WeepingVinesFeature.place` (`minecraft:weeping_vines`).
///
/// Needs an air origin under netherrack or nether wart block; grows a patch
/// of nether wart blocks on the ceiling and hangs weeping vines from it.
pub fn place_weeping_vines<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    origin: BlockPos,
) -> bool {
    if !world.is_air(origin) || !is_netherrack_or_wart(world.get_state(origin.above())) {
        return false;
    }
    // placeRoofNetherWart
    world.set_state(origin, states::NETHER_WART_BLOCK);
    for _ in 0..200 {
        let dx = rng.next_int_bounded(6) - rng.next_int_bounded(6);
        let dy = rng.next_int_bounded(2) - rng.next_int_bounded(5);
        let dz = rng.next_int_bounded(6) - rng.next_int_bounded(6);
        let pos = origin.offset(dx, dy, dz);
        if !world.is_air(pos) {
            continue;
        }
        let mut count = 0;
        for dir in Direction::ALL {
            if is_netherrack_or_wart(world.get_state(pos.rel(dir))) {
                count += 1;
            }
            if count > 1 {
                break;
            }
        }
        if count == 1 {
            world.set_state(pos, states::NETHER_WART_BLOCK);
        }
    }
    // placeRoofWeepingVines
    for _ in 0..100 {
        let dx = rng.next_int_bounded(8) - rng.next_int_bounded(8);
        let dy = rng.next_int_bounded(2) - rng.next_int_bounded(7);
        let dz = rng.next_int_bounded(8) - rng.next_int_bounded(8);
        let pos = origin.offset(dx, dy, dz);
        if !world.is_air(pos) || !is_netherrack_or_wart(world.get_state(pos.above())) {
            continue;
        }
        let mut max = mth_next_int(rng, 1, 8);
        if rng.next_int_bounded(6) == 0 {
            max *= 2;
        }
        if rng.next_int_bounded(5) == 0 {
            max = 1;
        }
        weeping_vines_column(world, rng, pos, max, 17, 25);
    }
    true
}

/// `TwistingVinesFeature.isInvalidPlacementLocation`.
fn twisting_invalid<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos) -> bool {
    if !world.is_air(pos) {
        return true;
    }
    let below = world.get_state(pos.below());
    !below.is(b::NETHERRACK) && !below.is(b::WARPED_NYLIUM) && !below.is(b::WARPED_WART_BLOCK)
}

/// Java `TwistingVinesFeature.place` (`minecraft:twisting_vines`).
///
/// Needs an air origin on netherrack, warped nylium or warped wart block.
/// Tries `spread_width²` random positions, drops each to the ground and grows
/// a twisting vines column up to `max_height` (sometimes doubled or cut to 1).
pub fn place_twisting_vines<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &TwistingVinesConfig,
    origin: BlockPos,
) -> bool {
    if twisting_invalid(world, origin) {
        return false;
    }
    let (w, h) = (cfg.spread_width, cfg.spread_height);
    for _ in 0..w * w {
        let dx = mth_next_int(rng, -w, w);
        let dy = mth_next_int(rng, -h, h);
        let dz = mth_next_int(rng, -w, w);
        let mut pos = origin.offset(dx, dy, dz);
        // findFirstAirBlockAboveGround
        let found = loop {
            pos = pos.below();
            if world.is_outside_build_height(pos.y) {
                break false;
            }
            if !world.get_state(pos).is_air() {
                pos = pos.above();
                break true;
            }
        };
        if !found || twisting_invalid(world, pos) {
            continue;
        }
        let mut height = mth_next_int(rng, 1, cfg.max_height);
        if rng.next_int_bounded(6) == 0 {
            height *= 2;
        }
        if rng.next_int_bounded(5) == 0 {
            height = 1;
        }
        // placeWeepingVinesColumn (twisting variant, growing up).
        for i in 1..=height {
            if world.is_air(pos) {
                if i == height || !world.is_air(pos.above()) {
                    let age = mth_next_int(rng, 17, 25);
                    world.set_state(pos, states::TWISTING_VINES.with_int("age", age));
                    break;
                }
                world.set_state(pos, states::TWISTING_VINES_PLANT);
            }
            pos = pos.above();
        }
    }
    true
}

// ---- nether_forest_vegetation ---------------------------------------------

/// Java `NetherForestVegetationFeature.place` (`minecraft:nether_forest_vegetation`).
///
/// Needs `#nylium` below the origin. Scatters `spread_width²` states from the
/// provider into air blocks where they can survive; `true` if any was placed.
pub fn place_nether_forest_vegetation<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &NetherForestVegetationConfig,
    origin: BlockPos,
) -> bool {
    if !world.get_state(origin.below()).is_in(tags::NYLIUM) {
        return false;
    }
    let y = origin.y;
    if !(y > world.min_y() && y + 1 < world.max_y()) {
        return false;
    }
    let (w, h) = (cfg.spread_width, cfg.spread_height);
    let mut placed = 0;
    for _ in 0..w * w {
        let dx = rng.next_int_bounded(w) - rng.next_int_bounded(w);
        let dy = rng.next_int_bounded(h) - rng.next_int_bounded(h);
        let dz = rng.next_int_bounded(w) - rng.next_int_bounded(w);
        let pos = origin.offset(dx, dy, dz);
        let state = provide_state(&cfg.state_provider, rng, pos);
        if world.is_air(pos) && pos.y > world.min_y() && can_survive(world, state, pos) {
            world.set_state(pos, state);
            placed += 1;
        }
    }
    placed > 0
}

// ---- netherrack_replace_blobs ---------------------------------------------

/// Java `ReplaceBlobsFeature.place` (`minecraft:netherrack_replace_blobs`).
///
/// Searches down from the origin (clamped into the build range) for the
/// target block, then replaces every target block within the random
/// ellipsoid-ish Manhattan blob around it.
pub fn place_netherrack_replace_blobs<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &ReplaceBlobsConfig,
    origin: BlockPos,
) -> bool {
    let target = cfg.target.block();
    let mut cursor = origin.with_y(origin.y.clamp(world.min_y() + 1, world.max_y() - 1));
    let start = loop {
        if cursor.y <= world.min_y() + 1 {
            return false;
        }
        if world.get_state(cursor).is(target) {
            break cursor;
        }
        cursor = cursor.below();
    };
    let rx = cfg.radius.sample(rng);
    let ry = cfg.radius.sample(rng);
    let rz = cfg.radius.sample(rng);
    let max_radius = rx.max(ry.max(rz));
    let mut replaced = false;
    for pos in WithinManhattan::new(start, rx, ry, rz) {
        if dist_manhattan(pos, start) > max_radius {
            break;
        }
        if world.get_state(pos).is(target) {
            world.set_state(pos, cfg.state);
            replaced = true;
        }
    }
    replaced
}

// ---- basalt_columns -------------------------------------------------------

/// `BasaltColumnsFeature.CANNOT_PLACE_ON`.
const COLUMN_CANNOT_PLACE_ON: [JavaBlock; 10] = [
    b::LAVA,
    b::BEDROCK,
    b::MAGMA_BLOCK,
    b::SOUL_SAND,
    b::NETHER_BRICKS,
    b::NETHER_BRICK_FENCE,
    b::NETHER_BRICK_STAIRS,
    b::NETHER_WART,
    b::CHEST,
    b::SPAWNER,
];

#[inline]
fn is_air_or_lava_ocean<W: FeatureWorld + ?Sized>(
    world: &mut W,
    sea_level: i32,
    pos: BlockPos,
) -> bool {
    let s = world.get_state(pos);
    s.is_air() || (s.is(b::LAVA) && pos.y <= sea_level)
}

/// `BasaltColumnsFeature.canPlaceAt`.
fn column_can_place_at<W: FeatureWorld + ?Sized>(
    world: &mut W,
    sea_level: i32,
    pos: BlockPos,
) -> bool {
    if !is_air_or_lava_ocean(world, sea_level, pos) {
        return false;
    }
    let below = world.get_state(pos.below());
    !below.is_air() && !COLUMN_CANNOT_PLACE_ON.contains(&below.block())
}

/// Java `BasaltColumnsFeature.place` (`minecraft:basalt_columns`).
///
/// Needs an air (or lava-ocean) origin on a solid-ish block. Picks clustered
/// (90%, 50 columns within 5) or scattered (15 within 8) column positions;
/// each column spreads over `reach` and stacks basalt up to the remaining
/// height.
pub fn place_basalt_columns<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &ColumnConfig,
    origin: BlockPos,
) -> bool {
    let sea_level = world.sea_level();
    if !column_can_place_at(world, sea_level, origin) {
        return false;
    }
    let column_height = cfg.height.sample(rng);
    let clustered = rng.next_float() < 0.9;
    let reach = column_height.min(if clustered { 5 } else { 8 });
    let size = if clustered { 50 } else { 15 };
    let mut placed = false;
    // `BlockPos.randomBetweenClosed` draws each position lazily.
    let width = 2 * reach + 1;
    for _ in 0..size {
        let x = origin.x - reach + rng.next_int_bounded(width);
        let y = origin.y + rng.next_int_bounded(1);
        let z = origin.z - reach + rng.next_int_bounded(width);
        let pos = BlockPos::new(x, y, z);
        let height_at = column_height - dist_manhattan(pos, origin);
        if height_at >= 0 {
            let r = cfg.reach.sample(rng);
            placed |= basalt_column(world, sea_level, pos, height_at, r);
        }
    }
    placed
}

fn basalt_column<W: FeatureWorld + ?Sized>(
    world: &mut W,
    sea_level: i32,
    origin: BlockPos,
    column_height: i32,
    reach: i32,
) -> bool {
    let mut placed = false;
    for z in origin.z - reach..=origin.z + reach {
        for x in origin.x - reach..=origin.x + reach {
            let pos = BlockPos::new(x, origin.y, z);
            let distance = dist_manhattan(pos, origin);
            let start = if is_air_or_lava_ocean(world, sea_level, pos) {
                column_find_surface(world, sea_level, pos, distance)
            } else {
                column_find_air(world, pos, distance)
            };
            let Some(mut cursor) = start else {
                continue;
            };
            let mut height = column_height - distance / 2;
            while height >= 0 {
                if is_air_or_lava_ocean(world, sea_level, cursor) {
                    world.set_state(cursor, states::BASALT);
                    cursor = cursor.above();
                    placed = true;
                } else {
                    if !world.get_state(cursor).is(b::BASALT) {
                        break;
                    }
                    cursor = cursor.above();
                }
                height -= 1;
            }
        }
    }
    placed
}

fn column_find_surface<W: FeatureWorld + ?Sized>(
    world: &mut W,
    sea_level: i32,
    mut cursor: BlockPos,
    mut limit: i32,
) -> Option<BlockPos> {
    while cursor.y > world.min_y() + 1 && limit > 0 {
        limit -= 1;
        if column_can_place_at(world, sea_level, cursor) {
            return Some(cursor);
        }
        cursor = cursor.below();
    }
    None
}

fn column_find_air<W: FeatureWorld + ?Sized>(
    world: &mut W,
    mut cursor: BlockPos,
    mut limit: i32,
) -> Option<BlockPos> {
    while cursor.y < world.max_y() && limit > 0 {
        limit -= 1;
        let s = world.get_state(cursor);
        if COLUMN_CANNOT_PLACE_ON.contains(&s.block()) {
            return None;
        }
        if s.is_air() {
            return Some(cursor);
        }
        cursor = cursor.above();
    }
    None
}

// ---- basalt_pillar --------------------------------------------------------

/// Java `BasaltPillarFeature.place` (`minecraft:basalt_pillar`).
///
/// Needs an air origin under a non-air block. Fills basalt down to the
/// ground with hanging side blocks, then turns the ground around the base
/// into a random basalt patch (sinking up to 3 blocks into overhangs).
pub fn place_basalt_pillar<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    origin: BlockPos,
) -> bool {
    if !world.is_air(origin) || world.is_air(origin.above()) {
        return false;
    }
    let mut pos = origin;
    let (mut north, mut south, mut west, mut east) = (true, true, true, true);
    while world.is_air(pos) {
        if world.is_outside_build_height(pos.y) {
            return true;
        }
        world.set_state(pos, states::BASALT);
        north = north && hang_off(world, rng, pos.rel(Direction::North));
        south = south && hang_off(world, rng, pos.rel(Direction::South));
        west = west && hang_off(world, rng, pos.rel(Direction::West));
        east = east && hang_off(world, rng, pos.rel(Direction::East));
        pos = pos.below();
    }
    pos = pos.above();
    for dir in [
        Direction::North,
        Direction::South,
        Direction::West,
        Direction::East,
    ] {
        if rng.next_bool() {
            world.set_state(pos.rel(dir), states::BASALT);
        }
    }
    pos = pos.below();
    for dx in -3i32..4 {
        for dz in -3i32..4 {
            let probability = dx.abs() * dz.abs();
            if rng.next_int_bounded(10) >= 10 - probability {
                continue;
            }
            let mut base = pos.offset(dx, 0, dz);
            let mut max_depth = 3;
            while world.is_air(base.below()) {
                base = base.below();
                max_depth -= 1;
                if max_depth <= 0 {
                    break;
                }
            }
            if !world.is_air(base.below()) {
                world.set_state(base, states::BASALT);
            }
        }
    }
    true
}

/// `BasaltPillarFeature.placeHangOff`.
#[inline]
fn hang_off<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    pos: BlockPos,
) -> bool {
    if rng.next_int_bounded(10) != 0 {
        world.set_state(pos, states::BASALT);
        true
    } else {
        false
    }
}

// ---- delta_feature --------------------------------------------------------

/// `DeltaFeature.CANNOT_REPLACE`.
const DELTA_CANNOT_REPLACE: [JavaBlock; 7] = [
    b::BEDROCK,
    b::NETHER_BRICKS,
    b::NETHER_BRICK_FENCE,
    b::NETHER_BRICK_STAIRS,
    b::NETHER_WART,
    b::CHEST,
    b::SPAWNER,
];

/// `DeltaFeature.isClear`: not already contents, not protected, open only
/// above.
fn delta_clear<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos, cfg: &DeltaConfig) -> bool {
    let s = world.get_state(pos);
    if s.is(cfg.contents.block()) || DELTA_CANNOT_REPLACE.contains(&s.block()) {
        return false;
    }
    for dir in Direction::ALL {
        let air = world.get_state(pos.rel(dir)).is_air();
        if (air && dir != Direction::Up) || (!air && dir == Direction::Up) {
            return false;
        }
    }
    true
}

/// Java `DeltaFeature.place` (`minecraft:delta_feature`).
///
/// A flat Manhattan disc of `contents` (lava/water pools in basalt deltas),
/// each block only where the surface is enclosed except above, optionally
/// with a `rim` placed at the unshifted position.
pub fn place_delta<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &DeltaConfig,
    origin: BlockPos,
) -> bool {
    let mut placed = false;
    let rim_enabled = rng.next_double() < 0.9;
    let rim_x = if rim_enabled {
        cfg.rim_size.sample(rng)
    } else {
        0
    };
    let rim_z = if rim_enabled {
        cfg.rim_size.sample(rng)
    } else {
        0
    };
    let place_rim = rim_enabled && rim_x != 0 && rim_z != 0;
    let radius_x = cfg.size.sample(rng);
    let radius_z = cfg.size.sample(rng);
    let limit = radius_x.max(radius_z);
    for pos in WithinManhattan::new(origin, radius_x, 0, radius_z) {
        if dist_manhattan(pos, origin) > limit {
            break;
        }
        if delta_clear(world, pos, cfg) {
            if place_rim {
                placed = true;
                world.set_state(pos, cfg.rim);
            }
            let contents_pos = pos.offset(rim_x, 0, rim_z);
            if delta_clear(world, contents_pos, cfg) {
                placed = true;
                world.set_state(contents_pos, cfg.contents);
            }
        }
    }
    placed
}

// ---- glowstone_blob -------------------------------------------------------

/// Java `GlowstoneFeature.place` (`minecraft:glowstone_blob`).
///
/// Needs an air origin under netherrack, basalt or blackstone; grows a
/// hanging glowstone cluster where each new block touches exactly one
/// existing glowstone.
pub fn place_glowstone_blob<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    origin: BlockPos,
) -> bool {
    if !world.is_air(origin) {
        return false;
    }
    let above = world.get_state(origin.above());
    if !above.is(b::NETHERRACK) && !above.is(b::BASALT) && !above.is(b::BLACKSTONE) {
        return false;
    }
    world.set_state(origin, states::GLOWSTONE);
    for _ in 0..1500 {
        let dx = rng.next_int_bounded(8) - rng.next_int_bounded(8);
        let dy = -rng.next_int_bounded(12);
        let dz = rng.next_int_bounded(8) - rng.next_int_bounded(8);
        let pos = origin.offset(dx, dy, dz);
        if !world.get_state(pos).is_air() {
            continue;
        }
        let mut count = 0;
        for dir in Direction::ALL {
            if world.get_state(pos.rel(dir)).is(b::GLOWSTONE) {
                count += 1;
            }
            if count > 1 {
                break;
            }
        }
        if count == 1 {
            world.set_state(pos, states::GLOWSTONE);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::blocks::{BlockPredicate, StateProvider};
    use crate::features::test_world::TestWorld;
    use crate::features::values::IntProvider;

    #[test]
    fn within_manhattan_order_and_coverage() {
        let o = BlockPos::new(0, 0, 0);
        let all: Vec<BlockPos> = WithinManhattan::new(o, 2, 1, 2).collect();
        assert_eq!(all[0], o);
        // Every box position appears exactly once, in non-decreasing distance.
        assert_eq!(all.len(), 5 * 3 * 5);
        let mut sorted = all.clone();
        sorted.sort_by_key(|p| (p.x, p.y, p.z));
        sorted.dedup();
        assert_eq!(sorted.len(), all.len());
        for w in all.windows(2) {
            assert!(dist_manhattan(w[0], o) <= dist_manhattan(w[1], o));
        }
        // Mirrors follow their +z twin.
        let i = all
            .iter()
            .position(|p| *p == BlockPos::new(0, 0, 1))
            .unwrap();
        assert_eq!(all[i + 1], BlockPos::new(0, 0, -1));
    }

    fn nether_world() -> TestWorld {
        let mut w = TestWorld::flat(32);
        w.fill = states::NETHERRACK;
        w.gen_depth = 128;
        w
    }

    fn crimson() -> HugeFungusConfig {
        HugeFungusConfig {
            valid_base_block: states::CRIMSON_NYLIUM,
            stem_state: states::CRIMSON_STEM,
            hat_state: states::NETHER_WART_BLOCK,
            decor_state: states::SHROOMLIGHT,
            replaceable_blocks: BlockPredicate::MatchingBlocks {
                set: tags::REPLACEABLE_BY_TREES,
                offset: [0, 0, 0],
            },
            planted: false,
        }
    }

    #[test]
    fn huge_fungus_needs_base_and_room() {
        let mut w = nether_world();
        let mut rng = FeatureRandom::new(4);
        assert!(!place_huge_fungus(
            &mut w,
            &mut rng,
            &crimson(),
            BlockPos::new(0, 32, 0)
        ));

        // Too close to the generation depth (128): 124 + 4 + 1 >= 128.
        w.blocks
            .insert(BlockPos::new(0, 123, 0), states::CRIMSON_NYLIUM);
        assert!(!place_huge_fungus(
            &mut w,
            &mut rng,
            &crimson(),
            BlockPos::new(0, 124, 0)
        ));

        let mut w = nether_world();
        w.blocks
            .insert(BlockPos::new(0, 31, 0), states::CRIMSON_NYLIUM);
        let mut rng = FeatureRandom::new(4);
        assert!(place_huge_fungus(
            &mut w,
            &mut rng,
            &crimson(),
            BlockPos::new(0, 32, 0)
        ));
        let stems: Vec<_> = w
            .blocks
            .iter()
            .filter(|(_, s)| s.is(b::CRIMSON_STEM))
            .collect();
        assert!(stems.len() >= 4);
        assert!(w.count(states::NETHER_WART_BLOCK) > 0);
    }

    #[test]
    fn weeping_vines_hang_from_roof() {
        let mut w = TestWorld::flat(-64);
        for x in -10..=10 {
            for z in -10..=10 {
                for y in 50..55 {
                    w.blocks.insert(BlockPos::new(x, y, z), states::NETHERRACK);
                }
            }
        }
        let mut rng = FeatureRandom::new(8);
        assert!(place_weeping_vines(
            &mut w,
            &mut rng,
            BlockPos::new(0, 49, 0)
        ));
        let blocks = w.blocks.clone();
        for (pos, s) in &blocks {
            if s.is(b::WEEPING_VINES) || s.is(b::WEEPING_VINES_PLANT) {
                assert!(can_survive(&mut w, *s, *pos), "vine at {pos:?} unsupported");
            }
        }
        assert!(w.count(states::NETHER_WART_BLOCK) > 0);
        assert!(!place_weeping_vines(
            &mut w,
            &mut rng,
            BlockPos::new(0, 20, 0)
        ));
    }

    #[test]
    fn twisting_vines_grow_up() {
        let mut w = nether_world();
        let mut rng = FeatureRandom::new(2);
        let cfg = TwistingVinesConfig {
            spread_width: 8,
            spread_height: 4,
            max_height: 8,
        };
        assert!(place_twisting_vines(
            &mut w,
            &mut rng,
            &cfg,
            BlockPos::new(0, 32, 0)
        ));
        let mut heads = 0;
        for (pos, s) in &w.blocks.clone() {
            if s.is(b::TWISTING_VINES) {
                heads += 1;
                let age = s.int_value("age").unwrap();
                assert!((17..=25).contains(&age));
            }
            if s.is(b::TWISTING_VINES) || s.is(b::TWISTING_VINES_PLANT) {
                assert!(can_survive(&mut w, *s, *pos));
            }
        }
        assert!(heads > 0);
    }

    #[test]
    fn replace_blobs_only_replaces_target() {
        let mut w = nether_world();
        let mut rng = FeatureRandom::new(6);
        let cfg = ReplaceBlobsConfig {
            target: states::NETHERRACK,
            state: states::BLACKSTONE,
            radius: IntProvider::Uniform { min: 3, max: 7 },
        };
        assert!(place_netherrack_replace_blobs(
            &mut w,
            &mut rng,
            &cfg,
            BlockPos::new(0, 60, 0)
        ));
        assert!(w.count(states::BLACKSTONE) > 0);
        for (pos, s) in &w.blocks {
            assert_eq!(*s, states::BLACKSTONE);
            assert!(pos.y < 32);
        }
    }

    #[test]
    fn delta_pools_only_in_enclosed_surface() {
        let mut w = nether_world();
        w.fill = states::BASALT;
        let mut rng = FeatureRandom::new(1);
        let cfg = DeltaConfig {
            contents: states::LAVA,
            rim: states::MAGMA_BLOCK,
            size: IntProvider::Uniform { min: 3, max: 7 },
            rim_size: IntProvider::Uniform { min: 0, max: 2 },
        };
        let placed = place_delta(&mut w, &mut rng, &cfg, BlockPos::new(0, 31, 0));
        assert_eq!(placed, !w.blocks.is_empty());
        for pos in w.blocks.keys() {
            assert_eq!(pos.y, 31);
        }
    }

    #[test]
    fn basalt_columns_and_pillar() {
        let mut w = nether_world();
        let mut rng = FeatureRandom::new(3);
        let cfg = ColumnConfig {
            reach: IntProvider::Uniform { min: 0, max: 3 },
            height: IntProvider::Uniform { min: 1, max: 4 },
        };
        assert!(place_basalt_columns(
            &mut w,
            &mut rng,
            &cfg,
            BlockPos::new(0, 32, 0)
        ));
        for (pos, s) in &w.blocks {
            assert_eq!(*s, states::BASALT);
            assert!(pos.y >= 32);
        }
        assert!(!place_basalt_columns(
            &mut w,
            &mut rng,
            &cfg,
            BlockPos::new(0, 10, 0)
        ));

        let mut w = nether_world();
        w.blocks.insert(BlockPos::new(0, 41, 0), states::NETHERRACK);
        assert!(place_basalt_pillar(
            &mut w,
            &mut rng,
            BlockPos::new(0, 40, 0)
        ));
        for y in 32..=40 {
            assert_eq!(w.blocks[&BlockPos::new(0, y, 0)], states::BASALT);
        }
        assert!(!place_basalt_pillar(
            &mut w,
            &mut rng,
            BlockPos::new(5, 40, 5)
        ));
    }

    #[test]
    fn glowstone_is_connected() {
        let mut w = TestWorld::flat(-64);
        w.blocks.insert(BlockPos::new(0, 51, 0), states::NETHERRACK);
        let mut rng = FeatureRandom::new(10);
        assert!(place_glowstone_blob(
            &mut w,
            &mut rng,
            BlockPos::new(0, 50, 0)
        ));
        let n = w.count(states::GLOWSTONE);
        assert!(n > 1);
        for (pos, s) in &w.blocks.clone() {
            if s.is(b::GLOWSTONE) && *pos != BlockPos::new(0, 50, 0) {
                assert!(
                    Direction::ALL
                        .iter()
                        .any(|d| w.get_state(pos.rel(*d)).is(b::GLOWSTONE))
                );
            }
        }
        assert!(!place_glowstone_blob(
            &mut w,
            &mut rng,
            BlockPos::new(0, 20, 0)
        ));
    }

    #[test]
    fn forest_vegetation_needs_nylium() {
        let mut w = nether_world();
        let mut rng = FeatureRandom::new(5);
        let cfg = NetherForestVegetationConfig {
            state_provider: StateProvider::Simple(states::CRIMSON_ROOTS),
            spread_width: 8,
            spread_height: 4,
        };
        assert!(!place_nether_forest_vegetation(
            &mut w,
            &mut rng,
            &cfg,
            BlockPos::new(0, 32, 0)
        ));
        for x in -8..=8 {
            for z in -8..=8 {
                w.blocks
                    .insert(BlockPos::new(x, 31, z), states::CRIMSON_NYLIUM);
            }
        }
        assert!(place_nether_forest_vegetation(
            &mut w,
            &mut rng,
            &cfg,
            BlockPos::new(0, 32, 0)
        ));
        for (pos, s) in &w.blocks {
            if s.is(b::CRIMSON_ROOTS) {
                assert_eq!(pos.y, 32);
            }
        }
    }
}
