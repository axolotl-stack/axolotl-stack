//! Native authorization for actual projectile impacts, teleports and effects.
//!
//! Handlers run from snapshots without holding the registry borrowed. Vetoed
//! impacts discard the projectile without its normal impact effects. Teleport
//! destinations and individual effect recipients are checked at their actual
//! application boundary, rather than authorizing only the original item click.

use crate::entity::components::{
    EffectType, GameMode, Health, PlayerSession, PlayerUuid, Position,
};
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
    request_id: Option<String>,
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
    /// Read native queued relocation context; ordinary item teleports have none.
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
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

/// Immutable account facts retained for accepted teleport accounting.
#[derive(Clone, Debug)]
pub struct TeleportIdentity {
    session_id: Option<u64>,
    uuid: Option<uuid::Uuid>,
    xuid: Option<String>,
}
impl TeleportIdentity {
    /// Read the original transport identifier, when connected.
    pub fn session_id(&self) -> Option<u64> {
        self.session_id
    }
    /// Read the persistent actor UUID, when present.
    pub fn uuid(&self) -> Option<uuid::Uuid> {
        self.uuid
    }
    /// Read the source account identifier supplied by its connection.
    pub fn xuid(&self) -> Option<&str> {
        self.xuid.as_deref()
    }
}

/// An accepted teleport after destination validation and authoritative movement.
#[derive(Event, Clone, Debug)]
pub struct PlayerTeleported {
    player: Entity,
    source: Option<Entity>,
    from: DVec3,
    destination: DVec3,
    cause: crate::server::game::item_use::TeleportCause,
    request_id: Option<String>,
    identity: TeleportIdentity,
}
impl PlayerTeleported {
    /// Read the actor that actually moved.
    pub fn player(&self) -> Entity {
        self.player
    }
    /// Read its original applying actor, if any.
    pub fn source(&self) -> Option<Entity> {
        self.source
    }
    /// Read the actual original feet position.
    pub fn from(&self) -> DVec3 {
        self.from
    }
    /// Read the final accepted feet position, including native rewrites.
    pub fn destination(&self) -> DVec3 {
        self.destination
    }
    /// Read the protocol teleport reason.
    pub fn cause(&self) -> crate::server::game::item_use::TeleportCause {
        self.cause
    }
    /// Read the original native queued relocation context, when present.
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }
    /// Read immutable account facts even if another notification removed the actor.
    pub fn identity(&self) -> &TeleportIdentity {
        &self.identity
    }
}

