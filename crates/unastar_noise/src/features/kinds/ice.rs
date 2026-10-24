//! Frozen-biome features: `minecraft:ice_spike`, `minecraft:iceberg`,
//! `minecraft:blue_ice` and `minecraft:freeze_top_layer`.
//!
//! Java evaluates the iceberg shape with `Math.sin`/`Math.cos`; Rust's libm
//! results can differ from the JVM's in the last ulp, which only matters for
//! positions exactly on an ellipse boundary.

use crate::features::blocks::{BlockPos, Direction, Heightmap, JavaState};
use crate::features::eval::FeatureWorld;
use crate::features::generated::BIOME_CLIMATE;
use crate::features::java_blocks::{blocks as b, states, tags};
use crate::features::random::FeatureRandom;

/// `Mth.ceil(float)`.
#[inline]
fn ceil_f(f: f32) -> i32 {
    let i = f as i32;
    if f > i as f32 { i + 1 } else { i }
}

/// Blocks an ice spike may overwrite (air, dirt, snow block, ice).
#[inline]
fn spike_replaceable(s: JavaState) -> bool {
    s.is_air() || s.is_in(tags::DIRT) || s.is(b::SNOW_BLOCK) || s.is(b::ICE)
}

/// Java `minecraft:ice_spike` (`IceSpikeFeature`).
///
/// Drops to the first non-air block (stopping at `min_y + 2`), which must be a
/// snow block, then raises the base by `nextInt(4)`. The spike is `7..=10`
/// tall with a radius of `height / 4 + nextInt(2)`; radii above 1 have a 1/60
/// chance of a tall spike lifted by `10 + nextInt(30)`. Layers taper with
/// height, edge corners are skipped with a 25% chance each, and layers above
/// the base are mirrored downward when the layer radius exceeds 1. Finally a
/// packed-ice column (3x3 with random corner gaps, or 1x1 for thin spikes) is
/// driven down to y = 50 through air, dirt, snow and ice.
pub fn place_ice_spike<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    origin: BlockPos,
) -> bool {
    let mut pos = origin;
    while world.is_air(pos) && pos.y > world.min_y() + 2 {
        pos = pos.below();
    }
    if !world.is_block(pos, b::SNOW_BLOCK) {
        return false;
    }
    pos = pos.offset(0, rng.next_int_bounded(4), 0);
    let height = rng.next_int_bounded(4) + 7;
    let radius = height / 4 + rng.next_int_bounded(2);
    if radius > 1 && rng.next_int_bounded(60) == 0 {
        pos = pos.offset(0, 10 + rng.next_int_bounded(30), 0);
    }

    for k in 0..height {
        let f = (1.0 - k as f32 / height as f32) * radius as f32;
        let l = ceil_f(f);
        for i1 in -l..=l {
            let f1 = i1.abs() as f32 - 0.25;
            for j1 in -l..=l {
                let f2 = j1.abs() as f32 - 0.25;
                let inside = (i1 == 0 && j1 == 0) || f1 * f1 + f2 * f2 <= f * f;
                if !inside {
                    continue;
                }
                let edge = i1 == -l || i1 == l || j1 == -l || j1 == l;
                if edge && rng.next_float() > 0.75 {
                    continue;
                }
                let up = pos.offset(i1, k, j1);
                if spike_replaceable(world.get_state(up)) {
                    world.set_state(up, states::PACKED_ICE);
                }
                if k != 0 && l > 1 {
                    let down = pos.offset(i1, -k, j1);
                    if spike_replaceable(world.get_state(down)) {
                        world.set_state(down, states::PACKED_ICE);
                    }
                }
            }
        }
    }

    let r = (radius - 1).clamp(0, 1);
    for dx in -r..=r {
        for dz in -r..=r {
            let mut p = pos.offset(dx, -1, dz);
            let mut budget = 50;
            if dx.abs() == 1 && dz.abs() == 1 {
                budget = rng.next_int_bounded(5);
            }
            while p.y > 50 {
                let s = world.get_state(p);
                if !spike_replaceable(s) && !s.is(b::PACKED_ICE) {
                    break;
                }
                world.set_state(p, states::PACKED_ICE);
                p = p.below();
                budget -= 1;
                if budget <= 0 {
                    p = p.offset(0, -(rng.next_int_bounded(5) + 1), 0);
                    budget = rng.next_int_bounded(5);
                }
            }
        }
    }
    true
}

