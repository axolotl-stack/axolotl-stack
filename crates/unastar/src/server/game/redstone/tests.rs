//! Replays of the BDS 1.26.51 redstone circuits
//! (`crates/unastar-data/data/measured/redstone/`): every circuit is built in
//! an in-memory world with the same placements and block updates, run for
//! the same timeline with the same changes at the same ticks, and every
//! watched block is compared with what BDS stored at every tick.
//!
//! Alignment: BDS works redstone every other tick; the row's `witness` says
//! which reads show an update, and the replay starts the circuit's tick
//! counter so its updates fall on the same ticks. Clicks on levers and
//! buttons reach BDS a few ticks after the order; the replay applies each
//! switch change at the tick the trace shows it (a button's release a tick
//! earlier, as BDS releases it inside the tick before the read).

use std::collections::HashMap;

use glam::{DVec3, IVec3};
use serde_json::{Map, Value};

use super::{Circuit, Redstone, container_reading};
use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::registry::block::StateValue;
use crate::server::game::block_update::{UpdateTarget, changed};

fn registry() -> &'static BlockRegistry {
    BlockRegistry::vanilla()
}

fn air() -> u32 {
    registry()
        .default_state_id_by_name("minecraft:air")
        .unwrap()
}

/// A superflat cell: grass at y = -1, dirt below, bedrock at -4.
struct Cell {
    blocks: HashMap<IVec3, u32>,
    redstone: Option<Redstone>,
    /// Outside changes made while the circuit steps (block updates of its
    /// own changes).
    deferred: Vec<(IVec3, u32)>,
    /// Container contents: slot -> (count, max stack).
    containers: HashMap<IVec3, HashMap<usize, (u32, u32)>>,
}

impl Cell {
    fn new(redstone: Redstone) -> Self {
        Self {
            blocks: HashMap::new(),
            redstone: Some(redstone),
            deferred: Vec::new(),
            containers: HashMap::new(),
        }
    }

    fn floor(y: i32) -> Option<&'static str> {
        match y {
            -1 => Some("minecraft:grass_block"),
            -2 | -3 => Some("minecraft:dirt"),
            -4 => Some("minecraft:bedrock"),
            _ => None,
        }
    }

    /// Set `state` at `pos` from outside the circuit, with the block update
    /// a placement triggers when `physics`.
    fn set(&mut self, pos: IVec3, state: u32, physics: bool) {
        UpdateTarget::write(self, pos, state);
        if physics {
            changed(self, pos, true);
        }
    }

    /// Change the block at `pos` from outside the circuit, with the block
    /// updates of a change (not a placement).
    fn change(&mut self, pos: IVec3, state: u32) {
        UpdateTarget::write(self, pos, state);
        changed(self, pos, false);
    }

    fn step(&mut self) {
        // The block updates of the circuit's changes of the last tick.
        let written = self.redstone.as_mut().expect("circuit").take_written();
        for pos in written {
            changed(self, pos, false);
        }
        let mut redstone = self.redstone.take().expect("circuit");
        redstone.step(registry(), self);
        for (pos, old) in std::mem::take(&mut self.deferred) {
            redstone.outside_change(pos, old);
        }
        self.redstone = Some(redstone);
    }
}

impl BlockSource for Cell {
    fn block_at(&self, pos: IVec3) -> Option<u32> {
        Some(self.blocks.get(&pos).copied().unwrap_or_else(|| {
            Self::floor(pos.y)
                .and_then(|name| registry().default_state_id_by_name(name))
                .unwrap_or_else(air)
        }))
    }

    fn registry(&self) -> &BlockRegistry {
        registry()
    }
}

impl UpdateTarget for Cell {
    fn write(&mut self, pos: IVec3, state: u32) {
        let old = self.block_at(pos).unwrap_or_else(air);
        self.blocks.insert(pos, state);
        if old != state {
            match self.redstone.as_mut() {
                Some(redstone) => redstone.outside_change(pos, old),
                None => self.deferred.push((pos, old)),
            }
        }
    }

    fn pop(&mut self, pos: IVec3, _state: u32) {
        UpdateTarget::write(self, pos, air());
    }
}

impl Circuit for Cell {
    fn write(&mut self, pos: IVec3, state: u32) {
        self.blocks.insert(pos, state);
    }

    fn break_block(&mut self, pos: IVec3) {
        self.blocks.insert(pos, air());
    }

    fn container_signal(&mut self, pos: IVec3) -> Option<u8> {
        let state = self.block_at(pos)?;
        let entry = registry().get_by_runtime_id(state)?;
        let slots = match entry.string_id.as_str() {
            "minecraft:chest" | "minecraft:trapped_chest" | "minecraft:barrel" => 27,
            "minecraft:hopper" => 5,
            "minecraft:dispenser" | "minecraft:dropper" => 9,
            "minecraft:furnace" | "minecraft:blast_furnace" | "minecraft:smoker" => 3,
            _ => return None,
        };
        let contents = self.containers.get(&pos);
        Some(container_reading(
            slots,
            contents
                .into_iter()
                .flat_map(|slots| slots.values().copied()),
        ))
    }
}

