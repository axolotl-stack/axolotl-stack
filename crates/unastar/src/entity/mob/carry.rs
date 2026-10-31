//! The single real block carried by a mob, its display, death drop and
//! version-independent actor-save representation.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use zuri_nbt::NBTTag;

use crate::entity::components::Position;
use crate::entity::systems::BlockSource;
use crate::registry::block::StateValue;
use crate::server::game::world_stack;

/// A carried canonical block state. It is independent of hand equipment.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CarriedBlock(pub u32);

/// Malformed saved carry data never silently becomes a different block.
#[derive(Debug, thiserror::Error)]
#[error("invalid saved carried block: {0}")]
pub struct CarryStateError(&'static str);

/// Commit carried ownership; native equipment replication projects its display.
/// Callers validate the source transaction before invoking this helper.
pub(crate) fn set(world: &mut World, actor: Entity, state: Option<u32>) {
    let mut entity = world.entity_mut(actor);
    if let Some(state) = state {
        entity.insert(CarriedBlock(state));
    } else {
        entity.remove::<CarriedBlock>();
    }
}

/// Build a display-only hand stack using the exact carried canonical state.
/// It never enters physical equipment or inventory ownership.
pub(crate) fn display_stack(
    state: u32,
    items: &crate::registry::ItemRegistry,
) -> Option<crate::item::ItemStack> {
    let block = crate::registry::BlockRegistry::vanilla().get_by_runtime_id(state)?;
    let mut stack = items.stack(&block.string_id, 1)?;
    stack.runtime_id = i32::try_from(state).ok()?;
    Some(stack)
}

/// Drop the owned block once when death commits, preserving ordinary hand gear.
pub(crate) fn drop_on_death(world: &mut World, actor: Entity) {
    let Some(state) = world.get::<CarriedBlock>(actor).copied() else {
        return;
    };
    let (Some(position), Some(block)) = (
        world.get::<Position>(actor),
        world.registry().get_by_runtime_id(state.0),
    ) else {
        return;
    };
    let Some(stack) = world_stack(world, &block.string_id, 1) else {
        return;
    };
    let position = position.0;
    set(world, actor, None);
    crate::entity::spawn_item_drop(world, position, stack, None);
}

/// Encode the actual carried name and every typed property, never a runtime
/// ordinal whose meaning might change with the next registry version.
pub(crate) fn save(world: &World, actor: Entity) -> Option<NBTTag> {
    let state = world.get::<CarriedBlock>(actor)?.0;
    let block = world.registry().get_by_runtime_id(state)?;
    let properties = block
        .data
        .properties
        .iter()
        .map(|property| {
            let value = match block
                .state_value(state, property.name)
                .expect("canonical block property")
            {
                StateValue::Byte(value) => NBTTag::Byte(value.into()),
                StateValue::Int(value) => NBTTag::Int(value.into()),
                StateValue::Str(value) => NBTTag::String(value.into()),
            };
            (property.name.to_owned(), value)
        })
        .collect::<HashMap<_, _>>();
    Some(NBTTag::Compound(
        HashMap::from([
            (
                "Name".to_owned(),
                NBTTag::String(block.string_id.clone().into()),
            ),
            ("States".to_owned(), NBTTag::Compound(properties.into())),
        ])
        .into(),
    ))
}

/// Restore a named canonical state only if all properties and its item mapping
/// remain valid. Unknown/missing states are explicit actor restore failures.
pub(crate) fn restore(
    world: &mut World,
    actor: Entity,
    saved: Option<&NBTTag>,
) -> Result<(), CarryStateError> {
    let Some(saved) = saved else { return Ok(()) };
    let NBTTag::Compound(fields) = saved else {
        return Err(CarryStateError("expected compound"));
    };
    let (Some(NBTTag::String(name)), Some(NBTTag::Compound(states))) =
        (fields.0.get("Name"), fields.0.get("States"))
    else {
        return Err(CarryStateError("expected name and states"));
    };
    if fields.0.len() != 2 {
        return Err(CarryStateError("unexpected fields"));
    }
    let Some(block) = world.registry().get_by_name(&name.0) else {
        return Err(CarryStateError("unknown block"));
    };
    if block.data.properties.len() != states.0.len()
        || world_stack(world, &block.string_id, 1).is_none()
        || block.string_id == "minecraft:air"
        || block.data.liquid
    {
        return Err(CarryStateError("invalid block or property count"));
    }
    let state = (block.min_state_id..=block.max_state_id)
        .find(|&runtime| {
            block.data.properties.iter().all(|property| {
                match (
                    block.state_value(runtime, property.name),
                    states.0.get(property.name),
                ) {
                    (Some(StateValue::Byte(value)), Some(NBTTag::Byte(saved))) => value == saved.0,
                    (Some(StateValue::Int(value)), Some(NBTTag::Int(saved))) => value == saved.0,
                    (Some(StateValue::Str(value)), Some(NBTTag::String(saved))) => value == saved.0,
                    _ => false,
                }
            })
        })
        .ok_or(CarryStateError("unknown property value"))?;
    set(world, actor, Some(state));
    Ok(())
}
