//! Native biome identities/measurements with behaviour-pack enrichment.
//!
//! Data is generated from protocolgen and bedrock-samples at build time,
//! embedded, and decoded once. EULA-derived rows are not checked in (D13).

use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// Native biome definition enriched with the complete pack components.
#[derive(Debug, Clone, PartialEq)]
pub struct BiomeData {
    pub id: u32,
    pub identifier: String,
    pub native_definition_json: String,
    pub format_version: String,
    pub source_file: String,
    pub climate: Option<BiomeClimate>,
    pub tags: Vec<String>,
    pub component_names: Vec<String>,
    pub components: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiomeClimate {
    pub temperature: f32,
    pub downfall: f32,
    pub snow_accumulation: Option<(f32, f32)>,
}

#[derive(Deserialize)]
struct Row {
    id: u32,
    identifier: String,
    native_definition: Value,
    format_version: String,
    source_file: String,
    components: BTreeMap<String, Value>,
}

/// Build-generated canonical JSON, exposed for manifest generation.
pub const VANILLA_JSON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/biomes.json"));

static VANILLA: LazyLock<Vec<BiomeData>> = LazyLock::new(|| {
    let rows: Vec<Row> = serde_json::from_slice(VANILLA_JSON).expect("embedded biome data");
    rows.into_iter()
        .map(|row| {
            let n = &row.native_definition;
            let number = |field: &str| {
                n[field]
                    .as_f64()
                    .unwrap_or_else(|| panic!("{}: missing native {field}", row.identifier))
                    as f32
            };
            let snow_accumulation = match (n["minSnowLevel"].as_f64(), n["maxSnowLevel"].as_f64()) {
                (Some(min), Some(max)) => Some((min as f32, max as f32)),
                _ => row
                    .components
                    .get("minecraft:climate")
                    .and_then(|c| c["snow_accumulation"].as_array())
                    .filter(|a| a.len() == 2)
                    .and_then(|a| Some((a[0].as_f64()? as f32, a[1].as_f64()? as f32))),
            };
            let climate = Some(BiomeClimate {
                temperature: number("temperature"),
                downfall: number("downfall"),
                snow_accumulation,
            });
            let mut tags: Vec<String> = row
                .components
                .get("minecraft:tags")
                .and_then(|c| c["tags"].as_array())
                .map(|a| {
                    a.iter()
                        .map(|v| v.as_str().expect("biome tag string").to_owned())
                        .collect()
                })
                .unwrap_or_default();
            tags.sort();
            tags.dedup();
            BiomeData {
                id: row.id,
                identifier: row.identifier,
                native_definition_json: row.native_definition.to_string(),
                format_version: row.format_version,
                source_file: row.source_file,
                climate,
                tags,
                component_names: row.components.keys().cloned().collect(),
                components: row.components,
            }
        })
        .collect()
});

pub fn vanilla() -> &'static [BiomeData] {
    &VANILLA
}

pub fn get(identifier: &str) -> Option<&'static BiomeData> {
    let rows = vanilla();
    rows.binary_search_by(|row| row.identifier.as_str().cmp(identifier))
        .ok()
        .map(|i| &rows[i])
}

pub fn iter() -> impl Iterator<Item = &'static BiomeData> {
    vanilla().iter()
}
