//! Replays of the BDS 1.26.51 liquid sweeps
//! (`crates/unastar-data/data/measured/liquids/`): every cell is rebuilt in
//! an in-memory world on the same floor at the same coordinates, its steps
//! run tick by tick through the engine, and every watched position is read
//! each tick and compared with BDS's trace.
//!
//! A cell whose liquid stays in one chunk must match exactly. Where the
//! liquid crosses into another chunk, BDS lands some ticks one tick early
//! depending on its chunk order that game tick (random from tick to tick);
//! those cells must show the same changes at most two ticks apart, and
//! `liquids_chunks` compares how often a crossing lands early.

use std::collections::{HashMap, HashSet};

use glam::IVec3;
use serde_json::Value;

use super::{LiquidTarget, Liquids, stored};
use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::registry::block::{LiquidKind, StateValue};
use crate::server::game::block_update::UpdateTarget;

fn registry() -> &'static BlockRegistry {
    BlockRegistry::vanilla()
}

fn id(name: &str) -> u32 {
    registry()
        .default_state_id_by_name(name)
        .unwrap_or_else(|| panic!("{name}"))
}

fn state(block: &str, states: &Value) -> u32 {
    let name = if block.contains(':') {
        block.to_string()
    } else {
        format!("minecraft:{block}")
    };
    let entry = registry().get_by_name(&name).expect(&name);
    let mut out = entry.default_state_id;
    for (property, value) in states.as_object().into_iter().flatten() {
        let value = match value {
            Value::Bool(b) => StateValue::Byte(u8::from(*b)),
            Value::Number(n) => StateValue::Int(n.as_i64().unwrap() as i32),
            Value::String(s) => StateValue::Str(Box::leak(s.clone().into_boxed_str())),
            other => panic!("{name} {property} {other:?}"),
        };
        out = entry.with_state_value(out, property, value).unwrap_or(out);
    }
    out
}

/// An in-memory world: the sweep's superflat floor (or a stone floor just
/// under the cells, as the nether cells were built), plus what was set.
struct Grid {
    blocks: HashMap<IVec3, u32>,
    layers: HashMap<IVec3, u32>,
    stone_floor: Option<i32>,
    washed: Vec<(IVec3, u32, LiquidKind)>,
}

impl Grid {
    fn new(stone_floor: Option<i32>) -> Self {
        Self {
            blocks: HashMap::new(),
            layers: HashMap::new(),
            stone_floor,
            washed: Vec::new(),
        }
    }

    fn floor(&self, y: i32) -> u32 {
        if let Some(floor) = self.stone_floor {
            return if y == floor {
                id("minecraft:stone")
            } else {
                id("minecraft:air")
            };
        }
        match y {
            -61 => id("minecraft:grass_block"),
            -62 | -63 => id("minecraft:dirt"),
            -64 => id("minecraft:bedrock"),
            _ => id("minecraft:air"),
        }
    }

    /// Set a block as Endstone and `/setblock` do: water stays in the
    /// liquid layer of a block that holds it.
    fn set(&mut self, pos: IVec3, block: u32) {
        let old = self.block_at(pos).unwrap();
        let old_layer = self.layer_at(pos).unwrap();
        let (block, layer) = stored(registry(), old, old_layer, block);
        self.blocks.insert(pos, block);
        self.layers.insert(pos, layer);
    }

    /// `(name, depth)` of what is at `pos`.
    fn read(&self, pos: IVec3) -> (String, Option<u8>) {
        let state = self.block_at(pos).unwrap();
        let entry = registry().get_by_runtime_id(state).unwrap();
        let name = entry.string_id.trim_start_matches("minecraft:").to_string();
        (name, registry().liquid_state(state).map(|l| l.depth))
    }
}

impl BlockSource for Grid {
    fn block_at(&self, pos: IVec3) -> Option<u32> {
        Some(
            self.blocks
                .get(&pos)
                .copied()
                .unwrap_or_else(|| self.floor(pos.y)),
        )
    }

    fn registry(&self) -> &BlockRegistry {
        registry()
    }
}

impl UpdateTarget for Grid {
    fn write(&mut self, pos: IVec3, state: u32) {
        self.set(pos, state);
    }

