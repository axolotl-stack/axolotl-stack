//! `behavior.take_block` and `behavior.place_block`: the enderman picks a
//! block up and puts it down again (BDS 1.26.51, `sweep.py enderman
//! --items carry`, `docs/research/enderman-bds-1.26.51.md`).
//!
//! Native rules:
//! - The carried block is the mob's main-hand item: the block's item with
//!   the block state, shown to viewers as a `MobEquipment`.
//! - Taking: a mob holding nothing, with mobGriefing on (when the goal is
//!   affected by it), rolls the goal's chance each tick; on a hit it picks
//!   one block offset uniformly in the ranges around its feet block. A
//!   listed block there that its eye sees (when the goal needs line of
//!   sight: the first collision box on the line from the eye to the
//!   block's centre is its own, so a flower is never seen) is taken: the
//!   block turns to air and the mob holds it.
//! - Placing: a mob holding a block it may place rolls the chance each
//!   tick; on a hit it picks one offset uniformly in the ranges. An air
//!   block over a block with a full collision box gets the carried block,
//!   and the hand empties.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use rand::Rng;

use super::goals::GoalContext;
use super::sensing::{Sight, first_hit};
use crate::entity::mob::components::{OffsetRange, PlaceBlockGoal, TakeBlockGoal};
use crate::entity::mob::gear::{GearSlot, MobEquipment};
use crate::entity::systems::BlockSource;
use crate::item::ItemStack;
use crate::registry::collision::block_collision_aabbs;
use crate::server::game::blocks::set_block;
use crate::world::chunk::blocks::AIR;
use crate::world::rules::GameRules;

/// What a carry goal asks of the world after the mob's tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarryWork {
    /// Take the block state at this position.
    Take(IVec3, u32),
    /// Put the carried block down here.
    Place(IVec3),
}

fn griefing(world: &World) -> bool {
    world
        .get_resource::<GameRules>()
        .is_none_or(|rules| rules.mob_griefing)
}

/// The block state the mob carries (its main-hand block item), if any.
pub fn carried(world: &World, mob: Entity) -> Option<u32> {
    let stack = world.get::<MobEquipment>(mob)?.get(GearSlot::Mainhand);
    if stack.is_empty() {
        return None;
    }
    let state = u32::try_from(stack.runtime_id).ok()?;
    world
        .registry()
        .get_by_runtime_id(state)
        .is_some_and(|block| block.string_id == stack.item_id())
        .then_some(state)
}

fn offset(rng: &mut impl Rng, xz: OffsetRange, y: OffsetRange) -> IVec3 {
    IVec3::new(
        rng.gen_range(xz.min..=xz.max),
        rng.gen_range(y.min..=y.max),
        rng.gen_range(xz.min..=xz.max),
    )
}

/// Whether the block at `pos` has a full-cube collision box.
fn full_block(world: &World, pos: IVec3) -> bool {
    world.block_at(pos).is_some_and(|state| {
        let mut boxes = block_collision_aabbs(state);
        boxes
            .next()
            .is_some_and(|b| b.min == DVec3::ZERO && b.max == DVec3::ONE && boxes.next().is_none())
    })
}

impl GoalContext<'_> {
    pub(super) fn take_can_start(&mut self) -> bool {
        let Some(goal) = self.config::<TakeBlockGoal>() else {
            return false;
        };
        if carried(self.world, self.entity).is_some()
            || (goal.griefing && !griefing(self.world))
            || self.rng.r#gen::<f32>() >= goal.chance
        {
            return false;
        }
        let pos = self.body.feet.floor().as_ivec3() + offset(self.rng, goal.xz, goal.y);
        let Some(state) = self.world.block_at(pos) else {
            return false;
        };
        if !goal.blocks.contains(state) || state == *AIR {
            return false;
        }
        if goal.line_of_sight && !self.sees_block(pos) {
            return false;
        }
        self.effects.carry = Some(CarryWork::Take(pos, state));
        true
    }

    pub(super) fn place_can_start(&mut self) -> bool {
        let Some(goal) = self.config::<PlaceBlockGoal>() else {
            return false;
        };
        let Some(state) = carried(self.world, self.entity) else {
            return false;
        };
        if !goal.blocks.contains(state)
            || (goal.griefing && !griefing(self.world))
            || self.rng.r#gen::<f32>() >= goal.chance
        {
            return false;
        }
        let pos = self.body.feet.floor().as_ivec3() + offset(self.rng, goal.xz, goal.y);
        let free = self.world.block_at(pos).is_some_and(|here| here == *AIR);
        if !free || !full_block(self.world, pos - IVec3::Y) {
            return false;
        }
        self.effects.carry = Some(CarryWork::Place(pos));
        true
    }

    /// The mob's eye sees the block at `pos`: the first collision box on
    /// the line to its centre is the block's own (a block without one, a
    /// flower, is never seen).
    fn sees_block(&self, pos: IVec3) -> bool {
        let centre = pos.as_dvec3() + DVec3::splat(0.5);
        first_hit(self.world, self.eye(), centre) == Sight::Block(pos)
    }
}

/// Do a carry goal's world work.
pub fn finish(world: &mut World, mob: Entity, work: CarryWork) {
    match work {
        CarryWork::Take(pos, state) => take(world, mob, pos, state),
        CarryWork::Place(pos) => place(world, mob, pos),
    }
}

fn take(world: &mut World, mob: Entity, pos: IVec3, state: u32) {
    let Some(stack) = block_stack(world, state) else {
        return;
    };
    set_block(world, pos, *AIR, false);
    // The carried block always drops with the mob, as a picked-up item.
    let mut entity = world.entity_mut(mob);
    let mut gear = entity.take::<MobEquipment>().unwrap_or_default();
    gear.set(GearSlot::Mainhand, stack, true);
    entity.insert(gear);
}

fn place(world: &mut World, mob: Entity, pos: IVec3) {
    let Some(state) = carried(world, mob) else {
        return;
    };
    set_block(world, pos, state, false);
    if let Some(mut gear) = world.get_mut::<MobEquipment>(mob) {
        gear.take(GearSlot::Mainhand);
    }
}

/// One of the block's item, carrying the block state.
fn block_stack(world: &World, state: u32) -> Option<ItemStack> {
    let block = world.registry().get_by_runtime_id(state)?;
    let items = &world
        .get_resource::<crate::server::game::types::ItemRegistryResource>()?
        .0;
    let item = items.item_type(&block.string_id)?;
    Some(ItemStack::new(item, 1).with_runtime_id(i32::try_from(state).ok()?))
}
