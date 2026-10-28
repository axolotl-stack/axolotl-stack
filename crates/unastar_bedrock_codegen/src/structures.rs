//! Data-driven structures: jigsaw structures (`worldgen/structures`), template
//! pools (`worldgen/template_pools`), structure sets (`worldgen/structure_sets`),
//! processor lists (`worldgen/processors`) and the structure templates they
//! place, plus the templates of `structure_template_feature` and
//! `fossil_feature`.
//!
//! **Templates.** A template location `a/b` is the file
//! `behavior_packs/<pack>/structures/a/b.nbt` (gzip, big-endian Java structure
//! format) or `.../a/b.mcstructure` (little-endian Bedrock format). Later packs
//! of the [`PackStack`] override earlier ones; the `structures/*.brarchive`
//! entries are empty placeholders and are not read. Every template is decoded
//! into a palette of runtime ids (one per horizontal rotation) and a list of
//! non-void cells.
//!
//! **Java palettes.** `.nbt` templates use Java block names and properties.
//! [`java_descriptor`] maps each `(name, properties)` to a Bedrock 1.26.51
//! primary state. Template lowering retains `waterlogged` and intrinsic plant
//! water in a separate layer before projection collapses dry/wet primary ids.
//! Other properties are mapped or explicitly dropped with a reason; unknown
//! properties fail the build. Dropped: leaves `distance` (not stored), and `snowy`
//! (Bedrock derives the snowy look from the block above), campfire
//! `signal_fire` (derived from the block below), chest `type` (Bedrock pairs
//! chests through block entities), copper golem statue pose and vine `up`
//! (no Bedrock state). Connection states that Bedrock 1.26.51 does store
//! (fence/pane `minecraft:connection_*`, wall `wall_connection_type_*`, stair
//! `minecraft:corner`) are mapped instead of being recomputed.
//!
//! **Rotation.** Pieces are placed in 4 clockwise quarter turns (seen from
//! above: north -> east -> south -> west). Java properties are rotated before
//! conversion ([`rotate_java_properties`]); Bedrock states of `.mcstructure`
//! palettes are rotated directly ([`rotate_bedrock`]). Positions rotate as
//! `(x, z) -> (-z, x)` per quarter turn ([`rotate_offset`]).
//!
//! **Special cells.** Native void indices preserve each destination layer
//! independently; cells with two void layers are omitted. `structure_block`
//! data blocks are skipped; Java `structure_void` leaves the world unchanged. Jigsaws
//! become their `final_state` and are recorded as [`Connector`]s for assembly.
//! Block actors are compiled to native identities and static network-NBT fields;
//! template entities and processor loot modifiers remain separate concerns.
//! Both native layers are retained in paired, rotation-resolved palette entries;
//! packed cells stay five bytes. No runtime NBT parsing or liquid inference.

use crate::blocks::{self, BlockDescriptor, BlockPalette, StateValue};
use crate::error::{Error, Result};
use crate::ir::{self, BiomeFilter, Feature, Obj};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use unastar_packs::block_entity::{self, CompiledBlockEntity};
use unastar_packs::nbt::{self, Encoding, Reader, Tag};
use unastar_packs::packs::{Document, Family, PackStack};

/// The built-in empty pool / processor list / jigsaw target.
pub const EMPTY: &str = "minecraft:empty";
/// Largest `limit` of a `minecraft:capped` processor (the runtime keeps the
/// selected cells in a fixed array).
pub const MAX_CAPPED_LIMIT: u32 = 64;
/// Largest number of `minecraft:capped` processors in one list.
pub const MAX_CAPPED_PER_LIST: usize = 4;

/// A template layer left unchanged at placement; never a block runtime id.
pub const KEEP_LAYER: u32 = u32::MAX;

/// Generation steps a jigsaw structure may name, in execution order.
pub const STEPS: [&str; 11] = [
    "raw_generation",
    "lakes",
    "local_modifications",
    "underground_structures",
    "surface_structures",
    "strongholds",
    "underground_ores",
    "underground_decoration",
    "fluid_springs",
    "vegetal_decoration",
    "top_layer_modification",
];

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

/// The six block faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Dir6 {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Dir6 {
    /// Parse a lower-case direction name.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "down" => Self::Down,
            "up" => Self::Up,
            "north" => Self::North,
            "south" => Self::South,
            "west" => Self::West,
            "east" => Self::East,
            _ => return None,
        })
    }

    /// Lower-case direction name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::Up => "up",
            Self::North => "north",
            Self::South => "south",
            Self::West => "west",
            Self::East => "east",
        }
    }

    /// Rotate clockwise (seen from above) by `quarter_turns`; up/down are fixed.
    pub fn rotate(self, quarter_turns: u8) -> Self {
        let mut d = self;
        for _ in 0..quarter_turns % 4 {
            d = match d {
                Self::North => Self::East,
                Self::East => Self::South,
                Self::South => Self::West,
                Self::West => Self::North,
                vertical => vertical,
            };
        }
        d
    }
}

/// Rotate a horizontal offset clockwise by `quarter_turns`: `(x, z) -> (-z, x)`
/// per turn (east becomes south).
pub fn rotate_offset(x: i32, z: i32, quarter_turns: u8) -> (i32, i32) {
    let (mut x, mut z) = (x, z);
    for _ in 0..quarter_turns % 4 {
        (x, z) = (-z, x);
    }
    (x, z)
}

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

/// A jigsaw connector of a template (unrotated, template-local).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connector {
    pub pos: [i32; 3],
    /// The face the connector attaches through.
    pub front: Dir6,
    /// The connector's up direction (matters for vertical `aligned` joints).
    pub top: Dir6,
    pub name: String,
    pub target: String,
    /// Pool the attached piece is drawn from (`minecraft:empty` for none).
    pub pool: String,
    /// `joint: rollable` (any child rotation around the connection axis).
    pub rollable: bool,
    pub selection_priority: i32,
    pub placement_priority: i32,
}

/// A block actor attached to one packed template cell.
#[derive(Debug, Clone)]
pub struct TemplateBlockEntity {
    pub cell: u32,
    pub data: CompiledBlockEntity,
    /// Java-format source loot identifiers need the native loot-path conversion.
    pub java_loot: bool,
}

/// One deduplicated pair of layers, resolved for all clockwise quarter turns.
///
/// Native `-1` indices retain the destination layer, independently of the other
/// layer. Palette pairs keep the packed cell format at five bytes per cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TemplateState {
    pub primary: [u32; 4],
    pub secondary: [u32; 4],
}

/// A decoded structure template.
#[derive(Debug, Clone)]
pub struct Template {
    /// Location below `structures/` without extension (`fossils/fossil_skull_01`),
    /// or `feature:<id>` for the 1x1x1 shape of a feature pool element.
    pub key: String,
    /// File the template was read from (for errors and docs).
    pub origin: String,
    pub size: [i32; 3],
    /// Paired layer runtime ids per clockwise quarter turn.
    pub palette: Vec<TemplateState>,
    /// Non-void cells: template-local position and palette index.
    pub cells: Vec<([u8; 3], u16)>,
    pub connectors: Vec<Connector>,
    pub block_entities: Vec<TemplateBlockEntity>,
}

/// Accumulates a template's palette and cells.
struct TemplateBuilder {
    template: Template,
    dedupe: BTreeMap<TemplateState, u16>,
}

impl TemplateBuilder {
    fn new(key: &str, origin: &str, size: [i32; 3]) -> Result<Self> {
        if size.iter().any(|&s| !(1..=255).contains(&s)) {
            return Err(Error::data(
                origin,
                format!("template size {size:?} outside 1..=255"),
            ));
        }
        Ok(Self {
            template: Template {
                key: key.to_owned(),
                origin: origin.to_owned(),
                size,
                palette: Vec::new(),
                cells: Vec::new(),
                connectors: Vec::new(),
                block_entities: Vec::new(),
            },
            dedupe: BTreeMap::new(),
        })
    }

    fn entry(&mut self, ids: TemplateState) -> Result<u16> {
        if let Some(&i) = self.dedupe.get(&ids) {
            return Ok(i);
        }
        let i = u16::try_from(self.template.palette.len())
            .map_err(|_| Error::data(&self.template.origin, "more than 65535 palette entries"))?;
        self.template.palette.push(ids);
        self.dedupe.insert(ids, i);
        Ok(i)
    }

    fn cell(&mut self, pos: [i32; 3], entry: u16) -> Result<()> {
        let size = self.template.size;
        if (0..3).any(|a| pos[a] < 0 || pos[a] >= size[a]) {
            return Err(Error::data(
                &self.template.origin,
                format!("cell {pos:?} outside the template size {size:?}"),
            ));
        }
        self.template
            .cells
            .push(([pos[0] as u8, pos[1] as u8, pos[2] as u8], entry));
        Ok(())
    }

    fn block_entity(&mut self, tag: &Tag, java_loot: bool) -> Result<()> {
        let source = if java_loot {
            block_entity::Source::Java
        } else {
            block_entity::Source::Bedrock
        };
        let data = block_entity::compile(&self.template.origin, tag, source)?;
        let cell =
            u32::try_from(self.template.cells.len() - 1).expect("template cell count fits u32");
        self.template.block_entities.push(TemplateBlockEntity {
            cell,
            data,
            java_loot,
        });
        Ok(())
    }
}

