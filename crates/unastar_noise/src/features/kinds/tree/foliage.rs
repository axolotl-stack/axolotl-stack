//! Foliage placers (`FoliagePlacer` subclasses).

use super::{FoliageAttachment, TreeCtx, valid_tree_pos_state};
use crate::features::blocks::{BlockPos, Direction};
use crate::features::config::FoliagePlacer;
use crate::features::eval::{FeatureWorld, provide_state};
use crate::features::random::FeatureRandom;
use crate::features::values::IntProvider;

/// `radius` and `offset` providers of every foliage placer.
fn radius_offset(placer: &FoliagePlacer) -> (&IntProvider, &IntProvider) {
    match placer {
        FoliagePlacer::Blob { radius, offset, .. }
        | FoliagePlacer::Bush { radius, offset, .. }
        | FoliagePlacer::Fancy { radius, offset, .. }
        | FoliagePlacer::Jungle { radius, offset, .. }
        | FoliagePlacer::Spruce { radius, offset, .. }
        | FoliagePlacer::Pine { radius, offset, .. }
        | FoliagePlacer::Acacia { radius, offset }
        | FoliagePlacer::DarkOak { radius, offset }
        | FoliagePlacer::MegaPine { radius, offset, .. }
        | FoliagePlacer::RandomSpread { radius, offset, .. }
        | FoliagePlacer::Cherry { radius, offset, .. } => (radius, offset),
    }
}

/// `FoliagePlacer.foliageHeight(random, treeHeight, config)`.
pub(super) fn foliage_height(
    placer: &FoliagePlacer,
    rng: &mut FeatureRandom,
    tree_height: i32,
) -> i32 {
    match placer {
        FoliagePlacer::Blob { height, .. }
        | FoliagePlacer::Bush { height, .. }
        | FoliagePlacer::Fancy { height, .. }
        | FoliagePlacer::Jungle { height, .. } => *height,
        FoliagePlacer::Spruce { trunk_height, .. } => 4.max(tree_height - trunk_height.sample(rng)),
        FoliagePlacer::Pine { height, .. } => height.sample(rng),
        FoliagePlacer::Acacia { .. } => 0,
        FoliagePlacer::DarkOak { .. } => 4,
        FoliagePlacer::MegaPine { crown_height, .. } => crown_height.sample(rng),
        FoliagePlacer::RandomSpread { foliage_height, .. } => foliage_height.sample(rng),
        FoliagePlacer::Cherry { height, .. } => height.sample(rng),
    }
}

/// `FoliagePlacer.foliageRadius(random, trunkHeight)`; pine adds
/// `nextInt(max(trunkHeight + 1, 1))`.
pub(super) fn foliage_radius(
    placer: &FoliagePlacer,
    rng: &mut FeatureRandom,
    trunk_height: i32,
) -> i32 {
    let r = radius_offset(placer).0.sample(rng);
    match placer {
        FoliagePlacer::Pine { .. } => r + rng.next_int_bounded((trunk_height + 1).max(1)),
        _ => r,
    }
}

