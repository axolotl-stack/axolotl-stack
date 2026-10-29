//! `minecraft:behavior.pickup_items` and `minecraft:behavior.equip_item`.
//!
//! The pickup goal runs while the mob may pick items up: its
//! `pickup_based_on_chance` roll passed (when authored), it has
//! `shareables` (unless `pickup_same_items_as_in_hand`) and walk
//! navigation, the `mobgriefing` rule is on and its last hurt by an actor
//! is at least `cooldown_after_being_attacked` old. Candidates are the
//! dropped items within `max_dist` horizontally and `search_height`
//! vertically of the mob's feet whose pickup delay ran out and that the
//! mob wants (`mob::gear::choice`), nearest first; the list is cached and
//! each start attempt takes the next one, keeping it only when it is not
//! `excluded_items`, `stop_if_holding_item` finds its hand empty, and a
//! path ends within `goal_radius` of it (counting half the mob's width),
//! or there is no path but the item is already within `goal_radius`.
//!
//! While running, the goal looks at the item and re-paths when the timer
//! (10 ticks at the start, then `4 + U[0, 7)`, `9 + …` past 16 blocks,
//! `14 + …` past 32 blocks, 15 more after a failed search) has run out,
//! the item moved `goal_radius` away from where the path was made for, and
//! the mob sees it or `track_target` is set. Within `goal_radius` the mob
//! takes the item: the count its `shareables` entry allows and its hand
//! stack has room for; `consume_item` entries are used up, anything else
//! becomes the pending stack of `behavior.equip_item` (mobs with
//! `minecraft:equip_item` only). The item loses that many (removed at
//! zero), every player is sent `TakeItemActor` and the mob becomes
//! persistent. `on_pickup_item_start`/`_end` fire when the goal starts and
//! stops; it stops once the item is taken or gone or the path ends.
//!
//! The equip goal equips the pending stack: armour (and head-wearable
//! blocks) first drops the piece worn in its slot; a hand item joins a
//! stack it can join (up to the stack size) or drops what the hand holds.
//! The stack goes to its armour slot when the mob `can_wear_armor` and the
//! item is not in `excluded_items`, else to its hand (the shield to the
//! off hand); an item it displaces there drops too. Equipped items count
//! as picked up (they always drop on death) and the mob becomes
//! persistent. Riding is not implemented, so the zombie's refusal to pick
//! eggs up while riding a chicken never applies.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::types::ActorRuntimeId;
use jolyne::valentine::{McpePacket, TakeItemActorPacket};
use rand::Rng;
use tracing::warn;

use super::goals::GoalContext;
use super::sensing::can_see;
use crate::entity::components::{ItemEntityStack, PickupDelay, PlayerSession, Position, RuntimeId};
use crate::entity::mob::MobKind;
use crate::entity::mob::components::{
    EquipItemConfig, EquipItemGoal, PickupItemsGoal, SelfEvent, ShareablesConfig,
    WalkNavigationConfig,
};
use crate::entity::mob::constants::{
    PICKUP_HEAD_TURN, PICKUP_REPATH_FAIL_TICKS, PICKUP_REPATH_FAR_DIST_SQ, PICKUP_REPATH_FAR_TICKS,
    PICKUP_REPATH_MID_DIST_SQ, PICKUP_REPATH_MID_TICKS, PICKUP_REPATH_NEAR_TICKS,
    PICKUP_REPATH_RANDOM, PICKUP_REPATH_START_TICKS,
};
use crate::entity::mob::events::fire_event;
use crate::entity::mob::gear::choice::{
    consumes, count_to_pickup, joins_hand, stackable, will_pickup,
};
use crate::entity::mob::gear::{
    CanPickUpLoot, GearSlot, MobEquipment, PendingEquip, held_slot, worn_slot,
};
use crate::entity::mob::state::{LastHurt, Persistent};
use crate::entity::spawn_item_drop;
use crate::item::ItemStack;
use crate::world::rules::GameRules;

