//! Compilation of the typed spawn rules into [`SpawnEntry`]s and per-biome
//! candidate pools.
//!
//! Every `conditions[]` element of a spawn rule file is one entry with its
//! own weight. An entry is in a biome's pool when its `biome_filter` accepts
//! the biome's tags; the spawn cycle draws from the pool among the entries
//! whose conditions the spawn position meets. Entries fall in three groups:
//!
//! - **spawnable**: every condition is implemented and the mob can be built;
//! - **blocked**: the conditions are evaluated (so the entry keeps its share
//!   of the weighted draw), but when it is drawn nothing spawns, because the
//!   mob itself or the native checks after the draw are not implemented;
//! - **unsupported**: a condition cannot be evaluated, so the entry is left
//!   out of every pool.
//!
//! Blocked and unsupported entries carry their reason and are logged once
//! when the rules are installed.

use std::collections::HashMap;
use std::sync::Arc;

use unastar_data::actor_identifiers::ActorType;
use unastar_data::spawn_rules::{SpawnRuleFile, body};
use unastar_rules::filter::{
    BuiltinTest, Fact, Filter, FilterContext, FilterRegistry, FilterTest, Subject,
};

use crate::entity::mob::MobRegistry;
use crate::entity::mob::constants::{
    DEFAULT_SPAWN_DISTANCE_MAX, DEFAULT_SPAWN_DISTANCE_MIN, DEFAULT_SPAWN_WEIGHT,
    LEGACY_BLOCK_NAMES, SPAWN_DISTANCE_CAP,
};
use crate::registry::BlockRegistry;

/// A `population_control` category with a population cap. The order is the
/// index into the cap tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Animal,
    Monster,
    WaterAnimal,
    Villager,
    Ambient,
    Cat,
    Pillager,
}

impl Category {
    pub const ALL: [Category; 7] = [
        Self::Animal,
        Self::Monster,
        Self::WaterAnimal,
        Self::Villager,
        Self::Ambient,
        Self::Cat,
        Self::Pillager,
    ];

    /// The category a `population_control` names (case-insensitive). Other
    /// names have no cap: such mobs are never counted and never spawn.
    pub fn from_population_control(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "animal" => Self::Animal,
            "monster" => Self::Monster,
            "water_animal" => Self::WaterAnimal,
            "villager" => Self::Villager,
            "ambient" => Self::Ambient,
            "cat" => Self::Cat,
            "pillager" => Self::Pillager,
            _ => return None,
        })
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// World difficulty, ordered as `difficulty_filter` compares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}

impl From<body::SpawnDifficultyLegacy> for Difficulty {
    fn from(value: body::SpawnDifficultyLegacy) -> Self {
        match value {
            body::SpawnDifficultyLegacy::Peaceful => Self::Peaceful,
            body::SpawnDifficultyLegacy::Easy => Self::Easy,
            body::SpawnDifficultyLegacy::Normal => Self::Normal,
            body::SpawnDifficultyLegacy::Hard => Self::Hard,
        }
    }
}

/// One `minecraft:herd` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Herd {
    pub min_size: i32,
    pub max_size: i32,
    /// Fired on every herd member from index `event_skip_count` on.
    pub event: Option<String>,
    pub event_skip_count: usize,
    /// Fired on the first `initial_event_count` members.
    pub initial_event: Option<String>,
    pub initial_event_count: usize,
}

impl Herd {
    /// The herd used when a condition authors none.
    pub const SINGLE: Herd = Herd {
        min_size: 1,
        max_size: 1,
        event: None,
        event_skip_count: 0,
        initial_event: None,
        initial_event_count: 0,
    };
}

/// One `minecraft:permute_type` entry: herd members beyond the guaranteed
/// ones become `entity_type` (or the rule's own mob when it is `None`) with
/// probability `weight / Σ weight`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permutation {
    pub weight: u32,
    pub min_guaranteed: u32,
    pub entity_type: Option<Arc<str>>,
    /// `entity_type` as an actor type (`None` when absent or not an actor).
    pub actor: Option<ActorType>,
}

