//! Native authorization for actual projectile impacts, teleports and effects.
//!
//! Handlers run from snapshots without holding the registry borrowed. Vetoed
//! impacts discard the projectile without its normal impact effects. Teleport
//! destinations and individual effect recipients are checked at their actual
//! application boundary, rather than authorizing only the original item click.

use crate::entity::components::{EffectType, PlayerSession, PlayerUuid, Position};
use bevy_ecs::prelude::*;
use glam::DVec3;
use std::sync::Arc;

/// The actual object encountered by a projectile flight segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectileImpactTarget {
    Block,
    Entity(Entity),
}

/// A pending impact with immutable source, actual target and collision point.
#[derive(Event, Clone, Debug)]
pub struct ProjectileImpact {
    projectile: Entity,
    owner: Option<Entity>,
    target: ProjectileImpactTarget,
    position: DVec3,
    cancelled: bool,
}
impl ProjectileImpact {
    /// Read the projectile actor.
    pub fn projectile(&self) -> Entity {
        self.projectile
    }
    /// Read its current owner, if still present.
    pub fn owner(&self) -> Option<Entity> {
        self.owner
    }
    /// Read the actual collision target.
    pub fn target(&self) -> ProjectileImpactTarget {
        self.target
    }
    /// Read the actual collision point.
    pub fn position(&self) -> DVec3 {
        self.position
    }
    /// Discard this projectile without ordinary impact effects.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
    /// Whether an earlier extension vetoed the impact.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A proposed player teleport at the actual destination application boundary.
#[derive(Event, Clone, Debug)]
pub struct PlayerTeleport {
    player: Entity,
    source: Option<Entity>,
    from: DVec3,
    cause: crate::server::game::item_use::TeleportCause,
    /// Destination in feet coordinates, matching the public item teleport API.
    pub destination: DVec3,
    cancelled: bool,
}
impl PlayerTeleport {
    /// Read the teleported player.
    pub fn player(&self) -> Entity {
        self.player
    }
    /// Read the originating actor, such as an impacting projectile.
    pub fn source(&self) -> Option<Entity> {
        self.source
    }
    /// Read the original feet position.
    pub fn from(&self) -> DVec3 {
        self.from
    }
    /// Read the protocol reason for this teleport.
    pub fn cause(&self) -> crate::server::game::item_use::TeleportCause {
        self.cause
    }
    /// Prevent teleportation and normal teleport packets.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
    /// Whether teleportation was rejected.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A proposed effect on one actual recipient.
#[derive(Event, Clone, Debug)]
pub struct EffectApplication {
    target: Entity,
    source: Option<Entity>,
    effect: EffectType,
    pub amplifier: u8,
    pub duration_ticks: u32,
    cancelled: bool,
}
impl EffectApplication {
    /// Read the actual affected actor.
    pub fn target(&self) -> Entity {
        self.target
    }
    /// Read the source actor, when the effect has one.
    pub fn source(&self) -> Option<Entity> {
        self.source
    }
    /// Read the effect being applied.
    pub fn effect(&self) -> EffectType {
        self.effect
    }
    /// Prevent this recipient from receiving the effect.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
    /// Whether this recipient's application was rejected.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// Native callbacks for projectile and effect application boundaries.
pub trait ProjectileHandler: Send + Sync + 'static {
    /// Authorize an actual collision before its ordinary effects.
    fn on_impact(&self, _world: &mut World, _event: &mut ProjectileImpact) {}
    /// Authorize or redirect a destination before the player moves.
    fn on_teleport(&self, _world: &mut World, _event: &mut PlayerTeleport) {}
    /// Authorize or cap an effect for an actual recipient.
    fn on_effect(&self, _world: &mut World, _event: &mut EffectApplication) {}
}

/// Ordered native projectile/effect authorization registry.
#[derive(Resource, Default, Clone)]
pub struct ProjectileHooks {
    handlers: Vec<Arc<dyn ProjectileHandler>>,
}
impl ProjectileHooks {
    /// Install a handler for subsequent dispatch snapshots.
    pub fn register(&mut self, handler: impl ProjectileHandler) {
        self.handlers.push(Arc::new(handler));
    }
}

/// Snapshot handlers so callbacks may freely access the live world.
fn handlers(world: &World) -> Vec<Arc<dyn ProjectileHandler>> {
    world
        .get_resource::<ProjectileHooks>()
        .map_or_else(Vec::new, |hooks| hooks.handlers.clone())
}

/// Account and outbound transport facts retained across an extension callback.
type SessionSnapshot = (
    u64,
    Option<String>,
    Option<String>,
    tokio::sync::mpsc::Sender<jolyne::valentine::McpePacket>,
);

/// Retain actor and transport identity without an ECS borrow.
struct Identity {
    uuid: Option<uuid::Uuid>,
    session: Option<SessionSnapshot>,
}
impl Identity {
    /// Snapshot identity for callback revalidation.
    fn capture(world: &World, actor: Entity) -> Self {
        Self {
            uuid: world.get::<PlayerUuid>(actor).map(|id| id.0),
            session: world.get::<PlayerSession>(actor).map(|session| {
                (
                    session.session_id,
                    session.xuid.clone(),
                    session.uuid.clone(),
                    session.outbound_tx.clone(),
                )
            }),
        }
    }
    /// Require the same live actor and account after callbacks.
    fn matches(&self, world: &World, actor: Entity) -> bool {
        world.get_entity(actor).is_ok()
            && world.get::<PlayerUuid>(actor).map(|id| id.0) == self.uuid
            && match (&self.session, world.get::<PlayerSession>(actor)) {
                (Some((id, xuid, uuid, sender)), Some(session)) => {
                    *id == session.session_id
                        && *xuid == session.xuid
                        && *uuid == session.uuid
                        && sender.same_channel(&session.outbound_tx)
                }
                (None, None) => true,
                _ => false,
            }
    }
}

/// Authorize the actual target while rejecting callback removal/source changes.
pub(crate) fn impact(
    world: &mut World,
    projectile: Entity,
    owner: Option<Entity>,
    target: ProjectileImpactTarget,
    position: DVec3,
) -> bool {
    let before = world.get::<Position>(projectile).map(|position| position.0);
    let flight_owner = world
        .get::<super::ProjectileFlight>(projectile)
        .map(|flight| flight.owner);
    let config = world
        .get::<super::ProjectileConfig>(projectile)
        .map(|config| Arc::clone(&config.0));
    let owner_identity = owner
        .filter(|owner| world.get_entity(*owner).is_ok())
        .map(|owner| (owner, Identity::capture(world, owner)));
    let mut event = ProjectileImpact {
        projectile,
        owner,
        target,
        position,
        cancelled: false,
    };
    for handler in handlers(world) {
        handler.on_impact(world, &mut event);
    }
    world.trigger_ref(&mut event);
    // Observer Commands must finish before stale-state revalidation and costs.
    world.flush();
    !event.cancelled
        && position.is_finite()
        && world.get::<Position>(projectile).map(|position| position.0) == before
        && world.get_entity(projectile).is_ok()
        && world
            .get::<super::ProjectileFlight>(projectile)
            .map(|flight| flight.owner)
            == flight_owner
        && match (&config, world.get::<super::ProjectileConfig>(projectile)) {
            (Some(before), Some(after)) => Arc::ptr_eq(before, &after.0),
            (None, None) => true,
            _ => false,
        }
        && owner_identity.is_none_or(|(owner, identity)| identity.matches(world, owner))
        && match target {
            ProjectileImpactTarget::Block => true,
            ProjectileImpactTarget::Entity(actor) => world.get_entity(actor).is_ok(),
        }
}

/// Authorize a finite feet destination while preserving the original actor state.
pub(crate) fn teleport(
    world: &mut World,
    player: Entity,
    source: Option<Entity>,
    cause: crate::server::game::item_use::TeleportCause,
    destination: DVec3,
) -> Option<DVec3> {
    let eye = world.get::<Position>(player)?.0;
    let identity = Identity::capture(world, player);
    let mut event = PlayerTeleport {
        player,
        source,
        from: eye - DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT,
        cause,
        destination,
        cancelled: false,
    };
    for handler in handlers(world) {
        handler.on_teleport(world, &mut event);
    }
    world.trigger_ref(&mut event);
    // Observer Commands must finish before stale-state revalidation and costs.
    world.flush();
    (!event.cancelled
        && identity.matches(world, player)
        && world
            .get::<Position>(player)
            .is_some_and(|position| position.0 == eye)
        && event.destination.is_finite()
        && event.destination.as_vec3().is_finite()
        && source.is_none_or(|source| world.get_entity(source).is_ok()))
    .then_some(event.destination)
}

/// Authorize one actual effect recipient and validate adjusted protocol values.
pub(crate) fn effect(
    world: &mut World,
    target: Entity,
    source: Option<Entity>,
    effect: EffectType,
    amplifier: u8,
    duration_ticks: u32,
) -> Option<(u8, u32)> {
    let identity = Identity::capture(world, target);
    let mut event = EffectApplication {
        target,
        source,
        effect,
        amplifier,
        duration_ticks,
        cancelled: false,
    };
    for handler in handlers(world) {
        handler.on_effect(world, &mut event);
    }
    world.trigger_ref(&mut event);
    // Observer Commands must finish before stale-state revalidation and costs.
    world.flush();
    (!event.cancelled
        && identity.matches(world, target)
        && (event.duration_ticks <= i32::MAX as u32 || event.duration_ticks == u32::MAX)
        && source.is_none_or(|source| world.get_entity(source).is_ok()))
    .then_some((event.amplifier, event.duration_ticks))
}

#[cfg(test)]
mod tests;
