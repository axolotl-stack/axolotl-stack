//! Dripstone features: `minecraft:dripstone_cluster`, `minecraft:large_dripstone`
//! and `minecraft:pointed_dripstone`, with Java's shared dripstone helpers
//! (pointed dripstone columns, dripstone block placement, column scans).

use crate::features::blocks::{BlockPos, Direction, Heightmap, JavaState};
use crate::features::config::{
    DripstoneClusterConfig, LargeDripstoneConfig, PointedDripstoneConfig,
};
use crate::features::eval::{FeatureWorld, clamped_map};
use crate::features::java_blocks::{blocks as b, states, tags};
use crate::features::random::FeatureRandom;
use crate::features::values::FloatProvider;

// ---- Mth trigonometry -----------------------------------------------------

/// `Mth.sin(double)`: Java's 65536-entry sine table, entry `i` being
/// `(float) Math.sin(i * 2π / 65536)`.
#[inline]
fn mth_sin(v: f64) -> f32 {
    let i = ((v * 10430.378350470453) as i64 & 65535) as f64;
    (i * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32
}

/// `Mth.cos(double)`.
#[inline]
fn mth_cos(v: f64) -> f32 {
    let i = ((v * 10430.378350470453 + 16384.0) as i64 & 65535) as f64;
    (i * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32
}

/// `Mth.clampedMap` on floats.
#[inline]
fn clamped_map_f32(v: f32, from_lo: f32, from_hi: f32, to_lo: f32, to_hi: f32) -> f32 {
    let t = (v - from_lo) / (from_hi - from_lo);
    if t < 0.0 {
        to_lo
    } else if t > 1.0 {
        to_hi
    } else {
        to_lo + t * (to_hi - to_lo)
    }
}

// ---- DripstoneUtils -------------------------------------------------------

/// `DripstoneUtils.isEmptyOrWater(state)`: air or the water block.
#[inline]
fn is_empty_or_water(s: JavaState) -> bool {
    s.is_air() || s.is(b::WATER)
}

/// `DripstoneUtils.isNeitherEmptyNorWater(state)`.
#[inline]
fn is_neither_empty_nor_water(s: JavaState) -> bool {
    !s.is_air() && !s.is(b::WATER)
}

/// `DripstoneUtils.isEmptyOrWaterOrLava(state)`.
#[inline]
fn is_empty_or_water_or_lava(s: JavaState) -> bool {
    s.is_air() || s.is(b::WATER) || s.is(b::LAVA)
}

/// `DripstoneUtils.isDripstoneBase(state)`: dripstone block or
/// `#dripstone_replaceable_blocks`.
#[inline]
fn is_dripstone_base(s: JavaState) -> bool {
    s.is(b::DRIPSTONE_BLOCK) || s.is_in(tags::DRIPSTONE_REPLACEABLE_BLOCKS)
}

/// `DripstoneUtils.isDripstoneBaseOrLava(state)`.
#[inline]
fn is_dripstone_base_or_lava(s: JavaState) -> bool {
    is_dripstone_base(s) || s.is(b::LAVA)
}

/// `DripstoneUtils.getDripstoneHeight`: height of a large dripstone at
/// horizontal distance `xz` from its axis.
fn dripstone_height(xz: f64, radius: f64, scale: f64, bluntness: f64) -> f64 {
    let xz = if xz < bluntness { bluntness } else { xz };
    let r = xz / radius * 0.384;
    let part1 = 0.75 * r.powf(1.333_333_333_333_333_3);
    let part2 = r.powf(0.666_666_666_666_666_6);
    let part3 = 0.333_333_333_333_333_3 * r.ln();
    let h = (scale * (part1 - part2 - part3)).max(0.0);
    h / 0.384 * radius
}

/// `DripstoneUtils.isCircleMostlyEmbeddedInStone`.
fn is_circle_mostly_embedded_in_stone<W: FeatureWorld + ?Sized>(
    world: &mut W,
    center: BlockPos,
    radius: i32,
) -> bool {
    if is_empty_or_water_or_lava(world.get_state(center)) {
        return false;
    }
    let increment = 6.0f32 / radius as f32;
    let mut angle = 0.0f32;
    while angle < std::f64::consts::TAU as f32 {
        let dx = (mth_cos(angle as f64) * radius as f32) as i32;
        let dz = (mth_sin(angle as f64) * radius as f32) as i32;
        if is_empty_or_water_or_lava(world.get_state(center.offset(dx, 0, dz))) {
            return false;
        }
        angle += increment;
    }
    true
}

/// `PointedDripstoneBlock` state with `vertical_direction` and `thickness`.
#[inline]
fn pointed(direction: Direction, thickness: &str) -> JavaState {
    states::POINTED_DRIPSTONE
        .with("vertical_direction", direction.name())
        .with("thickness", thickness)
}

/// `DripstoneUtils.growPointedDripstone`: a base-to-tip column of `height`
/// pointed dripstone starting at `start` and growing towards `dir`, only if
/// the block behind `start` is a dripstone base. Each piece is waterlogged
/// when placed into water.
fn grow_pointed_dripstone<W: FeatureWorld + ?Sized>(
    world: &mut W,
    start: BlockPos,
    dir: Direction,
    height: i32,
    merged_tip: bool,
) {
    if !is_dripstone_base(world.get_state(start.rel(dir.opposite()))) {
        return;
    }
    let mut pos = start;
    let mut put = |world: &mut W, thickness: &str| {
        let state = pointed(dir, thickness).with_bool("waterlogged", world.fluid(pos).is_water());
        world.set_state(pos, state);
        pos = pos.rel(dir);
    };
    // `buildBaseToTipColumn`.
    if height >= 3 {
        put(world, "base");
        for _ in 0..height - 3 {
            put(world, "middle");
        }
    }
    if height >= 2 {
        put(world, "frustum");
    }
    if height >= 1 {
        put(world, if merged_tip { "tip_merge" } else { "tip" });
    }
}

/// `DripstoneUtils.placeDripstoneBlockIfPossible`.
#[inline]
fn place_dripstone_block_if_possible<W: FeatureWorld + ?Sized>(
    world: &mut W,
    pos: BlockPos,
) -> bool {
    if world
        .get_state(pos)
        .is_in(tags::DRIPSTONE_REPLACEABLE_BLOCKS)
    {
        world.set_state(pos, states::DRIPSTONE_BLOCK);
        true
    } else {
        false
    }
}

// ---- Column ---------------------------------------------------------------

/// Java `Column`: optional floor and ceiling Y around an open run of blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Column {
    floor: Option<i32>,
    ceiling: Option<i32>,
}

impl Column {
    /// `getHeight()`: number of open blocks between floor and ceiling.
    #[inline]
    fn height(self) -> Option<i32> {
        match (self.floor, self.ceiling) {
            (Some(f), Some(c)) => Some(c - f - 1),
            _ => None,
        }
    }

    /// `Column.scan(level, pos, searchRange, insideColumn, validEdge)`.
    fn scan<W: FeatureWorld + ?Sized>(
        world: &mut W,
        pos: BlockPos,
        search_range: i32,
        inside: fn(JavaState) -> bool,
        edge: fn(JavaState) -> bool,
    ) -> Option<Column> {
        if !inside(world.get_state(pos)) {
            return None;
        }
        let ceiling = Self::scan_direction(world, search_range, pos, Direction::Up, inside, edge);
        let floor = Self::scan_direction(world, search_range, pos, Direction::Down, inside, edge);
        Some(Column { floor, ceiling })
    }

    fn scan_direction<W: FeatureWorld + ?Sized>(
        world: &mut W,
        search_range: i32,
        start: BlockPos,
        dir: Direction,
        inside: fn(JavaState) -> bool,
        edge: fn(JavaState) -> bool,
    ) -> Option<i32> {
        let mut pos = start;
        let mut i = 1;
        while i < search_range && inside(world.get_state(pos)) {
            pos = pos.rel(dir);
            i += 1;
        }
        edge(world.get_state(pos)).then_some(pos.y)
    }
}

// ---- dripstone_cluster ----------------------------------------------------

/// Java `DripstoneClusterFeature.place` (`minecraft:dripstone_cluster`).
///
/// Fails if the origin is not air or water. Otherwise every column of the
/// random `radius` x `radius` rectangle is scanned for a floor and ceiling;
/// dripstone blocks are laid into both, an optional water pool replaces the
/// floor block, and stalactites/stalagmites grow towards each other (merging
/// their tips when they meet exactly).
///
/// Normal samples share the caller's `WorldgenRandom` Gaussian cache, including
/// a value left by an earlier feature before the caller reseeds the bit source.
/// Do not reconstruct or independently reseed the random generator here.
pub fn place_cluster<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &DripstoneClusterConfig,
    origin: BlockPos,
) -> bool {
    if !is_empty_or_water(world.get_state(origin)) {
        return false;
    }
    let height = cfg.height.sample(rng);
    let wetness = cfg.wetness.sample(rng);
    let density = cfg.density.sample(rng);
    let x_radius = cfg.radius.sample(rng);
    let z_radius = cfg.radius.sample(rng);
    for dx in -x_radius..=x_radius {
        for dz in -z_radius..=z_radius {
            let chance = column_chance(x_radius, z_radius, dx, dz, cfg);
            let pos = origin.offset(dx, 0, dz);
            place_cluster_column(
                world, rng, pos, dx, dz, wetness, chance, height, density, cfg,
            );
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn place_cluster_column<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    pos: BlockPos,
    dx: i32,
    dz: i32,
    chance_of_water: f32,
    chance_of_column: f64,
    cluster_height: i32,
    density: f32,
    cfg: &DripstoneClusterConfig,
) {
    let Some(column) = Column::scan(
        world,
        pos,
        cfg.floor_to_ceiling_search_range,
        is_empty_or_water,
        is_neither_empty_nor_water,
    ) else {
        return;
    };
    let ceiling = column.ceiling;
    let floor = column.floor;
    if ceiling.is_none() && floor.is_none() {
        return;
    }

    let place_water = rng.next_float() < chance_of_water;
    let water_column = match floor {
        Some(floor_y) if place_water && can_place_pool(world, pos.with_y(floor_y)) => {
            world.set_state(pos.with_y(floor_y), states::WATER);
            Column {
                floor: Some(floor_y - 1),
                ceiling,
            }
        }
        _ => column,
    };
    let floor_after_water = water_column.floor;

    let place_stalactite = rng.next_double() < chance_of_column;
    let stalactite_height = match ceiling {
        Some(ceiling_y)
            if place_stalactite && !world.get_state(pos.with_y(ceiling_y)).is(b::LAVA) =>
        {
            let thickness = cfg.dripstone_block_layer_thickness.sample(rng);
            replace_with_dripstone_blocks(world, pos.with_y(ceiling_y), thickness, Direction::Up);
            let max_height = match floor_after_water {
                Some(f) => cluster_height.min(ceiling_y - f),
                None => cluster_height,
            };
            cluster_dripstone_height(rng, dx, dz, density, max_height, cfg)
        }
        _ => 0,
    };

    let place_stalagmite = rng.next_double() < chance_of_column;
    let stalagmite_height = match floor_after_water {
        Some(floor_y) if place_stalagmite && !world.get_state(pos.with_y(floor_y)).is(b::LAVA) => {
            let thickness = cfg.dripstone_block_layer_thickness.sample(rng);
            replace_with_dripstone_blocks(world, pos.with_y(floor_y), thickness, Direction::Down);
            if ceiling.is_some() {
                let diff = cfg.max_stalagmite_stalactite_height_diff;
                0.max(stalactite_height + rng.next_int_between_inclusive(-diff, diff))
            } else {
                cluster_dripstone_height(rng, dx, dz, density, cluster_height, cfg)
            }
        }
        _ => 0,
    };

    let (actual_stalactite, actual_stalagmite) = match (ceiling, floor_after_water) {
        (Some(ceiling_y), Some(floor_y))
            if ceiling_y - stalactite_height <= floor_y + stalagmite_height =>
        {
            let lowest_tip = (ceiling_y - stalactite_height).max(floor_y + 1);
            let highest_tip = (floor_y + stalagmite_height).min(ceiling_y - 1);
            let stalactite_tip = rng.next_int_between_inclusive(lowest_tip, highest_tip + 1);
            let stalagmite_tip = stalactite_tip - 1;
            (ceiling_y - stalactite_tip, stalagmite_tip - floor_y)
        }
        _ => (stalactite_height, stalagmite_height),
    };

    let merge_tips = rng.next_bool()
        && actual_stalactite > 0
        && actual_stalagmite > 0
        && water_column
            .height()
            .is_some_and(|h| actual_stalactite + actual_stalagmite == h);
    if let Some(ceiling_y) = ceiling {
        grow_pointed_dripstone(
            world,
            pos.with_y(ceiling_y - 1),
            Direction::Down,
            actual_stalactite,
            merge_tips,
        );
    }
    if let Some(floor_y) = floor_after_water {
        grow_pointed_dripstone(
            world,
            pos.with_y(floor_y + 1),
            Direction::Up,
            actual_stalagmite,
            merge_tips,
        );
    }
}

/// `DripstoneClusterFeature.getDripstoneHeight`.
fn cluster_dripstone_height(
    rng: &mut FeatureRandom,
    dx: i32,
    dz: i32,
    density: f32,
    max_height: i32,
    cfg: &DripstoneClusterConfig,
) -> i32 {
    if rng.next_float() > density {
        return 0;
    }
    let distance = dx.abs() + dz.abs();
    let mean = clamped_map(
        distance as f64,
        0.0,
        cfg.max_distance_from_center_affecting_height_bias as f64,
        max_height as f64 / 2.0,
        0.0,
    ) as f32;
    // `ClampedNormalFloat.sample(random, mean, deviation, 0, maxHeight)`.
    let v = FloatProvider::ClampedNormal {
        mean,
        deviation: cfg.height_deviation as f32,
        min: 0.0,
        max: max_height as f32,
    }
    .sample(rng);
    v as i32
}

/// `DripstoneClusterFeature.canPlacePool`.
fn can_place_pool<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos) -> bool {
    let s = world.get_state(pos);
    if s.is(b::WATER) || s.is(b::DRIPSTONE_BLOCK) || s.is(b::POINTED_DRIPSTONE) {
        return false;
    }
    if world.fluid(pos.above()).is_water() {
        return false;
    }
    for dir in Direction::HORIZONTAL {
        if !can_be_adjacent_to_water(world, pos.rel(dir)) {
            return false;
        }
    }
    can_be_adjacent_to_water(world, pos.below())
}

#[inline]
fn can_be_adjacent_to_water<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos) -> bool {
    let s = world.get_state(pos);
    s.is_in(tags::BASE_STONE_OVERWORLD) || s.fluid().is_water()
}

