//! Block identity at build time: canonical palette, descriptor parsing,
//! pre-flattening aliases and predicate expansion.
//!
//! unastar's runtime block ids are the ordinals of protocolgen's native
//! `canonical_block_states.nbt` (the same palette `unastar-data` reads), so a
//! block descriptor in pack JSON is resolved here, once, to a `u32` constant
//! that the emitted code uses directly.
//!
//! **Defaults.** A descriptor that omits a state takes the value of the block's
//! first canonical state. In the 1.26.51 palette that state is the block's
//! default permutation for the blocks the vanilla features name (for example
//! `oak_log` starts at `pillar_axis=y`).
//!
//! **Aliases.** Older feature and biome documents still use pre-flattening
//! names whose variant lives in a state (`minecraft:stone` + `stone_type`,
//! `minecraft:tallgrass` + `tall_grass_type`, `minecraft:log` +
//! `old_log_type`, ...). [`LEGACY_BLOCKS`] maps them to the flattened
//! identifiers BDS 1.26.51 uses; [`STATE_RENAMES`] covers states that were
//! renamed on a surviving block. An unknown legacy value is a build error, never
//! a guess.

use crate::error::{Error, Result};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::RangeInclusive;
use std::path::Path;
use unastar_packs::nbt::{Encoding, Reader, Tag};

/// A block state value. Booleans are stored as `Int(0|1)` like the palette.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StateValue {
    Int(i32),
    Str(String),
}

impl std::fmt::Display for StateValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int(v) => write!(f, "{v}"),
            Self::Str(v) => write!(f, "{v:?}"),
        }
    }
}

/// A block name plus explicitly given states (already upgraded).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockDescriptor {
    pub name: String,
    pub states: BTreeMap<String, StateValue>,
}

impl BlockDescriptor {
    pub fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            states: BTreeMap::new(),
        }
    }
}

/// A block test as used by `may_replace`, `may_grow_on`, ... lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockPredicate {
    /// Matches every state of `name` whose properties agree with the given states.
    Block(BlockDescriptor),
    /// `query.any_tag(...)`: matches every state of a block carrying any tag.
    AnyTag(Vec<String>),
}

/// A pre-flattening block whose variant was encoded in one state.
pub struct LegacyBlock {
    pub name: &'static str,
    /// The variant state; `None` for plain renames.
    pub variant_state: Option<&'static str>,
    /// `(variant value, flattened name)`; the first row is the default variant.
    /// Integer variant values index this list (old data values).
    pub variants: &'static [(&'static str, &'static str)],
}

