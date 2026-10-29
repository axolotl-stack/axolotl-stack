//! Natural spawning of mobs from the behaviour-pack spawn rules
//! (`spawn_rules/*.json`), on world generation and over time.
//!
//! [`rules`] compiles the typed vanilla spawn rules into weighted entries per
//! biome; [`natural_spawning`] is the spawn cycle (see `cycle.rs` for the
//! cycle, the world-generation pass and how brightness is derived).
//!
//! Population caps count the mobs of each `population_control` category in
//! the 9 × 9 chunks around the spawning chunk, separately for mobs the cycle
//! spawned on the surface ([`SurfaceMob`]) and all others; a cluster never
//! takes the spawnable mob count past the global limit. Mobs spawned on
//! world generation are not persistent (unless the rule says
//! `minecraft:is_persistent`), so they despawn like any other.

mod cycle;
pub mod rules;

#[cfg(test)]
mod tests;

use bevy_ecs::prelude::*;

pub use cycle::natural_spawning;
pub use rules::{Category, NaturalSpawnRules, SpawnEntry};

use crate::entity::mob::MobRegistry;

/// Marks a mob the natural cycle spawned on the surface; it counts towards
/// the surface population caps. Every other mob (spawn eggs, `/summon`,
/// breeding, restored from storage, underground spawns) counts towards the
/// underground caps.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[component(storage = "SparseSet")]
pub struct SurfaceMob;

/// Compile the vanilla spawn rules against the server's mob and block
/// registries, log what cannot spawn once, and install the rules resource.
pub fn install(world: &mut World) {
    let rules = {
        let mobs = world.resource::<MobRegistry>();
        let blocks = &world
            .resource::<crate::server::game::types::BlockRegistryResource>()
            .0;
        NaturalSpawnRules::compile(unastar_data::spawn_rules::vanilla(), mobs, blocks)
    };
    for entry in &rules.unsupported {
        tracing::info!(
            source = %entry.source,
            identifier = %entry.identifier,
            reason = %entry.reason,
            "spawn rule condition not supported; it never spawns"
        );
    }
    for entry in &rules.entries {
        if let Some(reason) = &entry.blocked {
            tracing::info!(
                source = %entry.source,
                identifier = %entry.identifier,
                reason = %reason,
                "spawn rule condition blocked; drawn spawns spawn nothing"
            );
        }
    }
    world.insert_resource(rules);
}
