//! Behaviour-pack driven mobs (living actors such as the cow).
//!
//! A mob is an entity carrying [`MobKind`] (its compiled [`MobArchetype`]),
//! a definition stack, and the derived runtime state the rest of the server
//! already understands: `PhysicsBody` for block physics, `Health`,
//! `EntityMetadata`/`ActorFlags`/`ActorProperties`/`ActorAttributes` for the
//! client. [`spawn_mob`] is the single entry point.
//!
//! Layout:
//! - [`archetype`]: per-identifier archetypes compiled once at startup;
//!   [`derive`]: the derived body/flags/metadata/attributes; [`events`]:
//!   firing authored events; [`state`]: runtime state shared between the
//!   subsystems; [`constants`] and [`native`]: native behaviour the packs do
//!   not describe; [`persistence`]: saving and restoring mobs.
//! - [`components`]: the behaviour-pack component kinds, one file per family.
//! - [`ai`], [`combat`], [`life`] (ageing, breeding, egg laying, despawn),
//!   [`interaction`] and [`net`] (replication and sounds): the systems.
//!
//! Tick order (see `server::game`): the definition apply barrier, then
//! [`derive_mob_state`] (re-derives body/flags/metadata/attributes when the
//! composition or health changed), then physics, AI and combat, then
//! `net::network::sync_mob_viewers`.

pub mod ai;
pub mod archetype;
pub mod combat;
pub mod components;
pub mod constants;
pub mod derive;
pub mod events;
pub mod gear;
pub mod interaction;
pub mod life;
pub mod native;
pub mod net;
pub mod persistence;
pub mod state;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_flags;
use thiserror::Error;

pub use archetype::{ArchetypeError, MobArchetype, MobRegistry, MobRegistryError};
pub(crate) use derive::derive_entity;
pub use derive::derive_mob_state;
pub use events::EventError;

use crate::entity::components::{
    ActorViewers, Age, FallDistance, Living, Mob, OnGround, PhysicsBody, Position, Rotation,
    RuntimeId, Velocity,
};
use crate::entity::definition::attributes::ActorAttributes;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::definition::properties::{ActorProperties, PropertyError};
use crate::entity::definition::{apply_pending_definitions, attach_definition};
use crate::entity::item_drop::next_actor_id;