/// Pre-flattening names that appear in shipped data or classic documents.
/// Values follow the Bedrock flattening changelogs (1.20.x-1.21.x).
pub const LEGACY_BLOCKS: &[LegacyBlock] = &[
    LegacyBlock {
        name: "minecraft:grass",
        variant_state: None,
        variants: &[("", "minecraft:grass_block")],
    },
    LegacyBlock {
        name: "minecraft:yellow_flower",
        variant_state: None,
        variants: &[("", "minecraft:dandelion")],
    },
    LegacyBlock {
        name: "minecraft:stone",
        variant_state: Some("stone_type"),
        variants: &[
            ("stone", "minecraft:stone"),
            ("granite", "minecraft:granite"),
            ("granite_smooth", "minecraft:polished_granite"),
            ("diorite", "minecraft:diorite"),
            ("diorite_smooth", "minecraft:polished_diorite"),
            ("andesite", "minecraft:andesite"),
            ("andesite_smooth", "minecraft:polished_andesite"),
        ],
    },
    LegacyBlock {
        name: "minecraft:dirt",
        variant_state: Some("dirt_type"),
        variants: &[
            ("normal", "minecraft:dirt"),
            ("coarse", "minecraft:coarse_dirt"),
        ],
    },
    LegacyBlock {
        name: "minecraft:sand",
        variant_state: Some("sand_type"),
        variants: &[("normal", "minecraft:sand"), ("red", "minecraft:red_sand")],
    },
    LegacyBlock {
        name: "minecraft:tallgrass",
        variant_state: Some("tall_grass_type"),
        variants: &[
            ("default", "minecraft:short_grass"),
            ("tall", "minecraft:short_grass"),
            ("fern", "minecraft:fern"),
        ],
    },
    LegacyBlock {
        name: "minecraft:planks",
        variant_state: Some("wood_type"),
        variants: WOOD_PLANKS,
    },
    LegacyBlock {
        name: "minecraft:log",
        variant_state: Some("old_log_type"),
        variants: &[
            ("oak", "minecraft:oak_log"),
            ("spruce", "minecraft:spruce_log"),
            ("birch", "minecraft:birch_log"),
            ("jungle", "minecraft:jungle_log"),
        ],
    },
    LegacyBlock {
        name: "minecraft:log2",
        variant_state: Some("new_log_type"),
        variants: &[
            ("acacia", "minecraft:acacia_log"),
            ("dark_oak", "minecraft:dark_oak_log"),
        ],
    },
    LegacyBlock {
        name: "minecraft:leaves",
        variant_state: Some("old_leaf_type"),
        variants: &[
            ("oak", "minecraft:oak_leaves"),
            ("spruce", "minecraft:spruce_leaves"),
            ("birch", "minecraft:birch_leaves"),
            ("jungle", "minecraft:jungle_leaves"),
        ],
    },
    LegacyBlock {
        name: "minecraft:leaves2",
        variant_state: Some("new_leaf_type"),
        variants: &[
            ("acacia", "minecraft:acacia_leaves"),
            ("dark_oak", "minecraft:dark_oak_leaves"),
        ],
    },
    LegacyBlock {
        name: "minecraft:red_flower",
        variant_state: Some("flower_type"),
        variants: &[
            ("poppy", "minecraft:poppy"),
            ("orchid", "minecraft:blue_orchid"),
            ("allium", "minecraft:allium"),
            ("houstonia", "minecraft:azure_bluet"),
            ("tulip_red", "minecraft:red_tulip"),
            ("tulip_orange", "minecraft:orange_tulip"),
            ("tulip_white", "minecraft:white_tulip"),
            ("tulip_pink", "minecraft:pink_tulip"),
            ("oxeye", "minecraft:oxeye_daisy"),
            ("cornflower", "minecraft:cornflower"),
            ("lily_of_the_valley", "minecraft:lily_of_the_valley"),
        ],
    },
    LegacyBlock {
        name: "minecraft:double_plant",
        variant_state: Some("double_plant_type"),
        variants: &[
            ("sunflower", "minecraft:sunflower"),
            ("syringa", "minecraft:lilac"),
            ("grass", "minecraft:tall_grass"),
            ("fern", "minecraft:large_fern"),
            ("rose", "minecraft:rose_bush"),
            ("paeonia", "minecraft:peony"),
        ],
    },
    LegacyBlock {
        name: "minecraft:sapling",
        variant_state: Some("sapling_type"),
        variants: &[
            ("oak", "minecraft:oak_sapling"),
            ("spruce", "minecraft:spruce_sapling"),
            ("birch", "minecraft:birch_sapling"),
            ("jungle", "minecraft:jungle_sapling"),
            ("acacia", "minecraft:acacia_sapling"),
            ("dark_oak", "minecraft:dark_oak_sapling"),
        ],
    },
];

const WOOD_PLANKS: &[(&str, &str)] = &[
    ("oak", "minecraft:oak_planks"),
    ("spruce", "minecraft:spruce_planks"),
    ("birch", "minecraft:birch_planks"),
    ("jungle", "minecraft:jungle_planks"),
    ("acacia", "minecraft:acacia_planks"),
    ("dark_oak", "minecraft:dark_oak_planks"),
];

