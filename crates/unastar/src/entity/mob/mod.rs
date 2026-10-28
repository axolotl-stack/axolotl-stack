//! Behaviour-pack driven mobs (living actors such as the cow).
//!
//! A mob is an entity carrying [`MobKind`] (its compiled [`MobArchetype`]),
//! a definition stack, and the derived runtime state the rest of the server
//! already understands: [`PhysicsBody`] for block physics, [`Health`],
//! [`EntityMetadata`]/[`ActorFlags`]/[`ActorProperties`]/[`ActorAttributes`]
//! for the client. [`spawn_mob`] is the single entry point.
//!
//! Tick order (see `server::game`): the definition apply barrier, then
//! [`derive_mob_state`] (re-derives body/flags/metadata/attributes when the
//! composition or health changed), then physics, AI and combat, then
//! `network::sync_mob_viewers`.

pub mod archetype;
pub mod config;
pub mod constants;
pub mod events;
pub mod network;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_flags;

pub use archetype::{MobArchetype, MobRegistry};

use crate::entity::attributes::{ActorAttributes, AttributeValue};
use crate::entity::components::{
    ActorViewers, Age, AirSupply, Health, Living, Mob, OnGround, PhysicsBody, Position, Rotation,
    RuntimeId, Velocity,
};
use crate::entity::definition::{apply_pending_definitions, attach_definition};
use crate::entity::definition_components::{
    MovementConfig, PhysicsConfig, RawCollisionBox, ScaleConfig,
};
use crate::entity::flags::ActorFlags;
use crate::entity::item_drop::next_actor_id;
use crate::entity::metadata::EntityMetadata;
use crate::entity::properties::ActorProperties;
use config::WalkNavigationConfig;

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
) -> Result<Entity, String> {
    let registry = world
        .get_resource::<MobRegistry>()
        .ok_or_else(|| "mob registry not initialised".to_owned())?;
    let archetype = match registry.get(identifier) {
        Some(archetype) => Arc::clone(archetype),
        None => {
            return Err(match registry.unsupported_reason(identifier) {
                Some(reason) => format!("{identifier} is not supported yet: {reason}"),
                None => format!("unknown actor {identifier}"),
            });
        }
    };
    let entity = spawn_unattached(world, &archetype, position, yaw)?;
    attach_definition(world, entity, Arc::clone(&archetype.definition))?;
    apply_pending_definitions(world);
    let event = match cause {
        SpawnCause::Spawned => archetype.lifecycle.spawned.as_ref(),
        SpawnCause::Born => archetype.lifecycle.born.as_ref(),
        SpawnCause::Loaded => None,
    };
    if let Some(event) = event
        && let Err(error) = events::fire_event(world, entity, event, None)
    {
        world.despawn(entity);
        return Err(error);
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
) -> Result<Entity, String> {
    let runtime_id = next_actor_id(world);
    let seed = {
        use rand::Rng;
        world
            .resource_mut::<crate::entity::EntityRandom>()
            .0
            .r#gen::<u64>()
    };
    let mut molang = PropertyDefaults(rand::SeedableRng::seed_from_u64(seed));
    let properties = ActorProperties::new(Arc::clone(&archetype.properties), &mut molang)
        .map_err(|e| e.to_string())?;
    let attributes = ActorAttributes::new(
        Arc::clone(&archetype.attributes),
        archetype.attribute_seed.clone(),
    )
    .map_err(|e| e.to_string())?;
    let mut flags = ActorFlags::default();
    if archetype.legacy_friction {
        flags
            .update(actor_flags::USES_LEGACY_FRICTION, true)
            .expect("catalogue flag index");
    }
    Ok(world
        .spawn((
            (
                Mob,
                MobKind(Arc::clone(archetype)),
                Living,
                Position(position),
                Rotation::new(yaw, 0.0),
                Velocity::default(),
                OnGround(false),
                Age::default(),
                RuntimeId(runtime_id),
                ActorViewers::default(),
                network::MobNetState::default(),
            ),
            (
                // Replaced by `derive_mob_state` once the composition applies.
                PhysicsBody::living(0.0, 0.0),
                EntityMetadata::living_base(),
                flags,
                properties,
                attributes,
            ),
        ))
        .id())
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

/// Inputs of the derived state, gathered from the current composition.
#[allow(clippy::type_complexity)]
type DeriveInputs<'a> = (
    &'a MobKind,
    Option<&'a RawCollisionBox>,
    Option<&'a ScaleConfig>,
    Option<&'a PhysicsConfig>,
    Option<&'a WalkNavigationConfig>,
    Option<&'a Health>,
    Option<&'a MovementConfig>,
    Option<&'a AirSupply>,
);

/// Derived state of one mob from its composition.
struct Derived {
    body: PhysicsBody,
    scale: f32,
    gravity: bool,
    collision: bool,
    can_walk: bool,
    health: Option<(f32, f32)>,
    movement: Option<f32>,
    air: Option<(i16, i16)>,
}

fn derive(
    (_, collision, scale, physics, walk, health, movement, air): DeriveInputs<'_>,
) -> Derived {
    let scale = scale.map_or(1.0, |s| s.value);
    // Published rule: negative collision dimensions count as zero.
    let (width, height) = collision.map_or((0.0, 0.0), |b| (b.width.max(0.0), b.height.max(0.0)));
    let (gravity, has_collision) =
        physics.map_or((false, false), |p| (p.has_gravity, p.has_collision));
    let mut body = PhysicsBody::living(f64::from(width * scale), f64::from(height * scale));
    if !gravity {
        body.gravity = 0.0;
    }
    Derived {
        body,
        scale,
        gravity,
        collision: has_collision,
        can_walk: walk.is_some_and(|w| w.can_walk),
        health: health.map(|h| (h.current, h.max)),
        movement: movement.map(|m| m.value),
        air: air.map(|a| (a.current_ticks as i16, a.max_ticks as i16)),
    }
}

/// Write derived state into the wire/physics components.
fn store(
    archetype: &MobArchetype,
    derived: &Derived,
    body: &mut PhysicsBody,
    flags: &mut ActorFlags,
    metadata: &mut EntityMetadata,
    attributes: &mut ActorAttributes,
) {
    use jolyne::valentine::actor_data_i_ds as ids;
    if *body != derived.body {
        *body = derived.body;
    }
    for (flag, on) in [
        (actor_flags::HAS_GRAVITY, derived.gravity),
        (actor_flags::HAS_COLLISION, derived.collision),
        (actor_flags::CANWALK, derived.can_walk),
    ] {
        flags.update(flag, on).expect("catalogue flag index");
    }
    let words = flags.snapshot();
    metadata.set_long(ids::RESERVED_0, words.words()[0] as i64);
    metadata.set_long(ids::RESERVED_092, words.words()[1] as i64);
    metadata.set_float(ids::RESERVED_038, derived.scale);
    metadata.set_float(ids::RESERVED_053, derived.body.width as f32);
    metadata.set_float(ids::RESERVED_054, derived.body.height as f32);
    if let Some((current, max)) = derived.air {
        metadata.set_short(ids::AIR_SUPPLY, current);
        metadata.set_short(ids::AIR_SUPPLY_MAX, max);
    }
    if let (Some(id), Some((current, max))) = (&archetype.health_attribute, derived.health) {
        let mut value = attributes.get(id).expect("archetype attribute layout");
        value.current = current;
        value.max = max;
        attributes
            .update(id, value)
            .expect("archetype attribute layout");
    }
    if let (Some(id), Some(speed)) = (&archetype.movement_attribute, derived.movement) {
        let value = attributes.get(id).expect("archetype attribute layout");
        attributes
            .update(
                id,
                AttributeValue {
                    current: speed,
                    ..value
                },
            )
            .expect("archetype attribute layout");
    }
}

/// System: re-derive body, flags, metadata and attributes of mobs whose
/// composition, health or air changed. Runs right after the definition
/// apply barrier, before physics.
#[allow(clippy::type_complexity)]
pub fn derive_mob_state(
    mut mobs: Query<
        (
            DeriveInputs<'_>,
            &mut PhysicsBody,
            &mut ActorFlags,
            &mut EntityMetadata,
            &mut ActorAttributes,
        ),
        Or<(
            Changed<crate::entity::definition::DefinitionState>,
            Changed<Health>,
            Changed<AirSupply>,
        )>,
    >,
) {
    for (inputs, mut body, mut flags, mut metadata, mut attributes) in &mut mobs {
        let archetype = Arc::clone(&inputs.0.0);
        let derived = derive(inputs);
        store(
            &archetype,
            &derived,
            &mut body,
            &mut flags,
            &mut metadata,
            &mut attributes,
        );
    }
}

/// Derive one mob immediately (spawn path).
fn derive_entity(world: &mut World, entity: Entity) {
    let mut state: SystemState<
        Query<(
            DeriveInputs<'_>,
            &mut PhysicsBody,
            &mut ActorFlags,
            &mut EntityMetadata,
            &mut ActorAttributes,
        )>,
    > = SystemState::new(world);
    let mut query = state.get_mut(world);
    if let Ok((inputs, mut body, mut flags, mut metadata, mut attributes)) = query.get_mut(entity) {
        let archetype = Arc::clone(&inputs.0.0);
        let derived = derive(inputs);
        store(
            &archetype,
            &derived,
            &mut body,
            &mut flags,
            &mut metadata,
            &mut attributes,
        );
    }
}

use bevy_ecs::system::SystemState;