/// Accepted teleport facts retained until the caller actually updates the actor.
pub(crate) struct TeleportPermit {
    pub destination: DVec3,
    committed: PlayerTeleported,
}
impl TeleportPermit {
    /// Notify native handlers and observers after authoritative position commit.
    pub(crate) fn commit(self, world: &mut World) {
        for handler in handlers(world) {
            handler.on_teleported(world, &self.committed);
        }
        world.trigger(self.committed);
        world.flush();
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

/// Pending removal of fire from one actual water-impact recipient.
#[derive(Event, Clone, Debug)]
pub struct ProjectileExtinguish {
    source: Entity,
    target: Entity,
    cancelled: bool,
}
impl ProjectileExtinguish {
    /// Read the water projectile producing this effect.
    pub fn source(&self) -> Entity {
        self.source
    }
    /// Read the affected actor.
    pub fn target(&self) -> Entity {
        self.target
    }
    /// Keep this actor's current burning state.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
    /// Whether a callback rejected this recipient.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// One ordinary projectile block mutation, authorized at the affected block.
#[derive(Event, Clone, Debug)]
pub struct ProjectileBlockChange {
    source: Entity,
    owner: Option<Entity>,
    position: glam::IVec3,
    old_state: u32,
    new_state: u32,
    cancelled: bool,
}
impl ProjectileBlockChange {
    /// Read the projectile producing this block effect.
    pub fn source(&self) -> Entity {
        self.source
    }
    /// Read its current owner.
    pub fn owner(&self) -> Option<Entity> {
        self.owner
    }
    /// Read the actual affected block position.
    pub fn position(&self) -> glam::IVec3 {
        self.position
    }
    /// Read the block state being replaced.
    pub fn old_state(&self) -> u32 {
        self.old_state
    }
    /// Read the proposed replacement block state.
    pub fn new_state(&self) -> u32 {
        self.new_state
    }
    /// Veto this affected block without changing other independently accepted blocks.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
    /// Whether any native callback rejected this mutation.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// Native callbacks for projectile and effect application boundaries.
pub trait ProjectileHandler: Send + Sync + 'static {
    /// Authorize removal of fire from an actual water-impact recipient.
    fn on_extinguish(&self, _world: &mut World, _event: &mut ProjectileExtinguish) {}
    /// Authorize an actual affected block, such as water extinguishing fire.
    fn on_block_change(&self, _world: &mut World, _event: &mut ProjectileBlockChange) {}
    /// Authorize an actual collision before its ordinary effects.
    fn on_impact(&self, _world: &mut World, _event: &mut ProjectileImpact) {}
    /// Authorize or redirect a destination before the player moves.
    fn on_teleport(&self, _world: &mut World, _event: &mut PlayerTeleport) {}
    /// Observe an accepted teleport; attempts and late vetoes produce no notification.
    fn on_teleported(&self, _world: &mut World, _event: &PlayerTeleported) {}
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
pub(crate) struct Identity {
    position: Option<DVec3>,
    health: Option<(f32, f32)>,
    mode: Option<GameMode>,
    uuid: Option<uuid::Uuid>,
    session: Option<SessionSnapshot>,
}
impl Identity {
    /// Snapshot identity for callback revalidation.
    pub(crate) fn capture(world: &World, actor: Entity) -> Self {
        Self {
            position: world.get::<Position>(actor).map(|position| position.0),
            health: world
                .get::<Health>(actor)
                .map(|health| (health.current, health.max)),
            mode: world.get::<GameMode>(actor).copied(),
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
    /// Require the same live account and transport after a committed notification.
    pub(crate) fn matches_account(&self, world: &World, actor: Entity) -> bool {
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
    /// Require the same actor, account and mutable state after authorization.
    fn matches(&self, world: &World, actor: Entity) -> bool {
        self.matches_account(world, actor)
            && world.get::<Position>(actor).map(|position| position.0) == self.position
            && world
                .get::<Health>(actor)
                .map(|health| (health.current, health.max))
                == self.health
            && world.get::<GameMode>(actor).copied() == self.mode
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
) -> Option<TeleportPermit> {
    teleport_with_request(world, player, source, cause, destination, None)
}

/// Run the ordinary native teleport policy with an immutable engine-owned context.
pub(crate) fn teleport_with_request(
    world: &mut World,
    player: Entity,
    source: Option<Entity>,
    cause: crate::server::game::item_use::TeleportCause,
    destination: DVec3,
    request_id: Option<String>,
) -> Option<TeleportPermit> {
    let eye = world.get::<Position>(player)?.0;
    let source_identity = source.map(|source| (source, Identity::capture(world, source)));
    let flight = source.map(|source| world.get::<super::ProjectileFlight>(source).copied());
    let identity = Identity::capture(world, player);
    let mut event = PlayerTeleport {
        player,
        source,
        from: eye - DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT,
        cause,
        request_id: request_id.clone(),
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
        && source_identity.is_none_or(|(source, identity)| identity.matches(world, source))
        && source.map(|source| world.get::<super::ProjectileFlight>(source).copied()) == flight)
        .then_some(TeleportPermit {
            destination: event.destination,
            committed: PlayerTeleported {
                player,
                source,
                from: event.from,
                destination: event.destination,
                cause,
                request_id,
                identity: TeleportIdentity {
                    session_id: identity.session.as_ref().map(|session| session.0),
                    uuid: identity.uuid,
                    xuid: identity
                        .session
                        .as_ref()
                        .and_then(|session| session.1.clone()),
                },
            },
        })
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
    if source.is_some_and(|source| world.get_entity(source).is_err()) {
        return None;
    }
    let identity = Identity::capture(world, target);
    let source_identity = source.map(|source| (source, Identity::capture(world, source)));
    let flight = source.map(|source| world.get::<super::ProjectileFlight>(source).copied());
    let owner_identity = flight
        .flatten()
        .and_then(|flight| flight.owner)
        .filter(|owner| world.get_entity(*owner).is_ok())
        .map(|owner| (owner, Identity::capture(world, owner)));
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
        && source_identity.is_none_or(|(source, identity)| identity.matches(world, source))
        && owner_identity.is_none_or(|(owner, identity)| identity.matches(world, owner))
        && source.map(|source| world.get::<super::ProjectileFlight>(source).copied()) == flight)
        .then_some((event.amplifier, event.duration_ticks))
}

/// Authorize removal of fire without permitting stale actor/source state.
pub(crate) fn extinguish(world: &mut World, source: Entity, target: Entity) -> bool {
    let before = Identity::capture(world, target);
    let origin = Identity::capture(world, source);
    let flight = world.get::<super::ProjectileFlight>(source).copied();
    let owner_identity = flight
        .and_then(|flight| flight.owner)
        .filter(|owner| world.get_entity(*owner).is_ok())
        .map(|owner| (owner, Identity::capture(world, owner)));
    let mut event = ProjectileExtinguish {
        source,
        target,
        cancelled: false,
    };
    for handler in handlers(world) {
        handler.on_extinguish(world, &mut event);
    }
    world.trigger_ref(&mut event);
    world.flush();
    !event.cancelled
        && before.matches(world, target)
        && origin.matches(world, source)
        && world.get::<super::ProjectileFlight>(source).copied() == flight
        && owner_identity.is_none_or(|(owner, identity)| identity.matches(world, owner))
}

/// Authorize one affected block while rejecting callback replacement/removal.
pub(crate) fn block_change(
    world: &mut World,
    source: Entity,
    position: glam::IVec3,
    new_state: u32,
) -> bool {
    use crate::entity::systems::physics::BlockSource;
    let Some(old_state) = world.block_at(position) else {
        return false;
    };
    let identity = Identity::capture(world, source);
    let owner = world
        .get::<super::ProjectileFlight>(source)
        .and_then(|flight| flight.owner);
    let owner_identity = owner
        .filter(|owner| world.get_entity(*owner).is_ok())
        .map(|owner| (owner, Identity::capture(world, owner)));
    let mut event = ProjectileBlockChange {
        source,
        owner,
        position,
        old_state,
        new_state,
        cancelled: false,
    };
    for handler in handlers(world) {
        handler.on_block_change(world, &mut event);
    }
    world.trigger_ref(&mut event);
    world.flush();
    !event.cancelled
        && identity.matches(world, source)
        && world.block_at(position) == Some(old_state)
        && world
            .get::<super::ProjectileFlight>(source)
            .and_then(|flight| flight.owner)
            == owner
        && owner_identity.is_none_or(|(owner, identity)| identity.matches(world, owner))
}

#[cfg(test)]
mod tests;