/// A state renamed on a block that kept its identifier.
pub struct StateRename {
    pub block: &'static str,
    pub old: &'static str,
    pub new: &'static str,
    /// `(old integer value, new string value)`.
    pub values: &'static [(i32, &'static str)],
}

/// Legacy `direction` (0 south, 1 west, 2 north, 3 east) became
/// `minecraft:cardinal_direction` on several blocks.
pub const STATE_RENAMES: &[StateRename] = &[StateRename {
    block: "minecraft:big_dripleaf",
    old: "direction",
    new: "minecraft:cardinal_direction",
    values: &[(0, "south"), (1, "west"), (2, "north"), (3, "east")],
}];

fn namespaced(name: &str) -> String {
    if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    }
}

fn json_state(origin: &str, key: &str, value: &Value) -> Result<StateValue> {
    match value {
        Value::Bool(b) => Ok(StateValue::Int(i32::from(*b))),
        Value::Number(n) => n
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .map(StateValue::Int)
            .ok_or_else(|| Error::data(origin, format!("state {key}={n} is not an i32"))),
        Value::String(s) => Ok(StateValue::Str(s.clone())),
        other => Err(Error::data(
            origin,
            format!("state {key}={other} has an unsupported type"),
        )),
    }
}

/// Apply [`LEGACY_BLOCKS`] and [`STATE_RENAMES`].
pub fn upgrade(origin: &str, mut desc: BlockDescriptor) -> Result<BlockDescriptor> {
    desc.name = namespaced(&desc.name);
    if let Some(legacy) = LEGACY_BLOCKS.iter().find(|l| l.name == desc.name) {
        let new_name = match legacy.variant_state {
            None => legacy.variants[0].1,
            Some(key) => match desc.states.remove(key) {
                None => legacy.variants[0].1,
                Some(StateValue::Str(v)) => legacy
                    .variants
                    .iter()
                    .find(|(name, _)| *name == v)
                    .map(|(_, new)| *new)
                    .ok_or_else(|| {
                        Error::data(origin, format!("{} {key}={v:?} has no alias", desc.name))
                    })?,
                Some(StateValue::Int(i)) => usize::try_from(i)
                    .ok()
                    .and_then(|i| legacy.variants.get(i))
                    .map(|(_, new)| *new)
                    .ok_or_else(|| {
                        Error::data(origin, format!("{} {key}={i} has no alias", desc.name))
                    })?,
            },
        };
        desc.name = new_name.to_owned();
    }
    for rename in STATE_RENAMES.iter().filter(|r| r.block == desc.name) {
        if let Some(old) = desc.states.remove(rename.old) {
            let StateValue::Int(i) = old else {
                return Err(Error::data(
                    origin,
                    format!("{} {}={old} is not an integer", desc.name, rename.old),
                ));
            };
            let new = rename
                .values
                .iter()
                .find(|(v, _)| *v == i)
                .map(|(_, n)| *n)
                .ok_or_else(|| {
                    Error::data(
                        origin,
                        format!("{} {}={i} has no rename", desc.name, rename.old),
                    )
                })?;
            desc.states
                .insert(rename.new.to_owned(), StateValue::Str(new.to_owned()));
        }
    }
    Ok(desc)
}

/// Parse a block descriptor (`"minecraft:x"` or `{"name": ..., "states": {...}}`).
pub fn parse_descriptor(origin: &str, value: &Value) -> Result<BlockDescriptor> {
    let desc = match value {
        Value::String(name) => BlockDescriptor::named(name),
        Value::Object(map) => {
            let name = map.get("name").and_then(Value::as_str).ok_or_else(|| {
                Error::data(origin, format!("block descriptor without name: {value}"))
            })?;
            if let Some(extra) = map.keys().find(|k| *k != "name" && *k != "states") {
                return Err(Error::data(
                    origin,
                    format!("unknown block descriptor key {extra:?}"),
                ));
            }
            let mut desc = BlockDescriptor::named(name);
            if let Some(states) = map.get("states") {
                let states = states
                    .as_object()
                    .ok_or_else(|| Error::data(origin, "block states is not an object"))?;
                for (k, v) in states {
                    desc.states.insert(k.clone(), json_state(origin, k, v)?);
                }
            }
            desc
        }
        other => {
            return Err(Error::data(
                origin,
                format!("not a block descriptor: {other}"),
            ));
        }
    };
    upgrade(origin, desc)
}

