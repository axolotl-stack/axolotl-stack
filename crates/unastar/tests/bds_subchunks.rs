//! Sub-chunk records saved by BDS 1.26.51 decode to the blocks BDS reports.
//!
//! The fixture (`fixtures/bds_1_26_51_subchunks.json`) holds raw sub-chunk
//! values from worlds BDS saved, written by `tools/bds-sweep/subchunk_fixture.py`:
//! every palette bit width BDS uses, both storage layers, the three dimensions,
//! and the flat-ground record whose misreading crashed the loader. `probes`
//! are cells BDS confirmed with `/testforblock` after reloading the world;
//! `cells_sha256` covers all 4096 cells of a layer (format in the fixture).

use bleveldb::options::Compression;
use bleveldb::{DB, Options, WriteBatch, WriteOptions};
use serde_json::Value;
use sha2::{Digest, Sha256};
use unastar::registry::BlockRegistry;
use unastar::registry::block::StateValue;
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
