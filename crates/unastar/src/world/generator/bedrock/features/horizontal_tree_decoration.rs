//! `minecraft:horizontal_tree_decoration_feature` (shelf mushroom): attaches
//! `places_block` to one side of a tree log.
//!
//! The input is the log itself (trees call their `log_decoration_feature` on
//! every trunk log, fallen trees on every log of the fallen trunk). One of the
//! allowed sides is drawn uniformly; the decoration is placed in the air cell
//! on that side, rotated to face away from the log (its
//! `minecraft:cardinal_direction` is the side it sits on). With
//! `bark_side_only` the allowed sides are the log's bark sides (the four sides
//! of an upright log, the two sides along a horizontal log, every side of a
//! wood block); otherwise all four. Without `allow_adjacent`, nothing is
//! placed when another `places_block` already touches the target cell.
//!
//! BDS's draw order is unknown (statistical parity): one side is drawn and
//! the feature fails when that side is occupied, so partly covered trunks get
//! fewer decorations, as with any single-try decoration.

use super::super::generated::bb;
use super::super::world::{BlockPos, BlockWorld, Ctx, Direction};
use super::multiface::FACES;

/// Parameters of one `minecraft:horizontal_tree_decoration_feature`.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// `places_block` facing each [`Direction`] (indexed by `Direction as usize`).
    pub rotations: [u32; 4],
    /// Any state of `places_block` (the `allow_adjacent` test).
    pub is_decoration: fn(u32) -> bool,
    /// Sides a decoration may attach to, as a mask over `Direction as usize`
    /// (`bark_side_only` resolved by the codegen).
    pub sides: fn(u32) -> u8,
    /// `allow_adjacent`.
    pub allow_adjacent: bool,
}

/// Index of the `n`-th (0-based) set bit of `mask`, which has more than `n`.
pub fn nth_set_bit(mask: u8, n: u32) -> usize {
    let mut rest = mask;
    for _ in 0..n {
        rest &= rest - 1;
    }
    rest.trailing_zeros() as usize
}

/// Decorate the log at `pos`; `Some(spot)` of the placed decoration.
pub fn place<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, p: &Params) -> Option<BlockPos> {
    let sides = (p.sides)(ctx.get(pos)) & 0b1111;
    if sides == 0 {
        return None;
    }
    let side = nth_set_bit(sides, ctx.rng.next_int(sides.count_ones() as i32) as u32);
    let target = pos.side(Direction::ALL[side]);
    if !bb::air(ctx.get(target)) {
        return None;
    }
    if !p.allow_adjacent
        && FACES.iter().any(|&(dx, dy, dz)| {
            let n = target.offset(dx, dy, dz);
            n != pos && (p.is_decoration)(ctx.get(n))
        })
    {
        return None;
    }
    ctx.set(target, p.rotations[side]).then_some(target)
}

#[cfg(test)]
mod tests {
    use super::super::super::rng::BedrockRandom;
    use super::super::multiface::test_world::{Cube, with_ctx};
    use super::*;

    #[test]
    fn nth_set_bit_walks_the_mask() {
        assert_eq!(nth_set_bit(0b1010, 0), 1);
        assert_eq!(nth_set_bit(0b1010, 1), 3);
        assert_eq!(nth_set_bit(0b1111, 2), 2);
        assert_eq!(nth_set_bit(0b0100, 0), 2);
    }

    fn is_mark(id: u32) -> bool {
        matches!(id, bb::SAND | bb::RED_SAND | bb::GRAVEL | bb::CLAY)
    }
    /// A log with bark on its south and north sides only (axis x).
    fn sides(id: u32) -> u8 {
        if id == bb::OAK_LOG { 0b0101 } else { 0 }
    }
    const PARAMS: Params = Params {
        rotations: [bb::SAND, bb::RED_SAND, bb::GRAVEL, bb::CLAY],
        is_decoration: is_mark,
        sides,
        allow_adjacent: false,
    };

    #[test]
    fn decoration_sits_on_a_bark_side_facing_out() {
        let mut hits = [0u32; 4];
        for seed in 0..64 {
            let mut world = Cube::filled(8, bb::AIR);
            world.set(BlockPos::new(0, 0, 0), bb::OAK_LOG);
            let mut rng = BedrockRandom::new(seed);
            let at = with_ctx(&mut world, &mut rng, |ctx| place(ctx, BlockPos::new(0, 0, 0), &PARAMS)).expect("placed");
            let side = Direction::ALL.iter().position(|&d| BlockPos::new(0, 0, 0).side(d) == at).expect("beside the log");
            assert_eq!(world.get(at), PARAMS.rotations[side], "rotated towards its side");
            hits[side] += 1;
        }
        assert_eq!(hits[1] + hits[3], 0, "west/east are cut ends");
        assert!(hits[0] > 0 && hits[2] > 0, "both bark sides are used: {hits:?}");
    }

    #[test]
    fn adjacent_decoration_blocks_placement() {
        for seed in 0..16 {
            let mut world = Cube::filled(8, bb::AIR);
            world.set(BlockPos::new(0, 0, 0), bb::OAK_LOG);
            // Existing decorations above both bark-side targets.
            world.set(BlockPos::new(0, 1, 1), bb::SAND);
            world.set(BlockPos::new(0, 1, -1), bb::GRAVEL);
            let mut rng = BedrockRandom::new(seed);
            assert_eq!(with_ctx(&mut world, &mut rng, |ctx| place(ctx, BlockPos::new(0, 0, 0), &PARAMS)), None);
        }
    }
}