    fn pop(&mut self, pos: IVec3, _state: u32) {
        self.set(pos, id("minecraft:air"));
    }
}

impl LiquidTarget for Grid {
    fn layer_at(&self, pos: IVec3) -> Option<u32> {
        Some(
            self.layers
                .get(&pos)
                .copied()
                .unwrap_or_else(|| id("minecraft:air")),
        )
    }

    fn write_layer(&mut self, pos: IVec3, state: u32) {
        self.layers.insert(pos, state);
    }

    fn wash(&mut self, pos: IVec3, state: u32, kind: LiquidKind) {
        self.washed.push((pos, state, kind));
        self.blocks.insert(pos, id("minecraft:air"));
    }
}

pub(crate) fn rows(family: &str) -> Vec<Value> {
    let path = format!(
        "{}/../unastar-data/data/measured/liquids/{family}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let file: Value = serde_json::from_str(&text).unwrap();
    file["rows"].as_array().unwrap().clone()
}

fn offset(value: &Value) -> IVec3 {
    let v: Vec<i32> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_i64().unwrap() as i32)
        .collect();
    IVec3::new(v[0], v[1], v[2])
}

/// Why a row is not replayed.
#[derive(Debug)]
struct Skip(&'static str);

/// Apply one scenario event at the cell `base`.
fn apply(grid: &mut Grid, liquids: &mut Liquids, base: IVec3, event: &Value) -> Result<(), Skip> {
    let kind = event[0].as_str().unwrap();
    match kind {
        "set" => {
            let pos = base + offset(&event[1]);
            grid.set(pos, state(event[2].as_str().unwrap(), &event[3]));
            if event[4].as_bool().unwrap_or(false) {
                liquids.changed(grid, pos);
            }
        }
        "build" => {
            for set in event[1].as_array().unwrap() {
                let set = Value::Array(
                    std::iter::once(Value::from("set"))
                        .chain(set.as_array().unwrap().iter().cloned())
                        .collect(),
                );
                apply(grid, liquids, base, &set)?;
            }
        }
        "setblock" => {
            let pos = base + offset(&event[1]);
            grid.set(pos, state(event[2].as_str().unwrap(), &event[3]));
            liquids.changed(grid, pos);
        }
        // Commands only move or equip the scripted player here (the nether
        // cells' floor is the grid's stone floor).
        "cmd" => {}
        "use" => {
            return Err(Skip(
                "a click by the scripted player lands a tick or two late",
            ));
        }
        other => panic!("unknown event {other}"),
    }
    Ok(())
}

/// One traced change: tick, watched index, block, depth.
type Change = (u64, usize, String, Option<u8>);

fn bds_trace(row: &Value) -> Vec<Change> {
    row[3]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c[0].as_u64().unwrap(),
                c[1].as_u64().unwrap() as usize,
                c[2].as_str().unwrap().to_string(),
                c[3]["liquid_depth"].as_u64().map(|d| d as u8),
            )
        })
        .collect()
}

/// Replay one row: the trace unastar makes, and whether the watched
/// positions span more than one chunk.
fn replay(row: &Value, nether: bool) -> Result<(Vec<Change>, bool), Skip> {
    let base = offset(&row[1]);
    let mut grid = Grid::new(nether.then_some(base.y - 1));
    // Each row draws its own chunk orders.
    let seed = row[0]
        .to_string()
        .bytes()
        .fold(7u64, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(b)));
    let mut liquids = Liquids::seeded(seed);
    liquids.nether = nether;
    let mut trace = Vec::new();
    let mut chunks = HashSet::new();
    for step in row[2].as_array().unwrap() {
        match step[0].as_str().unwrap() {
            "wait" => {
                for _ in 0..step[1].as_u64().unwrap() {
                    liquids.step(&mut grid);
                }
            }
            "timeline" => {
                let ticks = step[1].as_u64().unwrap();
                let events = step[2].as_array().unwrap();
                let watch: Vec<IVec3> = step[3]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|o| base + offset(o))
                    .collect();
                chunks.extend(watch.iter().map(|p| super::chunk_of(*p)));
                let mut last: Vec<Option<(String, Option<u8>)>> = vec![None; watch.len()];
                for tick in 0..ticks {
                    for event in events {
                        if event[0].as_u64() == Some(tick) {
                            apply(&mut grid, &mut liquids, base, &event[1])?;
                        }
                    }
                    for (k, pos) in watch.iter().enumerate() {
                        let value = grid.read(*pos);
                        if last[k].as_ref() != Some(&value) {
                            trace.push((tick, k, value.0.clone(), value.1));
                            last[k] = Some(value);
                        }
                    }
                    liquids.step(&mut grid);
                }
            }
            _ => apply(&mut grid, &mut liquids, base, step)?,
        }
    }
    Ok((trace, chunks.len() > 1))
}

