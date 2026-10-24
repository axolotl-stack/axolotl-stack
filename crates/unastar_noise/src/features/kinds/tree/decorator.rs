//! Tree decorators (`TreeDecorator` subclasses), shared by `tree` and
//! `fallen_tree`.
//!
//! Java builds one `TreeDecorator.Context` per tree: copies of the log, leaf
//! and root sets (in `HashSet` iteration order) stable-sorted by Y, shared by
//! every decorator of the tree, so blocks placed by one decorator never show
//! up in the lists another decorator walks.

use crate::features::blocks::{BlockPos, Direction, JavaState};
use crate::features::config::TreeDecorator;
use crate::features::eval::{FeatureWorld, provide_state};
use crate::features::java_blocks::{blocks as b, tags};
use crate::features::random::FeatureRandom;

struct Ctx<'a, W: FeatureWorld + ?Sized> {
    world: &'a mut W,
    rng: &'a mut FeatureRandom,
    logs: Vec<BlockPos>,
    leaves: Vec<BlockPos>,
    roots: Vec<BlockPos>,
    /// Decoration positions (TreeFeature records them for its bounding box).
    placed: Option<&'a mut Vec<BlockPos>>,
}

impl<W: FeatureWorld + ?Sized> Ctx<'_, W> {
    fn set_block(&mut self, pos: BlockPos, state: JavaState) -> bool {
        if let Some(p) = self.placed.as_deref_mut() {
            p.push(pos);
        }
        self.world.set_state(pos, state)
    }

    #[inline]
    fn is_air(&mut self, pos: BlockPos) -> bool {
        self.world.is_air(pos)
    }

    fn place_vine(&mut self, pos: BlockPos, face: Direction) {
        let state = b::VINE.default_state().with_bool(face.name(), true);
        self.set_block(pos, state);
    }
}

/// Run `decorators` in order over one context (Java `decorators.forEach(d ->
/// d.place(context))`).
///
/// `logs`, `leaves` and `roots` are in Java `HashSet` iteration order; they
/// are stable-sorted by Y here like the Java context does. `placed`, when
/// given, receives every decoration position (the tree feature's decoration
/// set). `pale_moss_patch` places the `minecraft:pale_moss_patch` configured
/// feature for [`TreeDecorator::PaleMoss`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_decorators<W, F>(
    world: &mut W,
    rng: &mut FeatureRandom,
    decorators: &[TreeDecorator],
    mut logs: Vec<BlockPos>,
    mut leaves: Vec<BlockPos>,
    mut roots: Vec<BlockPos>,
    placed: Option<&mut Vec<BlockPos>>,
    pale_moss_patch: &mut F,
) where
    W: FeatureWorld + ?Sized,
    F: FnMut(&mut W, &mut FeatureRandom, BlockPos) -> bool,
{
    logs.sort_by_key(|p| p.y);
    leaves.sort_by_key(|p| p.y);
    roots.sort_by_key(|p| p.y);
    let mut ctx = Ctx {
        world,
        rng,
        logs,
        leaves,
        roots,
        placed,
    };
    for d in decorators {
        match *d {
            TreeDecorator::TrunkVine => trunk_vine(&mut ctx),
            TreeDecorator::LeaveVine { probability } => leave_vine(&mut ctx, probability),
            TreeDecorator::Cocoa { probability } => cocoa(&mut ctx, probability),
            TreeDecorator::Beehive { probability } => beehive(&mut ctx, probability),
            TreeDecorator::AlterGround { ref provider } => alter_ground(&mut ctx, provider),
            TreeDecorator::AttachedToLeaves {
                probability,
                exclusion_radius_xz,
                exclusion_radius_y,
                ref block_provider,
                required_empty_blocks,
                directions,
            } => attached_to_leaves(
                &mut ctx,
                probability,
                exclusion_radius_xz,
                exclusion_radius_y,
                block_provider,
                required_empty_blocks,
                directions,
            ),
            TreeDecorator::AttachedToLogs {
                probability,
                ref block_provider,
                directions,
            } => attached_to_logs(&mut ctx, probability, block_provider, directions),
            TreeDecorator::PlaceOnGround {
                tries,
                radius,
                height,
                ref block_state_provider,
            } => place_on_ground(&mut ctx, tries, radius, height, block_state_provider),
            TreeDecorator::CreakingHeart { probability } => creaking_heart(&mut ctx, probability),
            TreeDecorator::PaleMoss {
                leaves_probability,
                trunk_probability,
                ground_probability,
            } => pale_moss(
                &mut ctx,
                leaves_probability,
                trunk_probability,
                ground_probability,
                pale_moss_patch,
            ),
        }
    }
}