/// Block runtime-id ranges a block descriptor matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSet {
    ranges: Vec<(u32, u32)>,
    names: Vec<String>,
}

impl BlockSet {
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn contains(&self, runtime_id: u32) -> bool {
        self.ranges
            .iter()
            .any(|&(min, max)| (min..=max).contains(&runtime_id))
    }

    /// Whether every descriptor names `minecraft:grass_block`.
    fn only_grass(&self) -> bool {
        !self.names.is_empty()
            && self
                .names
                .iter()
                .all(|name| name == "minecraft:grass_block")
    }
}

/// A compiled spawn condition.
#[derive(Debug, Clone)]
pub struct SpawnEntry {
    /// The spawn rule's `description.identifier`.
    pub identifier: Arc<str>,
    /// The identifier as an actor type (`None` for spawn identifiers that
    /// name no actor, such as `minecraft:pillager_patrol`).
    pub actor: Option<ActorType>,
    /// Pack entry and condition index, for reports.
    pub source: String,
    pub category: Option<Category>,
    pub weight: u32,
    /// `minecraft:weight.rarity`: a drawn entry spawns 1 in `rarity` times.
    pub rarity: u32,
    pub herds: Vec<Herd>,
    pub surface: bool,
    pub underground: bool,
    pub underwater: bool,
    pub lava: bool,
    pub brightness: (i32, i32),
    pub height: (i32, i32),
    pub difficulty: (Difficulty, Difficulty),
    pub distance: (i32, i32),
    /// World age bounds in seconds.
    pub world_age: (u64, u64),
    /// `minecraft:density_limit` (surface, underground); negative is no limit.
    pub density: [i32; 2],
    pub on_blocks: BlockSet,
    pub prevented_blocks: BlockSet,
    pub above_blocks: Option<(BlockSet, i32)>,
    pub permutations: Vec<Permutation>,
    pub persistent: bool,
    /// Why a drawn entry spawns nothing (see the module docs).
    pub blocked: Option<String>,
}

/// A spawn rule condition left out of every pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedEntry {
    pub identifier: String,
    pub source: String,
    pub reason: String,
}

/// The compiled natural spawn rules.
#[derive(bevy_ecs::prelude::Resource, Debug, Clone, Default)]
pub struct NaturalSpawnRules {
    pub entries: Vec<SpawnEntry>,
    /// Bedrock biome id → indices into `entries`, in rule order.
    pub pools: HashMap<u32, Vec<usize>>,
    /// Mob type → population category (from its spawn rule).
    pub categories: HashMap<ActorType, Category>,
    pub unsupported: Vec<UnsupportedEntry>,
}