/// Every template file below `<pack>/structures`, later layers overriding
/// earlier ones, keyed by location (relative path without extension).
pub fn template_files(stack: &PackStack) -> Result<BTreeMap<String, PathBuf>> {
    let mut out = BTreeMap::new();
    for layer in stack.layers.iter().filter(|l| !l.is_definitions) {
        let root = layer.root.join("structures");
        let mut files = Vec::new();
        walk(&root, &mut files);
        let mut in_layer: BTreeMap<String, PathBuf> = BTreeMap::new();
        for path in files {
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if ext != "nbt" && ext != "mcstructure" {
                continue;
            }
            let Ok(rel) = path.strip_prefix(&root) else {
                continue;
            };
            let rel = rel.with_extension("");
            let key = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            if let Some(previous) = in_layer.insert(key.clone(), path.clone()) {
                return Err(Error::data(
                    path.display().to_string(),
                    format!(
                        "template {key} also defined in the same layer by {}",
                        previous.display()
                    ),
                ));
            }
            out.insert(key, path);
        }
    }
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Read and decode the template file at `path`.
pub fn load_template(palette: &BlockPalette, key: &str, path: &Path) -> Result<Template> {
    let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
    let origin = path.display().to_string();
    match path.extension().and_then(|e| e.to_str()) {
        Some("nbt") => load_java_template(palette, key, &origin, &bytes),
        Some("mcstructure") => load_bedrock_template(palette, key, &origin, &bytes),
        _ => Err(Error::data(origin, "not a structure template")),
    }
}

/// The 1x1x1 shape of a `minecraft:feature_pool_element`: one connector at the
/// origin facing down (`minecraft:bottom`, top south, rollable), no cells.
pub fn feature_template(feature: &str) -> Template {
    Template {
        key: format!("feature:{feature}"),
        origin: format!("feature pool element {feature}"),
        size: [1, 1, 1],
        palette: Vec::new(),
        cells: Vec::new(),
        block_entities: Vec::new(),
        connectors: vec![Connector {
            pos: [0, 0, 0],
            front: Dir6::Down,
            top: Dir6::South,
            name: "minecraft:bottom".to_owned(),
            target: EMPTY.to_owned(),
            pool: EMPTY.to_owned(),
            rollable: true,
            selection_priority: 0,
            placement_priority: 0,
        }],
    }
}

// --- NBT helpers ------------------------------------------------------------

fn compound<'t>(origin: &str, tag: &'t Tag, what: &str) -> Result<&'t BTreeMap<String, Tag>> {
    tag.as_compound()
        .ok_or_else(|| Error::data(origin, format!("{what} is not a compound")))
}

fn child<'t>(origin: &str, map: &'t BTreeMap<String, Tag>, key: &str) -> Result<&'t Tag> {
    map.get(key)
        .ok_or_else(|| Error::data(origin, format!("missing NBT key {key:?}")))
}

fn list<'t>(origin: &str, tag: &'t Tag, what: &str) -> Result<&'t [Tag]> {
    tag.as_list()
        .ok_or_else(|| Error::data(origin, format!("{what} is not a list")))
}

fn text<'t>(origin: &str, tag: &'t Tag, what: &str) -> Result<&'t str> {
    tag.as_str()
        .ok_or_else(|| Error::data(origin, format!("{what} is not a string")))
}

fn integer(origin: &str, tag: &Tag, what: &str) -> Result<i32> {
    tag.as_i64()
        .and_then(|v| i32::try_from(v).ok())
        .ok_or_else(|| Error::data(origin, format!("{what} is not an i32")))
}

fn int3(origin: &str, tag: &Tag, what: &str) -> Result<[i32; 3]> {
    match tag {
        Tag::List(items) if items.len() == 3 => Ok([
            integer(origin, &items[0], what)?,
            integer(origin, &items[1], what)?,
            integer(origin, &items[2], what)?,
        ]),
        Tag::IntArray(items) if items.len() == 3 => Ok([items[0], items[1], items[2]]),
        _ => Err(Error::data(origin, format!("{what} is not three integers"))),
    }
}

/// Strict NBT: every key must be known.
fn check_keys(
    origin: &str,
    map: &BTreeMap<String, Tag>,
    allowed: &[&str],
    what: &str,
) -> Result<()> {
    match map.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(extra) => Err(Error::data(
            origin,
            format!("unknown {what} NBT key {extra:?}"),
        )),
        None => Ok(()),
    }
}

// --- Java templates ---------------------------------------------------------

fn load_java_template(
    palette: &BlockPalette,
    key: &str,
    origin: &str,
    bytes: &[u8],
) -> Result<Template> {
    let root = nbt::read_gzip_java(origin, bytes)?;
    let map = compound(origin, &root, "template")?;
    // `entities` (mobs, item frames) are out of scope; the rest is metadata.
    check_keys(
        origin,
        map,
        &[
            "DataVersion",
            "author",
            "version",
            "size",
            "palette",
            "blocks",
            "entities",
        ],
        "template",
    )?;
    let size = int3(origin, child(origin, map, "size")?, "size")?;
    let mut entries: Vec<(String, BTreeMap<String, String>)> = Vec::new();
    for e in list(origin, child(origin, map, "palette")?, "palette")? {
        let m = compound(origin, e, "palette entry")?;
        check_keys(origin, m, &["Name", "Properties"], "palette entry")?;
        let name = text(origin, child(origin, m, "Name")?, "Name")?.to_owned();
        let mut props = BTreeMap::new();
        if let Some(p) = m.get("Properties") {
            for (k, v) in compound(origin, p, "Properties")? {
                props.insert(k.clone(), text(origin, v, k)?.to_owned());
            }
        }
        entries.push((name, props));
    }
    let mut builder = TemplateBuilder::new(key, origin, size)?;
    // Every palette entry must convert, used or not; special blocks have no state.
    let mut converted: Vec<Option<TemplateState>> = Vec::with_capacity(entries.len());
    for (name, props) in &entries {
        converted.push(match name.as_str() {
            "minecraft:structure_void" | "minecraft:structure_block" | "minecraft:jigsaw" => None,
            _ => Some(java_template_state(palette, origin, name, props)?),
        });
    }
    for b in list(origin, child(origin, map, "blocks")?, "blocks")? {
        let m = compound(origin, b, "block")?;
        check_keys(origin, m, &["pos", "state", "nbt"], "block")?;
        let pos = int3(origin, child(origin, m, "pos")?, "pos")?;
        let state = integer(origin, child(origin, m, "state")?, "state")?;
        let (name, props) = usize::try_from(state)
            .ok()
            .and_then(|i| entries.get(i))
            .ok_or_else(|| {
                Error::data(origin, format!("block state {state} outside the palette"))
            })?;
        match name.as_str() {
            "minecraft:structure_void" | "minecraft:structure_block" => {}
            "minecraft:jigsaw" => {
                let data = compound(origin, child(origin, m, "nbt")?, "jigsaw nbt")?;
                let (connector, final_state) = jigsaw(origin, pos, props, data)?;
                let (fname, fprops) = parse_state_string(origin, &final_state)?;
                let ids = java_template_state(palette, origin, &fname, &fprops)?;
                let entry = builder.entry(ids)?;
                builder.cell(pos, entry)?;
                builder.template.connectors.push(connector);
            }
            _ => {
                let ids = converted[state as usize].ok_or_else(|| {
                    Error::data(origin, format!("palette entry {name} has no block state"))
                })?;
                let entry = builder.entry(ids)?;
                builder.cell(pos, entry)?;
                if let Some(actor) = m.get("nbt") {
                    builder.block_entity(actor, true)?;
                }
            }
        }
    }
    Ok(builder.template)
}

/// Decode a jigsaw block: its connector and `final_state` (default air).
fn jigsaw(
    origin: &str,
    pos: [i32; 3],
    props: &BTreeMap<String, String>,
    data: &BTreeMap<String, Tag>,
) -> Result<(Connector, String)> {
    check_keys(
        origin,
        data,
        &[
            "id",
            "name",
            "target",
            "pool",
            "joint",
            "final_state",
            "selection_priority",
            "placement_priority",
            "components",
        ],
        "jigsaw",
    )?;
    if let Some(c) = data.get("components")
        && !compound(origin, c, "components")?.is_empty()
    {
        return Err(Error::data(
            origin,
            format!("jigsaw at {pos:?} has data components"),
        ));
    }
    let orientation = props
        .get("orientation")
        .ok_or_else(|| Error::data(origin, format!("jigsaw at {pos:?} has no orientation")))?;
    let (front, top) = orientation
        .split_once('_')
        .and_then(|(f, t)| Some((Dir6::parse(f)?, Dir6::parse(t)?)))
        .ok_or_else(|| Error::data(origin, format!("jigsaw orientation {orientation:?}")))?;
    if let Some(extra) = props.keys().find(|k| *k != "orientation") {
        return Err(Error::data(origin, format!("jigsaw property {extra:?}")));
    }
    let string = |key: &str, default: &str| -> Result<String> {
        match data.get(key) {
            Some(t) => Ok(text(origin, t, key)?.to_owned()),
            None => Ok(default.to_owned()),
        }
    };
    let rollable = match data.get("joint") {
        Some(t) => match text(origin, t, "joint")? {
            "rollable" => true,
            "aligned" => false,
            other => return Err(Error::data(origin, format!("jigsaw joint {other:?}"))),
        },
        // Vanilla default: horizontal connectors are aligned, vertical ones rollable.
        None => matches!(front, Dir6::Up | Dir6::Down),
    };
    let priority =
        |key: &str| -> Result<i32> { data.get(key).map_or(Ok(0), |t| integer(origin, t, key)) };
    Ok((
        Connector {
            pos,
            front,
            top,
            name: string("name", EMPTY)?,
            target: string("target", EMPTY)?,
            pool: string("pool", EMPTY)?,
            rollable,
            selection_priority: priority("selection_priority")?,
            placement_priority: priority("placement_priority")?,
        },
        string("final_state", "minecraft:air")?,
    ))
}

/// Split `minecraft:x[a=b,c=d]` into name and properties.
pub fn parse_state_string(origin: &str, s: &str) -> Result<(String, BTreeMap<String, String>)> {
    let Some((name, rest)) = s.split_once('[') else {
        return Ok((s.to_owned(), BTreeMap::new()));
    };
    let body = rest
        .strip_suffix(']')
        .ok_or_else(|| Error::data(origin, format!("block state string {s:?}")))?;
    let mut props = BTreeMap::new();
    for pair in body.split(',').filter(|p| !p.is_empty()) {
        let (k, v) = pair
            .split_once('=')
            .ok_or_else(|| Error::data(origin, format!("block state string {s:?}")))?;
        props.insert(k.trim().to_owned(), v.trim().to_owned());
    }
    Ok((name.to_owned(), props))
}

