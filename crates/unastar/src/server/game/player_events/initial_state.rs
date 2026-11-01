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

/// Native initial packet construction requires a complete unfenced physical source.
pub(crate) fn initial_state_ready(world: &World, player: Entity) -> bool {
    complete(world, player)
        && !super::super::inventory_effects::is_quarantined(world, player)
        && !super::super::inventory_effects::is_recovery_fenced(world, player)
        && !super::super::player_state::source_actors::admission_blocked(world, player)
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
    if let Some(session) = world.get::<PlayerSession>(player) {
        session.hold_initial_packets();
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
    if !pending.matches(world, player) || !initial_state_ready(world, player) {
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
            if !pending.matches(world, entity)
                || !initial_state_ready(world, entity)
                || crate::plugin::manager::sessions::blocks_publication(world, entity)
            {
                return None;
            }
            world.entity_mut(entity).remove::<PendingInitialState>();
            Some(entity)
        })
        .collect()
}

/// Open ordinary producers only after the complete native initial batch was enqueued.
pub(crate) fn mark_initial_state_published(world: &mut World, player: Entity) {
    super::super::player_state::forget_initial_record(world, player);
    if let Some(session) = world.get::<PlayerSession>(player) {
        session.release_initial_packets();
    }
    world.entity_mut(player).insert(InitialStatePublished);
}

/// Consume one exact ready barrier as part of an already reserved native initial batch.
pub(crate) fn take_ready_initial_state(world: &mut World, player: Entity) -> bool {
    let ready = world
        .get::<PendingInitialState>(player)
        .is_some_and(|pending| pending.ready && pending.matches(world, player))
        && initial_state_ready(world, player)
        && !crate::plugin::manager::sessions::blocks_publication(world, player);
    if ready {
        world.entity_mut(player).remove::<PendingInitialState>();
    }
    ready
}
