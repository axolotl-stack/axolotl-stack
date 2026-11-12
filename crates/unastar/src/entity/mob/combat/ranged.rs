//! A mob's ranged shot (`minecraft:shooter` with
//! `behavior.ranged_attack`), made after the AI tick that decided it.
//!
//! As BDS 1.26.51 shoots (measured with `sweep.py ranged`): the projectile
//! is created at the shooter's feet with the centre of its box
//! [`RANGED_SHOT_CREATED_CENTRE`] up, where its `disappeared` sound plays,
//! its `minecraft:entity_spawned` seeing the shooter as `other` (the
//! arrow's `mob_arrow` group, power 1.6). It then starts
//! [`RANGED_SHOT_FORWARD`] horizontally from the shooter's feet toward the
//! target's feet, at [`RANGED_SHOT_EYE_RATIO`] of the shooter's height plus
//! the projectile's `offset`. It aims at the centre of the target's box
//! along the drag-free parabola of its power and gravity, measured from
//! the shooter's horizontal position ([`aim_direction`]); each axis of that
//! unit direction gets a gaussian times
//! [`RANGED_SPREAD_SCALE`] × the projectile's uncertainty (`uncertainty_base
//! - difficulty × uncertainty_multiplier`), and the sum times the power is
//! its velocity. The projectile carries the shooter as its owner and the
//! shooter's `aux_val` (a tipped arrow's potion) as `CUSTOM_DISPLAY`. The
//! shot sounds the unnamed [`RANGED_SHOT_SOUND`] and then the shooter's own
//! `sound`, both at the centre of the shooter's box.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_data_i_ds as ids;
use tracing::warn;

use crate::entity::EntityRandom;
use crate::entity::components::{Position, Rotation, Velocity};
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::ai::wire_actor_id;
use crate::entity::mob::components::ShooterConfig;
use crate::entity::mob::constants::{
    RANGED_SHOT_CREATED_CENTRE, RANGED_SHOT_EYE_RATIO, RANGED_SHOT_FORWARD, RANGED_SHOT_SOUND,
    RANGED_SPREAD_SCALE,
};
use crate::entity::mob::net::sound::play_level_sound;
use crate::entity::mob::state::Dead;
use crate::entity::mob::{MobRegistry, spawn_mob_by};
use crate::entity::projectile::{ArrowPotion, ProjectileConfig, ProjectileFlight, heading};
use crate::world::rules::Difficulty;