/// Runtime ids of a Java state in the 4 clockwise rotations.
pub fn java_rotations(
    palette: &BlockPalette,
    origin: &str,
    name: &str,
    props: &BTreeMap<String, String>,
) -> Result<[u32; 4]> {
    let mut ids = [0u32; 4];
    for (r, id) in ids.iter_mut().enumerate() {
        let rotated = rotate_java_properties(props, r as u8);
        let desc = java_descriptor(palette, origin, name, &rotated)?;
        *id = palette.resolve(origin, &desc)?;
    }
    Ok(ids)
}

/// Preserve a Java template's fluid alongside its rotated primary permutation.
fn java_template_state(
    palette: &BlockPalette,
    origin: &str,
    name: &str,
    props: &BTreeMap<String, String>,
) -> Result<TemplateState> {
    let primary = java_rotations(palette, origin, name, props)?;
    let waterlogged = props
        .get("waterlogged")
        .map(|value| java_flag(origin, name, "waterlogged", value))
        .transpose()?
        .unwrap_or(false);
    let carries_water = unastar_packs::block_state::java_secondary_water(name, waterlogged);
    let mut secondary = BlockDescriptor::named(if carries_water {
        "minecraft:water"
    } else {
        "minecraft:air"
    });
    if carries_water {
        secondary
            .states
            .insert("liquid_depth".to_owned(), StateValue::Int(0));
    }
    Ok(TemplateState {
        primary,
        secondary: [palette.resolve(origin, &secondary)?; 4],
    })
}

/// Rotate Java block properties clockwise by `quarter_turns`: `facing`,
/// `axis` and the per-side `north/east/south/west` properties move; stair
/// `shape` and chest `type` are relative to `facing` and stay.
pub fn rotate_java_properties(
    props: &BTreeMap<String, String>,
    quarter_turns: u8,
) -> BTreeMap<String, String> {
    let r = quarter_turns % 4;
    let mut out = BTreeMap::new();
    for (key, value) in props {
        let (k, v) = match key.as_str() {
            "facing" => (
                key.clone(),
                Dir6::parse(value).map_or_else(|| value.clone(), |d| d.rotate(r).name().to_owned()),
            ),
            "axis" if r % 2 == 1 => (
                key.clone(),
                match value.as_str() {
                    "x" => "z".to_owned(),
                    "z" => "x".to_owned(),
                    other => other.to_owned(),
                },
            ),
            "north" | "east" | "south" | "west" => (
                Dir6::parse(key).map_or_else(|| key.clone(), |d| d.rotate(r).name().to_owned()),
                value.clone(),
            ),
            _ => (key.clone(), value.clone()),
        };
        out.insert(k, v);
    }
    out
}

/// Java block names whose Bedrock 1.26.51 identifier differs.
const JAVA_RENAMES: &[(&str, &str)] = &[
    ("bricks", "brick_block"),
    ("chain", "iron_chain"),
    ("cobweb", "web"),
    ("dead_bush", "deadbush"),
    ("dirt_path", "grass_path"),
    ("light_gray_glazed_terracotta", "silver_glazed_terracotta"),
    ("lily_pad", "waterlily"),
    ("melon", "melon_block"),
    // Java `snow` is the layer block and `snow_block` the full block.
    ("snow", "snow_layer"),
    ("snow_block", "snow"),
    ("sugar_cane", "reeds"),
    ("terracotta", "hardened_clay"),
    ("water_cauldron", "cauldron"),
];

fn java_flag(origin: &str, name: &str, key: &str, v: &str) -> Result<bool> {
    match v {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(Error::data(
            origin,
            format!("{name} {key}={v:?} is not a boolean"),
        )),
    }
}

fn java_int(origin: &str, name: &str, key: &str, v: &str) -> Result<i32> {
    v.parse()
        .map_err(|_| Error::data(origin, format!("{name} {key}={v:?} is not an integer")))
}

/// Java `none/low/tall` side heights to Bedrock `none/short/tall`.
fn side_height(origin: &str, name: &str, key: &str, v: &str) -> Result<StateValue> {
    match v {
        "none" => Ok(StateValue::Str("none".into())),
        "low" => Ok(StateValue::Str("short".into())),
        "tall" => Ok(StateValue::Str("tall".into())),
        _ => Err(Error::data(origin, format!("{name} {key}={v:?}"))),
    }
}

/// Bedrock `huge_mushroom_bits` of a Java mushroom block (`true` = cap face).
/// Bedrock encodes the top cap plus at most one north/south and one
/// east/west side (north and west win when both of a pair are set); stems
/// are 10 (sides) or 15 (all faces).
pub fn mushroom_bits(props: &BTreeMap<String, String>, stem: bool) -> i32 {
    let side = |k: &str| props.get(k).is_some_and(|v| v == "true");
    let (n, e, s, w, u, d) = (
        side("north"),
        side("east"),
        side("south"),
        side("west"),
        side("up"),
        side("down"),
    );
    if stem {
        return if u || d { 15 } else { 10 };
    }
    if n && e && s && w && u && d {
        return 14;
    }
    if !(n || e || s || w || u || d) {
        return 0;
    }
    let ns = if n {
        0
    } else if s {
        2
    } else {
        1
    };
    let ew = if w {
        0
    } else if e {
        2
    } else {
        1
    };
    // Rows north/none/south, columns west/none/east.
    [[1, 2, 3], [4, 5, 6], [7, 8, 9]][ns][ew]
}

/// Map a Java `(name, properties)` to a Bedrock 1.26.51 descriptor. Every
/// property is mapped or dropped with a documented reason (see the module
/// docs); anything else is an error naming `origin`.
pub fn java_descriptor(
    palette: &BlockPalette,
    origin: &str,
    name: &str,
    props: &BTreeMap<String, String>,
) -> Result<BlockDescriptor> {
    let fail =
        |reason: String| Error::data(origin, format!("palette entry {name}{props:?}: {reason}"));
    let base = name
        .strip_prefix("minecraft:")
        .ok_or_else(|| fail("not a minecraft block".into()))?;
    let prop = |key: &str| props.get(key).map(String::as_str);
    let renamed = JAVA_RENAMES
        .iter()
        .find(|(java, _)| *java == base)
        .map_or(base, |(_, bedrock)| *bedrock);
    let target = if let Some(stem) = base.strip_suffix("_slab")
        && prop("type") == Some("double")
    {
        format!("minecraft:{stem}_double_slab")
    } else if matches!(base, "furnace" | "blast_furnace" | "smoker") && prop("lit") == Some("true")
    {
        format!("minecraft:lit_{base}")
    } else if base == "water" && prop("level").is_some_and(|l| l != "0") {
        "minecraft:flowing_water".to_owned()
    } else {
        format!("minecraft:{renamed}")
    };
    let keys: Vec<String> = palette
        .states_of(&target)
        .and_then(|mut states| states.next())
        .map(|(_, s)| s.keys().cloned().collect())
        .ok_or_else(|| fail(format!("no Bedrock block {target}")))?;
    let has = |key: &str| keys.iter().any(|k| k == key);
    let int = StateValue::Int;
    let string = |v: &str| StateValue::Str(v.to_owned());
    let mut states: BTreeMap<String, StateValue> = BTreeMap::new();
    if base == "water_cauldron" {
        states.insert("cauldron_liquid".into(), string("water"));
    }
    let folded_faces = has("huge_mushroom_bits") || has("vine_direction_bits");
    for (key, value) in props {
        let v = value.as_str();
        let flag = || java_flag(origin, name, key, v);
        let number = || java_int(origin, name, key, v);
        let bad_value = || fail(format!("unmapped value {key}={v}"));
        let (state, val): (String, StateValue) = match key.as_str() {
            // A second (liquid) block layer cannot be stored in one block id.
            "waterlogged" => {
                flag()?;
                continue;
            }
            "axis" if has("pillar_axis") => ("pillar_axis".into(), string(v)),
            "facing" => {
                if has("weirdo_direction") {
                    let d = match v {
                        "east" => 0,
                        "west" => 1,
                        "south" => 2,
                        "north" => 3,
                        _ => return Err(bad_value()),
                    };
                    ("weirdo_direction".into(), int(d))
                } else if has("facing_direction") {
                    let d = match v {
                        "down" => 0,
                        "up" => 1,
                        "north" => 2,
                        "south" => 3,
                        "west" => 4,
                        "east" => 5,
                        _ => return Err(bad_value()),
                    };
                    ("facing_direction".into(), int(d))
                } else if has("minecraft:cardinal_direction") {
                    if !matches!(v, "north" | "south" | "west" | "east") {
                        return Err(bad_value());
                    }
                    ("minecraft:cardinal_direction".into(), string(v))
                } else if has("direction") {
                    // Legacy order: 0 south, 1 west, 2 north, 3 east.
                    let d = match v {
                        "south" => 0,
                        "west" => 1,
                        "north" => 2,
                        "east" => 3,
                        _ => return Err(bad_value()),
                    };
                    ("direction".into(), int(d))
                } else {
                    return Err(fail("unmapped property facing".into()));
                }
            }
            "half" if has("upside_down_bit") => match v {
                "top" => ("upside_down_bit".into(), int(1)),
                "bottom" => ("upside_down_bit".into(), int(0)),
                _ => return Err(bad_value()),
            },
            "half" if has("upper_block_bit") => match v {
                "upper" => ("upper_block_bit".into(), int(1)),
                "lower" => ("upper_block_bit".into(), int(0)),
                _ => return Err(bad_value()),
            },
            "shape" if has("minecraft:corner") => match v {
                "straight" => ("minecraft:corner".into(), string("none")),
                "inner_left" | "inner_right" | "outer_left" | "outer_right" => {
                    ("minecraft:corner".into(), string(v))
                }
                _ => return Err(bad_value()),
            },
            "type" if has("minecraft:vertical_half") => match v {
                "bottom" | "top" => ("minecraft:vertical_half".into(), string(v)),
                // Already folded into the `_double_slab` name.
                "double" => continue,
                _ => return Err(bad_value()),
            },
            // Bedrock pairs chests through their block entities.
            "type"
                if base == "chest" || base == "trapped_chest" || base.ends_with("copper_chest") =>
            {
                if !matches!(v, "single" | "left" | "right") {
                    return Err(bad_value());
                }
                continue;
            }
            "north" | "east" | "south" | "west" | "up" | "down" if folded_faces => {
                flag()?;
                continue;
            }
            "north" | "east" | "south" | "west" if has(&format!("minecraft:connection_{key}")) => (
                format!("minecraft:connection_{key}"),
                int(i32::from(flag()?)),
            ),
            "north" | "east" | "south" | "west" if has(&format!("wall_connection_type_{key}")) => (
                format!("wall_connection_type_{key}"),
                side_height(origin, name, key, v)?,
            ),
            "north" | "east" | "south" | "west" if has(&format!("pale_moss_carpet_side_{key}")) => {
                (
                    format!("pale_moss_carpet_side_{key}"),
                    side_height(origin, name, key, v)?,
                )
            }
            "up" if has("wall_post_bit") => ("wall_post_bit".into(), int(i32::from(flag()?))),
            "bottom" if has("upper_block_bit") => {
                ("upper_block_bit".into(), int(i32::from(!flag()?)))
            }
            // Bedrock leaves do not store their log distance.
            "distance" if has("persistent_bit") => {
                number()?;
                continue;
            }
            "persistent" if has("persistent_bit") => {
                ("persistent_bit".into(), int(i32::from(flag()?)))
            }
            // Bedrock derives the snowy side texture from the block above.
            "snowy" => {
                flag()?;
                continue;
            }
            "lit" if matches!(base, "furnace" | "blast_furnace" | "smoker") => {
                // Already folded into the `lit_` name.
                flag()?;
                continue;
            }
            "lit" if has("extinguished") => ("extinguished".into(), int(i32::from(!flag()?))),
            // Bedrock derives the signal smoke from the block below.
            "signal_fire" if base == "campfire" || base == "soul_campfire" => {
                flag()?;
                continue;
            }
            "open" if has("open_bit") => ("open_bit".into(), int(i32::from(flag()?))),
            "age" if has("age") => ("age".into(), int(number()?)),
            "age" if has("growth") => ("growth".into(), int(number()?)),
            "stage" if has("age_bit") => ("age_bit".into(), int(number()?)),
            "moisture" if has("moisturized_amount") => {
                ("moisturized_amount".into(), int(number()?))
            }
            "level" if has("composter_fill_level") => {
                ("composter_fill_level".into(), int(number()?))
            }
            "level" if has("liquid_depth") => ("liquid_depth".into(), int(number()?)),
            // Java cauldron levels 1..=3 are Bedrock fill levels 2, 4, 6.
            "level" if has("fill_level") => ("fill_level".into(), int(number()? * 2)),
            "layers" if has("height") => ("height".into(), int(number()? - 1)),
            "segment_amount" | "flower_amount" if has("growth") => {
                ("growth".into(), int(number()? - 1))
            }
            "hanging" if has("hanging") => ("hanging".into(), int(i32::from(flag()?))),
            "tip" if has("tip") => ("tip".into(), int(i32::from(flag()?))),
            "natural" if has("natural") => ("natural".into(), int(i32::from(flag()?))),
            "creaking_heart_state" if has("creaking_heart_state") => {
                ("creaking_heart_state".into(), string(v))
            }
            "face" if has("attachment") => match v {
                "floor" => ("attachment".into(), string("standing")),
                "ceiling" => ("attachment".into(), string("hanging")),
                "wall" => ("attachment".into(), string("side")),
                _ => return Err(bad_value()),
            },
            "part" if has("head_piece_bit") => match v {
                "head" => ("head_piece_bit".into(), int(1)),
                "foot" => ("head_piece_bit".into(), int(0)),
                _ => return Err(bad_value()),
            },
            "occupied" if has("occupied_bit") => ("occupied_bit".into(), int(i32::from(flag()?))),
            // The statue pose lives in the Bedrock block entity.
            "copper_golem_pose" if base.ends_with("copper_golem_statue") => continue,
            _ => return Err(fail(format!("unmapped property {key}={v}"))),
        };
        states.insert(state, val);
    }
    if has("huge_mushroom_bits") {
        states.insert(
            "huge_mushroom_bits".into(),
            int(mushroom_bits(props, base == "mushroom_stem")),
        );
    }
    if has("vine_direction_bits") {
        // Bits: 1 south, 2 west, 4 north, 8 east; Java `up` has no Bedrock state.
        let mut bits = 0;
        for (side, bit) in [("south", 1), ("west", 2), ("north", 4), ("east", 8)] {
            if props.get(side).is_some_and(|v| v == "true") {
                bits |= bit;
            }
        }
        states.insert("vine_direction_bits".into(), int(bits));
    }
    Ok(BlockDescriptor {
        name: target,
        states,
    })
}

