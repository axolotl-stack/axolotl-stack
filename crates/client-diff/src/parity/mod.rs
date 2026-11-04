//! BDS-vs-server scenario parity: a scenario recorded on BDS with the
//! gophertunnel bot is retained under `crates/client-diff/scenarios/<name>/`
//! (trimmed recording plus manifest) and compared with the same scenario run
//! by [`crate::driver`] against another server.
//!
//! The comparison keeps the server-to-client packets of the manifest's
//! packet types inside the scenario window (from the first step packet to
//! the end of the retained recording), normalizes them with the manifest's
//! masks, aligns the two streams (LCS on packet type and target entity),
//! compares aligned packets field by field (floats within a tolerance) and
//! their timing relative to the preceding step packet, and checks that
//! every setup command was accepted. Every difference is either listed in
//! the manifest (known failing or accepted, each with a reason) or fails the
//! comparison; listed differences that no longer occur fail it too.

pub mod tree;

use std::{
    collections::HashMap,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    Record,
    scenario::{Scenario, ScenariosFile, Step, StepKind},
};
use tree::{Node, join};

/// Directory of the retained scenarios.
pub fn scenarios_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios")
}

/// The shared step script both drivers run.
pub fn shared_script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/client-diff/scenarios.json")
}

/// Packet types compared by default (items, entities and blocks lanes).
pub const DEFAULT_PACKETS: &[&str] = &[
    "ActorEvent",
    "AddActor",
    "AddItemActor",
    "Animate",
    "AnimateEntity",
    "BlockActorData",
    "BlockEvent",
    "ClientboundMapItemData",
    "CompletedUsingItem",
    "ContainerClose",
    "ContainerOpen",
    "ContainerSetData",
    "CorrectPlayerMovePrediction",
    "InventoryContent",
    "InventorySlot",
    "InventoryTransaction",
    "ItemStackResponse",
    "LevelEvent",
    "LevelEventGeneric",
    "LevelSoundEvent",
    "MobEffect",
    "MobEquipment",
    "MoveActorAbsolute",
    "MoveActorDelta",
    "MovePlayer",
    "PlaySound",
    "PlayerEnchantOptions",
    "PlayerHotbar",
    "PlayerStartItemCooldown",
    "RemoveActor",
    "SetActorData",
    "SetActorLink",
    "SetActorMotion",
    "SetHealth",
    "TakeItemActor",
    "Text",
    "UpdateAttributes",
    "UpdateBlock",
];

/// Packet types never retained: world streaming and per-tick client input
/// (each driver ticks on its own clock; the steps are what matter).
const NOT_RETAINED: &[&str] = &[
    "LevelChunk",
    "SubChunk",
    "NetworkChunkPublisherUpdate",
    "PlayerAuthInput",
];

