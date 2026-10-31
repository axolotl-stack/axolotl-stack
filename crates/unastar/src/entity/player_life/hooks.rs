//! Synchronous extension hooks for player damage, death and respawning.
//!
//! Register a [`PlayerLifecycleHandler`] in [`PlayerLifecycleHooks`], or install
//! Bevy observers for the event types in this module. Handlers run first, then
//! observers; cancellable hooks finish before the operation commits.
//! Committed notifications run after the corresponding state change.
//! Cancellation is sticky.
//! Damage is the incoming amount, before difficulty, armour and the hurt timer.
//!
//! Handlers run without any engine lock or borrowed registry. They may access
//! the world and receive nested lifecycle events, so implementations must avoid
//! holding a non-reentrant lock across an operation that dispatches another
//! event. Changes to the handler registry take effect on the next dispatch.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;

use super::death::DeathCause;
use crate::entity::mob::combat::Hit;

/// A proposed player hit. No health, inventory, timer or packet changes have
/// been committed when this event is delivered.
#[derive(Event, Debug, Clone)]
pub struct PlayerDamage {
    player: Entity,
    /// Incoming hit; extensions may change its damage and knockback. Modified
    /// values are validated before the ordinary damage rules are applied.
    pub hit: Hit,
    cancelled: bool,
}

impl PlayerDamage {
    /// Identify the player this hit would hurt.
    pub fn player(&self) -> Entity {
        self.player
    }

    /// Prevent the hit and its normal side effects. Later handlers cannot
    /// restore an operation that has been cancelled.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// Whether a handler has prevented this hit.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    /// Construct the decision for an incoming hit.
    pub(super) fn new(player: Entity, hit: Hit) -> Self {
        Self {
            player,
            hit,
            cancelled: false,
        }
    }
}

/// A native notification of positive health loss, after ordinary hit effects
/// and before a possible death. Cancelled, invalid and hurt-timer-suppressed
/// hits do not emit this event. This notification cannot cancel committed loss.
#[derive(Event, Debug, Clone, Copy)]
pub struct PlayerDamaged {
    /// Player whose health changed.
    pub player: Entity,
    /// The validated incoming hit after pre-damage hooks.
    pub hit: Hit,
    /// Actual health removed, after difficulty, armour and the hurt timer.
    pub damage: f32,
}

/// A committed player death, after the dead marker and ordinary drops are
/// applied and before an optional immediate respawn. This is a notification,
/// not a cancellable decision; prevent lethal damage with [`PlayerDamage`].
#[derive(Event, Debug, Clone, Copy)]
pub struct PlayerDeath {
    /// Player whose death committed.
    pub player: Entity,
    /// Damage cause and credited attacker.
    pub cause: DeathCause,
}

/// A proposed respawn, before health, hunger, position or packets change.
#[derive(Event, Debug, Clone)]
pub struct PlayerRespawn {
    player: Entity,
    /// Destination in the player's eye-height position convention. It must be
    /// finite and representable in the network's single-precision coordinates.
    pub position: DVec3,
    cancelled: bool,
}

impl PlayerRespawn {
    /// Identify the dead player requesting a respawn.
    pub fn player(&self) -> Entity {
        self.player
    }

    /// Keep the player dead and suppress the normal respawn acknowledgement.
    /// Later handlers cannot restore an operation that has been cancelled.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// Whether a handler has prevented this respawn.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    /// Construct the decision for a dead player's request.
    pub(super) fn new(player: Entity, position: DVec3) -> Self {
        Self {
            player,
            position,
            cancelled: false,
        }
    }
}

/// A committed respawn, after the player is alive at its new position and
/// the ordinary client updates have been sent.
#[derive(Event, Debug, Clone, Copy)]
pub struct PlayerRespawned {
    /// Player restored to life.
    pub player: Entity,
    /// Committed destination in the player's eye-height position convention.
    pub position: DVec3,
}

/// Native extension callbacks with synchronous access to the live world.
///
/// Methods may receive nested events. Implementations with internal locks
/// must account for this; the engine holds no registry borrow while calling
/// them. All methods default to allowing the ordinary server behavior.
pub trait PlayerLifecycleHandler: Send + Sync + 'static {
    /// Inspect, adjust or cancel a proposed hit before it commits.
    fn on_damage(&self, _world: &mut World, _event: &mut PlayerDamage) {}

    /// Observe positive health loss before a possible death notification.
    fn on_damaged(&self, _world: &mut World, _event: &PlayerDamaged) {}

    /// Observe a death that has committed exactly once.
    fn on_death(&self, _world: &mut World, _event: &PlayerDeath) {}

    /// Inspect, redirect or cancel a proposed respawn before it commits.
    fn on_respawn(&self, _world: &mut World, _event: &mut PlayerRespawn) {}

    /// Observe a respawn that has committed exactly once.
    fn on_respawned(&self, _world: &mut World, _event: &PlayerRespawned) {}
}

/// Ordered lifecycle handlers installed by an embedding application.
///
/// Each dispatch uses a snapshot in registration order. Bevy observers run
/// after these handlers and follow Bevy's observer ordering rules.
#[derive(Resource, Default, Clone)]
pub struct PlayerLifecycleHooks {
    handlers: Vec<Arc<dyn PlayerLifecycleHandler>>,
}

impl PlayerLifecycleHooks {
    /// Register an extension for subsequent lifecycle dispatches.
    pub fn register(&mut self, handler: impl PlayerLifecycleHandler) {
        self.handlers.push(Arc::new(handler));
    }
}

/// Clone the current handler list so callbacks can freely access the world.
fn handlers(world: &World) -> Vec<Arc<dyn PlayerLifecycleHandler>> {
    world
        .get_resource::<PlayerLifecycleHooks>()
        .map_or_else(Vec::new, |hooks| hooks.handlers.clone())
}

/// Run all hit handlers and immediate observers before committing damage.
pub(super) fn damage(world: &mut World, event: &mut PlayerDamage) {
    for handler in handlers(world) {
        handler.on_damage(world, event);
    }
    world.trigger_ref(event);
}

/// Report actual positive health loss without exposing an uncommitted attempt.
pub(super) fn damaged(world: &mut World, event: PlayerDamaged) {
    for handler in handlers(world) {
        handler.on_damaged(world, &event);
    }
    world.trigger(event);
}

/// Report a committed death to handlers and immediate observers.
pub(super) fn death(world: &mut World, event: PlayerDeath) {
    for handler in handlers(world) {
        handler.on_death(world, &event);
    }
    world.trigger(event);
}

/// Run all respawn handlers and immediate observers before restoring life.
pub(super) fn respawn(world: &mut World, event: &mut PlayerRespawn) {
    for handler in handlers(world) {
        handler.on_respawn(world, event);
    }
    world.trigger_ref(event);
}

/// Report a committed respawn to handlers and immediate observers.
pub(super) fn respawned(world: &mut World, event: PlayerRespawned) {
    for handler in handlers(world) {
        handler.on_respawned(world, &event);
    }
    world.trigger(event);
}