/// `shooter` shoots at `target` (its ranged goal's timer ran out this tick).
pub fn shoot(world: &mut World, shooter: Entity, target: Entity) {
    let Some(config) = world.get::<ShooterConfig>(shooter).cloned() else {
        return;
    };
    if world.get::<Dead>(shooter).is_some() {
        return;
    }
    let (Some(feet), Some(body)) = (
        world.get::<Position>(shooter).map(|p| p.0),
        world.get::<crate::entity::components::PhysicsBody>(shooter),
    ) else {
        return;
    };
    let height = body.height;
    let Some(aim) = crate::entity::mob::ai::target_centre(world, target) else {
        return;
    };
    let projectile = match create(world, &config.projectile, feet, shooter) {
        Ok(projectile) => projectile,
        Err(error) => {
            warn!(%error, projectile = %config.projectile, "A mob shot failed");
            return;
        }
    };
    let Some(rules) = world
        .get::<ProjectileConfig>(projectile)
        .map(|c| Arc::clone(&c.0))
    else {
        world.despawn(projectile);
        warn!(projectile = %config.projectile, "A mob shot a non-projectile");
        return;
    };

    let toward = DVec3::new(aim.x - feet.x, 0.0, aim.z - feet.z);
    // The projectile starts toward the target's feet: the same horizontal
    // line as its box centre.
    let forward = if toward.length_squared() > 0.0 {
        toward.normalize() * RANGED_SHOT_FORWARD
    } else {
        DVec3::ZERO
    };
    let start = feet + forward + DVec3::Y * (height * RANGED_SHOT_EYE_RATIO + rules.offset.y);
    let direction = aim_direction(feet, start, aim, rules.power, rules.gravity);
    let difficulty = world
        .get_resource::<Difficulty>()
        .copied()
        .unwrap_or_default();
    let uncertainty = f64::from(
        rules.uncertainty_base - f32::from(difficulty as u8) * rules.uncertainty_multiplier,
    );
    let mut direction = direction;
    if uncertainty > 0.0 {
        let rng = &mut world.resource_mut::<EntityRandom>().0;
        let spread = RANGED_SPREAD_SCALE * uncertainty;
        direction += DVec3::new(gaussian(rng), gaussian(rng), gaussian(rng)) * spread;
    }
    let velocity = direction * rules.power;
    let (yaw, pitch) = heading(velocity);
    let owner = wire_actor_id(world, shooter);
    let mut entity = world.entity_mut(projectile);
    // A projectile's head yaw stays 0 (its BDS 1.26.51 `AddActor`).
    entity.insert((
        Position(start),
        Velocity(velocity),
        Rotation::with_head_yaw(yaw, pitch, 0.0),
    ));
    if let Some(mut flight) = entity.get_mut::<ProjectileFlight>() {
        flight.owner = Some(shooter);
    }
    if let Some(aux) = config.aux {
        entity.insert(ArrowPotion(aux));
    }
    if let Some(mut metadata) = entity.get_mut::<EntityMetadata>() {
        metadata.set_long(ids::OWNER, owner);
        metadata.set_byte(ids::CUSTOM_DISPLAY, config.aux.map_or(0, |aux| aux as i8));
    }
    // Its first viewers see it as shot, not as it was created.
    crate::entity::mob::net::network::record_spawn_state(world, projectile, Some(false));

    let centre = feet + DVec3::Y * (height / 2.0);
    play_level_sound(world, None, centre, RANGED_SHOT_SOUND, -1);
    if let Some(sound) = &config.sound {
        play_level_sound(world, Some(shooter), centre, sound, -1);
    }
}

/// Create the projectile with the centre of its box
/// [`RANGED_SHOT_CREATED_CENTRE`] above the shooter's feet.
fn create(
    world: &mut World,
    identifier: &str,
    feet: DVec3,
    shooter: Entity,
) -> Result<Entity, crate::entity::mob::MobSpawnError> {
    let half_height = world
        .get_resource::<MobRegistry>()
        .and_then(|registry| registry.get(identifier))
        .map_or(0.0, |archetype| archetype.base_height / 2.0);
    let at = feet + DVec3::Y * (RANGED_SHOT_CREATED_CENTRE - half_height);
    spawn_mob_by(world, identifier, at, 0.0, shooter)
}

/// The unit launch direction at `aim` (the centre of the target's box):
/// toward it horizontally, pitched up by the drag-free parabola through it
/// at `speed` under `gravity`, measured from the shooter's horizontal
/// position and the projectile's start height. The lower of the two angles
/// is used. Out of reach (only far above the shooter: the authored ranges
/// are 15 blocks, the arc reaches about 50) it uses the 45° pitch that
/// reaches farthest; that case is unmeasured.
pub(crate) fn aim_direction(
    shooter: DVec3,
    start: DVec3,
    aim: DVec3,
    speed: f64,
    gravity: f64,
) -> DVec3 {
    let toward = DVec3::new(aim.x - shooter.x, 0.0, aim.z - shooter.z);
    let x = toward.length();
    let y = aim.y - start.y;
    let horizontal = if x > 0.0 { toward / x } else { DVec3::ZERO };
    let slope = if x <= 0.0 || gravity <= 0.0 {
        if x > 0.0 {
            y / x
        } else {
            y.signum() * f64::MAX
        }
    } else {
        let v2 = speed * speed;
        let disc = v2 * v2 - gravity * (gravity * x * x + 2.0 * y * v2);
        if disc >= 0.0 {
            (v2 - disc.sqrt()) / (gravity * x)
        } else {
            1.0
        }
    };
    let pitch = slope.atan();
    horizontal * pitch.cos() + DVec3::Y * pitch.sin()
}

/// A standard normal sample (Box-Muller).
fn gaussian(rng: &mut impl rand::Rng) -> f64 {
    let u: f64 = rng.r#gen::<f64>().max(f64::MIN_POSITIVE);
    let v: f64 = rng.r#gen();
    (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
}