/// `Util.shuffle(list, random)`: Fisher-Yates from the end.
pub(crate) fn shuffle(list: &mut [BlockPos], rng: &mut FeatureRandom) {
    let mut i = list.len();
    while i > 1 {
        let j = rng.next_int_bounded(i as i32) as usize;
        list.swap(i - 1, j);
        i -= 1;
    }
}

/// Vine faces placed by the vine decorators: (offset direction, vine face).
const VINE_SIDES: [(Direction, Direction); 4] = [
    (Direction::West, Direction::East),
    (Direction::East, Direction::West),
    (Direction::North, Direction::South),
    (Direction::South, Direction::North),
];

/// `TrunkVineDecorator`: per log and side, `nextInt(3) > 0` and air.
fn trunk_vine<W: FeatureWorld + ?Sized>(ctx: &mut Ctx<'_, W>) {
    for i in 0..ctx.logs.len() {
        let pos = ctx.logs[i];
        for (side, face) in VINE_SIDES {
            if ctx.rng.next_int_bounded(3) > 0 {
                let p = pos.rel(side);
                if ctx.is_air(p) {
                    ctx.place_vine(p, face);
                }
            }
        }
    }
}

/// `LeaveVineDecorator`: hanging vines up to 4 extra blocks long.
fn leave_vine<W: FeatureWorld + ?Sized>(ctx: &mut Ctx<'_, W>, probability: f32) {
    for i in 0..ctx.leaves.len() {
        let pos = ctx.leaves[i];
        for (side, face) in VINE_SIDES {
            if ctx.rng.next_float() < probability {
                let p = pos.rel(side);
                if ctx.is_air(p) {
                    ctx.place_vine(p, face);
                    let mut below = p.below();
                    let mut left = 4;
                    while ctx.is_air(below) && left > 0 {
                        ctx.place_vine(below, face);
                        below = below.below();
                        left -= 1;
                    }
                }
            }
        }
    }
}

/// `CocoaDecorator`: logs within two blocks of the lowest log entry.
fn cocoa<W: FeatureWorld + ?Sized>(ctx: &mut Ctx<'_, W>, probability: f32) {
    if ctx.rng.next_float() >= probability || ctx.logs.is_empty() {
        return;
    }
    let tree_y = ctx.logs[0].y;
    for i in 0..ctx.logs.len() {
        let pos = ctx.logs[i];
        if pos.y - tree_y > 2 {
            continue;
        }
        for dir in Direction::HORIZONTAL {
            if ctx.rng.next_float() <= 0.25 {
                let p = pos.rel(dir.opposite());
                if ctx.is_air(p) {
                    let age = ctx.rng.next_int_bounded(3);
                    let state = b::COCOA
                        .default_state()
                        .with_int("age", age)
                        .with("facing", dir.name());
                    ctx.set_block(p, state);
                }
            }
        }
    }
}

