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

use glam::IVec3;
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
    let path = format!(
        "{}/../unastar-data/data/measured/redstone/{family}.json",
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

/// A world a circuit row replays in: the in-memory [`Cell`] or the live
/// server world ([`LiveBench`]).
trait Bench: BlockSource {
    /// Set `state` at `pos` from outside the circuit, with the block update
    /// a placement triggers when `physics`.
    fn set(&mut self, pos: IVec3, state: u32, physics: bool);
    /// One game tick of the circuit.
    fn step(&mut self);
    /// `replaceitem block`: `count` items in `slot` of the container at
    /// `pos` (`None` empties it).
    fn replace_item(&mut self, pos: IVec3, slot: usize, count: Option<u32>) -> Result<(), Skip>;
}

impl Bench for Cell {
    fn set(&mut self, pos: IVec3, state: u32, physics: bool) {
        Cell::set(self, pos, state, physics);
    }

    fn step(&mut self) {
        Cell::step(self);
    }

    fn replace_item(&mut self, pos: IVec3, slot: usize, count: Option<u32>) -> Result<(), Skip> {
        let slots = self.containers.entry(pos).or_default();
        match count {
            Some(count) => slots.insert(slot, (count, 64)),
            None => slots.remove(&slot),
        };
        Ok(())
    }
}

/// The live server world: blocks go through the game's block path
/// ([`set_block`](crate::server::game::blocks::set_block)) and the circuit
/// steps through [`super::tick_redstone`], so its own writes take the same
/// path a player's would. The row's cell sits at `ORIGIN` on a superflat
/// floor in chunk (0, 0).
struct LiveBench(bevy_ecs::world::World);

impl LiveBench {
    const ORIGIN: IVec3 = IVec3::new(4, 64, 4);

    fn new(redstone: Redstone) -> Self {
        use crate::world::ecs::{BlockBroadcastEvent, ChunkData, ChunkManager, ChunkPosition};
        let mut world = bevy_ecs::world::World::new();
        world.insert_resource(ChunkManager::new(
            crate::world::WorldConfig::void_spawn_platform(1),
        ));
        world.insert_resource(crate::server::game::types::BlockRegistryResource(
            std::sync::Arc::new(registry().clone()),
        ));
        world.insert_resource(bevy_ecs::message::Messages::<BlockBroadcastEvent>::default());
        let mut chunk = crate::world::chunk::Chunk::new(0, 0);
        for x in 0..16u8 {
            for z in 0..16u8 {
                for y in -4..0 {
                    if let Some(name) = Cell::floor(y) {
                        let id = registry().default_state_id_by_name(name).unwrap();
                        chunk.set_block(x, (Self::ORIGIN.y + y) as i16, z, id);
                    }
                }
            }
        }
        let chunk = world.spawn(ChunkData::new(chunk)).id();
        world
            .resource_mut::<ChunkManager>()
            .insert(ChunkPosition::new(0, 0), chunk);
        world.insert_resource(redstone);
        Self(world)
    }
}

impl BlockSource for LiveBench {
    fn block_at(&self, pos: IVec3) -> Option<u32> {
        let at = pos + Self::ORIGIN;
        crate::server::game::blocks::block_at(&self.0, (at.x, at.y, at.z))
    }

    fn registry(&self) -> &BlockRegistry {
        registry()
    }
}

impl Bench for LiveBench {
    fn set(&mut self, pos: IVec3, state: u32, physics: bool) {
        use crate::server::game::blocks::{set_block, write_block};
        let at = pos + Self::ORIGIN;
        if physics {
            set_block(&mut self.0, at, state, false);
        } else {
            write_block(&mut self.0, at, state, false);
        }
    }

    fn step(&mut self) {
        super::tick_redstone(&mut self.0);
    }

    fn replace_item(&mut self, _: IVec3, _: usize, _: Option<u32>) -> Result<(), Skip> {
        Err("containers are not modelled in the live bench".to_string())
    }
}

/// Apply one scenario action to the bench; `Err` when the replay cannot.
fn apply(cell: &mut impl Bench, stored: &Stored, event: &Value) -> Result<(), Skip> {
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
            if text.starts_with("tp ") {
                return Ok(());
            }
            let words: Vec<&str> = text.split_whitespace().collect();
            if words.len() == 9 && words[0] == "replaceitem" && words[1] == "block" {
                let pos = offset(&event[2]);
                let slot: usize = words[6].parse().map_err(|_| text.to_string())?;
                let count: u32 = words[8].parse().map_err(|_| text.to_string())?;
                let count = (words[7] != "minecraft:air").then_some(count);
                return cell.replace_item(pos, slot, count);
            }
            return Err(format!("command `{text}` is not modelled"));
        }
        // Clicks are applied from the trace (see `switches`).
        "use" => {}
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

/// Replay one row in the in-memory [`Cell`], comparing every watched
/// position but `ignored`; `Ok(Err(skip))` for rows it cannot run.
fn replay(row: &Value, ignored: &[usize]) -> Result<Result<(), Skip>, String> {
    replay_in(row, ignored, Cell::new)
}

/// [`replay`] in the bench `bench` makes from the circuit.
fn replay_in<B: Bench>(
    row: &Value,
    ignored: &[usize],
    bench: impl FnOnce(Redstone) -> B,
) -> Result<Result<(), Skip>, String> {
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
    let mut cell = bench(Redstone::starting_at(base - waits - 1));
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
                            && let Err(skip) = apply(&mut cell, &stored, &event[1])
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
                    cell.step();
                }
            }
            _ => {
                if let Err(skip) = apply(&mut cell, &stored, step) {
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
    if has("\"tnt\"") {
        return Some("primed TNT is an actor unastar cannot spawn yet");
    }
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
    if has("\"piston\"") || has("\"sticky_piston\"") {
        return Some("pistons do not move blocks yet");
    }
    if has("\"torch_facing_direction\":\"unknown\"") {
        return Some("BDS turns a torch set with the unknown facing into a standing one");
    }
    None
}

/// Observer cells (family, row) where BDS pulsed a tick earlier than every
/// other cell of the same kind (one tick after the change instead of three;
/// 5 of 38 measured observer cells, no cause found in the circuit or the
/// cell). The same rows of the other family pulse on time.
const EARLY_OBSERVERS: &[(&str, &str)] = &[
    ("redstone_observers", r#"["lamp",4]"#),
    ("redstone_observers", r#"["repeater",4]"#),
    ("redstone_observer_timing", r#"["golden_rail",4]"#),
    ("redstone_observer_timing", r#"["dispenser",4]"#),
    ("redstone_observer_timing", r#"["torch",4]"#),
];

/// Replay every row of `family`; panics listing the rows that differ.
fn check_family(family: &str) {
    check_family_ignoring(family, &[]);
}

/// [`check_family`] without comparing the watched positions `ignored`.
fn check_family_ignoring(family: &str, ignored: &[usize]) {
    let mut failures = Vec::new();
    let mut skipped = Vec::new();
    let all = rows(family);
    for row in &all {
        let name = &row[0];
        if let Some(reason) = skip_reason(name) {
            skipped.push(format!("{name}: {reason}"));
            continue;
        }
        if EARLY_OBSERVERS.contains(&(family, name.to_string().as_str())) {
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

/// The burnout rows through the live server world: the circuit's own writes
/// (a torch turning off) take the game's block path and must not count as
/// outside changes, which would recover the burnt-out torch next tick.
#[test]
fn torch_burnout_runs_like_bds_in_the_live_world() {
    let torches = rows("redstone_torches");
    let burnout = rows("redstone_burnout");
    let picked: Vec<&Value> = torches
        .iter()
        .filter(|row| {
            row[0][0]
                .as_str()
                .is_some_and(|name| name.starts_with("burnout"))
        })
        .chain(&burnout)
        .collect();
    assert!(picked.len() >= 2);
    for row in picked {
        match replay_in(row, &[], LiveBench::new) {
            Ok(Ok(())) => {}
            Ok(Err(skip)) => panic!("{}: skipped: {skip}", row[0]),
            Err(message) => panic!("{}: {message}", row[0]),
        }
    }
}

/// A circuit the world had before the server knew it (loaded from disk) is
/// found from a change next to it, through a solid block: a lever on stone
/// lights the lamp on the far side.
#[test]
fn a_loaded_circuit_is_found_through_a_solid_block() {
    let lever = |on: bool| {
        state(
            "lever",
            &serde_json::json!({"lever_direction": "west", "open_bit": on}),
        )
    };
    let lamp = IVec3::new(2, 0, 0);
    let mut cell = Cell::new(Redstone::starting_at(1000));
    // In the world before the circuit started: not changes it saw.
    cell.blocks.insert(IVec3::ZERO, lever(false));
    cell.blocks
        .insert(IVec3::X, state("stone", &serde_json::json!({})));
    cell.blocks
        .insert(lamp, state("redstone_lamp", &serde_json::json!({})));
    for _ in 0..4 {
        cell.step();
    }
    cell.set(IVec3::ZERO, lever(true), true);
    for _ in 0..4 {
        cell.step();
    }
    assert_eq!(
        describe(cell.block_at(lamp).unwrap()).0,
        "lit_redstone_lamp"
    );
}