fn replace_with_dripstone_blocks<W: FeatureWorld + ?Sized>(
    world: &mut W,
    first: BlockPos,
    max_count: i32,
    dir: Direction,
) {
    let mut pos = first;
    for _ in 0..max_count {
        if !place_dripstone_block_if_possible(world, pos) {
            return;
        }
        pos = pos.rel(dir);
    }
}

/// `DripstoneClusterFeature.getChanceOfStalagmiteOrStalactite`.
fn column_chance(
    x_radius: i32,
    z_radius: i32,
    dx: i32,
    dz: i32,
    cfg: &DripstoneClusterConfig,
) -> f64 {
    let x_edge = x_radius - dx.abs();
    let z_edge = z_radius - dz.abs();
    let edge = x_edge.min(z_edge);
    clamped_map_f32(
        edge as f32,
        0.0,
        cfg.max_distance_from_edge_affecting_chance_of_dripstone_column as f32,
        cfg.chance_of_dripstone_column_at_max_distance_from_center,
        1.0,
    ) as f64
}

// ---- large_dripstone ------------------------------------------------------

/// Java `WindOffsetter`: shears a large dripstone horizontally with distance
/// from the origin height.
#[derive(Debug, Clone, Copy)]
struct Wind {
    origin_y: i32,
    speed: Option<(f64, f64)>,
}