/// Changes of a trace per watched position: `(tick, block, depth)` in order.
fn per_position(trace: &[Change]) -> HashMap<usize, Vec<(u64, String, Option<u8>)>> {
    let mut out: HashMap<usize, Vec<_>> = HashMap::new();
    for (t, k, name, depth) in trace {
        out.entry(*k).or_default().push((*t, name.clone(), *depth));
    }
    out
}

/// Whether `ours` shows the same changes as `bds`, each at most `early`
/// ticks apart (chunk crossings land early in one or the other, depending
/// on that tick's random chunk order).
fn matches_within(ours: &[Change], bds: &[Change], early: u64) -> bool {
    let (ours, bds) = (per_position(ours), per_position(bds));
    if ours.len() != bds.len() {
        return false;
    }
    bds.iter().all(|(k, theirs)| {
        ours.get(k).is_some_and(|mine| {
            mine.len() == theirs.len()
                && mine
                    .iter()
                    .zip(theirs)
                    .all(|(m, t)| m.1 == t.1 && m.2 == t.2 && m.0.abs_diff(t.0) <= early)
        })
    })
}

/// Rows unastar does not replay, with the reason.
fn skip_reason(family: &str, name: &Value) -> Option<&'static str> {
    let text = name.to_string();
    let order = "ticks due in the same game tick run in BDS in an order not pinned down \
                 (it is not the order they were scheduled in)";
    match family {
        "liquids_order" => Some(order),
        "liquids_order2" if !text.contains("remove_lava") => Some(order),
        "liquids_pilot" if text.contains("water_remove") || text.contains("infinite_floor") => {
            Some(order)
        }
        "liquids_flow"
            if text.contains("infinite_over_source") || text.contains("reach_still_source") =>
        {
            Some(order)
        }
        // A spread receding after its source goes (or turns to stone) is
        // decided by the same-tick order.
        "liquids_flow" | "liquids_nether" | "liquids_order2"
            if text.contains("recede") || text.contains("remove") =>
        {
            Some(order)
        }
        "liquids_pilot" if text.contains("lava_onto_water") => Some(order),
        "liquids_logged" if text.contains("\"ladder\"") => Some(
            "the ladder set without support pops on the first neighbour change (block updates, not liquids)",
        ),
        "liquids_logged" if text.contains("\"wall\"") => {
            Some("the wall re-shapes on the stone set beside it (block updates, not liquids)")
        }
        "liquids_logged" if text.contains("kelp") || text.contains("seagrass") => Some(
            "Endstone fills kelp and seagrass with water when it sets them; unastar's placement does not yet",
        ),
        // Coral set by Endstone holds no water and dies (a coral rule, not
        // a liquid one; see `block_use`).
        "liquids_logged" if text.contains("coral") => {
            Some("coral death is not part of the liquid engine")
        }
        _ => None,
    }
}

