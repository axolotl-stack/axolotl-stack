//! Sub-chunk records saved by BDS 1.26.51 decode to the blocks BDS reports, and
//! unastar writes the bytes BDS writes.
//!
//! The fixture (`fixtures/bds_1_26_51_subchunks.json`) holds raw sub-chunk
//! values from worlds BDS saved, written by `tools/bds-sweep/subchunk_fixture.py`:
//! every palette bit width BDS uses, both storage layers, the three dimensions,
//! and the flat-ground record whose misreading crashed the loader. `probes`
//! are cells BDS confirmed with `/testforblock` after reloading the world;
//! `cells_sha256` covers all 4096 cells of a layer (format in the fixture).
//!
//! The edit fixture (`fixtures/bds_1_26_51_subchunk_edits.json`, written by
//! `tools/bds-sweep/subchunk_roundtrip.py`) holds sections of a BDS world before
//! BDS edited them, the cell writes, and the hash of the record BDS saved when
//! the edited chunk unloaded.

use std::collections::BTreeMap;

use bleveldb::options::Compression;
use bleveldb::{DB, Options, ReadOptions, WriteBatch, WriteOptions};
use serde_json::Value;
use sha2::{Digest, Sha256};
use unastar::registry::BlockRegistry;
use unastar::registry::block::{SavedStateValue, StateValue};
use unastar::storage::{LevelDBWorldProvider, WorldProvider};
use unastar::world::{Chunk, ChunkPos};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/bds_1_26_51_subchunks.json")).unwrap()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

/// `name|key=<b|i|s>value,...`, properties sorted by key: the fixture's cell form.
fn describe(state: u32) -> String {
    let block = BlockRegistry::vanilla()
        .get_by_runtime_id(state)
        .unwrap_or_else(|| panic!("runtime id {state} is not a block state"));
    let mut properties: Vec<&str> = block.data.properties.iter().map(|p| p.name).collect();
    properties.sort_unstable();
    let states: Vec<String> = properties
        .iter()
        .map(|&key| match block.state_value(state, key).unwrap() {
            StateValue::Byte(value) => format!("{key}=b{value}"),
            StateValue::Int(value) => format!("{key}=i{value}"),
            StateValue::Str(value) => format!("{key}=s{value}"),
        })
        .collect();
    format!("{}|{}", block.string_id, states.join(","))
}

struct Record {
    id: String,
    dimension: i32,
    chunk: (i32, i32),
    y: i8,
    value: Vec<u8>,
    json: Value,
}

fn records() -> Vec<Record> {
    fixture()["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|json| Record {
            id: json["id"].as_str().unwrap().to_owned(),
            dimension: json["dimension"].as_i64().unwrap() as i32,
            chunk: (
                json["chunk"][0].as_i64().unwrap() as i32,
                json["chunk"][1].as_i64().unwrap() as i32,
            ),
            y: json["y"].as_i64().unwrap() as i8,
            value: unhex(json["value"].as_str().unwrap()),
            json: json.clone(),
        })
        .collect()
}

fn decode(record: &Record) -> Chunk {
    let mut chunk = Chunk::new(record.chunk.0, record.chunk.1);
    chunk
        .decode_subchunk(i32::from(record.y), &record.value)
        .unwrap_or_else(|error| panic!("{}: {error}", record.id));
    chunk
}

/// Every cell of one layer, in the fixture's cell form and XZY order.
fn layer_cells(chunk: &Chunk, y: i8, layer: u8) -> Vec<String> {
    let base = i16::from(y) * 16;
    let mut cells = Vec::with_capacity(4096);
    for x in 0..16u8 {
        for z in 0..16u8 {
            for dy in 0..16i16 {
                cells.push(describe(chunk.get_block_layer(x, base + dy, z, layer)));
            }
        }
    }
    cells
}