/// `BeehiveDecorator`. Writes the `bee_nest` state only; the bees are
/// block-entity data the runtime adds later. Java draws the bee count
/// (`2 + nextInt(2)`) and one `nextInt(599)` per bee from the tree random
/// once the nest block entity exists, so those draws are made here whenever
/// the nest was written.
fn beehive<W: FeatureWorld + ?Sized>(ctx: &mut Ctx<'_, W>, probability: f32) {
    if ctx.logs.is_empty() {
        return;
    }
    if ctx.rng.next_float() >= probability {
        return;
    }
    let first_log_y = ctx.logs[0].y;
    let hive_y = if let Some(leaf) = ctx.leaves.first() {
        (leaf.y - 1).max(first_log_y + 1)
    } else {
        let last_y = ctx.logs[ctx.logs.len() - 1].y;
        (first_log_y + 1 + ctx.rng.next_int_bounded(3)).min(last_y)
    };
    const SPAWN: [Direction; 3] = [Direction::East, Direction::South, Direction::West];
    let mut placements: Vec<BlockPos> = Vec::new();
    for &p in ctx.logs.iter().filter(|p| p.y == hive_y) {
        for d in SPAWN {
            placements.push(p.rel(d));
        }
    }
    if placements.is_empty() {
        return;
    }
    shuffle(&mut placements, ctx.rng);
    for p in placements {
        if ctx.is_air(p) && ctx.is_air(p.rel(Direction::South)) {
            let state = b::BEE_NEST.default_state().with("facing", "south");
            if ctx.set_block(p, state) {
                let bees = 2 + ctx.rng.next_int_bounded(2);
                for _ in 0..bees {
                    ctx.rng.next_int_bounded(599);
                }
            }
            return;
        }
    }
}

/// `TreeFeature.getLowestTrunkOrRootOfTree`.
fn lowest_trunk_or_root<W: FeatureWorld + ?Sized>(ctx: &Ctx<'_, W>) -> Vec<BlockPos> {
    let mut out = Vec::new();
    if ctx.roots.is_empty() {
        out.extend_from_slice(&ctx.logs);
    } else if !ctx.logs.is_empty() && ctx.roots[0].y == ctx.logs[0].y {
        out.extend_from_slice(&ctx.logs);
        out.extend_from_slice(&ctx.roots);
    } else {
        out.extend_from_slice(&ctx.roots);
    }
    out
}

/// `AlterGroundDecorator` (podzol / mud / rooted dirt circles).
fn alter_ground<W: FeatureWorld + ?Sized>(
    ctx: &mut Ctx<'_, W>,
    provider: &crate::features::blocks::StateProvider,
) {
    let positions = lowest_trunk_or_root(ctx);
    let Some(first) = positions.first() else {
        return;
    };
    let min_y = first.y;
    for &p in positions.iter().filter(|p| p.y == min_y) {
        alter_circle(ctx, provider, p.offset(-1, 0, -1));
        alter_circle(ctx, provider, p.offset(2, 0, -1));
        alter_circle(ctx, provider, p.offset(-1, 0, 2));
        alter_circle(ctx, provider, p.offset(2, 0, 2));
        for _ in 0..5 {
            let placement = ctx.rng.next_int_bounded(64);
            let xx = placement % 8;
            let zz = placement / 8;
            if xx == 0 || xx == 7 || zz == 0 || zz == 7 {
                alter_circle(ctx, provider, p.offset(-3 + xx, 0, -3 + zz));
            }
        }
    }
}

fn alter_circle<W: FeatureWorld + ?Sized>(
    ctx: &mut Ctx<'_, W>,
    provider: &crate::features::blocks::StateProvider,
    pos: BlockPos,
) {
    for xx in -2i32..=2 {
        for zz in -2i32..=2 {
            if xx.abs() != 2 || zz.abs() != 2 {
                alter_block_at(ctx, provider, pos.offset(xx, 0, zz));
            }
        }
    }
}