/// Runtime state of the pickup goal.
#[derive(Debug, Default)]
pub struct PickupState {
    /// Cached candidates, farthest first (the next one is popped).
    candidates: Vec<Entity>,
    /// The item being fetched.
    target: Option<Entity>,
    /// `can_start` found a path to the target (staged for `start`).
    staged_path: bool,
    /// The tick the re-path timer runs out.
    repath_at: u64,
    /// Where the item was when the current path was made.
    last_target: DVec3,
}

/// An item the pickup goal takes this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pickup {
    pub item: Entity,
    pub count: u8,
    /// The `shareables` entry uses the item up.
    pub consume: bool,
}

/// The world changes the two goals request for one mob, made once the AI
/// tick ends ([`finish`]).
#[derive(Debug, Default)]
pub struct GearWork {
    pub pickup: Option<Pickup>,
    /// Equip the pending stack now.
    pub equip: bool,
    /// `on_pickup_item_start`/`_end` to fire, in order.
    pub events: Vec<SelfEvent>,
}

impl GearWork {
    pub fn is_empty(&self) -> bool {
        self.pickup.is_none() && !self.equip && self.events.is_empty()
    }
}

/// A live, non-empty dropped item: its position and stack.
fn item_view(world: &World, item: Entity) -> Option<(DVec3, &ItemStack)> {
    let entity = world.get_entity(item).ok()?;
    let stack = &entity.get::<ItemEntityStack>()?.0;
    if stack.is_empty() {
        return None;
    }
    Some((entity.get::<Position>()?.0, stack))
}

