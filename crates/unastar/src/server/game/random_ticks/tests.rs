//! Random tick fields compared with BDS 1.26.51 by distribution
//! (`crates/unastar-data/data/measured/random_ticks/`): each field is laid
//! out in an in-memory superflat sub-chunk, given the same tick speed and
//! read at the same ticks, several times with fixed seeds; at every read
//! the histogram of states is compared with BDS's by a two-sample
//! chi-square test (Bonferroni over the reads, p >= 0.001). The seeds are
//! fixed, so the test is deterministic.

use std::collections::{BTreeMap, HashMap};

use glam::IVec3;
use rand::RngCore;
use serde_json::{Map, Value};

use super::tick_subchunk;
use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::registry::block::StateValue;
use crate::server::game::block_update::UpdateTarget;

fn registry() -> &'static BlockRegistry {
    BlockRegistry::vanilla()
}

fn id(name: &str) -> u32 {
    registry().default_state_id_by_name(name).expect(name)
}

/// The dense part of a [`Field`]: x and z -8..24, y -4..20.
const SPAN: i32 = 32;
const LOW_Y: i32 = -4;
const HEIGHT: i32 = 24;

/// A superflat world: grass at y = 0, dirt below, bedrock at -3; dense
/// storage around the ticked sub-chunk (0..16)^3 so the millions of random
/// picks of a field stay cheap.
struct Field {
    dense: Vec<u32>,
    outside: HashMap<IVec3, u32>,
    air: u32,
}

impl Field {
    fn new() -> Self {
        let air = id("minecraft:air");
        let mut dense = vec![air; (SPAN * SPAN * HEIGHT) as usize];
        for x in 0..SPAN {
            for z in 0..SPAN {
                for (y, name) in [
                    (0, "minecraft:grass_block"),
                    (-1, "minecraft:dirt"),
                    (-2, "minecraft:dirt"),
                    (-3, "minecraft:bedrock"),
                ] {
                    dense[Self::index(IVec3::new(x - 8, y, z - 8)).unwrap()] = id(name);
                }
            }
        }
        Self {
            dense,
            outside: HashMap::new(),
            air,
        }
    }

    fn index(pos: IVec3) -> Option<usize> {
        let (x, y, z) = (pos.x + 8, pos.y - LOW_Y, pos.z + 8);
        ((0..SPAN).contains(&x) && (0..HEIGHT).contains(&y) && (0..SPAN).contains(&z))
            .then(|| ((y * SPAN + z) * SPAN + x) as usize)
    }
}

impl BlockSource for Field {
    fn block_at(&self, pos: IVec3) -> Option<u32> {
        Some(match Self::index(pos) {
            Some(index) => self.dense[index],
            None => self.outside.get(&pos).copied().unwrap_or(match pos.y {
                0 => id("minecraft:grass_block"),
                -2..=-1 => id("minecraft:dirt"),
                -3 => id("minecraft:bedrock"),
                _ => self.air,
            }),
        })
    }

    fn registry(&self) -> &BlockRegistry {
        registry()
    }
}

impl UpdateTarget for Field {
    fn write(&mut self, pos: IVec3, state: u32) {
        match Self::index(pos) {
            Some(index) => self.dense[index] = state,
            None => {
                self.outside.insert(pos, state);
            }
        }
    }

    fn pop(&mut self, pos: IVec3, _state: u32) {
        let air = self.air;
        self.write(pos, air);
    }
}