/// `AlterGroundDecorator.placeBlockAt`: first dirt-tagged block from 2 above
/// down to 3 below; the provider is sampled at `pos`, not at the hit.
fn alter_block_at<W: FeatureWorld + ?Sized>(
    ctx: &mut Ctx<'_, W>,
    provider: &crate::features::blocks::StateProvider,
    pos: BlockPos,
) {
    let mut dy = 2;
    while dy >= -3 {
        let cursor = pos.offset(0, dy, 0);
        if ctx.world.get_state(cursor).is_in(tags::DIRT) {
            let state = provide_state(provider, ctx.rng, pos);
            ctx.set_block(cursor, state);
            break;
        }
        if !ctx.is_air(cursor) && dy < 0 {
            break;
        }
        dy -= 1;
    }
}

/// `AttachedToLeavesDecorator` (mangrove propagules, pale oak hanging moss
/// variants): shuffled leaves, exclusion boxes around each placement.
#[allow(clippy::too_many_arguments)]
fn attached_to_leaves<W: FeatureWorld + ?Sized>(
    ctx: &mut Ctx<'_, W>,
    probability: f32,
    exclusion_xz: i32,
    exclusion_y: i32,
    provider: &crate::features::blocks::StateProvider,
    required_empty: i32,
    directions: &[Direction],
) {
    // Blocked positions are the union of boxes around earlier placements.
    let mut placed_at: Vec<BlockPos> = Vec::new();
    let mut leaves = ctx.leaves.clone();
    shuffle(&mut leaves, ctx.rng);
    for leaf in leaves {
        let dir = directions[ctx.rng.next_int_bounded(directions.len() as i32) as usize];
        let place = leaf.rel(dir);
        let blocked = placed_at.iter().any(|q| {
            (place.x - q.x).abs() <= exclusion_xz
                && (place.y - q.y).abs() <= exclusion_y
                && (place.z - q.z).abs() <= exclusion_xz
        });
        if blocked || ctx.rng.next_float() >= probability {
            continue;
        }
        let mut empty = true;
        for i in 1..=required_empty {
            if !ctx.is_air(leaf.rel_n(dir, i)) {
                empty = false;
                break;
            }
        }
        if empty {
            placed_at.push(place);
            let state = provide_state(provider, ctx.rng, place);
            ctx.set_block(place, state);
        }
    }
}

/// `AttachedToLogsDecorator` (mushrooms on fallen logs).
fn attached_to_logs<W: FeatureWorld + ?Sized>(
    ctx: &mut Ctx<'_, W>,
    probability: f32,
    provider: &crate::features::blocks::StateProvider,
    directions: &[Direction],
) {
    let mut logs = ctx.logs.clone();
    shuffle(&mut logs, ctx.rng);
    for log in logs {
        let dir = directions[ctx.rng.next_int_bounded(directions.len() as i32) as usize];
        let place = log.rel(dir);
        if ctx.rng.next_float() <= probability && ctx.is_air(place) {
            let state = provide_state(provider, ctx.rng, place);
            ctx.set_block(place, state);
        }
    }
}

/// `BlockState.isSolidRender()` approximation: the Java block data here has
/// collision but no occlusion shapes, so a state counts as solid-render when
/// its collision shape is a full cube and it is not one of the full-cube,
/// non-occluding families (leaves, glass/`impermeable`, ice, frosted ice).
fn is_solid_render(state: JavaState) -> bool {
    state.is_collision_full_cube()
        && !state.is_in(tags::LEAVES)
        && !state.is_in(tags::IMPERMEABLE)
        && !state.is(b::ICE)
        && !state.is(b::FROSTED_ICE)
}

