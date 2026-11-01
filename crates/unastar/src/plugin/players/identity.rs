//! Engine-owned actor lineage and immutable world observations for components.

use super::super::wit_types as wit;
use crate::entity::components::*;
use crate::server::game::player_events::{ConnectionIdentity, DetachedPlayer};
use bevy_ecs::prelude::*;
use bevy_ecs::world::WorldId;

/// A connection/actor lifetime, preserved through migration and never inherited on reconnect.
#[derive(Component, Clone)]
pub(crate) struct PlayerLineage {
    pub(super) id: uuid::Uuid,
    pub(super) uuid: uuid::Uuid,
    pub(super) runtime: i64,
    pub(super) identity: wit::PlayerIdentity,
    connection: Option<ConnectionIdentity>,
}
impl PlayerLineage {
    /// Account for strings cloned into a retained value, including its exact connection binding.
    pub(super) fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.identity.name.len()
            + self.identity.uuid.len()
            + self.connection.as_ref().map_or(0, |connection| {
                connection.name().len()
                    + connection.xuid().map_or(0, str::len)
                    + connection.uuid().map_or(0, str::len)
            })
    }

    /// Opaque connection/actor lifetime key for internal owned presentation state.
    pub(crate) fn key(&self) -> uuid::Uuid {
        self.id
    }
    /// Require the same engine actor and exact transport, or its retained sessionless actor.
    pub(crate) fn matches(&self, world: &World, player: Entity) -> bool {
        world.get::<Player>(player).is_some()
            && world
                .get::<PlayerLineage>(player)
                .is_some_and(|lineage| lineage.id == self.id)
            && world
                .get::<RuntimeEntityId>(player)
                .is_some_and(|runtime| runtime.0 == self.runtime)
            && world
                .get::<PlayerUuid>(player)
                .is_some_and(|uuid| uuid.0 == self.uuid)
            && world
                .get::<PlayerName>(player)
                .is_some_and(|name| name.0 == self.identity.name)
            && match (self.connection.as_ref(), world.get::<PlayerSession>(player)) {
                (Some(connection), Some(_)) => connection.matches(world, player),
                (_, None) => {
                    world.get::<DetachedPlayer>(player).is_some()
                        && xuid(crate::server::game::player_state::account_xuid(
                            world, player,
                        ))
                        .ok()
                            == Some(self.identity.authenticated_xuid)
                }
                (None, Some(_)) => false,
            }
    }
}

/// Immutable reference to one ECS world lifetime, independent of its dimension number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WorldReference {
    pub(super) id: WorldId,
    pub(super) dimension: i32,
}
impl WorldReference {
    /// Bind to the actual ECS lifetime rather than a replaceable dimension number.
    pub(super) fn of(world: &World) -> Result<Self, wit::AccessError> {
        Ok(Self {
            id: world.id(),
            dimension: world
                .get_resource::<crate::world::ChunkManager>()
                .ok_or(wit::AccessError::Unavailable)?
                .dimension(),
        })
    }
    /// Reject a closed/replaced world even if it uses the same dimension.
    pub(super) fn matches(self, world: &World) -> bool {
        Self::of(world).is_ok_and(|current| current == self)
    }
}

/// Minimal owned observation; no world or component borrow leaves capture.
#[derive(Clone)]
pub(super) struct ActorObservation {
    pub reference: PlayerLineage,
    pub world: WorldReference,
    pub snapshot: Result<wit::PlayerSnapshot, wit::AccessError>,
}

/// Global immutable directory, refreshed only outside all local ECS borrows.
#[derive(Resource, Clone, Default)]
pub(crate) struct ComponentDirectory {
    pub(super) worlds: Vec<WorldReference>,
    pub(super) actors: Vec<ActorObservation>,
}
impl ComponentDirectory {
    /// Capture connected and retained actors without granting live cross-world access.
    pub(crate) fn capture(router: &mut crate::server::worlds::DimensionRouter) -> Self {
        let mut directory = Self::default();
        for (_, server) in router.iter_mut() {
            let world = server.ecs.world_mut();
            if let Ok(reference) = WorldReference::of(world) {
                directory.worlds.push(reference);
            }
            directory.actors.extend(observe(world));
        }
        directory
    }
    /// Resolve exactly one actor lineage; duplicated engine state fails closed.
    pub(super) fn actor(
        &self,
        reference: &PlayerLineage,
    ) -> Result<&ActorObservation, wit::AccessError> {
        let mut matches = self
            .actors
            .iter()
            .filter(|actor| actor.reference.id == reference.id);
        let actor = matches.next().ok_or(wit::AccessError::StalePlayer)?;
        if matches.next().is_some() {
            return Err(wit::AccessError::InvalidIdentity);
        }
        Ok(actor)
    }
}