/// Parse a list entry that may also be `{"tags": "query.any_tag('a', ...)"}`.
pub fn parse_predicate(origin: &str, value: &Value) -> Result<BlockPredicate> {
    if let Some(tags) = value.get("tags") {
        let expr = tags
            .as_str()
            .ok_or_else(|| Error::data(origin, "block tags is not a string"))?;
        return parse_any_tag(expr)
            .map(BlockPredicate::AnyTag)
            .ok_or_else(|| {
                Error::data(origin, format!("unsupported block tag expression {expr:?}"))
            });
    }
    parse_descriptor(origin, value).map(BlockPredicate::Block)
}

/// Parse `query.any_tag('a', 'b')` (or `q.any_tag`) into its tag list.
pub fn parse_any_tag(expr: &str) -> Option<Vec<String>> {
    let expr = expr.trim();
    let args = expr
        .strip_prefix("query.any_tag(")
        .or_else(|| expr.strip_prefix("q.any_tag("))?
        .strip_suffix(')')?;
    args.split(',')
        .map(|arg| {
            let arg = arg.trim();
            arg.strip_prefix('\'')
                .and_then(|a| a.strip_suffix('\''))
                .filter(|a| !a.is_empty() && !a.contains('\''))
                .map(str::to_owned)
        })
        .collect()
}

/// A set of runtime ids as sorted, merged inclusive ranges.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct IdSet {
    ranges: Vec<RangeInclusive<u32>>,
}

impl IdSet {
    pub fn from_ids(ids: impl IntoIterator<Item = u32>) -> Self {
        let mut ids: Vec<u32> = ids.into_iter().collect();
        ids.sort_unstable();
        ids.dedup();
        let mut ranges: Vec<RangeInclusive<u32>> = Vec::new();
        for id in ids {
            match ranges.last_mut() {
                Some(r) if *r.end() + 1 == id => *r = *r.start()..=id,
                _ => ranges.push(id..=id),
            }
        }
        Self { ranges }
    }

    pub fn union(&self, other: &IdSet) -> IdSet {
        Self::from_ids(self.iter().chain(other.iter()))
    }

    pub fn ranges(&self) -> &[RangeInclusive<u32>] {
        &self.ranges
    }

    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.ranges.iter().flat_map(|r| r.clone())
    }

    pub fn contains(&self, id: u32) -> bool {
        self.ranges.iter().any(|r| r.contains(&id))
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }
}

#[derive(Debug, Clone)]
struct BlockRange {
    first: u32,
    /// One entry per state ordinal in `first..first+len`.
    states: Vec<BTreeMap<String, StateValue>>,
}

/// Physical flags of one state (protocolgen native BDS measurements).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlockFlags {
    /// `solid` of the state (block_state_data.json).
    pub solid: bool,
    /// `opaqueFullBlock` of the state.
    pub opaque_full_block: bool,
    /// The block's material is a liquid (block_properties_table.json).
    pub liquid: bool,
    /// The block's material blocks motion.
    pub blocks_motion: bool,
}

/// The canonical block palette plus block tags and physical flags.
#[derive(Debug, Clone, Default)]
pub struct BlockPalette {
    blocks: HashMap<String, BlockRange>,
    names_by_tag: HashMap<String, BTreeSet<String>>,
    /// Block names in palette order, for id -> name lookups.
    order: Vec<(u32, String)>,
    /// One entry per runtime id (empty when built without measurements).
    flags: Vec<BlockFlags>,
    len: u32,
}