impl Wind {
    const NONE: Wind = Wind {
        origin_y: 0,
        speed: None,
    };

    fn new(origin_y: i32, rng: &mut FeatureRandom, speed: &FloatProvider) -> Wind {
        let s = speed.sample(rng);
        // `Mth.randomBetween(random, 0, (float) Math.PI)`.
        let direction = rng.next_float() * std::f32::consts::PI;
        Wind {
            origin_y,
            speed: Some((
                (mth_cos(direction as f64) * s) as f64,
                (mth_sin(direction as f64) * s) as f64,
            )),
        }
    }

    #[inline]
    fn offset(self, pos: BlockPos) -> BlockPos {
        match self.speed {
            None => pos,
            Some((sx, sz)) => {
                let dy = (self.origin_y - pos.y) as f64;
                pos.offset((sx * dy).floor() as i32, 0, (sz * dy).floor() as i32)
            }
        }
    }
}

/// Java `LargeDripstone`: one half (stalactite or stalagmite) of a large
/// dripstone.
#[derive(Debug, Clone, Copy)]
struct LargeDripstone {
    root: BlockPos,
    pointing_up: bool,
    radius: i32,
    bluntness: f64,
    scale: f64,
}

impl LargeDripstone {
    fn new(
        root: BlockPos,
        pointing_up: bool,
        rng: &mut FeatureRandom,
        radius: i32,
        bluntness: &FloatProvider,
        scale: &FloatProvider,
    ) -> Self {
        let bluntness = bluntness.sample(rng) as f64;
        let scale = scale.sample(rng) as f64;
        Self {
            root,
            pointing_up,
            radius,
            bluntness,
            scale,
        }
    }

