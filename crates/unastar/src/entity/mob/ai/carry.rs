//! One-shot block take/place goals. Read-only proposals are revalidated after
//! every AI event and before world/carried state changes commit together.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use rand::Rng;

use super::goals::GoalContext;
use super::sensing::can_see;
use crate::block_entity::ContainerKind;
use crate::entity::components::{Health, PhysicsBody, Player, Position};
use crate::entity::mob::carry::{self, CarriedBlock};
use crate::entity::mob::components::carry::{CarryGoal, PlaceBlockGoal, TakeBlockGoal};
use crate::entity::mob::events::MobFacts;
use crate::entity::mob::life::triggers;
use crate::entity::mob::state::{AttackTarget, Dead};
use crate::entity::systems::BlockSource;
use crate::registry::collision::Aabb;
use crate::server::game::{ContainerMutationLocks, world_stack};
use crate::world::chunk::blocks;
use crate::world::ecs::{
    BlockBroadcastEvent, BlockChanged, ChunkData, ChunkManager, ChunkStateFlags,
    world_to_chunk_coords, world_to_local_coords,
};
use crate::world::rules::GameRules;

/// Immutable work selected by one read-only goal evaluation.
#[derive(Debug, Clone)]
pub(super) enum CarryWork {
    Take {
        origin: DVec3,
        position: IVec3,
        state: u32,
        goal: TakeBlockGoal,
    },
    Place {
        origin: DVec3,
        position: IVec3,
        state: u32,
        carried: Option<u32>,
        random_index: Option<usize>,
        goal: PlaceBlockGoal,
    },
}

/// Each instantaneous goal owns its own staged proposal; nonexclusive goals
/// cannot overwrite one another's work during the selector's start phase.
#[derive(Debug, Default)]
pub(super) struct CarryState {
    pub take: Option<CarryWork>,
    pub place: Option<CarryWork>,
}

/// Current world conditions shared by proposal and committed revalidation.
fn enabled(world: &World, actor: Entity, goal: &CarryGoal) -> bool {
    !world.get::<Health>(actor).is_none_or(Health::is_dead)
        && world.get::<Dead>(actor).is_none()
        && (!goal.affected_by_griefing || world.resource::<GameRules>().mob_griefing)
        && goal.filter.as_ref().is_none_or(|filter| {
            filter.evaluate(&MobFacts {
                world,
                entity: actor,
                other: world.get::<AttackTarget>(actor).map(|target| target.0),
            })
        })
}

/// Locked, unloaded, outside-height and block-actor cells cannot participate.
fn cell(world: &World, position: IVec3) -> Option<u32> {
    if world
        .get_resource::<ContainerMutationLocks>()
        .is_some_and(|locks| locks.is_locked(position))
    {
        return None;
    }
    let (cx, cz) = world_to_chunk_coords(position.x, position.z);
    let chunk = world.get::<ChunkData>(world.resource::<ChunkManager>().get_by_coords(cx, cz)?)?;
    if !chunk.inner.block_bounds().contains(&position.y)
        || chunk
            .inner
            .block_entities()
            .iter()
            .any(|actor| actor.position() == position.to_array())
    {
        return None;
    }
    world.block_at(position)
}

/// The target must be an authored non-fluid block with a real item mapping;
/// native container types remain ineligible even if their actor is missing.
fn takeable(
    world: &World,
    actor: Entity,
    position: IVec3,
    state: u32,
    goal: &TakeBlockGoal,
) -> bool {
    let Some(block) = world.registry().get_by_runtime_id(state) else {
        return false;
    };
    if state == *blocks::AIR
        || block.data.liquid
        || ContainerKind::from_block(&block.string_id).is_some()
        || world_stack(world, &block.string_id, 1).is_none()
        || (!goal.blocks.is_empty() && !goal.blocks.iter().any(|block| block.matches(state)))
    {
        return false;
    }
    if !goal.line_of_sight {
        return true;
    }
    let (Some(position_actor), Some(body)) = (
        world.get::<Position>(actor),
        world.get::<PhysicsBody>(actor),
    ) else {
        return false;
    };
    let eye = position_actor.0
        + DVec3::Y * body.height * crate::entity::mob::constants::MOB_EYE_HEIGHT_RATIO;
    let closest = eye.clamp(position.as_dvec3(), position.as_dvec3() + DVec3::ONE);
    let end = closest + (eye - closest).normalize_or_zero() * 1.0e-4;
    can_see(world, eye, end)
}