// --- Bedrock templates ------------------------------------------------------

/// States whose meaning depends on orientation but which [`rotate_bedrock`]
/// does not rotate; a `.mcstructure` palette using one fails the build.
const UNROTATED_STATES: &[&str] = &[
    "vine_direction_bits",
    "huge_mushroom_bits",
    "rail_direction",
    "ground_sign_direction",
    "torch_facing_direction",
    "lever_direction",
    "minecraft:block_face",
    "minecraft:facing_direction",
    "coral_direction",
    "multi_face_direction_bits",
    "orientation",
    "rotation",
    "portal_axis",
    "wall_post_bit",
];

/// State name prefixes in the same situation as [`UNROTATED_STATES`].
const UNROTATED_PREFIXES: &[&str] = &[
    "minecraft:connection_",
    "wall_connection_type_",
    "pale_moss_carpet_side_",
];

/// Rotate a Bedrock descriptor clockwise by `quarter_turns`.
pub fn rotate_bedrock(
    origin: &str,
    desc: &BlockDescriptor,
    quarter_turns: u8,
) -> Result<BlockDescriptor> {
    let r = quarter_turns % 4;
    let mut out = BlockDescriptor::named(&desc.name);
    // Index cycles of the clockwise rotation in each encoding.
    const LEGACY: [i32; 4] = [0, 1, 2, 3]; // south, west, north, east: +1 per turn
    const FACING: [i32; 4] = [2, 5, 3, 4]; // north, east, south, west
    const WEIRDO: [i32; 4] = [3, 0, 2, 1]; // north, east, south, west
    let cycle = |order: &[i32; 4], v: i32| -> Option<i32> {
        let i = order.iter().position(|&o| o == v)?;
        Some(order[(i + r as usize) % 4])
    };
    for (key, value) in &desc.states {
        let bad = || {
            Error::data(
                origin,
                format!("{} {key}={value} cannot be rotated", desc.name),
            )
        };
        let rotated = match (key.as_str(), value) {
            ("minecraft:cardinal_direction", StateValue::Str(s)) => {
                let d = Dir6::parse(s).ok_or_else(bad)?;
                StateValue::Str(d.rotate(r).name().to_owned())
            }
            ("facing_direction", StateValue::Int(v)) if *v <= 1 => value.clone(),
            ("facing_direction", StateValue::Int(v)) => {
                StateValue::Int(cycle(&FACING, *v).ok_or_else(bad)?)
            }
            ("direction", StateValue::Int(v)) => {
                StateValue::Int(cycle(&LEGACY, *v).ok_or_else(bad)?)
            }
            ("weirdo_direction", StateValue::Int(v)) => {
                StateValue::Int(cycle(&WEIRDO, *v).ok_or_else(bad)?)
            }
            ("pillar_axis", StateValue::Str(s)) if r % 2 == 1 => StateValue::Str(
                match s.as_str() {
                    "x" => "z",
                    "z" => "x",
                    other => other,
                }
                .to_owned(),
            ),
            (k, _)
                if UNROTATED_STATES.contains(&k)
                    || UNROTATED_PREFIXES.iter().any(|p| k.starts_with(p)) =>
            {
                if r == 0 {
                    value.clone()
                } else {
                    return Err(bad());
                }
            }
            _ => value.clone(),
        };
        out.states.insert(key.clone(), rotated);
    }
    Ok(out)
}

