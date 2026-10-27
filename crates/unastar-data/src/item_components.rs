//! Behaviour-pack item documents (`minecraft:item`) and their components.
//!
//! BDS loads item JSON from the behaviour-pack stack on top of its built-in
//! item classes. `format_version` selects the model: below 1.16.100 the
//! document *augments* a built-in item with legacy components (`use_duration`
//! in ticks, named food saturation, `seed`, `camera`, ...); from 1.16.100 it
//! defines a component-based item. Both spellings are kept as written: the
//! component kinds are typed one Rust struct per kind, and the gameplay layer
//! implements each kind once.
//!
//! Vanilla documents come from bedrock-samples, embedded at build time
//! (`build.rs`, D13) and decoded at startup ([`decode_vanilla`]). The same
//! [`parse_item_document`] is the entry point for addon packs later (D5).
//! Component names this loader does not know are kept as
//! [`ItemComponents::unknown`] (BDS logs and ignores them).

use serde::Deserialize;
use serde_json::Value;

include!(concat!(env!("OUT_DIR"), "/vanilla_items.rs"));

/// Pack `format_version` as `major.minor.patch` (missing parts are 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FormatVersion(pub [u32; 3]);

impl FormatVersion {
    /// First version whose item documents define component-based items.
    pub const DATA_DRIVEN: Self = Self([1, 16, 100]);

    /// Parse `"1.21.80"`-style versions.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = [0u32; 3];
        let mut count = 0;
        for part in text.split('.') {
            *parts.get_mut(count)? = part.parse().ok()?;
            count += 1;
        }
        (count > 0).then_some(Self(parts))
    }

    /// Whether documents of this version augment a built-in item (legacy).
    pub fn is_legacy(self) -> bool {
        self < Self::DATA_DRIVEN
    }
}