/// Java `minecraft:iceberg` (`IcebergFeature`); `state` is the main block
/// (`packed_ice` for `iceberg_packed`, `blue_ice` for `iceberg_blue`).
///
/// The origin is moved to sea level. Java picks a round or (30%) elliptical
/// shape, builds the part above water layer by layer, snow-caps it when
/// `snow_on_top` (30%) holds, removes overhangs and lone blocks in a smoothing
/// pass, builds the submerged part with a steeper radius, and finally carves
/// an air/water cut-out channel (90% for elliptical, 30% for round bergs).
/// Only air, snow blocks, ice and water are ever replaced by the berg.
pub fn place_iceberg<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    state: JavaState,
    origin: BlockPos,
) -> bool {
    let origin = origin.with_y(world.sea_level());
    let snow_on_top = rng.next_double() > 0.7;
    let angle = rng.next_double() * 2.0 * std::f64::consts::PI;
    let ellipse_a = 11 - rng.next_int_bounded(5);
    let ellipse_c = 3 + rng.next_int_bounded(3);
    let elliptical = rng.next_double() > 0.7;
    let mut height = if elliptical {
        rng.next_int_bounded(6) + 6
    } else {
        rng.next_int_bounded(15) + 3
    };
    if !elliptical && rng.next_double() > 0.9 {
        height += rng.next_int_bounded(19) + 7;
    }
    let depth = (height + rng.next_int_bounded(11)).min(18);
    let major = (height + rng.next_int_bounded(7) - rng.next_int_bounded(5)).min(11);
    let extent = if elliptical { ellipse_a } else { 11 };

    let shape = Shape {
        origin,
        elliptical,
        ellipse_c,
        angle,
        snow_on_top,
        main: state,
    };

    for x in -extent..extent {
        for z in -extent..extent {
            for y in 0..height {
                let radius = if elliptical {
                    radius_ellipse(y, height, major)
                } else {
                    radius_round(rng, y, height, major)
                };
                if elliptical || x < radius {
                    shape.generate_block(world, rng, height, x, y, z, radius, extent);
                }
            }
        }
    }

    smooth(world, origin, major, height, elliptical, ellipse_a);

    for x in -extent..extent {
        for z in -extent..extent {
            let mut y = -1;
            while y > -depth {
                let a = if elliptical {
                    ceil_f(extent as f32 * (1.0 - (y * y) as f32 / (depth as f32 * 8.0)))
                } else {
                    extent
                };
                let radius = radius_steep(rng, -y, depth, major);
                if x < radius {
                    shape.generate_block(world, rng, depth, x, y, z, radius, a);
                }
                y -= 1;
            }
        }
    }

    let cut_out = if elliptical {
        rng.next_double() > 0.1
    } else {
        rng.next_double() > 0.7
    };
    if cut_out {
        generate_cut_out(
            world, rng, major, height, origin, elliptical, ellipse_a, angle, ellipse_c,
        );
    }
    true
}

/// Per-berg constants shared by both building passes.
struct Shape {
    origin: BlockPos,
    elliptical: bool,
    ellipse_c: i32,
    angle: f64,
    snow_on_top: bool,
    main: JavaState,
}

impl Shape {
    /// `generateIcebergBlock`.
    #[allow(clippy::too_many_arguments)]
    fn generate_block<W: FeatureWorld + ?Sized>(
        &self,
        world: &mut W,
        rng: &mut FeatureRandom,
        height: i32,
        x: i32,
        y: i32,
        z: i32,
        radius: i32,
        a: i32,
    ) {
        let d = if self.elliptical {
            signed_distance_ellipse(
                x,
                z,
                0,
                0,
                a,
                ellipse_c_at(y, height, self.ellipse_c),
                self.angle,
            )
        } else {
            signed_distance_circle(x, z, radius, rng)
        };
        if d >= 0.0 {
            return;
        }
        let pos = self.origin.offset(x, y, z);
        let edge = if self.elliptical {
            -0.5
        } else {
            (-6 - rng.next_int_bounded(3)) as f64
        };
        if d > edge && rng.next_double() > 0.9 {
            return;
        }
        self.set_block(world, rng, pos, height - y, height);
    }