/// An ordinary solid support is required; placement never overwrites even a
/// replaceable decoration or liquid and never puts a block inside a living actor.
fn clear_supported(world: &World, position: IVec3) -> bool {
    if cell(world, position) != Some(*blocks::AIR) {
        return false;
    }
    let Some(support) = world
        .block_at(position - IVec3::Y)
        .and_then(|state| world.registry().get_by_runtime_id(state))
    else {
        return false;
    };
    support.data.solid && !support.data.liquid
}

/// Re-check actual actor occupancy at the authoritative commit boundary.
fn placeable(world: &mut World, position: IVec3) -> bool {
    if !clear_supported(world, position) {
        return false;
    }
    let destination = Aabb::new(position.as_dvec3(), position.as_dvec3() + DVec3::ONE);
    !world
        .query::<(
            &Position,
            Option<&PhysicsBody>,
            Has<Player>,
            Option<&Health>,
        )>()
        .iter(world)
        .any(|(position, body, player, health)| {
            if !player && health.is_none_or(Health::is_dead) {
                return false;
            }
            let box_ = if player {
                crate::server::game::player_box(position.0)
            } else if let Some(body) = body {
                Aabb::from_feet(position.0, body.width, body.height)
            } else {
                return false;
            };
            destination.intersects(&box_)
        })
}

impl GoalContext<'_> {
    /// Pick one authored offset without integer overflow or loading terrain.
    fn carry_position(&mut self, goal: &CarryGoal) -> Option<IVec3> {
        let origin = self.body.feet.floor().as_ivec3();
        Some(IVec3::new(
            origin
                .x
                .checked_add(self.rng.gen_range(goal.xz.0..=goal.xz.1))?,
            origin
                .y
                .checked_add(self.rng.gen_range(goal.y.0..=goal.y.1))?,
            origin
                .z
                .checked_add(self.rng.gen_range(goal.xz.0..=goal.xz.1))?,
        ))
    }

    /// Stage a take only after chance, current ownership and line of sight pass.
    pub(super) fn take_block_can_start(&mut self) -> bool {
        let Some(goal) = self.config::<TakeBlockGoal>() else {
            return false;
        };
        if self.world.get::<CarriedBlock>(self.entity).is_some()
            || !enabled(self.world, self.entity, &goal.base)
            || self.rng.r#gen::<f32>() >= goal.base.chance
        {
            return false;
        }
        let Some(position) = self.carry_position(&goal.base) else {
            return false;
        };
        let Some(state) = cell(self.world, position) else {
            return false;
        };
        if !takeable(self.world, self.entity, position, state, goal) {
            return false;
        }
        self.states.carry.take = Some(CarryWork::Take {
            origin: self.body.feet,
            position,
            state,
            goal: goal.clone(),
        });
        true
    }

    /// Stage a carried or weighted generated block; final occupancy is checked
    /// with the live actor query immediately before mutation.
    pub(super) fn place_block_can_start(&mut self) -> bool {
        let Some(goal) = self.config::<PlaceBlockGoal>() else {
            return false;
        };
        if !enabled(self.world, self.entity, &goal.base)
            || self.rng.r#gen::<f32>() >= goal.base.chance
        {
            return false;
        }
        let (state, carried, random_index) = if goal.random.is_empty() {
            let Some(carried) = self
                .world
                .get::<CarriedBlock>(self.entity)
                .map(|carried| carried.0)
            else {
                return false;
            };
            if !goal.carried.is_empty() && !goal.carried.iter().any(|block| block.matches(carried))
            {
                return false;
            }
            (carried, Some(carried), None)
        } else {
            let facts = MobFacts {
                world: self.world,
                entity: self.entity,
                other: self.states.targets.target,
            };
            let eligible: Vec<_> = goal
                .random
                .iter()
                .enumerate()
                .filter(|(_, block)| {
                    block
                        .filter
                        .as_ref()
                        .is_none_or(|filter| filter.evaluate(&facts))
                })
                .collect();
            let total: u64 = eligible.iter().map(|(_, block)| block.weight).sum();
            if total == 0 {
                return false;
            }
            let mut roll = self.rng.gen_range(0..total);
            let Some((index, block)) = eligible.into_iter().find(|(_, block)| {
                if roll < block.weight {
                    true
                } else {
                    roll -= block.weight;
                    false
                }
            }) else {
                return false;
            };
            (block.block.placement, None, Some(index))
        };
        let Some(position) = self.carry_position(&goal.base) else {
            return false;
        };
        if !clear_supported(self.world, position) {
            return false;
        }
        let area = Aabb::new(position.as_dvec3(), position.as_dvec3() + DVec3::ONE);
        if self.nearby.mobs_in(area).any(|mob| !mob.dead)
            || self
                .nearby
                .players_within(position.as_dvec3() + DVec3::splat(0.5), 3.0)
                .any(|player| area.intersects(&crate::server::game::player_box(player.eye)))
        {
            return false;
        }
        self.states.carry.place = Some(CarryWork::Place {
            origin: self.body.feet,
            position,
            state,
            carried,
            random_index,
            goal: goal.clone(),
        });
        true
    }

    /// Queue the accepted one-shot work for the post-AI authoritative boundary.
    pub(super) fn carry_block_tick(&mut self, take: bool) {
        let slot = if take {
            &mut self.states.carry.take
        } else {
            &mut self.states.carry.place
        };
        if let Some(work) = slot.take() {
            self.effects.carry.push(work);
        }
    }
}