impl GoalContext<'_> {
    /// The gates both `can_start` and `can_continue` apply.
    fn pickup_allowed(&self, goal: &PickupItemsGoal) -> bool {
        if goal.pickup_based_on_chance
            && self.config::<CanPickUpLoot>() != Some(&CanPickUpLoot(true))
        {
            return false;
        }
        if !goal.same_items_as_in_hand && self.config::<ShareablesConfig>().is_none() {
            return false;
        }
        if self.config::<WalkNavigationConfig>().is_none() {
            return false;
        }
        if self
            .world
            .get_resource::<GameRules>()
            .is_some_and(|rules| !rules.mob_griefing)
        {
            return false;
        }
        self.config::<LastHurt>()
            .filter(|hurt| hurt.attacker.is_some())
            .is_none_or(|hurt| hurt.tick + goal.cooldown_ticks <= self.tick)
    }

    /// Whether the mob wants `stack` from the ground.
    fn wants(&self, goal: &PickupItemsGoal, stack: &ItemStack) -> bool {
        if goal.same_items_as_in_hand {
            joins_hand(self.world, self.entity, stack)
        } else {
            will_pickup(self.world, self.entity, stack, goal.can_pickup_any_item)
        }
    }

    /// A candidate that is still on the ground, takeable and wanted.
    fn valid_item(&self, goal: &PickupItemsGoal, item: Entity) -> Option<(DVec3, &ItemStack)> {
        let (position, stack) = item_view(self.world, item)?;
        let ready = self
            .world
            .get::<PickupDelay>(item)
            .is_some_and(|delay| delay.0 == 0);
        (ready && self.wants(goal, stack)).then_some((position, stack))
    }

    fn fetch_candidates(&mut self, goal: &PickupItemsGoal) {
        let feet = self.body.feet;
        let reach = DVec3::new(
            f64::from(goal.max_dist),
            f64::from(goal.search_height),
            f64::from(goal.max_dist),
        );
        let mut found: Vec<(f64, Entity)> = self
            .nearby
            .items_in(feet - reach, feet + reach)
            .filter(|item| self.valid_item(goal, item.entity).is_some())
            .map(|item| (item.feet.distance_squared(feet), item.entity))
            .collect();
        // Farthest first: the nearest is popped first.
        found.sort_by(|a, b| b.0.total_cmp(&a.0));
        let candidates = &mut self.states.pickup.candidates;
        candidates.clear();
        candidates.extend(found.into_iter().map(|(_, item)| item));
    }

    pub(super) fn pickup_can_start(&mut self) -> bool {
        let Some(goal) = self.config::<PickupItemsGoal>() else {
            return false;
        };
        if !self.pickup_allowed(goal) {
            return false;
        }
        if self.states.pickup.candidates.is_empty() {
            self.fetch_candidates(goal);
        }
        let Some(item) = self.states.pickup.candidates.pop() else {
            return false;
        };
        let Some((position, stack)) = self.valid_item(goal, item) else {
            return false;
        };
        if goal.stop_if_holding_item
            && self
                .config::<MobEquipment>()
                .is_some_and(|gear| !gear.get(held_slot(stack.item_id())).is_empty())
        {
            return false;
        }
        if goal
            .excluded
            .iter()
            .any(|m| m.matches(stack.item_id(), stack.damage))
        {
            return false;
        }
        let has_path = self.path_toward(position);
        let accepted = if has_path {
            let end = self
                .states
                .staging
                .waypoints
                .last()
                .map_or(self.body.feet, |w| w.feet);
            let gap = position.distance(end) + self.body.width * 0.5;
            gap * gap < goal.goal_radius_sq
        } else {
            position.distance_squared(self.body.feet) <= goal.goal_radius_sq
        };
        if accepted {
            self.states.pickup.target = Some(item);
            self.states.pickup.staged_path = has_path;
            self.states.pickup.last_target = position;
        }
        accepted
    }

    pub(super) fn pickup_can_continue(&mut self) -> bool {
        let Some(goal) = self.config::<PickupItemsGoal>() else {
            return false;
        };
        self.pickup_allowed(goal)
            && self
                .states
                .pickup
                .target
                .is_some_and(|item| item_view(self.world, item).is_some())
            && !self.nav.is_done()
    }

    pub(super) fn pickup_start(&mut self) {
        let Some(goal) = self.config::<PickupItemsGoal>() else {
            return;
        };
        // The timer is counted down on this tick's goal tick too.
        self.states.pickup.repath_at = self.tick + PICKUP_REPATH_START_TICKS as u64 - 1;
        if std::mem::take(&mut self.states.pickup.staged_path) {
            self.follow_staged(goal.speed_multiplier);
        } else {
            self.nav.stop(self.movement);
        }
        if let Some(event) = &goal.on_start {
            self.effects.gear.events.push(event.clone());
        }
    }

    pub(super) fn pickup_stop(&mut self) {
        self.nav.stop(self.movement);
        self.states.pickup.target = None;
        if let Some(event) = self
            .config::<PickupItemsGoal>()
            .and_then(|goal| goal.on_end.as_ref())
        {
            self.effects.gear.events.push(event.clone());
        }
    }

    pub(super) fn pickup_tick(&mut self) {
        let Some(goal) = self.config::<PickupItemsGoal>() else {
            return;
        };
        let Some(item) = self.states.pickup.target else {
            return;
        };
        let Some((position, stack)) = item_view(self.world, item) else {
            return;
        };
        self.look.look_at(position, PICKUP_HEAD_TURN);
        let distance_sq = position.distance_squared(self.body.feet);
        let state = &self.states.pickup;
        if self.tick >= state.repath_at
            && position.distance_squared(state.last_target) >= goal.goal_radius_sq
            && (goal.track_target || can_see(self.world, self.self_eye(), position))
        {
            let base = if distance_sq > PICKUP_REPATH_FAR_DIST_SQ {
                PICKUP_REPATH_FAR_TICKS
            } else if distance_sq > PICKUP_REPATH_MID_DIST_SQ {
                PICKUP_REPATH_MID_TICKS
            } else {
                PICKUP_REPATH_NEAR_TICKS
            };
            let mut delay = base + self.rng.gen_range(0..PICKUP_REPATH_RANDOM);
            self.states.pickup.last_target = position;
            if self.path_toward(position) {
                self.follow_staged(goal.speed_multiplier);
            } else {
                delay += PICKUP_REPATH_FAIL_TICKS;
            }
            self.states.pickup.repath_at = self.tick + delay as u64;
        }
        if distance_sq <= goal.goal_radius_sq {
            let count = count_to_pickup(self.world, self.entity, stack);
            self.effects.persistent = true;
            self.effects.gear.pickup = Some(Pickup {
                item,
                count,
                consume: consumes(self.world, self.entity, stack),
            });
            self.states.pickup.target = None;
        }
    }

    pub(super) fn equip_can_start(&mut self) -> bool {
        self.config::<EquipItemGoal>().is_some() && self.config::<PendingEquip>().is_some()
    }

    pub(super) fn equip_tick(&mut self) {
        if self.config::<PendingEquip>().is_some() {
            self.effects.gear.equip = true;
        }
    }
}

