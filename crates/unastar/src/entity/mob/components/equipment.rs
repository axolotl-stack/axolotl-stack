//! Equipment components: `minecraft:equipment` (spawn gear and drop
//! chances), `minecraft:equip_item`, `minecraft:shareables` and the goals
//! `minecraft:behavior.pickup_items` / `minecraft:behavior.equip_item`.
//! The runtime lives in `mob::gear` (slots, spawn roll, drops,
//! replication, persistence) and `mob::ai::pickup` (the goals).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body::{self, EventTrigger, ItemDescriptor};

use super::common::{
    ItemMatcher, ItemNameError, SelfEvent, SelfEventError, UnknownLootTable, insert, loot_table,
    self_event,
};
use super::hostile::controls;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::goal_selector::GoalControls;
use crate::entity::mob::constants::TICKS_PER_SECOND;
use crate::entity::mob::gear::{self, GearSlot};
use crate::entity::mob::persistence::Restoring;
use crate::registry::ItemRegistry;
use crate::registry::item_tags::{TagExpressionError, items_matching};

/// Why an equipment body cannot be compiled.
#[derive(Debug, Error)]
pub enum EquipmentError {
    #[error(transparent)]
    Table(#[from] UnknownLootTable),
    #[error(transparent)]
    Item(#[from] ItemNameError),
    #[error(transparent)]
    Event(#[from] SelfEventError),
    #[error("item descriptor {0:?} is neither an item name nor a tag expression")]
    ItemDescriptor(String),
    #[error("item descriptor tags: {0}")]
    Tags(#[from] TagExpressionError),
    #[error("`slot.armor.body` drop chances need body armour, which no supported mob wears")]
    BodySlot,
    #[error(
        "shareables `{0}` needs an inventory, admiring or bartering, none of which is implemented"
    )]
    ShareableFeature(&'static str),
}

impl ComponentError for EquipmentError {}

/// `minecraft:equipment`: the gear table (loot registry key) rolled when the
/// component is added, and the authored drop chance per slot (`None`: the
/// native default).
#[derive(Component, Debug, Clone, PartialEq)]
pub struct EquipmentConfig {
    pub table: Option<Arc<str>>,
    pub drop_chances: [Option<f32>; GearSlot::COUNT],
}

/// `minecraft:equip_item`: picked-up items are equipped; `excluded` items
/// are held in the hand even when they could be worn.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct EquipItemConfig {
    pub can_wear_armor: bool,
    pub excluded: Arc<[ItemMatcher]>,
}

impl EquipItemConfig {
    pub fn excludes(&self, item_id: &str, aux: i16) -> bool {
        self.excluded.iter().any(|m| m.matches(item_id, aux))
    }
}

/// One wanted item of `minecraft:shareables`.
#[derive(Debug, Clone, PartialEq)]
pub struct ShareableEntry {
    pub item: ItemMatcher,
    /// 0 is the most wanted.
    pub priority: i32,
    /// Most the mob holds (-1: no limit).
    pub max_amount: i32,
    /// Most taken from one item actor (-1: no limit).
    pub pickup_limit: i32,
    pub want_amount: i32,
    pub surplus_amount: i32,
    /// The item is used up on pickup instead of being equipped.
    pub consume_item: bool,
}

/// `minecraft:shareables`: the wanted items in authored order (the first
/// match wins).
#[derive(Component, Debug, Clone, PartialEq)]
pub struct ShareablesConfig {
    pub items: Arc<[ShareableEntry]>,
    pub all_items: bool,
    pub all_items_max_amount: i32,
    pub all_items_want_amount: i32,
    pub all_items_surplus_amount: i32,
    pub singular_pickup: bool,
}

impl ShareablesConfig {
    /// The first entry matching `item_id`/`aux`.
    pub fn entry(&self, item_id: &str, aux: i16) -> Option<&ShareableEntry> {
        self.items.iter().find(|e| e.item.matches(item_id, aux))
    }
}

/// `minecraft:behavior.pickup_items`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct PickupItemsGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
    pub speed_multiplier: f32,
    /// Horizontal search half-extent, blocks.
    pub max_dist: i32,
    /// Vertical search half-extent, blocks.
    pub search_height: i32,
    /// Squared `goal_radius`: the item is taken once this close.
    pub goal_radius_sq: f64,
    pub can_pickup_any_item: bool,
    pub can_pickup_to_hand_or_equipment: bool,
    /// Only mobs whose spawn-time pickup roll passed run the goal.
    pub pickup_based_on_chance: bool,
    /// Only items the hand holds more of are taken (no `shareables` needed).
    pub same_items_as_in_hand: bool,
    pub stop_if_holding_item: bool,
    /// Keep re-pathing to an item the mob cannot see.
    pub track_target: bool,
    /// Ticks after being attacked before the mob picks items up again.
    pub cooldown_ticks: u64,
    pub excluded: Arc<[ItemMatcher]>,
    pub on_start: Option<SelfEvent>,
    pub on_end: Option<SelfEvent>,
}

/// `minecraft:behavior.equip_item`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct EquipItemGoal {
    pub priority: i32,
    pub(crate) controls: GoalControls,
}