/// One parsed `minecraft:item` document.
#[derive(Debug, Clone, PartialEq)]
pub struct PackItem {
    /// Pack-relative path of the document (for diagnostics).
    pub origin: String,
    pub identifier: String,
    pub format_version: FormatVersion,
    /// `description.menu_category` (data-driven items only).
    pub menu_category: Option<MenuCategory>,
    pub components: ItemComponents,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MenuCategory {
    pub category: String,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub is_hidden_in_commands: Option<bool>,
}

/// Every component kind vanilla item documents use, typed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemComponents {
    pub allow_off_hand: Option<bool>,
    /// Legacy `minecraft:block`: block identifier the item stands for.
    pub block: Option<String>,
    pub block_placer: Option<BlockPlacer>,
    pub bundle_interaction: Option<BundleInteraction>,
    pub camera: Option<Camera>,
    pub compostable: Option<Compostable>,
    pub cooldown: Option<Cooldown>,
    pub damage: Option<i32>,
    pub display_name: Option<String>,
    pub durability: Option<Durability>,
    pub enchantable: Option<Enchantable>,
    pub fire_resistant: Option<bool>,
    /// `minecraft:foil` (legacy) / `minecraft:glint`.
    pub glint: Option<bool>,
    pub food: Option<Food>,
    pub fuel: Option<Fuel>,
    pub hand_equipped: Option<bool>,
    pub icon: Option<Value>,
    pub kinetic_weapon: Option<KineticWeapon>,
    /// Legacy `minecraft:max_damage`.
    pub max_damage: Option<i32>,
    pub max_stack_size: Option<u8>,
    pub piercing_weapon: Option<PiercingWeapon>,
    pub projectile: Option<Projectile>,
    pub repairable: Option<Repairable>,
    /// Legacy `minecraft:seed`.
    pub seed: Option<Seed>,
    pub stacked_by_data: Option<bool>,
    pub storage_item: Option<StorageItem>,
    pub storage_weight_limit: Option<StorageWeightLimit>,
    pub storage_weight_modifier: Option<StorageWeightModifier>,
    pub swing_duration: Option<f32>,
    pub swing_sounds: Option<SwingSounds>,
    pub tags: Vec<String>,
    pub throwable: Option<Throwable>,
    pub use_animation: Option<String>,
    /// Legacy `minecraft:use_duration`, in ticks.
    pub use_duration: Option<i32>,
    pub use_modifiers: Option<UseModifiers>,
    pub wearable: Option<Wearable>,
    /// Components this loader does not know: (name, JSON value).
    pub unknown: Vec<(String, Value)>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BlockPlacer {
    pub block: String,
    #[serde(default)]
    pub replace_block_item: bool,
    #[serde(default)]
    pub use_on: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BundleInteraction {
    pub num_viewable_slots: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Camera {
    pub black_bars_duration: f32,
    pub black_bars_screen_ratio: f32,
    pub shutter_duration: f32,
    pub picture_duration: f32,
    pub slide_away_duration: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Compostable {
    pub composting_chance: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Cooldown {
    pub category: String,
    pub duration: f32,
    /// `"use"` (default) or `"attack"`.
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Durability {
    pub max_durability: i32,
    #[serde(default)]
    pub damage_chance: Option<IntRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct IntRange {
    pub min: i32,
    pub max: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct FloatRange {
    pub min: f32,
    pub max: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Enchantable {
    pub slot: String,
    pub value: i32,
}

/// `minecraft:food` in both spellings: data-driven documents give
/// `saturation_modifier` as a number, legacy ones as a name.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Food {
    pub nutrition: i32,
    pub saturation_modifier: Saturation,
    #[serde(default)]
    pub can_always_eat: bool,
    #[serde(default)]
    pub using_converts_to: Option<String>,
    #[serde(default)]
    pub effects: Vec<FoodEffect>,
    #[serde(default)]
    pub remove_effects: Vec<String>,
    #[serde(default)]
    pub on_use_action: Option<String>,
    #[serde(default)]
    pub on_use_range: Option<[f32; 3]>,
    #[serde(default)]
    pub cooldown_type: Option<String>,
    #[serde(default)]
    pub cooldown_time: Option<i32>,
    #[serde(default)]
    pub is_meat: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Saturation {
    Value(f32),
    Named(String),
}

impl Saturation {
    /// The modifier as a number. Legacy names map to the values BDS sends in
    /// `ItemRegistryPacket` for the same items (`items.nbt`; checked for every
    /// vanilla legacy food by `unastar`'s registry tests). `None` for an
    /// unknown name.
    pub fn value(&self) -> Option<f32> {
        match self {
            Self::Value(value) => Some(*value),
            Self::Named(name) => match name.as_str() {
                "poor" => Some(0.1),
                "low" => Some(0.3),
                "normal" => Some(0.6),
                "good" => Some(0.8),
                "max" => Some(1.0),
                "supernatural" => Some(1.2),
                _ => None,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FoodEffect {
    pub name: String,
    #[serde(default = "one")]
    pub chance: f32,
    /// Seconds.
    pub duration: i32,
    #[serde(default)]
    pub amplifier: i32,
}

fn one() -> f32 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Fuel {
    /// Seconds of burn time.
    pub duration: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct KineticWeapon {
    pub delay: i32,
    pub reach: FloatRange,
    pub creative_reach: FloatRange,
    pub hitbox_margin: f32,
    pub damage_multiplier: f32,
    pub damage_conditions: KineticConditions,
    pub knockback_conditions: KineticConditions,
    pub dismount_conditions: KineticConditions,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct KineticConditions {
    pub max_duration: i32,
    #[serde(default)]
    pub min_speed: Option<f32>,
    #[serde(default)]
    pub min_relative_speed: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PiercingWeapon {
    pub reach: FloatRange,
    pub creative_reach: FloatRange,
    pub hitbox_margin: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Projectile {
    pub projectile_entity: String,
    #[serde(default)]
    pub minimum_critical_power: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Repairable {
    pub repair_items: Vec<RepairItem>,
}

/// One `repair_items` row; `repair_amount` is a number or a Molang string.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RepairItem {
    pub items: Vec<ItemDescriptor>,
    pub repair_amount: Value,
}

/// An item reference: a plain identifier or `{"tags": "<molang>"}`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum ItemDescriptor {
    Name(String),
    Tags { tags: String },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Seed {
    pub crop_result: String,
    #[serde(default, deserialize_with = "one_or_many")]
    pub plant_at: Vec<String>,
    #[serde(default)]
    pub plant_at_any_solid_surface: bool,
    #[serde(default)]
    pub plant_at_face: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StorageItem {
    pub max_slots: u32,
    #[serde(default)]
    pub allow_nested_storage_items: bool,
    #[serde(default)]
    pub banned_items: Vec<String>,
    #[serde(default)]
    pub allowed_items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StorageWeightLimit {
    pub max_weight_limit: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StorageWeightModifier {
    pub weight_in_storage_item: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SwingSounds {
    #[serde(default)]
    pub attack_hit: Option<String>,
    #[serde(default)]
    pub attack_miss: Option<String>,
    #[serde(default)]
    pub attack_critical_hit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Throwable {
    #[serde(default)]
    pub do_swing_animation: bool,
    #[serde(default)]
    pub launch_power_scale: Option<f32>,
    #[serde(default)]
    pub max_launch_power: Option<f32>,
    #[serde(default)]
    pub max_draw_duration: Option<f32>,
    #[serde(default)]
    pub min_draw_duration: Option<f32>,
    #[serde(default)]
    pub scale_power_by_draw_duration: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UseModifiers {
    /// Seconds.
    pub use_duration: f32,
    #[serde(default)]
    pub movement_modifier: Option<f32>,
    #[serde(default)]
    pub emit_vibrations: Option<bool>,
    #[serde(default)]
    pub start_sound: Option<String>,
    #[serde(default)]
    pub start_using: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Wearable {
    pub slot: String,
    #[serde(default)]
    pub protection: i32,
}

fn one_or_many<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(value) => vec![value],
        OneOrMany::Many(values) => values,
    })
}

/// A document that cannot become an item.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{origin}: {message}")]
pub struct PackItemError {
    pub origin: String,
    pub message: String,
}

impl PackItemError {
    fn new(origin: &str, message: impl Into<String>) -> Self {
        Self {
            origin: origin.to_string(),
            message: message.into(),
        }
    }
}

/// Parse one `minecraft:item` document.
pub fn parse_item_document(origin: &str, document: &Value) -> Result<PackItem, PackItemError> {
    let format_version = document
        .get("format_version")
        .and_then(Value::as_str)
        .and_then(FormatVersion::parse)
        .ok_or_else(|| PackItemError::new(origin, "missing or malformed format_version"))?;
    let item = document
        .get("minecraft:item")
        .ok_or_else(|| PackItemError::new(origin, "not a minecraft:item document"))?;
    let description = item
        .get("description")
        .ok_or_else(|| PackItemError::new(origin, "missing description"))?;
    let identifier = description
        .get("identifier")
        .and_then(Value::as_str)
        .filter(|id| id.contains(':'))
        .ok_or_else(|| PackItemError::new(origin, "missing namespaced description.identifier"))?
        .to_string();
    let menu_category = description
        .get("menu_category")
        .map(|value| typed(origin, "description.menu_category", value))
        .transpose()?;
    let mut components = ItemComponents::default();
    if let Some(map) = item.get("components") {
        let map = map
            .as_object()
            .ok_or_else(|| PackItemError::new(origin, "components must be an object"))?;
        for (name, value) in map {
            apply_component(origin, &mut components, name, value)?;
        }
    }
    Ok(PackItem {
        origin: origin.to_string(),
        identifier,
        format_version,
        menu_category,
        components,
    })
}

fn typed<T: for<'de> Deserialize<'de>>(
    origin: &str,
    name: &str,
    value: &Value,
) -> Result<T, PackItemError> {
    T::deserialize(value).map_err(|e| PackItemError::new(origin, format!("{name}: {e}")))
}

/// Components written either as a bare value or as `{"value": ...}`.
fn bare_or_value<T: for<'de> Deserialize<'de>>(
    origin: &str,
    name: &str,
    value: &Value,
) -> Result<T, PackItemError> {
    match value.get("value") {
        Some(inner) if value.as_object().is_some_and(|map| map.len() == 1) => {
            typed(origin, name, inner)
        }
        _ => typed(origin, name, value),
    }
}

fn apply_component(
    origin: &str,
    c: &mut ItemComponents,
    name: &str,
    value: &Value,
) -> Result<(), PackItemError> {
    let short = name.strip_prefix("minecraft:").unwrap_or(name);
    match short {
        "allow_off_hand" => c.allow_off_hand = Some(bare_or_value(origin, name, value)?),
        "block" => c.block = Some(typed(origin, name, value)?),
        "block_placer" => c.block_placer = Some(typed(origin, name, value)?),
        "bundle_interaction" => c.bundle_interaction = Some(typed(origin, name, value)?),
        "camera" => c.camera = Some(typed(origin, name, value)?),
        "compostable" => c.compostable = Some(typed(origin, name, value)?),
        "cooldown" => c.cooldown = Some(typed(origin, name, value)?),
        "damage" => c.damage = Some(bare_or_value(origin, name, value)?),
        "display_name" => c.display_name = Some(bare_or_value(origin, name, value)?),
        "durability" => c.durability = Some(typed(origin, name, value)?),
        "enchantable" => c.enchantable = Some(typed(origin, name, value)?),
        "fire_resistant" => c.fire_resistant = Some(bare_or_value(origin, name, value)?),
        "foil" | "glint" => c.glint = Some(bare_or_value(origin, name, value)?),
        "food" => c.food = Some(typed(origin, name, value)?),
        "fuel" => c.fuel = Some(typed(origin, name, value)?),
        "hand_equipped" => c.hand_equipped = Some(bare_or_value(origin, name, value)?),
        "icon" => c.icon = Some(value.clone()),
        "kinetic_weapon" => c.kinetic_weapon = Some(typed(origin, name, value)?),
        "max_damage" => c.max_damage = Some(bare_or_value(origin, name, value)?),
        "max_stack_size" => c.max_stack_size = Some(bare_or_value(origin, name, value)?),
        "piercing_weapon" => c.piercing_weapon = Some(typed(origin, name, value)?),
        "projectile" => c.projectile = Some(typed(origin, name, value)?),
        "repairable" => c.repairable = Some(typed(origin, name, value)?),
        "seed" => c.seed = Some(typed(origin, name, value)?),
        "stacked_by_data" => c.stacked_by_data = Some(bare_or_value(origin, name, value)?),
        "storage_item" => c.storage_item = Some(typed(origin, name, value)?),
        "storage_weight_limit" => c.storage_weight_limit = Some(typed(origin, name, value)?),
        "storage_weight_modifier" => c.storage_weight_modifier = Some(typed(origin, name, value)?),
        "swing_duration" => c.swing_duration = Some(bare_or_value(origin, name, value)?),
        "swing_sounds" => c.swing_sounds = Some(typed(origin, name, value)?),
        "tags" => {
            #[derive(Deserialize)]
            struct Tags {
                tags: Vec<String>,
            }
            c.tags = typed::<Tags>(origin, name, value)?.tags;
        }
        "throwable" => c.throwable = Some(typed(origin, name, value)?),
        "use_animation" => c.use_animation = Some(bare_or_value(origin, name, value)?),
        "use_duration" => c.use_duration = Some(bare_or_value(origin, name, value)?),
        "use_modifiers" => c.use_modifiers = Some(typed(origin, name, value)?),
        "wearable" => c.wearable = Some(typed(origin, name, value)?),
        _ => c.unknown.push((name.to_string(), value.clone())),
    }
    Ok(())
}

/// Decode the embedded vanilla item documents, sorted by pack path.
pub fn decode_vanilla() -> Result<Vec<PackItem>, PackItemError> {
    VANILLA_ITEM_FILES
        .iter()
        .map(|(origin, json)| {
            let document: Value = serde_json::from_str(json)
                .map_err(|e| PackItemError::new(origin, e.to_string()))?;
            parse_item_document(origin, &document)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_vanilla_document_parses_into_known_components() {
        let items = decode_vanilla().expect("vanilla item documents parse");
        assert_eq!(items.len(), VANILLA_ITEM_FILES.len());
        for item in &items {
            assert!(
                item.components.unknown.is_empty(),
                "{}: {:?}",
                item.origin,
                item.components.unknown
            );
        }
        let mut ids: Vec<_> = items.iter().map(|item| item.identifier.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), items.len(), "one document per identifier");
    }

    #[test]
    fn legacy_and_data_driven_spellings_keep_their_meaning() {
        let bread = parse_item_document(
            "bread.json",
            &json!({"format_version": "1.10", "minecraft:item": {
                "description": {"identifier": "minecraft:bread"},
                "components": {"minecraft:use_duration": 32,
                    "minecraft:food": {"nutrition": 5, "saturation_modifier": "normal"},
                    "minecraft:hand_equipped": false}}}),
        )
        .unwrap();
        assert!(bread.format_version.is_legacy());
        assert_eq!(bread.components.use_duration, Some(32));
        let food = bread.components.food.unwrap();
        assert_eq!(food.saturation_modifier.value(), Some(0.6));
        assert_eq!(bread.components.hand_equipped, Some(false));

        let apple = parse_item_document(
            "apple.json",
            &json!({"format_version": "1.26.30", "minecraft:item": {
                "description": {"identifier": "minecraft:apple", "menu_category": {"category": "items"}},
                "components": {"minecraft:food": {"nutrition": 4, "saturation_modifier": 0.3},
                    "minecraft:hand_equipped": {"value": true},
                    "minecraft:use_modifiers": {"use_duration": 1.6, "movement_modifier": 0.35},
                    "minecraft:future_thing": {"x": 1}}}}),
        )
        .unwrap();
        assert!(!apple.format_version.is_legacy());
        assert_eq!(
            apple.components.food.unwrap().saturation_modifier.value(),
            Some(0.3)
        );
        assert_eq!(apple.components.hand_equipped, Some(true));
        assert_eq!(apple.components.use_modifiers.unwrap().use_duration, 1.6);
        assert_eq!(
            apple.components.unknown,
            [("minecraft:future_thing".to_string(), json!({"x": 1}))]
        );
    }

    #[test]
    fn malformed_documents_name_the_file() {
        for document in [
            json!({"minecraft:item": {"description": {"identifier": "minecraft:a"}}}),
            json!({"format_version": "1.10", "minecraft:item": {"description": {"identifier": "a"}}}),
            json!({"format_version": "1.10", "minecraft:item": {"description": {"identifier": "minecraft:a"},
                "components": {"minecraft:food": {"nutrition": "lots"}}}}),
        ] {
            let error = parse_item_document("bad.json", &document).unwrap_err();
            assert_eq!(error.origin, "bad.json");
        }
    }

    #[test]
    fn format_versions_order_numerically() {
        assert!(FormatVersion::parse("1.10").unwrap().is_legacy());
        assert!(FormatVersion::parse("1.16").unwrap().is_legacy());
        assert!(!FormatVersion::parse("1.16.100").unwrap().is_legacy());
        assert!(FormatVersion::parse("1.9.0") < FormatVersion::parse("1.10.0"));
        assert_eq!(FormatVersion::parse("1.x"), None);
        assert_eq!(FormatVersion::parse("1.2.3.4"), None);
    }
}
