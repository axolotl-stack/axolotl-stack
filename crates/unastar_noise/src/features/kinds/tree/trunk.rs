//! Trunk placers (`TrunkPlacer` subclasses).

use super::{FoliageAttachment, TreeCtx, is_air_or_leaves, valid_tree_pos};
use crate::features::blocks::{BlockPos, Direction};
use crate::features::config::{TrunkHeights, TrunkPlacer};
use crate::features::eval::{FeatureWorld, provide_state};
use crate::features::java_blocks::{blocks as b, tags};
use crate::features::random::FeatureRandom;

/// Heights shared by every trunk placer.
fn heights(placer: &TrunkPlacer) -> &TrunkHeights {
    match placer {
        TrunkPlacer::Straight(h)
        | TrunkPlacer::Forking(h)
        | TrunkPlacer::Giant(h)
        | TrunkPlacer::MegaJungle(h)
        | TrunkPlacer::DarkOak(h)
        | TrunkPlacer::Fancy(h) => h,
        TrunkPlacer::Bending { heights, .. }
        | TrunkPlacer::UpwardsBranching { heights, .. }
        | TrunkPlacer::Cherry { heights, .. } => heights,
    }
}

/// `TrunkPlacer.getTreeHeight`: `base + nextInt(a + 1) + nextInt(b + 1)`.
pub(super) fn tree_height(placer: &TrunkPlacer, rng: &mut FeatureRandom) -> i32 {
    let h = heights(placer);
    let a = rng.next_int_bounded(h.height_rand_a + 1);
    let b = rng.next_int_bounded(h.height_rand_b + 1);
    h.base_height + a + b
}

/// `Direction.Plane.HORIZONTAL.getRandomDirection(random)`.
#[inline]
pub(super) fn random_horizontal(rng: &mut FeatureRandom) -> Direction {
    Direction::HORIZONTAL[rng.next_int_bounded(4) as usize]
}

