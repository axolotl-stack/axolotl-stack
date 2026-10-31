//! `/setblock <position> <tileName> [blockStates] [replace|destroy|keep]`,
//! plus the block-argument resolution `/fill` and `/clone` share.
//!
//! Measured (BDS 1.26.51): placing answers `commands.setblock.success` (no
//! params) and viewers get `UpdateBlock` (flags 3, no place sound); placing
//! the block already there, or `keep` over a non-air block, answers
//! `commands.setblock.noChange`.
//!
//! Legacy block names BDS still accepts with old states
//! (`wool ["color"="red"]` is `minecraft:red_wool` on BDS) need BDS's legacy
//! block-state upgrade table, which unastar does not have: such names answer
//! a literal "not implemented" failure. Current identifiers with their
//! current states work.

use crate::command::parser::StateLiteral;
use crate::command::prelude::*;
use crate::registry::BlockRegistry;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::blocks::{block_at, destroy_block, set_block};
use crate::world::chunk::blocks::AIR;

arg_enum! {
    /// Captured `SetBlockMode`.
    pub enum SetBlockMode in "SetBlockMode" {
        Replace = "replace",
        Destroy = "destroy",
        Keep = "keep",
    }
}

overloads! {
    pub enum SetBlockArgs for "setblock" {
        0 => WithStates {
            position: BlockPosition,
            tile_name: EnumValue = "tileName",
            block_states: BlockStates = "blockStates",
            old_block_handling: Option<SetBlockMode> = "oldBlockHandling",
        },
        1 => Plain {
            position: BlockPosition,
            tile_name: EnumValue = "tileName",
            old_block_handling: Option<SetBlockMode> = "oldBlockHandling",
        },
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: SetBlockArgs) -> CommandResult {
    let (position, tile_name, states, mode) = match args {
        SetBlockArgs::WithStates {
            position,
            tile_name,
            block_states,
            old_block_handling,
        } => (position, tile_name, Some(block_states), old_block_handling),
        SetBlockArgs::Plain {
            position,
            tile_name,
            old_block_handling,
        } => (position, tile_name, None, old_block_handling),
    };
    let block = BlockSpec::resolve(&tile_name, states.as_ref())?.state();
    let pos = ctx.block_position(Some(position));
    if crate::server::game::container_cell_locked(ctx.world, pos) {
        return failure!("commands.setblock.noChange");
    }
    let Some(old) = block_at(ctx.world, (pos.x, pos.y, pos.z)) else {
        return failure!("commands.setblock.outOfWorld");
    };
    let mode = mode.unwrap_or(SetBlockMode::Replace);
    // [INFERENCE] `destroy` over the same block is no change either.
    if old == block || (mode == SetBlockMode::Keep && old != *AIR) {
        return failure!("commands.setblock.noChange");
    }
    if mode == SetBlockMode::Destroy {
        destroy_block(ctx.world, pos);
    }
    if block_at(ctx.world, (pos.x, pos.y, pos.z)) != Some(block) {
        set_block(ctx.world, pos, block, false);
    }
    success!("commands.setblock.success")
}

/// A block argument: the block and the states the command names.
#[derive(Debug, Clone)]
pub(super) struct BlockSpec {
    entry: &'static BlockEntry,
    states: Vec<(&'static str, StateValue)>,
}

impl BlockSpec {
    /// Resolves `tileName` (`stone`, `minecraft:stone`) and its block states
    /// with BDS's block-state errors.
    pub(super) fn resolve(
        tile_name: &str,
        states: Option<&BlockStates>,
    ) -> Result<Self, CommandOutput> {
        let identifier = if tile_name.contains(':') {
            tile_name.to_owned()
        } else {
            format!("minecraft:{tile_name}")
        };
        let Some(entry) = BlockRegistry::vanilla().get_by_name(&identifier) else {
            // Every captured `Block` value that is not a current identifier
            // is a legacy alias resolved through the upgrade table.
            return Err(CommandOutput::failure(
                format!("Legacy block name '{tile_name}' is not implemented in unastar yet"),
                vec![],
            ));
        };
        let mut resolved = Vec::new();
        for (name, literal) in states.map_or(&[][..], |s| s.0.as_slice()) {
            let Some(property) = entry
                .data
                .properties
                .iter()
                .find(|p| p.name == name.as_str())
            else {
                return Err(CommandOutput::failure(
                    "commands.blockstate.stateError",
                    vec![name.clone(), tile_name.to_owned()],
                ));
            };
            let value = property
                .values
                .iter()
                .copied()
                .find(|value| match (value, literal) {
                    (StateValue::Str(v), StateLiteral::String(s)) => *v == s.as_str(),
                    (StateValue::Int(v), StateLiteral::Int(i)) => v == i,
                    (StateValue::Byte(v), StateLiteral::Bool(b)) => *v == u8::from(*b),
                    _ => false,
                });
            let type_matches = matches!(
                (property.values.first(), literal),
                (Some(StateValue::Str(_)), StateLiteral::String(_))
                    | (Some(StateValue::Int(_)), StateLiteral::Int(_))
                    | (Some(StateValue::Byte(_)), StateLiteral::Bool(_))
            );
            match value {
                Some(value) => resolved.push((property.name, value)),
                None if type_matches => {
                    return Err(CommandOutput::failure(
                        "commands.blockstate.valueError",
                        vec![name.clone()],
                    ));
                }
                None => {
                    return Err(CommandOutput::failure(
                        "commands.blockstate.typeError",
                        vec![name.clone()],
                    ));
                }
            }
        }
        let spec = Self {
            entry,
            states: resolved,
        };
        // A combination of values no canonical state has.
        if spec.try_state().is_none() {
            return Err(CommandOutput::failure(
                "commands.blockstate.valueError",
                vec![spec.states.last().map_or("", |(name, _)| name).to_owned()],
            ));
        }
        Ok(spec)
    }

    fn try_state(&self) -> Option<u32> {
        self.states
            .iter()
            .try_fold(self.entry.default_state_id, |state, (name, value)| {
                self.entry.with_state_value(state, name, *value)
            })
    }

    /// The runtime state: the block's default with the named states set.
    pub(super) fn state(&self) -> u32 {
        self.try_state()
            .expect("resolve checked the state combination")
    }

    /// Whether `state` is this block with the named states (any value for
    /// the states not named), as fill/clone filters match.
    pub(super) fn matches(&self, state: u32) -> bool {
        (self.entry.min_state_id..=self.entry.max_state_id).contains(&state)
            && self
                .states
                .iter()
                .all(|(name, value)| self.entry.state_value(state, name) == Some(*value))
    }
}

/// Whether every block of the box `min..=max` is inside the world height and
/// in a loaded chunk.
pub(super) fn box_loaded(world: &World, min: IVec3, max: IVec3) -> bool {
    use crate::world::ecs::{ChunkManager, world_to_chunk_coords};
    let bottom = crate::world::chunk::MIN_Y;
    let top = bottom + i32::from(crate::world::chunk::SUBCHUNK_COUNT) * 16;
    if min.y < bottom || max.y >= top {
        return false;
    }
    let Some(chunks) = world.get_resource::<ChunkManager>() else {
        return false;
    };
    let (min_cx, min_cz) = world_to_chunk_coords(min.x, min.z);
    let (max_cx, max_cz) = world_to_chunk_coords(max.x, max.z);
    (min_cx..=max_cx).all(|cx| (min_cz..=max_cz).all(|cz| chunks.get_by_coords(cx, cz).is_some()))
}

/// Number of blocks in the box `min..=max`.
pub(super) fn volume(min: IVec3, max: IVec3) -> i64 {
    let size = (max - min).as_i64vec3() + glam::I64Vec3::ONE;
    size.x * size.y * size.z
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::command::args::assert_covers;
    use crate::command::exec::test_support::Harness;

    pub(in crate::command::exec) fn state(name: &str) -> u32 {
        BlockRegistry::vanilla()
            .default_state_id_by_name(name)
            .expect(name)
    }

    pub(in crate::command::exec) fn block(h: &mut Harness, x: i32, y: i32, z: i32) -> u32 {
        block_at(h.world(), (x, y, z)).expect("a loaded block")
    }

    pub(in crate::command::exec) fn place(h: &mut Harness, x: i32, y: i32, z: i32, name: &str) {
        set_block(h.world(), IVec3::new(x, y, z), state(name), false);
    }

    #[test]
    fn enum_covers_the_captured_values() {
        assert_covers::<SetBlockMode>();
    }

    #[test]
    fn places_then_reports_no_change_like_bds() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        assert_eq!(
            h.run(op, "/setblock ~ ~-1 ~ stone"),
            CommandOutput::success("commands.setblock.success", vec![])
        );
        assert_eq!(block(&mut h, 3, 63, 3), state("minecraft:stone"));
        assert_eq!(
            h.run(op, "/setblock ~ ~-1 ~ stone"),
            CommandOutput::failure("commands.setblock.noChange", vec![])
        );
        assert_eq!(
            h.run(op, "/setblock 3 63 3 minecraft:dirt"),
            CommandOutput::success("commands.setblock.success", vec![])
        );
        assert_eq!(block(&mut h, 3, 63, 3), state("minecraft:dirt"));
    }

    #[test]
    fn keep_only_fills_air() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        place(&mut h, 4, 64, 3, "minecraft:red_wool");
        assert_eq!(
            h.run(op, "/setblock ~1 ~ ~ blue_concrete keep"),
            CommandOutput::failure("commands.setblock.noChange", vec![])
        );
        assert_eq!(block(&mut h, 4, 64, 3), state("minecraft:red_wool"));
        assert_eq!(
            h.run(op, "/setblock ~2 ~ ~ blue_concrete keep"),
            CommandOutput::success("commands.setblock.success", vec![])
        );
        assert_eq!(block(&mut h, 5, 64, 3), state("minecraft:blue_concrete"));
    }

    #[test]
    fn block_states_select_the_state() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        assert_eq!(
            h.run(
                op,
                r#"/setblock 1 64 1 oak_stairs ["weirdo_direction"=2,"upside_down_bit"=true]"#
            ),
            CommandOutput::success("commands.setblock.success", vec![])
        );
        let stairs = BlockRegistry::vanilla()
            .get_by_name("minecraft:oak_stairs")
            .unwrap();
        let placed = block(&mut h, 1, 64, 1);
        assert_eq!(
            stairs.state_value(placed, "weirdo_direction"),
            Some(StateValue::Int(2))
        );
        assert_eq!(
            stairs.state_value(placed, "upside_down_bit"),
            Some(StateValue::Byte(1))
        );
        assert_eq!(
            h.run(op, r#"/setblock 1 64 1 oak_stairs ["color"="red"]"#),
            CommandOutput::failure(
                "commands.blockstate.stateError",
                vec!["color".into(), "oak_stairs".into()]
            )
        );
        assert_eq!(
            h.run(op, r#"/setblock 1 64 1 oak_stairs ["weirdo_direction"=9]"#),
            CommandOutput::failure(
                "commands.blockstate.valueError",
                vec!["weirdo_direction".into()]
            )
        );
        assert_eq!(
            h.run(
                op,
                r#"/setblock 1 64 1 oak_stairs ["weirdo_direction"="north"]"#
            ),
            CommandOutput::failure(
                "commands.blockstate.typeError",
                vec!["weirdo_direction".into()]
            )
        );
    }

    #[test]
    fn legacy_names_and_unloaded_blocks_fail() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        let legacy = h.run(op, r#"/setblock ~1 ~ ~ minecraft:wool ["color"="red"]"#);
        assert!(!legacy.is_success());
        assert!(
            legacy.messages[0]
                .key
                .contains("is not implemented in unastar yet")
        );
        assert_eq!(block(&mut h, 4, 64, 3), *AIR);
        assert_eq!(
            h.run(op, "/setblock 100 64 0 stone"),
            CommandOutput::failure("commands.setblock.outOfWorld", vec![])
        );
        assert_eq!(
            h.run(op, "/setblock 0 400 0 stone"),
            CommandOutput::failure("commands.setblock.outOfWorld", vec![])
        );
    }

    /// The dropped items as `(item id, count)`.
    pub(in crate::command::exec) fn drops(h: &mut Harness) -> Vec<(String, u8)> {
        use crate::entity::components::{DroppedItem, ItemEntityStack};
        use bevy_ecs::prelude::With;
        let world = h.world();
        world
            .query_filtered::<&ItemEntityStack, With<DroppedItem>>()
            .iter(world)
            .map(|stack| (stack.0.item_id().to_owned(), stack.0.count))
            .collect()
    }

    #[test]
    fn destroy_breaks_the_block_like_bds() {
        use crate::world::ecs::{ChunkManager, ChunkViewers};
        use jolyne::valentine::McpePacketData;
        let mut h = Harness::new();
        let op = h.player("Op", true);
        let chunk = h
            .world()
            .resource::<ChunkManager>()
            .get_by_coords(0, 0)
            .unwrap();
        let mut viewers = ChunkViewers::default();
        viewers.insert(op);
        h.world().entity_mut(chunk).insert(viewers);
        place(&mut h, 3, 63, 3, "minecraft:dirt");
        h.sent(op);

        assert_eq!(
            h.run(op, "/setblock ~ ~-1 ~ air destroy"),
            CommandOutput::success("commands.setblock.success", vec![])
        );
        assert_eq!(block(&mut h, 3, 63, 3), *AIR);
        let ids = crate::server::game::types::block_network_ids(h.world());
        let sent: Vec<_> = h
            .sent(op)
            .into_iter()
            .filter_map(|packet| match packet {
                McpePacketData::LevelEventPacket(event) => Some(format!(
                    "LevelEvent {} ({}, {}, {}) {}",
                    event.event_id,
                    event.position.x,
                    event.position.y,
                    event.position.z,
                    event.data
                )),
                McpePacketData::UpdateBlockPacket(update) => Some(format!(
                    "UpdateBlock ({}, {}, {}) {} flags={}",
                    update.block_position.x,
                    update.block_position.y,
                    update.block_position.z,
                    update.block_runtime_id,
                    update.flags
                )),
                _ => None,
            })
            .collect();
        assert_eq!(
            sent,
            [
                format!(
                    "LevelEvent 2001 (3.5, 63.5, 3.5) {}",
                    ids.to_wire(state("minecraft:dirt")) as i32
                ),
                format!("UpdateBlock (3, 63, 3) {} flags=3", ids.to_wire(*AIR)),
            ]
        );
        assert_eq!(drops(&mut h), [("minecraft:dirt".to_owned(), 1)]);
    }

    #[test]
    fn destroy_then_places_the_new_block() {
        let mut h = Harness::new();
        let op = h.player("Op", true);
        place(&mut h, 5, 64, 5, "minecraft:oak_planks");
        assert_eq!(
            h.run(op, "/setblock 5 64 5 stone destroy"),
            CommandOutput::success("commands.setblock.success", vec![])
        );
        assert_eq!(block(&mut h, 5, 64, 5), state("minecraft:stone"));
        assert_eq!(drops(&mut h), [("minecraft:oak_planks".to_owned(), 1)]);
    }
}
