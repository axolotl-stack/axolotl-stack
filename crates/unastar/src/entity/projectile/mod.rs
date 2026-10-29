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

pub mod cloud;
pub mod config;
pub mod flight;
pub mod hit;
pub mod launch;
pub mod pickup;

use bevy_ecs::prelude::*;

pub use cloud::tick_clouds;
pub(crate) use config::check_spawns;
pub use config::{
    ImpactDamage, OnSpawn, ParticleOnHit, ProjectileConfig, ProjectileError, ProjectileRules,
    SpawnChance, register_component_kinds,
};
pub use flight::tick_projectiles;
#[cfg(test)]
pub(crate) use hit::spawn_count;
pub(crate) use launch::derive_projectile;
pub use launch::{LaunchError, LaunchSpec, apply_throws, launch, launch_with, thrown_by};
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

/// The potion a thrown potion or tipped arrow carries (its aux value).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectilePotion(pub i16);

/// Give a just-launched projectile its potion and show it to clients, as
/// BDS 1.26.51 does: thrown potions carry the potion aux in actor data
/// `AUX_VALUE_DATA`, tipped arrows their arrow aux (potion + 1) in
/// `CUSTOM_DISPLAY`.
pub fn carry_potion(world: &mut World, projectile: Entity, potion: i16, arrow: bool) {
    use jolyne::valentine::actor_data_i_ds as ids;
    let mut entity = world.entity_mut(projectile);
    entity.insert(ProjectilePotion(potion));
    if let Some(mut metadata) =
        entity.get_mut::<crate::entity::definition::metadata::EntityMetadata>()
    {
        if arrow {
            metadata.set_byte(ids::CUSTOM_DISPLAY, (potion + 1) as i8);
        } else {
            metadata.set_short(ids::AUX_VALUE_DATA, potion);
        }
    }
}

/// Impaling level of a thrown trident: 2.5 extra damage per level to an
/// actor in water (BDS 1.26.51: a drowned in water dies to an Impaling V
/// trident, one on land takes no bonus).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Impaling(pub u8);

/// A projectile that hurt an actor (`stop_on_hurt`); a Loyalty trident
/// starts its return from here.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct DealtDamage;

/// A Loyalty trident flying back to its owner: moved by its owner's return
/// rule, not by flight, so it neither collides nor falls.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct Returning;

/// The item a player gets back by walking into the stuck projectile
/// (arrows shot and tridents thrown in survival).
#[derive(Component, Debug, Clone, PartialEq)]
pub struct ProjectileItem(pub crate::item::ItemStack);

/// Players that used a throwable item this tick (queued by the inventory
/// system, thrown by [`apply_throws`]).
#[derive(Resource, Default)]
pub struct ThrowQueue(pub Vec<Entity>);