fn sha256(cells: &[String]) -> String {
    Sha256::digest(cells.join("\n").as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The decoded storages match BDS's layout facts and every cell matches.
#[test]
fn every_bds_record_decodes_to_the_blocks_bds_reports() {
    let records = records();
    assert!(records.len() >= 15, "fixture has {} records", records.len());
    let mut probes = 0;
    for record in &records {
        let chunk = decode(record);
        let layers = record.json["layers"].as_array().unwrap();
        assert_eq!(usize::from(record.value[1]), layers.len(), "{}", record.id);
        for (layer, expected) in layers.iter().enumerate() {
            let cells = layer_cells(&chunk, record.y, layer as u8);
            assert_eq!(
                sha256(&cells),
                expected["cells_sha256"].as_str().unwrap(),
                "{} layer {layer}",
                record.id
            );
        }
        let states = record.json["probe_states"].as_array().unwrap();
        let record_probes = record.json["probes"].as_array().unwrap();
        assert!(record_probes.len() >= 64, "{}: few BDS cells", record.id);
        for probe in record_probes {
            let [x, y, z, state] = [0, 1, 2, 3].map(|i| probe[i].as_u64().unwrap());
            let cell =
                describe(chunk.get_block(x as u8, i16::from(record.y) * 16 + y as i16, z as u8));
            assert_eq!(
                cell,
                states[state as usize].as_str().unwrap(),
                "{} cell ({x}, {y}, {z})",
                record.id
            );
            probes += 1;
        }
    }
    assert!(probes >= 1_500, "only {probes} BDS-confirmed cells");
    // Every bit width BDS writes, and both layers, are in the fixture.
    let mut widths: Vec<u64> = records
        .iter()
        .flat_map(|record| record.json["layers"].as_array().unwrap().clone())
        .map(|layer| layer["bits"].as_u64().unwrap())
        .collect();
    widths.sort_unstable();
    widths.dedup();
    assert_eq!(widths, [0, 1, 2, 3, 4, 5, 6, 8, 16]);
    assert!(records.iter().any(|record| record.value[1] == 2));
    assert_eq!(
        records
            .iter()
            .map(|record| record.dimension)
            .collect::<std::collections::BTreeSet<_>>(),
        [0, 1, 2].into()
    );
}

/// The record that crashed the loader: the flat-ground sub-chunk of a BDS
/// world, a 2-bit persistent storage whose `i32` palette length was read as
/// a varint (2 instead of 4), leaving indices 2 and 3 outside the palette.
#[test]
fn the_flat_ground_record_that_crashed_the_loader_decodes() {
    let record = records()
        .into_iter()
        .find(|record| record.id == "flat_ground")
        .unwrap();
    assert_eq!(&record.value[..4], &[9, 1, 0xFC, 4]);
    let chunk = decode(&record);
    let name = |y| {
        BlockRegistry::vanilla()
            .get_by_runtime_id(chunk.get_block(7, y, 9))
            .unwrap()
            .string_id
            .clone()
    };
    assert_eq!(name(-64), "minecraft:bedrock");
    assert_eq!(name(-63), "minecraft:dirt");
    assert_eq!(name(-62), "minecraft:dirt");
    assert_eq!(name(-61), "minecraft:grass_block");
    assert_eq!(name(-60), "minecraft:air");
    assert_eq!(chunk.height_map().at(7, 9), -60);
}

/// Whole columns of every dimension load through the LevelDB provider from
/// the raw BDS records, under BDS's own keys.
#[tokio::test]
async fn bds_records_load_through_the_leveldb_provider() {
    let directory = tempfile::tempdir().unwrap();
    let records = records();
    {
        let options = Options::new();
        options.create_if_missing(true);
        options.compression(Compression::ZlibRaw);
        let db = DB::open(directory.path(), &options).unwrap();
        let mut batch = WriteBatch::new();
        for record in &records {
            let mut key = Vec::with_capacity(14);
            key.extend_from_slice(&record.chunk.0.to_le_bytes());
            key.extend_from_slice(&record.chunk.1.to_le_bytes());
            if record.dimension != 0 {
                key.extend_from_slice(&record.dimension.to_le_bytes());
            }
            let column = key.clone();
            key.extend_from_slice(&[0x2F, record.y as u8]);
            batch.put(&key, &record.value);
            // BDS 1.26.51 writes chunk version 42.
            batch.put(&[column.as_slice(), &[0x2C]].concat(), &[42]);
        }
        batch.write(&db, &WriteOptions::new()).unwrap();
    }
    let provider = LevelDBWorldProvider::open(directory.path()).unwrap();
    for record in &records {
        let pos = ChunkPos::new(record.chunk.0, record.chunk.1);
        let column = provider
            .load_column(pos, record.dimension)
            .await
            .unwrap_or_else(|error| panic!("{}: {error}", record.id))
            .unwrap_or_else(|| panic!("{}: column missing", record.id));
        let expected = decode(record);
        for layer in 0..2 {
            assert_eq!(
                layer_cells(&column.chunk, record.y, layer),
                layer_cells(&expected, record.y, layer),
                "{} layer {layer}",
                record.id
            );
        }
    }
}

/// An unchanged section is saved exactly as BDS saved it: every fixture
/// record, decoded and encoded again, comes out byte for byte (palette order,
/// unused entries, width, zero-bit storages, both layers, the NBT entries and
/// their `version`).
#[test]
fn every_bds_record_is_written_back_byte_for_byte() {
    for record in records() {
        let chunk = decode(&record);
        assert_eq!(
            chunk.encode_subchunk_disk(i32::from(record.y)),
            Some(record.value.clone()),
            "{}",
            record.id
        );
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Canonical state of a cell-form string `name|key=<b|i|s>value,...`.
fn parse_state(text: &str) -> u32 {
    let (name, states) = text.split_once('|').unwrap();
    let states: Vec<(&str, SavedStateValue<'_>)> = states
        .split(',')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (key, value) = part.split_once('=').unwrap();
            let value = match &value[..1] {
                "b" => SavedStateValue::Byte(value[1..].parse().unwrap()),
                "i" => SavedStateValue::Int(value[1..].parse().unwrap()),
                "s" => SavedStateValue::Str(&value[1..]),
                other => panic!("state type {other}"),
            };
            (key, value)
        })
        .collect();
    BlockRegistry::vanilla()
        .state_by_properties(name, &states)
        .unwrap_or_else(|error| panic!("{text}: {error}"))
}

/// One write of the edit fixture: local x, absolute y, local z, both layers.
type Write = (u8, i16, u8, u32, Option<u32>);

/// One section of the edit fixture.
struct EditedSection {
    id: String,
    dimension: i32,
    chunk: (i32, i32),
    y: i32,
    /// BDS's record before the edits, if the section had one.
    before: Option<Vec<u8>>,
    /// Cell writes in order.
    writes: Vec<Write>,
    /// SHA-256 and length of the record BDS saved when the chunk unloaded.
    after: Option<(String, usize)>,
}

fn edited_sections() -> Vec<EditedSection> {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/bds_1_26_51_subchunk_edits.json")).unwrap();
    let states: Vec<u32> = fixture["state_table"]
        .as_array()
        .unwrap()
        .iter()
        .map(|text| parse_state(text.as_str().unwrap()))
        .collect();
    let liquid = |index: i64| (index >= 0).then(|| states[index as usize]);
    let number = |value: &Value| value.as_i64().unwrap();
    fixture["sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|json| {
            let dimension = number(&json["dimension"]) as i32;
            let chunk = (
                number(&json["chunk"][0]) as i32,
                number(&json["chunk"][1]) as i32,
            );
            let y = number(&json["y"]) as i32;
            let base = (y * 16) as i16;
            let id = format!("{dimension}/{}/{}/{y}", chunk.0, chunk.1);
            let before = if let Some(build) = json.get("before_build") {
                // Rebuild the record from its placements into air: the
                // palette follows the placement order, as BDS's did.
                let mut built = Chunk::new(chunk.0, chunk.1);
                for cell in build.as_array().unwrap() {
                    let [x, cy, z, state] = [0, 1, 2, 3].map(|i| number(&cell[i]));
                    built.set_block(x as u8, base + cy as i16, z as u8, states[state as usize]);
                }
                let record = built.encode_subchunk_disk(y).unwrap();
                assert_eq!(
                    hex_sha256(&record),
                    json["before_sha256"].as_str().unwrap(),
                    "{id}: rebuilt record"
                );
                Some(record)
            } else {
                json["before"].as_str().map(unhex)
            };
            let mut writes: Vec<Write> = Vec::new();
            for write in json["writes"].as_array().unwrap() {
                if let Some(fill) = write.get("fill") {
                    // Every cell of the section.
                    let state = states[number(&fill[0]) as usize];
                    let liquid = liquid(number(&fill[1]));
                    writes.extend((0..4096u16).map(|i| {
                        let (x, z, cy) = (i >> 8, (i >> 4) & 15, i & 15);
                        (x as u8, base + cy as i16, z as u8, state, liquid)
                    }));
                } else {
                    let [x, cy, z, state, fluid] = [0, 1, 2, 3, 4].map(|i| number(&write[i]));
                    writes.push((
                        x as u8,
                        base + cy as i16,
                        z as u8,
                        states[state as usize],
                        liquid(fluid),
                    ));
                }
            }
            let after = json["after_sha256"]
                .as_str()
                .map(|hash| (hash.to_owned(), number(&json["after_len"]) as usize));
            EditedSection {
                id,
                dimension,
                chunk,
                y,
                before,
                writes,
                after,
            }
        })
        .collect()
}

/// After the same cell writes, unastar saves the record BDS saved when the
/// chunk unloaded, byte for byte: entries appended in the order the writes
/// add them, palettes that outgrow their width widened (0 to 1, 1 to 2, 2 to
/// 3, 3 to 4, 4 to 5, 5 to 6, 6 to 8 and 8 to 16 bits), unused entries dropped exactly when
/// that narrows the width (down to 0 bits), the liquid layer of blocks set
/// into water, and no record for a section cleared to air.
#[test]
fn edited_sections_are_saved_as_bds_saves_them_on_unload() {
    let sections = edited_sections();
    assert!(sections.len() >= 20, "{} sections", sections.len());
    for section in &sections {
        let mut chunk = Chunk::new(section.chunk.0, section.chunk.1);
        if let Some(before) = &section.before {
            chunk.decode_subchunk(section.y, before).unwrap();
            // Unchanged, the section is written back as BDS wrote it.
            assert_eq!(
                chunk.encode_subchunk_disk(section.y).as_ref(),
                Some(before),
                "{}: unchanged",
                section.id
            );
        }
        for &(x, y, z, primary, liquid) in &section.writes {
            chunk.set_block_layers(x, y, z, primary, liquid).unwrap();
        }
        let saved = chunk.encode_subchunk_disk(section.y);
        assert_eq!(
            saved.map(|record| (hex_sha256(&record), record.len())),
            section.after,
            "{}",
            section.id
        );
    }
}

/// The same edits through the LevelDB provider: BDS's records loaded from
/// LevelDB, the columns edited and saved, then every edited record on disk is
/// the one BDS saved, and the cleared section's record is gone.
#[tokio::test]
async fn edited_columns_saved_through_the_leveldb_provider_match_bds() {
    let sections = edited_sections();
    let directory = tempfile::tempdir().unwrap();
    let column_key = |section: &EditedSection| {
        let mut key = Vec::with_capacity(14);
        key.extend_from_slice(&section.chunk.0.to_le_bytes());
        key.extend_from_slice(&section.chunk.1.to_le_bytes());
        if section.dimension != 0 {
            key.extend_from_slice(&section.dimension.to_le_bytes());
        }
        key
    };
    let section_key = |section: &EditedSection| {
        [column_key(section).as_slice(), &[0x2F, section.y as u8]].concat()
    };
    {
        let options = Options::new();
        options.create_if_missing(true);
        options.compression(Compression::ZlibRaw);
        let db = DB::open(directory.path(), &options).unwrap();
        let mut batch = WriteBatch::new();
        for section in &sections {
            // BDS 1.26.51 writes chunk version 42.
            batch.put(&[column_key(section).as_slice(), &[0x2C]].concat(), &[42]);
            if let Some(before) = &section.before {
                batch.put(&section_key(section), before);
            }
        }
        batch.write(&db, &WriteOptions::new()).unwrap();
    }
    let mut columns: BTreeMap<(i32, i32, i32), Vec<&EditedSection>> = BTreeMap::new();
    for section in &sections {
        columns
            .entry((section.dimension, section.chunk.0, section.chunk.1))
            .or_default()
            .push(section);
    }
    {
        let provider = LevelDBWorldProvider::open(directory.path()).unwrap();
        for (&(dimension, x, z), sections) in &columns {
            let pos = ChunkPos::new(x, z);
            let mut column = provider.load_column(pos, dimension).await.unwrap().unwrap();
            for section in sections {
                for &(x, y, z, primary, liquid) in &section.writes {
                    column
                        .chunk
                        .set_block_layers(x, y, z, primary, liquid)
                        .unwrap();
                }
            }
            provider.save_column(pos, dimension, &column).await.unwrap();
        }
        provider.close().await.unwrap();
    }
    let db = DB::open(directory.path(), &Options::new()).unwrap();
    for section in &sections {
        let record = db.get(&section_key(section), &ReadOptions::new()).unwrap();
        assert_eq!(
            record.map(|record| (hex_sha256(&record), record.len())),
            section.after,
            "{}",
            section.id
        );
    }
}
