//! Campfires and soul campfires (`extinguished`,
//! `minecraft:cardinal_direction`; the `Campfire` block actor keeps four
//! item slots `Item1`..`Item4` and their cooking times `ItemTime1`..
//! `ItemTime4`).
//!
//! - **Placement** faces the campfire the way the placing player looks.
//! - **Use**:
//!   - a shovel puts out a lit campfire with `extinguish.fire` at the
//!     block's corner (data −1) and one point of wear;
//!   - flint and steel or a fire charge lights an unlit one with `ignite`
//!     at the block's corner (data −1; provisional: the position follows
//!     putting out); flint and steel wears by one, a fire charge is used up
//!     outside creative;
//!   - an item with a campfire recipe (the vanilla `minecraft:recipe_furnace`
//!     rows tagged `campfire` / `soul_campfire`) goes into the first free
//!     slot, one at a time, used up outside creative, with LevelEvent 1040
//!     at the block's corner (data 0);
//!   - anything else is not handled.
//! - **Cooking** runs only while the campfire is lit: every slot's time
//!   rises by one per tick, and on the 600th tick the slot's result drops at
//!   the campfire's centre and the slot empties. Putting the fire out pauses
//!   the times; lighting it resumes them. The actor always lists all four
//!   `ItemTime`s.
//! - **Breaking**: a player breaking the campfire drops what lies on it.
//!
//! Signal fires (a hay bale below) only change the client's smoke; there
//! is no state for them. Splash water and burning arrows belong to the
//! projectile lane, which puts out or lights a campfire with [`set_lit`].
//! Not modelled: the crackle ambient sound (about one in 50 ticks at
//! `(x + 0.5, y + 0.2, z + 0.5)`) and smoke particles, rain, entities
//! taking fire damage on it (entity lane).

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use unastar_data::recipes::{Ingredient, RecipeKind};
use zuri_nbt::{NBTTag, tag};

use super::super::{Place, actors, bit, schedule_tick, with};
use crate::ecs::resources::TickCounter;
use crate::entity::components::{GameMode, PhysicsBody};
use crate::entity::systems::BlockSource;
use crate::item::ItemStack;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::blocks::set_block;
use crate::server::game::durability::wear_held;
use crate::server::game::item_use::{held, set_slot};
use crate::server::game::placement::Horizontal;
use crate::server::game::types::{
    BlockRegistryResource, ItemRegistryResource, RecipeRegistryResource,
};

/// State property.
pub const EXTINGUISHED: &str = "extinguished";
/// The campfire's block actor id.
pub const ACTOR: &str = "Campfire";
/// Item slots.
pub const SLOTS: usize = 4;
/// Ticks one item cooks.
pub const COOK_TICKS: i32 = 600;
/// LevelEvent when an item is put on the campfire.
pub const LEVEL_EVENT_ITEM_ADDED: i32 = 1040;

/// Lit or not, and since which tick, for each campfire whose times have
/// been brought up to date.
#[derive(Resource, Debug, Default)]
pub struct Cooking(HashMap<IVec3, (u64, bool)>);

fn now(world: &World) -> u64 {
    world.get_resource::<TickCounter>().map_or(0, |t| t.current)
}

/// What lies on a campfire: each slot's item and cooking time.
type Slots = [Option<(ItemStack, i32)>; SLOTS];

/// The items on the campfire at `pos`.
pub fn slots(world: &World, pos: IVec3) -> Slots {
    let mut out: Slots = Default::default();
    let Some(actor) = actors::read_kind(world, pos, ACTOR) else {
        return out;
    };
    let items = &world.resource::<ItemRegistryResource>().0;
    let blocks = &world.resource::<BlockRegistryResource>().0;
    for (index, slot) in out.iter_mut().enumerate() {
        let Some(NBTTag::Compound(item)) = actor.0.get(&format!("Item{}", index + 1)) else {
            continue;
        };
        let Some(stack) = crate::block_entity::item_from_nbt(item, items, blocks) else {
            continue;
        };
        let time = match actor.0.get(&format!("ItemTime{}", index + 1)) {
            Some(NBTTag::Int(time)) => time.0,
            _ => 0,
        };
        *slot = Some((stack, time));
    }
    out
}

