//! Infested blocks changing under silverfish: a silverfish merging into a
//! host block and a silverfish waking the ones hidden around it (the goals
//! are `mob::ai::silverfish`). Measured on BDS 1.26.51 (`sweep.py
//! silverfish`):
//!
//! - A merge sends the mob's `SilverfishMergeAnim` actor event, turns the
//!   block into its infested form (`UpdateBlock`, no particles) and removes
//!   the mob without a death.
//! - A woken block with mobGriefing on shows its destroy particles (level
//!   event 2001 at the block centre, the infested block's network id),
//!   becomes air and lets out a silverfish (the measured released actor) at
//!   the block's corner moved [`SILVERFISH_RELEASE_OFFSET_X`] along x. With
//!   mobGriefing off it turns back into its host block.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::types::EnumsActorEvent;

use crate::entity::mob::ai::silverfish::SilverfishWork;
use crate::entity::mob::components::{InfestedBlocks, MergeWithStoneGoal, WakeUpFriendsGoal};
use crate::entity::mob::constants::{
    LEVEL_EVENT_PARTICLES_DESTROY_BLOCK, SILVERFISH_RELEASE_OFFSET_X,
};
use crate::entity::mob::interaction::interact::broadcast_actor_event;
use crate::entity::mob::net::sound::level_event;
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::entity::systems::BlockSource;
use crate::server::game::blocks::set_block;
use crate::world::chunk::blocks;
use crate::world::rules::GameRules;

/// Do what a silverfish goal asked for this tick.
pub fn finish(world: &mut World, mob: Entity, work: SilverfishWork) {
    match work {
        SilverfishWork::Merge(pos) => merge(world, mob, pos),
        SilverfishWork::Wake(woken) => wake(world, mob, &woken),
    }
}

fn merge(world: &mut World, mob: Entity, pos: IVec3) {
    let Some(infested) = world
        .get::<MergeWithStoneGoal>(mob)
        .zip(world.block_at(pos))
        .and_then(|(goal, state)| goal.blocks.infested_state(state))
    else {
        return;
    };
    broadcast_actor_event(world, mob, EnumsActorEvent::SilverfishMergeAnim, 0);
    set_block(world, pos, infested, false);
    world.despawn(mob);
}

fn wake(world: &mut World, mob: Entity, woken: &[IVec3]) {
    let Some(table) = world
        .get::<WakeUpFriendsGoal>(mob)
        .map(|goal| goal.blocks.clone())
    else {
        return;
    };
    let griefing = world
        .get_resource::<GameRules>()
        .is_none_or(|rules| rules.mob_griefing);
    for &pos in woken {
        let Some(state) = world
            .block_at(pos)
            .filter(|&state| table.is_infested(state))
        else {
            continue;
        };
        if !griefing {
            if let Some(host) = table.host_state(state) {
                set_block(world, pos, host, false);
            }
            continue;
        }
        let network_id = crate::server::game::types::block_network_ids(world).to_wire(state);
        level_event(
            world,
            pos.as_dvec3() + DVec3::splat(0.5),
            LEVEL_EVENT_PARTICLES_DESTROY_BLOCK,
            network_id as i32,
        );
        set_block(world, pos, *blocks::AIR, false);
        release(world, &table, pos);
    }
}

/// Let the silverfish out of the infested block that broke at `pos` (also
/// the hook for a player breaking an infested block).
pub fn release(world: &mut World, table: &InfestedBlocks, pos: IVec3) {
    let feet = pos.as_dvec3() + DVec3::X * SILVERFISH_RELEASE_OFFSET_X;
    if let Err(error) = spawn_mob(world, table.released(), feet, 0.0, SpawnCause::Spawned) {
        tracing::warn!(%error, "an infested block's silverfish could not spawn");
    }
}