    #[inline]
    fn height_at_radius(&self, r: f32) -> i32 {
        dripstone_height(r as f64, self.radius as f64, self.scale, self.bluntness) as i32
    }

    #[inline]
    fn suitable_for_wind(&self, cfg: &LargeDripstoneConfig) -> bool {
        self.radius >= cfg.min_radius_for_wind
            && self.bluntness >= cfg.min_bluntness_for_wind as f64
    }

    /// `moveBackUntilBaseIsInsideStoneAndShrinkRadiusIfNecessary`.
    fn settle<W: FeatureWorld + ?Sized>(&mut self, world: &mut W, wind: Wind) -> bool {
        while self.radius > 1 {
            let mut root = self.root;
            let tries = 10.min(self.height_at_radius(0.0));
            for _ in 0..tries {
                if world.get_state(root).is(b::LAVA) {
                    return false;
                }
                if is_circle_mostly_embedded_in_stone(world, wind.offset(root), self.radius) {
                    self.root = root;
                    return true;
                }
                root = root.rel(if self.pointing_up {
                    Direction::Down
                } else {
                    Direction::Up
                });
            }
            self.radius /= 2;
        }
        false
    }

    fn place_blocks<W: FeatureWorld + ?Sized>(
        &self,
        world: &mut W,
        rng: &mut FeatureRandom,
        wind: Wind,
    ) {
        let step = if self.pointing_up {
            Direction::Up
        } else {
            Direction::Down
        };
        for dx in -self.radius..=self.radius {
            for dz in -self.radius..=self.radius {
                let current = ((dx * dx + dz * dz) as f32).sqrt();
                if current > self.radius as f32 {
                    continue;
                }
                let mut height = self.height_at_radius(current);
                if height <= 0 {
                    continue;
                }
                if (rng.next_float() as f64) < 0.2 {
                    // `Mth.randomBetween(random, 0.8F, 1.0F)`.
                    let f = rng.next_float() * (1.0 - 0.8) + 0.8;
                    height = (height as f32 * f) as i32;
                }
                let mut pos = self.root.offset(dx, 0, dz);
                let mut been_out_of_stone = false;
                let max_y = if self.pointing_up {
                    world.get_height(Heightmap::WorldSurfaceWg, pos.x, pos.z)
                } else {
                    i32::MAX
                };
                let mut i = 0;
                while i < height && pos.y < max_y {
                    let p = wind.offset(pos);
                    if is_empty_or_water_or_lava(world.get_state(p)) {
                        been_out_of_stone = true;
                        world.set_state(p, states::DRIPSTONE_BLOCK);
                    } else if been_out_of_stone
                        && world.get_state(p).is_in(tags::BASE_STONE_OVERWORLD)
                    {
                        break;
                    }
                    pos = pos.rel(step);
                    i += 1;
                }
            }
        }
    }
}

