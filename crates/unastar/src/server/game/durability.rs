//! Item durability: wear from mining, attacking, item use and armour hits,
//! the Unbreaking enchantment, and breaking.
//!
//! An item's durability is its pack `minecraft:durability.max_durability`
//! (spears) or else its native `maxDamage`; items without either never
//! wear. Damage is stored in the item's `Damage` tag like BDS.
//!
//! Wear amounts that are not item data (public references, recorded in
//! `bds-quirks.md` until measured): a hit with a sword, trident or mace
//! costs 1, with an axe, pickaxe, shovel or hoe 2; armour loses
//! `max(1, damage / 4)` per worn piece for damage armour reduces.
//! Unbreaking spares a tool point with chance `level / (level + 1)` and an
//! armour point with chance `0.4 × level / (level + 1)`.

use bevy_ecs::prelude::*;
use rand::Rng;
use unastar_data::actor_components::body::ActorDamageCause;

use crate::entity::components::{ArmourInventory, GameMode, HeldSlot, MainInventory, Position};
use crate::item::ItemStack;
use crate::item::enchant::{
    Enchantment, durability_damage, enchantment_level, with_durability_damage,
};
use crate::registry::item::ItemEntry;

/// The durability of `entry`, 0 when it does not wear.
pub fn max_durability(entry: &ItemEntry) -> i32 {
    entry
        .components()
        .durability
        .as_ref()
        .map_or(entry.data().max_damage, |d| d.max_durability)
        .max(0)
}

/// Durability a melee hit with `entry` costs.
pub fn attack_wear(entry: &ItemEntry) -> i32 {
    if max_durability(entry) == 0 {
        return 0;
    }
    if entry.has_tag("minecraft:is_sword")
        || entry.has_tag("minecraft:is_trident")
        || entry.identifier() == "minecraft:mace"
    {
        1
    } else if ["axe", "pickaxe", "shovel", "hoe"]
        .iter()
        .any(|kind| entry.has_tag(&format!("minecraft:is_{kind}")))
    {
        2
    } else {
        0
    }
}

/// Whether an armour piece loses durability for a hit of `cause`.
pub fn armour_wears_from(cause: ActorDamageCause) -> bool {
    matches!(
        cause,
        ActorDamageCause::EntityAttack
            | ActorDamageCause::Projectile
            | ActorDamageCause::EntityExplosion
            | ActorDamageCause::BlockExplosion
            | ActorDamageCause::Contact
            | ActorDamageCause::Thorns
            | ActorDamageCause::FallingBlock
            | ActorDamageCause::Anvil
    )
}

/// How an Unbreaking level spares wear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WearKind {
    Tool,
    Armour,
}

/// `points` of wear on `stack` after Unbreaking: the worn stack, or empty
/// when it broke. `None` when nothing changed.
pub fn wear(
    stack: &ItemStack,
    points: i32,
    kind: WearKind,
    rng: &mut impl Rng,
) -> Option<ItemStack> {
    let max = stack.item().map(max_durability).filter(|max| *max > 0)?;
    if points <= 0 {
        return None;
    }
    let level = enchantment_level(stack, Enchantment::Unbreaking);
    let applied = (0..points)
        .filter(|_| match (level, kind) {
            (0, _) => true,
            (n, WearKind::Tool) => rng.gen_range(0..=n) == 0,
            (n, WearKind::Armour) => rng.r#gen::<f32>() < 0.6 + 0.4 / (f32::from(n) + 1.0),
        })
        .count() as i32;
    if applied == 0 {
        return None;
    }
    let damage = durability_damage(stack) + applied;
    Some(if damage >= max {
        ItemStack::empty()
    } else {
        with_durability_damage(stack, damage)
    })
}

/// Wear the item in the player's selected hotbar slot. Creative players'
/// items never wear. Returns whether the item broke.
pub fn wear_held(world: &mut World, player: Entity, points: i32) -> bool {
    if world.get::<GameMode>(player) == Some(&GameMode::Creative) {
        return false;
    }
    let Some(slot) = world.get::<HeldSlot>(player).map(|s| usize::from(s.0)) else {
        return false;
    };
    let Some(stack) = world
        .get::<MainInventory>(player)
        .and_then(|inv| inv.0.item(slot).cloned())
        .filter(|s| !s.is_empty())
    else {
        return false;
    };
    wear_slot(world, player, slot, &stack, points)
}

