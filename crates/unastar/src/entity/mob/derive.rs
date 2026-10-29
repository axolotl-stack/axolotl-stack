//! Derived mob state: the physics body, actor flags, metadata and
//! attributes that follow from the current composition, health and air.
//! [`derive_mob_state`] re-derives them each tick for mobs whose inputs
//! changed; [`derive_entity`] derives one mob at once (spawn and restore).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use jolyne::valentine::actor_flags;

use super::MobKind;
use super::archetype::MobArchetype;
use super::components::WalkNavigationConfig;
use crate::entity::components::{AirSupply, Health, PhysicsBody};
use crate::entity::definition::DefinitionState;
use crate::entity::definition::attributes::{ActorAttributes, AttributeValue};
use crate::entity::definition::components::{
    MovementConfig, PhysicsConfig, RawCollisionBox, ScaleConfig,
};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;

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
    (kind, collision, scale, physics, walk, health, movement, air): DeriveInputs<'_>,
) -> Derived {
    let scale = scale.map_or(1.0, |s| s.value);
    // Published rule: negative collision dimensions count as zero.
    let (width, height) = collision.map_or((0.0, 0.0), |b| (b.width.max(0.0), b.height.max(0.0)));
    let (gravity, has_collision) =
        physics.map_or((false, false), |p| (p.has_gravity, p.has_collision));
    let mut body = PhysicsBody::living(f64::from(width * scale), f64::from(height * scale));
    body.wing_flap_drag = kind.0.native.wing_flap_drag;
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
    metadata.set_float(
        ids::FALL_DAMAGE_MULTIPLIER,
        archetype.native.fall_damage_multiplier,
    );
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

/// Derived-state outputs written by [`store`].
type DeriveOutputs<'a> = (
    &'a mut PhysicsBody,
    &'a mut ActorFlags,
    &'a mut EntityMetadata,
    &'a mut ActorAttributes,
);

/// Changes that require re-deriving a mob's state.
type DeriveTrigger = Or<(
    Changed<DefinitionState>,
    Changed<Health>,
    Changed<AirSupply>,
)>;

/// System: re-derive body, flags, metadata and attributes of mobs whose
/// composition, health or air changed. Runs right after the definition
/// apply barrier, before physics.
pub fn derive_mob_state(mut mobs: Query<(DeriveInputs<'_>, DeriveOutputs<'_>), DeriveTrigger>) {
    for (inputs, (mut body, mut flags, mut metadata, mut attributes)) in &mut mobs {
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

/// Derive one mob immediately (spawn and restore paths).
pub(crate) fn derive_entity(world: &mut World, entity: Entity) {
    if world
        .get::<crate::entity::components::Projectile>(entity)
        .is_some()
    {
        crate::entity::projectile::derive_projectile(world, entity);
        return;
    }
    let mut query = world.query::<(DeriveInputs<'_>, DeriveOutputs<'_>)>();
    if let Ok((inputs, (mut body, mut flags, mut metadata, mut attributes))) =
        query.get_mut(world, entity)
    {
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