/// Why [`spawn_mob`] could not create a mob.
#[derive(Debug, Error)]
pub enum MobSpawnError {
    #[error("mob registry not initialised")]
    NoRegistry,
    #[error("unknown actor {0}")]
    UnknownActor(String),
    #[error("{identifier} has no event {event}")]
    UnknownSpawnEvent { identifier: String, event: String },
    #[error("{identifier} is not supported yet: {reason}")]
    Unsupported {
        identifier: String,
        #[source]
        reason: Arc<ArchetypeError>,
    },
    /// A property default expression failed to evaluate.
    #[error(transparent)]
    Properties(#[from] PropertyError),
    /// The lifecycle event failed; the mob was removed again.
    #[error(transparent)]
    Event(#[from] EventError),
}

/// The compiled archetype of a mob entity.
#[derive(Component, Clone)]
pub struct MobKind(pub Arc<MobArchetype>);

/// Why a mob is being created; selects the lifecycle event fired on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnCause {
    /// `/summon`, spawn eggs and natural spawning: `minecraft:entity_spawned`.
    Spawned,
    /// Breeding: `minecraft:entity_born`.
    Born,
    /// Restored from storage: no event; the saved groups are re-applied.
    Loaded,
}

/// Create a mob of `identifier` with its feet at `position`, fire its
/// lifecycle event and materialise the resulting composition, so the entity
/// is complete (body, health, metadata) before the next network sync.
pub fn spawn_mob(
    world: &mut World,
    identifier: &str,
    position: DVec3,
    yaw: f32,
    cause: SpawnCause,
) -> Result<Entity, MobSpawnError> {
    spawn(world, identifier, position, yaw, cause, None)
}

/// [`spawn_mob`] for `/summon` with a spawn event: `event` is fired instead
/// of `minecraft:entity_spawned`. An event the actor does not define spawns
/// nothing.
pub fn spawn_mob_with_event(
    world: &mut World,
    identifier: &str,
    position: DVec3,
    yaw: f32,
    event: &str,
) -> Result<Entity, MobSpawnError> {
    spawn(
        world,
        identifier,
        position,
        yaw,
        SpawnCause::Spawned,
        Some(event),
    )
}

fn spawn(
    world: &mut World,
    identifier: &str,
    position: DVec3,
    yaw: f32,
    cause: SpawnCause,
    spawn_event: Option<&str>,
) -> Result<Entity, MobSpawnError> {
    let registry = world
        .get_resource::<MobRegistry>()
        .ok_or(MobSpawnError::NoRegistry)?;
    let archetype = match registry.get(identifier) {
        Some(archetype) => Arc::clone(archetype),
        None => {
            return Err(match registry.unsupported(identifier) {
                Some(reason) => MobSpawnError::Unsupported {
                    identifier: identifier.to_owned(),
                    reason: Arc::clone(reason),
                },
                None => MobSpawnError::UnknownActor(identifier.to_owned()),
            });
        }
    };
    let custom_event = spawn_event
        .map(|name| {
            archetype
                .events
                .event(name)
                .ok_or_else(|| MobSpawnError::UnknownSpawnEvent {
                    identifier: identifier.to_owned(),
                    event: name.to_owned(),
                })
        })
        .transpose()?;
    let entity = spawn_unattached(world, &archetype, position, yaw)?;
    if cause == SpawnCause::Loaded {
        world.entity_mut(entity).insert(persistence::Restoring);
    }
    attach_definition(world, entity, Arc::clone(&archetype.definition))
        .expect("a freshly spawned mob exists and has no definition stack");
    apply_pending_definitions(world);
    let event = match cause {
        _ if custom_event.is_some() => custom_event.as_ref(),
        SpawnCause::Spawned => archetype.lifecycle.spawned.as_ref(),
        SpawnCause::Born => archetype.lifecycle.born.as_ref(),
        SpawnCause::Loaded => None,
    };
    if let Some(event) = event
        && let Err(error) = events::fire_event(world, entity, event, None)
    {
        world.despawn(entity);
        return Err(error.into());
    }
    apply_pending_definitions(world);
    derive_entity(world, entity);
    Ok(entity)
}

/// The archetype-independent part of a mob: identity, transform, wire state.
fn spawn_unattached(
    world: &mut World,
    archetype: &Arc<MobArchetype>,
    position: DVec3,
    yaw: f32,
) -> Result<Entity, PropertyError> {
    let runtime_id = next_actor_id(world);
    let seed = {
        use rand::Rng;
        world
            .resource_mut::<crate::entity::EntityRandom>()
            .0
            .r#gen::<u64>()
    };
    let mut molang = PropertyDefaults(rand::SeedableRng::seed_from_u64(seed));
    let properties = ActorProperties::new(Arc::clone(&archetype.properties), &mut molang)?;
    let attributes = ActorAttributes::new(
        Arc::clone(&archetype.attributes),
        archetype.attribute_seed.clone(),
    )
    .expect("an archetype's attribute seed is built with its layout");
    let mut flags = ActorFlags::default();
    if archetype.legacy_friction {
        flags
            .update(actor_flags::USES_LEGACY_FRICTION, true)
            .expect("catalogue flag index");
    }
    let metadata = if archetype.projectile {
        // Projectiles are spawned invisible (shown from their second tick).
        flags
            .update(actor_flags::INVISIBLE, true)
            .expect("catalogue flag index");
        EntityMetadata::actor_base()
    } else {
        let mut metadata = EntityMetadata::living_base();
        metadata.set_float(
            jolyne::valentine::actor_data_i_ds::AMBIENT_SOUND_INTERVAL,
            archetype.native.ambient_sound_interval,
        );
        if let Some(kind) = archetype.native.aux_value {
            metadata.set_short(jolyne::valentine::actor_data_i_ds::AUX_VALUE_DATA, kind);
        }
        // Enderman lane: the native carried-block data, always air.
        if archetype.native.carried_block_data
            && let Some(air) =
                crate::registry::BlockRegistry::vanilla().default_state_id_by_name("minecraft:air")
        {
            let ids = world
                .get_resource::<crate::server::game::types::ServerConfigResource>()
                .map_or_else(Default::default, |config| config.0.block_network_ids);
            metadata.set_int(
                jolyne::valentine::actor_data_i_ds::DEPRECATED_CARRY_BLOCK_RUNTIME_ID,
                ids.to_wire(air) as i32,
            );
        }
        metadata
    };
    let launched_tick = world
        .get_resource::<crate::ecs::resources::TickCounter>()
        .map_or(0, |tick| tick.current);
    let mut entity = world.spawn((
        (
            MobKind(Arc::clone(archetype)),
            Position(position),
            Rotation::new(yaw, 0.0),
            Velocity::default(),
            OnGround(false),
            Age::default(),
            RuntimeId(runtime_id),
            ActorViewers::default(),
            net::network::MobNetState::default(),
        ),
        (metadata, flags, properties, attributes),
    ));
    if archetype.projectile {
        // Projectiles fly by their own rules (`entity::projectile`), not the living
        // physics step, and have no brain.
        entity.insert((
            crate::entity::components::Projectile,
            crate::entity::projectile::ProjectileFlight {
                owner: None,
                launched_tick,
                critical: false,
            },
        ));
    } else {
        entity.insert((
            Mob,
            Living,
            // Replaced by `derive_mob_state` once the composition applies.
            PhysicsBody::living(0.0, 0.0),
            FallDistance::default(),
            crate::entity::components::Climbing::default(),
            ai::MobBrain::default(),
        ));
    }
    Ok(entity.id())
}

/// Molang context for property default expressions: random only.
struct PropertyDefaults(rand::rngs::StdRng);

impl unastar_rules::molang::eval::MolangContext for PropertyDefaults {
    fn query(
        &mut self,
        _: unastar_rules::molang::QueryId,
        _: &[unastar_rules::molang::eval::Value],
    ) -> Option<unastar_rules::molang::eval::Value> {
        None
    }
    fn variable(&self, _: &str) -> Option<unastar_rules::molang::eval::Value> {
        None
    }
    fn set_variable(&mut self, _: &str, _: unastar_rules::molang::eval::Value) {}
    fn context(&self, _: &str) -> Option<unastar_rules::molang::eval::Value> {
        None
    }
    fn this(&self) -> unastar_rules::molang::eval::Value {
        unastar_rules::molang::eval::Value::Num(0.0)
    }
    fn enter(&mut self, _: u64) -> bool {
        false
    }
    fn leave(&mut self) {}
    fn random(&mut self) -> f32 {
        use rand::Rng;
        self.0.r#gen()
    }
}
