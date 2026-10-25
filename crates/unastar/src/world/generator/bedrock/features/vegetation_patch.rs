//! `minecraft:vegetation_patch_feature` (moss patches, moss ceilings, clay
//! pools with dripleaves): replaces the ground of a patch with `ground_block`
//! and decorates it with `vegetation_feature` (vanilla vegetation patch,
//! shared by both editions).
//!
//! Shape: a rectangle of half-extents `horizontal_radius + 1` (drawn
//! separately for x and z) without its corners; each edge column is kept
//! only with `extra_edge_column_chance`. For each column, the surface is
//! searched from the input row: up to `vertical_range` steps into the ground
//! while in air, then up to `vertical_range` steps back out while in a block.
//! The column counts when it ends on an air cell in front of a sturdy ground
//! block (below for `floor`, above for `ceiling`). The ground is then replaced
//! `depth` blocks deep (+1 with `extra_deep_block_chance`) while the blocks
//! are `replaceable_blocks`; a column whose first block is not replaceable is
//! dropped. Like vanilla, meeting a block that already is the ground block
//! ends the column successfully.
//!
//! `waterlogged` (floor only): surface blocks whose four sides and bottom are
//! all sturdy become water, the others are dropped; vegetation then grows
//! from the water cell itself. Otherwise vegetation grows from the open cell
//! in front of the ground. Each kept cell gets the vegetation with
//! `vegetation_chance`, after the whole ground is placed. Like vanilla, the
//! feature reports success when it kept at least one cell.

use super::super::generated::bb;
use super::super::world::{BlockPos, BlockWorld, Ctx};
use super::IntRange;
use super::multiface::is_sturdy;

/// Largest supported `horizontal_radius` (checked by the codegen); bounds the
/// on-stack cell buffer.
pub const MAX_RADIUS: i32 = 11;
const MAX_EXTENT: usize = 2 * (MAX_RADIUS as usize + 1) + 1;
const MAX_CELLS: usize = MAX_EXTENT * MAX_EXTENT;

/// Parameters of one `minecraft:vegetation_patch_feature`.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// `replaceable_blocks`.
    pub replaceable: fn(u32) -> bool,
    /// `ground_block`.
    pub ground: u32,
    /// `surface`: `false` floor, `true` ceiling.
    pub ceiling: bool,
    /// `depth`.
    pub depth: IntRange,
    /// `vertical_range`.
    pub vertical_range: i32,
    /// `vegetation_chance`.
    pub vegetation_chance: f32,
    /// `horizontal_radius` (at most [`MAX_RADIUS`]).
    pub horizontal_radius: IntRange,
    /// `extra_deep_block_chance`.
    pub extra_deep_block_chance: f32,
    /// `extra_edge_column_chance`.
    pub extra_edge_column_chance: f32,
    /// `waterlogged`.
    pub waterlogged: bool,
}

/// Place one patch around `pos`, calling `vegetation` (the compiled
/// `vegetation_feature`) on the decorated cells; `Some(pos)` when any cell
/// was kept.
pub fn place<W, F>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
    p: &Params,
    mut vegetation: F,
) -> Option<BlockPos>
where
    W: BlockWorld,
    F: FnMut(&mut Ctx<'_, W>, BlockPos) -> Option<BlockPos>,
{
    let rx = p.horizontal_radius.sample(ctx.rng) + 1;
    let rz = p.horizontal_radius.sample(ctx.rng) + 1;
    debug_assert!(
        rx <= MAX_RADIUS + 1 && rz <= MAX_RADIUS + 1,
        "horizontal_radius above MAX_RADIUS"
    );
    let mut cells = [BlockPos::new(0, 0, 0); MAX_CELLS];
    let mut n = ground_patch(ctx, pos, p, rx, rz, &mut cells);
    // Direction from the ground towards the open side.
    let out = if p.ceiling { -1 } else { 1 };
    if p.waterlogged {
        // Decide every cell before writing any water, so the checks only see
        // the ground blocks.
        let mut kept = 0;
        for k in 0..n {
            let cell = cells[k];
            if !is_exposed(ctx, cell) {
                cells[kept] = cell;
                kept += 1;
            }
        }
        n = kept;
        for &cell in &cells[..n] {
            ctx.set(cell, bb::WATER);
        }
    }
    for &cell in &cells[..n] {
        if p.vegetation_chance > 0.0 && ctx.rng.next_float() < p.vegetation_chance {
            let at = if p.waterlogged {
                cell
            } else {
                cell.offset(0, out, 0)
            };
            vegetation(&mut *ctx, at);
        }
    }
    (n > 0).then_some(pos)
}

/// Replace the ground of every column; returns how many surface blocks
/// (the first ground block of each kept column) were written to `cells`.
fn ground_patch<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
    p: &Params,
    rx: i32,
    rz: i32,
    cells: &mut [BlockPos; MAX_CELLS],
) -> usize {
    // Direction from the open side into the ground.
    let inward = if p.ceiling { 1 } else { -1 };
    let mut n = 0;
    for x in -rx..=rx {
        for z in -rz..=rz {
            match column(x, z, rx, rz) {
                Column::Corner => continue,
                Column::Edge => {
                    if !(p.extra_edge_column_chance != 0.0
                        && ctx.rng.next_float() <= p.extra_edge_column_chance)
                    {
                        continue;
                    }
                }
                Column::Inner => {}
            }
            let mut at = pos.offset(x, 0, z);
            let mut i = 0;
            while i < p.vertical_range && bb::air(ctx.get(at)) {
                at.y += inward;
                i += 1;
            }
            let mut i = 0;
            while i < p.vertical_range && !bb::air(ctx.get(at)) {
                at.y -= inward;
                i += 1;
            }
            let ground = at.offset(0, inward, 0);
            if !bb::air(ctx.get(at)) || !is_sturdy(ctx.get(ground)) {
                continue;
            }
            let extra =
                p.extra_deep_block_chance > 0.0 && ctx.rng.next_float() < p.extra_deep_block_chance;
            let depth = p.depth.sample(ctx.rng) + i32::from(extra);
            if place_ground(ctx, ground, inward, depth, p) {
                cells[n] = ground;
                n += 1;
            }
        }
    }
    n
}