/// Register the equipment component kinds; tag descriptors resolve against
/// `items`.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
    items: &Arc<ItemRegistry>,
) -> Result<(), DefinitionError> {
    builder.register(
        |b: body::Equipment| compile_equipment(b).map_err(Into::into),
        |config: &EquipmentConfig, entity| {
            entity.insert(config.clone());
            let roll = !entity.contains::<Restoring>();
            let mob = entity.id();
            entity.world_scope(|world| gear::spawn::equip_from_table(world, mob, config, roll));
        },
        |entity| {
            entity.remove::<EquipmentConfig>();
        },
    )?;
    let equip_items = Arc::clone(items);
    insert(builder, move |b: body::EquipItem| {
        Ok::<_, EquipmentError>(EquipItemConfig {
            can_wear_armor: b.can_wear_armor,
            excluded: matchers(&b.excluded_items, &equip_items)?,
        })
    })?;
    insert(builder, compile_shareables)?;
    let pickup_items = Arc::clone(items);
    builder.register(
        move |b: body::BehaviorPickupItems| compile_pickup(b, &pickup_items).map_err(Into::into),
        |goal: &PickupItemsGoal, entity| {
            entity.insert(goal.clone());
            if goal.pickup_based_on_chance && !entity.contains::<Restoring>() {
                let mob = entity.id();
                entity.world_scope(|world| gear::spawn::roll_loot_pickup(world, mob));
            }
        },
        |entity| {
            entity.remove::<PickupItemsGoal>();
        },
    )?;
    insert(builder, |b: body::BehaviorEquipItem| {
        Ok::<_, EquipmentError>(EquipItemGoal {
            priority: b.priority,
            controls: controls(&b.control_flags, GoalControls::NONE),
        })
    })
}

fn compile_equipment(b: body::Equipment) -> Result<EquipmentConfig, EquipmentError> {
    let mut drop_chances = [None; GearSlot::COUNT];
    for entry in &b.slot_drop_chance {
        let slot = GearSlot::from_body(entry.slot).ok_or(EquipmentError::BodySlot)?;
        drop_chances[slot as usize] = Some(entry.drop_chance);
    }
    Ok(EquipmentConfig {
        table: b.table.as_deref().map(loot_table).transpose()?,
        drop_chances,
    })
}

fn compile_shareables(b: body::Shareables) -> Result<ShareablesConfig, EquipmentError> {
    let items = b
        .items
        .iter()
        .map(|item| {
            for (set, feature) in [
                (item.admire, "admire"),
                (item.barter, "barter"),
                (item.stored_in_inventory, "stored_in_inventory"),
                (item.pickup_only, "pickup_only"),
                (item.craft_into.is_some(), "craft_into"),
            ] {
                if set {
                    return Err(EquipmentError::ShareableFeature(feature));
                }
            }
            Ok(ShareableEntry {
                item: ItemMatcher::parse(&item.item)?,
                priority: item.priority,
                max_amount: item.max_amount,
                pickup_limit: item.pickup_limit,
                want_amount: item.want_amount,
                surplus_amount: item.surplus_amount,
                consume_item: item.consume_item,
            })
        })
        .collect::<Result<Vec<_>, EquipmentError>>()?;
    Ok(ShareablesConfig {
        items: items.into(),
        all_items: b.all_items,
        all_items_max_amount: b.all_items_max_amount,
        all_items_want_amount: b.all_items_want_amount,
        all_items_surplus_amount: b.all_items_surplus_amount,
        singular_pickup: b.singular_pickup,
    })
}

fn compile_pickup(
    b: body::BehaviorPickupItems,
    items: &ItemRegistry,
) -> Result<PickupItemsGoal, EquipmentError> {
    let event = |trigger: &body::ActorDefinitionTrigger| {
        (!trigger.event.is_empty())
            .then(|| self_event(&EventTrigger::Trigger(trigger.clone())))
            .transpose()
    };
    let goal_radius = f64::from(b.goal_radius);
    Ok(PickupItemsGoal {
        priority: b.priority,
        controls: controls(
            &b.control_flags,
            GoalControls::MOVE.union(GoalControls::LOOK),
        ),
        speed_multiplier: b.speed_multiplier,
        max_dist: b.max_dist,
        search_height: b.search_height,
        goal_radius_sq: goal_radius * goal_radius,
        can_pickup_any_item: b.can_pickup_any_item,
        can_pickup_to_hand_or_equipment: b.can_pickup_to_hand_or_equipment,
        pickup_based_on_chance: b.pickup_based_on_chance,
        same_items_as_in_hand: b.pickup_same_items_as_in_hand,
        stop_if_holding_item: b.stop_if_holding_item,
        track_target: b.track_target,
        cooldown_ticks: (b.cooldown_after_being_attacked.max(0) as f32 * TICKS_PER_SECOND) as u64,
        excluded: matchers(&b.excluded_items, items)?,
        on_start: event(&b.on_pickup_item_start)?,
        on_end: event(&b.on_pickup_item_end)?,
    })
}

