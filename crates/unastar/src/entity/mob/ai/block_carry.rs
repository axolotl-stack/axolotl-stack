//! `behavior.take_block` and `behavior.place_block` (compiled in
//! `mob::components::block_carry`).
//!
//! Both goals act once, the tick they start. Each start needs the
//! `mobGriefing` game rule (when `affected_by_griefing_rule`), a uniform
//! roll below `chance` and the goal's filter. The goal then picks one block
//! position: the mob's block position plus a uniform offset in the `x`,
//! `y` and `z` reach, inclusive, drawn in that order.
//!
//! - Take: the mob's hand is empty; the block there is not air, is one of
//!   `blocks` (any when empty) and, with `requires_line_of_sight`, is seen
//!   from the mob's eye. The block moves into the mob's hand (an item that
//!   always drops on death) and the position becomes air.
//! - Place: the mob carries a block item (one of `placeable_carried_blocks`
//!   when listed); the position is air above a block with a full top face.
//!   The carried block's default state is placed and the hand empties.
//!
//! Viewers see the hand change through the mob's equipment replication.
//! `on_take` / `on_place` fire on the mob afterwards.

use bevy_ecs::prelude::*;
use glam::IVec3;
use rand::Rng;
use tracing::warn;

use super::goals::GoalContext;
use super::sensing::can_see_block;
use crate::entity::mob::components::block_carry::{BlockReach, PlaceBlockGoal, TakeBlockGoal};
use crate::entity::mob::events::MobFacts;
use crate::entity::mob::gear::{GearSlot, MobEquipment, mainhand};
use crate::entity::mob::life::sensors::fire;
use crate::entity::systems::physics::{BlockSource, collect_block_boxes};
use crate::registry::collision::Aabb;
use crate::world::rules::GameRules;

/// A block move a goal chose this tick; made once the AI's resource scopes
/// end (it edits the world and fires events).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarryWork {
    Take(IVec3),
    Place(IVec3),
}

/// The block a carried item places: the registry block named like the item.
fn carried_block(
    world: &World,
    mob: Entity,
) -> Option<&'static crate::registry::block::BlockEntry> {
    let stack = mainhand(world, mob)?;
    crate::registry::BlockRegistry::vanilla().get_by_name(stack.item_id())
}

/// Whether the block at `pos` has a collision box reaching its top face.
pub(crate) fn full_top(world: &World, pos: IVec3) -> bool {
    let min = pos.as_dvec3();
    let mut boxes = Vec::new();
    collect_block_boxes(world, Aabb::new(min, min + glam::DVec3::ONE), &mut boxes);
    boxes.iter().any(|b| b.max.y >= min.y + 1.0)
}

fn is_air(world: &World, pos: IVec3) -> bool {
    world.block_at(pos).is_some_and(|state| {
        world
            .registry()
            .get_by_runtime_id(state)
            .is_some_and(|entry| entry.string_id == "minecraft:air")
    })
}

impl GoalContext<'_> {
    fn griefing_allowed(&self, affected: bool) -> bool {
        !affected
            || self
                .world
                .get_resource::<GameRules>()
                .is_some_and(|rules| rules.mob_griefing)
    }

    fn passes(&self, filter: &unastar_rules::filter::Filter) -> bool {
        filter.evaluate(&MobFacts {
            world: self.world,
            entity: self.entity,
            other: None,
        })
    }

    fn reach_offset(&mut self, (min, max): (i32, i32)) -> i32 {
        if max > min {
            self.rng.gen_range(min..=max)
        } else {
            min
        }
    }

    fn pick_position(&mut self, reach: BlockReach) -> IVec3 {
        let base = self.body.feet.floor().as_ivec3();
        let x = self.reach_offset(reach.xz);
        let y = self.reach_offset(reach.y);
        let z = self.reach_offset(reach.xz);
        base + IVec3::new(x, y, z)
    }

    pub(super) fn take_block_can_start(&mut self) -> bool {
        let Some(goal) = self.config::<TakeBlockGoal>() else {
            return false;
        };
        mainhand(self.world, self.entity).is_none()
            && self.griefing_allowed(goal.affected_by_griefing_rule)
            && self.rng.r#gen::<f32>() < goal.chance
            && self.passes(&goal.can_take)
    }

    pub(super) fn take_block_start(&mut self) {
        let Some(goal) = self.config::<TakeBlockGoal>() else {
            return;
        };
        let pos = self.pick_position(goal.reach);
        let Some(state) = self.world.block_at(pos) else {
            return;
        };
        let Some(entry) = self.world.registry().get_by_runtime_id(state) else {
            return;
        };
        if entry.string_id == "minecraft:air"
            || (!goal.blocks.is_empty() && !goal.blocks.contains(&entry.id))
        {
            return;
        }
        if goal.requires_line_of_sight && !can_see_block(self.world, self.self_eye(), pos) {
            return;
        }
        self.effects.carry = Some(CarryWork::Take(pos));
    }

    pub(super) fn place_block_can_start(&mut self) -> bool {
        let Some(goal) = self.config::<PlaceBlockGoal>() else {
            return false;
        };
        if !self.griefing_allowed(goal.affected_by_griefing_rule)
            || self.rng.r#gen::<f32>() >= goal.chance
        {
            return false;
        }
        let Some(block) = carried_block(self.world, self.entity) else {
            return false;
        };
        (goal.placeable_carried_blocks.is_empty()
            || goal.placeable_carried_blocks.contains(&block.id))
            && self.passes(&goal.can_place)
    }

    pub(super) fn place_block_start(&mut self) {
        let Some(goal) = self.config::<PlaceBlockGoal>() else {
            return;
        };
        let pos = self.pick_position(goal.reach);
        if is_air(self.world, pos)
            && !is_air(self.world, pos - IVec3::Y)
            && full_top(self.world, pos - IVec3::Y)
        {
            self.effects.carry = Some(CarryWork::Place(pos));
        }
    }
}

/// Make the block move `work` of `mob`.
pub(super) fn finish(world: &mut World, mob: Entity, work: CarryWork) {
    let event = match work {
        CarryWork::Take(pos) => take(world, mob, pos),
        CarryWork::Place(pos) => place(world, mob, pos),
    };
    if let Some(event) = event {
        fire(world, mob, &event, "block carry");
    }
}

fn take(
    world: &mut World,
    mob: Entity,
    pos: IVec3,
) -> Option<crate::entity::mob::components::SelfEvent> {
    let goal = world.get::<TakeBlockGoal>(mob)?.clone();
    let state = world.block_at(pos)?;
    let entry = world.registry().get_by_runtime_id(state)?;
    let name = entry.string_id.clone();
    let Some(stack) = crate::server::game::world_stack(world, &name, 1) else {
        warn!(block = %name, "take_block: the block has no item to carry");
        return None;
    };
    let air =
        crate::registry::BlockRegistry::vanilla().default_state_id_by_name("minecraft:air")?;
    super::eat_block::set_block(world, pos, air);
    world
        .entity_mut(mob)
        .entry::<MobEquipment>()
        .or_default()
        .get_mut()
        .set(GearSlot::Mainhand, stack, true);
    goal.on_take
}

fn place(
    world: &mut World,
    mob: Entity,
    pos: IVec3,
) -> Option<crate::entity::mob::components::SelfEvent> {
    let goal = world.get::<PlaceBlockGoal>(mob)?.clone();
    let state = carried_block(world, mob)?.default_state_id;
    world.get_mut::<MobEquipment>(mob)?.take(GearSlot::Mainhand);
    super::eat_block::set_block(world, pos, state);
    goal.on_place
}