/// Make the changes the gear goals requested for `mob` this tick.
pub fn finish(world: &mut World, mob: Entity, work: GearWork) {
    if let Some(pickup) = work.pickup {
        take_item(world, mob, pickup);
    }
    if work.equip {
        equip_pending(world, mob);
    }
    for event in work.events {
        let Some(handle) = world
            .get::<MobKind>(mob)
            .and_then(|kind| kind.0.events.event(&event.0))
        else {
            warn!(event = %event.0, "pickup_items names an event the mob does not define");
            continue;
        };
        if let Err(error) = fire_event(world, mob, &handle, None) {
            warn!(event = %event.0, %error, "pickup_items event failed");
        }
    }
}

/// Take `pickup.count` of the item actor: into the pending equip stack
/// unless consumed, telling every player.
fn take_item(world: &mut World, mob: Entity, pickup: Pickup) {
    let (Some(item_rid), Some(mob_rid)) = (
        world.get::<RuntimeId>(pickup.item).map(|id| id.0),
        world.get::<RuntimeId>(mob).map(|id| id.0),
    ) else {
        return;
    };
    let Some(mut stack) = world.get_mut::<ItemEntityStack>(pickup.item) else {
        return;
    };
    let count = pickup.count.min(stack.0.count);
    let mut taken = stack.0.clone();
    taken.count = count;
    stack.0.count -= count;
    let emptied = stack.0.count == 0;
    if emptied {
        stack.0 = ItemStack::empty();
    }
    if count > 0 && !pickup.consume && world.get::<EquipItemConfig>(mob).is_some() {
        world.entity_mut(mob).insert(PendingEquip(taken));
    }
    let packet = McpePacket::from(TakeItemActorPacket {
        item_runtime_id: ActorRuntimeId {
            actor_runtime_id: item_rid as u64,
        },
        actor_runtime_id: ActorRuntimeId {
            actor_runtime_id: mob_rid as u64,
        },
    });
    for session in world.query::<&PlayerSession>().iter(world) {
        let _ = session.send(packet.clone());
    }
    if emptied {
        world.despawn(pickup.item);
    }
}

/// Equip the pending stack of `mob` (`behavior.equip_item`).
fn equip_pending(world: &mut World, mob: Entity) {
    let Some(PendingEquip(mut stack)) = world.entity_mut(mob).take::<PendingEquip>() else {
        return;
    };
    let Some(feet) = world.get::<Position>(mob).map(|p| p.0) else {
        return;
    };
    let (can_wear, excluded) = world
        .get::<EquipItemConfig>(mob)
        .map_or((true, false), |config| {
            (
                config.can_wear_armor,
                config.excludes(stack.item_id(), stack.damage),
            )
        });
    let mut equipment = world
        .entity_mut(mob)
        .take::<MobEquipment>()
        .unwrap_or_default();
    let mut dropped = Vec::new();
    let worn = stack.item().and_then(worn_slot);
    let hand = held_slot(stack.item_id());
    match worn {
        Some(slot) => dropped.push(equipment.take(slot).stack),
        None => {
            let carried = equipment.take(hand).stack;
            if stackable(&carried, &stack) {
                stack.count = carried
                    .count
                    .saturating_add(stack.count)
                    .min(carried.max_stack_size());
            } else {
                dropped.push(carried);
            }
        }
    }
    let slot: GearSlot = match worn {
        Some(slot) if can_wear && !excluded => slot,
        _ => hand,
    };
    dropped.push(equipment.set(slot, stack, true).stack);
    world.entity_mut(mob).insert((equipment, Persistent));
    for stack in dropped {
        spawn_item_drop(world, feet, stack, None);
    }
}