/// Feedback that means the server did not run a command at all.
const UNSUPPORTED_COMMAND: &[&str] = &[
    "commands.generic.unknown",
    "Unknown command",
    "commands.generic.syntax",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Scenario name in `tools/client-diff/scenarios.json`.
    pub scenario: String,
    /// The scenario's first `steps` steps run, then the client idles for the
    /// `after_ms` of the next step (see [`Scenario::prefix`]).
    pub steps: usize,
    /// SHA-256 of those steps as JSON: the retained recording belongs to them.
    pub steps_sha256: String,
    pub description: String,
    /// The first `setup_steps` steps only prepare the behaviour under test
    /// (give items, place blocks, clear effects): their server packets are
    /// not compared (unsupported commands still fail). `setup_reason` says
    /// why and is printed in every report.
    #[serde(default)]
    pub setup_steps: usize,
    #[serde(default)]
    pub setup_reason: String,
    pub source: Source,
    pub world: World,
    pub compare: Compare,
    #[serde(default, rename = "mask")]
    pub masks: Vec<Mask>,
    /// Differences unastar still has (not parity): listed so the test fails
    /// on anything else, and fails when one of them disappears. Each names
    /// the lane that owns the fix.
    #[serde(default)]
    pub known_failing: Vec<Expectation>,
    /// Differences the integrator accepted as not player-visible.
    #[serde(default)]
    pub accepted: Vec<Expectation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// The bot capture the recording was cut from.
    pub run: String,
    pub server: String,
    pub protocol: u32,
    pub recorded: String,
    /// The player's IDs in the recording (from its StartGame).
    pub player_runtime_id: u64,
    pub player_unique_id: i64,
    /// The player's enchanting seed (StartGame): the server under test gives
    /// the player the same seed so enchanting offers can be compared.
    pub player_enchantment_seed: i32,
    /// Scenario window: milliseconds from the first step packet to the end
    /// of the retained recording.
    pub window_ms: f64,
    pub recording_sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldKind {
    /// BDS default superflat: bedrock y=-64, dirt -63..-62, grass block -61,
    /// spawn feet (0, -60, 0).
    Superflat,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct World {
    pub kind: WorldKind,
    /// The world's default game mode at join (the BDS world's StartGame):
    /// `survival`, `creative` or `adventure`.
    pub game_mode: String,
    /// Difficulty at join: `peaceful`, `easy`, `normal` or `hard`.
    pub difficulty: String,
    /// Every game rule at join (the BDS join GameRulesChanged).
    pub game_rules: std::collections::BTreeMap<String, RuleValue>,
    /// Other server settings and state the recording started from.
    pub setup: String,
}

/// A game rule value as the world had it.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum RuleValue {
    Bool(bool),
    Int(i32),
    Float(f32),
}

/// The world settings a BDS capture joined into: the StartGame default game
/// mode and difficulty and the join GameRulesChanged.
fn joined_world(
    packets: &[Packet],
) -> Result<(
    String,
    String,
    std::collections::BTreeMap<String, RuleValue>,
)> {
    let start = packets
        .iter()
        .find(|p| p.name == "StartGame")
        .context("the capture has no StartGame")?;
    let setting = |field: &str| {
        start
            .tree
            .field("settings")
            .and_then(|settings| settings.field(field))
            .map(|value| value.inline().to_lowercase())
            .with_context(|| format!("StartGame has no settings.{field}"))
    };
    let rules = packets
        .iter()
        .find(|p| p.name == "GameRulesChanged")
        .context("the capture has no GameRulesChanged at join")?;
    let Some(Node::List(list)) = rules
        .tree
        .field("rule_data")
        .and_then(|data| data.field("rules_list"))
    else {
        bail!("GameRulesChanged has no rules list");
    };
    let mut game_rules = std::collections::BTreeMap::new();
    for rule in list {
        let name = rule
            .field("rule_name")
            .map(Node::inline)
            .context("game rule without a name")?;
        let name = name.trim_matches('"').to_owned();
        let value = match rule.field("rule_value") {
            Some(Node::Tuple(kind, items)) if items.len() == 1 => {
                let text = items[0].inline();
                match kind.as_str() {
                    "Bool" => RuleValue::Bool(text.parse()?),
                    "Int" | "Int32" => RuleValue::Int(text.parse()?),
                    "Float" => RuleValue::Float(text.parse()?),
                    other => bail!("game rule {name} has a {other} value"),
                }
            }
            other => bail!("game rule {name} has value {other:?}"),
        };
        game_rules.insert(name, value);
    }
    Ok((
        setting("game_type")?,
        setting("game_difficulty")?,
        game_rules,
    ))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Compare {
    /// Server-to-client packet types compared (valentine names without the
    /// `Packet` suffix).
    pub packets: Vec<String>,
    /// Allowed difference of a packet's delay after its step packet.
    pub timing_tolerance_ms: f64,
    /// Allowed absolute difference of floating point fields.
    pub float_tolerance: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    /// Not compared.
    Ignore,
    /// Entity runtime IDs: compared by order of first appearance, the
    /// player as `self` (0 stays 0).
    RuntimeId,
    /// Entity unique IDs, likewise (-1 and 0 stay).
    UniqueId,
    /// Item stack network IDs, likewise (0 stays).
    StackId,
}

/// A normalization applied before comparing. `field` matches every struct
/// field of that name; `path` one location (`actor_data.data[id=5].payload`,
/// `[*]` for every list element). `packets` restricts it (empty: all).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mask {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packets: Vec<String>,
    pub rule: Rule,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffKind {
    /// An aligned packet differs in a field.
    Field,
    /// BDS sent a packet unastar did not.
    Missing,
    /// unastar sent a packet BDS did not.
    Extra,
    /// An aligned packet came after another step or outside the tolerance.
    Timing,
    /// A setup command was not run by unastar.
    Command,
    /// The step packets of the two runs differ (a step was skipped).
    Steps,
}

/// A difference the manifest lists. `packet` and `path` narrow the match
/// (`path` matches itself and everything below it).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Expectation {
    pub kind: DiffKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packet: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The lane that owns the fix (required for `known_failing`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub reason: String,
}

impl Expectation {
    fn matches(&self, difference: &Difference) -> bool {
        self.kind == difference.kind
            && self
                .packet
                .as_ref()
                .is_none_or(|packet| *packet == difference.packet)
            && self.path.as_ref().is_none_or(|path| {
                difference.path == *path
                    || difference
                        .path
                        .strip_prefix(path.as_str())
                        .is_some_and(|rest| rest.starts_with(['.', '[']))
            })
    }
}

/// The default masks a retained scenario starts with, with their reasons.
pub fn default_masks() -> Vec<Mask> {
    let mask = |field: &str, rule, reason: &str| Mask {
        field: Some(field.into()),
        path: None,
        packets: Vec::new(),
        rule,
        reason: reason.into(),
    };
    vec![
        mask(
            "actor_runtime_id",
            Rule::RuntimeId,
            "Runtime IDs are allocated per server; entities are compared by order of first appearance.",
        ),
        mask(
            "actor_unique_id",
            Rule::UniqueId,
            "Unique IDs are allocated per server; entities are compared by order of first appearance.",
        ),
        Mask {
            field: None,
            path: Some("actor_data.data[id=5].payload".into()),
            packets: vec![
                "SetActorData".into(),
                "AddActor".into(),
                "AddItemActor".into(),
            ],
            rule: Rule::UniqueId,
            reason: "Metadata key 5 (owner) holds a unique ID.".into(),
        },
        mask(
            "net_id_variant",
            Rule::StackId,
            "Stack network IDs come from a per-server counter; stacks are compared by order of first appearance.",
        ),
        mask(
            "item_stack_net_id",
            Rule::StackId,
            "Stack network IDs come from a per-server counter; stacks are compared by order of first appearance.",
        ),
        mask(
            "inputtick",
            Rule::Ignore,
            "Server tick numbers start at different values on each server and session.",
        ),
    ]
}

/// A retained scenario: manifest and recording.
pub struct Case {
    pub name: String,
    pub dir: PathBuf,
    pub manifest: Manifest,
    pub recording: Vec<Record>,
}

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn steps_sha256(steps: &[Step]) -> Result<String> {
    Ok(sha256_hex(&serde_json::to_vec(steps)?))
}

impl Case {
    /// Loads `dir`, checking the recording against its recorded hash.
    pub fn load(dir: &Path) -> Result<Case> {
        let name = dir
            .file_name()
            .and_then(|name| name.to_str())
            .context("scenario directory name")?
            .to_owned();
        let manifest_path = dir.join("manifest.toml");
        let manifest: Manifest = toml::from_str(
            &fs::read_to_string(&manifest_path)
                .with_context(|| format!("reading {}", manifest_path.display()))?,
        )
        .with_context(|| format!("parsing {}", manifest_path.display()))?;
        let recording_path = dir.join("recording.jsonl");
        let text = fs::read(&recording_path)
            .with_context(|| format!("reading {}", recording_path.display()))?;
        ensure!(
            sha256_hex(&text) == manifest.source.recording_sha256,
            "{} does not match recording_sha256 in the manifest",
            recording_path.display()
        );
        for mask in &manifest.masks {
            ensure!(
                mask.field.is_some() != mask.path.is_some(),
                "{name}: a mask names exactly one of field or path"
            );
        }
        for entry in &manifest.known_failing {
            ensure!(
                entry.owner.as_deref().is_some_and(|owner| !owner.is_empty()),
                "{name}: known_failing entry {:?} names no owner",
                entry.reason
            );
        }
        let recording = crate::parse(
            std::str::from_utf8(&text).context("recording is not UTF-8")?,
            &recording_path.display().to_string(),
        )?;
        Ok(Case {
            name,
            dir: dir.to_owned(),
            manifest,
            recording,
        })
    }

    /// Every retained scenario, by directory name.
    pub fn all() -> Result<Vec<Case>> {
        let mut dirs: Vec<_> = fs::read_dir(scenarios_root())?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<_, _>>()?;
        dirs.retain(|dir| dir.join("manifest.toml").is_file());
        dirs.sort();
        dirs.iter().map(|dir| Case::load(dir)).collect()
    }

    /// The steps to run, checked against the steps the recording belongs to.
    pub fn script(&self, file: &ScenariosFile) -> Result<Scenario> {
        let scenario = file
            .scenario(&self.manifest.scenario)?
            .prefix(self.manifest.steps)?;
        ensure!(
            steps_sha256(&scenario.steps)? == self.manifest.steps_sha256,
            "{}: the steps of scenario {:?} changed since the recording was retained; record and retain it again",
            self.name,
            self.manifest.scenario
        );
        Ok(scenario)
    }
}

/// One side of a comparison.
pub struct Side<'a> {
    pub records: &'a [Record],
    pub runtime_id: u64,
    pub unique_id: i64,
}

/// A decoded packet of one side.
struct Packet {
    seq: u64,
    t: f64,
    name: String,
    c2s: bool,
    tree: Node,
}

/// A capture record's packet type (valentine name without `Packet`) and its
/// decoded tree (the raw hex for packets valentine cannot decode).
pub fn decode_record(record: &Record) -> Result<(String, Node)> {
    let bytes = hex::decode(&record.hex).context("invalid packet hex")?;
    Ok(match crate::decode(&bytes) {
        Some((_, data)) => {
            let name = format!("{:?}", data.packet_id());
            let name = name.strip_suffix("Packet").unwrap_or(&name).to_owned();
            // `Name(Name { .. })`: compare the inner struct.
            let tree = match Node::of(&data)? {
                Node::Tuple(_, mut items) if items.len() == 1 => items.remove(0),
                other => other,
            };
            (name, tree)
        }
        None => (
            format!("Unknown_{}", record.id),
            Node::Atom(record.hex.clone()),
        ),
    })
}

fn decode(record: &Record) -> Result<Packet> {
    let (name, tree) = decode_record(record)?;
    Ok(Packet {
        seq: record.seq,
        t: record.t_ms,
        name,
        c2s: record.dir == "c2s",
        tree,
    })
}

/// Client packet names that locate steps (see [`StepKind::marker`]).
fn is_marker(name: &str) -> bool {
    matches!(
        name,
        "Text"
            | "CommandRequest"
            | "MobEquipment"
            | "Interact"
            | "ContainerClose"
            | "BookEdit"
            | "PlayerToggleCrafterSlotRequest"
            | "MapInfoRequest"
            | "ItemStackRequest"
            | "InventoryTransaction"
    )
}

/// Canonical labels for IDs, by order of first appearance.
#[derive(Default)]
struct Canon {
    labels: [HashMap<String, String>; 3],
}

impl Canon {
    fn new(runtime_id: u64, unique_id: i64) -> Self {
        let mut canon = Canon::default();
        canon.labels[0].insert(runtime_id.to_string(), "self".into());
        canon.labels[1].insert(unique_id.to_string(), "self".into());
        canon
    }

    fn relabel(&mut self, node: &mut Node, rule: Rule) {
        let (index, prefix, literal): (usize, &str, &[&str]) = match rule {
            Rule::Ignore => {
                *node = Node::Atom("<masked>".into());
                return;
            }
            Rule::RuntimeId => (0, "runtime#", &["0"]),
            Rule::UniqueId => (1, "unique#", &["0", "-1"]),
            Rule::StackId => (2, "stack#", &["0"]),
        };
        match node {
            Node::Atom(atom) => {
                if atom.parse::<i128>().is_ok() && !literal.contains(&atom.as_str()) {
                    let labels = &mut self.labels[index];
                    let next = format!("{prefix}{}", labels.len());
                    *atom = labels.entry(atom.clone()).or_insert(next).clone();
                }
            }
            Node::Struct(_, fields) => {
                for (_, value) in fields {
                    self.relabel(value, rule);
                }
            }
            Node::Tuple(_, items) | Node::List(items) => {
                for item in items {
                    self.relabel(item, rule);
                }
            }
            Node::Map(entries) => {
                for (_, value) in entries {
                    self.relabel(value, rule);
                }
            }
        }
    }
}

/// A mask with its parsed path.
type CompiledMask = (Mask, Option<Vec<(String, Selector)>>);

/// One step of a mask path: a field, optionally selecting list elements.
#[derive(Debug)]
enum Selector {
    None,
    All,
    Where(String, String),
}

fn parse_path(path: &str) -> Result<Vec<(String, Selector)>> {
    path.split('.')
        .map(|segment| {
            let Some((field, rest)) = segment.split_once('[') else {
                return Ok((segment.to_owned(), Selector::None));
            };
            let inner = rest
                .strip_suffix(']')
                .with_context(|| format!("unclosed selector in {path:?}"))?;
            let selector = if inner == "*" {
                Selector::All
            } else {
                let (key, value) = inner
                    .split_once('=')
                    .with_context(|| format!("selector {inner:?} is neither * nor key=value"))?;
                Selector::Where(key.to_owned(), value.to_owned())
            };
            Ok((field.to_owned(), selector))
        })
        .collect()
}

/// Steps into single-value wrappers (`Some(x)`, enum variants with one
/// value) so paths can name the fields inside them.
fn unwrap_mut(node: &mut Node) -> &mut Node {
    if matches!(node, Node::Tuple(_, items) if items.len() == 1) {
        let Node::Tuple(_, items) = node else {
            unreachable!("matched above")
        };
        return unwrap_mut(&mut items[0]);
    }
    node
}

fn apply_path(node: &mut Node, segments: &[(String, Selector)], apply: &mut dyn FnMut(&mut Node)) {
    let Some(((field, selector), rest)) = segments.split_first() else {
        apply(node);
        return;
    };
    let Node::Struct(_, fields) = unwrap_mut(node) else {
        return;
    };
    let Some((_, value)) = fields.iter_mut().find(|(name, _)| name == field) else {
        return;
    };
    match selector {
        Selector::None => apply_path(value, rest, apply),
        Selector::All | Selector::Where(..) => {
            let Node::List(items) = unwrap_mut(value) else {
                return;
            };
            for item in items {
                let selected = match selector {
                    Selector::Where(key, expected) => {
                        unwrap_mut(item).field(key).map(Node::inline).as_deref()
                            == Some(expected.as_str())
                    }
                    _ => true,
                };
                if selected {
                    apply_path(item, rest, apply);
                }
            }
        }
    }
}

fn apply_field(node: &mut Node, field: &str, apply: &mut dyn FnMut(&mut Node)) {
    match node {
        Node::Struct(_, fields) => {
            for (name, value) in fields {
                if name == field {
                    apply(value);
                } else {
                    apply_field(value, field, apply);
                }
            }
        }
        Node::Tuple(_, items) | Node::List(items) => {
            for item in items {
                apply_field(item, field, apply);
            }
        }
        Node::Map(entries) => {
            for (_, value) in entries {
                apply_field(value, field, apply);
            }
        }
        Node::Atom(_) => {}
    }
}

fn normalize(packet: &mut Packet, masks: &[CompiledMask], canon: &mut Canon) {
    for (mask, path) in masks {
        if !mask.packets.is_empty() && !mask.packets.contains(&packet.name) {
            continue;
        }
        let mut apply = |node: &mut Node| canon.relabel(node, mask.rule);
        match (path, &mask.field) {
            (Some(path), _) => apply_path(&mut packet.tree, path, &mut apply),
            (None, Some(field)) => apply_field(&mut packet.tree, field, &mut apply),
            (None, None) => unreachable!("checked when loading"),
        }
    }
}

/// The alignment key: packet type and the (normalized) entity it targets.
fn key(packet: &Packet) -> String {
    fn target(node: &Node) -> Option<String> {
        match node {
            Node::Struct(_, fields) => fields.iter().find_map(|(name, value)| {
                if name.ends_with("runtime_id") {
                    Some(value.inline())
                } else {
                    target(value)
                }
            }),
            _ => None,
        }
    }
    match target(&packet.tree) {
        Some(target) => format!("{}@{target}", packet.name),
        None => packet.name.clone(),
    }
}

/// Lists keyed by these fields are compared entry by entry (actor metadata
/// by `id`, attributes by `name`).
const LIST_KEYS: &[&str] = &["id", "name"];

fn list_key(items: &[Node]) -> Option<&'static str> {
    LIST_KEYS.iter().copied().find(|key| {
        let mut seen = std::collections::HashSet::new();
        items.iter().all(|item| {
            item.field(key)
                .is_some_and(|value| seen.insert(value.inline()))
        })
    })
}