/// Why a condition cannot be compiled.
#[derive(Debug, thiserror::Error)]
pub enum RuleError {
    #[error("{0} is not implemented")]
    Unimplemented(&'static str),
    #[error("biome filter: {0}")]
    BiomeFilterParse(#[from] unastar_rules::ParseError),
    #[error("biome filter: {0:?}")]
    BiomeFilterDiagnostics(Vec<unastar_rules::Diagnostic>),
    #[error("biome filter: test {0} is not implemented")]
    BiomeFilterTest(String),
}

impl NaturalSpawnRules {
    /// Compile `files` against the mob and block registries.
    pub fn compile(files: &[SpawnRuleFile], mobs: &MobRegistry, blocks: &BlockRegistry) -> Self {
        let mut rules = Self::default();
        let mut filters = Vec::new();
        for file in files {
            let identifier: Arc<str> = Arc::from(file.identifier());
            let category = Category::from_population_control(
                &file.document.spawn_rules.description.population_control,
            );
            if let (Some(category), Some(actor)) = (category, ActorType::from_name(&identifier)) {
                rules.categories.insert(actor, category);
            }
            for (index, condition) in file.document.spawn_rules.conditions.iter().enumerate() {
                let source = format!("{}#{index}", file.source);
                match compile_entry(&identifier, &source, category, condition, mobs, blocks) {
                    Ok((entry, filter)) => {
                        rules.entries.push(entry);
                        filters.push(filter);
                    }
                    Err(error) => rules.unsupported.push(UnsupportedEntry {
                        identifier: identifier.to_string(),
                        source,
                        reason: error.to_string(),
                    }),
                }
            }
        }
        for biome in unastar_data::biomes::iter() {
            let tags = BiomeTags(&biome.tags);
            let pool: Vec<usize> = filters
                .iter()
                .enumerate()
                .filter(|(_, filter)| filter.as_ref().is_none_or(|f| f.evaluate(&tags)))
                .map(|(index, _)| index)
                .collect();
            if !pool.is_empty() {
                rules.pools.insert(biome.id, pool);
            }
        }
        rules
    }

    /// The population category of a mob type.
    pub fn category(&self, actor: ActorType) -> Option<Category> {
        self.categories.get(&actor).copied()
    }

    /// Entries that can spawn a mob.
    pub fn spawnable(&self) -> impl Iterator<Item = &SpawnEntry> {
        self.entries.iter().filter(|entry| entry.blocked.is_none())
    }
}

/// Biome tags as the filter context of a `biome_filter`.
struct BiomeTags<'a>(&'a [String]);

impl FilterContext for BiomeTags<'_> {
    fn fact(&self, _subject: Subject, test: &FilterTest) -> Option<Fact> {
        match test.builtin()? {
            BuiltinTest::HasBiomeTag => {
                let want = test.value.as_str()?;
                Some(Fact::Matches(self.0.iter().any(|tag| tag == want)))
            }
            _ => None,
        }
    }
}