/// Parse only authenticated runtime account identifiers; malformed state is not downgraded.
fn xuid(value: Option<&str>) -> Result<Option<u64>, wit::AccessError> {
    value
        .map(|value| {
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(wit::AccessError::InvalidIdentity);
            }
            value
                .parse::<u64>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or(wit::AccessError::InvalidIdentity)
        })
        .transpose()
}

/// Install lineage once from the engine's validated session or owned detached account binding.
pub(crate) fn ensure_identity(
    world: &mut World,
    player: Entity,
) -> Result<PlayerLineage, wit::AccessError> {
    if let Some(lineage) = world.get::<PlayerLineage>(player) {
        return lineage
            .matches(world, player)
            .then(|| lineage.clone())
            .ok_or(wit::AccessError::StalePlayer);
    }
    world
        .get::<Player>(player)
        .ok_or(wit::AccessError::StalePlayer)?;
    let uuid = world
        .get::<PlayerUuid>(player)
        .ok_or(wit::AccessError::Unavailable)?
        .0;
    let runtime = world
        .get::<RuntimeEntityId>(player)
        .ok_or(wit::AccessError::Unavailable)?
        .0;
    let name = world
        .get::<PlayerName>(player)
        .ok_or(wit::AccessError::Unavailable)?
        .0
        .clone();
    let connection = ConnectionIdentity::capture(world, player);
    if connection.is_none() && world.get::<DetachedPlayer>(player).is_none() {
        return Err(wit::AccessError::StalePlayer);
    }
    if connection.as_ref().is_some_and(|connection| {
        connection.name() != name
            || connection
                .uuid()
                .is_some_and(|supplied| supplied.parse::<uuid::Uuid>().ok() != Some(uuid))
    }) {
        return Err(wit::AccessError::InvalidIdentity);
    }
    let identity = wit::PlayerIdentity {
        authenticated_xuid: xuid(crate::server::game::player_state::account_xuid(
            world, player,
        ))?,
        uuid: uuid.to_string(),
        name,
        connection_id: connection.as_ref().map(ConnectionIdentity::session_id),
    };
    let lineage = PlayerLineage {
        id: uuid::Uuid::new_v4(),
        uuid,
        runtime,
        identity,
        connection,
    };
    world.entity_mut(player).insert(lineage.clone());
    Ok(lineage)
}

/// Capture every current player actor while keeping invalid identities out of resource lookup.
pub(super) fn observe(world: &mut World) -> Vec<ActorObservation> {
    let Ok(reference) = WorldReference::of(world) else {
        return Vec::new();
    };
    let players: Vec<_> = world
        .query_filtered::<Entity, With<Player>>()
        .iter(world)
        .collect();
    players
        .into_iter()
        .filter_map(|entity| {
            let lineage = ensure_identity(world, entity).ok()?;
            let snapshot = snapshot(world, entity, &lineage);
            Some(ActorObservation {
                reference: lineage,
                world: reference,
                snapshot,
            })
        })
        .collect()
}

/// Locate a handle only in the current transaction's world; foreign live access is forbidden.
pub(crate) fn resolve(
    world: &mut World,
    reference: &PlayerLineage,
) -> Result<Entity, wit::AccessError> {
    if world
        .get_resource::<ComponentDirectory>()
        .is_some_and(|directory| {
            matches!(
                directory.actor(reference),
                Err(wit::AccessError::InvalidIdentity)
            )
        })
    {
        return Err(wit::AccessError::InvalidIdentity);
    }
    let entities: Vec<_> = world
        .query_filtered::<Entity, With<PlayerLineage>>()
        .iter(world)
        .filter(|entity| {
            world
                .get::<PlayerLineage>(*entity)
                .is_some_and(|lineage| lineage.id == reference.id)
        })
        .collect();
    if entities.len() == 1 && reference.matches(world, entities[0]) {
        return Ok(entities[0]);
    }
    if entities.len() > 1 {
        return Err(wit::AccessError::InvalidIdentity);
    }
    if world
        .get_resource::<ComponentDirectory>()
        .is_some_and(|directory| directory.actor(reference).is_ok())
    {
        return Err(wit::AccessError::WrongWorld);
    }
    Err(wit::AccessError::StalePlayer)
}

/// Explicitly map semantic modes; effective transient modes do not overwrite durable bases.
pub(super) fn mode(mode: GameMode) -> wit::GameMode {
    match mode {
        GameMode::Survival => wit::GameMode::Survival,
        GameMode::Creative => wit::GameMode::Creative,
        GameMode::Adventure => wit::GameMode::Adventure,
        GameMode::Spectator => wit::GameMode::Spectator,
    }
}

