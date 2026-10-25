//! Root placers (`RootPlacer`; vanilla has only the mangrove one).

use super::{TreeCtx, valid_tree_pos};
use crate::features::blocks::{BlockPos, Direction, JavaState};
use crate::features::config::RootPlacer;
use crate::features::eval::{FeatureWorld, provide_state};
use crate::features::random::FeatureRandom;

/// `RootPlacer.getTrunkOrigin`: `origin.above(trunk_offset_y.sample())`.
pub(super) fn trunk_origin(
    placer: &RootPlacer,
    rng: &mut FeatureRandom,
    origin: BlockPos,
) -> BlockPos {
    let RootPlacer::Mangrove { trunk_offset_y, .. } = placer;
    origin.offset(0, trunk_offset_y.sample(rng), 0)
}

#[inline]
fn dist_manhattan(a: BlockPos, b: BlockPos) -> i32 {
    (a.x - b.x).abs() + (a.y - b.y).abs() + (a.z - b.z).abs()
}

impl<W: FeatureWorld + ?Sized> TreeCtx<'_, W> {
    /// `MangroveRootPlacer.canPlaceRoot`: valid tree position or
    /// `can_grow_through`.
    fn can_place_root(&mut self, placer: &RootPlacer, pos: BlockPos) -> bool {
        let RootPlacer::Mangrove {
            can_grow_through, ..
        } = placer;
        valid_tree_pos(self.world, pos) || self.world.get_state(pos).is_in(*can_grow_through)
    }

    /// `RootPlacer.getPotentiallyWaterloggedState` (any water fluid).
    fn root_waterlogged(&mut self, pos: BlockPos, state: JavaState) -> JavaState {
        if state.has("waterlogged") {
            let water = self.world.fluid(pos).is_water();
            state.with_bool("waterlogged", water)
        } else {
            state
        }
    }

    /// `MangroveRootPlacer.placeRoots`: simulate all four root arms first and
    /// place nothing unless every arm finds ground within `max_root_length`.
    pub(super) fn place_roots(
        &mut self,
        placer: &RootPlacer,
        origin: BlockPos,
        trunk_origin: BlockPos,
    ) -> bool {
        let mut pos = origin;
        while pos.y < trunk_origin.y {
            if !self.can_place_root(placer, pos) {
                return false;
            }
            pos = pos.above();
        }
        let mut root_positions = vec![trunk_origin.below()];
        let mut arm = Vec::new();
        for dir in Direction::HORIZONTAL {
            let root_pos = trunk_origin.rel(dir);
            arm.clear();
            if !self.simulate_roots(placer, root_pos, dir, trunk_origin, &mut arm, 0) {
                return false;
            }
            root_positions.extend_from_slice(&arm);
            root_positions.push(trunk_origin.rel(dir));
        }
        for p in root_positions {
            self.place_root(placer, p);
        }
        true
    }

    fn simulate_roots(
        &mut self,
        placer: &RootPlacer,
        root_pos: BlockPos,
        dir: Direction,
        root_origin: BlockPos,
        positions: &mut Vec<BlockPos>,
        layer: i32,
    ) -> bool {
        let RootPlacer::Mangrove {
            max_root_length, ..
        } = *placer;
        if layer == max_root_length || positions.len() as i32 > max_root_length {
            return false;
        }
        let (candidates, count) = self.potential_root_positions(placer, root_pos, dir, root_origin);
        for &p in &candidates[..count] {
            if self.can_place_root(placer, p) {
                positions.push(p);
                if !self.simulate_roots(placer, p, dir, root_origin, positions, layer + 1) {
                    return false;
                }
            }
        }
        true
    }

    fn potential_root_positions(
        &mut self,
        placer: &RootPlacer,
        pos: BlockPos,
        prev_dir: Direction,
        root_origin: BlockPos,
    ) -> ([BlockPos; 2], usize) {
        let RootPlacer::Mangrove {
            max_root_width,
            random_skew_chance,
            ..
        } = *placer;
        let below = pos.below();
        let next_to = pos.rel(prev_dir);
        let distance = dist_manhattan(pos, root_origin);
        if distance > max_root_width - 3 && distance <= max_root_width {
            if self.rng.next_float() < random_skew_chance {
                ([below, next_to.below()], 2)
            } else {
                ([below, below], 1)
            }
        } else if distance > max_root_width || self.rng.next_float() < random_skew_chance {
            ([below, below], 1)
        } else if self.rng.next_bool() {
            ([next_to, next_to], 1)
        } else {
            ([below, below], 1)
        }
    }

    /// `MangroveRootPlacer.placeRoot`: muddy roots inside `muddy_roots_in`,
    /// otherwise a root (if placeable) with an optional block above it.
    fn place_root(&mut self, placer: &RootPlacer, pos: BlockPos) {
        let RootPlacer::Mangrove {
            root_provider,
            above_root_placement,
            muddy_roots_in,
            muddy_roots_provider,
            ..
        } = placer;
        if self.world.get_state(pos).is_in(*muddy_roots_in) {
            let s = provide_state(muddy_roots_provider, self.rng, pos);
            let s = self.root_waterlogged(pos, s);
            self.set_root(pos, s);
            return;
        }
        if !self.can_place_root(placer, pos) {
            return;
        }
        let s = provide_state(root_provider, self.rng, pos);
        let s = self.root_waterlogged(pos, s);
        self.set_root(pos, s);
        if let Some((above_provider, chance)) = above_root_placement {
            let above = pos.above();
            if self.rng.next_float() < *chance && self.world.is_air(above) {
                let s = provide_state(above_provider, self.rng, above);
                let s = self.root_waterlogged(above, s);
                self.set_root(above, s);
            }
        }
    }
}