/// The rows of a measured circuit family.
pub(crate) fn rows(family: &str) -> Vec<Value> {
    rows_in("redstone", family)
}

/// The rows of a measured circuit family under `data/measured/<dir>/`.
pub(crate) fn rows_in(dir: &str, family: &str) -> Vec<Value> {
    let path = format!(
        "{}/../unastar-data/data/measured/{dir}/{family}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut value: Value = serde_json::from_str(&text).expect(&path);
    match value["rows"].take() {
        Value::Array(rows) => rows,
        other => panic!("{path}: rows {other:?}"),
    }
}

fn full_name(name: &str) -> String {
    if name.contains(':') {
        name.to_string()
    } else {
        format!("minecraft:{name}")
    }
}

/// The runtime id of `block` with the measured `states`.
fn state(block: &str, states: &Value) -> u32 {
    let name = full_name(block);
    let entry = registry().get_by_name(&name).expect(&name);
    let mut id = entry.default_state_id;
    for (property, value) in states.as_object().expect("states") {
        let value = match value {
            Value::Bool(b) => StateValue::Byte(u8::from(*b)),
            Value::Number(n) => StateValue::Int(n.as_i64().unwrap() as i32),
            Value::String(s) => StateValue::Str(Box::leak(s.clone().into_boxed_str())),
            other => panic!("{name} {property} {other:?}"),
        };
        id = entry
            .with_state_value(id, property, value)
            .unwrap_or_else(|| panic!("{name} has no {property}={value:?}"));
    }
    id
}

/// `(short name, states)` of a runtime id as the fixtures write them.
fn describe(state: u32) -> (String, Map<String, Value>) {
    let entry = registry().get_by_runtime_id(state).unwrap();
    let mut states = Map::new();
    for property in entry.data.properties {
        let value = match entry.state_value(state, property.name).unwrap() {
            StateValue::Byte(b) => Value::Bool(b != 0),
            StateValue::Int(i) => Value::from(i),
            StateValue::Str(s) => Value::from(s),
        };
        states.insert(property.name.to_string(), value);
    }
    (
        entry
            .string_id
            .strip_prefix("minecraft:")
            .unwrap_or(&entry.string_id)
            .to_string(),
        states,
    )
}

fn offset(value: &Value) -> IVec3 {
    let v = value.as_array().expect("offset");
    IVec3::new(
        v[0].as_i64().unwrap() as i32,
        v[1].as_i64().unwrap() as i32,
        v[2].as_i64().unwrap() as i32,
    )
}

/// Why a row cannot be replayed (the world it needs is not modelled here).
type Skip = String;

/// What BDS stored at each watched position at tick 0, for blocks set with
/// states BDS normalises (a chest set without states faces north, a
/// chiseled bookshelf's books come from its empty actor, a torch's `unknown`
/// facing becomes `top`).
type Stored = HashMap<IVec3, (String, Value)>;

/// The state to set for `block` with `states` at `pos`: what BDS stored
/// there when it is the same block and not part of a circuit (a circuit's
/// own state settles in the replay).
fn placed(stored: &Stored, pos: IVec3, block: &str, states: &Value) -> u32 {
    let requested = state(block, states);
    let Some((stored_block, stored_states)) = stored.get(&pos) else {
        return requested;
    };
    if full_name(stored_block) != full_name(block) {
        return requested;
    }
    static TABLE: std::sync::LazyLock<super::kinds::Table> =
        std::sync::LazyLock::new(|| super::kinds::Table::build(registry()));
    if TABLE.get(requested).kind != super::kinds::Kind::None {
        return requested;
    }
    state(stored_block, stored_states)
}

/// Apply one scenario action to the cell; `Err` when the replay cannot.
/// With `recorded_actors` the actors a command or drop adds or removes come
/// from the row's actor records instead.
fn apply(
    cell: &mut Cell,
    stored: &Stored,
    event: &Value,
    recorded_actors: bool,
) -> Result<(), Skip> {
    let kind = event[0].as_str().unwrap();
    match kind {
        "set" => {
            let pos = offset(&event[1]);
            let physics = event[4].as_bool().unwrap_or(true);
            cell.set(pos, state(event[2].as_str().unwrap(), &event[3]), physics);
        }
        "build" => {
            for entry in event[1].as_array().unwrap() {
                let pos = offset(&entry[0]);
                let physics = entry[3].as_bool().unwrap_or(true);
                let placed = placed(stored, pos, entry[1].as_str().unwrap(), &entry[2]);
                cell.set(pos, placed, physics);
            }
        }
        "cmd" => {
            let text = event[1].as_str().unwrap();
            if text.starts_with("tp ")
                || recorded_actors
                    && (text.starts_with("summon ")
                        || text.starts_with("kill ")
                        || text.starts_with("execute as @e"))
            {
                return Ok(());
            }
            let words: Vec<&str> = text.split_whitespace().collect();
            if words.len() == 9 && words[0] == "replaceitem" && words[1] == "block" {
                let pos = offset(&event[2]);
                let slot: usize = words[6].parse().map_err(|_| text.to_string())?;
                let count: u32 = words[8].parse().map_err(|_| text.to_string())?;
                let slots = cell.containers.entry(pos).or_default();
                if words[7] == "minecraft:air" {
                    slots.remove(&slot);
                } else {
                    slots.insert(slot, (count, 64));
                }
                return Ok(());
            }
            return Err(format!("command `{text}` is not modelled"));
        }
        // Clicks are applied from the trace (see `switches`).
        "use" => {}
        "drop" if recorded_actors => {}
        other => return Err(format!("event `{other}` is not modelled")),
    }
    Ok(())
}

/// Switch changes to apply from the trace: (tick, before the step of that
/// tick instead of at its start, watch index, state).
fn switches(events: &[Value], watch: &[IVec3], trace: &[Value]) -> Vec<(u64, bool, usize, u32)> {
    let mut clicked = Vec::new();
    for event in events {
        if event[1][0].as_str() == Some("use") {
            let pos = offset(&event[1][1]);
            if let Some(k) = watch.iter().position(|w| *w == pos)
                && !clicked.contains(&k)
            {
                clicked.push(k);
            }
        }
    }
    let mut out = Vec::new();
    for entry in trace {
        let t = entry[0].as_u64().unwrap();
        let k = entry[1].as_u64().unwrap() as usize;
        if t == 0 || !clicked.contains(&k) {
            continue;
        }
        let block = entry[2].as_str().unwrap();
        let next = state(block, &entry[3]);
        // A button's release happens inside the tick before the read.
        let release =
            block.ends_with("_button") && entry[3]["button_pressed_bit"] == Value::Bool(false);
        if release {
            out.push((t - 1, true, k, next));
        } else {
            out.push((t, false, k, next));
        }
    }
    out
}

/// Replay one row, comparing every watched position but `ignored`;
/// `Ok(Err(skip))` for rows the in-memory world cannot run.
fn replay(row: &Value, ignored: &[usize]) -> Result<Result<(), Skip>, String> {
    replay_with(row, ignored, None)
}

/// What the recorded actors do in tick `t`: called before the cell steps
/// (`false`) and after (`true`).
type Pressing<'a> = &'a mut dyn FnMut(&mut Cell, u64, bool);

/// [`replay`], with `pressing` run every timeline tick around the step (the
/// actors of the row's records pressing string or pushed by pistons; their
/// commands and drops are then not modelled as such).
fn replay_with(
    row: &Value,
    ignored: &[usize],
    mut pressing: Option<Pressing>,
) -> Result<Result<(), Skip>, String> {
    let recorded_actors = pressing.is_some();
    let witness = row[1].as_u64().unwrap();
    let steps = row[2].as_array().unwrap();
    let trace = row[3].as_array().unwrap();
    let mut stored = Stored::new();
    // Dust climbing: BDS pops the dust on top of some blocks before the
    // circuit runs; that is the block's support, not redstone.
    if row[0][0] == "climb"
        && trace
            .iter()
            .any(|entry| entry[1].as_u64() == Some(3) && entry[2] != "redstone_wire")
    {
        return Ok(Err(
            "BDS does not keep dust on this block (the block itself falls off or changes)"
                .to_string(),
        ));
    }
    if let Some(timeline) = steps.iter().find(|step| step[0] == "timeline") {
        let watch: Vec<IVec3> = timeline[3].as_array().unwrap().iter().map(offset).collect();
        for entry in trace {
            if entry[0].as_u64() == Some(0) {
                let k = entry[1].as_u64().unwrap() as usize;
                stored.insert(
                    watch[k],
                    (entry[2].as_str().unwrap().to_string(), entry[3].clone()),
                );
            }
        }
    }
    let waits: u64 = steps
        .iter()
        .filter(|step| step[0] == "wait")
        .map(|step| step[1].as_u64().unwrap())
        .sum();
    // Timeline tick i is step `base + i`; BDS shows an update at the reads
    // whose parity is the witness's, so tick i updates when i + 1 - witness
    // is even.
    let base = 1000 + (witness + 1) % 2;
    let mut cell = Cell::new(Redstone::starting_at(base - waits - 1));
    for step in steps {
        match step[0].as_str().unwrap() {
            "wait" => {
                for _ in 0..step[1].as_u64().unwrap() {
                    cell.step();
                }
            }
            "timeline" => {
                let ticks = step[1].as_u64().unwrap();
                let events = step[2].as_array().unwrap();
                let watch: Vec<IVec3> = step[3].as_array().unwrap().iter().map(offset).collect();
                let switches = switches(events, &watch, trace);
                let mut expected: Vec<Option<(String, Value)>> = vec![None; watch.len()];
                let mut entries = trace.iter().peekable();
                for t in 0..ticks {
                    for event in events {
                        if event[0].as_u64() == Some(t)
                            && let Err(skip) = apply(&mut cell, &stored, &event[1], recorded_actors)
                        {
                            return Ok(Err(skip));
                        }
                    }
                    for (at, before_step, k, state) in &switches {
                        if *at == t && !before_step {
                            cell.set(watch[*k], *state, true);
                        }
                    }
                    while let Some(entry) = entries.peek() {
                        if entry[0].as_u64() != Some(t) {
                            break;
                        }
                        let k = entry[1].as_u64().unwrap() as usize;
                        expected[k] =
                            Some((entry[2].as_str().unwrap().to_string(), entry[3].clone()));
                        entries.next();
                    }
                    for (k, pos) in watch.iter().enumerate() {
                        if ignored.contains(&k) {
                            continue;
                        }
                        let Some((block, states)) = &expected[k] else {
                            continue;
                        };
                        let (got, got_states) = describe(cell.block_at(*pos).unwrap());
                        if got != *block || Value::Object(got_states.clone()) != *states {
                            return Err(format!(
                                "tick {t}, watch {k} at {pos}: BDS has {block}{states}, unastar {got}{}",
                                Value::Object(got_states)
                            ));
                        }
                    }
                    for (at, before_step, k, state) in &switches {
                        if *at == t && *before_step {
                            cell.set(watch[*k], *state, true);
                        }
                    }
                    if let Some(pressing) = pressing.as_mut() {
                        pressing(&mut cell, t, false);
                    }
                    cell.step();
                    if let Some(pressing) = pressing.as_mut() {
                        pressing(&mut cell, t, true);
                    }
                }
            }
            _ => {
                if let Err(skip) = apply(&mut cell, &stored, step, recorded_actors) {
                    return Ok(Err(skip));
                }
            }
        }
    }
    Ok(Ok(()))
}

/// Rows the replay skips on purpose, with the reason: the circuit is not
/// what differs, the block it is built around does something of its own.
fn skip_reason(name: &Value) -> Option<&'static str> {
    let text = name.to_string();
    let has = |part: &str| text.contains(part);
    if has("daylight_detector") {
        return Some("daylight detectors do not measure daylight yet");
    }
    if has("calibrated_sculk_sensor") || has("\"sculk_sensor\"") {
        return Some("sculk sensors do not react to power or vibrations yet");
    }
    if has("\"composter\"") && has("\"composter_fill_level\":7") {
        return Some("a composter at level 7 turning ready (8) is not modelled");
    }
    if has("\"cut\",\"bell\"") {
        return Some(
            "BDS keeps a standing bell set on dust until the dust changes (a bell set on a fence post drops)",
        );
    }
    if has("\"cut\",\"big_dripleaf\"") {
        return Some(
            "a big dripleaf on dust is not modelled (it stays, tilts and drops on its own)",
        );
    }
    if has("[\"lever_on_top\"]") {
        return Some(
            "Endstone set_data on a lever does not change its power (the replay's lever does)",
        );
    }
    if text.starts_with("[\"block\",") {
        for (prefix, reason) in PISTON_BLOCK_SKIPS {
            if text.starts_with(&format!("[\"block\",\"{prefix}")) {
                return Some(reason);
            }
        }
        if (has("_wall_sign\"") || has("\"wall_sign\"")) && has("\"facing_direction\":0") {
            return Some(SUPPORT_SKIP);
        }
    }
    if has("\"torch_facing_direction\":\"unknown\"") {
        return Some("BDS turns a torch set with the unknown facing into a standing one");
    }
    None
}

