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

/// Level difficulty as loot conditions read it (`random_difficulty_chance`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}

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
    /// The killer is a player or a tamed mob a player owns.
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
    /// The THIS actor is a baby; an absent actor does not match.
    IsBaby,
    /// The THIS actor rides an actor of this identifier (namespaced).
    PassengerOfEntity {
        entity_type: String,
    },
    /// Random chance per level difficulty, indexed by [`Difficulty`].
    RandomDifficultyChance {
        chances: [f32; 4],
    },
    /// Random chance of `max_chance` scaled by the clamped regional
    /// difficulty.
    RandomRegionalDifficultyChance {
        max_chance: f32,
    },
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
    /// With `chance` × clamped regional difficulty, randomly enchant a
    /// single item at level `5 + clamped regional difficulty × nextInt(18)`.
    EnchantRandomGear {
        chance: f32,
    },
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
    /// Pools rolled after this entry created an item (`pools` inside an
    /// entry; the `armor_set_*` tables chain armour pieces this way).
    pub nested_pools: Vec<LootPool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LootPool {
    pub rolls: LootNumber,
    pub bonus_rolls: f32,
    pub conditions: Vec<LootCondition>,
    pub entries: Vec<LootEntry>,
    /// `tiers` replace `rolls`: one entry picked by tier index.
    pub tiers: Option<LootTiers>,
}

/// Pool `tiers`: the entry at index `nextInt(initial_range)` plus one for
/// each of `bonus_rolls` draws below `bonus_chance` (no entry when past the
/// end). Entry conditions and weights are not consulted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LootTiers {
    pub initial_range: i32,
    pub bonus_rolls: i32,
    pub bonus_chance: f32,
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