impl BlockPalette {
    /// Load `canonical_block_states.nbt` and `block_tags.json` from protocolgen's
    /// native 1.26.51 directory.
    pub fn load(native_dir: &Path) -> Result<Self> {
        let nbt_path = native_dir.join("canonical_block_states.nbt");
        let bytes = std::fs::read(&nbt_path).map_err(|e| Error::io(&nbt_path, e))?;
        let origin = nbt_path.display().to_string();
        let mut reader = Reader::new(&bytes, Encoding::NetworkLittleEndian, &origin);
        let mut states = Vec::new();
        while !reader.is_empty() {
            let (_, tag) = reader.read_root()?;
            let name = tag
                .get("name")
                .and_then(Tag::as_str)
                .ok_or_else(|| Error::data(&origin, format!("state {} has no name", states.len())))?
                .to_owned();
            let props = tag
                .get("states")
                .and_then(Tag::as_compound)
                .ok_or_else(|| {
                    Error::data(&origin, format!("state {} has no states", states.len()))
                })?;
            let mut map = BTreeMap::new();
            for (k, v) in props {
                let value = match v {
                    Tag::String(s) => StateValue::Str(s.clone()),
                    other => StateValue::Int(
                        other
                            .as_i64()
                            .and_then(|i| i32::try_from(i).ok())
                            .ok_or_else(|| {
                                Error::data(&origin, format!("{name} state {k} is not integral"))
                            })?,
                    ),
                };
                map.insert(k.clone(), value);
            }
            states.push((name, map));
        }
        let tags_path = native_dir.join("block_tags.json");
        let tags_bytes = std::fs::read(&tags_path).map_err(|e| Error::io(&tags_path, e))?;
        let tags: BTreeMap<String, Vec<String>> =
            serde_json::from_slice(&tags_bytes).map_err(|source| Error::Json {
                origin: tags_path.display().to_string(),
                source,
            })?;
        let mut palette = Self::from_parts(states, tags)?;
        palette.flags = load_flags(native_dir, &palette)?;
        Ok(palette)
    }

    /// Build from `(name, states)` in palette order and a `block -> tags` map.
    pub fn from_parts(
        states: Vec<(String, BTreeMap<String, StateValue>)>,
        tags: BTreeMap<String, Vec<String>>,
    ) -> Result<Self> {
        let mut palette = BlockPalette {
            len: u32::try_from(states.len()).expect("palette fits u32"),
            ..Default::default()
        };
        let mut previous: Option<String> = None;
        for (ordinal, (name, props)) in states.into_iter().enumerate() {
            let ordinal = ordinal as u32;
            if previous.as_deref() != Some(name.as_str()) {
                if palette.blocks.contains_key(&name) {
                    return Err(Error::data(
                        "canonical palette",
                        format!("{name} is not contiguous"),
                    ));
                }
                palette.blocks.insert(
                    name.clone(),
                    BlockRange {
                        first: ordinal,
                        states: Vec::new(),
                    },
                );
                palette.order.push((ordinal, name.clone()));
                previous = Some(name.clone());
            }
            palette
                .blocks
                .get_mut(&name)
                .expect("inserted")
                .states
                .push(props);
        }
        for (block, block_tags) in tags {
            for tag in block_tags {
                palette
                    .names_by_tag
                    .entry(tag)
                    .or_default()
                    .insert(block.clone());
            }
        }
        Ok(palette)
    }

    /// Number of states in the palette.
    pub fn len(&self) -> u32 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn has_block(&self, name: &str) -> bool {
        self.blocks.contains_key(name)
    }