/// `PlaceOnGroundDecorator` (leaf litter): random positions in the box
/// around the lowest trunk/root layer, inflated by `radius`/`height`.
fn place_on_ground<W: FeatureWorld + ?Sized>(
    ctx: &mut Ctx<'_, W>,
    tries: i32,
    radius: i32,
    height: i32,
    provider: &crate::features::blocks::StateProvider,
) {
    let positions = lowest_trunk_or_root(ctx);
    let Some(&first) = positions.first() else {
        return;
    };
    let min_y = first.y;
    let (mut min_x, mut max_x, mut min_z, mut max_z) = (first.x, first.x, first.z, first.z);
    for p in positions.iter().filter(|p| p.y == min_y) {
        min_x = min_x.min(p.x);
        max_x = max_x.max(p.x);
        min_z = min_z.min(p.z);
        max_z = max_z.max(p.z);
    }
    let (x0, x1) = (min_x - radius, max_x + radius);
    let (y0, y1) = (min_y - height, min_y + height);
    let (z0, z1) = (min_z - radius, max_z + radius);
    for _ in 0..tries {
        let x = ctx.rng.next_int_between_inclusive(x0, x1);
        let y = ctx.rng.next_int_between_inclusive(y0, y1);
        let z = ctx.rng.next_int_between_inclusive(z0, z1);
        let pos = BlockPos::new(x, y, z);
        let above = pos.above();
        let above_state = ctx.world.get_state(above);
        if (above_state.is_air() || above_state.is(b::VINE))
            && is_solid_render(ctx.world.get_state(pos))
            && ctx.world.get_height(
                crate::features::blocks::Heightmap::MotionBlockingNoLeaves,
                x,
                z,
            ) <= above.y
        {
            let state = provide_state(provider, ctx.rng, above);
            ctx.set_block(above, state);
        }
    }
}

/// `CreakingHeartDecorator`: a dormant natural heart replacing a log whose
/// six neighbours are all logs.
fn creaking_heart<W: FeatureWorld + ?Sized>(ctx: &mut Ctx<'_, W>, probability: f32) {
    if ctx.logs.is_empty() || ctx.rng.next_float() >= probability {
        return;
    }
    let mut candidates = ctx.logs.clone();
    shuffle(&mut candidates, ctx.rng);
    for p in candidates {
        let enclosed = Direction::ALL
            .iter()
            .all(|&d| ctx.world.get_state(p.rel(d)).is_in(tags::LOGS));
        if enclosed {
            let state = b::CREAKING_HEART
                .default_state()
                .with("creaking_heart_state", "dormant")
                .with_bool("natural", true);
            ctx.set_block(p, state);
            return;
        }
    }
}

/// `PaleMossDecorator`: optional `pale_moss_patch` at the lowest log, then
/// hanging moss under logs and leaves.
fn pale_moss<W, F>(
    ctx: &mut Ctx<'_, W>,
    leaves_probability: f32,
    trunk_probability: f32,
    ground_probability: f32,
    pale_moss_patch: &mut F,
) where
    W: FeatureWorld + ?Sized,
    F: FnMut(&mut W, &mut FeatureRandom, BlockPos) -> bool,
{
    let mut logs = ctx.logs.clone();
    shuffle(&mut logs, ctx.rng);
    let Some(&first) = logs.first() else {
        return;
    };
    let mut root = first;
    for &p in &logs {
        if p.y < root.y {
            root = p;
        }
    }
    if ctx.rng.next_float() < ground_probability {
        pale_moss_patch(ctx.world, ctx.rng, root.above());
    }
    for i in 0..ctx.logs.len() {
        let pos = ctx.logs[i];
        if ctx.rng.next_float() < trunk_probability {
            let below = pos.below();
            if ctx.is_air(below) {
                moss_hanger(ctx, below);
            }
        }
    }
    for i in 0..ctx.leaves.len() {
        let pos = ctx.leaves[i];
        if ctx.rng.next_float() < leaves_probability {
            let below = pos.below();
            if ctx.is_air(below) {
                moss_hanger(ctx, below);
            }
        }
    }
}

fn moss_hanger<W: FeatureWorld + ?Sized>(ctx: &mut Ctx<'_, W>, mut pos: BlockPos) {
    while ctx.is_air(pos.below()) && (ctx.rng.next_float() as f64) >= 0.5 {
        let s = b::PALE_HANGING_MOSS.default_state().with_bool("tip", false);
        ctx.set_block(pos, s);
        pos = pos.below();
    }
    let s = b::PALE_HANGING_MOSS.default_state().with_bool("tip", true);
    ctx.set_block(pos, s);
}