    /// `setIcebergBlock`.
    fn set_block<W: FeatureWorld + ?Sized>(
        &self,
        world: &mut W,
        rng: &mut FeatureRandom,
        pos: BlockPos,
        height_remaining: i32,
        height: i32,
    ) {
        let s = world.get_state(pos);
        if !(s.is_air() || s.is(b::SNOW_BLOCK) || s.is(b::ICE) || s.is(b::WATER)) {
            return;
        }
        let keep_snow = !self.elliptical || rng.next_double() > 0.05;
        let div = if self.elliptical { 3 } else { 2 };
        let snow = self.snow_on_top
            && !s.is(b::WATER)
            && (height_remaining as f64)
                <= rng.next_int_bounded((height / div).max(1)) as f64 + height as f64 * 0.6
            && keep_snow;
        world.set_state(pos, if snow { states::SNOW_BLOCK } else { self.main });
    }
}

/// `getEllipseC`: the ellipse narrows over the top three layers.
#[inline]
fn ellipse_c_at(y: i32, height: i32, value: i32) -> i32 {
    if y > 0 && height - y <= 3 {
        value - (4 - (height - y))
    } else {
        value
    }
}

/// `signedDistanceCircle` (draws one float).
#[inline]
fn signed_distance_circle(x: i32, z: i32, radius: i32, rng: &mut FeatureRandom) -> f64 {
    let f = 10.0 * rng.next_float().clamp(0.2, 0.8) / radius as f32;
    let dx = x as f64;
    let dz = z as f64;
    let r = radius as f64;
    f as f64 + dx * dx + dz * dz - r * r
}

/// `signedDistanceEllipse` around `(cx, cz)`.
#[inline]
fn signed_distance_ellipse(x: i32, z: i32, cx: i32, cz: i32, a: i32, c: i32, angle: f64) -> f64 {
    let dx = (x - cx) as f64;
    let dz = (z - cz) as f64;
    let (sin, cos) = angle.sin_cos();
    let u = (dx * cos - dz * sin) / a as f64;
    let v = (dx * sin + dz * cos) / c as f64;
    u * u + v * v - 1.0
}

/// `heightDependentRadiusRound` (draws a float and one or two ints).
fn radius_round(rng: &mut FeatureRandom, y: i32, height: i32, value: i32) -> i32 {
    let f = 3.5 - rng.next_float();
    let mut f1 = (1.0 - (y * y) as f32 / (height as f32 * f)) * value as f32;
    if height > 15 + rng.next_int_bounded(5) {
        let i = if y < 3 + rng.next_int_bounded(6) {
            y / 2
        } else {
            y
        };
        f1 = (1.0 - i as f32 / (height as f32 * f * 0.4)) * value as f32;
    }
    ceil_f(f1 / 2.0)
}

/// `heightDependentRadiusEllipse`.
#[inline]
fn radius_ellipse(y: i32, height: i32, value: i32) -> i32 {
    let f1 = (1.0 - (y * y) as f32 / (height as f32 * 1.0)) * value as f32;
    ceil_f(f1 / 2.0)
}

/// `heightDependentRadiusSteep` (draws one float).
#[inline]
fn radius_steep(rng: &mut FeatureRandom, y: i32, height: i32, value: i32) -> i32 {
    let f = 1.0 + rng.next_float() / 2.0;
    let f1 = (1.0 - y as f32 / (height as f32 * f)) * value as f32;
    ceil_f(f1 / 2.0)
}

/// Packed ice, snow block or blue ice.
#[inline]
fn is_iceberg_state(s: JavaState) -> bool {
    s.is(b::PACKED_ICE) || s.is(b::SNOW_BLOCK) || s.is(b::BLUE_ICE)
}