fn atoms_equal(a: &str, b: &str, tolerance: f64) -> bool {
    if a == b {
        return true;
    }
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(a), Ok(b)) if a.is_finite() && b.is_finite() => (a - b).abs() <= tolerance,
        _ => false,
    }
}

/// Field differences between two normalized trees: (path, BDS, unastar).
fn diff_trees(
    a: &Node,
    b: &Node,
    path: &str,
    tolerance: f64,
    out: &mut Vec<(String, String, String)>,
) {
    match (a, b) {
        (Node::Atom(x), Node::Atom(y)) if atoms_equal(x, y, tolerance) => {}
        (Node::Struct(an, af), Node::Struct(bn, bf)) if an == bn => {
            for (field, av) in af {
                match bf.iter().find(|(name, _)| name == field) {
                    Some((_, bv)) => diff_trees(av, bv, &join(path, field), tolerance, out),
                    None => out.push((join(path, field), av.inline(), "<absent>".into())),
                }
            }
        }
        (Node::Tuple(an, ai), Node::Tuple(bn, bi)) if an == bn && ai.len() == bi.len() => {
            for (i, (av, bv)) in ai.iter().zip(bi).enumerate() {
                let segment = if ai.len() == 1 {
                    an.clone()
                } else {
                    format!("{an}.{i}")
                };
                diff_trees(av, bv, &join(path, &segment), tolerance, out);
            }
        }
        (Node::List(ai), Node::List(bi)) => {
            if let (Some(ka), Some(kb)) = (list_key(ai), list_key(bi))
                && ka == kb
            {
                let entry = |item: &Node| item.field(ka).map(Node::inline).unwrap_or_default();
                for av in ai {
                    let id = entry(av);
                    let at = format!("{path}[{ka}={id}]");
                    match bi.iter().find(|bv| entry(bv) == id) {
                        Some(bv) => diff_trees(av, bv, &at, tolerance, out),
                        None => out.push((at, av.inline(), "<absent>".into())),
                    }
                }
                for bv in bi {
                    let id = entry(bv);
                    if !ai.iter().any(|av| entry(av) == id) {
                        out.push((format!("{path}[{ka}={id}]"), "<absent>".into(), bv.inline()));
                    }
                }
            } else if ai.len() == bi.len() {
                for (i, (av, bv)) in ai.iter().zip(bi).enumerate() {
                    diff_trees(av, bv, &format!("{path}[{i}]"), tolerance, out);
                }
            } else {
                out.push((path.to_owned(), a.inline(), b.inline()));
            }
        }
        _ => out.push((path.to_owned(), a.inline(), b.inline())),
    }
}