/// `Mth.sin` / `Mth.cos` lookup-table values (65536 entries over a turn).
#[inline]
fn mth_table(index: i64) -> f32 {
    let i = (index & 65535) as f64;
    (i * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32
}

#[inline]
fn mth_sin(v: f64) -> f32 {
    mth_table((v * 10430.378350470453) as i64)
}

#[inline]
fn mth_cos(v: f64) -> f32 {
    mth_table((v * 10430.378350470453 + 16384.0) as i64)
}

/// `Mth.floor(float)`.
#[inline]
fn floor_f(v: f32) -> i32 {
    let i = v as i32;
    if v < i as f32 { i - 1 } else { i }
}

/// `Mth.floor(double)`.
#[inline]
fn floor_d(v: f64) -> i32 {
    let i = v as i32;
    if v < i as f64 { i - 1 } else { i }
}

#[inline]
fn dist_manhattan(a: BlockPos, b: BlockPos) -> i32 {
    (a.x - b.x).abs() + (a.y - b.y).abs() + (a.z - b.z).abs()
}

impl<W: FeatureWorld + ?Sized> TreeCtx<'_, W> {
    /// `TrunkPlacer.validTreePos` (upwards branching also accepts
    /// `can_grow_through`).
    pub(super) fn trunk_valid_pos(&mut self, pos: BlockPos) -> bool {
        if valid_tree_pos(self.world, pos) {
            return true;
        }
        match self.cfg.trunk_placer {
            TrunkPlacer::UpwardsBranching {
                can_grow_through, ..
            } => self.world.get_state(pos).is_in(can_grow_through),
            _ => false,
        }
    }

    /// `TrunkPlacer.isFree`: valid tree position or an existing log.
    pub(super) fn trunk_is_free(&mut self, pos: BlockPos) -> bool {
        self.trunk_valid_pos(pos) || self.world.get_state(pos).is_in(tags::LOGS)
    }

    /// `TrunkPlacer.placeLog` with an optional `axis` override (Java's
    /// `trySetValue(AXIS, ...)` state modifier).
    pub(super) fn place_log(&mut self, pos: BlockPos, axis: Option<&'static str>) -> bool {
        if !self.trunk_valid_pos(pos) {
            return false;
        }
        let mut state = provide_state(&self.cfg.trunk_provider, self.rng, pos);
        if let Some(axis) = axis {
            state = state.try_with("axis", axis);
        }
        self.set_log(pos, state);
        true
    }

    fn place_log_if_free(&mut self, pos: BlockPos) {
        if self.trunk_is_free(pos) {
            self.place_log(pos, None);
        }
    }

    /// `TrunkPlacer.setDirtAt`: replaced (through the trunk setter, so the
    /// position joins the log set) unless `force_dirt` is off and the block is
    /// already dirt other than grass and mycelium.
    pub(super) fn set_dirt_at(&mut self, pos: BlockPos) {
        if !self.cfg.force_dirt {
            let s = self.world.get_state(pos);
            if s.is_in(tags::DIRT) && !s.is(b::GRASS_BLOCK) && !s.is(b::MYCELIUM) {
                return;
            }
        }
        let state = provide_state(&self.cfg.dirt_provider, self.rng, pos);
        self.set_log(pos, state);
    }

    /// `TrunkPlacer.placeTrunk`.
    pub(super) fn place_trunk(&mut self, height: i32, origin: BlockPos) -> Vec<FoliageAttachment> {
        match self.cfg.trunk_placer {
            TrunkPlacer::Straight(_) => self.straight(height, origin),
            TrunkPlacer::Forking(_) => self.forking(height, origin),
            TrunkPlacer::Giant(_) => self.giant(height, origin),
            TrunkPlacer::MegaJungle(_) => self.mega_jungle(height, origin),
            TrunkPlacer::DarkOak(_) => self.dark_oak(height, origin),
            TrunkPlacer::Fancy(_) => self.fancy(height, origin),
            TrunkPlacer::Bending {
                min_height_for_leaves,
                bend_length,
                ..
            } => {
                let mut out = Vec::new();
                let dir = random_horizontal(self.rng);
                let log_height = height - 1;
                let mut pos = origin;
                self.set_dirt_at(pos.below());
                for i in 0..=log_height {
                    if i + 1 >= log_height + self.rng.next_int_bounded(2) {
                        pos = pos.rel(dir);
                    }
                    if valid_tree_pos(self.world, pos) {
                        self.place_log(pos, None);
                    }
                    if i >= min_height_for_leaves {
                        out.push(FoliageAttachment::new(pos, 0, false));
                    }
                    pos = pos.above();
                }
                let bend = bend_length.sample(self.rng);
                for _ in 0..=bend {
                    if valid_tree_pos(self.world, pos) {
                        self.place_log(pos, None);
                    }
                    out.push(FoliageAttachment::new(pos, 0, false));
                    pos = pos.rel(dir);
                }
                out
            }
            TrunkPlacer::UpwardsBranching {
                extra_branch_steps,
                place_branch_per_log_probability,
                extra_branch_length,
                ..
            } => {
                let mut out = Vec::new();
                for hp in 0..height {
                    let y = origin.y + hp;
                    let pos = BlockPos::new(origin.x, y, origin.z);
                    if self.place_log(pos, None)
                        && hp < height - 1
                        && self.rng.next_float() < place_branch_per_log_probability
                    {
                        let dir = random_horizontal(self.rng);
                        let len = extra_branch_length.sample(self.rng);
                        let branch_pos = 0.max(len - extra_branch_length.sample(self.rng) - 1);
                        let steps = extra_branch_steps.sample(self.rng);
                        self.upwards_branch(height, &mut out, pos, y, dir, branch_pos, steps);
                    }
                    if hp == height - 1 {
                        out.push(FoliageAttachment::new(
                            BlockPos::new(origin.x, y + 1, origin.z),
                            0,
                            false,
                        ));
                    }
                }
                out
            }
            TrunkPlacer::Cherry {
                branch_count,
                branch_horizontal_length,
                branch_start_offset_from_top,
                branch_end_offset_from_top,
                ..
            } => {
                self.set_dirt_at(origin.below());
                let (start_min, start_max) = branch_start_offset_from_top;
                let first =
                    0.max(height - 1 + self.rng.next_int_between_inclusive(start_min, start_max));
                let mut second = 0.max(
                    height - 1
                        + self
                            .rng
                            .next_int_between_inclusive(start_min, start_max - 1),
                );
                if second >= first {
                    second += 1;
                }
                let count = branch_count.sample(self.rng);
                let middle = count == 3;
                let both_sides = count >= 2;
                let trunk_height = if middle {
                    height
                } else if both_sides {
                    first.max(second) + 1
                } else {
                    first + 1
                };
                for y in 0..trunk_height {
                    self.place_log(origin.offset(0, y, 0), None);
                }
                let mut out = Vec::new();
                if middle {
                    out.push(FoliageAttachment::new(
                        origin.offset(0, trunk_height, 0),
                        0,
                        false,
                    ));
                }
                let dir = random_horizontal(self.rng);
                let side_axis = dir.axis();
                let a = self.cherry_branch(
                    height,
                    origin,
                    side_axis,
                    dir,
                    first,
                    first < trunk_height - 1,
                    branch_horizontal_length,
                    branch_end_offset_from_top,
                );
                out.push(a);
                if both_sides {
                    let a = self.cherry_branch(
                        height,
                        origin,
                        side_axis,
                        dir.opposite(),
                        second,
                        second < trunk_height - 1,
                        branch_horizontal_length,
                        branch_end_offset_from_top,
                    );
                    out.push(a);
                }
                out
            }
        }
    }

    /// `StraightTrunkPlacer`.
    fn straight(&mut self, height: i32, origin: BlockPos) -> Vec<FoliageAttachment> {
        self.set_dirt_at(origin.below());
        for hy in 0..height {
            self.place_log(origin.offset(0, hy, 0), None);
        }
        vec![FoliageAttachment::new(
            origin.offset(0, height, 0),
            0,
            false,
        )]
    }

    /// `ForkingTrunkPlacer` (acacia).
    fn forking(&mut self, height: i32, origin: BlockPos) -> Vec<FoliageAttachment> {
        self.set_dirt_at(origin.below());
        let mut out = Vec::new();
        let lean_dir = random_horizontal(self.rng);
        let lean_height = height - self.rng.next_int_bounded(4) - 1;
        let mut lean_steps = 3 - self.rng.next_int_bounded(3);
        let (mut tx, mut tz) = (origin.x, origin.z);
        let mut foliage_y = None;
        for hy in 0..height {
            let yy = origin.y + hy;
            if hy >= lean_height && lean_steps > 0 {
                let (dx, _, dz) = lean_dir.offset();
                tx += dx;
                tz += dz;
                lean_steps -= 1;
            }
            if self.place_log(BlockPos::new(tx, yy, tz), None) {
                foliage_y = Some(yy + 1);
            }
        }
        if let Some(fy) = foliage_y {
            out.push(FoliageAttachment::new(BlockPos::new(tx, fy, tz), 1, false));
        }
        tx = origin.x;
        tz = origin.z;
        let branch_dir = random_horizontal(self.rng);
        if branch_dir != lean_dir {
            let branch_pos = lean_height - self.rng.next_int_bounded(2) - 1;
            let mut branch_steps = 1 + self.rng.next_int_bounded(3);
            foliage_y = None;
            let mut hy = branch_pos;
            while hy < height && branch_steps > 0 {
                if hy >= 1 {
                    let yy = origin.y + hy;
                    let (dx, _, dz) = branch_dir.offset();
                    tx += dx;
                    tz += dz;
                    if self.place_log(BlockPos::new(tx, yy, tz), None) {
                        foliage_y = Some(yy + 1);
                    }
                }
                hy += 1;
                branch_steps -= 1;
            }
            if let Some(fy) = foliage_y {
                out.push(FoliageAttachment::new(BlockPos::new(tx, fy, tz), 0, false));
            }
        }
        out
    }

    /// `GiantTrunkPlacer` (2x2 trunk).
    fn giant(&mut self, height: i32, origin: BlockPos) -> Vec<FoliageAttachment> {
        let below = origin.below();
        self.set_dirt_at(below);
        self.set_dirt_at(below.offset(1, 0, 0));
        self.set_dirt_at(below.offset(0, 0, 1));
        self.set_dirt_at(below.offset(1, 0, 1));
        for hy in 0..height {
            self.place_log_if_free(origin.offset(0, hy, 0));
            if hy < height - 1 {
                self.place_log_if_free(origin.offset(1, hy, 0));
                self.place_log_if_free(origin.offset(1, hy, 1));
                self.place_log_if_free(origin.offset(0, hy, 1));
            }
        }
        vec![FoliageAttachment::new(origin.offset(0, height, 0), 0, true)]
    }

    /// `MegaJungleTrunkPlacer`: giant trunk plus sloped branches.
    fn mega_jungle(&mut self, height: i32, origin: BlockPos) -> Vec<FoliageAttachment> {
        let mut out = self.giant(height, origin);
        let mut branch_height = height - 2 - self.rng.next_int_bounded(4);
        while branch_height > height / 2 {
            let angle = self.rng.next_float() * (std::f32::consts::PI * 2.0);
            let (mut bx, mut bz) = (0, 0);
            for i in 0..5 {
                bx = (1.5f32 + mth_cos(angle as f64) * i as f32) as i32;
                bz = (1.5f32 + mth_sin(angle as f64) * i as f32) as i32;
                let pos = origin.offset(bx, branch_height - 3 + i / 2, bz);
                self.place_log(pos, None);
            }
            out.push(FoliageAttachment::new(
                origin.offset(bx, branch_height, bz),
                -2,
                false,
            ));
            branch_height -= 2 + self.rng.next_int_bounded(4);
        }
        out
    }

    /// `DarkOakTrunkPlacer`.
    fn dark_oak(&mut self, height: i32, origin: BlockPos) -> Vec<FoliageAttachment> {
        let mut out = Vec::new();
        let below = origin.below();
        self.set_dirt_at(below);
        self.set_dirt_at(below.offset(1, 0, 0));
        self.set_dirt_at(below.offset(0, 0, 1));
        self.set_dirt_at(below.offset(1, 0, 1));
        let lean_dir = random_horizontal(self.rng);
        let lean_height = height - self.rng.next_int_bounded(4);
        let mut lean_steps = 2 - self.rng.next_int_bounded(3);
        let (ox, oy, oz) = (origin.x, origin.y, origin.z);
        let (mut tx, mut tz) = (ox, oz);
        let ey = oy + height - 1;
        for dy in 0..height {
            if dy >= lean_height && lean_steps > 0 {
                let (dx, _, dz) = lean_dir.offset();
                tx += dx;
                tz += dz;
                lean_steps -= 1;
            }
            let p = BlockPos::new(tx, oy + dy, tz);
            if is_air_or_leaves(self.world, p) {
                self.place_log(p, None);
                self.place_log(p.offset(1, 0, 0), None);
                self.place_log(p.offset(0, 0, 1), None);
                self.place_log(p.offset(1, 0, 1), None);
            }
        }
        out.push(FoliageAttachment::new(BlockPos::new(tx, ey, tz), 0, true));
        for bx in -1..=2 {
            for bz in -1..=2 {
                if (!(0..=1).contains(&bx) || !(0..=1).contains(&bz))
                    && self.rng.next_int_bounded(3) <= 0
                {
                    let length = self.rng.next_int_bounded(3) + 2;
                    for i in 0..length {
                        self.place_log(BlockPos::new(ox + bx, ey - i - 1, oz + bz), None);
                    }
                    out.push(FoliageAttachment::new(
                        BlockPos::new(ox + bx, ey, oz + bz),
                        0,
                        false,
                    ));
                }
            }
        }
        out
    }

    /// `FancyTrunkPlacer` (large oak).
    fn fancy(&mut self, tree_height: i32, origin: BlockPos) -> Vec<FoliageAttachment> {
        let height = tree_height + 2;
        let trunk_height = floor_d(height as f64 * 0.618);
        self.set_dirt_at(origin.below());
        let clusters_per_y = 1.min(floor_d(1.382 + (height as f64 / 13.0).powf(2.0)));
        let trunk_top = origin.y + trunk_height;
        let mut relative_y = height - 5;
        // (attachment position, branch base Y)
        let mut coords: Vec<(BlockPos, i32)> = vec![(origin.offset(0, relative_y, 0), trunk_top)];
        while relative_y >= 0 {
            let shape = fancy_tree_shape(height, relative_y);
            if shape >= 0.0 {
                for _ in 0..clusters_per_y {
                    let radius = shape as f64 * (self.rng.next_float() as f64 + 0.328);
                    let angle = (self.rng.next_float() * 2.0f32) as f64 * std::f64::consts::PI;
                    let x = radius * angle.sin() + 0.5;
                    let z = radius * angle.cos() + 0.5;
                    let check_start = origin.offset(floor_d(x), relative_y - 1, floor_d(z));
                    let check_end = check_start.offset(0, 5, 0);
                    if self.fancy_limb(check_start, check_end, false) {
                        let dx = origin.x - check_start.x;
                        let dz = origin.z - check_start.z;
                        let branch_height =
                            check_start.y as f64 - ((dx * dx + dz * dz) as f64).sqrt() * 0.381;
                        let branch_top = if branch_height > trunk_top as f64 {
                            trunk_top
                        } else {
                            branch_height as i32
                        };
                        let base = BlockPos::new(origin.x, branch_top, origin.z);
                        if self.fancy_limb(base, check_start, false) {
                            coords.push((check_start, base.y));
                        }
                    }
                }
            }
            relative_y -= 1;
        }
        self.fancy_limb(origin, origin.offset(0, trunk_height, 0), true);
        // makeBranches
        for &(pos, branch_base) in &coords {
            let base = BlockPos::new(origin.x, branch_base, origin.z);
            if base != pos && fancy_trim(height, branch_base - origin.y) {
                self.fancy_limb(base, pos, true);
            }
        }
        coords
            .iter()
            .filter(|(_, base)| fancy_trim(height, base - origin.y))
            .map(|&(pos, _)| FoliageAttachment::new(pos, 0, false))
            .collect()
    }

    /// `FancyTrunkPlacer.makeLimb`: walk from `start` to `end`; place logs
    /// (axis along the dominant horizontal offset) or only test `isFree`.
    fn fancy_limb(&mut self, start: BlockPos, end: BlockPos, place: bool) -> bool {
        if !place && start == end {
            return true;
        }
        let (dx, dy, dz) = (end.x - start.x, end.y - start.y, end.z - start.z);
        let steps = dx.abs().max(dy.abs().max(dz.abs()));
        let fx = dx as f32 / steps as f32;
        let fy = dy as f32 / steps as f32;
        let fz = dz as f32 / steps as f32;
        for i in 0..=steps {
            let p = start.offset(
                floor_f(0.5 + i as f32 * fx),
                floor_f(0.5 + i as f32 * fy),
                floor_f(0.5 + i as f32 * fz),
            );
            if place {
                let xd = (p.x - start.x).abs();
                let zd = (p.z - start.z).abs();
                let m = xd.max(zd);
                let axis = if m > 0 {
                    if xd == m { "x" } else { "z" }
                } else {
                    "y"
                };
                self.place_log(p, Some(axis));
            } else if !self.trunk_is_free(p) {
                return false;
            }
        }
        true
    }

    /// `UpwardsBranchingTrunkPlacer.placeBranch`.
    #[allow(clippy::too_many_arguments)]
    fn upwards_branch(
        &mut self,
        height: i32,
        out: &mut Vec<FoliageAttachment>,
        log_pos: BlockPos,
        current_height: i32,
        dir: Direction,
        branch_pos: i32,
        mut steps: i32,
    ) {
        let mut along = current_height + branch_pos;
        let (mut lx, mut lz) = (log_pos.x, log_pos.z);
        let mut d = branch_pos;
        while d < height && steps > 0 {
            if d >= 1 {
                let y = current_height + d;
                let (sx, _, sz) = dir.offset();
                lx += sx;
                lz += sz;
                along = y;
                let p = BlockPos::new(lx, y, lz);
                if self.place_log(p, None) {
                    along = y + 1;
                }
                out.push(FoliageAttachment::new(p, 0, false));
            }
            d += 1;
            steps -= 1;
        }
        if along - current_height > 1 {
            let fp = BlockPos::new(lx, along, lz);
            out.push(FoliageAttachment::new(fp, 0, false));
            out.push(FoliageAttachment::new(fp.offset(0, -2, 0), 0, false));
        }
    }

    /// `CherryTrunkPlacer.generateBranch`.
    #[allow(clippy::too_many_arguments)]
    fn cherry_branch(
        &mut self,
        height: i32,
        origin: BlockPos,
        side_axis: &'static str,
        dir: Direction,
        offset: i32,
        middle_continues_up: bool,
        horizontal_length: crate::features::values::IntProvider,
        end_offset: crate::features::values::IntProvider,
    ) -> FoliageAttachment {
        let mut log = origin.offset(0, offset, 0);
        let end_y_offset = height - 1 + end_offset.sample(self.rng);
        let extend = middle_continues_up || end_y_offset < offset;
        let distance = horizontal_length.sample(self.rng) + i32::from(extend);
        let end = origin.rel_n(dir, distance).offset(0, end_y_offset, 0);
        let horizontal_steps = if extend { 2 } else { 1 };
        for _ in 0..horizontal_steps {
            log = log.rel(dir);
            self.place_log(log, Some(side_axis));
        }
        let vertical = if end.y > log.y {
            Direction::Up
        } else {
            Direction::Down
        };
        loop {
            let dist = dist_manhattan(log, end);
            if dist == 0 {
                return FoliageAttachment::new(end.above(), 0, false);
            }
            let chance = (end.y - log.y).abs() as f32 / dist as f32;
            let grow_vertically = self.rng.next_float() < chance;
            log = log.rel(if grow_vertically { vertical } else { dir });
            self.place_log(
                log,
                if grow_vertically {
                    None
                } else {
                    Some(side_axis)
                },
            );
        }
    }
}

/// `FancyTrunkPlacer.treeShape`.
fn fancy_tree_shape(height: i32, y: i32) -> f32 {
    if (y as f32) < height as f32 * 0.3 {
        return -1.0;
    }
    let radius = height as f32 / 2.0;
    let adjacent = radius - y as f32;
    let mut distance = (radius * radius - adjacent * adjacent).sqrt();
    if adjacent == 0.0 {
        distance = radius;
    } else if adjacent.abs() >= radius {
        return 0.0;
    }
    distance * 0.5
}

/// `FancyTrunkPlacer.trimBranches`.
#[inline]
fn fancy_trim(height: i32, local_y: i32) -> bool {
    local_y as f64 >= height as f64 * 0.2
}