/// Publish a real changed cell through the normal world save/observer/network
/// lanes after the carried state and terrain have both been updated.
fn publish(world: &mut World, chunk: Entity, position: IVec3, before: u32, after: u32) {
    if let Some(mut flags) = world.get_mut::<ChunkStateFlags>(chunk) {
        flags.mark_dirty();
    }
    world.trigger(BlockChanged {
        chunk_entity: chunk,
        block_pos: position,
        old_block: before,
        new_block: after,
    });
    world.write_message(BlockBroadcastEvent {
        chunk_entity: chunk,
        block_pos: position,
        new_block: after,
    });
}

/// Revalidate every mutable fact after prior actor events; failed work leaves
/// both the carried block and all terrain unchanged.
pub(super) fn finish(world: &mut World, actor: Entity, work: CarryWork) {
    let (origin, position, before, after, event, carry_update) = match work {
        CarryWork::Take {
            origin,
            position,
            state,
            goal,
        } => {
            if world.get::<TakeBlockGoal>(actor) != Some(&goal)
                || !enabled(world, actor, &goal.base)
                || world.get::<CarriedBlock>(actor).is_some()
                || cell(world, position) != Some(state)
                || !takeable(world, actor, position, state, &goal)
            {
                return;
            }
            (
                origin,
                position,
                state,
                *blocks::AIR,
                goal.base.event,
                Some(Some(state)),
            )
        }
        CarryWork::Place {
            origin,
            position,
            state,
            carried,
            random_index,
            goal,
        } => {
            if world.get::<PlaceBlockGoal>(actor) != Some(&goal)
                || !enabled(world, actor, &goal.base)
                || carried.is_some_and(|state| {
                    world.get::<CarriedBlock>(actor).map(|block| block.0) != Some(state)
                })
                || random_index.is_some_and(|index| {
                    goal.random.get(index).is_none_or(|entry| {
                        entry.block.placement != state
                            || entry.filter.as_ref().is_some_and(|filter| {
                                !filter.evaluate(&MobFacts {
                                    world,
                                    entity: actor,
                                    other: world.get::<AttackTarget>(actor).map(|target| target.0),
                                })
                            })
                    })
                })
                || !placeable(world, position)
            {
                return;
            }
            (
                origin,
                position,
                *blocks::AIR,
                state,
                goal.base.event,
                carried.map(|_| None),
            )
        }
    };
    if world
        .get::<Position>(actor)
        .is_none_or(|position| position.0 != origin)
    {
        return;
    }
    let (cx, cz) = world_to_chunk_coords(position.x, position.z);
    let Some(chunk) = world.resource::<ChunkManager>().get_by_coords(cx, cz) else {
        return;
    };
    let (x, y, z) = world_to_local_coords(position.x, position.y, position.z);
    let Some(mut data) = world.get_mut::<ChunkData>(chunk) else {
        return;
    };
    if data.inner.get_block(x, y, z) != before || data.inner.set_block(x, y, z, after).is_none() {
        return;
    }
    if let Some(state) = carry_update {
        carry::set(world, actor, state);
    }
    publish(world, chunk, position, before, after);
    if let Some(event) = event {
        triggers::fire(
            world,
            actor,
            &event,
            world.get::<AttackTarget>(actor).map(|target| target.0),
        );
    }
}
