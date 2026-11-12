//! Fuses (`minecraft:explode`, compiled in `mob::components::explode`).
//!
//! While a fuse burns the actor's `IGNITED` flag is set and its `FUSE_TIME`
//! counts down; the tick after the fuse runs out the actor explodes (a
//! sixteenth of its height above its feet, `server::game::explosion`) and
//! is removed without dying. A primed TNT lit at tick t explodes at tick
//! t + 81 (BDS 1.26.51, `data/measured/explosions/tnt_prime.json`).

use bevy_ecs::prelude::*;
use jolyne::valentine::actor_data_i_ds as ids;
use jolyne::valentine::actor_flags;

use crate::ecs::resources::TickCounter;
use crate::entity::components::{PhysicsBody, Position};
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::MobKind;
use crate::entity::mob::ai::set_flag;
use crate::entity::mob::components::explode::{Explode, Fuse};
use crate::entity::mob::state::Dead;
use crate::server::game::explosion::{Blast, blast_centre, explode};

/// System (exclusive, `EntityLogicSet`): burn the lit fuses down and
/// explode the actors whose fuse ran out.
pub fn tick_fuses(
    world: &mut World,
    fuses: &mut QueryState<(Entity, &Fuse, Has<Dead>), With<MobKind>>,
    mut burning: Local<Vec<(Entity, Fuse, bool)>>,
) {
    let now = world.resource::<TickCounter>().current;
    burning.extend(
        fuses
            .iter(world)
            .map(|(entity, fuse, dead)| (entity, *fuse, dead)),
    );
    for (entity, fuse, dead) in burning.drain(..) {
        let Ok(mut actor) = world.get_entity_mut(entity) else {
            continue;
        };
        set_flag(&mut actor, actor_flags::IGNITED, true);
        let remaining = fuse.explode_at.saturating_sub(now);
        if let Some(mut metadata) = actor.get_mut::<EntityMetadata>() {
            metadata.set_int(ids::FUSE_TIME, i32::try_from(remaining).unwrap_or(i32::MAX));
        }
        if now > fuse.explode_at && !dead {
            explode_actor(world, entity);
        }
    }
}

/// Explode `actor` with its `minecraft:explode` and remove it.
fn explode_actor(world: &mut World, actor: Entity) {
    let (Some(config), Some(feet), Some(body)) = (
        world
            .get::<Explode>(actor)
            .map(|explode| explode.config.clone()),
        world.get::<Position>(actor).map(|position| position.0),
        world.get::<PhysicsBody>(actor).copied(),
    ) else {
        return;
    };
    let griefing = world
        .get_resource::<crate::world::rules::GameRules>()
        .is_none_or(|rules| rules.mob_griefing);
    let blast = Blast {
        centre: blast_centre(feet, body.height),
        power: config.power,
        source: Some(actor),
        breaks_blocks: config.breaks_blocks && (!config.destroy_affected_by_griefing || griefing),
        max_resistance: config.max_resistance,
        damage_scaling: config.damage_scaling,
        knockback_scaling: config.knockback_scaling,
        sound: config.sound.clone(),
    };
    world.despawn(actor);
    explode(world, &blast);
}
