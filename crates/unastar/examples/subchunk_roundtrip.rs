//! Sub-chunk round trips with BDS, driven by `tools/bds-sweep/subchunk_roundtrip.py`.
//!
//! ```text
//! subchunk_roundtrip edit <db dir> <ops.json>
//! subchunk_roundtrip generate <db dir> <spec.json> <cells.json>
//! ```
//!
//! `edit` loads every column the ops touch through the LevelDB provider, writes
//! both layers of each op's cell in order (`Chunk::set_block_layers`, the write
//! every block change of the server ends in) and saves the columns. Ops are
//! `{"dim", "pos": [x, y, z], "primary": STATE, "liquid": STATE | null}` with
//! `STATE = [name, [[key, "byte" | "int" | "string", value], ...]]`.
//!
//! `generate` generates columns with the Java-style generator of each
//! dimension, saves them, and writes both layers of every probe cell in the
//! sub-chunk fixture's cell form (`name|key=<b|i|s>value,...`, keys sorted).

use std::collections::BTreeMap;
use std::error::Error;
use std::sync::Arc;

use serde_json::{Value, json};
use unastar::registry::BlockRegistry;
use unastar::registry::block::{SavedStateValue, StateValue};
use unastar::storage::{ChunkColumn, LevelDBWorldProvider, WorldProvider};
use unastar::world::generator::{
    ChunkPipeline, JavaDimension, JavaGenerationOptions, JavaGenerator,
};
use unastar::world::{Chunk, ChunkPos};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [mode, db, ops] if mode == "edit" => edit(db, ops).await,
        [mode, db, spec, cells] if mode == "generate" => generate(db, spec, cells).await,
        _ => Err("usage: subchunk_roundtrip edit DB OPS | generate DB SPEC CELLS".into()),
    }
}

fn read_json(path: &str) -> Result<Value> {
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

fn int(value: &Value) -> Result<i32> {
    let int = value
        .as_i64()
        .ok_or_else(|| format!("{value} is not an integer"))?;
    Ok(i32::try_from(int)?)
}

/// Canonical runtime state of a `[name, [[key, type, value], ...]]` state.
fn state(value: &Value) -> Result<u32> {
    let name = value[0].as_str().ok_or("state name")?;
    let mut states = Vec::new();
    for entry in value[1].as_array().ok_or("state list")? {
        let key = entry[0].as_str().ok_or("state key")?;
        let value = match entry[1].as_str().ok_or("state type")? {
            "byte" => SavedStateValue::Byte(u8::try_from(int(&entry[2])?)?),
            "int" => SavedStateValue::Int(int(&entry[2])?),
            "string" => SavedStateValue::Str(entry[2].as_str().ok_or("string state")?),
            other => return Err(format!("state type {other}").into()),
        };
        states.push((key, value));
    }
    Ok(BlockRegistry::vanilla().state_by_properties(name, &states)?)
}

async fn edit(db: &str, ops: &str) -> Result<()> {
    let ops = read_json(ops)?;
    // Ops per column, in order.
    let mut columns: BTreeMap<(i32, i32, i32), Vec<&Value>> = BTreeMap::new();
    for op in ops["ops"].as_array().ok_or("ops")? {
        let (x, z) = (int(&op["pos"][0])?, int(&op["pos"][2])?);
        columns
            .entry((int(&op["dim"])?, x >> 4, z >> 4))
            .or_default()
            .push(op);
    }
    let provider = LevelDBWorldProvider::open(db)?;
    let mut writes = 0;
    for (&(dim, cx, cz), ops) in &columns {
        let pos = ChunkPos::new(cx, cz);
        let mut column = provider
            .load_column(pos, dim)
            .await?
            .ok_or_else(|| format!("column {dim}/{cx}/{cz} is not stored"))?;
        for op in ops {
            let [x, y, z] = [0, 1, 2].map(|i| int(&op["pos"][i]));
            let (x, y, z) = (x?, y?, z?);
            let liquid = match &op["liquid"] {
                Value::Null => None,
                liquid => Some(state(liquid)?),
            };
            column
                .chunk
                .set_block_layers(
                    (x & 15) as u8,
                    i16::try_from(y)?,
                    (z & 15) as u8,
                    state(&op["primary"])?,
                    liquid,
                )
                .ok_or_else(|| format!("({x}, {y}, {z}) is outside the column"))?;
            writes += 1;
        }
        provider.save_column(pos, dim, &column).await?;
    }
    provider.close().await?;
    println!("{writes} cells written in {} columns", columns.len());
    Ok(())
}

/// The fixture's cell form: `name|key=<b|i|s>value,...` with keys sorted.
fn describe(state: u32) -> String {
    let block = BlockRegistry::vanilla()
        .get_by_runtime_id(state)
        .expect("chunks hold canonical states");
    let mut properties: Vec<&str> = block.data.properties.iter().map(|p| p.name).collect();
    properties.sort_unstable();
    let states: Vec<String> = properties
        .iter()
        .map(
            |&key| match block.state_value(state, key).expect("property value") {
                StateValue::Byte(value) => format!("{key}=b{value}"),
                StateValue::Int(value) => format!("{key}=i{value}"),
                StateValue::Str(value) => format!("{key}=s{value}"),
            },
        )
        .collect();
    format!("{}|{}", block.string_id, states.join(","))
}

async fn generate(db: &str, spec: &str, cells: &str) -> Result<()> {
    let spec = read_json(spec)?;
    let seed = spec["seed"].as_i64().ok_or("seed")?;
    let provider = LevelDBWorldProvider::open(db)?;
    let mut pipelines: BTreeMap<i32, ChunkPipeline> = BTreeMap::new();
    let mut chunks: BTreeMap<(i32, i32, i32), Chunk> = BTreeMap::new();
    for column in spec["columns"].as_array().ok_or("columns")? {
        let (dim, cx, cz) = (int(&column[0])?, int(&column[1])?, int(&column[2])?);
        let pipeline = pipelines.entry(dim).or_insert_with(|| {
            let dimension = match dim {
                1 => JavaDimension::Nether,
                2 => JavaDimension::End,
                _ => JavaDimension::Overworld,
            };
            // Structure templates are only embedded with the `java-templates`
            // build feature; terrain, carvers and biome features need nothing.
            let options = JavaGenerationOptions {
                dimension,
                generate_structures: false,
                ..JavaGenerationOptions::default()
            };
            ChunkPipeline::new(Arc::new(JavaGenerator::with_options(seed, options)))
        });
        let pos = ChunkPos::new(cx, cz);
        let chunk = pipeline.generate(pos)?;
        provider
            .save_column(pos, dim, &ChunkColumn::new(chunk.clone()))
            .await?;
        chunks.insert((dim, cx, cz), chunk);
    }
    provider.close().await?;
    let mut out = Vec::new();
    for probe in spec["probes"].as_array().ok_or("probes")? {
        let [dim, x, y, z] = [0, 1, 2, 3].map(|i| int(&probe[i]));
        let (dim, x, y, z) = (dim?, x?, y?, z?);
        let chunk = &chunks[&(dim, x >> 4, z >> 4)];
        let layer = |layer| {
            describe(chunk.get_block_layer((x & 15) as u8, y as i16, (z & 15) as u8, layer))
        };
        out.push(json!([dim, x, y, z, layer(0), layer(1)]));
    }
    std::fs::write(cells, serde_json::to_string(&json!({ "cells": out }))?)?;
    println!(
        "{} columns generated, {} cells reported",
        chunks.len(),
        out.len()
    );
    Ok(())
}