fn compile_entry(
    identifier: &Arc<str>,
    source: &str,
    category: Option<Category>,
    condition: &body::SpawnBiomeConditions,
    mobs: &MobRegistry,
    blocks: &BlockRegistry,
) -> Result<(SpawnEntry, Option<Filter>), RuleError> {
    if condition.is_experimental.is_some() {
        return Err(RuleError::Unimplemented(
            "minecraft:is_experimental (experimental gameplay is off, the condition never applies)",
        ));
    }
    for (present, name) in [
        (condition.delay_filter.is_some(), "minecraft:delay_filter"),
        (
            condition.mob_event_filter.is_some(),
            "minecraft:mob_event_filter",
        ),
        (
            condition.player_in_village_filter.is_some(),
            "minecraft:player_in_village_filter",
        ),
        (
            !condition.spawn_event.event.is_empty(),
            "minecraft:spawn_event",
        ),
    ] {
        if present {
            return Err(RuleError::Unimplemented(name));
        }
    }
    let filter = condition
        .biome_filter
        .as_ref()
        .map(compile_biome_filter)
        .transpose()?;
    let weight = condition.weight.as_ref();
    let herds = match &condition.herd {
        body::SpawnBiomeConditionsHerd::List(list) => list.iter().map(herd).collect(),
        body::SpawnBiomeConditionsHerd::SpawnHerd(one) => vec![herd(one)],
    };
    let permutations = match &condition.permute_type {
        body::SpawnBiomeConditionsPermuteType::List(list) => list.iter().map(permutation).collect(),
        body::SpawnBiomeConditionsPermuteType::SpawnPermuteType(one) => vec![permutation(one)],
    };
    let on_blocks = block_set(blocks, on_block_list(&condition.spawns_on_block_filter))?;
    let prevented_blocks = block_set(
        blocks,
        prevented_block_list(&condition.spawns_on_block_prevented_filter),
    )?;
    let above = &condition.spawns_above_block_filter;
    let above_set = block_set(
        blocks,
        match &above.blocks {
            body::SpawnSpawnAboveBlockFilterBlocks::List(list) => list.iter().collect(),
            body::SpawnSpawnAboveBlockFilterBlocks::BlockDescriptor(one) => vec![one],
        },
    )?;
    let distance = condition.distance_filter.as_ref().map_or(
        (DEFAULT_SPAWN_DISTANCE_MIN, DEFAULT_SPAWN_DISTANCE_MAX),
        |d| (d.min.max(0), d.max.min(SPAWN_DISTANCE_CAP)),
    );
    let world_age = condition
        .world_age_filter
        .as_ref()
        .map_or((0, u64::MAX), |age| {
            (
                u64::try_from(age.min).unwrap_or(0),
                u64::try_from(age.max).unwrap_or(u64::MAX),
            )
        });
    let mut entry = SpawnEntry {
        identifier: Arc::clone(identifier),
        actor: ActorType::from_name(identifier),
        source: source.to_owned(),
        category,
        weight: weight.map_or(DEFAULT_SPAWN_WEIGHT, |w| {
            u32::try_from(w.default).unwrap_or(0)
        }),
        rarity: weight.map_or(0, |w| u32::try_from(w.rarity).unwrap_or(0)),
        herds,
        surface: condition.spawns_on_surface.is_some(),
        underground: condition.spawns_underground.is_some(),
        underwater: condition.spawns_underwater.is_some(),
        lava: condition.spawns_lava.is_some(),
        brightness: condition
            .brightness_filter
            .as_ref()
            .map_or((0, 15), |b| (b.min, b.max)),
        height: condition
            .height_filter
            .as_ref()
            .map_or((i32::MIN, i32::MAX), |h| (h.min, h.max)),
        difficulty: condition
            .difficulty_filter
            .as_ref()
            .map_or((Difficulty::Peaceful, Difficulty::Hard), |d| {
                (d.min.into(), d.max.into())
            }),
        distance,
        world_age,
        density: condition
            .density_limit
            .as_ref()
            .map_or([-1, -1], |d| [d.surface, d.underground]),
        on_blocks,
        prevented_blocks,
        above_blocks: (!above_set.is_empty()).then_some((above_set, above.distance)),
        permutations,
        persistent: condition.is_persistent.is_some(),
        blocked: None,
    };
    entry.blocked = blocked_reason(&entry, mobs);
    Ok((entry, filter))
}

/// Why a compiled entry cannot spawn its mob, if it cannot.
fn blocked_reason(entry: &SpawnEntry, mobs: &MobRegistry) -> Option<String> {
    let mut targets: Vec<(&str, Option<ActorType>)> = Vec::new();
    if entry.permutations.is_empty()
        || entry
            .permutations
            .iter()
            .any(|p| p.entity_type.is_none() && p.weight > 0)
    {
        targets.push((&entry.identifier, entry.actor));
    }
    targets.extend(
        entry
            .permutations
            .iter()
            .filter_map(|p| Some((p.entity_type.as_deref()?, p.actor))),
    );
    for (name, actor) in targets {
        if name.contains('<') {
            return Some(format!(
                "spawning {name} with a spawn event is not implemented"
            ));
        }
        let Some(actor) = actor else {
            return Some(format!("{name} is not an actor type"));
        };
        if mobs.get(actor.name()).is_none() {
            return Some(match mobs.unsupported_reason(actor.name()) {
                Some(reason) => format!("{name} is not supported yet: {reason}"),
                None => format!("{name} is not a mob definition"),
            });
        }
    }
    // After the draw the mob class checks its walk-target value at the spawn
    // block; for an animal on a grass block it is the constant 10 (passes).
    // Elsewhere it depends on the client brightness ramp and on the native
    // mob class, neither of which is modelled.
    if entry.category != Some(Category::Animal) || !entry.on_blocks.only_grass() {
        return Some(
            "the native walk-target check is only implemented for animals spawning on grass blocks"
                .into(),
        );
    }
    None
}