fn store_slots(world: &mut World, pos: IVec3, slots: &Slots) {
    let mut actor = actors::new_compound(ACTOR, pos);
    for (index, slot) in slots.iter().enumerate() {
        let time = slot.as_ref().map_or(0, |(_, time)| *time);
        if let Some((stack, _)) = slot {
            actor.0.insert(
                format!("Item{}", index + 1),
                NBTTag::Compound(crate::block_entity::item_to_nbt(stack)),
            );
        }
        actor.0.insert(
            format!("ItemTime{}", index + 1),
            NBTTag::Int(tag::Int(time)),
        );
    }
    actors::store(world, pos, actor);
}

fn is_lit(world: &World, state: u32) -> bool {
    world
        .registry()
        .get_by_runtime_id(state)
        .is_some_and(|entry| !bit(entry, state, EXTINGUISHED))
}

/// The recipe tag of the campfire `state`.
fn recipe_tag(world: &World, state: u32) -> &'static str {
    match world.registry().get_by_runtime_id(state) {
        Some(entry) if entry.string_id == "minecraft:soul_campfire" => "soul_campfire",
        _ => "campfire",
    }
}

/// What `stack` cooks into on a campfire with recipe tag `tag`.
pub fn cooked(world: &World, tag: &str, stack: &ItemStack) -> Option<ItemStack> {
    let recipes = &world.get_resource::<RecipeRegistryResource>()?.0;
    let entry = stack.item()?;
    let output = recipes.recipes().iter().find_map(|recipe| {
        if !recipe.tags.iter().any(|t| t == tag) {
            return None;
        }
        let RecipeKind::Furnace { input, output } = &recipe.kind else {
            return None;
        };
        let matches = match input {
            Ingredient::Item { name, data, .. } => {
                name == entry.identifier() && data.is_none_or(|data| data == stack.damage)
            }
            Ingredient::Tag { tag, .. } => entry.has_tag(tag),
        };
        matches.then_some(output)
    })?;
    crate::server::game::world_stack(world, &output.name, output.count)
        .map(|stack| stack.with_damage(output.data))
}

/// Bring the cooking times of the campfire at `pos` up to now, finish what
/// is done, remember whether it burns from now on and schedule the next
/// finishing tick.
fn advance(world: &mut World, pos: IVec3, state: u32) {
    let at = now(world);
    let lit = is_lit(world, state);
    let (since, was_lit) = world
        .get_resource::<Cooking>()
        .and_then(|cooking| cooking.0.get(&pos).copied())
        .unwrap_or((at, lit));
    let elapsed = i32::try_from(at.saturating_sub(since)).unwrap_or(i32::MAX);
    let mut slots = slots(world, pos);
    let mut changed = false;
    let mut done = Vec::new();
    if was_lit && elapsed > 0 {
        for slot in &mut slots {
            if let Some((stack, time)) = slot {
                *time = time.saturating_add(elapsed);
                changed = true;
                if *time >= COOK_TICKS {
                    done.push(stack.clone());
                    *slot = None;
                }
            }
        }
    }
    if changed {
        store_slots(world, pos, &slots);
    }
    let tag = recipe_tag(world, state);
    for raw in done {
        if let Some(result) = cooked(world, tag, &raw) {
            let feet = pos.as_dvec3() + DVec3::new(0.5, 0.5 - PhysicsBody::ITEM.height / 2.0, 0.5);
            crate::entity::spawn_item_drop(world, feet, result, None);
        }
    }
    world
        .get_resource_or_init::<Cooking>()
        .0
        .insert(pos, (at, lit));
    if lit
        && let Some(left) = slots
            .iter()
            .flatten()
            .map(|(_, time)| COOK_TICKS - time)
            .min()
    {
        schedule_tick(world, pos, u64::try_from(left.max(1)).unwrap_or(1));
    }
}

/// Whether a player's `use` on `state` is a block interaction: always.
pub fn is_usable(_entry: &BlockEntry, _state: u32) -> bool {
    true
}

