//! `/fill <from> <to> <tileName> [blockStates] [outline|hollow|destroy|keep]`
//! and `/fill … replace [replaceTileName] [replaceBlockStates]`.
//!
//! Measured (BDS 1.26.51): filling a 3×3×3 box answers
//! `commands.fill.success [27]` (success count 1), both with glass over air
//! and with air over that glass; a box over 32768 blocks answers
//! `commands.fill.tooManyBlocks [<volume>, 32768]`. BDS sends the changes as
//! `UpdateSubChunkBlocks`; unastar's block path sends one `UpdateBlock` per
//! block.
//!
//! [INFERENCE] The count is the number of blocks that changed, and a fill
//! that changes nothing answers `commands.fill.failed`. A replace filter
//! without states matches every state of its block.

use super::setblock::{BlockSpec, box_loaded, volume};
use crate::command::prelude::*;
use crate::server::game::blocks::{block_at, destroy_block, set_block};
use crate::world::chunk::blocks::AIR;

/// The most blocks one `/fill` may cover.
const MAX_BLOCKS: i64 = 32_768;

arg_enum! {
    /// Captured `FillMode`.
    pub enum FillMode in "FillMode" {
        Outline = "outline",
        Hollow = "hollow",
        Destroy = "destroy",
        Keep = "keep",
    }
}

overloads! {
    pub enum FillArgs for "fill" {
        0 => WithStates {
            from: BlockPosition,
            to: BlockPosition,
            tile_name: EnumValue = "tileName",
            block_states: BlockStates = "blockStates",
            old_block_handling: Option<FillMode> = "oldBlockHandling",
        },
        1 => Plain {
            from: BlockPosition,
            to: BlockPosition,
            tile_name: EnumValue = "tileName",
            old_block_handling: Option<FillMode> = "oldBlockHandling",
        },
        2 => ReplaceWithStates {
            from: BlockPosition,
            to: BlockPosition,
            tile_name: EnumValue = "tileName",
            block_states: BlockStates = "blockStates",
            old_block_handling: EnumValue = "oldBlockHandling",
            replace_tile_name: Option<EnumValue> = "replaceTileName",
            replace_block_states: Option<BlockStates> = "replaceBlockStates",
        },
        3 => Replace {
            from: BlockPosition,
            to: BlockPosition,
            tile_name: EnumValue = "tileName",
            old_block_handling: EnumValue = "oldBlockHandling",
            replace_tile_name: Option<EnumValue> = "replaceTileName",
            replace_block_states: Option<BlockStates> = "replaceBlockStates",
        },
    }
}

/// What a fill does with the blocks already in the box.
enum Mode {
    /// Every block, or only those matching the filter.
    Replace(Option<BlockSpec>),
    Outline,
    Hollow,
    Destroy,
    Keep,
}

