//! `/summon`, on the entity runtime's `spawn_mob`.
//!
//! Measured (BDS 1.26.51): `commands.summon.success` with no parameters; an
//! unknown type is a syntax error (the `EntityType` enum).

use crate::command::prelude::*;
use crate::command::selector::normalize_type;
use crate::entity::EntityMetadata;
use crate::entity::geometry::yaw_towards;
use crate::entity::mob::{MobRegistry, MobSpawnError, SpawnCause, spawn_mob};

overloads! {
    pub enum SummonArgs for "summon" {
        0 => Rotated {
            entity_type: EnumValue = "entityType",
            position: Option<CommandPosition> = "spawnPos",
            yaw: Option<RelativeFloat> = "yRot",
            pitch: Option<RelativeFloat> = "xRot",
            event: Option<SoftEnumValue> = "spawnEvent",
            name: Option<Text> = "nameTag",
        },
        1 => Named {
            entity_type: EnumValue = "entityType",
            name: Text = "nameTag",
            position: Option<CommandPosition> = "spawnPos",
        },
        2 => FacingPosition {
            entity_type: EnumValue = "entityType",
            position: Option<CommandPosition> = "spawnPos",
            facing: EnumValue,
            at: CommandPosition = "lookAtPosition",
            event: Option<SoftEnumValue> = "spawnEvent",
            name: Option<Text> = "nameTag",
        },
        3 => FacingEntity {
            entity_type: EnumValue = "entityType",
            position: Option<CommandPosition> = "spawnPos",
            facing: EnumValue,
            at: Target = "lookAtEntity",
            event: Option<SoftEnumValue> = "spawnEvent",
            name: Option<Text> = "nameTag",
        },
    }
}

/// Which way the mob faces.
enum Facing {
    Yaw(Option<RelativeFloat>),
    Position(CommandPosition),
    Entity(Target),
}

pub fn run(ctx: &mut CommandContext<'_>, args: SummonArgs) -> CommandResult {
    use SummonArgs as S;
    let (entity_type, position, facing, event, name) = match args {
        S::Rotated {
            entity_type,
            position,
            yaw,
            event,
            name,
            ..
        } => (entity_type, position, Facing::Yaw(yaw), event, name),
        S::Named {
            entity_type,
            name,
            position,
        } => (entity_type, position, Facing::Yaw(None), None, Some(name)),
        S::FacingPosition {
            entity_type,
            position,
            at,
            event,
            name,
            ..
        } => (entity_type, position, Facing::Position(at), event, name),
        S::FacingEntity {
            entity_type,
            position,
            at,
            event,
            name,
            ..
        } => (entity_type, position, Facing::Entity(at), event, name),
    };
    let identifier = normalize_type(&entity_type);
    let position = ctx.position(position);
    let yaw = match facing {
        Facing::Yaw(yaw) => yaw.map_or(0.0, |y| y.resolve(ctx.source.yaw)),
        Facing::Position(at) => yaw_towards(position, ctx.position(Some(at))),
        Facing::Entity(target) => {
            let entity = ctx.entities(Some(&target))?[0];
            ctx.feet(entity).map_or(0.0, |at| yaw_towards(position, at))
        }
    };

    // A spawn event replaces the default `minecraft:entity_spawned`.
    let event = match &event {
        Some(name) => Some(
            ctx.world
                .get_resource::<MobRegistry>()
                .and_then(|registry| registry.get(&identifier))
                .and_then(|archetype| archetype.events.event(name))
                .ok_or_else(|| CommandOutput::failure("commands.summon.failed", vec![]))?,
        ),
        None => None,
    };
    let cause = if event.is_some() {
        SpawnCause::Loaded
    } else {
        SpawnCause::Spawned
    };
    let entity =
        spawn_mob(ctx.world, &identifier, position, yaw, cause).map_err(|error| match error {
            MobSpawnError::Unsupported { identifier, reason } => {
                CommandOutput::failure(format!("Unable to summon {identifier}: {reason}"), vec![])
            }
            _ => CommandOutput::failure("commands.summon.failed", vec![]),
        })?;
    if let Some(event) = event {
        crate::entity::mob::events::fire_event(ctx.world, entity, &event, None)
            .map_err(|_| CommandOutput::failure("commands.summon.failed", vec![]))?;
        crate::entity::definition::apply_pending_definitions(ctx.world);
    }
    if let Some(name) = name
        && let Some(mut metadata) = ctx.world.get_mut::<EntityMetadata>(entity)
    {
        let tagged = std::mem::take(&mut *metadata).with_nametag(&name);
        *metadata = tagged;
    }
    success!("commands.summon.success")
}