/// Where a column lies in the patch rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Column {
    Corner,
    Edge,
    Inner,
}

fn column(x: i32, z: i32, rx: i32, rz: i32) -> Column {
    let x_edge = x == -rx || x == rx;
    let z_edge = z == -rz || z == rz;
    match (x_edge, z_edge) {
        (true, true) => Column::Corner,
        (false, false) => Column::Inner,
        _ => Column::Edge,
    }
}

/// Replace up to `depth` blocks from `ground` inwards; `false` when the very
/// first block could not be replaced.
fn place_ground<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    ground: BlockPos,
    inward: i32,
    depth: i32,
    p: &Params,
) -> bool {
    let mut at = ground;
    for i in 0..depth {
        let id = ctx.get(at);
        if id == p.ground {
            return true;
        }
        if !(p.replaceable)(id) || !ctx.set(at, p.ground) {
            return i != 0;
        }
        at.y += inward;
    }
    true
}

/// A pool cell leaks when a side or its bottom is not sturdy.
fn is_exposed<W: BlockWorld>(ctx: &Ctx<'_, W>, cell: BlockPos) -> bool {
    [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, -1, 0)]
        .iter()
        .any(|&(dx, dy, dz)| !is_sturdy(ctx.get(cell.offset(dx, dy, dz))))
}

#[cfg(test)]
mod tests {
    use super::super::super::rng::BedrockRandom;
    use super::super::multiface::test_world::{Cube, with_ctx};
    use super::*;

    fn dirt(id: u32) -> bool {
        id == bb::DIRT
    }

    const PATCH: Params = Params {
        replaceable: dirt,
        ground: bb::CLAY,
        ceiling: false,
        depth: IntRange { min: 2, max: 2 },
        vertical_range: 5,
        vegetation_chance: 1.0,
        horizontal_radius: IntRange { min: 2, max: 2 },
        extra_deep_block_chance: 0.0,
        extra_edge_column_chance: 0.0,
        waterlogged: false,
    };

    /// Dirt below y = 0, air above; run `p` from 2 blocks above the floor.
    fn run(p: &Params) -> (Cube, Vec<BlockPos>, Option<BlockPos>) {
        let mut world = Cube::filled(8, bb::AIR);
        world.fill((-8, -8, -8), (7, -1, 7), bb::DIRT);
        let mut rng = BedrockRandom::new(11);
        let mut calls = Vec::new();
        let placed = with_ctx(&mut world, &mut rng, |ctx| {
            place(ctx, BlockPos::new(0, 2, 0), p, |_, at| {
                calls.push(at);
                None
            })
        });
        (world, calls, placed)
    }

    #[test]
    fn floor_patch_replaces_ground_and_decorates_above_it() {
        let (world, calls, placed) = run(&PATCH);
        assert_eq!(placed, Some(BlockPos::new(0, 2, 0)));
        // Radius 2 -> half-extent 3; no edge columns: the inner 5x5, 2 deep.
        let clay = world.positions(|id| id == bb::CLAY);
        assert_eq!(clay.len(), 50);
        assert!(
            clay.iter()
                .all(|at| at.x.abs() <= 2 && at.z.abs() <= 2 && (-2..=-1).contains(&at.y))
        );
        assert_eq!(calls.len(), 25);
        assert!(
            calls.iter().all(|at| at.y == 0),
            "vegetation grows on the open cell above the ground"
        );
    }

    #[test]
    fn waterlogged_patch_turns_enclosed_surface_into_water() {
        let (world, calls, placed) = run(&Params {
            waterlogged: true,
            ..PATCH
        });
        assert!(placed.is_some());
        let water = world.positions(|id| id == bb::WATER);
        assert_eq!(water.len(), 25);
        assert!(water.iter().all(|at| at.y == -1));
        assert_eq!(world.count(bb::CLAY), 25, "one clay layer under the water");
        let sorted = |v: &[BlockPos]| {
            let mut keys: Vec<(i32, i32, i32)> = v.iter().map(|p| (p.x, p.y, p.z)).collect();
            keys.sort_unstable();
            keys
        };
        assert_eq!(
            sorted(&calls),
            sorted(&water),
            "vegetation grows from the water cells"
        );
    }

    #[test]
    fn patch_is_a_rectangle_without_corners() {
        for (rx, rz) in [(1, 1), (2, 5), (MAX_RADIUS + 1, MAX_RADIUS + 1)] {
            let (mut corner, mut edge, mut inner) = (0, 0, 0);
            for x in -rx..=rx {
                for z in -rz..=rz {
                    match column(x, z, rx, rz) {
                        Column::Corner => corner += 1,
                        Column::Edge => edge += 1,
                        Column::Inner => inner += 1,
                    }
                }
            }
            assert_eq!(corner, 4);
            assert_eq!(edge, 2 * (2 * rx - 1) + 2 * (2 * rz - 1));
            assert_eq!(inner, (2 * rx - 1) * (2 * rz - 1));
            assert!(((2 * rx + 1) * (2 * rz + 1)) as usize <= MAX_CELLS);
        }
        assert_eq!(column(0, 0, 3, 3), Column::Inner);
        assert_eq!(column(3, 0, 3, 3), Column::Edge);
        assert_eq!(column(-3, 3, 3, 3), Column::Corner);
    }
}