/// A player's `use`: put out, light, or put food on the campfire.
pub fn use_block(world: &mut World, player: Entity, pos: IVec3, state: u32) -> bool {
    let Some((slot, stack)) = held(world, player) else {
        return false;
    };
    let Some(item) = stack.item().cloned() else {
        return false;
    };
    let lit = is_lit(world, state);
    let lights = matches!(
        item.identifier(),
        "minecraft:flint_and_steel" | "minecraft:fire_charge"
    );
    if item.has_tag("minecraft:is_shovel") && lit {
        set_fire(world, pos, state, false, "extinguish.fire");
        wear_held(world, player, 1);
        return true;
    }
    if lights && !lit {
        set_fire(world, pos, state, true, "ignite");
        if item.identifier() == "minecraft:flint_and_steel" {
            wear_held(world, player, 1);
        } else if world.get::<GameMode>(player) != Some(&GameMode::Creative) {
            set_slot(world, player, slot, stack.grow(-1));
        }
        return true;
    }
    if cooked(world, recipe_tag(world, state), &stack).is_none() {
        return false;
    }
    advance(world, pos, state);
    let mut slots = slots(world, pos);
    let Some(free) = slots.iter_mut().find(|slot| slot.is_none()) else {
        return false;
    };
    *free = Some((stack.clone().with_count(1), 0));
    store_slots(world, pos, &slots);
    advance(world, pos, state);
    crate::entity::mob::net::sound::level_event(world, pos.as_dvec3(), LEVEL_EVENT_ITEM_ADDED, 0);
    if world.get::<GameMode>(player) != Some(&GameMode::Creative) {
        set_slot(world, player, slot, stack.grow(-1));
    }
    true
}

/// Light or put out the campfire `state` at `pos` with `sound` at its
/// corner; the cooking times follow in [`placed`].
fn set_fire(world: &mut World, pos: IVec3, state: u32, lit: bool, sound: &str) {
    let Some(entry) = world.registry().get_by_runtime_id(state).cloned() else {
        return;
    };
    let next = with(
        &entry,
        state,
        EXTINGUISHED,
        StateValue::Byte(u8::from(!lit)),
    );
    set_block(world, pos, next, false);
    crate::server::game::blocks::queue_level_sound(world, None, pos.as_dvec3(), sound, -1);
}

/// Put out (`lit` false, `extinguish.fire`) or light (`ignite`) the campfire
/// at `pos` (splash water, burning arrows). Returns whether it changed.
pub fn set_lit(world: &mut World, pos: IVec3, lit: bool) -> bool {
    let Some(state) = world.block_at(pos) else {
        return false;
    };
    let is_campfire = world
        .registry()
        .get_by_runtime_id(state)
        .is_some_and(|entry| {
            matches!(
                entry.string_id.as_str(),
                "minecraft:campfire" | "minecraft:soul_campfire"
            )
        });
    if !is_campfire || is_lit(world, state) == lit {
        return false;
    }
    let sound = if lit { "ignite" } else { "extinguish.fire" };
    set_fire(world, pos, state, lit, sound);
    true
}

/// Placement: lit, facing the way the placing player looks.
pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    let looking = Horizontal::from_yaw(place.input.yaw);
    Some(with(
        place.block,
        place.state,
        "minecraft:cardinal_direction",
        StateValue::Str(looking.cardinal()),
    ))
}

/// A campfire stands anywhere.
pub fn can_survive(
    _blocks: &impl BlockSource,
    _pos: IVec3,
    _state: u32,
    _entry: &BlockEntry,
) -> bool {
    true
}

/// The campfire was set (placed, lit or put out): the cooking times catch
/// up under the old fire and continue under the new one.
pub fn placed(world: &mut World, pos: IVec3, state: u32) {
    if actors::read_kind(world, pos, ACTOR).is_none() {
        store_slots(world, pos, &Default::default());
    }
    advance(world, pos, state);
}

/// Neighbours do not affect a campfire.
pub fn neighbour_changed(_world: &mut World, _pos: IVec3, _state: u32, _from: IVec3) {}

