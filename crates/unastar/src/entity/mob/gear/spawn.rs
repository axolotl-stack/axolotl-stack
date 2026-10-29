//! Gear a mob starts with.
//!
//! `minecraft:equipment` rolls its table when the component is added to a
//! spawned, born or event-changed mob (never for a mob restored from
//! storage, which keeps its saved gear): every rolled stack replaces the
//! slot it belongs in: armour and head-wearable blocks by their slot, items
//! that may go in the off hand to the off hand, the rest to the main hand.
//! The authored `slot_drop_chance` entries are recorded on the mob either
//! way.

use bevy_ecs::prelude::*;
use jolyne::valentine::actor_flags;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use tracing::warn;

use super::{CanPickUpLoot, GearSlot, MobEquipment, worn_slot};
use crate::entity::EntityRandom;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::components::EquipmentConfig;
use crate::entity::mob::constants::PICKUP_LOOT_CHANCE;
use crate::item::ItemStack;
use crate::server::game::{LootContext, LootLevel, loot_stacks};

/// The slot a stack rolled from a gear table goes to.
fn table_slot(item_id: &str) -> GearSlot {
    if let Some(slot) = worn_slot(item_id) {
        return slot;
    }
    if crate::registry::item::item_data(item_id).is_some_and(|item| item.allow_offhand) {
        GearSlot::Offhand
    } else {
        GearSlot::Mainhand
    }
}

/// Record `config`'s drop chances on `mob` and, with `roll`, equip what its
/// table rolls.
pub fn equip_from_table(world: &mut World, mob: Entity, config: &EquipmentConfig, roll: bool) {
    let mut equipment = world
        .entity_mut(mob)
        .take::<MobEquipment>()
        .unwrap_or_default();
    for slot in GearSlot::ALL {
        if let Some(chance) = config.drop_chances[slot as usize] {
            equipment.set_drop_chance(slot, Some(chance));
        }
    }
    if roll && let Some(name) = &config.table {
        for stack in roll_table(world, mob, name) {
            equipment.set(table_slot(&stack.item_id), stack, false);
        }
    }
    world.entity_mut(mob).insert(equipment);
}

/// Roll the gear table `name` for `mob` with the level's difficulty facts.
fn roll_table(world: &mut World, mob: Entity, name: &str) -> Vec<ItemStack> {
    let Some(table) = unastar_data::loot::get(name) else {
        warn!(table = %name, "Gear table is not in the loot registry");
        return Vec::new();
    };
    let seed = world
        .get_resource_or_init::<EntityRandom>()
        .0
        .r#gen::<u64>();
    let ctx = LootContext {
        is_baby: world
            .get::<ActorFlags>(mob)
            .is_some_and(|flags| flags.get(actor_flags::BABY).unwrap_or(false)),
        level: LootLevel::of(world),
        ..LootContext::default()
    };
    match unastar_rules::loot::roll(
        unastar_data::loot::vanilla(),
        table,
        &ctx,
        &mut StdRng::seed_from_u64(seed),
    ) {
        Ok(drops) => loot_stacks(world, drops),
        Err(error) => {
            warn!(table = %name, %error, "Gear table cannot be evaluated");
            Vec::new()
        }
    }
}

/// The `pickup_based_on_chance` roll, made once per mob: it may pick items
/// up when a uniform roll is below [`PICKUP_LOOT_CHANCE`] times the clamped
/// regional difficulty.
pub fn roll_loot_pickup(world: &mut World, mob: Entity) {
    if world.get::<CanPickUpLoot>(mob).is_some() {
        return;
    }
    let limit =
        PICKUP_LOOT_CHANCE * LootLevel::of(world).map_or(0.0, |l| l.clamped_regional_difficulty);
    let roll: f32 = world.get_resource_or_init::<EntityRandom>().0.r#gen();
    world.entity_mut(mob).insert(CanPickUpLoot(roll < limit));
}
