//! `minecraft:projectile`: thrown and shot actors (eggs, snowballs, arrows,
//! tridents) from players and from mobs' `minecraft:shooter`.
//!
//! Player launch ([`launch`], [`launch_with`]): the projectile actor is
//! created at the thrower's eye, moved by the definition's `offset` (x/z
//! turned by the thrower's yaw, stopped by blocks) and sent along the
//! thrower's view direction (pitch plus `angle_offset × cos(pitch)`) at
//! `power` blocks per tick, or the speed and spread a bow, crossbow or
//! trident sets; the thrower's own velocity is not added.
//!
//! Mob shot ([`launch_at`]): the projectile is created with its shooter as
//! `other` of `minecraft:entity_spawned` (an arrow takes its `mob_arrow`
//! rules), starts at the shooter's eyes (`anchor`), a block toward the
//! target, and aims at the centre of the target's box along the drag-free
//! ballistic arc, spread by `uncertainty_base − difficulty ×
//! uncertainty_multiplier`. It is never critical, leaves nothing to pick up
//! and carries the shooter's `aux_val` ([`ProjectilePotion`]).
//!
//! Every projectile actor starts invisible and shows from its second tick
//! of flight.
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
//! On a hit the `on_hit` effects run:
//! - `arrow_effect`: the potion the projectile carries applies its effects
//!   to the hit actor for an eighth of their duration.
//! - `impact_damage`: the hit mob or player takes the rolled or speed
//!   damage (cause projectile, even 0) through its hurt rules, pushed away
//!   from the projectile by the projectile's knockback rules; speed damage
//!   is randomised by `difficulty_randomization`.
//! - `stick_in_ground`: a block hit stops the projectile ([`Stuck`]).
//! - `particle_on_hit`: level events at the projectile.
//! - `remove_on_hit`.
//! - `spawn_chance`: one roll for the first group and, only when that
//!   succeeded, one for the second; the spawns appear at the projectile
//!   (raised out of a block it is inside, turned to its yaw) and get the
//!   `on_spawn` events whose filters pass, with the projectile as `other`.
//!
//! Then the hit sound plays and a removed projectile despawns.
//!
//! Layout: [`config`] compiles the component, [`launch`] throws and shoots,
//! [`flight`] moves and collides, [`hit`] runs the `on_hit` effects,
//! [`pickup`] hands stuck projectiles' items to players.

pub mod config;
pub mod flight;
pub mod hit;
pub mod launch;
pub mod pickup;

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
    LaunchError, LaunchSpec, MobShot, apply_throws, launch, launch_at, launch_with, thrown_by,
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

/// The tipped-arrow data a projectile carries (its shooter's `aux_val`,
/// replicated as `CUSTOM_DISPLAY`): the potion id plus one. Its
/// `arrow_effect` applies that potion to the actor it hits.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectilePotion(pub i8);

/// Players that used a throwable item this tick (queued by the inventory
/// system, thrown by [`apply_throws`]).
#[derive(Resource, Default)]
pub struct ThrowQueue(pub Vec<Entity>);