/// An item finishes cooking.
pub fn scheduled_tick(world: &mut World, pos: IVec3, state: u32) {
    advance(world, pos, state);
}

/// Campfires have no random tick.
pub fn random_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Redstone power does not act on campfires.
pub fn power_changed(_world: &mut World, _pos: IVec3, _state: u32, _powered: bool) {}

/// A player breaks the campfire: what lies on it drops.
pub fn breaking(world: &mut World, pos: IVec3, _state: u32) {
    for (stack, _) in slots(world, pos).into_iter().flatten() {
        crate::server::game::block_drops::pop_resource(world, pos, stack);
    }
    store_slots(world, pos, &Default::default());
    if let Some(mut cooking) = world.get_resource_mut::<Cooking>() {
        cooking.0.remove(&pos);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::{DVec3, IVec3};

    use super::super::super::tests::{FLOOR, Harness, state};
    use super::*;
    use crate::registry::block::StateValue;

    const AT: IVec3 = IVec3::new(2, FLOOR + 1, 2);
    const TOP: DVec3 = DVec3::new(0.5, 0.5, 0.5);

    fn harness() -> Harness {
        let mut h = Harness::new();
        let recipes = unastar_data::recipes::RecipeRegistry::vanilla().expect("vanilla recipes");
        h.world
            .insert_resource(RecipeRegistryResource(Arc::new(recipes)));
        h.put(AT, state("minecraft:campfire", &[]));
        h
    }

    #[test]
    fn cooks_raw_beef_into_steak_after_600_ticks() {
        let mut h = harness();
        h.hold("minecraft:beef", 2);
        h.click(AT, 1, TOP);
        let held = crate::server::game::item_use::held(&h.world, h.player)
            .unwrap()
            .1;
        assert_eq!(held.count, 1);
        assert_eq!(
            slots(&h.world, AT)[0]
                .as_ref()
                .map(|(s, _)| s.item_id().to_owned()),
            Some("minecraft:beef".into())
        );
        h.tick(599);
        assert!(h.drops().is_empty());
        h.tick(1);
        assert_eq!(h.drops(), vec![("minecraft:cooked_beef".to_string(), 1)]);
        assert!(slots(&h.world, AT).iter().all(Option::is_none));
    }

    #[test]
    fn a_shovel_puts_it_out_and_cooking_pauses() {
        let mut h = harness();
        h.hold("minecraft:beef", 1);
        h.click(AT, 1, TOP);
        h.tick(100);
        h.hold("minecraft:wooden_shovel", 1);
        h.click(AT, 1, TOP);
        assert_eq!(h.value(AT, EXTINGUISHED), Some(StateValue::Byte(1)));
        h.tick(1000);
        assert!(h.drops().is_empty());
        h.hold("minecraft:flint_and_steel", 1);
        h.click(AT, 1, TOP);
        assert_eq!(h.value(AT, EXTINGUISHED), Some(StateValue::Byte(0)));
        h.tick(499);
        assert!(h.drops().is_empty());
        h.tick(1);
        assert_eq!(h.drops(), vec![("minecraft:cooked_beef".to_string(), 1)]);
    }

    #[test]
    fn uncookable_items_and_full_campfires_take_nothing() {
        let mut h = harness();
        h.hold("minecraft:stone", 1);
        h.click(AT, 1, TOP);
        assert!(slots(&h.world, AT).iter().all(Option::is_none));
        h.hold("minecraft:chicken", 5);
        for _ in 0..5 {
            h.click(AT, 1, TOP);
        }
        assert!(slots(&h.world, AT).iter().all(Option::is_some));
        let held = crate::server::game::item_use::held(&h.world, h.player)
            .unwrap()
            .1;
        assert_eq!(held.count, 1);
    }

    #[test]
    fn breaking_drops_what_lies_on_it() {
        let mut h = harness();
        h.hold("minecraft:potato", 1);
        h.click(AT, 1, TOP);
        h.break_at(AT);
        let drops = h.drops();
        assert!(drops.contains(&("minecraft:potato".to_string(), 1)));
    }
}
