//! `minecraft:tree` (Java `TreeFeature`).
//!
//! Order of operations, as in Java:
//! 1. tree height from the trunk placer, foliage height, foliage radius, then
//!    the root placer's trunk origin (all from the feature random);
//! 2. build-height check and `getMaxFreeTreeHeight` against `minimum_size`;
//! 3. roots (mangrove), trunk, then foliage for every attachment the trunk
//!    returned;
//! 4. decorators over the Y-sorted log/leaf/root lists;
//! 5. leaf `distance` recomputation over the bounding box (`updateLeaves`)
//!    and the shape update along the edge of the tree.
//!
//! Java collects logs, leaves and roots in `HashSet`s and decorators walk
//! them in hash-set order (stable-sorted by Y), so [`pos_set::JavaPosSet`]
//! reproduces that order exactly. Dirt placed under the trunk goes through
//! the trunk setter and therefore counts as a log, as in Java.
//!
//! Edge shape transitions are shared with templates through
//! [`crate::features::shape`], including native connection/support rules.
//! Scheduled ticks remain separate from these immediate state transitions.
//!
//! The `place_on_ground` decorator's `isSolidRender` test is approximated
//! (see `decorator::is_solid_render`), since the block data carries no
//! occlusion shapes.

mod decorator;
mod foliage;
mod pos_set;
mod root;
mod trunk;

pub(crate) use decorator::run_decorators;
pub(crate) use pos_set::JavaPosSet;

use crate::features::blocks::{BlockPos, Direction, JavaState};
use crate::features::config::{FeatureSize, TreeConfig};
use crate::features::eval::FeatureWorld;
use crate::features::java_blocks::{blocks as b, tags};
use crate::features::random::FeatureRandom;
use crate::features::shape::{VoxelShape, update_shape_at_edge};

/// Java `FoliagePlacer.FoliageAttachment`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FoliageAttachment {
    pos: BlockPos,
    radius_offset: i32,
    double_trunk: bool,
}

impl FoliageAttachment {
    #[inline]
    fn new(pos: BlockPos, radius_offset: i32, double_trunk: bool) -> Self {
        Self {
            pos,
            radius_offset,
            double_trunk,
        }
    }
}

/// `TreeFeature.validTreePos` on a state: air or `replaceable_by_trees`.
#[inline]
pub(crate) fn valid_tree_pos_state(s: JavaState) -> bool {
    s.is_air() || s.is_in(tags::REPLACEABLE_BY_TREES)
}

/// `TreeFeature.validTreePos`.
#[inline]
pub(crate) fn valid_tree_pos<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos) -> bool {
    valid_tree_pos_state(world.get_state(pos))
}

/// `TreeFeature.isAirOrLeaves`.
#[inline]
fn is_air_or_leaves<W: FeatureWorld + ?Sized>(world: &mut W, pos: BlockPos) -> bool {
    let s = world.get_state(pos);
    s.is_air() || s.is_in(tags::LEAVES)
}

/// State shared by the trunk, foliage and root placers of one tree.
struct TreeCtx<'a, W: FeatureWorld + ?Sized> {
    world: &'a mut W,
    rng: &'a mut FeatureRandom,
    cfg: &'a TreeConfig,
    roots: JavaPosSet,
    logs: JavaPosSet,
    foliage: JavaPosSet,
}

