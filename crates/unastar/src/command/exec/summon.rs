//! `/summon`, on the entity runtime's `spawn_mob`.

use crate::command::prelude::*;
use crate::command::selector::normalize_type;
use crate::entity::EntityMetadata;
use crate::entity::mob::{MobRegistry, MobSpawnError, SpawnCause, spawn_mob};

pub fn run(ctx: &mut CommandContext<'_>, args: &Args) -> CommandResult {
    let identifier = normalize_type(args.get::<&str>("entityType").expect("required"));
    let position = ctx.position(args.get("spawnPos"));
    let look_at = match (
        args.get::<&CommandPosition>("lookAtPosition"),
        args.get("lookAtEntity"),
    ) {
        (Some(p), _) => Some(ctx.position(Some(p))),
        (None, Some(target)) => {
            let entity = ctx.entities(Some(target))?[0];
            ctx.feet(entity)
        }
        (None, None) => None,
    };
    let yaw = match look_at {
        Some(at) => facing_yaw(position, at),
        None => args
            .get::<RelativeFloat>("yRot")
            .map_or(0.0, |y| y.resolve(ctx.source.yaw)),
    };

    // A spawn event replaces the default `minecraft:entity_spawned`.
    let event = match args.get::<&str>("spawnEvent") {
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
    if let Some(name) = args.get::<&str>("nameTag")
        && let Some(mut metadata) = ctx.world.get_mut::<EntityMetadata>(entity)
    {
        let tagged = std::mem::take(&mut *metadata).with_nametag(name);
        *metadata = tagged;
    }
    success!("commands.summon.success")
}

/// Yaw (degrees, Bedrock convention) that faces from `from` towards `to`.
pub fn facing_yaw(from: DVec3, to: DVec3) -> f32 {
    let delta = to - from;
    (-delta.x.atan2(delta.z)).to_degrees() as f32
}
