//! Runtime state shared between the mob subsystems (AI, combat,
//! interaction, persistence). Each component documents which subsystem
//! writes it and which ones read it.

use bevy_ecs::prelude::*;
use unastar_data::actor_components::body::ActorDamageCause;

/// The last damage a mob took. Written by combat; read by the panic goal
/// (recent-hurt window) and by death handling (killer, `last_hit_by_player`).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct LastHurt {
    /// Server tick (`TickCounter::current`) of the hit.
    pub tick: u64,
    pub cause: ActorDamageCause,
    /// The attacking actor, when there is one.
    pub attacker: Option<Entity>,
    /// Whether the attacker was a player.
    pub by_player: bool,
}

/// Never despawns. Added by naming, a successful player interaction (feeding,
/// milking, breeding) and when the tempt goal starts.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Persistent;

/// Love mode after being fed a breed item. Written by interaction; read by
/// the breed goal.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct InLove {
    pub ticks_left: u32,
    /// The player who fed the mob (credited with breeding XP).
    pub player: Option<Entity>,
}

/// Ticks until the mob may enter love mode again after mating.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreedCooldown {
    pub ticks_left: u32,
}

/// Raised by the breed goal when two partners in love touch at a mating
/// check; consumed by interaction, which spawns the baby.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MateRequest {
    pub a: Entity,
    pub b: Entity,
}
