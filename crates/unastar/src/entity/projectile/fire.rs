//! Entity and block ignition from committed projectile impacts.

use bevy_ecs::prelude::*;
use glam::IVec3;

use super::config::BlockFire;
use crate::ecs::resources::TickCounter;
use crate::entity::components::{GameMode, Health, Player};
use crate::entity::mob::state::Dead;
use crate::entity::systems::BlockSource;
use crate::server::game::types::BlockRegistryResource;
use crate::world::rules::GameRules;

/// Ignite a living collision target through its ordinary, immunity-aware lane.
pub(super) fn actor(world: &mut World, target: Entity, ticks: u64) {
    if ticks == 0
        || world.get::<Dead>(target).is_some()
        || world.get::<Health>(target).is_none_or(Health::is_dead)
        || matches!(
            world.get::<GameMode>(target),
            Some(GameMode::Creative | GameMode::Spectator)
        )
    {
        return;
    }
    if world.get::<Player>(target).is_some() {
        let now = world.resource::<TickCounter>().current;
        crate::entity::player_life::environment::ignite_player(world, target, now, ticks);
    } else {
        crate::entity::mob::combat::fire::ignite(world, target, ticks);
    }
}

/// Ignite only the air cell immediately outside the authoritative hit face.
/// Missing terrain, occupied cells and disabled mob griefing leave it unchanged.
pub(super) fn block(world: &mut World, owner: Option<Entity>, at: IVec3, rules: BlockFire) {
    if rules.affected_by_griefing
        && owner.is_none_or(|owner| world.get::<Player>(owner).is_none())
        && world
            .get_resource::<GameRules>()
            .is_none_or(|rules| !rules.mob_griefing)
    {
        return;
    }
    let Some(current) = world.block_at(at) else {
        return;
    };
    let Some(registry) = world.get_resource::<BlockRegistryResource>() else {
        return;
    };
    if registry
        .0
        .get_by_runtime_id(current)
        .is_none_or(|block| block.string_id != "minecraft:air")
    {
        return;
    }
    let soul = world
        .block_at(at - IVec3::Y)
        .and_then(|id| registry.0.get_by_runtime_id(id))
        .is_some_and(|block| {
            matches!(
                block.string_id.as_str(),
                "minecraft:soul_sand" | "minecraft:soul_soil"
            )
        });
    let Some(fire) = registry.0.default_state_id_by_name(if soul {
        "minecraft:soul_fire"
    } else {
        "minecraft:fire"
    }) else {
        return;
    };
    crate::server::game::blocks::set_block(world, at, fire, false);
}