/// `smooth`: clears blocks hanging over air (and the block above them) and
/// berg blocks with at most one berg neighbour (west, east, north, south).
fn smooth<W: FeatureWorld + ?Sized>(
    world: &mut W,
    origin: BlockPos,
    major: i32,
    height: i32,
    elliptical: bool,
    ellipse_a: i32,
) {
    let r = if elliptical { ellipse_a } else { major / 2 };
    for x in -r..=r {
        for z in -r..=r {
            for y in 0..=height {
                let pos = origin.offset(x, y, z);
                let s = world.get_state(pos);
                if !(is_iceberg_state(s) || s.is(b::SNOW)) {
                    continue;
                }
                if world.get_state(pos.below()).is_air() {
                    world.set_state(pos, states::AIR);
                    world.set_state(pos.above(), states::AIR);
                } else if is_iceberg_state(s) {
                    let mut open = 0;
                    for n in [
                        pos.rel(Direction::West),
                        pos.rel(Direction::East),
                        pos.rel(Direction::North),
                        pos.rel(Direction::South),
                    ] {
                        if !is_iceberg_state(world.get_state(n)) {
                            open += 1;
                        }
                    }
                    if open >= 3 {
                        world.set_state(pos, states::AIR);
                    }
                }
            }
        }
    }
}

/// `generateCutOut`: a channel through the berg, air above water and water
/// below. The lower loop bound redraws `nextInt(5)` on every check, as in Java.
#[allow(clippy::too_many_arguments)]
fn generate_cut_out<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    major: i32,
    height: i32,
    origin: BlockPos,
    elliptical: bool,
    ellipse_a: i32,
    angle: f64,
    ellipse_c: i32,
) {
    let sx = if rng.next_bool() { -1 } else { 1 };
    let sz = if rng.next_bool() { -1 } else { 1 };
    let mut k = rng.next_int_bounded((major / 2 - 2).max(1));
    if rng.next_bool() {
        k = major / 2 + 1 - rng.next_int_bounded((major - major / 2 - 1).max(1));
    }
    let mut l = rng.next_int_bounded((major / 2 - 2).max(1));
    if rng.next_bool() {
        l = major / 2 + 1 - rng.next_int_bounded((major - major / 2 - 1).max(1));
    }
    if elliptical {
        k = rng.next_int_bounded((ellipse_a - 5).max(1));
        l = k;
    }
    let (cx, cz) = (sx * k, sz * l);
    let cut_angle = if elliptical {
        angle + std::f64::consts::FRAC_PI_2
    } else {
        rng.next_double() * 2.0 * std::f64::consts::PI
    };
    let cut = Cut {
        origin,
        angle: cut_angle,
        cx,
        cz,
        ellipse_a,
        ellipse_c,
    };

    for y in 0..height - 3 {
        let radius = radius_round(rng, y, height, major);
        cut.carve(world, radius, y, false);
    }
    let mut y = -1;
    while y > -height + rng.next_int_bounded(5) {
        let radius = radius_steep(rng, -y, height, major);
        cut.carve(world, radius, y, true);
        y -= 1;
    }
}

struct Cut {
    origin: BlockPos,
    angle: f64,
    cx: i32,
    cz: i32,
    ellipse_a: i32,
    ellipse_c: i32,
}

impl Cut {
    /// `carve`.
    fn carve<W: FeatureWorld + ?Sized>(
        &self,
        world: &mut W,
        radius: i32,
        y: i32,
        under_water: bool,
    ) {
        let a = radius + 1 + self.ellipse_a / 3;
        let c = (radius - 3).min(3) + self.ellipse_c / 2 - 1;
        for x in -a..a {
            for z in -a..a {
                if signed_distance_ellipse(x, z, self.cx, self.cz, a, c, self.angle) >= 0.0 {
                    continue;
                }
                let pos = self.origin.offset(x, y, z);
                let s = world.get_state(pos);
                if !(is_iceberg_state(s) || s.is(b::SNOW_BLOCK)) {
                    continue;
                }
                if under_water {
                    world.set_state(pos, states::WATER);
                } else {
                    world.set_state(pos, states::AIR);
                    // removeFloatingSnowLayer
                    if world.is_block(pos.above(), b::SNOW) {
                        world.set_state(pos.above(), states::AIR);
                    }
                }
            }
        }
    }
}

