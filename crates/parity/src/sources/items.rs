//! Item rows: every item in the BDS 1.26.51 item registry
//! (`unastar_data::items`), with a sub-row per server-side behaviour facet.
//!
//! Facets are derived from data, never listed per item: native item
//! properties and tags, the measured BDS tables (fuel, armour, melee), the
//! vanilla item documents (`unastar_data::item_components`) and the actor and
//! block registries (an item that shares a block's identifier places it; one
//! that shares a projectile actor's identifier is thrown).

use std::collections::{BTreeMap, BTreeSet};

use unastar_data::item_components::{self, PackItem};
use unastar_data::{actor_definitions, actor_identifiers, blocks, equipment, fuel, items, melee};

use crate::catalog::Row;

pub fn rows() -> anyhow::Result<Vec<Row>> {
    let pack: BTreeMap<String, PackItem> = item_components::decode_vanilla()
        .map_err(|e| anyhow::anyhow!("vanilla item documents: {e}"))?
        .into_iter()
        .map(|item| (item.identifier.clone(), item))
        .collect();
    let actors: BTreeSet<&str> = actor_identifiers::vanilla()
        .iter()
        .map(|actor| actor.id.as_str())
        .collect();
    let hand_damage = melee::attack_damage(None);

    let mut rows = Vec::new();
    for item in items::vanilla() {
        let id = item.identifier.as_str();
        let pack = pack.get(id);
        let mut facets = BTreeSet::new();
        let components = pack.map(|p| &p.components);
        let pack_has = |f: fn(&item_components::ItemComponents) -> bool| components.is_some_and(f);

        // `minecraft:item.<block>` is the hidden block item of a block whose
        // name a real item also uses (`item.wheat` places the wheat crop).
        let placed_block = id
            .strip_prefix("minecraft:item.")
            .map_or_else(|| id.to_owned(), |block| format!("minecraft:{block}"));
        if blocks::get(&placed_block).is_some()
            || pack_has(|c| c.block_placer.is_some() || c.block.is_some() || c.seed.is_some())
        {
            facets.insert("block_placer");
        }
        if item.has_tag("minecraft:is_food") || pack_has(|c| c.food.is_some()) {
            facets.insert("food");
        }
        if item.max_damage > 0 || pack_has(|c| c.durability.is_some()) {
            facets.insert("durability");
        }
        if item.has_tag("minecraft:digger") || item.mine_block_type != 0 {
            facets.insert("digger");
        }
        if melee::attack_damage(Some(id)) != hand_damage || pack_has(|c| c.damage.is_some()) {
            facets.insert("weapon");
        }
        if equipment::get(id).is_some() || pack_has(|c| c.wearable.is_some()) {
            facets.insert("wearable");
        }
        if fuel::furnace_ticks(id).is_some() || pack_has(|c| c.fuel.is_some()) {
            facets.insert("fuel");
        }
        if item.has_tag("minecraft:spawn_egg") {
            facets.insert("spawn_egg");
        }
        if item.has_tag("minecraft:music_disc") {
            facets.insert("record");
        }
        // Items named like an actor: projectiles are thrown; food and block
        // items that merely share a mob's or primed block's name (cod, slime,
        // tnt) place nothing.
        if actors.contains(id) {
            let projectile = actor_definitions::get(id).is_some_and(|definition| {
                definition.components.contains_key("minecraft:projectile")
            });
            if projectile {
                facets.insert("throwable");
            } else if !facets.contains("food") && !facets.contains("block_placer") {
                facets.insert("entity_placer");
            }
        } else if [
            "minecraft:boat",
            "minecraft:is_minecart",
            "minecraft:hanging_actor",
        ]
        .iter()
        .any(|tag| item.has_tag(tag))
        {
            facets.insert("entity_placer");
        }
        if let Some(facet) = use_facet(id, item.use_animation) {
            facets.insert(facet);
        }
        for (present, facet) in [
            (pack_has(|c| c.projectile.is_some()), "throwable"),
            (pack_has(|c| c.kinetic_weapon.is_some()), "kinetic_weapon"),
            (pack_has(|c| c.piercing_weapon.is_some()), "piercing_weapon"),
            (pack_has(|c| c.storage_item.is_some()), "bundle"),
            (pack_has(|c| c.cooldown.is_some()), "cooldown"),
            (pack_has(|c| c.repairable.is_some()), "repairable"),
            (pack_has(|c| c.enchantable.is_some()), "enchantable"),
            (pack_has(|c| c.compostable.is_some()), "compostable"),
        ] {
            if present {
                facets.insert(facet);
            }
        }

        let row = Row::new(id, family(item, &facets));
        rows.extend(facets.iter().map(|facet| Row::child(&row, facet)));
        rows.push(row);
    }
    Ok(rows)
}

/// The use behaviour named by the native `useAnimation` code. Codes 1 (eat)
/// and 2 (drink) cover food, potions and buckets; buckets are their own facet.
fn use_facet(id: &str, use_animation: i32) -> Option<&'static str> {
    match use_animation {
        2 if id.ends_with("bucket") => Some("bucket"),
        2 => Some("drink"),
        3 => Some("shield"),
        4 | 9 => Some("shooter"),
        6 => Some("throwable"),
        10 => Some("spyglass"),
        11 => Some("instrument"),
        12 => Some("brush"),
        // 1 is eating (the `food` facet); 7 and 8 are Education Edition
        // glow sticks and sparklers.
        _ => None,
    }
}

/// Grouping family: the item's most specific behaviour.
fn family(item: &items::ItemData, facets: &BTreeSet<&str>) -> &'static str {
    const ORDER: [(&str, &str); 14] = [
        ("spawn_egg", "spawn_eggs"),
        ("food", "food"),
        ("wearable", "armour_wearables"),
        ("shooter", "ranged_weapons"),
        ("throwable", "throwables"),
        ("digger", "tools"),
        ("weapon", "melee_weapons"),
        ("bucket", "buckets"),
        ("drink", "potions_drinks"),
        ("entity_placer", "entity_placers"),
        ("record", "music_discs"),
        ("bundle", "bundles"),
        ("durability", "other_tools"),
        ("block_placer", "block_items"),
    ];
    if item.identifier.starts_with("minecraft:element_")
        || item.identifier.contains("chemistry")
        || item.identifier.contains("compound")
    {
        return "education";
    }
    ORDER
        .iter()
        .find(|(facet, _)| facets.contains(facet))
        .map_or("materials_misc", |(_, family)| family)
}
