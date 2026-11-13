//! Derived mob state: the physics body, actor flags, metadata and
//! attributes that follow from the current composition, health and air.
//! [`derive_mob_state`] re-derives them each tick for mobs whose inputs
//! changed; [`derive_entity`] derives one mob at once (spawn and restore).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use jolyne::valentine::actor_flags;

use super::MobKind;
use super::archetype::MobArchetype;
use super::components::{FloatNavigation, WalkNavigationConfig};
use crate::entity::components::{AirSupply, Health, PhysicsBody};
use crate::entity::definition::DefinitionState;
use crate::entity::definition::attributes::{ActorAttributes, AttributeValue};
use crate::entity::definition::components::{
    MovementConfig, PhysicsConfig, RawCollisionBox, ScaleConfig, UnderwaterMovementConfig,
    WaterMovementConfig,
};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::effects::EffectModifiers;
use crate::entity::mob::constants::LIVING_FRICTION_FACTOR;

/// Inputs of the derived state, gathered from the current composition.
#[allow(clippy::type_complexity)]
type DeriveInputs<'a> = (
    &'a MobKind,
    Option<&'a RawCollisionBox>,
    Option<&'a ScaleConfig>,
    Option<&'a PhysicsConfig>,
    Option<&'a WalkNavigationConfig>,
    Option<&'a FloatNavigation>,
    Option<&'a Health>,
    Option<&'a MovementConfig>,
    Option<&'a UnderwaterMovementConfig>,
    Option<&'a WaterMovementConfig>,
    Option<&'a AirSupply>,
    Option<&'a EffectModifiers>,
);

/// Derived state of one mob from its composition.
struct Derived {
    body: PhysicsBody,
    scale: f32,
    /// The authored collision box, before `scale`: actor data carries it
    /// unscaled next to the scale (the native baby zombie summon reports
    /// 0.98 × 1.96 with scale 0.5), while the physics box is scaled.
    box_size: (f32, f32),
    gravity: bool,
    collision: bool,
    can_walk: bool,
    health: Option<(f32, f32)>,
    movement: Option<f32>,
    /// The authored underwater speed; `None` keeps the native one.
    underwater: Option<f32>,
    air: Option<(i16, i16)>,
    /// What the active effects do to the movement and health attributes.
    modifiers: EffectModifiers,
}

fn derive(
    (
        kind,
        collision,
        scale,
        physics,
        walk,
        floats,
        health,
        movement,
        underwater,
        water,
        air,
        modifiers,
    ): DeriveInputs<'_>,
) -> Derived {
    let scale = scale.map_or(1.0, |s| s.value);
    // Published rule: negative collision dimensions count as zero.
    let (width, height) = collision.map_or((0.0, 0.0), |b| (b.width.max(0.0), b.height.max(0.0)));
    let (gravity, has_collision) =
        physics.map_or((false, false), |p| (p.has_gravity, p.has_collision));
    let mut body = PhysicsBody::living(f64::from(width * scale), f64::from(height * scale));
    body.wing_flap_drag = kind.0.native.wing_flap_drag;
    body.water_drag = water.map(|w| f64::from(w.drag_factor));
    if !gravity {
        body.gravity = 0.0;
    }
    if floats.is_some() {
        // A float-navigating flyer has no gravity whatever its physics
        // says, and air drags it alike on every axis (BDS 1.26.51: a
        // ghast's motion kept 0.91 a tick on x, y and z, and so did a
        // bat's on x and z, its native vertical factor aside).
        body.gravity = 0.0;
        body.vertical_drag = LIVING_FRICTION_FACTOR * kind.0.native.flight_vertical_factor;
    }
    Derived {
        body,
        scale,
        box_size: (width, height),
        gravity,
        collision: has_collision,
        can_walk: walk.is_some_and(|w| w.can_walk) || floats.is_some_and(|f| f.can_walk),
        health: health.map(|h| (h.current, h.max)),
        movement: movement.map(|m| m.value),
        underwater: underwater.map(|u| u.value),
        air: air.map(|a| (a.current_ticks as i16, a.max_ticks as i16)),
        modifiers: modifiers.cloned().unwrap_or_default(),
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
    metadata.set_float(ids::RESERVED_053, derived.box_size.0);
    metadata.set_float(ids::RESERVED_054, derived.box_size.1);
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
        attributes
            .set_modifiers(id, &derived.modifiers.health_modifiers())
            .expect("archetype attribute layout");
    }
    if let Some((id, native)) = &archetype.underwater_attribute {
        let value = attributes.get(id).expect("archetype attribute layout");
        attributes
            .update(
                id,
                AttributeValue {
                    current: derived.underwater.unwrap_or(*native),
                    ..value
                },
            )
            .expect("archetype attribute layout");
    }
    if let (Some(id), Some(speed)) = (&archetype.movement_attribute, derived.movement) {
        let value = attributes.get(id).expect("archetype attribute layout");
        attributes
            .update(
                id,
                AttributeValue {
                    current: derived.modifiers.movement(speed),
                    ..value
                },
            )
            .expect("archetype attribute layout");
        attributes
            .set_modifiers(id, &derived.modifiers.movement)
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

/// System: a mob's health attribute follows its health in the tick it
/// changed, just before the viewers are told (BDS 1.26.51 sends a hurt or
/// healed mob's health with the hurt animation, in the same tick), rather
/// than at the next tick's [`derive_mob_state`].
pub fn sync_mob_health(
    mut mobs: Query<(&MobKind, &Health, &mut ActorAttributes), Changed<Health>>,
) {
    for (kind, health, mut attributes) in &mut mobs {
        let Some(id) = &kind.0.health_attribute else {
            continue;
        };
        let mut value = attributes.get(id).expect("archetype attribute layout");
        value.current = health.current;
        value.max = health.max;
        attributes
            .update(id, value)
            .expect("archetype attribute layout");
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