#[derive(Clone, Debug)]
pub struct Difference {
    pub kind: DiffKind,
    pub packet: String,
    pub path: String,
    pub detail: String,
    /// Indexes into the compared packets of each side.
    bds: Option<usize>,
    unastar: Option<usize>,
}

/// A compared server packet with its position relative to the steps.
struct Compared {
    packet: Packet,
    /// Index of the last step packet sent before it.
    step: usize,
    /// Milliseconds after that step packet.
    after: f64,
}

struct Prepared {
    markers: Vec<Packet>,
    compared: Vec<Compared>,
    /// Server feedback (Text, CommandOutput) per step packet.
    feedback: Vec<Vec<String>>,
}

fn prepare(
    side: &Side<'_>,
    manifest: &Manifest,
    masks: &[CompiledMask],
    window_ms: f64,
    setup_markers: usize,
) -> Result<Prepared> {
    let packets = side
        .records
        .iter()
        .map(decode)
        .collect::<Result<Vec<_>>>()?;
    let first = packets
        .iter()
        .position(|p| p.c2s && is_marker(&p.name))
        .context("the run sent no step packet")?;
    let start = packets[first].t;
    let mut canon = Canon::new(side.runtime_id, side.unique_id);
    let mut prepared = Prepared {
        markers: Vec::new(),
        compared: Vec::new(),
        feedback: Vec::new(),
    };
    for mut packet in packets.into_iter().skip(first) {
        if packet.t - start >= window_ms {
            break;
        }
        if packet.c2s {
            if is_marker(&packet.name) {
                prepared.markers.push(packet);
                prepared.feedback.push(Vec::new());
            }
            continue;
        }
        let step = prepared.markers.len() - 1;
        if matches!(packet.name.as_str(), "Text" | "CommandOutput") {
            prepared.feedback[step].push(packet.tree.inline());
        }
        if step < setup_markers || !manifest.compare.packets.contains(&packet.name) {
            continue;
        }
        normalize(&mut packet, masks, &mut canon);
        let after = packet.t - prepared.markers[step].t;
        prepared.compared.push(Compared {
            packet,
            step,
            after,
        });
    }
    Ok(prepared)
}

