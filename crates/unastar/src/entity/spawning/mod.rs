//! Natural spawning of mobs from the behaviour-pack spawn rules
//! (`spawn_rules/*.json`), on world generation and over time.
//!
//! [`rules`] compiles the typed vanilla spawn rules into weighted entries per
//! biome; [`natural_spawning`] is the spawn cycle (see `cycle.rs` for the
//! cycle, the world-generation pass and how brightness is derived), and
//! `spawn_check.rs` the check each mob's native class runs at its spawn
//! position.
//!
//! Population caps count the mobs of each `population_control` category in
//! the 9 × 9 chunks around the spawning chunk, separately for mobs the cycle
//! spawned on the surface ([`SurfaceMob`]) and all others; a cluster never
//! takes the spawnable mob count past the global limit. Mobs spawned on
//! world generation are not persistent (unless the rule says
//! `minecraft:is_persistent`), so they despawn like any other.

mod cycle;
mod position;
pub mod rules;
pub mod spawn_check;

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
/// registries, log a one-line summary (per-rule reasons at DEBUG), and
/// install the rules resource.
pub fn install(world: &mut World) {
    let rules = {
        let mobs = world.resource::<MobRegistry>();
        let blocks = &world
            .resource::<crate::server::game::types::BlockRegistryResource>()
            .0;
        NaturalSpawnRules::compile(unastar_data::spawn_rules::vanilla(), mobs, blocks)
    };
    let mut active = std::collections::BTreeSet::<&str>::new();
    let mut skipped = std::collections::BTreeSet::<&str>::new();
    let mut active_rules = 0usize;
    for entry in &rules.unsupported {
        skipped.insert(&*entry.identifier);
        tracing::debug!(
            source = %entry.source,
            identifier = %entry.identifier,
            reason = %entry.reason,
            "spawn rule condition not supported; it never spawns"
        );
    }
    for entry in &rules.entries {
        if let Some(reason) = &entry.blocked {
            skipped.insert(&*entry.identifier);
            tracing::debug!(
                source = %entry.source,
                identifier = %entry.identifier,
                reason = %reason,
                "spawn rule condition blocked; drawn spawns spawn nothing"
            );
            continue;
        }
        active_rules += 1;
        active.insert(&*entry.identifier);
        let targets = std::iter::once(&entry.own).chain(
            entry
                .permutations
                .iter()
                .filter(|p| p.entity_type.is_some())
                .map(|p| &p.target),
        );
        for reason in targets.filter_map(rules::SpawnTarget::blocked) {
            tracing::debug!(
                source = %entry.source,
                identifier = %entry.identifier,
                reason = %reason,
                "spawn target blocked; herd members drawn as it spawn nothing"
            );
        }
    }
    let skipped = skipped.difference(&active).count();
    tracing::info!(
        rules = active_rules,
        mobs = active.len(),
        skipped,
        "natural spawning: {active_rules} spawn rules active for {} mobs; \
         {skipped} mobs skipped because they are not implemented yet \
         (reasons at debug level)",
        active.len()
    );
    world.insert_resource(rules);
}