pub fn run(ctx: &mut CommandContext<'_>, args: FillArgs) -> CommandResult {
    let (from, to, tile_name, states, mode, filter) = match args {
        FillArgs::WithStates {
            from,
            to,
            tile_name,
            block_states,
            old_block_handling,
        } => (
            from,
            to,
            tile_name,
            Some(block_states),
            old_block_handling,
            None,
        ),
        FillArgs::Plain {
            from,
            to,
            tile_name,
            old_block_handling,
        } => (from, to, tile_name, None, old_block_handling, None),
        FillArgs::ReplaceWithStates {
            from,
            to,
            tile_name,
            block_states,
            replace_tile_name,
            replace_block_states,
            ..
        } => (
            from,
            to,
            tile_name,
            Some(block_states),
            None,
            Some((replace_tile_name, replace_block_states)),
        ),
        FillArgs::Replace {
            from,
            to,
            tile_name,
            replace_tile_name,
            replace_block_states,
            ..
        } => (
            from,
            to,
            tile_name,
            None,
            None,
            Some((replace_tile_name, replace_block_states)),
        ),
    };
    let block = BlockSpec::resolve(&tile_name, states.as_ref())?.state();
    let mode = match (mode, filter) {
        (_, Some((Some(name), filter_states))) => {
            Mode::Replace(Some(BlockSpec::resolve(&name, filter_states.as_ref())?))
        }
        (_, Some((None, _))) | (None, None) => Mode::Replace(None),
        (Some(FillMode::Outline), None) => Mode::Outline,
        (Some(FillMode::Hollow), None) => Mode::Hollow,
        (Some(FillMode::Destroy), None) => Mode::Destroy,
        (Some(FillMode::Keep), None) => Mode::Keep,
    };

    let (a, b) = (ctx.block_position(Some(from)), ctx.block_position(Some(to)));
    let (min, max) = (a.min(b), a.max(b));
    let count = volume(min, max);
    if count > MAX_BLOCKS {
        return failure!("commands.fill.tooManyBlocks", count, MAX_BLOCKS);
    }
    if !box_loaded(ctx.world, min, max) {
        return failure!("commands.fill.outOfWorld");
    }

    let mut changed = 0usize;
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                let pos = IVec3::new(x, y, z);
                let old = block_at(ctx.world, (x, y, z)).expect("the box is loaded");
                let shell = x == min.x
                    || x == max.x
                    || y == min.y
                    || y == max.y
                    || z == min.z
                    || z == max.z;
                let new = match &mode {
                    Mode::Replace(None) | Mode::Destroy => Some(block),
                    Mode::Replace(Some(filter)) => filter.matches(old).then_some(block),
                    Mode::Outline => shell.then_some(block),
                    Mode::Hollow => Some(if shell { block } else { *AIR }),
                    Mode::Keep => (old == *AIR).then_some(block),
                };
                let Some(new) = new.filter(|&new| new != old) else {
                    continue;
                };
                if matches!(mode, Mode::Destroy) {
                    destroy_block(ctx.world, pos);
                }
                if block_at(ctx.world, (x, y, z)) != Some(new) {
                    set_block(ctx.world, pos, new, false);
                }
                changed += 1;
            }
        }
    }
    if changed == 0 {
        return failure!("commands.fill.failed");
    }
    success!("commands.fill.success", changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::args::assert_covers;
    use crate::command::exec::setblock::tests::{block, drops, place, state};
    use crate::command::exec::test_support::Harness;

    #[test]
    fn enum_covers_the_captured_values() {
        assert_covers::<FillMode>();
    }

    fn count(h: &mut Harness, min: IVec3, max: IVec3, name: &str) -> usize {
        let wanted = state(name);
        let mut n = 0;
        for x in min.x..=max.x {
            for y in min.y..=max.y {
                for z in min.z..=max.z {
                    n += usize::from(block(h, x, y, z) == wanted);
                }
            }
        }
        n
    }

    #[test]
    fn fills_and_clears_a_box_like_bds() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        let (min, max) = (IVec3::new(3, 64, 3), IVec3::new(5, 66, 5));
        assert_eq!(
            h.run(op, "/fill ~ ~ ~ ~2 ~2 ~2 glass"),
            CommandOutput::success("commands.fill.success", vec!["27".into()])
        );
        assert_eq!(count(&mut h, min, max, "minecraft:glass"), 27);
        assert_eq!(
            h.run(op, "/fill ~ ~ ~ ~2 ~2 ~2 air"),
            CommandOutput::success("commands.fill.success", vec!["27".into()])
        );
        assert_eq!(count(&mut h, min, max, "minecraft:air"), 27);
        assert_eq!(
            h.run(op, "/fill ~ ~ ~ ~2 ~2 ~2 air"),
            CommandOutput::failure("commands.fill.failed", vec![])
        );
    }

    #[test]
    fn too_many_blocks_and_outside_the_world_fail() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        assert_eq!(
            h.run(op, "/fill ~ ~ ~ ~100 ~100 ~100 stone"),
            CommandOutput::failure(
                "commands.fill.tooManyBlocks",
                vec!["1030301".into(), "32768".into()]
            )
        );
        assert_eq!(
            h.run(op, "/fill 10 64 10 20 64 10 stone"),
            CommandOutput::failure("commands.fill.outOfWorld", vec![])
        );
        assert_eq!(block(&mut h, 10, 64, 10), *AIR);
    }

    #[test]
    fn outline_hollow_and_keep() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        let (min, max) = (IVec3::new(0, 64, 0), IVec3::new(2, 66, 2));
        place(&mut h, 1, 65, 1, "minecraft:dirt");
        assert_eq!(
            h.run(op, "/fill 0 64 0 2 66 2 stone outline"),
            CommandOutput::success("commands.fill.success", vec!["26".into()])
        );
        assert_eq!(block(&mut h, 1, 65, 1), state("minecraft:dirt"));
        assert_eq!(
            h.run(op, "/fill 0 64 0 2 66 2 stone hollow"),
            CommandOutput::success("commands.fill.success", vec!["1".into()])
        );
        assert_eq!(block(&mut h, 1, 65, 1), *AIR);
        assert_eq!(count(&mut h, min, max, "minecraft:stone"), 26);
        assert_eq!(
            h.run(op, "/fill 0 64 0 2 66 2 glass keep"),
            CommandOutput::success("commands.fill.success", vec!["1".into()])
        );
        assert_eq!(block(&mut h, 1, 65, 1), state("minecraft:glass"));
        assert_eq!(count(&mut h, min, max, "minecraft:stone"), 26);
    }

    #[test]
    fn replace_with_a_filter_and_destroy() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        place(&mut h, 0, 64, 0, "minecraft:dirt");
        place(&mut h, 1, 64, 0, "minecraft:stone");
        assert_eq!(
            h.run(op, "/fill 0 64 0 2 64 0 glass replace dirt"),
            CommandOutput::success("commands.fill.success", vec!["1".into()])
        );
        assert_eq!(block(&mut h, 0, 64, 0), state("minecraft:glass"));
        assert_eq!(block(&mut h, 1, 64, 0), state("minecraft:stone"));
        assert_eq!(block(&mut h, 2, 64, 0), *AIR);
        assert_eq!(
            h.run(op, "/fill 0 64 0 2 64 0 dirt replace"),
            CommandOutput::success("commands.fill.success", vec!["3".into()])
        );
        assert_eq!(
            h.run(op, "/fill 0 64 0 1 64 0 air destroy"),
            CommandOutput::success("commands.fill.success", vec!["2".into()])
        );
        assert_eq!(block(&mut h, 0, 64, 0), *AIR);
        let dirt = ("minecraft:dirt".to_owned(), 1);
        assert_eq!(drops(&mut h), [dirt.clone(), dirt]);
    }

    #[test]
    fn outline_then_replace_count_the_changed_blocks_like_bds() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        let (min, max) = (IVec3::new(3, 64, 3), IVec3::new(5, 66, 5));
        assert_eq!(
            h.run(op, "/fill ~ ~ ~ ~2 ~2 ~2 stone outline"),
            CommandOutput::success("commands.fill.success", vec!["26".into()])
        );
        assert_eq!(count(&mut h, min, max, "minecraft:stone"), 26);
        assert_eq!(
            h.run(op, "/fill ~ ~ ~ ~2 ~2 ~2 glass replace stone"),
            CommandOutput::success("commands.fill.success", vec!["26".into()])
        );
        assert_eq!(count(&mut h, min, max, "minecraft:glass"), 26);
        assert_eq!(block(&mut h, 4, 65, 4), *AIR);
    }
}
