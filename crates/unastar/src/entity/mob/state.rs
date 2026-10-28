//! Runtime state shared between the mob subsystems (AI, combat,
//! interaction, persistence). Each component documents which subsystem
//! writes it and which ones read it.
//!
//! Transient states use sparse-set storage: they come and go during a mob's
//! life, and table storage would move every component of the mob to a new
//! archetype table on each insert/remove. Timers are absolute deadlines on
//! `TickCounter::current`, so nothing is mutated per tick while they run.

use bevy_ecs::prelude::*;
use unastar_data::actor_components::body::ActorDamageCause;

/// The last damage a mob took. Written by combat; read by the panic goal
/// (recent-hurt window) and by death handling (killer, `last_hit_by_player`).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
#[component(storage = "SparseSet")]
pub struct LastHurt {
    /// Server tick (`TickCounter::current`) of the hit.
    pub tick: u64,
    pub cause: ActorDamageCause,
    /// The attacking actor, when there is one.
    pub attacker: Option<Entity>,
    /// Whether the attacker was a player.
    pub by_player: bool,
}

/// The mob has died and is playing its death animation (see
/// `mob::combat::death`). Written by combat when health reaches zero; AI,
/// movement, interaction and despawn systems skip mobs carrying it. The
/// entity is removed `constants::DEATH_TICKS` later.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[component(storage = "SparseSet")]
pub struct Dead;

/// Never despawns. Added by naming, a successful player interaction (feeding,
/// milking, breeding) and when the tempt goal starts.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[component(storage = "SparseSet")]
pub struct Persistent;

/// Love mode after being fed a breed item. Written by interaction; read by
/// the breed goal. Active while `TickCounter::current < until_tick`.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct InLove {
    pub until_tick: u64,
    /// The player who fed the mob (credited with breeding XP).
    pub player: Option<Entity>,
}

/// The mob may not enter love mode again before `until_tick`.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct BreedCooldown {
    pub until_tick: u64,
}

/// Raised by the breed goal when two partners in love touch at a mating
/// check; consumed by interaction, which spawns the baby.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MateRequest {
    pub a: Entity,
    pub b: Entity,
}
