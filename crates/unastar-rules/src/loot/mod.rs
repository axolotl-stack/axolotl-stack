//! Shared loot data and evaluator (D2/D10). Vanilla canonical JSON is embedded
//! by unastar-data and decoded once; the engine has no ECS/item dependency.
//! Historical block-table extensions retain their recorded Java semantic
//! deviations. Function guards and recipe-backed furnace transformations share
//! this engine; native RNG/draw-order calibration remains separate work.

mod decode;
mod evaluate;

pub use decode::decode_table;
pub use evaluate::{LootContext, LootDrop, LootError, roll};
use std::collections::BTreeMap;

/// Borrowed canonical recipe output. Smelting preserves the loot count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmeltedItem<'a> {
    pub identifier: &'a str,
    pub data: i16,
}

/// Startup-compiled furnace lookup supplied by the caller's recipe registry.
pub trait FurnaceSmelt {
    fn smelt(&self, identifier: &str, data: i16) -> Option<SmeltedItem<'_>>;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LootNumber {
    Constant(f32),
    Uniform { min: f32, max: f32 },
    Binomial { n: u32, p: f32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnchantmentRequirement {
    pub enchantment: String,
    /// Resolved at decode, never name-looked-up while rolling.
    pub id: Option<i16>,
    pub min_level: u8,
    pub max_level: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LootStateValue {
    Int(i32),
    Str(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BonusFormula {
    OreDrops,
    UniformBonusCount { bonus_multiplier: f32 },
    BinomialWithBonusCount { extra: u32, probability: f32 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum LootCondition {
    RandomChance {
        chance: f32,
    },
    RandomChanceWithLooting {
        chance: f32,
        looting_multiplier: f32,
    },
    KilledByPlayer,
    KilledByPlayerOrPets,
    MatchTool {
        item: Option<String>,
        enchantments: Vec<EnchantmentRequirement>,
    },
    TableBonus {
        enchantment: String,
        id: Option<i16>,
        chances: Vec<f32>,
    },
    BlockStateProperty {
        properties: Vec<(String, LootStateValue)>,
    },
    Inverted(Box<LootCondition>),
    /// The supplied THIS actor's fire state; an absent actor does not match.
    ThisOnFire(bool),
    Other {
        name: String,
        json: serde_json::Value,
    },
}

/// Conditions guard one action, including its context access and RNG draws.
#[derive(Debug, Clone, PartialEq)]
pub struct LootFunction {
    pub conditions: Vec<LootCondition>,
    pub kind: LootFunctionKind,
}

/// A typed function action; its source conditions live in [`LootFunction`].
#[derive(Debug, Clone, PartialEq)]
pub enum LootFunctionKind {
    SetCount {
        count: LootNumber,
        add: bool,
    },
    SetData {
        data: LootNumber,
    },
    LootingEnchant {
        count: LootNumber,
        limit: u32,
    },
    ApplyBonus {
        enchantment: String,
        id: Option<i16>,
        formula: BonusFormula,
    },
    LimitCount {
        min: Option<f32>,
        max: Option<f32>,
    },
    ExplosionDecay,
    FurnaceSmelt,
    /// Data value from the THIS actor's palette colour; a legacy name that
    /// picks its item by data value (`minecraft:wool`) becomes that item.
    SetDataFromColorIndex,
    Other {
        name: String,
        json: serde_json::Value,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum LootEntryKind {
    Item(String),
    Table(String),
    Empty,
    Alternatives(Vec<LootEntry>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LootEntry {
    pub kind: LootEntryKind,
    pub weight: u32,
    pub quality: i32,
    pub conditions: Vec<LootCondition>,
    pub functions: Vec<LootFunction>,
    pub nested_pools_json: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LootPool {
    pub rolls: LootNumber,
    pub bonus_rolls: f32,
    pub conditions: Vec<LootCondition>,
    pub entries: Vec<LootEntry>,
    pub tiers_json: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LootTable {
    pub name: String,
    pub source: String,
    pub source_note: Option<String>,
    pub pools: Vec<LootPool>,
    /// Complete source table, retained for features not projected yet.
    pub canonical: serde_json::Value,
}

/// Identifier-keyed builder/registry. Load vanilla, apply addon rows, then
/// share it immutably after startup. Nested table references use this same set.
#[derive(Debug, Default)]
pub struct LootTables {
    tables: BTreeMap<String, LootTable>,
}

impl LootTables {
    pub fn insert(&mut self, table: LootTable) -> Option<LootTable> {
        self.tables.insert(table.name.clone(), table)
    }

    pub fn get(&self, name: &str) -> Option<&LootTable> {
        self.tables.get(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &LootTable> {
        self.tables.values()
    }

    pub fn len(&self) -> usize {
        self.tables.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }
}

#[cfg(test)]
mod tests;