impl<W: FeatureWorld + ?Sized> TreeCtx<'_, W> {
    /// Root setter: record, then `setBlock(pos, state, 19)`.
    fn set_root(&mut self, pos: BlockPos, state: JavaState) {
        self.roots.add(pos);
        self.world.set_state(pos, state);
    }

    /// Trunk setter.
    fn set_log(&mut self, pos: BlockPos, state: JavaState) {
        self.logs.add(pos);
        self.world.set_state(pos, state);
    }

    /// Foliage setter.
    fn set_leaf(&mut self, pos: BlockPos, state: JavaState) {
        self.foliage.add(pos);
        self.world.set_state(pos, state);
    }

    /// `TreeFeature.doPlace`.
    fn do_place(&mut self, origin: BlockPos) -> bool {
        let cfg = self.cfg;
        let tree_height = trunk::tree_height(&cfg.trunk_placer, self.rng);
        let foliage_height = foliage::foliage_height(&cfg.foliage_placer, self.rng, tree_height);
        let trunk_length = tree_height - foliage_height;
        let leaf_radius = foliage::foliage_radius(&cfg.foliage_placer, self.rng, trunk_length);
        let trunk_origin = match &cfg.root_placer {
            Some(r) => root::trunk_origin(r, self.rng, origin),
            None => origin,
        };
        let min_y = origin.y.min(trunk_origin.y);
        let max_y = origin.y.max(trunk_origin.y) + tree_height + 1;
        if min_y < self.world.min_y() + 1 || max_y > self.world.max_y() {
            return false;
        }
        let min_clipped = match cfg.minimum_size {
            FeatureSize::TwoLayers {
                min_clipped_height, ..
            }
            | FeatureSize::ThreeLayers {
                min_clipped_height, ..
            } => min_clipped_height,
        };
        let max_free = self.max_free_tree_height(tree_height, trunk_origin);
        if max_free < tree_height && min_clipped.is_none_or(|m| max_free < m) {
            return false;
        }
        if let Some(rp) = &cfg.root_placer
            && !self.place_roots(rp, origin, trunk_origin)
        {
            return false;
        }
        let attachments = self.place_trunk(max_free, trunk_origin);
        for att in attachments {
            self.create_foliage(max_free, att, foliage_height, leaf_radius);
        }
        true
    }

    /// `TreeFeature.getMaxFreeTreeHeight`.
    fn max_free_tree_height(&mut self, height: i32, trunk_origin: BlockPos) -> i32 {
        for y in 0..=height + 1 {
            let r = size_at_height(&self.cfg.minimum_size, height, y);
            for x in -r..=r {
                for z in -r..=r {
                    let p = trunk_origin.offset(x, y, z);
                    if !self.trunk_is_free(p)
                        || (!self.cfg.ignore_vines && self.world.get_state(p).is(b::VINE))
                    {
                        return y - 2;
                    }
                }
            }
        }
        height
    }
}

/// `FeatureSize.getSizeAtHeight`.
fn size_at_height(size: &FeatureSize, height: i32, y: i32) -> i32 {
    match *size {
        FeatureSize::TwoLayers {
            limit,
            lower_size,
            upper_size,
            ..
        } => {
            if y < limit {
                lower_size
            } else {
                upper_size
            }
        }
        FeatureSize::ThreeLayers {
            limit,
            upper_limit,
            lower_size,
            middle_size,
            upper_size,
            ..
        } => {
            if y < limit {
                lower_size
            } else if y >= height - upper_limit {
                upper_size
            } else {
                middle_size
            }
        }
    }
}

/// Place a `minecraft:tree` feature (Java `TreeFeature.place`).
///
/// Returns `false` when the tree does not fit (build height, free space,
/// mangrove roots) or placed no log and no leaf. `pale_moss_patch` places the
/// `minecraft:pale_moss_patch` configured feature for the `pale_moss`
/// decorator; it is not called for trees without that decorator.
pub fn place<W, F>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &TreeConfig,
    origin: BlockPos,
    mut pale_moss_patch: F,
) -> bool
where
    W: FeatureWorld + ?Sized,
    F: FnMut(&mut W, &mut FeatureRandom, BlockPos) -> bool,
{
    let mut ctx = TreeCtx {
        world,
        rng,
        cfg,
        roots: JavaPosSet::new(),
        logs: JavaPosSet::new(),
        foliage: JavaPosSet::new(),
    };
    if !ctx.do_place(origin) || (ctx.logs.is_empty() && ctx.foliage.is_empty()) {
        return false;
    }
    let TreeCtx {
        world,
        rng,
        roots,
        logs,
        foliage,
        ..
    } = ctx;
    let mut decorations = Vec::new();
    if !cfg.decorators.is_empty() {
        run_decorators(
            world,
            rng,
            cfg.decorators,
            logs.to_vec(),
            foliage.to_vec(),
            roots.to_vec(),
            Some(&mut decorations),
            &mut pale_moss_patch,
        );
    }
    let Some(bounds) = Bounds::encapsulating(
        roots
            .iter()
            .chain(logs.iter())
            .chain(foliage.iter())
            .chain(decorations.iter().copied()),
    ) else {
        return false;
    };
    let shape = update_leaves(world, &bounds, &logs, &decorations, &roots);
    update_shape_at_edge(world, &shape);
    true
}

/// Inclusive block box (`BoundingBox`).
#[derive(Debug, Clone, Copy)]
struct Bounds {
    min: BlockPos,
    max: BlockPos,
}

impl Bounds {
    /// `BoundingBox.encapsulatingPositions`.
    fn encapsulating(mut it: impl Iterator<Item = BlockPos>) -> Option<Self> {
        let first = it.next()?;
        let mut b = Bounds {
            min: first,
            max: first,
        };
        for p in it {
            b.min = BlockPos::new(b.min.x.min(p.x), b.min.y.min(p.y), b.min.z.min(p.z));
            b.max = BlockPos::new(b.max.x.max(p.x), b.max.y.max(p.y), b.max.z.max(p.z));
        }
        Some(b)
    }