const SUPPORT_SKIP: &str = "unastar's support rules treat this default state differently when it is set (not a piston rule)";

/// `pistons_blocks` rows whose block does something of its own in the cell
/// before or after the piston moves it.
const PISTON_BLOCK_SKIPS: &[(&str, &str)] = &[
    ("ladder", SUPPORT_SKIP),
    ("lever", SUPPORT_SKIP),
    ("hanging_roots", SUPPORT_SKIP),
    ("spore_blossom", SUPPORT_SKIP),
    ("pale_hanging_moss", SUPPORT_SKIP),
    ("weeping_vines", SUPPORT_SKIP),
    ("amethyst_cluster", SUPPORT_SKIP),
    ("large_amethyst_bud", SUPPORT_SKIP),
    ("medium_amethyst_bud", SUPPORT_SKIP),
    ("small_amethyst_bud", SUPPORT_SKIP),
    ("chalkboard", SUPPORT_SKIP),
    ("small_dripleaf_block", SUPPORT_SKIP),
    ("dead_brain_coral_wall_fan", SUPPORT_SKIP),
    ("dead_bubble_coral_wall_fan", SUPPORT_SKIP),
    ("dead_fire_coral_wall_fan", SUPPORT_SKIP),
    ("dead_horn_coral_wall_fan", SUPPORT_SKIP),
    ("dead_tube_coral_wall_fan", SUPPORT_SKIP),
    (
        "border_block",
        "unastar connects a border wall to the piston beside it; BDS does not (not a piston rule)",
    ),
    (
        "cave_vines",
        "BDS gives a cave vine set by the sweep a random age",
    ),
    (
        "large_fern",
        "Endstone sets the upper half of a double plant with the lower one",
    ),
    (
        "lilac",
        "Endstone sets the upper half of a double plant with the lower one",
    ),
    (
        "peony",
        "Endstone sets the upper half of a double plant with the lower one",
    ),
    (
        "pitcher_plant",
        "Endstone sets the upper half of a double plant with the lower one",
    ),
    (
        "rose_bush",
        "Endstone sets the upper half of a double plant with the lower one",
    ),
    (
        "sunflower",
        "Endstone sets the upper half of a double plant with the lower one",
    ),
    (
        "tall_grass",
        "Endstone sets the upper half of a double plant with the lower one",
    ),
    (
        "brain_coral_block",
        "a live coral block dies 45 ticks after it was set (block_updates/coral_timing.json), moved or not; the circuit replay does not run coral checks",
    ),
    (
        "bubble_coral_block",
        "a live coral block dies 45 ticks after it was set; the circuit replay does not run coral checks",
    ),
    (
        "fire_coral_block",
        "a live coral block dies 45 ticks after it was set; the circuit replay does not run coral checks",
    ),
    (
        "horn_coral_block",
        "a live coral block dies 45 ticks after it was set; the circuit replay does not run coral checks",
    ),
    (
        "tube_coral_block",
        "a live coral block dies 45 ticks after it was set; the circuit replay does not run coral checks",
    ),
];

