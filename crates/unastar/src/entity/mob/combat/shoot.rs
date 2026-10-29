//! A ranged attack's shot (`minecraft:shooter`), made after the AI tick
//! that decided it (`mob::ai::ranged_attack`).
//!
//! The shooter's projectile is launched at the target
//! (`entity::projectile::launch_at`) carrying the shooter's `aux_val`, then
//! the shooter's sound plays at the centre of its box and an angry shooter
//! stays angry (matches BDS 1.26.51).

use bevy_ecs::prelude::*;
use glam::DVec3;
use tracing::warn;

use crate::entity::components::{Health, PhysicsBody, Position};
use crate::entity::mob::components::ranged_attack::ShooterConfig;
use crate::entity::mob::life::angry::restart_anger;
use crate::entity::mob::net::sound::play_level_sound;
use crate::entity::mob::state::Dead;
use crate::entity::projectile::{MobShot, launch_at};

/// `shooter` shoots at `target`.
pub fn shoot(world: &mut World, shooter: Entity, target: Entity) {
    let Some(config) = world.get::<ShooterConfig>(shooter).cloned() else {
        return;
    };
    if world.get::<Dead>(shooter).is_some()
        || world.get::<Health>(target).is_none_or(Health::is_dead)
    {
        return;
    }
    let shot = MobShot {
        power: config.power,
        potion: config.aux_val,
    };
    if let Err(error) = launch_at(world, shooter, config.projectile, target, shot) {
        warn!(%error, "a ranged attack could not shoot");
        return;
    }
    if let Some(sound) = &config.sound
        && let (Some(feet), Some(body)) = (
            world.get::<Position>(shooter).map(|p| p.0),
            world.get::<PhysicsBody>(shooter),
        )
    {
        let at = feet + DVec3::Y * body.height / 2.0;
        play_level_sound(world, Some(shooter), at, sound, -1);
    }
    restart_anger(world, shooter);
}