/// Java `minecraft:blue_ice` (`BlueIceFeature`).
///
/// Requires the origin to be at most `sea_level - 1`, water at the origin or
/// below it, and packed ice on one of the five non-down neighbours. Places
/// blue ice at the origin, then makes 200 attempts to grow it at random
/// offsets (vertical `nextInt(5) - nextInt(6)`, horizontal spread shrinking
/// with depth) into air, water, packed ice or ice touching blue ice.
pub fn place_blue_ice<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    origin: BlockPos,
) -> bool {
    if origin.y > world.sea_level() - 1 {
        return false;
    }
    if !world.is_block(origin, b::WATER) && !world.is_block(origin.below(), b::WATER) {
        return false;
    }
    let mut touches_packed = false;
    for dir in Direction::ALL {
        if dir != Direction::Down && world.is_block(origin.rel(dir), b::PACKED_ICE) {
            touches_packed = true;
            break;
        }
    }
    if !touches_packed {
        return false;
    }
    world.set_state(origin, states::BLUE_ICE);
    for _ in 0..200 {
        let dy = rng.next_int_bounded(5) - rng.next_int_bounded(6);
        let mut spread = 3;
        if dy < 2 {
            spread += dy / 2;
        }
        if spread < 1 {
            continue;
        }
        let dx = rng.next_int_bounded(spread) - rng.next_int_bounded(spread);
        let dz = rng.next_int_bounded(spread) - rng.next_int_bounded(spread);
        let pos = origin.offset(dx, dy, dz);
        let s = world.get_state(pos);
        if s.is_air() || s.is(b::WATER) || s.is(b::PACKED_ICE) || s.is(b::ICE) {
            for dir in Direction::ALL {
                if world.is_block(pos.rel(dir), b::BLUE_ICE) {
                    world.set_state(pos, states::BLUE_ICE);
                    break;
                }
            }
        }
    }
    true
}