/// Observer cells where BDS pulsed a tick earlier than every other cell of
/// the same kind (one tick after the change instead of three; 7 of 38
/// measured observer cells, no cause found in the circuit or the cell).
const EARLY_OBSERVERS: &[&str] = &[
    r#"["lamp",4]"#,
    r#"["repeater",4]"#,
    r#"["golden_rail",4]"#,
    r#"["dispenser",4]"#,
    r#"["torch",4]"#,
];

/// Replay every row of `family`; panics listing the rows that differ.
fn check_family(family: &str) {
    check_family_ignoring(family, &[]);
}

/// [`check_family`] without comparing the watched positions `ignored`.
fn check_family_ignoring(family: &str, ignored: &[usize]) {
    check_rows(family, rows(family), ignored);
}

/// Replay `all` (the rows of `family`), comparing every watched position
/// but `ignored`; panics listing the rows that differ.
pub(crate) fn check_rows(family: &str, all: Vec<Value>, ignored: &[usize]) {
    let mut failures = Vec::new();
    let mut skipped = Vec::new();
    for row in &all {
        let name = &row[0];
        if let Some(reason) = skip_reason(name) {
            skipped.push(format!("{name}: {reason}"));
            continue;
        }
        if family.contains("observer") && EARLY_OBSERVERS.contains(&name.to_string().as_str()) {
            skipped.push(format!("{name}: BDS pulsed this observer a tick early"));
            continue;
        }
        match replay(row, ignored) {
            Ok(Ok(())) => {}
            Ok(Err(skip)) => skipped.push(format!("{name}: {skip}")),
            Err(message) => failures.push(format!("{name}: {message}")),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} {family} circuits differ from BDS (skipped {}):\n{}",
        failures.len(),
        all.len(),
        skipped.len(),
        failures.join("\n")
    );
    assert!(
        skipped.len() <= all.len() / 4 + 2,
        "{family}: too many skipped rows:\n{}",
        skipped.join("\n")
    );
}