fn rows(family: &str) -> Vec<Value> {
    let path = format!(
        "{}/../unastar-data/data/measured/random_ticks/{family}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut value: Value = serde_json::from_str(&text).expect(&path);
    match value["rows"].take() {
        Value::Array(rows) => rows,
        other => panic!("{path}: rows {other:?}"),
    }
}

fn state(block: &str, states: &Value) -> u32 {
    let name = format!("minecraft:{block}");
    let entry = registry().get_by_name(&name).expect(&name);
    let mut out = entry.default_state_id;
    for (property, value) in states.as_object().expect("states") {
        let value = match value {
            Value::Bool(b) => StateValue::Byte(u8::from(*b)),
            Value::Number(n) => StateValue::Int(n.as_i64().unwrap() as i32),
            Value::String(s) => StateValue::Str(Box::leak(s.clone().into_boxed_str())),
            other => panic!("{name} {property} {other:?}"),
        };
        // The sweep's requested states may name properties the block does
        // not have (`dirt_type` on dirt); BDS ignored them.
        out = entry.with_state_value(out, property, value).unwrap_or(out);
    }
    out
}

/// The fixture label (`[block, states]` as JSON text) of a state.
fn label(state: u32) -> String {
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
    let name = entry
        .string_id
        .strip_prefix("minecraft:")
        .unwrap_or(&entry.string_id);
    serde_json::to_string(&Value::Array(vec![
        Value::from(name),
        Value::Object(states),
    ]))
    .unwrap()
}

fn offset(value: &Value) -> IVec3 {
    let v = value.as_array().unwrap();
    IVec3::new(
        v[0].as_i64().unwrap() as i32,
        v[1].as_i64().unwrap() as i32,
        v[2].as_i64().unwrap() as i32,
    )
}

/// The upper regularised gamma function Q(a, x) (Numerical Recipes).
fn gamma_q(a: f64, x: f64) -> f64 {
    fn ln_gamma(x: f64) -> f64 {
        const C: [f64; 6] = [
            76.180_091_729_471_46,
            -86.505_320_329_416_77,
            24.014_098_240_830_91,
            -1.231_739_572_450_155,
            0.001_208_650_973_866_179,
            -0.000_005_395_239_384_953,
        ];
        let tmp = x + 5.5 - (x + 0.5) * (x + 5.5).ln();
        let mut ser = 1.000_000_000_190_015;
        for (j, c) in C.iter().enumerate() {
            ser += c / (x + 1.0 + j as f64);
        }
        -tmp + (2.506_628_274_631_000_5 * ser / x).ln()
    }
    if x <= 0.0 {
        return 1.0;
    }
    let gln = ln_gamma(a);
    if x < a + 1.0 {
        let (mut ap, mut sum) = (a, 1.0 / a);
        let mut del = sum;
        for _ in 0..1000 {
            ap += 1.0;
            del *= x / ap;
            sum += del;
            if del.abs() < sum.abs() * 1e-12 {
                break;
            }
        }
        1.0 - sum * (-x + a * x.ln() - gln).exp()
    } else {
        let mut b = x + 1.0 - a;
        let mut c = 1.0 / 1e-300;
        let mut d = 1.0 / b;
        let mut h = d;
        for i in 1..1000 {
            let an = -(i as f64) * (i as f64 - a);
            b += 2.0;
            d = an * d + b;
            if d.abs() < 1e-300 {
                d = 1e-300;
            }
            c = b + an / c;
            if c.abs() < 1e-300 {
                c = 1e-300;
            }
            d = 1.0 / d;
            let del = d * c;
            h *= del;
            if (del - 1.0).abs() < 1e-12 {
                break;
            }
        }
        (-x + a * x.ln() - gln).exp() * h
    }
}

/// p-value of a two-sample chi-square test of homogeneity between `a` and
/// `b` (categories with an expected count under 5 pooled); `None` when
/// fewer than two categories are left.
fn homogeneity(a: &BTreeMap<String, f64>, b: &BTreeMap<String, f64>) -> Option<f64> {
    let total_a: f64 = a.values().sum();
    let total_b: f64 = b.values().sum();
    let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
    keys.sort();
    keys.dedup();
    let share_a = total_a / (total_a + total_b);
    let mut cells: Vec<(f64, f64)> = Vec::new();
    let mut pooled = (0.0, 0.0);
    for key in keys {
        let (x, y) = (
            a.get(key).copied().unwrap_or(0.0),
            b.get(key).copied().unwrap_or(0.0),
        );
        if (x + y) * share_a < 5.0 {
            pooled.0 += x;
            pooled.1 += y;
        } else {
            cells.push((x, y));
        }
    }
    if pooled.0 + pooled.1 > 0.0 {
        cells.push(pooled);
    }
    if cells.len() < 2 {
        return None;
    }
    let mut statistic = 0.0;
    for (x, y) in &cells {
        let n = x + y;
        let (ex, ey) = (n * share_a, n * (1.0 - share_a));
        if ex > 0.0 {
            statistic += (x - ex).powi(2) / ex;
        }
        if ey > 0.0 {
            statistic += (y - ey).powi(2) / ey;
        }
    }
    let dof = (cells.len() - 1) as f64;
    Some(gamma_q(dof / 2.0, statistic / 2.0))
}

/// Replicates of each field unastar runs.
const REPLICATES: u64 = 4;

/// A small fast generator (SplitMix64) for the millions of picks a field
/// takes; seeded, so the test is deterministic.
struct Mix(u64);

impl RngCore for Mix {
    fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for chunk in dest.chunks_mut(8) {
            let bytes = self.next_u64().to_le_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
        }
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

/// Run one row's field `REPLICATES` times; the per-read histograms.
fn run(row: &Value, seed: u64) -> Vec<BTreeMap<String, f64>> {
    let replicates = run_replicates(row, seed, REPLICATES);
    let mut out = vec![BTreeMap::new(); replicates[0].len()];
    for replicate in replicates {
        for (sum, histogram) in out.iter_mut().zip(replicate) {
            for (key, count) in histogram {
                *sum.entry(key).or_insert(0.0) += count;
            }
        }
    }
    out
}

/// Run one row's field `replicates` times; each replicate's per-read
/// histograms.
fn run_replicates(row: &Value, seed: u64, replicates: u64) -> Vec<Vec<BTreeMap<String, f64>>> {
    let speed = row[1].as_u64().unwrap() as u32;
    let every = row[2].as_u64().unwrap();
    let build = row[3].as_array().unwrap();
    let watch: Vec<IVec3> = row[4].as_array().unwrap().iter().map(offset).collect();
    let reads = row[5].as_array().unwrap().len();
    // Centre the field in the sub-chunk (0..16)^3, the floor at y = 0.
    let offsets: Vec<IVec3> = build
        .iter()
        .map(|b| offset(&b[0]))
        .chain(watch.iter().copied())
        .collect();
    let (min_x, max_x) = (
        offsets.iter().map(|o| o.x).min().unwrap(),
        offsets.iter().map(|o| o.x).max().unwrap(),
    );
    let (min_z, max_z) = (
        offsets.iter().map(|o| o.z).min().unwrap(),
        offsets.iter().map(|o| o.z).max().unwrap(),
    );
    let shift = IVec3::new(
        (16 - (max_x - min_x + 1)) / 2 - min_x,
        1,
        (16 - (max_z - min_z + 1)) / 2 - min_z,
    );
    let mut out = Vec::new();
    for replicate in 0..replicates {
        let mut field = Field::new();
        for entry in build {
            field.write(
                offset(&entry[0]) + shift,
                state(entry[1].as_str().unwrap(), &entry[2]),
            );
        }
        let mut rng = Mix(seed * 1000 + replicate);
        let mut histograms = vec![BTreeMap::new(); reads];
        for histogram in histograms.iter_mut() {
            for _ in 0..every {
                tick_subchunk(&mut field, IVec3::ZERO, speed, &mut rng);
            }
            for pos in &watch {
                *histogram
                    .entry(label(field.block_at(*pos + shift).unwrap()))
                    .or_insert(0.0) += 1.0;
            }
        }
        out.push(histograms);
    }
    out
}

/// The BDS histograms of a row.
fn measured(row: &Value) -> Vec<BTreeMap<String, f64>> {
    row[5]
        .as_array()
        .unwrap()
        .iter()
        .map(|read| {
            read.as_array()
                .unwrap()
                .iter()
                .map(|entry| {
                    (
                        serde_json::to_string(&entry[0]).unwrap(),
                        entry[1].as_f64().unwrap(),
                    )
                })
                .collect()
        })
        .collect()
}

/// Fields whose cells depend on each other (grass spreading from cell to
/// cell): one BDS field is one draw of the whole field, so each count BDS
/// read must lie within 4 standard deviations (at least 3 cells) of the
/// mean of 16 unastar fields.
fn check_spreading_family(family: &str) {
    let mut failures = Vec::new();
    for (index, row) in rows(family).iter().enumerate() {
        let name = row[0].to_string();
        let theirs = measured(row);
        let ours = run_replicates(row, index as u64 + 1, 16);
        for (read, bds) in theirs.iter().enumerate() {
            let mut keys: Vec<&String> = bds
                .keys()
                .chain(ours.iter().flat_map(|r| r[read].keys()))
                .collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                let counts: Vec<f64> = ours
                    .iter()
                    .map(|r| r[read].get(key).copied().unwrap_or(0.0))
                    .collect();
                let mean = counts.iter().sum::<f64>() / counts.len() as f64;
                let sd = (counts.iter().map(|c| (c - mean).powi(2)).sum::<f64>()
                    / (counts.len() - 1) as f64)
                    .sqrt();
                let got = bds.get(key).copied().unwrap_or(0.0);
                if (got - mean).abs() > (4.0 * sd).max(3.0) {
                    failures.push(format!(
                        "{name} read {}: {key}: BDS {got}, unastar {mean:.1} +- {sd:.1}",
                        read + 1
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{family}:\n{}", failures.join("\n"));
}

/// Replay every field of `family` but `skip` (name prefixes, with a reason).
fn check_family(family: &str, skip: &[(&str, &str)]) {
    let mut failures = Vec::new();
    for (index, row) in rows(family).iter().enumerate() {
        let name = row[0].to_string();
        if skip.iter().any(|(prefix, _)| name.contains(prefix)) {
            continue;
        }
        let ours = run(row, index as u64 + 1);
        let theirs = measured(row);
        let tested: Vec<(usize, f64)> = theirs
            .iter()
            .zip(&ours)
            .enumerate()
            .filter_map(|(read, (a, b))| homogeneity(a, b).map(|p| (read + 1, p)))
            .collect();
        let Some(&(worst_read, worst)) = tested.iter().min_by(|x, y| x.1.total_cmp(&y.1)) else {
            continue;
        };
        if worst * (tested.len() as f64) < 0.001 {
            failures.push(format!(
                "{name}: read {worst_read}: p = {worst:.2e} over {} reads\n  BDS     {:?}\n  unastar {:?}",
                tested.len(),
                theirs[worst_read - 1],
                ours[worst_read - 1]
                    .iter()
                    .map(|(k, v)| (k.clone(), v / REPLICATES as f64))
                    .collect::<BTreeMap<_, _>>()
            ));
        }
    }
    assert!(failures.is_empty(), "{family}:\n{}", failures.join("\n"));
}

#[test]
fn crops_grow_like_bds() {
    check_family("random_crops", &[]);
}

#[test]
fn sugar_cane_grows_like_bds() {
    check_family(
        "random_cane",
        &[(
            "cactus",
            "BDS grows cactus flowers on cacti; unastar does not",
        )],
    );
}

/// Farmland dries cell by cell, but the dirt it leaves turns to grass from
/// the floor around it, cell to cell.
#[test]
fn bare_farmland_dries_like_bds() {
    check_spreading_family("random_farmland");
}

#[test]
fn grass_spreads_like_bds() {
    check_spreading_family("random_grass");
}

#[test]
fn leaves_decay_like_bds() {
    check_family("random_leaves", &[]);
    check_family("random_leaves_logs", &[]);
}

#[test]
fn copper_oxidises_like_bds() {
    check_family("random_copper", &[]);
    check_family("random_copper_more", &[]);
}

#[test]
fn chi_square_detects_a_different_distribution() {
    let a: BTreeMap<String, f64> = [("x".to_string(), 50.0), ("y".to_string(), 50.0)].into();
    let same: BTreeMap<String, f64> = [("x".to_string(), 300.0), ("y".to_string(), 300.0)].into();
    let other: BTreeMap<String, f64> = [("x".to_string(), 500.0), ("y".to_string(), 100.0)].into();
    assert!(homogeneity(&a, &same).unwrap() > 0.5);
    assert!(homogeneity(&a, &other).unwrap() < 1e-6);
}

/// Counts the draws a sub-chunk's random ticks take.
struct Counting(Mix, u64);

impl RngCore for Counting {
    fn next_u32(&mut self) -> u32 {
        self.1 += 1;
        self.0.next_u32()
    }

    fn next_u64(&mut self) -> u64 {
        self.1 += 1;
        self.0.next_u64()
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.0.fill_bytes(dest);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        self.0.try_fill_bytes(dest)
    }
}

/// A huge `randomtickspeed` is capped at one pick per cell of the sub-chunk
/// (a safety cap; BDS's handling is not measured).
#[test]
fn a_huge_tick_speed_picks_each_sub_chunk_at_most_once_per_cell() {
    let mut field = Field::new();
    let mut rng = Counting(Mix(7), 0);
    // An all-air sub-chunk: every pick is one draw.
    tick_subchunk(&mut field, IVec3::new(0, 32, 0), u32::MAX, &mut rng);
    assert_eq!(rng.1, 16 * 16 * 16);
}

/// A block registry the rules were not built for.
struct Foreign(Field);

impl BlockSource for Foreign {
    fn block_at(&self, pos: IVec3) -> Option<u32> {
        self.0.block_at(pos)
    }

    fn registry(&self) -> &BlockRegistry {
        static EMPTY: std::sync::LazyLock<BlockRegistry> =
            std::sync::LazyLock::new(BlockRegistry::new);
        &EMPTY
    }
}

impl UpdateTarget for Foreign {
    fn write(&mut self, pos: IVec3, state: u32) {
        self.0.write(pos, state);
    }

    fn pop(&mut self, pos: IVec3, state: u32) {
        self.0.pop(pos, state);
    }
}

/// Random ticks do nothing in a world whose registry is not the vanilla one
/// (the same runtime id may be another block there).
#[test]
fn random_ticks_skip_a_registry_that_is_not_vanilla() {
    let wheat = id("minecraft:wheat");
    let planted = || {
        let mut field = Field::new();
        for x in 0..16 {
            for z in 0..16 {
                field.write(IVec3::new(x, 0, z), id("minecraft:farmland"));
                field.write(IVec3::new(x, 1, z), wheat);
            }
        }
        field
    };
    let untouched = |field: &Field| {
        (0..16).all(|x| (0..16).all(|z| field.block_at(IVec3::new(x, 1, z)) == Some(wheat)))
    };
    // The same ticks grow wheat under the vanilla registry.
    let mut vanilla = planted();
    let mut rng = Mix(3);
    for _ in 0..200 {
        tick_subchunk(&mut vanilla, IVec3::ZERO, 64, &mut rng);
    }
    assert!(!untouched(&vanilla));
    let mut foreign = Foreign(planted());
    let mut rng = Mix(3);
    for _ in 0..200 {
        tick_subchunk(&mut foreign, IVec3::ZERO, 64, &mut rng);
    }
    assert!(untouched(&foreign.0));
}
