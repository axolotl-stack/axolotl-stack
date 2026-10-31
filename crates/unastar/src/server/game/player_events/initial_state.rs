//! Identity-bound publication barriers for asynchronous native initialization.

use super::ConnectionIdentity;
use crate::entity::components::{PlayerSession, PlayerUuid, RuntimeEntityId};
use bevy_ecs::prelude::*;

/// A delayed initial state belongs to exactly one actor and connection.
#[derive(Component, Clone)]
pub struct PendingInitialState {
    identity: ConnectionIdentity,
    runtime: i64,
    ready: bool,
}

/// Initial publication has completed for this actor; it cannot be deferred again.
#[derive(Component)]
pub(crate) struct InitialStatePublished;

/// Require every inventory group before an initial packet batch can be constructed.
fn complete(world: &World, player: Entity) -> bool {
    use crate::entity::components::{
        ArmourInventory, GameMode, Health, HeldSlot, Hunger, MainInventory, OffhandSlot, PlayerUi,
        Position, Rotation,
    };
    world.get::<MainInventory>(player).is_some()
        && world.get::<ArmourInventory>(player).is_some()
        && world.get::<OffhandSlot>(player).is_some()
        && world.get::<PlayerUi>(player).is_some()
        && world.get::<Health>(player).is_some()
        && world.get::<Hunger>(player).is_some()
        && world.get::<HeldSlot>(player).is_some()
        && world.get::<GameMode>(player).is_some()
        && world.get::<Position>(player).is_some()
        && world.get::<Rotation>(player).is_some()
}

impl PendingInitialState {
    /// Retained actors may complete initialization after their original transport leaves.
    fn matches(&self, world: &World, player: Entity) -> bool {
        world
            .get::<RuntimeEntityId>(player)
            .is_some_and(|id| id.0 == self.runtime)
            && (self.identity.matches(world, player)
                || (world.get::<PlayerSession>(player).is_none()
                    && world.get::<super::DetachedPlayer>(player).is_some()
                    && world.get::<PlayerUuid>(player).map(|id| id.0) == self.identity.player_uuid))
    }
}

/// Delay initial inventory, abilities, attributes and actor publication.
/// Call from `on_joined` before starting asynchronous initialization. Incoming
/// packets are discarded until release; disconnect remains available.
pub fn defer_initial_state(world: &mut World, player: Entity) -> bool {
    if world.get::<InitialStatePublished>(player).is_some() {
        return false;
    }
    let Some(identity) = ConnectionIdentity::capture(world, player) else {
        return false;
    };
    let Some(runtime) = world.get::<RuntimeEntityId>(player).map(|id| id.0) else {
        return false;
    };
    if let Some(pending) = world.get::<PendingInitialState>(player) {
        return pending.identity.matches(world, player) && pending.runtime == runtime;
    }
    super::discard_input(world, player);
    world.entity_mut(player).insert(PendingInitialState {
        identity,
        runtime,
        ready: false,
    });
    true
}

/// Queue exactly one initial publication after the current ECS tick finishes.
/// The original actor/connection must still match; a retained, sessionless actor
/// may be released to surrounding viewers without sending connection packets.
pub fn release_initial_state(world: &mut World, player: Entity) -> bool {
    let Some(pending) = world.get::<PendingInitialState>(player).cloned() else {
        return false;
    };
    if !pending.matches(world, player) || !complete(world, player) {
        return false;
    }
    world
        .get_mut::<PendingInitialState>(player)
        .expect("checked pending actor")
        .ready = true;
    true
}

/// Whether the actor's initial state has not yet been published.
pub fn initial_state_pending(world: &World, player: Entity) -> bool {
    world.get::<PendingInitialState>(player).is_some()
}

/// Consume only ready barriers whose actor identity remains current.
pub(crate) fn ready_initial_states(world: &mut World) -> Vec<Entity> {
    let mut query = world.query::<(Entity, &PendingInitialState)>();
    let ready: Vec<_> = query
        .iter(world)
        .filter(|(_, pending)| pending.ready)
        .map(|(entity, pending)| (entity, pending.clone()))
        .collect();
    ready
        .into_iter()
        .filter_map(|(entity, pending)| {
            if !pending.matches(world, entity) || !complete(world, entity) {
                return None;
            }
            world.entity_mut(entity).remove::<PendingInitialState>();
            Some(entity)
        })
        .collect()
}