/// Item matchers of a descriptor list: `"name"`, `{"item": "name"}`,
/// `{"name": "name"}`, or `{"tags": "<Molang>"}` (every item the tag
/// expression holds for, e.g. `q.all_tags('minecraft:is_spear')`).
fn matchers(
    descriptors: &[ItemDescriptor],
    items: &ItemRegistry,
) -> Result<Arc<[ItemMatcher]>, EquipmentError> {
    let mut out = Vec::new();
    for descriptor in descriptors {
        let name =
            match descriptor {
                ItemDescriptor::String(name) => name.as_str(),
                ItemDescriptor::Map(map) if map.len() == 1 => {
                    if let Some(tags) = map.get("tags") {
                        out.extend(items_matching(tags, items)?.into_iter().map(|item| {
                            ItemMatcher {
                                identifier: Arc::from(item.identifier()),
                                aux: None,
                            }
                        }));
                        continue;
                    }
                    map.get("item")
                        .or_else(|| map.get("name"))
                        .map(String::as_str)
                        .ok_or_else(|| EquipmentError::ItemDescriptor(format!("{map:?}")))?
                }
                ItemDescriptor::Map(map) => {
                    return Err(EquipmentError::ItemDescriptor(format!("{map:?}")));
                }
            };
        out.push(ItemMatcher::parse(name)?);
    }
    Ok(out.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vanilla_items() -> ItemRegistry {
        let mut items = ItemRegistry::new();
        items.load_vanilla();
        items
    }

    #[test]
    fn zombie_bodies_compile() {
        let items = vanilla_items();
        let pickup: body::BehaviorPickupItems = serde_json::from_value(serde_json::json!({
            "can_pickup_any_item": true,
            "excluded_items": ["minecraft:glow_ink_sac"],
            "pickup_based_on_chance": true,
            "goal_radius": 2,
            "priority": 6,
            "max_dist": 3,
            "speed_multiplier": 1
        }))
        .unwrap();
        let goal = compile_pickup(pickup, &items).unwrap();
        assert_eq!(goal.goal_radius_sq, 4.0);
        assert_eq!((goal.max_dist, goal.search_height), (3, 1));
        assert_eq!(goal.controls, GoalControls::MOVE.union(GoalControls::LOOK));
        assert!(goal.excluded[0].matches("minecraft:glow_ink_sac", 0));
        assert!(goal.on_start.is_none());

        let equip: body::EquipItem = serde_json::from_value(serde_json::json!({
            "excluded_items": [{"item": "minecraft:banner:15"}]
        }))
        .unwrap();
        let excluded = matchers(&equip.excluded_items, &items).unwrap();
        assert!(excluded[0].matches("minecraft:banner", 15));
        assert!(!excluded[0].matches("minecraft:banner", 0));
        assert!(equip.can_wear_armor);

        let shareables: body::Shareables = serde_json::from_value(serde_json::json!({
            "items": [
                {"item": "minecraft:netherite_sword", "priority": 0, "surplus_amount": 1, "want_amount": 1},
                {"item": "minecraft:iron_sword", "priority": 2}
            ]
        }))
        .unwrap();
        let wants = compile_shareables(shareables).unwrap();
        assert_eq!(wants.entry("minecraft:iron_sword", 0).unwrap().priority, 2);
        assert!(wants.entry("minecraft:stone_sword", 0).is_none());

        let barter: body::Shareables = serde_json::from_value(serde_json::json!({
            "items": [{"item": "minecraft:gold_ingot", "barter": true}]
        }))
        .unwrap();
        assert!(matches!(
            compile_shareables(barter),
            Err(EquipmentError::ShareableFeature("barter"))
        ));
    }

    #[test]
    fn tag_descriptors_match_every_item_with_the_tags() {
        let items = vanilla_items();
        let pickup: body::BehaviorPickupItems = serde_json::from_value(serde_json::json!({
            "excluded_items": [{"tags": "q.all_tags('minecraft:is_spear')"}],
            "priority": 5
        }))
        .unwrap();
        let goal = compile_pickup(pickup, &items).unwrap();
        let excludes = |id: &str| goal.excluded.iter().any(|m| m.matches(id, 0));
        assert!(excludes("minecraft:copper_spear"));
        assert!(excludes("minecraft:netherite_spear"));
        assert!(!excludes("minecraft:iron_sword"));

        let random: body::BehaviorPickupItems = serde_json::from_value(serde_json::json!({
            "excluded_items": [{"tags": "math.random(0, 1)"}],
            "priority": 5
        }))
        .unwrap();
        assert!(matches!(
            compile_pickup(random, &items),
            Err(EquipmentError::Tags(TagExpressionError::Random))
        ));
    }
}