fn load_bedrock_template(
    palette: &BlockPalette,
    key: &str,
    origin: &str,
    bytes: &[u8],
) -> Result<Template> {
    let mut reader = Reader::new(bytes, Encoding::LittleEndian, origin);
    let (_, root) = reader.read_root()?;
    let map = compound(origin, &root, "structure file")?;
    check_keys(
        origin,
        map,
        &[
            "format_version",
            "size",
            "structure",
            "structure_world_origin",
        ],
        "structure file",
    )?;
    let size = int3(origin, child(origin, map, "size")?, "size")?;
    let structure = compound(origin, child(origin, map, "structure")?, "structure")?;
    // `entities` are out of scope.
    check_keys(
        origin,
        structure,
        &["block_indices", "entities", "palette"],
        "structure",
    )?;
    let layers = list(
        origin,
        child(origin, structure, "block_indices")?,
        "block_indices",
    )?;
    if layers.len() != 2 {
        return Err(Error::data(
            origin,
            "block_indices must contain exactly two layers",
        ));
    }
    let primary = list(origin, &layers[0], "primary block layer")?;
    let secondary = list(origin, &layers[1], "secondary block layer")?;
    let palettes = compound(origin, child(origin, structure, "palette")?, "palette")?;
    check_keys(origin, palettes, &["default"], "palette")?;
    let default = compound(
        origin,
        child(origin, palettes, "default")?,
        "default palette",
    )?;
    check_keys(
        origin,
        default,
        &["block_palette", "block_position_data"],
        "default palette",
    )?;
    let mut builder = TemplateBuilder::new(key, origin, size)?;
    let mut entries: Vec<Option<[u32; 4]>> = Vec::new();
    for e in list(
        origin,
        child(origin, default, "block_palette")?,
        "block_palette",
    )? {
        let m = compound(origin, e, "block palette entry")?;
        check_keys(
            origin,
            m,
            &["name", "states", "version"],
            "block palette entry",
        )?;
        let name = text(origin, child(origin, m, "name")?, "name")?;
        if matches!(
            name,
            "minecraft:structure_void" | "minecraft:structure_block"
        ) {
            entries.push(None);
            continue;
        }
        let mut desc = BlockDescriptor::named(name);
        if let Some(states) = m.get("states") {
            for (k, v) in compound(origin, states, "states")? {
                let value = match v {
                    Tag::String(s) => StateValue::Str(s.clone()),
                    other => StateValue::Int(integer(origin, other, k)?),
                };
                desc.states.insert(k.clone(), value);
            }
        }
        let desc = blocks::upgrade(origin, desc)?;
        let mut ids = [0u32; 4];
        for (r, id) in ids.iter_mut().enumerate() {
            *id = palette.resolve(origin, &rotate_bedrock(origin, &desc, r as u8)?)?;
        }
        entries.push(Some(ids));
    }
    let [sx, sy, sz] = size;
    let volume = (sx * sy * sz) as usize;
    for (layer, indices) in [primary, secondary].into_iter().enumerate() {
        if indices.len() != volume {
            return Err(Error::data(
                origin,
                format!(
                    "layer {layer}: {} block indices for size {size:?}",
                    indices.len()
                ),
            ));
        }
    }
    let mut actors = BTreeMap::new();
    if let Some(positions) = default.get("block_position_data") {
        for (index, data) in compound(origin, positions, "block_position_data")? {
            let index: usize = index.parse().map_err(|_| {
                Error::data(origin, "block actor index is not a nonnegative integer")
            })?;
            if index >= volume {
                return Err(Error::data(
                    origin,
                    "block actor index outside the template volume",
                ));
            }
            let data = compound(origin, data, "block position data")?;
            check_keys(
                origin,
                data,
                &["block_entity_data", "tick_queue_data"],
                "block position data",
            )?;
            if let Some(actor) = data.get("block_entity_data") {
                // Structure-block markers are intentionally omitted with their blocks.
                if !matches!(
                    actor.get("id").and_then(Tag::as_str),
                    Some("StructureBlock" | "minecraft:structure_block")
                ) {
                    actors.insert(index, actor);
                }
            }
        }
    }
    for (i, (primary, secondary)) in primary.iter().zip(secondary).enumerate() {
        let state = TemplateState {
            primary: bedrock_template_layer(origin, primary, &entries)?,
            secondary: bedrock_template_layer(origin, secondary, &entries)?,
        };
        if state.primary == [KEEP_LAYER; 4] && state.secondary == [KEEP_LAYER; 4] {
            continue;
        }
        let entry = builder.entry(state)?;
        // Index order: x outermost, then y, then z.
        let i = i as i32;
        let pos = [i / (sy * sz), (i / sz) % sy, i % sz];
        builder.cell(pos, entry)?;
        if state.primary != [KEEP_LAYER; 4]
            && let Some(actor) = actors.remove(&(i as usize))
        {
            builder.block_entity(actor, false)?;
        }
    }
    if !actors.is_empty() {
        return Err(Error::data(
            origin,
            "block actor references a void template cell",
        ));
    }
    Ok(builder.template)
}

fn bedrock_template_layer(
    origin: &str,
    index: &Tag,
    entries: &[Option<[u32; 4]>],
) -> Result<[u32; 4]> {
    let index = integer(origin, index, "block index")?;
    if index == -1 {
        return Ok([KEEP_LAYER; 4]);
    }
    let entry = usize::try_from(index)
        .ok()
        .and_then(|index| entries.get(index))
        .ok_or_else(|| Error::data(origin, format!("block index {index} outside the palette")))?;
    Ok(entry.unwrap_or([KEEP_LAYER; 4]))
}

// ---------------------------------------------------------------------------
// Pools, processors, structures, sets
// ---------------------------------------------------------------------------

/// What a pool element places.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementKind {
    /// `minecraft:single_pool_element`: a template with optional processors.
    Template {
        template: usize,
        processors: Option<usize>,
    },
    /// `minecraft:feature_pool_element`: `template` is the element's 1x1x1
    /// shape ([`feature_template`]), `feature` indexes
    /// [`StructureData::pool_features`].
    Feature { template: usize, feature: usize },
    /// `minecraft:empty_pool_element`: ends the attempt for a connector.
    Empty,
}

/// A weighted pool element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub weight: u32,
    pub kind: ElementKind,
}

/// A `minecraft:template_pool`.
#[derive(Debug, Clone)]
pub struct Pool {
    pub identifier: String,
    pub source: String,
    pub elements: Vec<Element>,
    pub fallback: Option<usize>,
}

/// One rule of a `minecraft:rule` processor.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessorRule {
    /// Inclusive runtime-id range the template block must be in.
    pub input: (u32, u32),
    /// `random_block_match` probability (1 for `block_match` / `always_true`).
    pub probability: f32,
    pub output: u32,
}

/// One processor of a list.
#[derive(Debug, Clone, PartialEq)]
pub enum Processor {
    /// `minecraft:rule`: the first matching rule replaces the block.
    Rules(Vec<ProcessorRule>),
    /// `minecraft:capped`: the delegate rules change at most `limit` blocks
    /// of a piece, chosen at random.
    Capped {
        limit: u32,
        rules: Vec<ProcessorRule>,
    },
}

/// A `minecraft:processor_list`.
#[derive(Debug, Clone)]
pub struct ProcessorList {
    pub identifier: String,
    pub source: String,
    pub processors: Vec<Processor>,
}

/// `terrain_adaptation` of a jigsaw structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adaptation {
    None,
    Bury,
    BeardThin,
    BeardBox,
    Encapsulate,
}

/// A vertical anchor of `start_height`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Absolute(i32),
    AboveBottom(i32),
    BelowTop(i32),
}

/// A `minecraft:jigsaw` structure.
#[derive(Debug, Clone)]
pub struct JigsawStructure {
    pub identifier: String,
    pub source: String,
    pub biome_filter: BiomeFilter,
    /// Index in [`STEPS`].
    pub step: u8,
    pub adaptation: Adaptation,
    pub start_pool: usize,
    pub max_depth: u32,
    pub horizontal: i32,
    pub vertical: i32,
    pub start_height: Anchor,
    /// `heightmap_projection: world_surface`: the start height is relative to
    /// the surface at the start piece's centre.
    pub project_to_surface: bool,
    pub padding_bottom: i32,
    pub padding_top: i32,
}

/// A `minecraft:structure_set` with `minecraft:random_spread` placement.
#[derive(Debug, Clone)]
pub struct StructureSet {
    pub identifier: String,
    pub source: String,
    /// `(structure index, weight)`.
    pub structures: Vec<(usize, u32)>,
    pub spacing: i32,
    pub separation: i32,
    pub salt: i32,
    pub triangular: bool,
}

/// Every structure family plus the templates they and the template-based
/// features reference.
#[derive(Debug, Clone, Default)]
pub struct StructureData {
    pub templates: Vec<Template>,
    pub template_index: BTreeMap<String, usize>,
    pub pools: Vec<Pool>,
    pub pool_index: BTreeMap<String, usize>,
    pub processors: Vec<ProcessorList>,
    pub processor_index: BTreeMap<String, usize>,
    pub structures: Vec<JigsawStructure>,
    pub structure_index: BTreeMap<String, usize>,
    pub sets: Vec<StructureSet>,
    /// Templates of `structures/fossils/*`, sorted by location.
    pub fossils: Vec<usize>,
    /// `structure_name` of every `structure_template_feature` -> template.
    pub named_templates: BTreeMap<String, usize>,
    /// Features placed by `minecraft:feature_pool_element`s, in dispatcher order.
    pub pool_features: Vec<String>,
    /// Files read, for `rerun-if-changed`.
    pub inputs: Vec<PathBuf>,
}

impl StructureData {
    /// Resolve the four structure families, load every template they and the
    /// `structure_template_feature` / `fossil_feature` documents in `features`
    /// reference, and convert their palettes.
    pub fn load(
        stack: &PackStack,
        palette: &BlockPalette,
        features: &BTreeMap<String, Feature>,
    ) -> Result<Self> {
        let mut data = StructureData::default();
        let files = template_files(stack)?;
        for family in [
            Family::JigsawStructures,
            Family::StructureSets,
            Family::TemplatePools,
            Family::Processors,
        ] {
            data.inputs.extend(stack.inputs(family));
        }

        let processor_docs = stack.resolve(Family::Processors)?;
        for doc in processor_docs.values() {
            let list = parse_processor_list(palette, doc)?;
            data.processor_index
                .insert(list.identifier.clone(), data.processors.len());
            data.processors.push(list);
        }

        let pool_docs = stack.resolve(Family::TemplatePools)?;
        for (i, id) in pool_docs.keys().enumerate() {
            data.pool_index.insert(id.clone(), i);
        }
        for doc in pool_docs.values() {
            let pool = data.parse_pool(palette, &files, doc)?;
            data.pools.push(pool);
        }

        let structure_docs = stack.resolve(Family::JigsawStructures)?;
        for doc in structure_docs.values() {
            let s = data.parse_structure(doc)?;
            data.structure_index
                .insert(s.identifier.clone(), data.structures.len());
            data.structures.push(s);
        }

        for doc in stack.resolve(Family::StructureSets)?.values() {
            let set = data.parse_set(doc)?;
            data.sets.push(set);
        }

        for f in features.values() {
            if f.kind != "minecraft:structure_template_feature" {
                continue;
            }
            let name = f
                .body
                .get("structure_name")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::data(&f.source, "structure_name is not a string"))?;
            let location = template_location(name, &files).ok_or_else(|| {
                Error::data(
                    &f.source,
                    format!("no structure template file for {name:?}"),
                )
            })?;
            let index = data.template(palette, &files, &location, &f.source)?;
            data.named_templates.insert(name.to_owned(), index);
        }

        if features
            .values()
            .any(|f| f.kind == "minecraft:fossil_feature")
        {
            let fossils: Vec<String> = files
                .keys()
                .filter(|k| k.starts_with("fossils/"))
                .cloned()
                .collect();
            if fossils.is_empty() {
                return Err(Error::data(
                    "structures/fossils",
                    "fossil_feature needs fossil templates",
                ));
            }
            for key in fossils {
                let index = data.template(palette, &files, &key, "structures/fossils")?;
                data.fossils.push(index);
            }
        }

