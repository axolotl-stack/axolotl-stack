//! `minecraft:behavior.eat_block`: the mob lowers its head over an edible
//! block and, once the eat timer runs out, eats it.
//!
//! The goal may start when a roll in `[0, 1)` falls below `success_chance`
//! (Molang, evaluated per attempt) and the block at the mob's feet, or else
//! the block it stands on, is one of the authored eat blocks. Starting sends
//! viewers `ActorEvent` EatGrass and stops navigation. When the timer
//! (`time_until_eat` seconds) runs out the feet block, or else the one below,
//! is eaten: with the `mobgriefing` game rule on, viewers see its destroy
//! particles (placed at the feet block) and it becomes the authored
//! replacement block; then `on_eat` fires on the mob whether or not the
//! block changed.

use bevy_ecs::prelude::*;
use glam::IVec3;
use jolyne::valentine::types::Vec3;
use jolyne::valentine::{LevelEventPacket, McpePacket};
use rand::Rng;
use rand::rngs::StdRng;
use tracing::warn;
use unastar_rules::molang::eval::{MolangContext, Value as MolangValue, evaluate_num};
use unastar_rules::molang::{BuiltinQuery, QueryId};

use crate::entity::components::{PlayerSession, Position};
use crate::entity::mob::MobKind;
use crate::entity::mob::components::EatBlockGoal;
use crate::entity::mob::constants::LEVEL_EVENT_PARTICLES_DESTROY_BLOCK;
use crate::entity::systems::BlockSource;
use crate::world::ecs::{
    BlockBroadcastEvent, BlockChanged, ChunkData, ChunkManager, ChunkStateFlags, ChunkViewers,
    world_to_chunk_coords, world_to_local_coords,
};

/// Molang facts for `success_chance`: `query.is_baby` and random numbers.
struct EatMolang<'a> {
    baby: bool,
    rng: &'a mut StdRng,
}

impl MolangContext for EatMolang<'_> {
    fn query(&mut self, id: QueryId, _: &[MolangValue]) -> Option<MolangValue> {
        match BuiltinQuery::from_id(id)? {
            BuiltinQuery::IsBaby => Some(MolangValue::Num(if self.baby { 1.0 } else { 0.0 })),
            _ => None,
        }
    }
    fn variable(&self, _: &str) -> Option<MolangValue> {
        None
    }
    fn set_variable(&mut self, _: &str, _: MolangValue) {}
    fn context(&self, _: &str) -> Option<MolangValue> {
        None
    }
    fn this(&self) -> MolangValue {
        MolangValue::Num(0.0)
    }
    fn enter(&mut self, _: u64) -> bool {
        false
    }
    fn leave(&mut self) {}
    fn random(&mut self) -> f32 {
        self.rng.r#gen()
    }
}

/// This attempt's chance to start eating.
pub(super) fn success_chance(goal: &EatBlockGoal, baby: bool, rng: &mut StdRng) -> f32 {
    let Some(expr) = &goal.success_chance else {
        return 0.0;
    };
    match evaluate_num(expr, &mut EatMolang { baby, rng }) {
        Ok(chance) => chance,
        Err(error) => {
            warn!(%error, "eat_block success chance cannot be evaluated");
            0.0
        }
    }
}

/// The block at `pos` if the goal eats it: `(its runtime id, the
/// replacement's runtime id)`.
pub(super) fn edible(world: &World, goal: &EatBlockGoal, pos: IVec3) -> Option<(u32, u32)> {
    let state = world.block_at(pos)?;
    let block = world.registry().get_by_runtime_id(state)?.id;
    goal.pairs
        .iter()
        .find(|pair| pair.eat == block)
        .map(|pair| (state, pair.replace))
}

/// The eat timer of `mob` ran out: eat the block (see the module docs).
pub(super) fn finish_eating(world: &mut World, mob: Entity) {
    let Some(goal) = world.get::<EatBlockGoal>(mob).cloned() else {
        return;
    };
    let Some(feet) = world.get::<Position>(mob).map(|p| p.0.floor().as_ivec3()) else {
        return;
    };
    let below = feet - IVec3::Y;
    let Some((target, (eaten, replace))) = edible(world, &goal, feet)
        .map(|found| (feet, found))
        .or_else(|| edible(world, &goal, below).map(|found| (below, found)))
    else {
        return;
    };
    if world
        .resource::<crate::world::rules::GameRules>()
        .mob_griefing
    {
        let particles = LevelEventPacket {
            event_id: LEVEL_EVENT_PARTICLES_DESTROY_BLOCK,
            position: Vec3 {
                x: feet.x as f32 + 0.5,
                y: feet.y as f32 + 0.5,
                z: feet.z as f32 + 0.5,
            },
            data: eaten as i32,
        };
        replace_block(world, target, replace, particles);
    }
    let Some(event) = goal.on_eat else {
        return;
    };
    let handle = world
        .get::<MobKind>(mob)
        .and_then(|kind| kind.0.events.event(&event.0));
    match handle {
        Some(handle) => {
            if let Err(error) = crate::entity::mob::events::fire_event(world, mob, &handle, None) {
                warn!(event = %event.0, %error, "on_eat event failed");
            }
        }
        None => warn!(event = %event.0, "on_eat event is not defined"),
    }
}

/// Set the block at `pos` to `state` (block-change observers run and viewers
/// get the update) and send the chunk's viewers `particles` first.
pub(super) fn replace_block(
    world: &mut World,
    pos: IVec3,
    state: u32,
    particles: LevelEventPacket,
) {
    let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
    let Some(chunk) = world.resource::<ChunkManager>().get_by_coords(cx, cz) else {
        return;
    };
    if let Some(viewers) = world.get::<ChunkViewers>(chunk) {
        for viewer in viewers.iter() {
            if let Some(session) = world.get::<PlayerSession>(viewer) {
                let _ = session.send(McpePacket::from(particles.clone()));
            }
        }
    }
    let (lx, ly, lz) = world_to_local_coords(pos.x, pos.y, pos.z);
    let Some(mut data) = world.get_mut::<ChunkData>(chunk) else {
        return;
    };
    let old = data.inner.get_block(lx, ly, lz);
    data.inner.set_block(lx, ly, lz, state);
    if let Some(mut flags) = world.get_mut::<ChunkStateFlags>(chunk) {
        flags.mark_needs_rebroadcast();
    }
    world.trigger(BlockChanged {
        chunk_entity: chunk,
        block_pos: pos,
        old_block: old,
        new_block: state,
    });
    world.write_message(BlockBroadcastEvent {
        chunk_entity: chunk,
        block_pos: pos,
        new_block: state,
    });
}