impl<W: FeatureWorld + ?Sized> TreeCtx<'_, W> {
    /// `FoliagePlacer.createFoliage`: samples `offset`, then runs the
    /// placer-specific shape.
    pub(super) fn create_foliage(
        &mut self,
        _tree_height: i32,
        att: FoliageAttachment,
        foliage_height: i32,
        leaf_radius: i32,
    ) {
        let placer = self.cfg.foliage_placer;
        let offset = radius_offset(&placer).1.sample(self.rng);
        let dt = att.double_trunk;
        match placer {
            FoliagePlacer::Blob { .. } => {
                let mut yo = offset;
                while yo >= offset - foliage_height {
                    let r = (leaf_radius + att.radius_offset - 1 - yo / 2).max(0);
                    self.leaves_row(&placer, att.pos, r, yo, dt);
                    yo -= 1;
                }
            }
            FoliagePlacer::Bush { .. } => {
                let mut yo = offset;
                while yo >= offset - foliage_height {
                    let r = leaf_radius + att.radius_offset - 1 - yo;
                    self.leaves_row(&placer, att.pos, r, yo, dt);
                    yo -= 1;
                }
            }
            FoliagePlacer::Fancy { .. } => {
                let mut yo = offset;
                while yo >= offset - foliage_height {
                    let r = leaf_radius + i32::from(yo != offset && yo != offset - foliage_height);
                    self.leaves_row(&placer, att.pos, r, yo, dt);
                    yo -= 1;
                }
            }
            FoliagePlacer::Jungle { .. } => {
                let leaf_height = if dt {
                    foliage_height
                } else {
                    1 + self.rng.next_int_bounded(2)
                };
                let mut yo = offset;
                while yo >= offset - leaf_height {
                    let r = leaf_radius + att.radius_offset + 1 - yo;
                    self.leaves_row(&placer, att.pos, r, yo, dt);
                    yo -= 1;
                }
            }
            FoliagePlacer::Spruce { .. } => {
                let mut r = self.rng.next_int_bounded(2);
                let mut max_r = 1;
                let mut min_r = 0;
                let mut yo = offset;
                while yo >= -foliage_height {
                    self.leaves_row(&placer, att.pos, r, yo, dt);
                    if r >= max_r {
                        r = min_r;
                        min_r = 1;
                        max_r = (max_r + 1).min(leaf_radius + att.radius_offset);
                    } else {
                        r += 1;
                    }
                    yo -= 1;
                }
            }
            FoliagePlacer::Pine { .. } => {
                let mut r = 0;
                let mut yo = offset;
                while yo >= offset - foliage_height {
                    self.leaves_row(&placer, att.pos, r, yo, dt);
                    if r >= 1 && yo == offset - foliage_height + 1 {
                        r -= 1;
                    } else if r < leaf_radius + att.radius_offset {
                        r += 1;
                    }
                    yo -= 1;
                }
            }
            FoliagePlacer::Acacia { .. } => {
                let p = att.pos.offset(0, offset, 0);
                self.leaves_row(
                    &placer,
                    p,
                    leaf_radius + att.radius_offset,
                    -1 - foliage_height,
                    dt,
                );
                self.leaves_row(&placer, p, leaf_radius - 1, -foliage_height, dt);
                self.leaves_row(&placer, p, leaf_radius + att.radius_offset - 1, 0, dt);
            }
            FoliagePlacer::DarkOak { .. } => {
                let p = att.pos.offset(0, offset, 0);
                if dt {
                    self.leaves_row(&placer, p, leaf_radius + 2, -1, dt);
                    self.leaves_row(&placer, p, leaf_radius + 3, 0, dt);
                    self.leaves_row(&placer, p, leaf_radius + 2, 1, dt);
                    if self.rng.next_bool() {
                        self.leaves_row(&placer, p, leaf_radius, 2, dt);
                    }
                } else {
                    self.leaves_row(&placer, p, leaf_radius + 2, -1, dt);
                    self.leaves_row(&placer, p, leaf_radius + 1, 0, dt);
                }
            }
            FoliagePlacer::MegaPine { .. } => {
                let pos = att.pos;
                let mut prev = 0;
                let mut yy = pos.y - foliage_height + offset;
                while yy <= pos.y + offset {
                    let yo = pos.y - yy;
                    let smooth = leaf_radius
                        + att.radius_offset
                        + floor_f(yo as f32 / foliage_height as f32 * 3.5);
                    let jagged = if yo > 0 && smooth == prev && (yy & 1) == 0 {
                        smooth + 1
                    } else {
                        smooth
                    };
                    self.leaves_row(&placer, BlockPos::new(pos.x, yy, pos.z), jagged, 0, dt);
                    prev = smooth;
                    yy += 1;
                }
            }
            FoliagePlacer::RandomSpread {
                leaf_placement_attempts,
                ..
            } => {
                for _ in 0..leaf_placement_attempts {
                    let dx = self.rng.next_int_bounded(leaf_radius)
                        - self.rng.next_int_bounded(leaf_radius);
                    let dy = self.rng.next_int_bounded(foliage_height)
                        - self.rng.next_int_bounded(foliage_height);
                    let dz = self.rng.next_int_bounded(leaf_radius)
                        - self.rng.next_int_bounded(leaf_radius);
                    self.try_place_leaf(att.pos.offset(dx, dy, dz));
                }
            }
            FoliagePlacer::Cherry {
                hanging_leaves_chance,
                hanging_leaves_extension_chance,
                ..
            } => {
                let p = att.pos.offset(0, offset, 0);
                let r = leaf_radius + att.radius_offset - 1;
                self.leaves_row(&placer, p, r - 2, foliage_height - 3, dt);
                self.leaves_row(&placer, p, r - 1, foliage_height - 4, dt);
                let mut y = foliage_height - 5;
                while y >= 0 {
                    self.leaves_row(&placer, p, r, y, dt);
                    y -= 1;
                }
                self.leaves_row_hanging(
                    &placer,
                    p,
                    r,
                    -1,
                    dt,
                    hanging_leaves_chance,
                    hanging_leaves_extension_chance,
                );
                self.leaves_row_hanging(
                    &placer,
                    p,
                    r - 1,
                    -2,
                    dt,
                    hanging_leaves_chance,
                    hanging_leaves_extension_chance,
                );
            }
        }
    }

    /// `FoliagePlacer.placeLeavesRow`.
    fn leaves_row(&mut self, placer: &FoliagePlacer, origin: BlockPos, r: i32, y: i32, dt: bool) {
        let extra = i32::from(dt);
        for dx in -r..=r + extra {
            for dz in -r..=r + extra {
                if !self.skip_signed(placer, dx, y, dz, r, dt) {
                    self.try_place_leaf(origin.offset(dx, y, dz));
                }
            }
        }
    }

    /// `FoliagePlacer.placeLeavesRowWithHangingLeavesBelow`.
    #[allow(clippy::too_many_arguments)]
    fn leaves_row_hanging(
        &mut self,
        placer: &FoliagePlacer,
        origin: BlockPos,
        r: i32,
        y: i32,
        dt: bool,
        chance: f32,
        extension_chance: f32,
    ) {
        self.leaves_row(placer, origin, r, y, dt);
        let extra = i32::from(dt);
        let log_pos = origin.below();
        for dir in Direction::HORIZONTAL {
            let cw = dir.clockwise();
            let positive = matches!(cw, Direction::East | Direction::South);
            let cw_offset = if positive { r + extra } else { r };
            let mut pos = origin
                .offset(0, y - 1, 0)
                .rel_n(cw, cw_offset)
                .rel_n(dir, -r);
            let mut i = -r;
            while i < r + extra {
                let leaves_above = self.foliage.contains(pos.above());
                if leaves_above && self.try_place_extension(chance, log_pos, pos) {
                    self.try_place_extension(extension_chance, log_pos, pos.below());
                }
                i += 1;
                pos = pos.rel(dir);
            }
        }
    }

    fn try_place_extension(&mut self, chance: f32, log_pos: BlockPos, pos: BlockPos) -> bool {
        let d = (pos.x - log_pos.x).abs() + (pos.y - log_pos.y).abs() + (pos.z - log_pos.z).abs();
        if d >= 7 {
            return false;
        }
        if self.rng.next_float() > chance {
            return false;
        }
        self.try_place_leaf(pos)
    }

    /// `FoliagePlacer.tryPlaceLeaf`: skips persistent leaves and invalid
    /// positions; waterlogs leaves placed into source water.
    pub(super) fn try_place_leaf(&mut self, pos: BlockPos) -> bool {
        let current = self.world.get_state(pos);
        if current.value("persistent") == Some("true") || !valid_tree_pos_state(current) {
            return false;
        }
        let mut state = provide_state(&self.cfg.foliage_provider, self.rng, pos);
        if state.has("waterlogged") {
            let f = current.fluid();
            state = state.with_bool("waterlogged", f.is_water() && f.source);
        }
        self.set_leaf(pos, state);
        true
    }

    /// `shouldSkipLocationSigned` (dark oak overrides it).
    fn skip_signed(
        &mut self,
        placer: &FoliagePlacer,
        dx: i32,
        y: i32,
        dz: i32,
        r: i32,
        dt: bool,
    ) -> bool {
        if let FoliagePlacer::DarkOak { .. } = placer
            && y == 0
            && dt
            && (dx == -r || dx >= r)
            && (dz == -r || dz >= r)
        {
            return true;
        }
        let (x, z) = if dt {
            (dx.abs().min((dx - 1).abs()), dz.abs().min((dz - 1).abs()))
        } else {
            (dx.abs(), dz.abs())
        };
        self.skip(placer, x, y, z, r, dt)
    }

    /// `shouldSkipLocation` on absolute offsets.
    fn skip(&mut self, placer: &FoliagePlacer, dx: i32, y: i32, dz: i32, r: i32, dt: bool) -> bool {
        match *placer {
            FoliagePlacer::Blob { .. } => {
                dx == r && dz == r && (self.rng.next_int_bounded(2) == 0 || y == 0)
            }
            FoliagePlacer::Bush { .. } => dx == r && dz == r && self.rng.next_int_bounded(2) == 0,
            FoliagePlacer::Fancy { .. } => {
                let a = dx as f32 + 0.5;
                let b = dz as f32 + 0.5;
                a * a + b * b > (r * r) as f32
            }
            FoliagePlacer::Jungle { .. } | FoliagePlacer::MegaPine { .. } => {
                dx + dz >= 7 || dx * dx + dz * dz > r * r
            }
            FoliagePlacer::Spruce { .. } | FoliagePlacer::Pine { .. } => {
                dx == r && dz == r && r > 0
            }
            FoliagePlacer::Acacia { .. } => {
                if y == 0 {
                    (dx > 1 || dz > 1) && dx != 0 && dz != 0
                } else {
                    dx == r && dz == r && r > 0
                }
            }
            FoliagePlacer::DarkOak { .. } => {
                if y == -1 && !dt {
                    dx == r && dz == r
                } else {
                    y == 1 && dx + dz > r * 2 - 2
                }
            }
            FoliagePlacer::RandomSpread { .. } => false,
            FoliagePlacer::Cherry {
                wide_bottom_layer_hole_chance,
                corner_hole_chance,
                ..
            } => {
                if y == -1
                    && (dx == r || dz == r)
                    && self.rng.next_float() < wide_bottom_layer_hole_chance
                {
                    return true;
                }
                let corner = dx == r && dz == r;
                if r > 2 {
                    corner || (dx + dz > r * 2 - 2 && self.rng.next_float() < corner_hole_chance)
                } else {
                    corner && self.rng.next_float() < corner_hole_chance
                }
            }
        }
    }
}

/// `Mth.floor(float)`.
#[inline]
fn floor_f(v: f32) -> i32 {
    let i = v as i32;
    if v < i as f32 { i - 1 } else { i }
}