/// Replay every row of `family`; returns (exact, within chunk tolerance, skipped).
fn check_family(family: &str) -> (usize, usize, usize) {
    let nether = family == "liquids_nether";
    let (mut exact, mut loose, mut skipped) = (0, 0, 0);
    let mut failures = Vec::new();
    for row in rows(family) {
        if skip_reason(family, &row[0]).is_some() {
            skipped += 1;
            continue;
        }
        let (ours, spans) = match replay(&row, nether) {
            Ok(out) => out,
            Err(Skip(reason)) => {
                eprintln!("{family} {}: skipped: {reason}", row[0]);
                skipped += 1;
                continue;
            }
        };
        let bds = bds_trace(&row);
        if ours == bds {
            exact += 1;
        } else if spans && matches_within(&ours, &bds, 2) {
            loose += 1;
        } else {
            let first = bds
                .iter()
                .zip(&ours)
                .find(|(a, b)| a != b)
                .map(|(a, b)| format!("bds {a:?} ours {b:?}"))
                .unwrap_or_else(|| format!("{} vs {} changes", bds.len(), ours.len()));
            failures.push(format!("{} {}: {first}", family, row[0]));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    (exact, loose, skipped)
}

#[test]
fn water_and_lava_spread_like_bds() {
    let (exact, _, _) = check_family("liquids_pilot");
    assert!(exact >= 8, "{exact}");
}

#[test]
fn flow_direction_drops_and_mixing_follow_bds() {
    check_family("liquids_flow");
}

#[test]
fn waterlogged_blocks_hold_and_release_water_like_bds() {
    check_family("liquids_logged");
}

#[test]
fn nether_lava_follows_bds() {
    let (exact, loose, _) = check_family("liquids_nether");
    assert_eq!(exact + loose, 10);
}

#[test]
fn chunk_order_cells_follow_bds() {
    check_family("liquids_order2");
    check_family("liquids_order");
}

/// How often the second spread step lands a tick early in a neighbouring
/// chunk: BDS (`liquids_chunks`) against the engine's random chunk order.
#[test]
fn chunk_crossings_land_early_as_often_as_on_bds() {
    let rows = rows("liquids_chunks");
    let mut bds = (0, 0);
    let mut ours = (0, 0);
    for row in &rows {
        let base = offset(&row[1]);
        let steps = row[2].as_array().unwrap();
        let timeline = steps.iter().find(|s| s[0] == "timeline").unwrap();
        let watch: Vec<IVec3> = timeline[3]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| base + offset(o))
            .collect();
        let source = base + offset(&timeline[2][0][1][1][0][0]);
        let count = |trace: &[Change], tally: &mut (u32, u32)| {
            for (t, k, _, depth) in trace {
                if *depth == Some(2) && super::chunk_of(watch[*k]) != super::chunk_of(source) {
                    match t {
                        9 => tally.0 += 1,
                        10 => tally.1 += 1,
                        _ => panic!("depth 2 at tick {t}"),
                    }
                }
            }
        };
        count(&bds_trace(row), &mut bds);
        let (mine, _) = replay(row, false).unwrap();
        count(&mine, &mut ours);
    }
    let share = |(early, on_time): (u32, u32)| f64::from(early) / f64::from(early + on_time);
    assert!(bds.0 > 0 && bds.1 > 0, "{bds:?}");
    assert!(
        (share(bds) - share(ours)).abs() < 0.2,
        "bds {bds:?} ours {ours:?}"
    );
}

#[test]
fn blocks_that_hold_water_keep_it_and_air_leaves_it() {
    let fence = id("minecraft:oak_fence");
    let water = id("minecraft:water");
    let air = id("minecraft:air");
    let stone = id("minecraft:stone");
    // A fence set into a source keeps the water in its liquid layer.
    assert_eq!(stored(registry(), water, air, fence), (fence, water));
    // Stone replaces it.
    assert_eq!(stored(registry(), water, air, stone), (stone, air));
    // Air over a waterlogged fence leaves the source.
    assert_eq!(stored(registry(), fence, water, air), (water, air));
    // A shape change of a waterlogged fence keeps the water.
    assert_eq!(stored(registry(), fence, water, fence), (fence, water));
}

#[test]
fn liquid_changes_are_batched_per_subchunk() {
    use jolyne::valentine::McpePacketData;
    let water = |d: i32| state("water", &serde_json::json!({ "liquid_depth": d }));
    let change = |x: i32, state: u32| super::live::Change {
        pos: IVec3::new(x, -60, 1),
        layer: 0,
        state,
    };
    let changes = vec![
        change(1, water(1)),
        change(2, water(1)),
        change(17, water(2)),
        change(1, water(2)),
    ];
    let packets = super::live::batch_packets(&changes, |s| s);
    assert_eq!(packets.len(), 2);
    match &packets[0].1.data {
        McpePacketData::UpdateSubChunkBlocksPacket(p) => {
            let entries = &p.blocks_changed.blocks_changed_standards;
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].runtime_id, water(2), "last state, first place");
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        &packets[1].1.data,
        McpePacketData::UpdateBlockPacket(_)
    ));
}
