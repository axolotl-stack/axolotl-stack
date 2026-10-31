//! Immutable, token-free damage origins. Entity handles remain scoped to their
//! captured WorldId; they are descriptive and never authorize a replacement actor.
//!
//! The native runtime only supplies XUIDs after authenticated Login and key
//! proof. Trusted embedding applications may construct PlayerSession themselves;
//! they own that ingress boundary and must never put guest claims in its XUID.

use bevy_ecs::prelude::*;
use bevy_ecs::world::WorldId;
use uuid::Uuid;

use super::components::{Player, PlayerSession, PlayerUuid};

/// Stable account facts from a correctly bound, authenticated native session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AuthenticatedDamageOwner {
    uuid: Uuid,
    xuid: u64,
}
impl AuthenticatedDamageOwner {
    /// Persistent UUID bound to the launch session.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }
    /// Canonical, nonzero Xbox account number supplied by trusted native ingress.
    pub fn xuid(&self) -> u64 {
        self.xuid
    }
}

/// Original owner facts, retained even when that actor migrates or disconnects.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DamageOwner {
    entity: Entity,
    world: WorldId,
    dimension: Option<i32>,
    session: Option<u64>,
    player: bool,
    uuid: Option<Uuid>,
    authenticated: Option<AuthenticatedDamageOwner>,
    feet: Option<glam::DVec3>,
}
impl DamageOwner {
    /// Original actor handle; resolve it only in the captured world.
    pub fn entity(&self) -> Entity {
        self.entity
    }
    /// Runtime world instance containing the original actor.
    pub fn world_id(&self) -> WorldId {
        self.world
    }
    /// Configured dimension of that world, when available.
    pub fn dimension(&self) -> Option<i32> {
        self.dimension
    }
    /// Original connection identifier, descriptive even after the connection ends.
    pub fn session_id(&self) -> Option<u64> {
        self.session
    }
    /// Whether this was a player, independently of account authentication.
    pub fn is_player(&self) -> bool {
        self.player
    }
    /// Descriptive actor UUID; guests do not gain authenticated account status.
    pub fn player_uuid(&self) -> Option<Uuid> {
        self.uuid
    }
    /// Owner feet at the launch/deflection boundary, independent of later movement.
    pub fn feet_position(&self) -> Option<glam::DVec3> {
        self.feet
    }
    /// Read only correctly bound authenticated account facts.
    pub fn authenticated(&self) -> Option<AuthenticatedDamageOwner> {
        self.authenticated
    }
}

/// Origin of a hit/effect, independent of its currently live attacker handle.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct DamageAttribution {
    source: Option<Entity>,
    world: WorldId,
    dimension: Option<i32>,
    owner: Option<DamageOwner>,
}
impl DamageAttribution {
    /// Capture actual source/owner state at a trusted native launch boundary.
    /// A UUID alone, malformed XUID or mismatched session UUID never authenticates.
    pub fn capture(world: &World, source: Option<Entity>, owner: Option<Entity>) -> Self {
        let dimension = world
            .get_resource::<crate::world::ChunkManager>()
            .map(|chunks| chunks.dimension());
        let owner = owner.map(|entity| {
            let player = world.get::<Player>(entity).is_some();
            let uuid = world
                .get::<PlayerUuid>(entity)
                .map(|uuid| uuid.0)
                .filter(|uuid| !uuid.is_nil());
            let session = world.get::<PlayerSession>(entity);
            let authenticated = (|| {
                if !player {
                    return None;
                }
                let uuid = uuid?;
                let session = session?;
                if session
                    .uuid
                    .as_deref()
                    .and_then(|value| Uuid::parse_str(value).ok())
                    != Some(uuid)
                {
                    return None;
                }
                let raw = session.xuid.as_deref()?;
                if raw.starts_with('0') || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
                    return None;
                }
                let xuid = raw.parse::<u64>().ok().filter(|xuid| *xuid != 0)?;
                Some(AuthenticatedDamageOwner { uuid, xuid })
            })();
            DamageOwner {
                entity,
                world: world.id(),
                dimension,
                session: session.map(|session| session.session_id),
                player,
                uuid,
                authenticated,
                feet: super::geometry::feet_position(world, entity)
                    .filter(|position| position.is_finite() && position.as_vec3().is_finite()),
            }
        });
        Self {
            source,
            world: world.id(),
            dimension,
            owner,
        }
    }
    /// Actual actor that produced this hit/effect in the captured world.
    pub fn source(&self) -> Option<Entity> {
        self.source
    }
    /// Runtime world that scopes the source entity.
    pub fn world_id(&self) -> WorldId {
        self.world
    }
    /// Configured source dimension, when available.
    pub fn dimension(&self) -> Option<i32> {
        self.dimension
    }
    /// Immutable launch/deflection owner facts, including descriptive guest state.
    pub fn owner(&self) -> Option<&DamageOwner> {
        self.owner.as_ref()
    }
    /// Stable account attribution without resolving a stale source-world entity.
    pub fn authenticated_owner(&self) -> Option<AuthenticatedDamageOwner> {
        self.owner.and_then(|owner| owner.authenticated)
    }
    /// Resolve only a surviving owner in this exact world. This never searches
    /// other worlds or substitutes a reconnected account's new actor.
    pub fn live_owner(&self, world: &World) -> Option<Entity> {
        let owner = self.owner?;
        if owner.world != world.id() || world.get_entity(owner.entity).is_err() {
            return None;
        }
        let current = Self::capture(world, Some(owner.entity), Some(owner.entity)).owner?;
        (current.session == owner.session
            && current.uuid == owner.uuid
            && current.authenticated == owner.authenticated
            && current.player == owner.player)
            .then_some(owner.entity)
    }
    /// Retain original ownership while a new actor (such as a cloud) applies it.
    pub(crate) fn with_source(self, world: &World, source: Entity) -> Self {
        Self {
            source: Some(source),
            world: world.id(),
            dimension: world
                .get_resource::<crate::world::ChunkManager>()
                .map(|chunks| chunks.dimension()),
            owner: self.owner,
        }
    }
    /// Resolve captured projectile/cloud state; ordinary actors are their own source.
    pub fn for_source(world: &World, source: Option<Entity>) -> Self {
        source
            .and_then(|source| world.get::<Self>(source).copied())
            .unwrap_or_else(|| {
                let owner = source.and_then(|source| {
                    world
                        .get::<super::projectile::ProjectileFlight>(source)
                        .map(|flight| flight.owner)
                        .or_else(|| {
                            world
                                .get::<super::projectile::cloud::AreaEffectCloud>(source)
                                .map(|cloud| cloud.owner())
                        })
                        .unwrap_or(Some(source))
                });
                Self::capture(world, source, owner)
            })
    }
}

#[cfg(test)]
mod tests;