        // Every connector must name an existing pool (or the empty pool).
        for t in &data.templates {
            for c in &t.connectors {
                if c.pool != EMPTY && !data.pool_index.contains_key(&c.pool) {
                    return Err(Error::data(
                        &t.origin,
                        format!("jigsaw names unknown pool {}", c.pool),
                    ));
                }
            }
        }
        Ok(data)
    }

    /// Load (once) the template at `location`.
    fn template(
        &mut self,
        palette: &BlockPalette,
        files: &BTreeMap<String, PathBuf>,
        location: &str,
        referrer: &str,
    ) -> Result<usize> {
        if let Some(&i) = self.template_index.get(location) {
            return Ok(i);
        }
        let path = files
            .get(location)
            .ok_or_else(|| Error::data(referrer, format!("no structure template {location}")))?;
        let template = load_template(palette, location, path)?;
        self.inputs.push(path.clone());
        let i = self.templates.len();
        self.templates.push(template);
        self.template_index.insert(location.to_owned(), i);
        Ok(i)
    }

    /// The synthetic 1x1x1 template and dispatcher index of a pool feature.
    fn feature_element(&mut self, feature: &str) -> (usize, usize) {
        let key = format!("feature:{feature}");
        let template = match self.template_index.get(&key) {
            Some(&i) => i,
            None => {
                let i = self.templates.len();
                self.templates.push(feature_template(feature));
                self.template_index.insert(key, i);
                i
            }
        };
        let index = match self.pool_features.iter().position(|f| f == feature) {
            Some(i) => i,
            None => {
                self.pool_features.push(feature.to_owned());
                self.pool_features.len() - 1
            }
        };
        (template, index)
    }

    fn pool_ref(&self, origin: &str, id: &str) -> Result<Option<usize>> {
        if id == EMPTY {
            return Ok(None);
        }
        self.pool_index
            .get(id)
            .copied()
            .map(Some)
            .ok_or_else(|| Error::data(origin, format!("unknown template pool {id}")))
    }

    fn parse_pool(
        &mut self,
        palette: &BlockPalette,
        files: &BTreeMap<String, PathBuf>,
        doc: &Document,
    ) -> Result<Pool> {
        let origin = doc.source.as_str();
        let o = Obj::new(origin, &doc.body)?;
        o.ignore(&["description"]);
        let fallback = match o.opt("fallback") {
            Some(v) => {
                let id = v
                    .as_str()
                    .ok_or_else(|| Error::data(origin, "fallback is not a string"))?;
                self.pool_ref(origin, id)?
            }
            None => None,
        };
        let mut elements = Vec::new();
        for item in o.array("elements")? {
            let e = Obj::new(origin, item)?;
            let weight = e.i32("weight")?;
            if !(1..=150).contains(&weight) {
                return Err(Error::data(
                    origin,
                    format!("pool element weight {weight} outside 1..=150"),
                ));
            }
            let kind = self.parse_element(palette, files, origin, e.req("element")?)?;
            e.finish()?;
            elements.push(Element {
                weight: weight as u32,
                kind,
            });
        }
        o.finish()?;
        Ok(Pool {
            identifier: doc.identifier.clone(),
            source: doc.source.clone(),
            elements,
            fallback,
        })
    }

    fn parse_element(
        &mut self,
        palette: &BlockPalette,
        files: &BTreeMap<String, PathBuf>,
        origin: &str,
        v: &Value,
    ) -> Result<ElementKind> {
        let o = Obj::new(origin, v)?;
        let rigid = |o: &Obj<'_>| -> Result<()> {
            match o.opt("projection").map(|p| p.as_str()) {
                None | Some(Some("rigid")) => Ok(()),
                Some(other) => Err(Error::data(
                    origin,
                    format!("unsupported projection {other:?}"),
                )),
            }
        };
        let kind = match o.str("element_type")? {
            "minecraft:single_pool_element" => {
                let location = o.str("location")?;
                let location = location.strip_prefix("minecraft:").unwrap_or(location);
                let template = self.template(palette, files, location, origin)?;
                let processors = match o.opt("processors") {
                    None => None,
                    Some(Value::String(id)) if id == EMPTY => None,
                    Some(Value::String(id)) => {
                        Some(*self.processor_index.get(id).ok_or_else(|| {
                            Error::data(origin, format!("unknown processor list {id}"))
                        })?)
                    }
                    Some(other) => {
                        return Err(Error::data(
                            origin,
                            format!("processors must name a list: {other}"),
                        ));
                    }
                };
                rigid(&o)?;
                ElementKind::Template {
                    template,
                    processors,
                }
            }
            "minecraft:feature_pool_element" => {
                let feature = o.str("feature")?;
                rigid(&o)?;
                let (template, feature) = self.feature_element(feature);
                ElementKind::Feature { template, feature }
            }
            "minecraft:empty_pool_element" => ElementKind::Empty,
            other => {
                return Err(Error::data(
                    origin,
                    format!("unsupported pool element type {other}"),
                ));
            }
        };
        o.finish()?;
        Ok(kind)
    }

    fn parse_structure(&self, doc: &Document) -> Result<JigsawStructure> {
        let origin = doc.source.as_str();
        if doc.kind != "minecraft:jigsaw" {
            return Err(Error::data(
                origin,
                format!("unsupported structure type {}", doc.kind),
            ));
        }
        let o = Obj::new(origin, &doc.body)?;
        o.ignore(&["description"]);
        let biome_filter = BiomeFilter::parse(origin, o.req("biome_filters")?)?;
        let step_name = o.str("step")?;
        let step = STEPS
            .iter()
            .position(|s| *s == step_name)
            .ok_or_else(|| Error::data(origin, format!("unknown step {step_name:?}")))?
            as u8;
        let adaptation = match o.opt("terrain_adaptation").map(|v| v.as_str()) {
            None | Some(Some("none")) => Adaptation::None,
            Some(Some("bury")) => Adaptation::Bury,
            Some(Some("beard_thin")) => Adaptation::BeardThin,
            Some(Some("beard_box")) => Adaptation::BeardBox,
            Some(Some("encapsulate")) => Adaptation::Encapsulate,
            Some(other) => {
                return Err(Error::data(
                    origin,
                    format!("unknown terrain_adaptation {other:?}"),
                ));
            }
        };
        let start_pool = self
            .pool_ref(origin, o.str("start_pool")?)?
            .ok_or_else(|| Error::data(origin, "start_pool is the empty pool"))?;
        let max_depth = o.i32("max_depth")?;
        if !(0..=20).contains(&max_depth) {
            return Err(Error::data(
                origin,
                format!("max_depth {max_depth} outside 0..=20"),
            ));
        }
        let distance = o.req("max_distance_from_center")?;
        let (horizontal, vertical) = if distance.is_number() {
            let d = ir::int(origin, "max_distance_from_center", distance)?;
            (d, d)
        } else {
            let d = Obj::new(origin, distance)?;
            let pair = (d.i32("horizontal")?, d.i32("vertical")?);
            d.finish()?;
            pair
        };
        if !(1..=128).contains(&horizontal) || !(1..=4096).contains(&vertical) {
            return Err(Error::data(
                origin,
                format!("max_distance_from_center ({horizontal}, {vertical}) out of range"),
            ));
        }
        let start_height = height_provider(origin, o.req("start_height")?)?;
        let project_to_surface = match o.opt("heightmap_projection").map(|v| v.as_str()) {
            None | Some(Some("none")) => false,
            Some(Some("world_surface")) => true,
            Some(other) => {
                return Err(Error::data(
                    origin,
                    format!("unsupported heightmap_projection {other:?}"),
                ));
            }
        };
        let (padding_bottom, padding_top) = match o.opt("dimension_padding") {
            None => (0, 0),
            Some(v) if v.is_number() => {
                let p = ir::int(origin, "dimension_padding", v)?;
                (p, p)
            }
            Some(v) => {
                let p = Obj::new(origin, v)?;
                let pair = (p.i32_or("bottom", 0)?, p.i32_or("top", 0)?);
                p.finish()?;
                pair
            }
        };
        o.finish()?;
        Ok(JigsawStructure {
            identifier: doc.identifier.clone(),
            source: doc.source.clone(),
            biome_filter,
            step,
            adaptation,
            start_pool,
            max_depth: max_depth as u32,
            horizontal,
            vertical,
            start_height,
            project_to_surface,
            padding_bottom,
            padding_top,
        })
    }

    fn parse_set(&self, doc: &Document) -> Result<StructureSet> {
        let origin = doc.source.as_str();
        let o = Obj::new(origin, &doc.body)?;
        o.ignore(&["description"]);
        let p = Obj::new(origin, o.req("placement")?)?;
        let kind = p.str("type")?;
        if kind != "minecraft:random_spread" {
            return Err(Error::data(
                origin,
                format!("unsupported structure placement {kind}"),
            ));
        }
        let salt = p.i32("salt")?;
        let spacing = p.i32("spacing")?;
        let separation = p.i32("separation")?;
        if !(0..spacing).contains(&separation) || spacing > 4096 {
            return Err(Error::data(
                origin,
                format!("spacing {spacing} / separation {separation}"),
            ));
        }
        let triangular = match p.opt("spread_type").map(|v| v.as_str()) {
            None | Some(Some("linear")) => false,
            Some(Some("triangular")) => true,
            Some(other) => {
                return Err(Error::data(
                    origin,
                    format!("unknown spread_type {other:?}"),
                ));
            }
        };
        p.finish()?;
        let mut structures = Vec::new();
        for item in o.array("structures")? {
            let s = Obj::new(origin, item)?;
            let id = s.str("structure")?;
            let index = *self
                .structure_index
                .get(id)
                .ok_or_else(|| Error::data(origin, format!("unknown structure {id}")))?;
            let weight = s.i32("weight")?;
            if weight < 1 {
                return Err(Error::data(origin, format!("structure weight {weight}")));
            }
            s.finish()?;
            structures.push((index, weight as u32));
        }
        if structures.is_empty() {
            return Err(Error::data(origin, "structure set without structures"));
        }
        o.finish()?;
        Ok(StructureSet {
            identifier: doc.identifier.clone(),
            source: doc.source.clone(),
            structures,
            spacing,
            separation,
            salt,
            triangular,
        })
    }

    /// Template of a `structure_template_feature` `structure_name`.
    pub fn named_template(&self, name: &str) -> Option<usize> {
        self.named_templates.get(name).copied()
    }

    /// Largest horizontal extent of any template a structure can place.
    pub fn max_template_extent(&self) -> i32 {
        self.templates
            .iter()
            .map(|t| t.size[0].max(t.size[2]))
            .max()
            .unwrap_or(1)
    }
}