    #[inline]
    fn inside(&self, p: BlockPos) -> bool {
        p.x >= self.min.x
            && p.x <= self.max.x
            && p.y >= self.min.y
            && p.y <= self.max.y
            && p.z >= self.min.z
            && p.z <= self.max.z
    }

    #[inline]
    fn span(&self) -> (i32, i32, i32) {
        (
            self.max.x - self.min.x + 1,
            self.max.y - self.min.y + 1,
            self.max.z - self.min.z + 1,
        )
    }
}

/// `LeavesBlock.getOptionalDistanceAt`: 0 for logs, the leaf `distance`,
/// or none.
fn optional_distance(s: JavaState) -> Option<i32> {
    if s.is_in(tags::LOGS) {
        Some(0)
    } else if s.has("distance") && !s.is(b::SCAFFOLDING) {
        s.int_value("distance")
    } else {
        None
    }
}

/// `TreeFeature.updateLeaves`: bucketed flood fill from every log, setting
/// leaf distances 1..=6 inside the bounds (Java hash-set pop order kept).
fn update_leaves<W: FeatureWorld + ?Sized>(
    world: &mut W,
    bounds: &Bounds,
    logs: &JavaPosSet,
    decorations: &[BlockPos],
    roots: &JavaPosSet,
) -> VoxelShape {
    let (xs, ys, zs) = bounds.span();
    let mut shape = VoxelShape::new(bounds.min, [xs, ys, zs]);
    for p in decorations.iter().copied().chain(roots.iter()) {
        if bounds.inside(p) {
            shape.fill(p);
        }
    }
    let mut to_check: [JavaPosSet; 7] = std::array::from_fn(|_| JavaPosSet::new());
    for p in logs.iter() {
        to_check[0].add(p);
    }
    let mut smallest = 0usize;
    loop {
        while smallest >= 7 || !to_check[smallest].is_empty() {
            if smallest >= 7 {
                return shape;
            }
            let pos = to_check[smallest]
                .pop_first()
                .expect("non-empty distance bucket");
            if !bounds.inside(pos) {
                continue;
            }
            if smallest != 0 {
                let state = world.get_state(pos);
                world.set_state(pos, state.with_int("distance", smallest as i32));
            }
            shape.fill(pos);
            for dir in Direction::ALL {
                let n = pos.rel(dir);
                if !bounds.inside(n) || shape.contains(n) {
                    continue;
                }
                let Some(d) = optional_distance(world.get_state(n)) else {
                    continue;
                };
                let nd = d.min(smallest as i32 + 1) as usize;
                if nd < 7 {
                    to_check[nd].add(n);
                    smallest = smallest.min(nd);
                }
            }
        }
        smallest += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::blocks::StateProvider;
    use crate::features::config::{
        FoliagePlacer, RootPlacer, TreeDecorator, TrunkHeights, TrunkPlacer,
    };
    use crate::features::java_blocks::states;
    use crate::features::test_world::TestWorld;
    use crate::features::values::IntProvider;

    const SURFACE: i32 = 64;

    fn heights(base: i32, a: i32, b: i32) -> TrunkHeights {
        TrunkHeights {
            base_height: base,
            height_rand_a: a,
            height_rand_b: b,
        }
    }

    fn two_layers(limit: i32, lower: i32, upper: i32) -> FeatureSize {
        FeatureSize::TwoLayers {
            limit,
            lower_size: lower,
            upper_size: upper,
            min_clipped_height: None,
        }
    }

    fn leaves(block: crate::features::blocks::JavaBlock) -> StateProvider {
        StateProvider::Simple(
            block
                .default_state()
                .with_int("distance", 7)
                .with_bool("persistent", false)
                .with_bool("waterlogged", false),
        )
    }

    fn config(trunk: TrunkPlacer, foliage: FoliagePlacer, size: FeatureSize) -> TreeConfig {
        TreeConfig {
            trunk_provider: StateProvider::Simple(states::OAK_LOG),
            trunk_placer: trunk,
            foliage_provider: leaves(b::OAK_LEAVES),
            foliage_placer: foliage,
            root_placer: None,
            dirt_provider: StateProvider::Simple(states::DIRT),
            minimum_size: size,
            decorators: &[],
            ignore_vines: true,
            force_dirt: false,
        }
    }

    fn oak() -> TreeConfig {
        config(
            TrunkPlacer::Straight(heights(4, 2, 0)),
            FoliagePlacer::Blob {
                radius: IntProvider::Constant(2),
                offset: IntProvider::Constant(0),
                height: 3,
            },
            two_layers(1, 0, 1),
        )
    }

    /// Grass surface at `SURFACE - 1`.
    fn grass_world() -> TestWorld {
        let mut w = TestWorld::flat(SURFACE);
        for x in -24..=24 {
            for z in -24..=24 {
                w.blocks
                    .insert(BlockPos::new(x, SURFACE - 1, z), states::GRASS_BLOCK);
            }
        }
        w
    }

    fn no_patch(_: &mut TestWorld, _: &mut FeatureRandom, _: BlockPos) -> bool {
        false
    }

    fn grow(cfg: &TreeConfig, seed: i64) -> (bool, TestWorld) {
        let mut w = grass_world();
        let mut rng = FeatureRandom::new(seed);
        let ok = place(
            &mut w,
            &mut rng,
            cfg,
            BlockPos::new(0, SURFACE, 0),
            no_patch,
        );
        (ok, w)
    }

    fn logs_at_origin_column(w: &TestWorld) -> i32 {
        let mut h = 0;
        while w
            .blocks
            .get(&BlockPos::new(0, SURFACE + h, 0))
            .is_some_and(|s| s.is_in(tags::LOGS))
        {
            h += 1;
        }
        h
    }

    fn all_trunk_placers() -> Vec<(TrunkPlacer, i32, i32)> {
        // (placer, min height, max height) per `base + [0, a] + [0, b]`.
        vec![
            (TrunkPlacer::Straight(heights(4, 2, 0)), 4, 6),
            (TrunkPlacer::Forking(heights(5, 2, 2)), 5, 9),
            (TrunkPlacer::Giant(heights(13, 2, 14)), 13, 29),
            (TrunkPlacer::MegaJungle(heights(10, 2, 19)), 10, 31),
            (TrunkPlacer::DarkOak(heights(6, 2, 1)), 6, 9),
            (TrunkPlacer::Fancy(heights(3, 11, 0)), 3, 14),
            (
                TrunkPlacer::Bending {
                    heights: heights(4, 2, 0),
                    min_height_for_leaves: 3,
                    bend_length: IntProvider::Uniform { min: 1, max: 2 },
                },
                4,
                6,
            ),
            (
                TrunkPlacer::UpwardsBranching {
                    heights: heights(2, 1, 4),
                    extra_branch_steps: IntProvider::Uniform { min: 1, max: 4 },
                    place_branch_per_log_probability: 0.5,
                    extra_branch_length: IntProvider::Uniform { min: 0, max: 1 },
                    can_grow_through: tags::MANGROVE_LOGS_CAN_GROW_THROUGH,
                },
                2,
                7,
            ),
            (
                TrunkPlacer::Cherry {
                    heights: heights(7, 1, 0),
                    branch_count: IntProvider::WeightedList {
                        entries: &[
                            (IntProvider::Constant(1), 1),
                            (IntProvider::Constant(2), 1),
                            (IntProvider::Constant(3), 1),
                        ],
                        total_weight: 3,
                    },
                    branch_horizontal_length: IntProvider::Uniform { min: 2, max: 4 },
                    branch_start_offset_from_top: (-4, -3),
                    branch_end_offset_from_top: IntProvider::Uniform { min: -1, max: 0 },
                },
                7,
                8,
            ),
        ]
    }

    #[test]
    fn tree_height_in_range_for_every_trunk_placer() {
        for (placer, lo, hi) in all_trunk_placers() {
            let mut rng = FeatureRandom::new(99);
            for _ in 0..500 {
                let h = trunk::tree_height(&placer, &mut rng);
                assert!((lo..=hi).contains(&h), "{placer:?}: {h}");
            }
        }
    }

    #[test]
    fn straight_oak_trunk_height_and_dirt() {
        for seed in 0..40 {
            let (ok, w) = grow(&oak(), seed);
            assert!(ok);
            let h = logs_at_origin_column(&w);
            assert!((4..=6).contains(&h), "seed {seed}: trunk {h}");
            assert_eq!(w.blocks[&BlockPos::new(0, SURFACE - 1, 0)], states::DIRT);
            // Leaves have distances 1..=7 and never replace logs.
            for (p, s) in &w.blocks {
                if s.is(b::OAK_LEAVES) {
                    let d = s.int_value("distance").unwrap();
                    assert!((1..=7).contains(&d), "{p:?} distance {d}");
                }
            }
        }
    }

    /// Java's `updateLeaves` pops positions from hash-set buckets without
    /// skipping ones already filled, so a leaf queued at distance `t` and
    /// again at `t + 1` ends up with `t + 1`. Every leaf therefore holds its
    /// true flood-fill distance `t` (from any log) or `t + 1`, and leaves
    /// beyond 6 keep 7.
    #[test]
    fn leaf_distances_follow_the_flood_fill() {
        use std::collections::{HashMap, VecDeque};
        for seed in 0..10 {
            let (_, w) = grow(&oak(), seed);
            let mut dist: HashMap<BlockPos, i32> = HashMap::new();
            let mut queue = VecDeque::new();
            for (p, s) in &w.blocks {
                if s.is_in(tags::LOGS) {
                    dist.insert(*p, 0);
                    queue.push_back(*p);
                }
            }
            while let Some(p) = queue.pop_front() {
                let d = dist[&p];
                for dir in Direction::ALL {
                    let n = p.rel(dir);
                    if w.blocks.get(&n).is_some_and(|s| s.is(b::OAK_LEAVES))
                        && !dist.contains_key(&n)
                    {
                        dist.insert(n, d + 1);
                        queue.push_back(n);
                    }
                }
            }
            let mut leaves = 0;
            for (p, s) in &w.blocks {
                if !s.is(b::OAK_LEAVES) {
                    continue;
                }
                leaves += 1;
                let d = s.int_value("distance").unwrap();
                match dist.get(p) {
                    Some(&t) if t <= 6 => {
                        assert!(d == t || d == t + 1, "seed {seed}: {p:?} {d} vs {t}")
                    }
                    _ => assert_eq!(d, 7, "seed {seed}: {p:?}"),
                }
            }
            assert!(leaves > 0);
        }
    }

    #[test]
    fn every_trunk_placer_places_within_build_and_is_deterministic() {
        let foliage = FoliagePlacer::Blob {
            radius: IntProvider::Constant(2),
            offset: IntProvider::Constant(0),
            height: 3,
        };
        for (placer, _, hi) in all_trunk_placers() {
            let cfg = config(placer, foliage, two_layers(1, 0, 1));
            for seed in 0..10 {
                let (ok1, w1) = grow(&cfg, seed);
                let (ok2, w2) = grow(&cfg, seed);
                assert_eq!(ok1, ok2);
                assert_eq!(w1.blocks, w2.blocks, "{placer:?} not deterministic");
                assert!(ok1, "{placer:?} seed {seed} failed on open ground");
                for (p, s) in &w1.blocks {
                    if s.is(b::OAK_LOG) {
                        assert!(
                            p.y >= SURFACE && p.y <= SURFACE + 2 * hi,
                            "{placer:?}: log at {p:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn every_foliage_placer_only_replaces_valid_positions() {
        let r = |v| IntProvider::Constant(v);
        let placers = [
            FoliagePlacer::Blob {
                radius: r(2),
                offset: r(0),
                height: 3,
            },
            FoliagePlacer::Bush {
                radius: r(2),
                offset: r(1),
                height: 2,
            },
            FoliagePlacer::Fancy {
                radius: r(2),
                offset: r(4),
                height: 4,
            },
            FoliagePlacer::Jungle {
                radius: r(2),
                offset: r(0),
                height: 2,
            },
            FoliagePlacer::Spruce {
                radius: IntProvider::Uniform { min: 2, max: 3 },
                offset: IntProvider::Uniform { min: 0, max: 2 },
                trunk_height: IntProvider::Uniform { min: 1, max: 2 },
            },
            FoliagePlacer::Pine {
                radius: r(1),
                offset: r(1),
                height: IntProvider::Uniform { min: 3, max: 4 },
            },
            FoliagePlacer::Acacia {
                radius: r(2),
                offset: r(0),
            },
            FoliagePlacer::DarkOak {
                radius: r(0),
                offset: r(0),
            },
            FoliagePlacer::MegaPine {
                radius: r(0),
                offset: r(0),
                crown_height: IntProvider::Uniform { min: 13, max: 17 },
            },
            FoliagePlacer::RandomSpread {
                radius: r(3),
                offset: r(0),
                foliage_height: r(2),
                leaf_placement_attempts: 50,
            },
            FoliagePlacer::Cherry {
                radius: r(4),
                offset: r(0),
                height: r(5),
                wide_bottom_layer_hole_chance: 0.25,
                corner_hole_chance: 0.25,
                hanging_leaves_chance: 0.16666667,
                hanging_leaves_extension_chance: 0.33333334,
            },
        ];
        for foliage in placers {
            let cfg = config(
                TrunkPlacer::Straight(heights(6, 2, 0)),
                foliage,
                two_layers(1, 0, 1),
            );
            for seed in 0..10 {
                let mut w = grass_world();
                // A persistent leaf and a stone block inside the canopy must survive.
                let persistent = b::OAK_LEAVES.default_state().with_bool("persistent", true);
                w.blocks
                    .insert(BlockPos::new(1, SURFACE + 6, 1), persistent);
                w.blocks
                    .insert(BlockPos::new(-1, SURFACE + 6, 0), states::STONE);
                let mut rng = FeatureRandom::new(seed);
                place(
                    &mut w,
                    &mut rng,
                    &cfg,
                    BlockPos::new(0, SURFACE, 0),
                    no_patch,
                );
                assert_eq!(
                    w.blocks[&BlockPos::new(-1, SURFACE + 6, 0)],
                    states::STONE,
                    "{foliage:?}"
                );
                let kept = w.blocks[&BlockPos::new(1, SURFACE + 6, 1)];
                assert!(kept.is(b::OAK_LEAVES) && kept.value("persistent") == Some("true"));
                assert!(
                    w.blocks.values().any(|s| s.is(b::OAK_LEAVES)),
                    "{foliage:?}: no leaves"
                );
            }
        }
    }

    #[test]
    fn blocked_tree_is_rejected_without_changes() {
        let mut w = grass_world();
        // A stone ceiling two blocks above the origin leaves no room.
        for x in -2..=2 {
            for z in -2..=2 {
                w.blocks
                    .insert(BlockPos::new(x, SURFACE + 2, z), states::STONE);
            }
        }
        let before = w.blocks.clone();
        let mut rng = FeatureRandom::new(3);
        assert!(!place(
            &mut w,
            &mut rng,
            &oak(),
            BlockPos::new(0, SURFACE, 0),
            no_patch
        ));
        assert_eq!(w.blocks, before);
    }

    #[test]
    fn min_clipped_height_allows_short_trees() {
        let mut cfg = oak();
        cfg.minimum_size = FeatureSize::TwoLayers {
            limit: 1,
            lower_size: 0,
            upper_size: 1,
            min_clipped_height: Some(2),
        };
        let mut w = grass_world();
        for x in -2..=2 {
            for z in -2..=2 {
                w.blocks
                    .insert(BlockPos::new(x, SURFACE + 5, z), states::STONE);
            }
        }
        let mut rng = FeatureRandom::new(11);
        assert!(place(
            &mut w,
            &mut rng,
            &cfg,
            BlockPos::new(0, SURFACE, 0),
            no_patch
        ));
        // Free height is 5 - 2 = 3 logs.
        assert_eq!(logs_at_origin_column(&w), 3);
    }

    #[test]
    fn existing_dirt_is_kept_unless_forced() {
        let mut w = grass_world();
        w.blocks
            .insert(BlockPos::new(0, SURFACE - 1, 0), states::PODZOL);
        let mut rng = FeatureRandom::new(5);
        assert!(place(
            &mut w,
            &mut rng,
            &oak(),
            BlockPos::new(0, SURFACE, 0),
            no_patch
        ));
        assert_eq!(w.blocks[&BlockPos::new(0, SURFACE - 1, 0)], states::PODZOL);

        let mut cfg = oak();
        cfg.force_dirt = true;
        let mut w = grass_world();
        w.blocks
            .insert(BlockPos::new(0, SURFACE - 1, 0), states::PODZOL);
        let mut rng = FeatureRandom::new(5);
        assert!(place(
            &mut w,
            &mut rng,
            &cfg,
            BlockPos::new(0, SURFACE, 0),
            no_patch
        ));
        assert_eq!(w.blocks[&BlockPos::new(0, SURFACE - 1, 0)], states::DIRT);
    }

    #[test]
    fn vines_block_growth_unless_ignored() {
        let mut cfg = oak();
        cfg.ignore_vines = false;
        let mut w = grass_world();
        w.blocks
            .insert(BlockPos::new(0, SURFACE + 2, 0), states::VINE);
        let mut rng = FeatureRandom::new(1);
        assert!(!place(
            &mut w,
            &mut rng,
            &cfg,
            BlockPos::new(0, SURFACE, 0),
            no_patch
        ));
        cfg.ignore_vines = true;
        let mut rng = FeatureRandom::new(1);
        assert!(place(
            &mut w,
            &mut rng,
            &cfg,
            BlockPos::new(0, SURFACE, 0),
            no_patch
        ));
    }

    #[test]
    fn decorators_attach_only_where_allowed() {
        static DECORATORS: [TreeDecorator; 3] = [
            TreeDecorator::TrunkVine,
            TreeDecorator::LeaveVine { probability: 0.25 },
            TreeDecorator::Cocoa { probability: 1.0 },
        ];
        let mut cfg = config(
            TrunkPlacer::Straight(heights(6, 2, 0)),
            FoliagePlacer::Blob {
                radius: IntProvider::Constant(2),
                offset: IntProvider::Constant(0),
                height: 3,
            },
            two_layers(1, 0, 1),
        );
        cfg.decorators = &DECORATORS;
        cfg.trunk_provider = StateProvider::Simple(states::JUNGLE_LOG);
        for seed in 0..20 {
            let (ok, w) = grow(&cfg, seed);
            assert!(ok);
            for (p, s) in &w.blocks {
                if s.is(b::VINE) {
                    // Every vine face points at a log, a leaf, or a vine above with that face.
                    for d in Direction::HORIZONTAL {
                        if s.value(d.name()) == Some("true") {
                            let support = w.blocks.get(&p.rel(d)).copied().unwrap_or(states::AIR);
                            let above = w.blocks.get(&p.above()).copied().unwrap_or(states::AIR);
                            assert!(
                                support.is_in(tags::LOGS)
                                    || support.is_in(tags::LEAVES)
                                    || (above.is(b::VINE) && above.value(d.name()) == Some("true")),
                                "unsupported vine at {p:?}"
                            );
                        }
                    }
                }
                if s.is(b::COCOA) {
                    let facing = match s.value("facing").unwrap() {
                        "north" => Direction::North,
                        "south" => Direction::South,
                        "west" => Direction::West,
                        _ => Direction::East,
                    };
                    assert!(w.blocks[&p.rel(facing)].is(b::JUNGLE_LOG), "cocoa at {p:?}");
                    assert!(p.y <= SURFACE + 1, "cocoa too high at {p:?}");
                }
            }
        }
    }

    #[test]
    fn mega_spruce_podzol_only_replaces_dirt() {
        static DECORATORS: [TreeDecorator; 1] = [TreeDecorator::AlterGround {
            provider: StateProvider::Simple(states::PODZOL),
        }];
        let mut cfg = config(
            TrunkPlacer::Giant(heights(13, 2, 14)),
            FoliagePlacer::MegaPine {
                radius: IntProvider::Constant(0),
                offset: IntProvider::Constant(0),
                crown_height: IntProvider::Uniform { min: 13, max: 17 },
            },
            two_layers(1, 1, 2),
        );
        cfg.decorators = &DECORATORS;
        for seed in 0..5 {
            let mut w = grass_world();
            // A stone patch near the trunk must stay stone.
            w.blocks
                .insert(BlockPos::new(-2, SURFACE - 1, -2), states::STONE);
            let mut rng = FeatureRandom::new(seed);
            assert!(place(
                &mut w,
                &mut rng,
                &cfg,
                BlockPos::new(0, SURFACE, 0),
                no_patch
            ));
            assert_eq!(w.blocks[&BlockPos::new(-2, SURFACE - 1, -2)], states::STONE);
            let podzol = w.blocks.values().filter(|s| s.is(b::PODZOL)).count();
            assert!(podzol > 0, "seed {seed}: no podzol");
            for (p, s) in &w.blocks {
                if s.is(b::PODZOL) {
                    // Grass ground layer or the dirt under the 2x2 trunk.
                    assert_eq!(p.y, SURFACE - 1, "podzol at {p:?}");
                }
                if s.is(b::OAK_LOG) {
                    assert!(
                        (0..=1).contains(&p.x) && (0..=1).contains(&p.z),
                        "log at {p:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn beehive_faces_south_into_air() {
        static DECORATORS: [TreeDecorator; 1] = [TreeDecorator::Beehive { probability: 1.0 }];
        let mut cfg = oak();
        cfg.decorators = &DECORATORS;
        let mut found = 0;
        for seed in 0..10 {
            let (_, w) = grow(&cfg, seed);
            for (p, s) in &w.blocks {
                if s.is(b::BEE_NEST) {
                    found += 1;
                    assert_eq!(s.value("facing"), Some("south"));
                    assert!(
                        w.blocks
                            .get(&p.rel(Direction::South))
                            .is_none_or(|s| s.is_air())
                    );
                }
            }
        }
        assert!(found > 0);
    }

    #[test]
    fn mangrove_roots_reach_ground_or_fail() {
        let mut cfg = config(
            TrunkPlacer::UpwardsBranching {
                heights: heights(2, 1, 4),
                extra_branch_steps: IntProvider::Uniform { min: 1, max: 4 },
                place_branch_per_log_probability: 0.5,
                extra_branch_length: IntProvider::Uniform { min: 0, max: 1 },
                can_grow_through: tags::MANGROVE_LOGS_CAN_GROW_THROUGH,
            },
            FoliagePlacer::RandomSpread {
                radius: IntProvider::Constant(3),
                offset: IntProvider::Constant(0),
                foliage_height: IntProvider::Constant(2),
                leaf_placement_attempts: 70,
            },
            two_layers(2, 0, 2),
        );
        cfg.trunk_provider = StateProvider::Simple(states::MANGROVE_LOG);
        cfg.foliage_provider = leaves(b::MANGROVE_LEAVES);
        cfg.root_placer = Some(RootPlacer::Mangrove {
            trunk_offset_y: IntProvider::Uniform { min: 1, max: 3 },
            root_provider: StateProvider::Simple(
                b::MANGROVE_ROOTS
                    .default_state()
                    .with_bool("waterlogged", false),
            ),
            above_root_placement: Some((StateProvider::Simple(states::MOSS_CARPET), 0.5)),
            can_grow_through: tags::MANGROVE_ROOTS_CAN_GROW_THROUGH,
            muddy_roots_in: tags::MANGROVE_ROOTS_CAN_GROW_THROUGH,
            muddy_roots_provider: StateProvider::Simple(states::MUDDY_MANGROVE_ROOTS),
            max_root_width: 8,
            max_root_length: 15,
            random_skew_chance: 0.2,
        });
        let mut placed = 0;
        for seed in 0..10 {
            let (ok, w) = grow(&cfg, seed);
            if ok {
                placed += 1;
                // Roots stay within the root width of the trunk.
                for (p, s) in &w.blocks {
                    if s.is(b::MANGROVE_ROOTS) {
                        assert!(p.x.abs() + p.z.abs() <= 8 + 1, "root at {p:?}");
                        assert!(p.y >= SURFACE, "root dug into ground at {p:?}");
                    }
                }
            }
        }
        assert!(placed > 0);
        // Floating over a void deeper than the root length: always rejected.
        let mut w = TestWorld::flat(-60);
        let mut rng = FeatureRandom::new(1);
        assert!(!place(
            &mut w,
            &mut rng,
            &cfg,
            BlockPos::new(0, SURFACE, 0),
            no_patch
        ));
    }

    #[test]
    fn orphaned_tall_grass_half_is_removed() {
        let mut w = grass_world();
        let lower = b::TALL_GRASS.default_state().with("half", "lower");
        let upper = b::TALL_GRASS.default_state().with("half", "upper");
        // Five-log trunk; the blob row at SURFACE + 2 has radius 2, so the
        // upper half at (2, SURFACE + 2, 0) becomes a leaf while the lower
        // half (standing on dirt) stays outside the tree.
        let base = BlockPos::new(2, SURFACE + 1, 0);
        w.blocks.insert(base.below(), states::DIRT);
        w.blocks.insert(base, lower);
        w.blocks.insert(base.above(), upper);
        let mut rng = FeatureRandom::new(0);
        let cfg = config(
            TrunkPlacer::Straight(heights(5, 0, 0)),
            FoliagePlacer::Blob {
                radius: IntProvider::Constant(2),
                offset: IntProvider::Constant(0),
                height: 3,
            },
            two_layers(1, 0, 1),
        );
        assert!(place(
            &mut w,
            &mut rng,
            &cfg,
            BlockPos::new(0, SURFACE, 0),
            no_patch
        ));
        assert!(w.blocks[&base.above()].is(b::OAK_LEAVES));
        assert_eq!(w.blocks[&base], states::AIR);
    }

    #[test]
    fn pale_moss_patch_closure_called_at_root() {
        static DECORATORS: [TreeDecorator; 1] = [TreeDecorator::PaleMoss {
            leaves_probability: 0.5,
            trunk_probability: 0.5,
            ground_probability: 1.0,
        }];
        let mut cfg = oak();
        cfg.decorators = &DECORATORS;
        let mut w = grass_world();
        let mut rng = FeatureRandom::new(2);
        let mut calls = Vec::new();
        assert!(place(
            &mut w,
            &mut rng,
            &cfg,
            BlockPos::new(0, SURFACE, 0),
            |_, _, p| {
                calls.push(p);
                true
            }
        ));
        // The lowest log entry is the dirt block under the trunk.
        assert_eq!(calls, vec![BlockPos::new(0, SURFACE, 0)]);
        for (p, s) in &w.blocks {
            if s.is(b::PALE_HANGING_MOSS) {
                let above = w.blocks[&p.above()];
                assert!(
                    above.is_in(tags::LOGS)
                        || above.is_in(tags::LEAVES)
                        || above.is(b::PALE_HANGING_MOSS)
                );
            }
        }
    }
}
