//! Item durability: wear from mining, attacking, item use and armour hits,
//! the Unbreaking enchantment, and breaking.
//!
//! An item's durability is its pack `minecraft:durability.max_durability`
//! (spears) or else its native `maxDamage`; items without either never
//! wear. Damage is stored in the item's `Damage` tag like BDS.
//!
//! Melee and mining wear are measured on BDS 1.26.51 (attack and mining
//! sweeps, `unastar-data/data/measured/`): a hit with a sword, trident or
//! mace costs 1, with an axe, pickaxe, shovel or hoe 2, and with an item
//! whose durability is its `minecraft:durability` component (spears) 1;
//! breaking a block is in `mining::mining_wear`. A hit that hurts the player
//! wears every worn piece with armour points by `max(1, floor(damage / 4))`
//! of the damage before armour (BDS armour-wear sweep; the elytra never
//! wears). Unbreaking (BDS Unbreaking sweep, `durability/testdata/
//! bds-unbreaking.json`) spares a point of tool wear with chance
//! `level / (level + 1)` (one point per broken block or item use was
//! measured) and a whole hit's armour wear with chance
//! `0.4 × level / (level + 1)`: each piece wore 0 or 2 points from a hit
//! worth 2, never 1.

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
    } else if entry.components().durability.is_some() {
        // Every hit, whatever the component's `damage_chance` (BDS attack
        // sweep: each spear tier lost 1 on each of three hits).
        1
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
/// when it broke. `None` when nothing changed. Tool wear is spared point by
/// point, armour wear all at once.
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
    let applied = match (level, kind) {
        (0, _) => points,
        (n, WearKind::Tool) => (0..points).filter(|_| rng.gen_range(0..=n) == 0).count() as i32,
        (n, WearKind::Armour) if rng.r#gen::<f32>() < 0.6 + 0.4 / (f32::from(n) + 1.0) => points,
        (_, WearKind::Armour) => 0,
    };
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
    wear_held_sending(world, player, points, true)
}

/// [`wear_held`] without the slot update while the item lasts: the client
/// predicts that wear (BDS sends none after flint and steel lights a block).
pub fn wear_held_predicted(world: &mut World, player: Entity, points: i32) -> bool {
    wear_held_sending(world, player, points, false)
}

fn wear_held_sending(world: &mut World, player: Entity, points: i32, send: bool) -> bool {
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
    let Some(worn) = wear(
        &stack,
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
    if send || broke {
        super::send_slot(world, player, slot, &worn);
    }
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
        // Only a piece that gives armour points wears (BDS armour-wear
        // sweep: the elytra, measured with no defence, never does).
        if piece.is_empty()
            || unastar_data::equipment::get(piece.item_id())
                .is_none_or(|values| values.defense <= 0.0)
        {
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
        assert_eq!(attack_wear(&entry("minecraft:mace")), 1);
        // A spear's durability is its pack component: one per hit.
        assert_eq!(attack_wear(&entry("minecraft:wooden_spear")), 1);
        assert_eq!(attack_wear(&entry("minecraft:stick")), 0);
        assert_eq!(attack_wear(&entry("minecraft:bow")), 0);
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

    /// `durability/testdata/bds-unbreaking.json`.
    #[derive(serde::Deserialize)]
    struct BdsUnbreaking {
        mining: std::collections::BTreeMap<u8, Bernoulli>,
        armour: std::collections::BTreeMap<u8, ArmourHits>,
        #[serde(rename = "use")]
        item_use: std::collections::BTreeMap<u8, Bernoulli>,
    }

    /// Wear events and how many of them cost a point.
    #[derive(serde::Deserialize)]
    struct Bernoulli {
        samples: u32,
        worn: u32,
    }

    /// Per armour piece, the hits that wore it by 0, 1 and 2 points.
    #[derive(serde::Deserialize)]
    struct ArmourHits {
        hits: u32,
        pieces: std::collections::BTreeMap<String, [u32; 3]>,
    }

    /// Whether `ours` of `n` and `theirs` of `m` can be the same share: their
    /// difference within four standard errors of the pooled share.
    fn same_share(ours: u32, n: u32, theirs: u32, m: u32) -> bool {
        let (ours, n, theirs, m) = (
            f64::from(ours),
            f64::from(n),
            f64::from(theirs),
            f64::from(m),
        );
        let pooled = (ours + theirs) / (n + m);
        let error = (pooled * (1.0 - pooled) * (1.0 / n + 1.0 / m)).sqrt();
        (ours / n - theirs / m).abs() <= 4.0 * error.max(1e-9)
    }

    /// Unbreaking against the BDS sweep: a pickaxe breaking stone and flint
    /// and steel lighting fire (one point each), four diamond armour pieces
    /// hit for 2 points each, at levels 0-III, rolled from a seeded RNG.
    #[test]
    fn unbreaking_matches_the_bds_sweep() {
        let bds: BdsUnbreaking =
            serde_json::from_str(include_str!("durability/testdata/bds-unbreaking.json")).unwrap();
        let mut rng = StdRng::seed_from_u64(26);
        let rolls = 20_000;
        let mut failures = Vec::new();
        let mut points_worn = |name: &str, level: u8, points: i32, kind: WearKind| {
            let item = stack(name, 1);
            let item = if level > 0 {
                with_enchantments(&item, &[(Enchantment::Unbreaking, level)])
            } else {
                item
            };
            let mut counts = [0u32; 3];
            for _ in 0..rolls {
                let worn =
                    wear(&item, points, kind, &mut rng).map_or(0, |out| durability_damage(&out));
                counts[worn as usize] += 1;
            }
            counts
        };
        for (what, name, table) in [
            ("mining", "minecraft:netherite_pickaxe", &bds.mining),
            ("use", "minecraft:flint_and_steel", &bds.item_use),
        ] {
            for (&level, row) in table {
                let ours = points_worn(name, level, 1, WearKind::Tool)[1];
                if !same_share(ours, rolls, row.worn, row.samples) {
                    failures.push(format!(
                        "{what} {level}: ours {ours}/{rolls}, BDS {}/{}",
                        row.worn, row.samples
                    ));
                }
            }
        }
        for (&level, row) in &bds.armour {
            let ours = points_worn("minecraft:diamond_chestplate", level, 2, WearKind::Armour);
            for (piece, theirs) in &row.pieces {
                for points in 0..3 {
                    if !same_share(ours[points], rolls, theirs[points], row.hits) {
                        failures.push(format!(
                            "armour {level} {piece}: {points} points ours {}/{rolls}, BDS {}/{}",
                            ours[points], theirs[points], row.hits
                        ));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