/// Java `LargeDripstoneFeature.place` (`minecraft:large_dripstone`).
///
/// Needs an air/water origin inside a floor-to-ceiling column of dripstone
/// bases (or lava) at least 4 blocks tall. Builds a stalactite from the
/// ceiling and a stalagmite from the floor out of dripstone blocks; when both
/// are wide and blunt enough they are sheared by a random wind.
pub fn place_large<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &LargeDripstoneConfig,
    origin: BlockPos,
) -> bool {
    if !is_empty_or_water(world.get_state(origin)) {
        return false;
    }
    let Some(column) = Column::scan(
        world,
        origin,
        cfg.floor_to_ceiling_search_range,
        is_empty_or_water,
        is_dripstone_base_or_lava,
    ) else {
        return false;
    };
    let (Some(floor), Some(ceiling), Some(height)) =
        (column.floor, column.ceiling, column.height())
    else {
        return false;
    };
    if height < 4 {
        return false;
    }
    let by_cave_height = (height as f32 * cfg.max_column_radius_to_cave_height_ratio) as i32;
    let min_r = cfg.column_radius.min_value();
    let max_r = cfg.column_radius.max_value();
    // `Mth.clamp(int, min, max)`.
    let max_radius = by_cave_height.max(min_r).min(max_r);
    let radius = rng.next_int_between_inclusive(min_r, max_radius);
    let mut stalactite = LargeDripstone::new(
        origin.with_y(ceiling - 1),
        false,
        rng,
        radius,
        &cfg.stalactite_bluntness,
        &cfg.height_scale,
    );
    let mut stalagmite = LargeDripstone::new(
        origin.with_y(floor + 1),
        true,
        rng,
        radius,
        &cfg.stalagmite_bluntness,
        &cfg.height_scale,
    );
    let wind = if stalactite.suitable_for_wind(cfg) && stalagmite.suitable_for_wind(cfg) {
        Wind::new(origin.y, rng, &cfg.wind_speed)
    } else {
        Wind::NONE
    };
    let stalactite_ok = stalactite.settle(world, wind);
    let stalagmite_ok = stalagmite.settle(world, wind);
    if stalactite_ok {
        stalactite.place_blocks(world, rng, wind);
    }
    if stalagmite_ok {
        stalagmite.place_blocks(world, rng, wind);
    }
    true
}

