//! Equipment dropped when a mob dies.
//!
//! Every non-empty slot is considered once, in slot order. Its chance is
//! the authored `slot_drop_chance`, else [`GEAR_DEFAULT_DROP_CHANCE`]; a
//! killing blow from an actor adds [`GEAR_LOOTING_DROP_BONUS`] per Looting
//! level of the item that actor holds. A chance of 0 never drops, except
//! for picked-up items, which always drop as they are. Otherwise the slot
//! drops only when a player killed the mob or the chance is at least 1
//! (guaranteed), on a uniform roll below the chance. A non-guaranteed
//! damageable, breakable drop is worn down: `k` uniform in
//! `0..max(max_damage, GEAR_DROP_MIN_MAX_DAMAGE) - 3`, `r` uniform in
//! `0..=k`, and the item drops with wear `max_damage - r`. Drops pop at the
//! mob's feet like block drops (10-tick pickup delay) and leave the slot.

use bevy_ecs::prelude::*;
use rand::Rng;
use zuri_nbt::NBTTag;

use super::{GearSlot, MobEquipment, mainhand};
use crate::entity::components::Position;
use crate::entity::mob::constants::{
    GEAR_DEFAULT_DROP_CHANCE, GEAR_DROP_MIN_MAX_DAMAGE, GEAR_LOOTING_DROP_BONUS,
};
use crate::entity::mob::events::held_item;
use crate::entity::mob::state::LastHurt;
use crate::entity::{EntityRandom, spawn_item_drop};
use crate::item::ItemStack;
use crate::item::enchant::{Enchantment, enchantment_level, item_tag, with_durability_damage};

/// The item the actor that dealt `mob`'s last hit holds (a player's
/// selected hotbar stack, a mob's main hand).
fn killer_weapon(world: &World, mob: Entity) -> Option<&ItemStack> {
    let killer = world.get::<LastHurt>(mob)?.attacker?;
    held_item(world, killer).or_else(|| mainhand(world, killer))
}

fn unbreakable(stack: &ItemStack) -> bool {
    item_tag(stack).is_some_and(
        |tag| matches!(tag.0.get("Unbreakable"), Some(NBTTag::Byte(flag)) if flag.0 != 0),
    )
}

/// The wear a non-guaranteed drop of `stack` keeps; `None` when the item
/// takes no wear.
fn worn_down(stack: &ItemStack, rng: &mut impl Rng) -> Option<i32> {
    let max_damage = crate::registry::item::item_data(&stack.item_id)?.max_damage;
    if max_damage <= 0 || unbreakable(stack) {
        return None;
    }
    let k = rng.gen_range(0..max_damage.max(GEAR_DROP_MIN_MAX_DAMAGE) - 3);
    let r = rng.gen_range(0..=k);
    Some(max_damage - r)
}