/// Wear the captured original slot only while it still contains that stack.
/// A changed selection never transfers a completed operation's cost to another
/// item. The caller additionally validates its originating actor identity.
pub(super) fn wear_slot(
    world: &mut World,
    player: Entity,
    slot: usize,
    stack: &ItemStack,
    points: i32,
) -> bool {
    if world.get::<GameMode>(player) == Some(&GameMode::Creative)
        || world
            .get::<MainInventory>(player)
            .and_then(|inv| inv.0.item(slot))
            != Some(stack)
    {
        return false;
    }
    let Some(worn) = wear(
        stack,
        points,
        WearKind::Tool,
        &mut world
            .get_resource_or_init::<crate::entity::EntityRandom>()
            .0,
    ) else {
        return false;
    };
    let broke = worn.is_empty();
    if let Some(mut inventory) = world.get_mut::<MainInventory>(player) {
        let _ = inventory.0.set_item(slot, worn.clone());
    }
    super::send_slot(world, player, slot, &worn);
    if broke {
        item_broke(world, player);
    }
    broke
}

/// A hit of `damage` wears every worn armour piece.
pub fn wear_armour(world: &mut World, player: Entity, damage: f32, cause: ActorDamageCause) {
    if !armour_wears_from(cause)
        || world.get::<GameMode>(player) == Some(&GameMode::Creative)
        || damage <= 0.0
    {
        return;
    }
    let points = ((damage / 4.0).floor() as i32).max(1);
    let Some(pieces) = world
        .get::<ArmourInventory>(player)
        .map(|a| a.slots().to_vec())
    else {
        return;
    };
    let mut broke = false;
    for (slot, piece) in pieces.iter().enumerate() {
        if piece.is_empty() {
            continue;
        }
        let Some(worn) = wear(
            piece,
            points,
            WearKind::Armour,
            &mut world
                .get_resource_or_init::<crate::entity::EntityRandom>()
                .0,
        ) else {
            continue;
        };
        broke |= worn.is_empty();
        if let Some(mut armour) = world.get_mut::<ArmourInventory>(player) {
            let _ = armour.set_slot(slot, worn);
        }
        super::inventory::send_armour_slot(world, player, slot);
    }
    if broke {
        item_broke(world, player);
    }
}

/// The item break sound at the player.
fn item_broke(world: &mut World, player: Entity) {
    if let Some(at) = world.get::<Position>(player).map(|p| p.0) {
        crate::entity::mob::net::sound::play_level_sound(world, Some(player), at, "break", -1);
    }
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use super::*;
    use crate::item::enchant::with_enchantments;
    use crate::item::test_support::stack;

    fn entry(name: &str) -> ItemEntry {
        stack(name, 1).item().unwrap().clone()
    }

    #[test]
    fn durability_comes_from_the_pack_component_or_native_max_damage() {
        assert_eq!(max_durability(&entry("minecraft:diamond_pickaxe")), 1561);
        assert_eq!(max_durability(&entry("minecraft:bow")), 384);
        let spear = entry("minecraft:iron_spear");
        assert_eq!(
            max_durability(&spear),
            spear
                .components()
                .durability
                .as_ref()
                .unwrap()
                .max_durability
        );
        assert_eq!(max_durability(&entry("minecraft:stone")), 0);
    }

    #[test]
    fn swords_wear_one_and_tools_two_per_hit() {
        assert_eq!(attack_wear(&entry("minecraft:iron_sword")), 1);
        assert_eq!(attack_wear(&entry("minecraft:iron_axe")), 2);
        assert_eq!(attack_wear(&entry("minecraft:wooden_hoe")), 2);
        assert_eq!(attack_wear(&entry("minecraft:trident")), 1);
        assert_eq!(attack_wear(&entry("minecraft:stick")), 0);
    }

    #[test]
    fn worn_out_items_break() {
        let mut rng = StdRng::seed_from_u64(1);
        let hoe = stack("minecraft:wooden_hoe", 1);
        let max = max_durability(hoe.item().unwrap());
        let almost = with_durability_damage(&hoe, max - 2);
        let once = wear(&almost, 1, WearKind::Tool, &mut rng).unwrap();
        assert_eq!(durability_damage(&once), max - 1);
        assert!(wear(&once, 1, WearKind::Tool, &mut rng).unwrap().is_empty());
        assert!(wear(&stack("minecraft:stone", 1), 1, WearKind::Tool, &mut rng).is_none());
    }

    #[test]
    fn unbreaking_spares_tools_more_than_armour() {
        let worn = |name: &str, points, kind| {
            let item = with_enchantments(&stack(name, 1), &[(Enchantment::Unbreaking, 3)]);
            let out = wear(&item, points, kind, &mut StdRng::seed_from_u64(9)).unwrap();
            durability_damage(&out)
        };
        // Tools keep 1/4 of the points; armour 0.6 + 0.4/4 = 70 %.
        let tool = worn("minecraft:netherite_pickaxe", 1000, WearKind::Tool);
        assert!((200..300).contains(&tool), "{tool}");
        let armour = worn("minecraft:netherite_chestplate", 400, WearKind::Armour);
        assert!((250..310).contains(&armour), "{armour}");
    }
}
