//! `/testfor`, `/testforblock` and `/testforblocks`: read-only checks.
//!
//! Measured on BDS 1.26.51:
//! - `/testfor` answers `commands.testfor.success` with the matched names
//!   (success count: how many matched); no match is
//!   `commands.generic.noTargetMatch`.
//! - `/testforblock` answers `commands.testforblock.success [x, y, z]`, or
//!   `commands.testforblock.failed.tile [x, y, z, %found, %expected]` naming
//!   both blocks by their measured translation keys
//!   ([`names::block_name_key`]), or `…failed.data [x, y, z]` for the right
//!   block in other states; a position below the world holds air.
//! - `/testforblocks` answers `commands.compare.success [blocks compared]`
//!   or `commands.compare.failed`; `masked` skips air in the source.

use super::setblock::{BlockSpec, box_loaded, in_height, volume};
use crate::command::names;
use crate::command::prelude::*;
use crate::registry::BlockRegistry;
use crate::server::game::blocks::block_at;
use crate::world::chunk::blocks::AIR;

overloads! {
    pub enum TestForArgs for "testfor" {
        0 => TestFor { victim: Target },
    }
}

pub fn run_testfor(ctx: &mut CommandContext<'_>, args: TestForArgs) -> CommandResult {
    let TestForArgs::TestFor { victim } = args;
    let found = ctx.entities(Some(&victim))?;
    Ok(
        CommandOutput::success("commands.testfor.success", vec![ctx.names(&found)])
            .with_count(found.len()),
    )
}

overloads! {
    pub enum TestForBlockArgs for "testforblock" {
        0 => TestForBlock {
            position: BlockPosition,
            tile_name: EnumValue = "tileName",
            block_states: Option<BlockStates> = "blockStates",
        },
    }
}

pub fn run_testforblock(ctx: &mut CommandContext<'_>, args: TestForBlockArgs) -> CommandResult {
    let TestForBlockArgs::TestForBlock {
        position,
        tile_name,
        block_states,
    } = args;
    let spec = BlockSpec::resolve(&tile_name, block_states.as_ref())?;
    let pos = ctx.block_position(Some(position));
    // Measured: below the world is air (`8 -100 8` is not out of the world);
    // where nothing is loaded (`/execute in nether`) it is out of the world.
    let found = if in_height(pos.y) {
        let Some(found) = ctx.block_at(pos) else {
            return failure!("commands.testforblock.outOfWorld");
        };
        found
    } else {
        *AIR
    };
    let [x, y, z] = [pos.x, pos.y, pos.z].map(|c| c.to_string());
    if spec.matches(found) {
        return success!("commands.testforblock.success", x, y, z);
    }
    if spec.is_block(found) {
        return failure!("commands.testforblock.failed.data", x, y, z);
    }
    let name = |state: u32| {
        BlockRegistry::vanilla()
            .get_by_runtime_id(state)
            .map(|entry| format!("%{}", names::block_name_key(&entry.string_id)))
            .unwrap_or_default()
    };
    failure!(
        "commands.testforblock.failed.tile",
        x,
        y,
        z,
        name(found),
        name(spec.state())
    )
}

arg_enum! {
    /// Captured `TestForBlocksMode`.
    pub enum CompareMode in "TestForBlocksMode" {
        Masked = "masked",
        All = "all",
    }
}

overloads! {
    pub enum TestForBlocksArgs for "testforblocks" {
        0 => TestForBlocks {
            begin: BlockPosition,
            end: BlockPosition,
            destination: BlockPosition,
            mode: Option<CompareMode>,
        },
    }
}

/// The most blocks one comparison may cover. [INFERENCE] Java Edition's
/// limit; the capture's oversized area was out of the world first.
const MAX_COMPARED: i64 = 524_288;

pub fn run_testforblocks(ctx: &mut CommandContext<'_>, args: TestForBlocksArgs) -> CommandResult {
    let TestForBlocksArgs::TestForBlocks {
        begin,
        end,
        destination,
        mode,
    } = args;
    let [begin, end, destination] = [begin, end, destination].map(|p| ctx.block_position(Some(p)));
    let masked = mode == Some(CompareMode::Masked);
    match compare_regions(ctx.world, begin, end, destination, masked) {
        Ok(compared) => success!("commands.compare.success", compared),
        Err(Compare::TooMany(count)) => {
            failure!("commands.compare.tooManyBlocks", count, MAX_COMPARED)
        }
        Err(Compare::OutOfWorld) => failure!("commands.generic.outOfWorld"),
        Err(Compare::Differ) => failure!("commands.compare.failed"),
    }
}

/// Why two regions did not compare equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Compare {
    TooMany(i64),
    OutOfWorld,
    Differ,
}

/// Compares the box `begin..=end` with the box of the same size at
/// `destination`; `masked` skips air in the source. Returns how many blocks
/// were compared.
pub(super) fn compare_regions(
    world: &World,
    begin: IVec3,
    end: IVec3,
    destination: IVec3,
    masked: bool,
) -> Result<u64, Compare> {
    let (min, max) = (begin.min(end), begin.max(end));
    // Operator coordinates span the whole i32 range: an area whose copy
    // would leave it is out of the world, not an overflow.
    let dest_max = destination.as_i64vec3() + (max.as_i64vec3() - min.as_i64vec3());
    if dest_max.max_element() > i64::from(i32::MAX) || dest_max.min_element() < i64::from(i32::MIN)
    {
        return Err(Compare::OutOfWorld);
    }
    // Measured: an area reaching out of the loaded world fails as such
    // before its size is checked.
    if !box_loaded(world, min, max) || !box_loaded(world, destination, dest_max.as_ivec3()) {
        return Err(Compare::OutOfWorld);
    }
    // Both boxes are loaded, so they lie well inside the i32 range.
    let offset = destination - min;
    let count = volume(min, max);
    if count > MAX_COMPARED {
        return Err(Compare::TooMany(count));
    }
    let mut compared = 0u64;
    for y in min.y..=max.y {
        for z in min.z..=max.z {
            for x in min.x..=max.x {
                let source = block_at(world, (x, y, z)).unwrap_or(*AIR);
                if masked && source == *AIR {
                    continue;
                }
                let target =
                    block_at(world, (x + offset.x, y + offset.y, z + offset.z)).unwrap_or(*AIR);
                if source != target {
                    return Err(Compare::Differ);
                }
                compared += 1;
            }
        }
    }
    Ok(compared)
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::Replay;

    #[test]
    fn testfor_matches_the_bds_capture() {
        Replay::new().assert_segment("testfor", &[]);
    }

    #[test]
    fn testforblock_matches_the_bds_capture() {
        Replay::new().assert_segment("testforblock", &[]);
    }

    #[test]
    fn testforblocks_matches_the_bds_capture() {
        Replay::new().assert_segment("testforblocks", &[]);
    }
    /// Coordinates at the ends of the i32 range answer out of the world
    /// instead of overflowing.
    #[test]
    fn extreme_coordinates_are_out_of_the_world() {
        use crate::command::exec::test_support::Harness;
        use crate::command::output::CommandOutput;
        let mut h = Harness::new();
        let op = h.player("Op", true);
        for line in [
            "/testforblocks 0 60 0 1 61 1 2147483647 60 0",
            "/testforblocks 0 60 0 1 61 1 -2147483648 60 0",
            "/testforblocks -2147483648 60 0 2147483647 60 0 0 60 0",
        ] {
            assert_eq!(
                h.run(op, line),
                CommandOutput::failure("commands.generic.outOfWorld", vec![]),
                "{line}"
            );
        }
    }
}