    /// Every block name in palette order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.order.iter().map(|(_, n)| n.as_str())
    }

    /// `(block name, runtime id of its first state, the state's properties)` of `id`.
    pub fn describe(&self, id: u32) -> Option<(&str, u32, &BTreeMap<String, StateValue>)> {
        let at = self
            .order
            .partition_point(|(first, _)| *first <= id)
            .checked_sub(1)?;
        let (first, name) = &self.order[at];
        let range = &self.blocks[name];
        let state = range.states.get((id - first) as usize)?;
        Some((name, *first, state))
    }

    /// Physical flags of a runtime id (all `false` for palettes built without
    /// measurements, i.e. in unit tests).
    pub fn flags(&self, id: u32) -> BlockFlags {
        self.flags.get(id as usize).copied().unwrap_or_default()
    }

    /// Every runtime id whose flags satisfy `pred`.
    pub fn ids_where(&self, pred: impl Fn(BlockFlags) -> bool) -> IdSet {
        IdSet::from_ids((0..self.len).filter(|&id| pred(self.flags(id))))
    }

    /// Tags carried by block `name`.
    pub fn tags_of(&self, name: &str) -> impl Iterator<Item = &str> {
        self.names_by_tag
            .iter()
            .filter(move |(_, names)| names.contains(name))
            .map(|(tag, _)| tag.as_str())
    }

    /// Every state of `name` as `(runtime id, states)`.
    pub fn states_of(
        &self,
        name: &str,
    ) -> Option<impl Iterator<Item = (u32, &BTreeMap<String, StateValue>)>> {
        let range = self.blocks.get(name)?;
        Some(
            range
                .states
                .iter()
                .enumerate()
                .map(|(i, s)| (range.first + i as u32, s)),
        )
    }

    fn block(&self, origin: &str, desc: &BlockDescriptor) -> Result<&BlockRange> {
        let range = self
            .blocks
            .get(&desc.name)
            .ok_or_else(|| Error::data(origin, format!("unknown block {}", desc.name)))?;
        let default = &range.states[0];
        for (key, value) in &desc.states {
            if !default.contains_key(key) {
                return Err(Error::data(
                    origin,
                    format!("{} has no state {key}", desc.name),
                ));
            }
            if !range.states.iter().any(|s| s.get(key) == Some(value)) {
                return Err(Error::data(
                    origin,
                    format!("{} {key}={value} is not a valid value", desc.name),
                ));
            }
        }
        Ok(range)
    }

    /// Runtime id of the exact state: defaults overridden by `desc.states`.
    pub fn resolve(&self, origin: &str, desc: &BlockDescriptor) -> Result<u32> {
        let range = self.block(origin, desc)?;
        let mut wanted = range.states[0].clone();
        wanted.extend(desc.states.clone());
        range
            .states
            .iter()
            .position(|s| *s == wanted)
            .map(|i| range.first + i as u32)
            .ok_or_else(|| {
                Error::data(
                    origin,
                    format!("{desc:?} is not a canonical state combination"),
                )
            })
    }

    /// Every runtime id a predicate matches.
    pub fn matching(&self, origin: &str, pred: &BlockPredicate) -> Result<IdSet> {
        match pred {
            BlockPredicate::Block(desc) => {
                let range = self.block(origin, desc)?;
                Ok(IdSet::from_ids(range.states.iter().enumerate().filter_map(
                    |(i, s)| {
                        desc.states
                            .iter()
                            .all(|(k, v)| s.get(k) == Some(v))
                            .then_some(range.first + i as u32)
                    },
                )))
            }
            BlockPredicate::AnyTag(tags) => {
                let mut ids = Vec::new();
                for tag in tags {
                    let names = self.names_by_tag.get(tag).ok_or_else(|| {
                        Error::data(origin, format!("no block carries tag {tag:?}"))
                    })?;
                    for name in names {
                        let range = &self.blocks[name];
                        ids.extend(range.first..range.first + range.states.len() as u32);
                    }
                }
                Ok(IdSet::from_ids(ids))
            }
        }
    }

    /// Every runtime id of the named blocks (all states).
    pub fn all_states(&self, origin: &str, names: &[&str]) -> Result<IdSet> {
        let mut out = IdSet::default();
        for name in names {
            out = out.union(
                &self.matching(origin, &BlockPredicate::Block(BlockDescriptor::named(name)))?,
            );
        }
        Ok(out)
    }
}