/// Java `minecraft:freeze_top_layer` (`SnowAndFreezeFeature`).
///
/// For each column of the 16x16 area starting at the origin's x/z, takes the
/// `MOTION_BLOCKING` surface position and the biome there; freezes the water
/// just below it (`shouldFreeze` without the edge requirement), places a snow
/// layer at the surface when `shouldSnow` holds, and sets `snowy=true` on the
/// block under the snow if it has that property. Uses no randomness.
pub fn place_freeze_top_layer<W: FeatureWorld + ?Sized>(
    world: &mut W,
    _rng: &mut FeatureRandom,
    origin: BlockPos,
) -> bool {
    for i in 0..16 {
        for j in 0..16 {
            let x = origin.x + i;
            let z = origin.z + j;
            let y = world.get_height(Heightmap::MotionBlocking, x, z);
            let top = BlockPos::new(x, y, z);
            let below = top.below();
            let climate = &BIOME_CLIMATE[world.biome(top) as usize];
            if climate.should_freeze(world, below, false) {
                world.set_state(below, states::ICE);
            }
            if climate.should_snow(world, top) {
                world.set_state(top, states::SNOW);
                let s = world.get_state(below);
                if s.has("snowy") {
                    world.set_state(below, s.with_bool("snowy", true));
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::generated::biome_index;
    use crate::features::test_world::TestWorld;

    fn snow_ground(y: i32) -> TestWorld {
        let mut w = TestWorld::flat(y);
        for x in -16..=16 {
            for z in -16..=16 {
                w.blocks
                    .insert(BlockPos::new(x, y - 1, z), states::SNOW_BLOCK);
            }
        }
        w
    }

    #[test]
    fn ice_spike_needs_snow_block() {
        let mut w = TestWorld::flat(70);
        let mut rng = FeatureRandom::new(5);
        assert!(!place_ice_spike(&mut w, &mut rng, BlockPos::new(0, 90, 0)));
        assert!(w.blocks.is_empty());
    }

    #[test]
    fn ice_spike_places_packed_ice_only() {
        for seed in 0..20 {
            let mut w = snow_ground(70);
            let mut rng = FeatureRandom::new(seed);
            assert!(place_ice_spike(&mut w, &mut rng, BlockPos::new(0, 90, 0)));
            let ice = w.count(states::PACKED_ICE);
            assert!(ice > 0);
            for (p, s) in &w.blocks {
                assert!(
                    *s == states::PACKED_ICE || *s == states::SNOW_BLOCK,
                    "{p:?}"
                );
                assert!(p.y > 50, "column stops above y=50: {p:?}");
            }
        }
    }

    #[test]
    fn iceberg_only_writes_berg_blocks() {
        for seed in 0..10 {
            let mut w = TestWorld::flat(40);
            for x in -20..=20 {
                for z in -20..=20 {
                    for y in 40..63 {
                        w.blocks.insert(BlockPos::new(x, y, z), states::WATER);
                    }
                }
            }
            let mut rng = FeatureRandom::new(seed);
            assert!(place_iceberg(
                &mut w,
                &mut rng,
                states::PACKED_ICE,
                BlockPos::new(0, 0, 0)
            ));
            assert!(w.count(states::PACKED_ICE) > 0, "seed {seed}");
            for (p, s) in &w.blocks {
                assert!(
                    [
                        states::WATER,
                        states::AIR,
                        states::PACKED_ICE,
                        states::SNOW_BLOCK
                    ]
                    .contains(s),
                    "{p:?} {s:?}"
                );
            }
        }
    }

    #[test]
    fn iceberg_deterministic() {
        let run = |seed| {
            let mut w = TestWorld::flat(40);
            let mut rng = FeatureRandom::new(seed);
            place_iceberg(&mut w, &mut rng, states::BLUE_ICE, BlockPos::new(8, 0, 8));
            let mut v: Vec<_> = w.blocks.into_iter().collect();
            v.sort_by_key(|(p, _)| (p.x, p.y, p.z));
            (v, rng.next_long())
        };
        assert_eq!(run(42), run(42));
    }

    #[test]
    fn blue_ice_requirements() {
        let origin = BlockPos::new(0, 50, 0);
        let mut w = TestWorld::flat(40);
        w.blocks.insert(origin, states::WATER);
        let mut rng = FeatureRandom::new(1);
        assert!(
            !place_blue_ice(&mut w, &mut rng, origin),
            "needs packed ice"
        );
        w.blocks.insert(origin.above(), states::PACKED_ICE);
        assert!(
            !place_blue_ice(&mut w, &mut rng, origin.offset(0, 30, 0)),
            "above sea level"
        );
        assert!(place_blue_ice(&mut w, &mut rng, origin));
        assert_eq!(w.get_state(origin), states::BLUE_ICE);
    }

    #[test]
    fn freeze_top_layer_snows_and_freezes() {
        let mut w = TestWorld::flat(64);
        w.biome = biome_index("minecraft:snowy_plains").unwrap();
        for x in 0..16 {
            for z in 0..16 {
                let s = if x < 8 {
                    states::GRASS_BLOCK
                } else {
                    states::WATER
                };
                w.blocks.insert(BlockPos::new(x, 63, z), s);
            }
        }
        let mut rng = FeatureRandom::new(0);
        assert!(place_freeze_top_layer(
            &mut w,
            &mut rng,
            BlockPos::new(0, -64, 0)
        ));
        for z in 0..16 {
            assert_eq!(w.get_state(BlockPos::new(8, 63, z)), states::ICE);
            assert_eq!(w.get_state(BlockPos::new(0, 64, z)), states::SNOW);
            assert_eq!(
                w.get_state(BlockPos::new(0, 63, z)).value("snowy"),
                Some("true")
            );
        }
        // Columns outside the 16x16 area are untouched.
        assert!(w.get_state(BlockPos::new(16, 64, 0)).is_air());
    }

    #[test]
    fn freeze_top_layer_warm_biome_noop() {
        let mut w = TestWorld::flat(64);
        w.biome = biome_index("minecraft:desert").unwrap();
        let mut rng = FeatureRandom::new(0);
        place_freeze_top_layer(&mut w, &mut rng, BlockPos::new(0, -64, 0));
        assert!(w.blocks.is_empty());
    }
}