#[test]
fn dust_follows_bds_tick_by_tick() {
    check_family("redstone_dust");
}

#[test]
fn power_through_blocks_follows_bds() {
    check_family("redstone_power");
}

#[test]
fn repeaters_follow_bds_tick_by_tick() {
    check_family("redstone_repeaters");
}

#[test]
fn comparators_follow_bds_tick_by_tick() {
    check_family("redstone_comparators");
}

#[test]
fn torches_follow_bds_tick_by_tick() {
    check_family("redstone_torches");
}

#[test]
fn consumers_follow_bds_tick_by_tick() {
    check_family("redstone_consumers");
}

#[test]
fn observers_follow_bds_tick_by_tick() {
    check_family("redstone_observers");
    check_family("redstone_observer_timing");
}

#[test]
fn torch_clocks_burn_out_like_bds() {
    check_family("redstone_burnout");
}

/// The candidate block itself (watch 2) is not compared: what it does on
/// its own when set (falling off, dying, taking its facing) is not a
/// redstone rule.
#[test]
fn conductors_match_bds() {
    check_family_ignoring("redstone_conductors", &[2]);
}

#[test]
fn dust_climbs_and_is_cut_like_bds() {
    check_family_ignoring("redstone_dust_blocks", &[2]);
    check_family_ignoring("redstone_dust_cut", &[3]);
}

#[test]
fn buttons_and_levers_power_like_bds() {
    check_family("redstone_switches");
}

/// The rows of a measured piston family.
fn piston_rows(family: &str) -> Vec<Value> {
    rows_in("pistons", family)
}

#[test]
fn pistons_push_lines_like_bds() {
    check_rows("pistons_push", piston_rows("pistons_push"), &[]);
}

#[test]
fn pistons_treat_every_block_like_bds() {
    check_rows("pistons_blocks", piston_rows("pistons_blocks"), &[]);
}