fn load_flags(native_dir: &Path, palette: &BlockPalette) -> Result<Vec<BlockFlags>> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct StateData {
        solid: bool,
        opaque_full_block: bool,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Material {
        liquid: bool,
        blocks_motion: bool,
    }
    #[derive(serde::Deserialize)]
    struct Properties {
        material: Material,
    }
    let read = |name: &str| -> Result<Vec<u8>> {
        let path = native_dir.join(name);
        std::fs::read(&path).map_err(|e| Error::io(&path, e))
    };
    let json_err = |name: &str| {
        let origin = native_dir.join(name).display().to_string();
        move |source| Error::Json { origin, source }
    };
    let states: HashMap<u32, StateData> = serde_json::from_slice(&read("block_state_data.json")?)
        .map_err(json_err("block_state_data.json"))?;
    let props: HashMap<String, Properties> =
        serde_json::from_slice(&read("block_properties_table.json")?)
            .map_err(json_err("block_properties_table.json"))?;
    let mut out = Vec::with_capacity(palette.len as usize);
    for id in 0..palette.len {
        let (name, _, _) = palette.describe(id).expect("id in palette");
        let state = states.get(&id).ok_or_else(|| {
            Error::data(
                "block_state_data.json",
                format!("no measurements for state {id}"),
            )
        })?;
        let material = &props
            .get(name)
            .ok_or_else(|| {
                Error::data(
                    "block_properties_table.json",
                    format!("no properties for {name}"),
                )
            })?
            .material;
        out.push(BlockFlags {
            solid: state.solid,
            opaque_full_block: state.opaque_full_block,
            liquid: material.liquid,
            blocks_motion: material.blocks_motion,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn palette() -> BlockPalette {
        let s = |name: &str, props: &[(&str, StateValue)]| {
            (
                name.to_owned(),
                props
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect(),
            )
        };
        let axis = |v: &str| ("pillar_axis", StateValue::Str(v.into()));
        BlockPalette::from_parts(
            vec![
                s("minecraft:air", &[]),
                s("minecraft:oak_log", &[axis("y")]),
                s("minecraft:oak_log", &[axis("x")]),
                s("minecraft:oak_log", &[axis("z")]),
                s("minecraft:andesite", &[]),
                s("minecraft:short_grass", &[]),
                s("minecraft:fern", &[]),
                s(
                    "minecraft:big_dripleaf",
                    &[
                        ("big_dripleaf_head", StateValue::Int(0)),
                        (
                            "minecraft:cardinal_direction",
                            StateValue::Str("south".into()),
                        ),
                    ],
                ),
                s(
                    "minecraft:big_dripleaf",
                    &[
                        ("big_dripleaf_head", StateValue::Int(1)),
                        (
                            "minecraft:cardinal_direction",
                            StateValue::Str("west".into()),
                        ),
                    ],
                ),
            ],
            BTreeMap::from([
                ("minecraft:oak_log".to_owned(), vec!["log".to_owned()]),
                ("minecraft:andesite".to_owned(), vec!["stone".to_owned()]),
            ]),
        )
        .unwrap()
    }

    #[test]
    fn omitted_states_take_the_first_canonical_state() {
        let p = palette();
        let log = parse_descriptor("t", &json!("minecraft:oak_log")).unwrap();
        assert_eq!(p.resolve("t", &log).unwrap(), 1);
        let x = parse_descriptor(
            "t",
            &json!({"name": "oak_log", "states": {"pillar_axis": "x"}}),
        )
        .unwrap();
        assert_eq!(p.resolve("t", &x).unwrap(), 2);
    }

    #[test]
    fn legacy_names_upgrade_through_their_variant_state() {
        let p = palette();
        let old_log = parse_descriptor("t", &json!({"name": "minecraft:log", "states": {"old_log_type": "oak", "pillar_axis": "z"}})).unwrap();
        assert_eq!(p.resolve("t", &old_log).unwrap(), 3);
        let andesite = parse_descriptor(
            "t",
            &json!({"name": "minecraft:stone", "states": {"stone_type": "andesite"}}),
        )
        .unwrap();
        assert_eq!(andesite.name, "minecraft:andesite");
        let by_data_value = parse_descriptor(
            "t",
            &json!({"name": "minecraft:stone", "states": {"stone_type": 5}}),
        )
        .unwrap();
        assert_eq!(by_data_value.name, "minecraft:andesite");
        let fern = parse_descriptor(
            "t",
            &json!({"name": "minecraft:tallgrass", "states": {"tall_grass_type": "fern"}}),
        )
        .unwrap();
        assert_eq!(p.resolve("t", &fern).unwrap(), 6);
        assert_eq!(
            parse_descriptor("t", &json!("minecraft:tallgrass"))
                .unwrap()
                .name,
            "minecraft:short_grass"
        );
        assert_eq!(
            parse_descriptor("t", &json!("minecraft:grass"))
                .unwrap()
                .name,
            "minecraft:grass_block"
        );
    }

    #[test]
    fn unknown_legacy_values_fail() {
        let err = parse_descriptor(
            "f.json",
            &json!({"name": "minecraft:stone", "states": {"stone_type": "marble"}}),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("f.json") && err.contains("marble"), "{err}");
    }

    #[test]
    fn renamed_direction_state_maps_to_cardinal_direction() {
        let p = palette();
        let leaf = parse_descriptor("t", &json!({"name": "minecraft:big_dripleaf", "states": {"big_dripleaf_head": true, "direction": 1}})).unwrap();
        assert_eq!(p.resolve("t", &leaf).unwrap(), 8);
    }

    #[test]
    fn invalid_states_are_build_errors() {
        let p = palette();
        let bad_key = parse_descriptor(
            "t",
            &json!({"name": "minecraft:oak_log", "states": {"color": "red"}}),
        )
        .unwrap();
        assert!(
            p.resolve("t", &bad_key)
                .unwrap_err()
                .to_string()
                .contains("no state color")
        );
        let bad_value = parse_descriptor(
            "t",
            &json!({"name": "minecraft:oak_log", "states": {"pillar_axis": "w"}}),
        )
        .unwrap();
        assert!(p.resolve("t", &bad_value).is_err());
        assert!(
            p.resolve("t", &BlockDescriptor::named("minecraft:nope"))
                .is_err()
        );
    }

    #[test]
    fn predicates_expand_to_merged_ranges() {
        let p = palette();
        let any_log = parse_predicate("t", &json!("minecraft:oak_log")).unwrap();
        assert_eq!(p.matching("t", &any_log).unwrap().ranges(), &[1..=3]);
        let tags = parse_predicate("t", &json!({"tags": "query.any_tag('log', 'stone')"})).unwrap();
        assert_eq!(p.matching("t", &tags).unwrap().ranges(), &[1..=4]);
        let z = parse_predicate(
            "t",
            &json!({"name": "minecraft:oak_log", "states": {"pillar_axis": "z"}}),
        )
        .unwrap();
        assert_eq!(p.matching("t", &z).unwrap().ranges(), &[3..=3]);
        assert!(
            p.matching("t", &BlockPredicate::AnyTag(vec!["nope".into()]))
                .is_err()
        );
    }

    #[test]
    fn any_tag_parser_accepts_only_quoted_lists() {
        assert_eq!(
            parse_any_tag("query.any_tag('dirt', 'sand')"),
            Some(vec!["dirt".into(), "sand".into()])
        );
        assert_eq!(parse_any_tag("query.any_tag(dirt)"), None);
        assert_eq!(parse_any_tag("query.all_tags('dirt')"), None);
    }
}
