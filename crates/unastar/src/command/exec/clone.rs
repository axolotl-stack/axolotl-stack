//! `/clone <begin> <end> <destination> [replace|masked] [normal|force|move]`
//! and `/clone … filtered <normal|force|move> <tileName> [blockStates]`.
//!
//! Measured (BDS 1.26.51): cloning a 2×1×2 floor answers
//! `commands.clone.success [4]` (success count 1).
//!
//! [INFERENCE] As the lang keys and the Java rules describe: the count is
//! the number of blocks cloned (every block with `replace`, non-air with
//! `masked`, filter matches with `filtered`) and 0 answers
//! `commands.clone.failed`; overlapping regions need `force`
//! (`commands.clone.noOverlap`); `move` clears the cloned source blocks
//! before writing the destination; regions outside the world or in unloaded
//! chunks answer `commands.generic.outOfWorld`; the limit is 524288 blocks
//! (`commands.clone.tooManyBlocks`).
//!
//! Block entity data (container contents, sign text) is not cloned: only
//! block states are.

use super::setblock::{BlockSpec, box_loaded, volume};
use crate::command::prelude::*;
use crate::server::game::blocks::{block_at, set_block};
use crate::world::chunk::blocks::AIR;

/// The most blocks one `/clone` may cover.
const MAX_BLOCKS: i64 = 524_288;

arg_enum! {
    /// Captured `MaskMode`.
    pub enum MaskMode in "MaskMode" {
        Replace = "replace",
        Masked = "masked",
    }
}

arg_enum! {
    /// Captured `CloneMode`.
    pub enum CloneMode in "CloneMode" {
        Normal = "normal",
        Force = "force",
        Move = "move",
    }
}

overloads! {
    pub enum CloneArgs for "clone" {
        0 => Masked {
            begin: BlockPosition,
            end: BlockPosition,
            destination: BlockPosition,
            mask_mode: Option<MaskMode> = "maskMode",
            clone_mode: Option<CloneMode> = "cloneMode",
        },
        1 => Filtered {
            begin: BlockPosition,
            end: BlockPosition,
            destination: BlockPosition,
            mask_mode: EnumValue = "maskMode",
            clone_mode: CloneMode = "cloneMode",
            tile_name: EnumValue = "tileName",
            block_states: Option<BlockStates> = "blockStates",
        },
    }
}

/// Which source blocks are cloned.
enum Mask {
    All,
    NonAir,
    Matching(BlockSpec),
}

pub fn run(ctx: &mut CommandContext<'_>, args: CloneArgs) -> CommandResult {
    let (begin, end, destination, mask, clone_mode) = match args {
        CloneArgs::Masked {
            begin,
            end,
            destination,
            mask_mode,
            clone_mode,
        } => (
            begin,
            end,
            destination,
            match mask_mode {
                Some(MaskMode::Masked) => Mask::NonAir,
                Some(MaskMode::Replace) | None => Mask::All,
            },
            clone_mode.unwrap_or(CloneMode::Normal),
        ),
        CloneArgs::Filtered {
            begin,
            end,
            destination,
            clone_mode,
            tile_name,
            block_states,
            ..
        } => (
            begin,
            end,
            destination,
            Mask::Matching(BlockSpec::resolve(&tile_name, block_states.as_ref())?),
            clone_mode,
        ),
    };
    let (a, b) = (
        ctx.block_position(Some(begin)),
        ctx.block_position(Some(end)),
    );
    let (min, max) = (a.min(b), a.max(b));
    let dest_min = ctx.block_position(Some(destination));
    let dest_max = dest_min + (max - min);

    let count = volume(min, max);
    if count > MAX_BLOCKS {
        return failure!("commands.clone.tooManyBlocks", count, MAX_BLOCKS);
    }
    if !box_loaded(ctx.world, min, max) || !box_loaded(ctx.world, dest_min, dest_max) {
        return failure!("commands.generic.outOfWorld");
    }
    if crate::server::game::container_area_locked(ctx.world, dest_min, dest_max)
        || (clone_mode == CloneMode::Move
            && crate::server::game::container_area_locked(ctx.world, min, max))
    {
        return failure!("commands.clone.failed");
    }
    let overlap = min.cmple(dest_max).all() && dest_min.cmple(max).all();
    if overlap && clone_mode != CloneMode::Force {
        return failure!("commands.clone.noOverlap");
    }

    // Read the whole source first: with `force` the destination may overlap it.
    let mut cloned = Vec::new();
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                let state = block_at(ctx.world, (x, y, z)).expect("the box is loaded");
                let keep = match &mask {
                    Mask::All => true,
                    Mask::NonAir => state != *AIR,
                    Mask::Matching(filter) => filter.matches(state),
                };
                if keep {
                    cloned.push((IVec3::new(x, y, z) - min, state));
                }
            }
        }
    }
    if cloned.is_empty() {
        return failure!("commands.clone.failed");
    }
    if clone_mode == CloneMode::Move {
        for &(offset, _) in &cloned {
            write(ctx.world, min + offset, *AIR);
        }
    }
    for &(offset, state) in &cloned {
        write(ctx.world, dest_min + offset, state);
    }
    success!("commands.clone.success", cloned.len())
}