#[test]
fn slime_and_honey_drag_blocks_like_bds() {
    check_rows("pistons_slime", piston_rows("pistons_slime"), &[]);
}

#[test]
fn pistons_are_powered_like_bds() {
    check_rows("pistons_power", piston_rows("pistons_power"), &[]);
}

#[test]
fn pistons_time_moves_like_bds() {
    check_rows("pistons_timing", piston_rows("pistons_timing"), &[]);
}

#[test]
fn pistons_push_pistons_like_bds() {
    check_rows("pistons_chains", piston_rows("pistons_chains"), &[]);
}

/// Blocks whose tool-less drops unastar gets wrong whether a piston breaks
/// them or a block update pops them (no vanilla loot table, so unastar drops
/// the block itself; a campfire drops charcoal or soul soil): a drop-table
/// gap, not a piston rule.
const POPPED_DROP_GAPS: &[&str] = &[
    "bush",
    "campfire",
    "chorus_plant",
    "orange_poplar_leaves",
    "poplar_standing_sign",
    "red_poplar_leaves",
    "resin_clump",
    "sculk_vein",
    "short_dry_grass",
    "soul_campfire",
    "suspicious_gravel",
    "suspicious_sand",
    "tall_dry_grass",
    "twisting_vines",
    "yellow_poplar_leaves",
];

