//! Entity/block facts supplied to the shared loot evaluator.
//! Evaluation and typed loot schemas live in the bevy-free `unastar-rules`
//! crate; this adapter alone knows about ItemStack and the block registry.

use bevy_ecs::prelude::World;
use unastar_data::actor_identifiers::ActorType;
use unastar_rules::enchanting::ItemEnchanting;
use unastar_rules::loot::{Difficulty as LootDifficulty, LootError, LootStateValue};

use crate::entity::mob::constants::{LEGACY_ITEM_NAMES, LEGACY_WOOL, WOOL_ITEMS};
use crate::item::ItemStack;
use crate::item::enchant::{Enchantment, enchantment_level};
use crate::registry::block::{BlockEntry, StateValue};
use crate::world::regional_difficulty::clamped_regional_difficulty;
use crate::world::rules::{Difficulty, WorldTime};

#[derive(Debug, Clone, Copy, Default)]
pub struct LootContext<'a> {
    pub tool: Option<&'a ItemStack>,
    pub block: Option<(&'a BlockEntry, u32)>,
    pub killed_by_player: bool,
    /// The killer was a tamed mob a player owns (`killed_by_player_or_pets`).
    pub killed_by_pet: bool,
    pub looting: u8,
    /// None means this evaluation has no THIS actor.
    pub on_fire: Option<bool>,
    pub recipes: Option<&'a unastar_data::recipes::RecipeRegistry>,
    /// Palette colour of the THIS actor, when it has one.
    pub color_index: Option<u8>,
    /// The THIS actor is a baby (`is_baby`).
    pub is_baby: bool,
    /// The actor the THIS actor rides (`passenger_of_entity`).
    pub vehicle: Option<ActorType>,
    /// Level facts for difficulty conditions and gear enchanting; tables
    /// that need them fail to evaluate without.
    pub level: Option<LootLevel>,
}

/// The level facts loot reads: difficulty and clamped regional difficulty.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LootLevel {
    pub difficulty: Difficulty,
    pub clamped_regional_difficulty: f32,
}

impl LootLevel {
    /// The level facts of `world` from its `Difficulty` and `WorldTime`
    /// resources; `None` for a world without them.
    pub fn of(world: &World) -> Option<Self> {
        let difficulty = *world.get_resource::<Difficulty>()?;
        world.get_resource::<WorldTime>()?;
        Some(Self {
            difficulty,
            clamped_regional_difficulty: clamped_regional_difficulty(world),
        })
    }
}

impl unastar_rules::loot::LootContext for LootContext<'_> {
    fn tool_item(&self) -> Option<&str> {
        self.tool
            .filter(|t| !t.is_empty())
            .map(|t| t.item_id.as_str())
    }

    fn enchantment_level(&self, id: i16) -> u8 {
        let enchantment = match id {
            8 => Enchantment::AquaAffinity,
            14 => Enchantment::Looting,
            15 => Enchantment::Efficiency,
            16 => Enchantment::SilkTouch,
            17 => Enchantment::Unbreaking,
            18 => Enchantment::Fortune,
            _ => return 0,
        };
        self.tool
            .map_or(0, |tool| enchantment_level(tool, enchantment))
    }

    fn block_state_matches(&self, property: &str, expected: &LootStateValue) -> bool {
        let Some((block, state)) = self.block else {
            return false;
        };
        match (block.state_value(state, property), expected) {
            (Some(StateValue::Int(v)), LootStateValue::Int(e)) => v == *e,
            (Some(StateValue::Byte(v)), LootStateValue::Int(e)) => i32::from(v) == *e,
            (Some(StateValue::Str(v)), LootStateValue::Str(e)) => v == e,
            _ => false,
        }
    }

    fn killed_by_player(&self) -> bool {
        self.killed_by_player
    }
    fn killed_by_pet(&self) -> bool {
        self.killed_by_pet
    }
    fn looting(&self) -> u8 {
        self.looting
    }

    fn is_baby(&self) -> bool {
        self.is_baby
    }

    fn vehicle_is(&self, entity_type: &str) -> bool {
        self.vehicle
            .is_some_and(|vehicle| vehicle.name() == entity_type)
    }

    fn difficulty(&self) -> Option<LootDifficulty> {
        self.level.map(|level| match level.difficulty {
            Difficulty::Peaceful => LootDifficulty::Peaceful,
            Difficulty::Easy => LootDifficulty::Easy,
            Difficulty::Normal => LootDifficulty::Normal,
            Difficulty::Hard => LootDifficulty::Hard,
        })
    }

    fn clamped_regional_difficulty(&self) -> Option<f32> {
        self.level.map(|level| level.clamped_regional_difficulty)
    }

    fn item_enchanting(&self, item: &str) -> Result<ItemEnchanting, LootError> {
        crate::item::enchanting::item_enchanting(item)
            .ok_or_else(|| LootError::Unsupported(format!("random enchanting of {item}")))
    }

    fn on_fire(&self) -> Option<bool> {
        self.on_fire
    }

    fn furnace(&self) -> Option<&dyn unastar_rules::loot::FurnaceSmelt> {
        self.recipes
            .map(|recipes| recipes as &dyn unastar_rules::loot::FurnaceSmelt)
    }

    fn this_color_index(&self) -> Option<u8> {
        self.color_index
    }

    fn item_alias(&self, name: &str) -> Option<&str> {
        LEGACY_ITEM_NAMES
            .iter()
            .find(|(legacy, _)| legacy.eq_ignore_ascii_case(name))
            .map(|(_, modern)| *modern)
    }

    fn data_alias(&self, name: &str, data: i16) -> Option<&str> {
        if !LEGACY_WOOL.eq_ignore_ascii_case(name) {
            return None;
        }
        usize::try_from(data)
            .ok()
            .and_then(|colour| WOOL_ITEMS.get(colour))
            .copied()
    }
}

#[cfg(test)]
mod tests;
