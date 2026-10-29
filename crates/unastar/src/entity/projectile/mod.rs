//! `minecraft:projectile`: thrown actors (the egg).
//!
//! Launch ([`launch`]): the projectile actor is created at the thrower's eye,
//! moved by the definition's `offset` (x/z turned by the thrower's yaw,
//! stopped by blocks) and sent along the thrower's view direction (pitch
//! plus `angle_offset × cos(pitch)`) at `power` blocks per tick; the
//! thrower's own velocity is not added. Every projectile actor starts
//! invisible and shows from its second tick of flight.
//!
//! Flight ([`tick_projectiles`]): each tick the segment from the position to
//! position + velocity is tested against block collision boxes and against
//! the boxes of mobs and players, grown by `(ticks in flight - 2) × 0.05`
//! (at most 0.3). The thrower is never hit: only a bounced or reflected
//! projectile may hit its owner, and neither is implemented. Hits run
//! nearest first, only the first unless `multiple_targets`. Then the
//! projectile moves by its velocity without collision, its velocity is
//! scaled by `inertia` (`liquid_inertia` in water) and loses `gravity`, and
//! its rotation turns 20 % of the way to its heading.
//!
//! On a hit the `on_hit` effects run in alphabetical order:
//! - `impact_damage`: a hit mob takes the rolled damage (cause projectile,
//!   even 0), pushed away from the projectile by the projectile's knockback
//!   rules; players have no hurt path on this server and only stop the
//!   projectile.
//! - `particle_on_hit`: level events at the projectile.
//! - `remove_on_hit`.
//! - `spawn_chance`: one roll for the first group and, only when that
//!   succeeded, one for the second; the spawns appear at the projectile
//!   (raised out of a block it is inside, turned to its yaw) and get the
//!   `on_spawn` events whose filters pass, with the projectile as `other`.
//!
//! Then the hit sound plays and a removed projectile despawns.
//!
//! Layout: [`config`] compiles the component, [`launch`] throws,
//! [`flight`] moves and collides, [`hit`] runs the `on_hit` effects.

pub mod config;
pub mod flight;
pub mod hit;
pub mod launch;

use bevy_ecs::prelude::*;

pub(crate) use config::check_spawns;
pub use config::{
    ImpactDamage, OnSpawn, ParticleOnHit, ProjectileConfig, ProjectileError, ProjectileRules,
    SpawnChance, register_component_kinds,
};
pub use flight::tick_projectiles;
#[cfg(test)]
pub(crate) use hit::spawn_count;
pub(crate) use launch::derive_projectile;
pub use launch::{LaunchError, apply_throws, launch, thrown_by};

/// Flight state of a projectile actor.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct ProjectileFlight {
    pub owner: Option<Entity>,
    /// The server tick of its first flight tick.
    pub launched_tick: u64,
}

/// Players that used a throwable item this tick (queued by the inventory
/// system, thrown by [`apply_throws`]).
#[derive(Resource, Default)]
pub struct ThrowQueue(pub Vec<Entity>);