/// A block a piston breaks drops what it drops by hand without the
/// correct-tool requirement, like a popped block: every item BDS dropped is
/// a possible drop, and blocks whose drops never vary drop exactly BDS's.
#[test]
fn blocks_pistons_break_drop_like_bds() {
    use bevy_ecs::prelude::World;

    use crate::entity::item_drop::EntityRandom;
    use crate::registry::ItemRegistry;
    use crate::server::game::block_drops::popped_drops;
    use crate::server::game::types::{BlockRegistryResource, ItemRegistryResource};

    let mut world = World::new();
    let mut items = ItemRegistry::new();
    items.load_vanilla();
    world.insert_resource(ItemRegistryResource(std::sync::Arc::new(items)));
    world.insert_resource(BlockRegistryResource(std::sync::Arc::new(
        registry().clone(),
    )));
    world.insert_resource(EntityRandom::seeded(4));
    let table = super::kinds::Table::build(registry());
    let mut failures = Vec::new();
    let mut compared = 0;
    for row in piston_rows("pistons_blocks") {
        let name = &row[0];
        if skip_reason(name).is_some() {
            continue;
        }
        let block = name[1].as_str().unwrap();
        if POPPED_DROP_GAPS.contains(&block) {
            continue;
        }
        let state = state(block, &name[2]);
        if table.get(state).movement != unastar_data::pistons::Movement::Breaks {
            continue;
        }
        let mut bds: Vec<(String, u64)> = row[4]
            .as_array()
            .unwrap()
            .iter()
            .map(|stack| {
                (
                    full_name(stack[0].as_str().unwrap()),
                    stack[2].as_u64().unwrap(),
                )
            })
            .collect();
        bds.sort();
        let rolls: Vec<Vec<(String, u64)>> = (0..32)
            .map(|_| {
                let mut out: Vec<(String, u64)> = popped_drops(&mut world, state)
                    .iter()
                    .map(|stack| (stack.item_id().to_string(), u64::from(stack.count)))
                    .collect();
                out.sort();
                out
            })
            .collect();
        compared += 1;
        let fixed = rolls.iter().all(|roll| *roll == rolls[0]);
        if fixed {
            if rolls[0] != bds {
                failures.push(format!(
                    "{name}: BDS dropped {bds:?}, unastar {:?}",
                    rolls[0]
                ));
            }
        } else if let Some((item, _)) = bds
            .iter()
            .find(|(item, _)| !rolls.iter().flatten().any(|(got, _)| got == item))
        {
            failures.push(format!("{name}: BDS dropped {item}, unastar never does"));
        }
    }
    assert!(compared > 200, "only {compared} broken blocks compared");
    assert!(
        failures.is_empty(),
        "{} of {compared} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The box an actor of the sweep presses plates with.
fn presser_size(kind: &str) -> Option<(f64, f64)> {
    Some(match kind {
        "armor_stand" => (0.5, 1.975),
        "pig" => (0.9, 0.9),
        "item" => (0.25, 0.25),
        _ => return None,
    })
}

/// The actors of a row's records (`[tick, [[id, type, dx, dy, dz, ...]]]`)
/// at read `read`: (id, presser).
fn actors_at(actors: &[Value], read: u64) -> Vec<(i64, super::plates::Presser)> {
    use super::plates::Presser;
    use crate::registry::collision::Aabb;
    actors
        .iter()
        .rev()
        .find(|entry| entry[0].as_u64().unwrap() <= read)
        .map(|entry| {
            entry[1]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|a| {
                    let (width, height) = presser_size(a[1].as_str()?)?;
                    let feet = DVec3::new(a[2].as_f64()?, a[3].as_f64()?, a[4].as_f64()?);
                    Some((
                        a[0].as_i64()?,
                        Presser {
                            aabb: Aabb::from_feet(feet, width, height),
                            item: a[1] == "item",
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The actors that press plates in tick `t`: those that existed when it
/// began (read `t`), where its movement left them (read `t + 1`); a mob
/// removed in the tick still counts where it was, an item merged into
/// another does not (items merge before plates read; a mob is removed
/// after).
fn pressers_in_tick(actors: &[Value], t: u64) -> Vec<super::plates::Presser> {
    let after = actors_at(actors, t + 1);
    actors_at(actors, t)
        .into_iter()
        .filter_map(
            |(id, presser)| match after.iter().find(|(other, _)| *other == id) {
                Some((_, moved)) => Some(*moved),
                None => (!presser.item).then_some(presser),
            },
        )
        .collect()
}

/// Every plate row replayed from the actors BDS had in the cell each tick:
/// the plate's stored signal matches at every read (wooden, stone,
/// blackstone and weighted plates under armour stands, pigs, items and
/// stacks; the weighted plates under 1-64 items as they merge).
#[test]
fn plates_press_like_bds_under_the_measured_actors() {
    use super::plates::{PlateKind, Plates};
    let mut failures = Vec::new();
    let mut compared = 0;
    for family in ["plates_press", "plates_weighted"] {
        for row in rows(family) {
            let name = &row[0];
            let plate = full_name(name[0].as_str().unwrap());
            let kind = PlateKind::of(&plate).expect("a plate");
            let pos = IVec3::new(1, 0, 0);
            let plate_at = |trace: &[Value], t: u64| {
                trace
                    .iter()
                    .rev()
                    .find(|e| e[1] == 1 && e[0].as_u64().unwrap() <= t)
                    .map_or(0, |e| e[3]["redstone_signal"].as_u64().unwrap_or(0) as u8)
            };
            let trace = row[3].as_array().unwrap();
            let actors = row[5].as_array().unwrap();
            let ticks = row[2]
                .as_array()
                .unwrap()
                .iter()
                .find(|step| step[0] == "timeline")
                .unwrap()[1]
                .as_u64()
                .unwrap();
            let mut plates = Plates::default();
            let mut stored = 0u8;
            for t in 0..ticks.saturating_sub(1) {
                let seen = pressers_in_tick(actors, t);
                for (_, signal) in plates.tick(t, &[(pos, kind, stored)], &seen) {
                    stored = signal;
                }
                compared += 1;
                let bds = plate_at(trace, t + 1);
                if bds != stored {
                    failures.push(format!(
                        "{name}: read {}: BDS {bds}, unastar {stored}",
                        t + 1
                    ));
                    break;
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} rows differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(compared > 3000, "{compared}");
}

/// Every tripwire row replayed block by block, tick by tick, with the
/// string pressed by the actors BDS had in the cell each tick: the hooks,
/// the string and the lamp a hook powers match BDS at every read (the line
/// attaching as it is built, an armour stand and items on the string, the
/// string cut).
#[test]
fn tripwires_follow_bds_under_the_measured_actors() {
    let mut replayed = 0;
    for family in ["plates_tripwire", "plates_tripwire_more"] {
        for row in rows(family) {
            let actors = row[5].as_array().unwrap().clone();
            let mut pressing = tripwire_pressing(actors);
            match replay_with(&row, &[], Some(&mut pressing)) {
                Ok(Ok(())) => replayed += 1,
                Ok(Err(skip)) => panic!("{}: {skip}", row[0]),
                Err(message) => panic!("{}: {message}", row[0]),
            }
        }
    }
    assert!(replayed >= 5, "{replayed}");
}

/// The string and hooks the recorded `actors` press each tick, through the
/// live server's `plates::press`.
fn tripwire_pressing(actors: Vec<Value>) -> impl FnMut(&mut Cell, u64, bool) {
    use super::plates::Plates;
    let mut plates = Plates::default();
    move |cell: &mut Cell, t: u64, stepped: bool| {
        if stepped {
            return;
        }
        let seen = pressers_in_tick(&actors, t);
        let candidates: Vec<IVec3> = cell.blocks.keys().copied().collect();
        super::plates::press(cell, &mut plates, t, candidates, &seen, Cell::change);
    }
}

/// Every piston row with actors replayed block by block, and every actor
/// BDS had in the cell moved by the replay's piston moves of each tick: what
/// it moved along the move's axis between two reads is the push or carry
/// plus its own motion (the velocity BDS read before or after the tick: a
/// landing item moves with the one before, within 0.02).
#[test]
fn pistons_move_actors_like_bds() {
    let mut compared = 0;
    for family in ["pistons_entities", "pistons_riding"] {
        for row in piston_rows(family) {
            let actors = row[5].as_array().unwrap().clone();
            // Ticks a command moves the actors itself (a `tp`).
            let moved_by_command: Vec<u64> = row[2]
                .as_array()
                .unwrap()
                .iter()
                .filter(|step| step[0] == "timeline")
                .flat_map(|step| step[2].as_array().unwrap().iter())
                .filter(|event| {
                    event[1][0] == "cmd"
                        && event[1][1]
                            .as_str()
                            .is_some_and(|text| text.contains(" run tp "))
                })
                .filter_map(|event| event[0].as_u64())
                .collect();
            let mut failures = Vec::new();
            let mut check = |cell: &mut Cell, t: u64, stepped: bool| {
                if !stepped {
                    return;
                }
                let pushes = cell.redstone.as_ref().expect("circuit").actor_pushes();
                let before = recorded_actors(&actors, t);
                for (id, kind, feet, velocity_before) in &before {
                    let Some((width, height)) = presser_size(kind) else {
                        continue;
                    };
                    let Some((_, _, after, velocity)) = recorded_actors(&actors, t + 1)
                        .into_iter()
                        .find(|(other, ..)| other == id)
                    else {
                        continue;
                    };
                    let aabb = crate::registry::collision::Aabb::from_feet(*feet, width, height);
                    let axis = pushes
                        .first()
                        .map_or(DVec3::X, |push| push.dir.abs().as_dvec3());
                    let moved = super::piston_actors::moved(&aabb, &pushes);
                    let own = (after - *feet - moved).dot(axis);
                    if moved_by_command.contains(&(t + 1)) {
                        continue;
                    }
                    compared += 1;
                    if [velocity, *velocity_before]
                        .iter()
                        .all(|v| (own - v.dot(axis)).abs() > 0.02)
                    {
                        failures.push(format!(
                            "read {}: {kind} from {feet} to {after}, unastar moves it {moved}",
                            t + 1
                        ));
                    }
                }
            };
            match replay_with(&row, &[], Some(&mut check)) {
                Ok(Ok(())) => {}
                Ok(Err(skip)) => panic!("{}: {skip}", row[0]),
                Err(message) => panic!("{}: {message}", row[0]),
            }
            assert!(
                failures.is_empty(),
                "{}: {}",
                row[0],
                failures.join(
                    "
"
                )
            );
        }
    }
    assert!(compared > 300, "{compared}");
}

/// The actors of a row's records at read `read`: (id, type, feet, velocity).
fn recorded_actors(actors: &[Value], read: u64) -> Vec<(i64, String, DVec3, DVec3)> {
    actors
        .iter()
        .rev()
        .find(|entry| entry[0].as_u64().unwrap() <= read)
        .map(|entry| {
            entry[1]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|a| {
                    let v = |i: usize| a[i].as_f64();
                    Some((
                        a[0].as_i64()?,
                        a[1].as_str()?.to_string(),
                        DVec3::new(v(2)?, v(3)?, v(4)?),
                        DVec3::new(v(5)?, v(6)?, v(7)?),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Why a block has no measured piston class: the per-block sweep could not
/// stand its default state in a cell. Such blocks break.
fn unmeasured_reason(id: &str) -> Option<&'static str> {
    let short = id.strip_prefix("minecraft:").unwrap_or(id);
    let water = short.ends_with("_coral_fan") && !short.starts_with("dead_")
        || short.ends_with("_coral_wall_fan") && !short.starts_with("dead_")
        || matches!(
            short,
            "kelp"
                | "seagrass"
                | "sea_pickle"
                | "underwater_torch"
                | "underwater_tnt"
                | "water"
                | "flowing_water"
                | "lava"
                | "flowing_lava"
                | "bubble_column"
        );
    if water {
        return Some("water plants and liquids flood the cells beside them");
    }
    if short.ends_with("_button")
        || matches!(
            short,
            "frame"
                | "glow_frame"
                | "chalkboard"
                | "small_dripleaf_block"
                | "wall_banner"
                | "tripwire_hook"
        )
    {
        return Some("the default state falls off when set");
    }
    if short == "air" || short == "moving_block" || short == "unknown" {
        return Some("not a block a piston meets as such");
    }
    if matches!(short, "fire" | "soul_fire" | "portal" | "end_gateway") {
        return Some(
            "not measured: the sweep's cell does not keep it (fire burns out, a portal needs its frame)",
        );
    }
    None
}

/// Every block without a measured piston class has a reason; the list
/// fails when a new block appears that the sweep has not measured.
#[test]
fn every_unmeasured_piston_block_has_a_reason() {
    let measured = unastar_data::pistons::blocks();
    let mut missing = Vec::new();
    for entry in registry().iter() {
        let id = entry.string_id.as_str();
        let rest = id.strip_prefix("minecraft:").unwrap_or(id);
        let base = ["lit_", "unlit_", "powered_", "unpowered_"]
            .iter()
            .find_map(|prefix| rest.strip_prefix(prefix))
            .unwrap_or(rest);
        let known = ["", "lit_", "unlit_", "powered_", "unpowered_"]
            .iter()
            .any(|prefix| measured.movement(&format!("{prefix}{base}")).is_some());
        if !known && unmeasured_reason(id).is_none() {
            missing.push(id.to_string());
        }
    }
    missing.sort();
    assert!(
        missing.is_empty(),
        "{} blocks have no piston class or reason: {missing:?}",
        missing.len()
    );
}