/// Read native availability independently of any plugin's game rules.
pub(super) fn availability(world: &World, player: Entity) -> wit::PlayerAvailability {
    use crate::server::game::{inventory_effects, player_events};
    if inventory_effects::is_recovery_fenced(world, player) {
        wit::PlayerAvailability::Recovering
    } else if inventory_effects::is_quarantined(world, player) {
        wit::PlayerAvailability::Quarantined
    } else if player_events::initial_state_pending(world, player) {
        wit::PlayerAvailability::Initializing
    } else if crate::server::worlds::is_loading(world, player) {
        wit::PlayerAvailability::Loading
    } else if world
        .get::<crate::entity::player_life::Dead>(player)
        .is_some()
        || world
            .get::<Health>(player)
            .is_some_and(|health| health.is_dead())
    {
        wit::PlayerAvailability::Dead
    } else if world.get::<DetachedPlayer>(player).is_some() {
        wit::PlayerAvailability::Retained
    } else if world.get::<Position>(player).is_none()
        || world.get::<Health>(player).is_none()
        || world.get::<Rotation>(player).is_none()
        || world.get::<PlayerState>(player).is_none()
        || world.get::<GameMode>(player).is_none()
    {
        wit::PlayerAvailability::Incomplete
    } else {
        wit::PlayerAvailability::Ready
    }
}

/// Complete factual observations fail instead of inventing health, position or movement values.
pub(super) fn snapshot(
    world: &World,
    player: Entity,
    lineage: &PlayerLineage,
) -> Result<wit::PlayerSnapshot, wit::AccessError> {
    if !lineage.matches(world, player) {
        return Err(wit::AccessError::StalePlayer);
    }
    let position = world
        .get::<Position>(player)
        .ok_or(wit::AccessError::Unavailable)?
        .0;
    let rotation = world
        .get::<Rotation>(player)
        .ok_or(wit::AccessError::Unavailable)?;
    let health = world
        .get::<Health>(player)
        .ok_or(wit::AccessError::Unavailable)?;
    let state = world
        .get::<PlayerState>(player)
        .ok_or(wit::AccessError::Unavailable)?;
    if !position.is_finite()
        || !rotation.yaw.is_finite()
        || !rotation.pitch.is_finite()
        || !health.current.is_finite()
        || !health.max.is_finite()
        || health.max <= 0.0
        || health.current < 0.0
        || health.current > health.max
    {
        return Err(wit::AccessError::Unavailable);
    }
    let effective = *world
        .get::<GameMode>(player)
        .ok_or(wit::AccessError::Unavailable)?;
    let base =
        crate::command::exec::base_game_mode(world, player).ok_or(wit::AccessError::Unavailable)?;
    let configured_admin = world
        .get_resource::<crate::server::game::types::ServerConfigResource>()
        .is_some_and(|config| {
            config.0.operators.iter().any(|entry| {
                !entry.is_empty()
                    && (entry.eq_ignore_ascii_case(&lineage.identity.name)
                        || lineage
                            .identity
                            .authenticated_xuid
                            .is_some_and(|xuid| *entry == xuid.to_string()))
            })
        });
    let flags = wit::PlayerFlags {
        on_ground: world
            .get::<PlayerInput>(player)
            .map(|input| input.on_ground),
        sneaking: state.sneaking,
        sprinting: state.sprinting,
        swimming: state.swimming,
        gliding: state.gliding,
        flying: state.flying,
        invisible: world.get::<Effects>(player).is_some_and(|effects| {
            effects
                .get(EffectType::INVISIBILITY)
                .is_some_and(|effect| !effect.is_expired())
        }),
        burning: world
            .get::<crate::entity::player_life::environment::Burning>(player)
            .is_some()
            || world
                .get::<OnFire>(player)
                .is_some_and(|fire| fire.is_on_fire()),
        dead: world
            .get::<crate::entity::player_life::Dead>(player)
            .is_some()
            || health.is_dead(),
    };
    Ok(wit::PlayerSnapshot {
        identity: lineage.identity.clone(),
        dimension: WorldReference::of(world)?.dimension,
        position: wit::Vec3 {
            x: position.x,
            y: position.y - crate::entity::PLAYER_EYE_HEIGHT,
            z: position.z,
        },
        rotation: (rotation.yaw, rotation.pitch),
        health: health.current,
        max_health: health.max,
        game_mode: mode(base),
        effective_game_mode: mode(effective),
        flags,
        availability: availability(world, player),
        configured_admin,
        captured_tick: world
            .get_resource::<crate::ecs::resources::TickCounter>()
            .map_or(0, |tick| tick.current),
    })
}