fn compile_biome_filter(json: &serde_json::Value) -> Result<Filter, RuleError> {
    let mut diagnostics = unastar_rules::Diagnostics::default();
    let filter = Filter::parse(
        json,
        FilterRegistry::vanilla(),
        "biome_filter",
        &mut diagnostics,
    )?;
    if !diagnostics.items.is_empty() {
        return Err(RuleError::BiomeFilterDiagnostics(diagnostics.items));
    }
    let mut failure = None;
    filter.for_each_test(&mut |test| {
        if failure.is_none() && test.builtin() != Some(BuiltinTest::HasBiomeTag) {
            failure = Some(test.name.clone());
        }
    });
    match failure {
        Some(test) => Err(RuleError::BiomeFilterTest(test)),
        None => Ok(filter),
    }
}

fn herd(herd: &body::SpawnHerd) -> Herd {
    let text = |s: &str| (!s.is_empty()).then(|| s.to_owned());
    Herd {
        min_size: herd.min_size,
        max_size: herd.max_size,
        event: text(&herd.event),
        event_skip_count: usize::try_from(herd.event_skip_count).unwrap_or(0),
        initial_event: text(&herd.initial_event),
        initial_event_count: usize::try_from(herd.initial_event_count).unwrap_or(0),
    }
}

fn permutation(permutation: &body::SpawnPermuteType) -> Permutation {
    Permutation {
        weight: u32::try_from(permutation.weight).unwrap_or(0),
        min_guaranteed: u32::try_from(permutation.min_guaranteed).unwrap_or(0),
        entity_type: (!permutation.entity_type.is_empty())
            .then(|| Arc::from(permutation.entity_type.as_str())),
        actor: ActorType::from_name(&permutation.entity_type),
    }
}

fn on_block_list(
    filter: &body::SpawnBiomeConditionsSpawnsOnBlockFilter,
) -> Vec<&body::BlockDescriptor> {
    match filter {
        body::SpawnBiomeConditionsSpawnsOnBlockFilter::List(list) => list.iter().collect(),
        body::SpawnBiomeConditionsSpawnsOnBlockFilter::BlockDescriptor(one) => vec![one],
    }
}

fn prevented_block_list(
    filter: &body::SpawnBiomeConditionsSpawnsOnBlockPreventedFilter,
) -> Vec<&body::BlockDescriptor> {
    match filter {
        body::SpawnBiomeConditionsSpawnsOnBlockPreventedFilter::List(list) => list.iter().collect(),
        body::SpawnBiomeConditionsSpawnsOnBlockPreventedFilter::BlockDescriptor(one) => vec![one],
    }
}

/// Resolve block descriptors by name. A name no block has never matches.
fn block_set(
    blocks: &BlockRegistry,
    descriptors: Vec<&body::BlockDescriptor>,
) -> Result<BlockSet, RuleError> {
    let mut set = BlockSet {
        ranges: Vec::new(),
        names: Vec::new(),
    };
    for descriptor in descriptors {
        let name = match descriptor {
            body::BlockDescriptor::String(name) => name.as_str(),
            body::BlockDescriptor::BlockDescriptorProxy(proxy) => {
                if !proxy.states.is_empty() || !proxy.tags.is_empty() {
                    return Err(RuleError::Unimplemented(
                        "block descriptors with states or tags",
                    ));
                }
                proxy.name.as_str()
            }
        };
        let name = if name.contains(':') {
            name.to_owned()
        } else {
            format!("minecraft:{name}")
        };
        let name = LEGACY_BLOCK_NAMES
            .iter()
            .find(|(legacy, _)| *legacy == name)
            .map_or(name, |(_, current)| (*current).to_owned());
        if let Some(entry) = blocks.get_by_name(&name) {
            set.ranges.push((entry.min_state_id, entry.max_state_id));
        }
        set.names.push(name);
    }
    Ok(set)
}