// ---- pointed_dripstone ----------------------------------------------------

/// Java `PointedDripstoneFeature.place` (`minecraft:pointed_dripstone`).
///
/// Needs a dripstone base above (stalactite) or below (stalagmite) the origin
/// (a coin flip picks when both are present). Lays a small random patch of
/// dripstone blocks around the base, then grows a pointed dripstone of
/// length 1, or 2 with `chance_of_taller_dripstone` when the next block is
/// air or water.
pub fn place_pointed<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &PointedDripstoneConfig,
    origin: BlockPos,
) -> bool {
    let above = is_dripstone_base(world.get_state(origin.above()));
    let below = is_dripstone_base(world.get_state(origin.below()));
    let tip = if above && below {
        if rng.next_bool() {
            Direction::Down
        } else {
            Direction::Up
        }
    } else if above {
        Direction::Down
    } else if below {
        Direction::Up
    } else {
        return false;
    };
    let root = origin.rel(tip.opposite());
    place_dripstone_block_if_possible(world, root);
    for dir in Direction::HORIZONTAL {
        if rng.next_float() > cfg.chance_of_directional_spread {
            continue;
        }
        let p1 = root.rel(dir);
        place_dripstone_block_if_possible(world, p1);
        if rng.next_float() > cfg.chance_of_spread_radius2 {
            continue;
        }
        let p2 = p1.rel(Direction::ALL[rng.next_int_bounded(6) as usize]);
        place_dripstone_block_if_possible(world, p2);
        if rng.next_float() > cfg.chance_of_spread_radius3 {
            continue;
        }
        let p3 = p2.rel(Direction::ALL[rng.next_int_bounded(6) as usize]);
        place_dripstone_block_if_possible(world, p3);
    }
    let height = if rng.next_float() < cfg.chance_of_taller_dripstone
        && is_empty_or_water(world.get_state(origin.rel(tip)))
    {
        2
    } else {
        1
    };
    grow_pointed_dripstone(world, origin, tip, height, false);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::test_world::TestWorld;
    use crate::features::values::IntProvider;

    /// A cave: stone everywhere except an air gap `floor < y < ceiling`.
    fn cave(floor: i32, ceiling: i32) -> TestWorld {
        let mut w = TestWorld::flat(200);
        for x in -12..=12 {
            for z in -12..=12 {
                for y in floor + 1..ceiling {
                    w.blocks.insert(BlockPos::new(x, y, z), states::AIR);
                }
            }
        }
        w
    }

    fn is_air_or_dripstone(s: JavaState) -> bool {
        s.is_air() || s.is(b::POINTED_DRIPSTONE) || s.is(b::DRIPSTONE_BLOCK) || s.is(b::WATER)
    }

    #[test]
    fn mth_trig_matches_table_points() {
        assert_eq!(mth_sin(0.0), 0.0);
        assert_eq!(mth_cos(0.0), 1.0);
        assert!((mth_sin(std::f64::consts::FRAC_PI_2) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn column_builder_shapes() {
        let mut w = cave(0, 10);
        grow_pointed_dripstone(&mut w, BlockPos::new(0, 1, 0), Direction::Up, 4, false);
        let t = |y| w.blocks[&BlockPos::new(0, y, 0)].value("thickness");
        assert_eq!(t(1), Some("base"));
        assert_eq!(t(2), Some("middle"));
        assert_eq!(t(3), Some("frustum"));
        assert_eq!(t(4), Some("tip"));
        assert_eq!(
            w.blocks[&BlockPos::new(0, 4, 0)].value("vertical_direction"),
            Some("up")
        );
    }

    #[test]
    fn pointed_needs_base() {
        let mut w = TestWorld::flat(-64);
        let mut rng = FeatureRandom::new(1);
        let cfg = PointedDripstoneConfig {
            chance_of_taller_dripstone: 0.2,
            chance_of_directional_spread: 0.7,
            chance_of_spread_radius2: 0.5,
            chance_of_spread_radius3: 0.5,
        };
        assert!(!place_pointed(
            &mut w,
            &mut rng,
            &cfg,
            BlockPos::new(0, 10, 0)
        ));
        assert!(w.blocks.is_empty());

        let mut w = cave(0, 10);
        assert!(place_pointed(
            &mut w,
            &mut rng,
            &cfg,
            BlockPos::new(0, 9, 0)
        ));
        let s = w.blocks[&BlockPos::new(0, 9, 0)];
        assert!(s.is(b::POINTED_DRIPSTONE));
        assert_eq!(s.value("vertical_direction"), Some("down"));
        assert_eq!(w.blocks[&BlockPos::new(0, 10, 0)], states::DRIPSTONE_BLOCK);
    }

    fn cluster_cfg() -> DripstoneClusterConfig {
        DripstoneClusterConfig {
            floor_to_ceiling_search_range: 12,
            height: IntProvider::Uniform { min: 3, max: 6 },
            radius: IntProvider::Uniform { min: 2, max: 8 },
            max_stalagmite_stalactite_height_diff: 1,
            height_deviation: 3,
            dripstone_block_layer_thickness: IntProvider::Uniform { min: 2, max: 4 },
            density: FloatProvider::Uniform {
                min: 0.3,
                max_exclusive: 0.7,
            },
            wetness: FloatProvider::ClampedNormal {
                mean: 0.1,
                deviation: 0.3,
                min: 0.1,
                max: 0.9,
            },
            chance_of_dripstone_column_at_max_distance_from_center: 0.1,
            max_distance_from_edge_affecting_chance_of_dripstone_column: 3,
            max_distance_from_center_affecting_height_bias: 8,
        }
    }

    #[test]
    fn cluster_stays_in_cave_and_is_deterministic() {
        let run = |seed| {
            let mut w = cave(0, 8);
            let mut rng = FeatureRandom::new(seed);
            assert!(place_cluster(
                &mut w,
                &mut rng,
                &cluster_cfg(),
                BlockPos::new(0, 4, 0)
            ));
            w
        };
        let a = run(5);
        assert_eq!(a.blocks, run(5).blocks);
        assert!(a.count(states::DRIPSTONE_BLOCK) > 0);
        for (pos, s) in &a.blocks {
            assert!(is_air_or_dripstone(*s), "unexpected {s:?} at {pos:?}");
            if s.is(b::POINTED_DRIPSTONE) {
                // A stalagmite may start in a pool at the old floor (y = 0).
                assert!(pos.y >= 0 && pos.y < 8);
            }
        }
    }

    #[test]
    fn cluster_rejects_solid_origin() {
        let mut w = TestWorld::flat(100);
        let mut rng = FeatureRandom::new(5);
        assert!(!place_cluster(
            &mut w,
            &mut rng,
            &cluster_cfg(),
            BlockPos::new(0, 4, 0)
        ));
    }

    #[test]
    fn large_dripstone_builds_from_floor_and_ceiling() {
        let cfg = LargeDripstoneConfig {
            floor_to_ceiling_search_range: 30,
            column_radius: IntProvider::Uniform { min: 3, max: 19 },
            height_scale: FloatProvider::Uniform {
                min: 0.4,
                max_exclusive: 2.0,
            },
            max_column_radius_to_cave_height_ratio: 0.33,
            stalactite_bluntness: FloatProvider::Uniform {
                min: 0.3,
                max_exclusive: 0.9,
            },
            stalagmite_bluntness: FloatProvider::Uniform {
                min: 0.4,
                max_exclusive: 1.0,
            },
            wind_speed: FloatProvider::Uniform {
                min: 0.0,
                max_exclusive: 0.3,
            },
            min_radius_for_wind: 4,
            min_bluntness_for_wind: 0.6,
        };
        let mut w = cave(0, 20);
        let mut rng = FeatureRandom::new(17);
        assert!(place_large(&mut w, &mut rng, &cfg, BlockPos::new(0, 10, 0)));
        assert!(w.count(states::DRIPSTONE_BLOCK) > 0);
        for s in w.blocks.values() {
            assert!(s.is_air() || s.is(b::DRIPSTONE_BLOCK));
        }
        // Too low a cave.
        let mut w = cave(0, 4);
        assert!(!place_large(&mut w, &mut rng, &cfg, BlockPos::new(0, 2, 0)));
    }
}