/// The outcome of comparing a run with a retained scenario.
pub struct Comparison {
    pub differences: Vec<Difference>,
    /// Indexes into `differences`.
    pub unexpected: Vec<usize>,
    pub known_failing: Vec<usize>,
    pub accepted: Vec<usize>,
    /// Manifest entries that matched nothing.
    pub stale: Vec<String>,
    pub report: String,
}

impl Comparison {
    pub fn passed(&self) -> bool {
        self.unexpected.is_empty() && self.stale.is_empty()
    }
}

/// Compares a run of `case` (made with `script`) against its recording.
pub fn compare(
    case: &Case,
    script: &Scenario,
    run: &Side<'_>,
    notes: &[String],
) -> Result<Comparison> {
    let manifest = &case.manifest;
    ensure!(
        manifest.compare.float_tolerance.is_finite()
            && manifest.compare.float_tolerance >= 0.0
            && manifest.compare.timing_tolerance_ms.is_finite()
            && manifest.compare.timing_tolerance_ms >= 0.0,
        "{}: tolerances must be finite and non-negative",
        case.name
    );
    let masks = manifest
        .masks
        .iter()
        .map(|mask| {
            Ok((
                mask.clone(),
                mask.path.as_deref().map(parse_path).transpose()?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let window = manifest.source.window_ms;
    ensure!(
        manifest.setup_steps <= manifest.steps
            && (manifest.setup_steps == 0) == manifest.setup_reason.is_empty(),
        "{}: setup_steps must be within the steps and come with a setup_reason",
        case.name
    );
    // Server packets after the step packets of setup steps are not compared.
    let setup_markers = script.steps[..manifest.setup_steps]
        .iter()
        .filter(|step| step.kind.marker().is_some())
        .count();
    let bds = prepare(
        &Side {
            records: &case.recording,
            runtime_id: manifest.source.player_runtime_id,
            unique_id: manifest.source.player_unique_id,
        },
        manifest,
        &masks,
        window,
        setup_markers,
    )?;
    let other = prepare(run, manifest, &masks, window, setup_markers)?;
    let mut differences = Vec::new();

    // Step packets: both drivers send one per step, so their sequences
    // match unless a step was skipped or refused on one side.
    let names = |side: &Prepared| {
        side.markers
            .iter()
            .map(|p| p.name.clone())
            .collect::<Vec<_>>()
    };
    let (bds_steps, run_steps) = (names(&bds), names(&other));
    if bds_steps != run_steps {
        differences.push(Difference {
            kind: DiffKind::Steps,
            packet: String::new(),
            path: String::new(),
            detail: format!(
                "step packets differ:\n  BDS:     {bds_steps:?}\n  unastar: {run_steps:?}"
            ),
            bds: None,
            unastar: None,
        });
    }

    // Commands the server did not run.
    for (k, marker) in other.markers.iter().enumerate() {
        if marker.name != "CommandRequest" {
            continue;
        }
        let refused = |feedback: &[String]| {
            feedback
                .iter()
                .find(|text| UNSUPPORTED_COMMAND.iter().any(|p| text.contains(p)))
                .cloned()
        };
        let Some(answer) = refused(&other.feedback[k]) else {
            continue;
        };
        if bds.feedback.get(k).is_some_and(|f| refused(f).is_some()) {
            continue;
        }
        let command = marker
            .tree
            .field("command")
            .map(Node::inline)
            .unwrap_or_default();
        differences.push(Difference {
            kind: DiffKind::Command,
            packet: "CommandRequest".into(),
            path: String::new(),
            detail: format!(
                "unsupported command {command} (step packet {k}): unastar answered {answer}; BDS answered {:?}",
                bds.feedback.get(k).cloned().unwrap_or_default()
            ),
            bds: None,
            unastar: None,
        });
    }

    let keys = |side: &Prepared| {
        side.compared
            .iter()
            .map(|c| key(&c.packet))
            .collect::<Vec<_>>()
    };
    let (bds_keys, run_keys) = (keys(&bds), keys(&other));
    let rows = crate::align(&bds_keys, &run_keys)?;
    let tolerance = manifest.compare.timing_tolerance_ms;
    for &(left, right) in &rows {
        match (left, right) {
            (Some(i), Some(j)) => {
                let (a, b) = (&bds.compared[i], &other.compared[j]);
                let mut fields = Vec::new();
                diff_trees(
                    &a.packet.tree,
                    &b.packet.tree,
                    "",
                    manifest.compare.float_tolerance,
                    &mut fields,
                );
                for (path, x, y) in fields {
                    differences.push(Difference {
                        kind: DiffKind::Field,
                        packet: a.packet.name.clone(),
                        path,
                        detail: format!("BDS {x} | unastar {y}"),
                        bds: Some(i),
                        unastar: Some(j),
                    });
                }
                if a.step != b.step || (a.after - b.after).abs() > tolerance {
                    differences.push(Difference {
                        kind: DiffKind::Timing,
                        packet: a.packet.name.clone(),
                        path: String::new(),
                        detail: format!(
                            "BDS {:.0} ms after step packet {} | unastar {:.0} ms after step packet {} (tolerance {tolerance} ms)",
                            a.after, a.step, b.after, b.step
                        ),
                        bds: Some(i),
                        unastar: Some(j),
                    });
                }
            }
            (Some(i), None) => differences.push(Difference {
                kind: DiffKind::Missing,
                packet: bds.compared[i].packet.name.clone(),
                path: String::new(),
                detail: format!(
                    "BDS sent it {:.0} ms after step packet {}: {}",
                    bds.compared[i].after,
                    bds.compared[i].step,
                    bds.compared[i].packet.tree.inline()
                ),
                bds: Some(i),
                unastar: None,
            }),
            (None, Some(j)) => differences.push(Difference {
                kind: DiffKind::Extra,
                packet: other.compared[j].packet.name.clone(),
                path: String::new(),
                detail: format!(
                    "unastar sent it {:.0} ms after step packet {}: {}",
                    other.compared[j].after,
                    other.compared[j].step,
                    other.compared[j].packet.tree.inline()
                ),
                bds: None,
                unastar: Some(j),
            }),
            (None, None) => unreachable!("alignment rows have a side"),
        }
    }

    let mut comparison = Comparison {
        differences,
        unexpected: Vec::new(),
        known_failing: Vec::new(),
        accepted: Vec::new(),
        stale: Vec::new(),
        report: String::new(),
    };
    let mut used_known = vec![false; manifest.known_failing.len()];
    let mut used_accepted = vec![false; manifest.accepted.len()];
    for (index, difference) in comparison.differences.iter().enumerate() {
        if let Some(k) = manifest
            .known_failing
            .iter()
            .position(|e| e.matches(difference))
        {
            used_known[k] = true;
            comparison.known_failing.push(index);
        } else if let Some(k) = manifest.accepted.iter().position(|e| e.matches(difference)) {
            used_accepted[k] = true;
            comparison.accepted.push(index);
        } else {
            comparison.unexpected.push(index);
        }
    }
    for (list, used, label) in [
        (&manifest.known_failing, &used_known, "known_failing"),
        (&manifest.accepted, &used_accepted, "accepted"),
    ] {
        for (entry, used) in list.iter().zip(used) {
            if !used {
                comparison.stale.push(format!(
                    "{label} {:?} {} {}: no longer occurs; remove it ({})",
                    entry.kind,
                    entry.packet.as_deref().unwrap_or("*"),
                    entry.path.as_deref().unwrap_or(""),
                    entry.reason
                ));
            }
        }
    }
    comparison.report = report(case, script, &bds, &other, &rows, &comparison, notes)?;
    Ok(comparison)
}

/// The step each step packet belongs to, for labels.
fn marker_steps(script: &Scenario) -> Vec<usize> {
    script
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.kind.marker().is_some())
        .map(|(i, _)| i)
        .collect()
}

fn step_label(script: &Scenario, steps: &[usize], marker: usize) -> String {
    match steps
        .get(marker)
        .and_then(|&i| script.steps.get(i).map(|s| (i, s)))
    {
        Some((i, step)) if matches!(step.kind, StepKind::Chat | StepKind::Command) => {
            format!("step {i} {} {:?}", step.kind.name(), step.text)
        }
        Some((i, step)) => format!("step {i} {}", step.kind.name()),
        None => format!("step packet {marker}"),
    }
}

const SHOWN_DIFFERENCES: usize = 40;
const SIDE_BY_SIDE: usize = 6;
const COLUMN: usize = 72;

fn side_by_side(out: &mut String, a: Option<&Node>, b: Option<&Node>) {
    let leaves = |node: Option<&Node>| node.map(Node::leaves).unwrap_or_default();
    let (left, right) = (leaves(a), leaves(b));
    let mut paths: Vec<&String> = left.iter().map(|(p, _)| p).collect();
    for (path, _) in &right {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    let value = |side: &[(String, String)], path: &str| {
        side.iter()
            .find(|(p, _)| p == path)
            .map_or_else(|| "<absent>".to_owned(), |(_, v)| v.clone())
    };
    let clip = |text: String| {
        if text.chars().count() > COLUMN {
            format!("{}…", text.chars().take(COLUMN - 1).collect::<String>())
        } else {
            text
        }
    };
    let _ = writeln!(out, "```text\n  {:<COLUMN$} | unastar", "BDS");
    for path in paths {
        let (x, y) = (value(&left, path), value(&right, path));
        let mark = if x == y { ' ' } else { '*' };
        let _ = writeln!(
            out,
            "{mark} {:<COLUMN$} | {}",
            clip(format!("{path} = {x}")),
            clip(format!("{path} = {y}"))
        );
    }
    out.push_str("```\n\n");
}

fn report(
    case: &Case,
    script: &Scenario,
    bds: &Prepared,
    run: &Prepared,
    rows: &[(Option<usize>, Option<usize>)],
    comparison: &Comparison,
    notes: &[String],
) -> Result<String> {
    let manifest = &case.manifest;
    let steps = marker_steps(script);
    let mut out = String::new();
    writeln!(
        out,
        "# Scenario {}: {} steps 0..{} vs BDS\n\n{}\n",
        case.name, manifest.scenario, manifest.steps, manifest.description
    )?;
    writeln!(
        out,
        "Compared packets: BDS {}, unastar {}; aligned {}. Differences: {} unexpected, {} known failing, {} accepted, {} stale manifest entries.\n",
        bds.compared.len(),
        run.compared.len(),
        rows.iter()
            .filter(|(a, b)| a.is_some() && b.is_some())
            .count(),
        comparison.unexpected.len(),
        comparison.known_failing.len(),
        comparison.accepted.len(),
        comparison.stale.len()
    )?;
    if manifest.setup_steps > 0 {
        writeln!(
            out,
            "Setup: steps 0..{} are setup, their server packets are not compared: {}\n",
            manifest.setup_steps, manifest.setup_reason
        )?;
    }
    if !notes.is_empty() {
        out.push_str("## Driver notes\n\n");
        for note in notes {
            writeln!(out, "- {note}")?;
        }
        out.push('\n');
    }
    for entry in &comparison.stale {
        writeln!(out, "- STALE {entry}")?;
    }
    let describe = |out: &mut String, index: usize| -> Result<()> {
        let d = &comparison.differences[index];
        let at = d
            .bds
            .map(|i| &bds.compared[i])
            .or(d.unastar.map(|j| &run.compared[j]));
        let place = at.map_or_else(String::new, |c| {
            format!(" after {}", step_label(script, &steps, c.step))
        });
        let path = if d.path.is_empty() {
            String::new()
        } else {
            format!(" `{}`", d.path)
        };
        writeln!(
            out,
            "- [{:?}] {}{path}{place}: {}",
            d.kind, d.packet, d.detail
        )?;
        Ok(())
    };
    if !comparison.unexpected.is_empty() {
        let mut summary: Vec<((DiffKind, &str), usize)> = Vec::new();
        for &index in &comparison.unexpected {
            let d = &comparison.differences[index];
            let key = (d.kind, d.packet.as_str());
            match summary.iter_mut().find(|(k, _)| *k == key) {
                Some((_, count)) => *count += 1,
                None => summary.push((key, 1)),
            }
        }
        out.push_str("| Unexpected | Packet | Count |\n|---|---|---:|\n");
        for ((kind, packet), count) in summary {
            writeln!(out, "| {kind:?} | {packet} | {count} |")?;
        }
        out.push('\n');
    }
    writeln!(
        out,
        "## Unexpected differences ({})\n",
        comparison.unexpected.len()
    )?;
    for &index in comparison.unexpected.iter().take(SHOWN_DIFFERENCES) {
        describe(&mut out, index)?;
    }
    if comparison.unexpected.len() > SHOWN_DIFFERENCES {
        writeln!(
            out,
            "- … {} more",
            comparison.unexpected.len() - SHOWN_DIFFERENCES
        )?;
    }
    out.push('\n');
    let mut shown = Vec::new();
    for &index in &comparison.unexpected {
        let d = &comparison.differences[index];
        let pair = (d.bds, d.unastar);
        if shown.contains(&pair)
            || shown.len() >= SIDE_BY_SIDE
            || pair.0.is_none()
            || pair.1.is_none()
        {
            continue;
        }
        shown.push(pair);
        let name = &d.packet;
        writeln!(
            out,
            "### {name}: BDS seq {} / unastar seq {}\n",
            d.bds
                .map_or("-".into(), |i| bds.compared[i].packet.seq.to_string()),
            d.unastar
                .map_or("-".into(), |j| run.compared[j].packet.seq.to_string())
        )?;
        side_by_side(
            &mut out,
            d.bds.map(|i| &bds.compared[i].packet.tree),
            d.unastar.map(|j| &run.compared[j].packet.tree),
        );
    }
    for (title, list, entries) in [
        (
            "Known failing",
            &comparison.known_failing,
            &manifest.known_failing,
        ),
        ("Accepted", &comparison.accepted, &manifest.accepted),
    ] {
        if entries.is_empty() {
            continue;
        }
        writeln!(out, "## {title} ({})\n", list.len())?;
        for entry in entries {
            let count = list
                .iter()
                .filter(|&&i| entry.matches(&comparison.differences[i]))
                .count();
            writeln!(
                out,
                "- {:?} {} {} ×{count}{}: {}",
                entry.kind,
                entry.packet.as_deref().unwrap_or("*"),
                entry.path.as_deref().unwrap_or(""),
                entry
                    .owner
                    .as_deref()
                    .map(|owner| format!(" (owner: {owner})"))
                    .unwrap_or_default(),
                entry.reason
            )?;
        }
        out.push('\n');
    }
    out.push_str("## Alignment\n\n| BDS seq | +ms | unastar seq | +ms | packet | after |\n|---:|---:|---:|---:|---|---|\n");
    for &(left, right) in rows {
        let a = left.map(|i| &bds.compared[i]);
        let b = right.map(|j| &run.compared[j]);
        let any = a.or(b).context("empty alignment row")?;
        writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} |",
            a.map_or("-".into(), |c| c.packet.seq.to_string()),
            a.map_or("-".into(), |c| format!("{:.0}", c.after)),
            b.map_or("-".into(), |c| c.packet.seq.to_string()),
            b.map_or("-".into(), |c| format!("{:.0}", c.after)),
            any.packet.name,
            step_label(script, &steps, any.step)
        )?;
    }
    Ok(out)
}

/// Options of [`retain`].
pub struct Retain<'a> {
    /// The bot capture (`runs/<run>/<dir>/<scenario>-vanilla-go.jsonl`).
    pub run: &'a Path,
    pub scenario: &'a str,
    pub steps: usize,
    /// Output directory (`crates/client-diff/scenarios/<name>`).
    pub dir: &'a Path,
    pub description: &'a str,
    /// Leading steps that only set the scenario up (see [`Manifest::setup_steps`]).
    pub setup_steps: usize,
    pub setup_reason: &'a str,
    pub setup: &'a str,
    pub server: &'a str,
    pub recorded: &'a str,
}

/// Cuts the window of the first `steps` steps out of a bot capture and
/// writes `recording.jsonl` plus a manifest with the default packet types
/// and masks. The window runs from the first step packet to the step packet
/// of the first step left out (or the end of the capture).
pub fn retain(file: &ScenariosFile, options: &Retain<'_>) -> Result<()> {
    let scenario = file.scenario(options.scenario)?;
    let script = scenario.prefix(options.steps)?;
    let text = fs::read_to_string(options.run)
        .with_context(|| format!("reading {}", options.run.display()))?;
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let records = crate::parse(&text, &options.run.display().to_string())?;
    let packets = records.iter().map(decode).collect::<Result<Vec<_>>>()?;
    let (game_mode, difficulty, game_rules) = joined_world(&packets)?;
    let enchantment_seed = packets
        .iter()
        .find(|p| p.name == "StartGame")
        .and_then(|p| p.tree.field("enchantment_seed"))
        .map(Node::inline)
        .context("StartGame has no enchantment_seed")?
        .parse::<i32>()?;
    let (runtime_id, unique_id) = packets
        .iter()
        .find_map(|p| {
            (p.name == "StartGame").then(|| {
                let id = |field: &str, inner: &str| {
                    p.tree
                        .field(field)
                        .and_then(|n| n.field(inner))
                        .map(Node::inline)
                };
                Some((
                    id("runtime_id", "actor_runtime_id")?.parse::<u64>().ok()?,
                    id("entity_id", "actor_unique_id")?.parse::<i64>().ok()?,
                ))
            })
        })
        .flatten()
        .context("the capture has no StartGame")?;
    let expected: Vec<&str> = scenario
        .steps
        .iter()
        .filter_map(|step| step.kind.marker())
        .collect();
    let markers: Vec<usize> = packets
        .iter()
        .enumerate()
        .filter(|(_, p)| p.c2s && is_marker(&p.name))
        .map(|(i, _)| i)
        .collect();
    let included = script
        .steps
        .iter()
        .filter(|s| s.kind.marker().is_some())
        .count();
    ensure!(included > 0, "the retained steps send no step packet");
    // The retained steps and the first one left out must line up.
    let checked = markers.len().min(expected.len()).min(included + 1);
    let found: Vec<&str> = markers[..checked]
        .iter()
        .map(|&i| packets[i].name.as_str())
        .collect();
    ensure!(
        found.as_slice() == &expected[..checked] && markers.len() >= included,
        "the capture's step packets do not follow the scenario (a lenient step was skipped or a window was refused):\n  expected {expected:?}\n  found    {found:?}"
    );
    let first = markers[0];
    let end = if options.steps < scenario.steps.len() {
        ensure!(
            scenario.steps[options.steps].kind.marker().is_some(),
            "step {} ({}) sends no packet, so the window cannot end there; retain one step more or less",
            options.steps,
            scenario.steps[options.steps].kind.name()
        );
        *markers
            .get(included)
            .context("the capture ends before the first step left out")?
    } else {
        packets.len()
    };
    let mut kept = String::new();
    for (packet, line) in packets[first..end].iter().zip(&lines[first..end]) {
        if !NOT_RETAINED.contains(&packet.name.as_str()) {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    let window_ms = packets.get(end).map_or_else(|| packets[end - 1].t, |p| p.t) - packets[first].t;
    let manifest = Manifest {
        scenario: options.scenario.into(),
        steps: options.steps,
        steps_sha256: steps_sha256(&script.steps)?,
        description: options.description.into(),
        setup_steps: options.setup_steps,
        setup_reason: options.setup_reason.into(),
        source: Source {
            run: options.run.display().to_string().replace('\\', "/"),
            server: options.server.into(),
            protocol: 2193,
            recorded: options.recorded.into(),
            player_runtime_id: runtime_id,
            player_unique_id: unique_id,
            player_enchantment_seed: enchantment_seed,
            window_ms,
            recording_sha256: sha256_hex(kept.as_bytes()),
        },
        world: World {
            kind: WorldKind::Superflat,
            game_mode,
            difficulty,
            game_rules,
            setup: options.setup.into(),
        },
        compare: Compare {
            packets: DEFAULT_PACKETS.iter().map(|&p| p.to_owned()).collect(),
            timing_tolerance_ms: 120.0,
            float_tolerance: 0.001,
        },
        masks: default_masks(),
        known_failing: Vec::new(),
        accepted: Vec::new(),
    };
    let manifest_path = options.dir.join("manifest.toml");
    if manifest_path.exists() {
        bail!(
            "{} exists; remove the scenario directory to retain it again",
            manifest_path.display()
        );
    }
    fs::create_dir_all(options.dir)?;
    fs::write(options.dir.join("recording.jsonl"), kept)?;
    fs::write(manifest_path, toml::to_string_pretty(&manifest)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(text: &str) -> Node {
        Node::parse(text).unwrap()
    }

    #[test]
    fn masks_relabel_ids_by_first_appearance_and_select_by_key() {
        let masks: Vec<_> = default_masks()
            .into_iter()
            .map(|m| {
                let path = m.path.as_deref().map(|p| parse_path(p).unwrap());
                (m, path)
            })
            .collect();
        let mut canon = Canon::new(1, -8589934591);
        let mut packet = Packet {
            seq: 0,
            t: 0.0,
            name: "SetActorData".into(),
            c2s: false,
            tree: node(
                "SetActorDataPacket { target_runtime_id: ActorRuntimeId { actor_runtime_id: 7 }, actor_data: SynchedActorDataCopyableDataList { data: [DataItemEntry { id: 5, payload: DataItemInt64Payload(DataItemInt64Payload { value: -8589934591 }) }, DataItemEntry { id: 4, payload: DataItemInt64Payload(DataItemInt64Payload { value: 99 }) }] }, tick: PlayerInputTick { inputtick: 1234 } }",
            ),
        };
        normalize(&mut packet, &masks, &mut canon);
        assert_eq!(
            packet.tree.inline(),
            "SetActorDataPacket { target_runtime_id: ActorRuntimeId { actor_runtime_id: runtime#1 }, actor_data: SynchedActorDataCopyableDataList { data: [DataItemEntry { id: 5, payload: DataItemInt64Payload(DataItemInt64Payload { value: self }) }, DataItemEntry { id: 4, payload: DataItemInt64Payload(DataItemInt64Payload { value: 99 }) }] }, tick: PlayerInputTick { inputtick: <masked> } }"
        );
        assert_eq!(
            key(&packet),
            "SetActorData@ActorRuntimeId { actor_runtime_id: runtime#1 }"
        );
    }

    #[test]
    fn keyed_lists_and_floats_compare_by_entry_and_tolerance() {
        let a = node("P { data: [E { id: 0, v: 1.0 }, E { id: 5, v: 2 }], pos: V { x: 0.5 } }");
        let b = node(
            "P { data: [E { id: 5, v: 3 }, E { id: 0, v: 1.0004 }, E { id: 9, v: 1 }], pos: V { x: 0.5009 } }",
        );
        let mut out = Vec::new();
        diff_trees(&a, &b, "", 0.001, &mut out);
        assert_eq!(
            out,
            vec![
                ("data[id=5].v".into(), "2".into(), "3".into()),
                (
                    "data[id=9]".into(),
                    "<absent>".into(),
                    "E { id: 9, v: 1 }".into()
                ),
            ]
        );
    }

    #[test]
    fn expectations_match_paths_below_them() {
        let expectation = Expectation {
            kind: DiffKind::Field,
            packet: Some("SetActorData".into()),
            path: Some("actor_data.data[id=0]".into()),
            owner: None,
            reason: String::new(),
        };
        let difference = |path: &str| Difference {
            kind: DiffKind::Field,
            packet: "SetActorData".into(),
            path: path.into(),
            detail: String::new(),
            bds: None,
            unastar: None,
        };
        assert!(expectation.matches(&difference("actor_data.data[id=0]")));
        assert!(expectation.matches(&difference("actor_data.data[id=0].payload")));
        assert!(!expectation.matches(&difference("actor_data.data[id=01]")));
    }

    /// Every retained scenario loads, its recording matches its hash and its
    /// steps still match the shared script.
    #[test]
    fn retained_scenarios_are_consistent() {
        let file = ScenariosFile::load(&shared_script()).unwrap();
        let cases = Case::all().unwrap();
        assert!(!cases.is_empty(), "no retained scenarios");
        for case in &cases {
            case.script(&file).unwrap();
        }
    }

    /// A recording compared with itself has no differences: the comparison
    /// is deterministic and its masks are consistent.
    #[test]
    fn retained_recordings_match_themselves() {
        let file = ScenariosFile::load(&shared_script()).unwrap();
        for case in Case::all().unwrap() {
            let script = case.script(&file).unwrap();
            let side = Side {
                records: &case.recording,
                runtime_id: case.manifest.source.player_runtime_id,
                unique_id: case.manifest.source.player_unique_id,
            };
            let mut manifest_free = Case {
                name: case.name.clone(),
                dir: case.dir.clone(),
                manifest: case.manifest.clone(),
                recording: case.recording.clone(),
            };
            manifest_free.manifest.known_failing.clear();
            manifest_free.manifest.accepted.clear();
            let comparison = compare(&manifest_free, &script, &side, &[]).unwrap();
            assert!(comparison.passed(), "{}", comparison.report);
        }
    }
}
