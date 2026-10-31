//! Authored projectile launch, collision, reflection and hit effects.
//!
//! Player items supply their eye origin through [`launch_with`]. Actor-owned
//! [`launch_aimed`] resolves the authored origin, player eye or bounding-box
//! middle, applies the local offset and difficulty-adjusted Gaussian spread,
//! then launches at an authoritative world-space target. Mob eye anchors and
//! non-default actor-shot damage randomization are explicitly unsupported.
//!
//! Flight sweeps block and actor collision boxes nearest first. Impacts use
//! the actual segment contact point. The configured launch immunity permits
//! an owner to be hit later; reflection changes the owner and direction without
//! replacing the actor or adding living physics. A blocked projectile can stick
//! at its hit point, emit particles, spawn authored actors or be removed.
//!
//! Impact damage uses the ordinary player/mob damage lanes. Accepted damaging
//! fire impacts ignite through their existing immunity-aware fire handlers.
//! Block ignition affects only the air cell outside the hit face and honors
//! mob griefing when configured. Unimplemented hit subcomponents fail compilation.

pub mod config;
mod fire;
pub mod flight;
pub mod hit;
pub mod launch;
pub mod pickup;
pub(crate) mod reflect;

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
pub use launch::{
    LaunchError, LaunchSpec, apply_throws, launch, launch_aimed, launch_with, thrown_by,
};
pub use pickup::pick_up_stuck_projectiles;

/// Flight state of a projectile actor.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct ProjectileFlight {
    pub owner: Option<Entity>,
    /// The server tick of its first flight tick.
    pub launched_tick: u64,
    /// A fully drawn bow or a crossbow shot: speed damage gets the
    /// critical bonus.
    pub critical: bool,
}

/// A projectile that stuck in a block (`on_hit.stick_in_ground`), since
/// this server tick. It no longer moves or hits.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct Stuck {
    pub since: u64,
}

/// The item a player gets back by walking into the stuck projectile
/// (arrows shot and tridents thrown in survival).
#[derive(Component, Debug, Clone, PartialEq)]
pub struct ProjectileItem(pub crate::item::ItemStack);

/// Players that used a throwable item this tick (queued by the inventory
/// system, thrown by [`apply_throws`]).
#[derive(Resource, Default)]
pub struct ThrowQueue(pub Vec<Entity>);

#[cfg(test)]
mod tests;