fn write(world: &mut World, pos: IVec3, state: u32) {
    if block_at(world, (pos.x, pos.y, pos.z)) != Some(state) {
        set_block(world, pos, state, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::args::assert_covers;
    use crate::command::exec::setblock::tests::{block, place, state};
    use crate::command::exec::test_support::Harness;

    #[test]
    fn enums_cover_the_captured_values() {
        assert_covers::<MaskMode>();
        assert_covers::<CloneMode>();
    }

    /// A 2×1×2 floor under the player at (3, 63, 3): stone, dirt / glass, air.
    fn floor(h: &mut Harness) {
        place(h, 3, 63, 3, "minecraft:stone");
        place(h, 3, 63, 4, "minecraft:dirt");
        place(h, 4, 63, 3, "minecraft:glass");
    }

    #[test]
    fn clones_a_floor_like_bds() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        floor(&mut h);
        place(&mut h, 9, 63, 9, "minecraft:oak_planks");
        assert_eq!(
            h.run(op, "/clone ~ ~-1 ~ ~1 ~-1 ~1 ~5 ~-1 ~5"),
            CommandOutput::success("commands.clone.success", vec!["4".into()])
        );
        assert_eq!(block(&mut h, 8, 63, 8), state("minecraft:stone"));
        assert_eq!(block(&mut h, 8, 63, 9), state("minecraft:dirt"));
        assert_eq!(block(&mut h, 9, 63, 8), state("minecraft:glass"));
        // `replace` copies the air too.
        assert_eq!(block(&mut h, 9, 63, 9), *AIR);
        // The source is untouched.
        assert_eq!(block(&mut h, 3, 63, 3), state("minecraft:stone"));
    }

    #[test]
    fn masked_filtered_and_move() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        floor(&mut h);
        place(&mut h, 9, 63, 9, "minecraft:oak_planks");
        assert_eq!(
            h.run(op, "/clone 3 63 3 4 63 4 8 63 8 masked"),
            CommandOutput::success("commands.clone.success", vec!["3".into()])
        );
        assert_eq!(block(&mut h, 9, 63, 9), state("minecraft:oak_planks"));

        assert_eq!(
            h.run(op, "/clone 3 63 3 4 63 4 3 70 3 filtered normal dirt"),
            CommandOutput::success("commands.clone.success", vec!["1".into()])
        );
        assert_eq!(block(&mut h, 3, 70, 4), state("minecraft:dirt"));
        assert_eq!(block(&mut h, 3, 70, 3), *AIR);
        assert_eq!(
            h.run(op, "/clone 3 63 3 4 63 4 3 71 3 filtered normal oak_planks"),
            CommandOutput::failure("commands.clone.failed", vec![])
        );

        assert_eq!(
            h.run(op, "/clone 3 63 3 4 63 4 3 60 3 replace move"),
            CommandOutput::success("commands.clone.success", vec!["4".into()])
        );
        assert_eq!(block(&mut h, 3, 60, 3), state("minecraft:stone"));
        assert_eq!(block(&mut h, 3, 63, 3), *AIR);
        assert_eq!(block(&mut h, 3, 63, 4), *AIR);
    }

    #[test]
    fn overlap_limits_and_the_world_edge() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        floor(&mut h);
        assert_eq!(
            h.run(op, "/clone ~ ~ ~ ~1 ~1 ~1 ~1 ~ ~"),
            CommandOutput::failure("commands.clone.noOverlap", vec![])
        );
        assert_eq!(
            h.run(op, "/clone 3 63 3 4 63 4 4 63 4"),
            CommandOutput::failure("commands.clone.noOverlap", vec![])
        );
        assert_eq!(
            h.run(op, "/clone 3 63 3 4 63 4 4 63 4 replace force"),
            CommandOutput::success("commands.clone.success", vec!["4".into()])
        );
        assert_eq!(block(&mut h, 4, 63, 4), state("minecraft:stone"));
        assert_eq!(block(&mut h, 4, 63, 5), state("minecraft:dirt"));
        assert_eq!(
            h.run(op, "/clone 0 0 0 100 100 100 0 0 0"),
            CommandOutput::failure(
                "commands.clone.tooManyBlocks",
                vec!["1030301".into(), "524288".into()]
            )
        );
        assert_eq!(
            h.run(op, "/clone 3 63 3 4 63 4 15 63 15"),
            CommandOutput::failure("commands.generic.outOfWorld", vec![])
        );
    }
}