/// Drop `mob`'s equipment at its feet (`killed_by_player`: the death
/// counts as a player kill).
pub fn drop_equipment(world: &mut World, mob: Entity, killed_by_player: bool) {
    let Some(position) = world.get::<Position>(mob).map(|p| p.0) else {
        return;
    };
    let looting = killer_weapon(world, mob)
        .map_or(0, |weapon| enchantment_level(weapon, Enchantment::Looting));
    let Some(mut equipment) = world.get::<MobEquipment>(mob).cloned() else {
        return;
    };
    world.init_resource::<EntityRandom>();
    let mut drops = Vec::new();
    world.resource_scope(|_, mut rng: Mut<EntityRandom>| {
        for slot in GearSlot::ALL {
            let item = equipment.item(slot);
            if item.stack.is_empty() {
                continue;
            }
            let chance = equipment
                .drop_chance(slot)
                .unwrap_or(GEAR_DEFAULT_DROP_CHANCE);
            if chance == 0.0 && !item.picked_up {
                continue;
            }
            let chance = chance + f32::from(looting) * GEAR_LOOTING_DROP_BONUS;
            let guaranteed = chance >= 1.0;
            let stack = if item.picked_up {
                item.stack.clone()
            } else if (killed_by_player || guaranteed) && rng.0.r#gen::<f32>() < chance {
                match (!guaranteed)
                    .then(|| worn_down(&item.stack, &mut rng.0))
                    .flatten()
                {
                    Some(damage) => with_durability_damage(&item.stack, damage),
                    None => item.stack.clone(),
                }
            } else {
                continue;
            };
            equipment.take(slot);
            drops.push(stack);
        }
    });
    world.entity_mut(mob).insert(equipment);
    for stack in drops {
        spawn_item_drop(world, position, stack, None);
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use super::*;
    use crate::entity::components::{DroppedItem, ItemEntityStack};
    use crate::item::enchant::durability_damage;

    fn mob(world: &mut World, slot: GearSlot, stack: ItemStack, picked_up: bool) -> Entity {
        let mut equipment = MobEquipment::default();
        equipment.set(slot, stack, picked_up);
        world
            .spawn((Position(DVec3::new(0.5, 64.0, 0.5)), equipment))
            .id()
    }

    fn drops(world: &mut World) -> Vec<ItemStack> {
        world
            .query_filtered::<&ItemEntityStack, With<DroppedItem>>()
            .iter(world)
            .map(|stack| stack.0.clone())
            .collect()
    }

    #[test]
    fn worn_drops_keep_little_durability() {
        let mut rng = StdRng::seed_from_u64(4);
        let sword = ItemStack::new("minecraft:iron_sword", 1);
        let max = crate::registry::item::item_data("minecraft:iron_sword")
            .unwrap()
            .max_damage;
        for _ in 0..100 {
            let wear = worn_down(&sword, &mut rng).unwrap();
            assert!((4..=max).contains(&wear), "{wear}");
        }
        assert!(worn_down(&ItemStack::new("minecraft:dirt", 1), &mut rng).is_none());
    }

    #[test]
    fn picked_up_items_always_drop_and_others_need_a_player_kill() {
        let mut world = World::new();
        world.insert_resource(EntityRandom::seeded(1));
        let sword = ItemStack::new("minecraft:iron_sword", 1);
        let picked = mob(&mut world, GearSlot::Mainhand, sword.clone(), true);
        drop_equipment(&mut world, picked, false);
        let dropped = drops(&mut world);
        assert_eq!(dropped.len(), 1);
        assert_eq!(durability_damage(&dropped[0]), 0, "dropped as it was");
        assert_eq!(world.get::<MobEquipment>(picked).unwrap().count(), 0);

        let mut kept = 0;
        for _ in 0..50 {
            let rolled = mob(&mut world, GearSlot::Mainhand, sword.clone(), false);
            drop_equipment(&mut world, rolled, false);
            kept += world.get::<MobEquipment>(rolled).unwrap().count();
        }
        assert_eq!(kept, 50, "no player kill, no drop");
        assert_eq!(drops(&mut world).len(), 1);

        let guaranteed = mob(
            &mut world,
            GearSlot::Head,
            ItemStack::new("minecraft:iron_helmet", 1),
            false,
        );
        world
            .get_mut::<MobEquipment>(guaranteed)
            .unwrap()
            .set_drop_chance(GearSlot::Head, Some(1.0));
        drop_equipment(&mut world, guaranteed, false);
        assert_eq!(world.get::<MobEquipment>(guaranteed).unwrap().count(), 0);
        let never = mob(
            &mut world,
            GearSlot::Head,
            ItemStack::new("minecraft:iron_helmet", 1),
            false,
        );
        world
            .get_mut::<MobEquipment>(never)
            .unwrap()
            .set_drop_chance(GearSlot::Head, Some(0.0));
        drop_equipment(&mut world, never, true);
        assert_eq!(world.get::<MobEquipment>(never).unwrap().count(), 1);
    }
}
