//! Lifecycle events emitted only after authoritative combat transitions.

use bevy_ecs::prelude::*;
use tracing::warn;

use crate::entity::components::Player;
use crate::entity::mob::MobKind;
use crate::entity::mob::components::triggers::{
    ActorTrigger, OnHurt, OnHurtByPlayer, OnTargetAcquired, TriggerTarget,
};
use crate::entity::mob::events::{MobFacts, fire_event};

/// Evaluate an authored trigger against its source and optional other actor.
pub fn fire(world: &mut World, actor: Entity, trigger: &ActorTrigger, other: Option<Entity>) {
    if trigger.filter.as_ref().is_some_and(|filter| {
        !filter.evaluate(&MobFacts {
            world,
            entity: actor,
            other,
        })
    }) {
        return;
    }
    let receiver = match trigger.target {
        TriggerTarget::SelfActor => actor,
        TriggerTarget::OtherActor => match other {
            Some(other) => other,
            None => return,
        },
    };
    let Some(handle) = world
        .get::<MobKind>(receiver)
        .and_then(|kind| kind.0.events.event(&trigger.event))
    else {
        warn!(event = %trigger.event, "lifecycle trigger has no receiving mob event");
        return;
    };
    let counterpart = if receiver == actor {
        other
    } else {
        Some(actor)
    };
    if let Err(error) = fire_event(world, receiver, &handle, counterpart) {
        warn!(event = %trigger.event, %error, "lifecycle trigger failed");
    }
}

/// Fire the current hit triggers after the hurt timer and health are committed.
/// The ordinary hurt event precedes the player-specific event. Rejected hits
/// never reach this boundary, while an accepted zero-damage hit does.
pub fn hurt(world: &mut World, actor: Entity, attacker: Option<Entity>, by_player: bool) {
    let ordinary = world.get::<OnHurt>(actor).cloned();
    let player = (by_player
        && attacker.is_some_and(|entity| world.get::<Player>(entity).is_some()))
    .then(|| world.get::<OnHurtByPlayer>(actor).cloned())
    .flatten();
    if let Some(trigger) = ordinary {
        fire(world, actor, &trigger.0, attacker);
    }
    if let Some(trigger) = player {
        fire(world, actor, &trigger.0, attacker);
    }
}

/// Notify the current definition after a nonempty new target was committed.
pub fn target_acquired(world: &mut World, actor: Entity, target: Entity) {
    if let Some(trigger) = world.get::<OnTargetAcquired>(actor).cloned() {
        fire(world, actor, &trigger.0, Some(target));
    }
}