/// Template location of a `structure_template_feature` name: Bedrock stores
/// `namespace:path` as `structures/namespace/path`; `minecraft:` and
/// `mystructure:` names may also live directly in `structures/`.
pub fn template_location(name: &str, files: &BTreeMap<String, PathBuf>) -> Option<String> {
    let (namespace, path) = name.split_once(':').unwrap_or(("mystructure", name));
    let nested = format!("{namespace}/{path}");
    if files.contains_key(&nested) {
        return Some(nested);
    }
    (matches!(namespace, "minecraft" | "mystructure") && files.contains_key(path))
        .then(|| path.to_owned())
}

/// `start_height`: `{"type": "constant", "value": <anchor>}` or a bare anchor.
fn height_provider(origin: &str, v: &Value) -> Result<Anchor> {
    let o = Obj::new(origin, v)?;
    let anchor = match o.opt("type") {
        Some(t) => {
            if t.as_str() != Some("constant") && t.as_str() != Some("minecraft:constant") {
                return Err(Error::data(
                    origin,
                    format!("unsupported height provider {t}"),
                ));
            }
            vertical_anchor(origin, o.req("value")?)?
        }
        None => {
            let a = anchor_keys(origin, &o)?;
            o.finish()?;
            return Ok(a);
        }
    };
    o.finish()?;
    Ok(anchor)
}

fn vertical_anchor(origin: &str, v: &Value) -> Result<Anchor> {
    let o = Obj::new(origin, v)?;
    let a = anchor_keys(origin, &o)?;
    o.finish()?;
    Ok(a)
}

fn anchor_keys(origin: &str, o: &Obj<'_>) -> Result<Anchor> {
    match (o.opt("absolute"), o.opt("above_bottom"), o.opt("below_top")) {
        (Some(v), None, None) => Ok(Anchor::Absolute(ir::int(origin, "absolute", v)?)),
        (None, Some(v), None) => Ok(Anchor::AboveBottom(ir::int(origin, "above_bottom", v)?)),
        (None, None, Some(v)) => Ok(Anchor::BelowTop(ir::int(origin, "below_top", v)?)),
        _ => Err(Error::data(
            origin,
            "vertical anchor needs exactly one of absolute/above_bottom/below_top",
        )),
    }
}

fn parse_processor_list(palette: &BlockPalette, doc: &Document) -> Result<ProcessorList> {
    let origin = doc.source.as_str();
    if doc.kind != "minecraft:processor_list" {
        return Err(Error::data(
            origin,
            format!("unsupported processor document {}", doc.kind),
        ));
    }
    let o = Obj::new(origin, &doc.body)?;
    o.ignore(&["description"]);
    let processors = o
        .array("processors")?
        .iter()
        .map(|p| parse_processor(palette, origin, p))
        .collect::<Result<Vec<_>>>()?;
    o.finish()?;
    let capped = processors
        .iter()
        .filter(|p| matches!(p, Processor::Capped { .. }))
        .count();
    if capped > MAX_CAPPED_PER_LIST {
        return Err(Error::data(
            origin,
            format!("{capped} capped processors (max {MAX_CAPPED_PER_LIST})"),
        ));
    }
    Ok(ProcessorList {
        identifier: doc.identifier.clone(),
        source: doc.source.clone(),
        processors,
    })
}

fn parse_processor(palette: &BlockPalette, origin: &str, v: &Value) -> Result<Processor> {
    let o = Obj::new(origin, v)?;
    let p = match o.str("processor_type")? {
        "minecraft:rule" => Processor::Rules(
            o.array("rules")?
                .iter()
                .map(|r| parse_rule(palette, origin, r))
                .collect::<Result<_>>()?,
        ),
        "minecraft:capped" => {
            let limit = o.i32("limit")?;
            if !(1..=MAX_CAPPED_LIMIT as i32).contains(&limit) {
                return Err(Error::data(
                    origin,
                    format!("capped limit {limit} outside 1..={MAX_CAPPED_LIMIT}"),
                ));
            }
            let Processor::Rules(rules) = parse_processor(palette, origin, o.req("delegate")?)?
            else {
                return Err(Error::data(
                    origin,
                    "capped delegate must be a rule processor",
                ));
            };
            Processor::Capped {
                limit: limit as u32,
                rules,
            }
        }
        other => {
            return Err(Error::data(
                origin,
                format!("unsupported processor type {other}"),
            ));
        }
    };
    o.finish()?;
    Ok(p)
}

fn always_true(origin: &str, v: &Value) -> Result<()> {
    let o = Obj::new(origin, v)?;
    let t = o.str("predicate_type")?;
    if t != "minecraft:always_true" {
        return Err(Error::data(origin, format!("unsupported predicate {t}")));
    }
    o.finish()
}

fn parse_rule(palette: &BlockPalette, origin: &str, v: &Value) -> Result<ProcessorRule> {
    let o = Obj::new(origin, v)?;
    let input = Obj::new(origin, o.req("input_predicate")?)?;
    let block_range = |input: &Obj<'_>| -> Result<(u32, u32)> {
        let set = ir::block_set(palette, origin, input.req("block")?)?;
        match set.ranges() {
            [r] => Ok((*r.start(), *r.end())),
            _ => Err(Error::data(
                origin,
                "processor input block must be one contiguous state range",
            )),
        }
    };
    let (range, probability) = match input.str("predicate_type")? {
        "minecraft:block_match" => (block_range(&input)?, 1.0),
        "minecraft:random_block_match" => (block_range(&input)?, input.f32("probability")?),
        "minecraft:always_true" => ((0, u32::MAX - 1), 1.0),
        other => {
            return Err(Error::data(
                origin,
                format!("unsupported input predicate {other}"),
            ));
        }
    };
    input.finish()?;
    if let Some(p) = o.opt("location_predicate") {
        always_true(origin, p)?;
    }
    if let Some(p) = o.opt("position_predicate") {
        always_true(origin, p)?;
    }
    let output = ir::block(palette, origin, o.req("output_state")?)?;
    // Loot tables of the replaced block entity are out of scope.
    o.ignore(&["block_entity_modifier"]);
    o.finish()?;
    Ok(ProcessorRule {
        input: range,
        probability,
        output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// Every combination of the given state values, in canonical order.
    fn block(
        name: &str,
        states: &[(&str, Vec<StateValue>)],
    ) -> Vec<(String, BTreeMap<String, StateValue>)> {
        let mut out: Vec<BTreeMap<String, StateValue>> = vec![BTreeMap::new()];
        for (key, values) in states {
            let mut next = Vec::new();
            for m in &out {
                for v in values {
                    let mut m = m.clone();
                    m.insert(key.to_string(), v.clone());
                    next.push(m);
                }
            }
            out = next;
        }
        out.into_iter().map(|m| (name.to_owned(), m)).collect()
    }

    fn ints(n: i32) -> Vec<StateValue> {
        (0..n).map(StateValue::Int).collect()
    }

    fn strs(values: &[&str]) -> Vec<StateValue> {
        values
            .iter()
            .map(|v| StateValue::Str(v.to_string()))
            .collect()
    }

    fn palette() -> BlockPalette {
        let mut states = Vec::new();
        states.extend(block("minecraft:air", &[]));
        states.extend(block("minecraft:water", &[("liquid_depth", ints(16))]));
        states.extend(block(
            "minecraft:oak_stairs",
            &[
                (
                    "minecraft:corner",
                    strs(&[
                        "none",
                        "inner_left",
                        "inner_right",
                        "outer_left",
                        "outer_right",
                    ]),
                ),
                ("upside_down_bit", ints(2)),
                ("weirdo_direction", ints(4)),
            ],
        ));
        states.extend(block(
            "minecraft:oak_log",
            &[("pillar_axis", strs(&["y", "x", "z"]))],
        ));
        states.extend(block(
            "minecraft:brick_slab",
            &[("minecraft:vertical_half", strs(&["bottom", "top"]))],
        ));
        states.extend(block(
            "minecraft:brick_double_slab",
            &[("minecraft:vertical_half", strs(&["bottom", "top"]))],
        ));
        states.extend(block("minecraft:oak_planks", &[]));
        states.extend(block(
            "minecraft:oak_fence",
            &[
                ("minecraft:connection_east", ints(2)),
                ("minecraft:connection_north", ints(2)),
                ("minecraft:connection_south", ints(2)),
                ("minecraft:connection_west", ints(2)),
            ],
        ));
        states.extend(block(
            "minecraft:oak_leaves",
            &[("persistent_bit", ints(2)), ("update_bit", ints(2))],
        ));
        states.extend(block("minecraft:brick_block", &[]));
        states.extend(block(
            "minecraft:jigsaw",
            &[("facing_direction", ints(6)), ("rotation", ints(4))],
        ));
        BlockPalette::from_parts(states, BTreeMap::new()).unwrap()
    }

    fn convert(p: &BlockPalette, name: &str, pairs: &[(&str, &str)]) -> Result<u32> {
        p.resolve("t", &java_descriptor(p, "t", name, &props(pairs))?)
    }

    fn expect(p: &BlockPalette, name: &str, states: &[(&str, StateValue)]) -> u32 {
        let mut d = BlockDescriptor::named(name);
        for (k, v) in states {
            d.states.insert(k.to_string(), v.clone());
        }
        p.resolve("t", &d).unwrap()
    }

    #[test]
    fn collapsed_java_primaries_keep_distinct_wet_and_dry_palette_actions() {
        let p = palette();
        let mut properties = props(&[
            ("facing", "north"),
            ("half", "bottom"),
            ("shape", "straight"),
            ("waterlogged", "false"),
        ]);
        let dry = java_template_state(&p, "authored", "minecraft:oak_stairs", &properties).unwrap();
        properties.insert("waterlogged".to_owned(), "true".to_owned());
        let wet = java_template_state(&p, "authored", "minecraft:oak_stairs", &properties).unwrap();
        assert_eq!(dry.primary, wet.primary);
        let mut builder = TemplateBuilder::new("authored", "authored", [2, 1, 1]).unwrap();
        let dry_entry = builder.entry(dry).unwrap();
        let wet_entry = builder.entry(wet).unwrap();
        assert_eq!(
            builder.template.palette[dry_entry as usize].secondary,
            [expect(&p, "minecraft:air", &[]); 4],
        );
        assert_eq!(
            builder.template.palette[wet_entry as usize].secondary,
            [expect(
                &p,
                "minecraft:water",
                &[("liquid_depth", StateValue::Int(0))]
            ); 4],
        );
    }

    #[test]
    fn stairs_map_facing_half_and_shape() {
        let p = palette();
        let got = convert(
            &p,
            "minecraft:oak_stairs",
            &[
                ("facing", "south"),
                ("half", "top"),
                ("shape", "outer_left"),
                ("waterlogged", "false"),
            ],
        )
        .unwrap();
        let want = expect(
            &p,
            "minecraft:oak_stairs",
            &[
                ("weirdo_direction", StateValue::Int(2)),
                ("upside_down_bit", StateValue::Int(1)),
                ("minecraft:corner", StateValue::Str("outer_left".into())),
            ],
        );
        assert_eq!(got, want);
        let east = convert(
            &p,
            "minecraft:oak_stairs",
            &[
                ("facing", "east"),
                ("half", "bottom"),
                ("shape", "straight"),
            ],
        )
        .unwrap();
        assert_eq!(east, expect(&p, "minecraft:oak_stairs", &[]));
    }

    #[test]
    fn logs_slabs_planks_and_renames() {
        let p = palette();
        let log = convert(&p, "minecraft:oak_log", &[("axis", "x")]).unwrap();
        assert_eq!(
            log,
            expect(
                &p,
                "minecraft:oak_log",
                &[("pillar_axis", StateValue::Str("x".into()))]
            )
        );
        let top = convert(
            &p,
            "minecraft:brick_slab",
            &[("type", "top"), ("waterlogged", "false")],
        )
        .unwrap();
        assert_eq!(
            top,
            expect(
                &p,
                "minecraft:brick_slab",
                &[("minecraft:vertical_half", StateValue::Str("top".into()))]
            )
        );
        let double = convert(
            &p,
            "minecraft:brick_slab",
            &[("type", "double"), ("waterlogged", "false")],
        )
        .unwrap();
        assert_eq!(double, expect(&p, "minecraft:brick_double_slab", &[]));
        assert_eq!(
            convert(&p, "minecraft:oak_planks", &[]).unwrap(),
            expect(&p, "minecraft:oak_planks", &[])
        );
        assert_eq!(
            convert(&p, "minecraft:bricks", &[]).unwrap(),
            expect(&p, "minecraft:brick_block", &[])
        );
        let leaves = convert(
            &p,
            "minecraft:oak_leaves",
            &[
                ("distance", "3"),
                ("persistent", "true"),
                ("waterlogged", "false"),
            ],
        )
        .unwrap();
        assert_eq!(
            leaves,
            expect(
                &p,
                "minecraft:oak_leaves",
                &[("persistent_bit", StateValue::Int(1))]
            )
        );
    }

    #[test]
    fn legacy_bastion_chain_uses_iron_chain_and_preserves_axis() {
        // Shipped bastion stages still name `chain`; native 26.51 calls it
        // `iron_chain`, with default pillar_axis=y and horizontal x/z states.
        let p = BlockPalette::from_parts(
            block(
                "minecraft:iron_chain",
                &[("pillar_axis", strs(&["y", "x", "z"]))],
            ),
            BTreeMap::new(),
        )
        .unwrap();
        let id = |axis: &str| {
            expect(
                &p,
                "minecraft:iron_chain",
                &[("pillar_axis", StateValue::Str(axis.into()))],
            )
        };
        assert_eq!(
            convert(&p, "minecraft:chain", &[("waterlogged", "false")]).unwrap(),
            id("y")
        );
        let ids = java_rotations(
            &p,
            "legacy bastion",
            "minecraft:chain",
            &props(&[("axis", "x"), ("waterlogged", "false")]),
        )
        .unwrap();
        assert_eq!(ids, [id("x"), id("z"), id("x"), id("z")]);
    }

    #[test]
    fn unmapped_properties_and_blocks_fail() {
        let p = palette();
        assert!(convert(&p, "minecraft:oak_log", &[("powered", "true")]).is_err());
        assert!(convert(&p, "minecraft:oak_planks", &[("facing", "north")]).is_err());
        assert!(convert(&p, "minecraft:not_a_block", &[]).is_err());
        assert!(convert(&p, "minecraft:oak_stairs", &[("facing", "sideways")]).is_err());
    }

    #[test]
    fn rotation_moves_facing_axis_and_sides() {
        let p = palette();
        let stairs = props(&[
            ("facing", "north"),
            ("half", "bottom"),
            ("shape", "straight"),
        ]);
        let ids = java_rotations(&p, "t", "minecraft:oak_stairs", &stairs).unwrap();
        // north -> east -> south -> west
        let dir = |d: i32| {
            expect(
                &p,
                "minecraft:oak_stairs",
                &[("weirdo_direction", StateValue::Int(d))],
            )
        };
        assert_eq!(ids, [dir(3), dir(0), dir(2), dir(1)]);
        let log = java_rotations(&p, "t", "minecraft:oak_log", &props(&[("axis", "x")])).unwrap();
        assert_eq!(log[0], log[2]);
        assert_eq!(
            log[1],
            expect(
                &p,
                "minecraft:oak_log",
                &[("pillar_axis", StateValue::Str("z".into()))]
            )
        );
        let fence = rotate_java_properties(
            &props(&[
                ("north", "true"),
                ("east", "false"),
                ("south", "false"),
                ("west", "false"),
            ]),
            1,
        );
        assert_eq!(fence.get("east").map(String::as_str), Some("true"));
        assert_eq!(fence.get("north").map(String::as_str), Some("false"));
    }

    #[test]
    fn offsets_and_directions_rotate_consistently() {
        // Rotating the offset of a direction equals the offset of the rotated direction.
        let delta = |d: Dir6| match d {
            Dir6::North => (0, -1),
            Dir6::South => (0, 1),
            Dir6::West => (-1, 0),
            Dir6::East => (1, 0),
            _ => (0, 0),
        };
        for d in [Dir6::North, Dir6::East, Dir6::South, Dir6::West] {
            for r in 0..4 {
                let (x, z) = delta(d);
                assert_eq!(rotate_offset(x, z, r), delta(d.rotate(r)), "{d:?} r{r}");
            }
        }
        assert_eq!(rotate_offset(3, 5, 4), (3, 5));
        assert_eq!(Dir6::Up.rotate(1), Dir6::Up);
    }

    #[test]
    fn bedrock_states_rotate() {
        let mut d = BlockDescriptor::named("minecraft:jigsaw");
        d.states
            .insert("facing_direction".into(), StateValue::Int(2));
        let r = rotate_bedrock("t", &d, 1).unwrap();
        assert_eq!(r.states["facing_direction"], StateValue::Int(5));
        let mut legacy = BlockDescriptor::named("minecraft:cocoa");
        legacy.states.insert("direction".into(), StateValue::Int(0));
        assert_eq!(
            rotate_bedrock("t", &legacy, 1).unwrap().states["direction"],
            StateValue::Int(1)
        );
        let mut vine = BlockDescriptor::named("minecraft:vine");
        vine.states
            .insert("vine_direction_bits".into(), StateValue::Int(1));
        assert!(rotate_bedrock("t", &vine, 1).is_err());
        assert!(rotate_bedrock("t", &vine, 0).is_ok());
    }

    #[test]
    fn mushroom_faces_fold_into_bits() {
        let p = |pairs: &[(&str, &str)]| props(pairs);
        let all = |v: &str| {
            p(&[
                ("north", v),
                ("east", v),
                ("south", v),
                ("west", v),
                ("up", v),
                ("down", v),
            ])
        };
        assert_eq!(mushroom_bits(&all("true"), false), 14);
        assert_eq!(mushroom_bits(&all("false"), false), 0);
        let nw = p(&[
            ("north", "true"),
            ("east", "false"),
            ("south", "false"),
            ("west", "true"),
            ("up", "true"),
            ("down", "false"),
        ]);
        assert_eq!(mushroom_bits(&nw, false), 1);
        let se = p(&[
            ("north", "false"),
            ("east", "true"),
            ("south", "true"),
            ("west", "false"),
            ("up", "true"),
            ("down", "false"),
        ]);
        assert_eq!(mushroom_bits(&se, false), 9);
        assert_eq!(mushroom_bits(&all("true"), true), 15);
    }

    #[test]
    fn state_strings_parse() {
        let (name, props) = parse_state_string("t", "minecraft:oak_log[axis=x]").unwrap();
        assert_eq!(name, "minecraft:oak_log");
        assert_eq!(props.get("axis").map(String::as_str), Some("x"));
        let (bare, none) = parse_state_string("t", "minecraft:dirt_path").unwrap();
        assert_eq!(bare, "minecraft:dirt_path");
        assert!(none.is_empty());
        assert!(parse_state_string("t", "minecraft:x[a=b").is_err());
    }

    #[test]
    fn feature_names_resolve_to_nested_or_root_templates() {
        let mut files = BTreeMap::new();
        files.insert(
            "minecraft/sulfur_spring_small_1".to_owned(),
            PathBuf::from("a"),
        );
        files.insert("hot_air_balloon".to_owned(), PathBuf::from("b"));
        assert_eq!(
            template_location("minecraft:sulfur_spring_small_1", &files).as_deref(),
            Some("minecraft/sulfur_spring_small_1")
        );
        assert_eq!(
            template_location("mystructure:hot_air_balloon", &files).as_deref(),
            Some("hot_air_balloon")
        );
        assert_eq!(template_location("example:hot_air_balloon", &files), None);
    }
}
