//! Parity metrics against reference-world fixtures (decision D8).
//!
//! Fixtures are written by `examples/bds_worldgen_reference.rs` (BDS, Bedrock names and
//! numeric biome ids) and `examples/java_worldgen_reference.rs` (Java Edition, Java names
//! and biome identifiers). Java names and biomes are normalised to Bedrock here through
//! the generated `java_blocks` / `java_biomes` tables, so both fixtures compare against
//! the Bedrock runtime ids a generator writes. `tests/worldgen_parity.rs` ratchets the
//! metrics; `examples/worldgen_parity_report.rs` prints them.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use unastar_noise::java_biomes::JavaBiome;
use unastar_noise::java_blocks;

use super::ChunkPipeline;
use crate::registry::BlockRegistry;
use crate::world::ChunkPos;
use crate::world::chunk::{Chunk, MIN_Y};

/// Which edition wrote a fixture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edition {
    /// BDS reference world.
    Bedrock,
    /// Java Edition server reference world.
    Java,
}

/// One fixture chunk, with names and biomes normalised to Bedrock.
#[derive(Debug, Clone)]
pub struct FixtureChunk {
    pub pos: ChunkPos,
    /// Highest non-air block per column (`z * 16 + x`).
    pub surface: Vec<i32>,
    /// Highest terrain block per column ([`is_terrain_block`]).
    pub terrain: Vec<i32>,
    /// Bedrock name of the surface block per column.
    pub surface_block: Vec<String>,
    /// Bedrock biome id at the surface block per column.
    pub surface_biome: Vec<u32>,
    /// Bedrock block name -> count over the chunk (air excluded).
    pub histogram: BTreeMap<String, u64>,
}

/// A reference-world fixture.
#[derive(Debug, Clone)]
pub struct Fixture {
    pub edition: Edition,
    pub seed: i64,
    pub spawn: (i32, i32),
    pub chunks: Vec<FixtureChunk>,
}

#[derive(Deserialize)]
struct RawFixture {
    #[serde(default)]
    edition: Option<String>,
    seed: i64,
    spawn: (i32, i32),
    palette: Vec<String>,
    chunks: Vec<RawChunk>,
}

#[derive(Deserialize)]
struct RawChunk {
    x: i32,
    z: i32,
    surface: Vec<i32>,
    terrain: Vec<i32>,
    surface_block: Vec<usize>,
    surface_biome: Vec<serde_json::Value>,
    histogram: BTreeMap<String, u64>,
}

/// Bedrock name of a Java block name (its default state's Bedrock block).
fn java_to_bedrock_name(registry: &BlockRegistry, name: &str) -> Result<String, String> {
    let java = java_blocks::default_state(name)
        .ok_or_else(|| format!("unknown Java block {name} in fixture"))?;
    let runtime = java_blocks::JAVA_TO_BEDROCK[java as usize];
    registry
        .get_by_runtime_id(runtime)
        .map(|entry| entry.string_id.clone())
        .ok_or_else(|| format!("Java block {name} maps to unknown runtime id {runtime}"))
}

impl Fixture {
    /// Load and normalise a fixture file.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let raw: RawFixture =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let edition = match raw.edition.as_deref() {
            None => Edition::Bedrock,
            Some("java") => Edition::Java,
            Some(other) => return Err(format!("unknown fixture edition {other}")),
        };
        let registry = BlockRegistry::vanilla();
        let normalise = |name: &str| -> Result<String, String> {
            match edition {
                Edition::Bedrock => Ok(name.to_string()),
                Edition::Java => java_to_bedrock_name(registry, name),
            }
        };
        let palette = raw
            .palette
            .iter()
            .map(|name| normalise(name))
            .collect::<Result<Vec<_>, _>>()?;
        let mut chunks = Vec::with_capacity(raw.chunks.len());
        for chunk in raw.chunks {
            let surface_biome = chunk
                .surface_biome
                .iter()
                .map(|value| match value {
                    serde_json::Value::Number(n) => n
                        .as_u64()
                        .map(|id| id as u32)
                        .ok_or_else(|| format!("bad biome id {n}")),
                    serde_json::Value::String(name) => JavaBiome::from_name(name)
                        .map(|biome| biome.data().bedrock_id)
                        .ok_or_else(|| format!("unknown Java biome {name}")),
                    other => Err(format!("bad biome {other}")),
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut histogram = BTreeMap::new();
            for (name, count) in &chunk.histogram {
                *histogram.entry(normalise(name)?).or_default() += count;
            }
            chunks.push(FixtureChunk {
                pos: ChunkPos::new(chunk.x, chunk.z),
                surface: chunk.surface,
                terrain: chunk.terrain,
                surface_block: chunk
                    .surface_block
                    .iter()
                    .map(|&i| palette[i].clone())
                    .collect(),
                surface_biome,
                histogram,
            });
        }
        Ok(Self {
            edition,
            seed: raw.seed,
            spawn: raw.spawn,
            chunks,
        })
    }
}

/// Blocks placed by the noise router, surface rules and ore veins (Bedrock names).
/// Decoration and fluids are excluded so terrain height compares terrain shape only.
/// Mirrors the reference tools' lists.
pub fn is_terrain_block(name: &str) -> bool {
    let n = name.trim_start_matches("minecraft:");
    matches!(
        n,
        "stone"
            | "deepslate"
            | "dirt"
            | "coarse_dirt"
            | "grass_block"
            | "podzol"
            | "mycelium"
            | "sand"
            | "red_sand"
            | "sandstone"
            | "red_sandstone"
            | "gravel"
            | "terracotta"
            | "snow"
            | "ice"
            | "packed_ice"
            | "powder_snow"
            | "calcite"
            | "mud"
            | "bedrock"
            | "tuff"
            | "granite"
            | "raw_iron_block"
            | "raw_copper_block"
            | "iron_ore"
            | "copper_ore"
            | "deepslate_iron_ore"
            | "deepslate_copper_ore"
    ) || n.ends_with("_terracotta")
}

/// Fixture parity metrics; every rate is in `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParityMetrics {
    pub columns: u64,
    pub terrain_exact: f64,
    pub terrain_within_2: f64,
    pub terrain_mean_abs_error: f64,
    pub surface_exact: f64,
    pub surface_block: f64,
    pub surface_biome: f64,
    /// `sum |reference - generated| / sum reference` over all block counts.
    pub histogram_l1: f64,
}

/// Metrics plus the details a report prints.
#[derive(Debug, Clone)]
pub struct ParityReport {
    pub metrics: ParityMetrics,
    /// `(reference surface block, generated surface block) -> columns`.
    pub surface_confusion: BTreeMap<(String, String), u64>,
    /// `block -> (reference count, generated count)`.
    pub histogram: BTreeMap<String, (u64, u64)>,
}

fn name_of(registry: &BlockRegistry, runtime: u32) -> &str {
    registry
        .get_by_runtime_id(runtime)
        .map_or("?", |entry| entry.string_id.as_str())
}

fn is_air(name: &str) -> bool {
    name == "minecraft:air"
}

/// Generate every fixture chunk through `pipeline` and compare.
pub fn measure(fixture: &Fixture, pipeline: &ChunkPipeline) -> ParityReport {
    let registry = BlockRegistry::vanilla();
    let mut columns = 0u64;
    let (mut terrain_exact, mut terrain_close, mut terrain_abs) = (0u64, 0u64, 0u64);
    let (mut surface_exact, mut surface_block, mut surface_biome) = (0u64, 0u64, 0u64);
    let mut confusion: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut histogram: BTreeMap<String, (u64, u64)> = BTreeMap::new();

    for reference in &fixture.chunks {
        let chunk = pipeline.generate(reference.pos)
            .expect("parity fixture contains an unrepresentable native actor");
        for (name, count) in &reference.histogram {
            histogram.entry(name.clone()).or_default().0 += count;
        }
        count_blocks(&chunk, registry, &mut histogram);
        for column in 0..256usize {
            let (x, z) = ((column & 15) as u8, (column >> 4) as u8);
            let (surface, top, terrain) = column_tops(&chunk, registry, x, z);
            columns += 1;
            let reference_terrain = reference.terrain[column];
            let diff = terrain.abs_diff(reference_terrain) as u64;
            terrain_exact += u64::from(diff == 0);
            terrain_close += u64::from(diff <= 2);
            terrain_abs += diff;
            surface_exact += u64::from(surface == reference.surface[column]);
            surface_block += u64::from(top == reference.surface_block[column]);
            *confusion
                .entry((reference.surface_block[column].clone(), top.to_string()))
                .or_default() += 1;
            let biome = chunk.biome_at(x, surface as i16, z);
            surface_biome += u64::from(biome == Some(reference.surface_biome[column]));
        }
    }

    let reference_total: u64 = histogram.values().map(|(r, _)| r).sum();
    let l1: u64 = histogram.values().map(|(r, g)| r.abs_diff(*g)).sum();
    let rate = |n: u64| n as f64 / columns.max(1) as f64;
    ParityReport {
        metrics: ParityMetrics {
            columns,
            terrain_exact: rate(terrain_exact),
            terrain_within_2: rate(terrain_close),
            terrain_mean_abs_error: rate(terrain_abs),
            surface_exact: rate(surface_exact),
            surface_block: rate(surface_block),
            surface_biome: rate(surface_biome),
            histogram_l1: l1 as f64 / reference_total.max(1) as f64,
        },
        surface_confusion: confusion,
        histogram,
    }
}

/// `(surface y, surface block name, terrain y)` of a column; air-only columns report
/// `MIN_Y` / `minecraft:air` / `MIN_Y - 1` like the reference tools.
fn column_tops<'r>(
    chunk: &Chunk,
    registry: &'r BlockRegistry,
    x: u8,
    z: u8,
) -> (i32, &'r str, i32) {
    let mut top: Option<(i32, &str)> = None;
    for y in (MIN_Y..MIN_Y + 384).rev() {
        let name = name_of(registry, chunk.get_block(x, y as i16, z));
        if is_air(name) {
            continue;
        }
        if top.is_none() {
            top = Some((y, name));
        }
        if is_terrain_block(name) {
            let (sy, sname) = top.unwrap_or((y, name));
            return (sy, sname, y);
        }
    }
    let (sy, sname) = top.unwrap_or((MIN_Y, "minecraft:air"));
    (sy, sname, MIN_Y - 1)
}

fn count_blocks(
    chunk: &Chunk,
    registry: &BlockRegistry,
    histogram: &mut BTreeMap<String, (u64, u64)>,
) {
    let mut counts: BTreeMap<u32, u64> = BTreeMap::new();
    for y in MIN_Y..MIN_Y + 384 {
        for z in 0..16u8 {
            for x in 0..16u8 {
                *counts.entry(chunk.get_block(x, y as i16, z)).or_default() += 1;
            }
        }
    }
    for (runtime, count) in counts {
        let name = name_of(registry, runtime);
        if !is_air(name) {
            histogram.entry(name.to_string()).or_default().1 += count;
        }
    }
}

/// A pre-decoration reference fixture (Java world generated with biome features removed
/// and structures off): exact noise, aquifer, ore-vein, surface and carver voxels plus
/// quart biomes.
#[derive(Debug, Clone)]
pub struct ProtoFixture {
    pub seed: i64,
    pub chunks: Vec<ProtoFixtureChunk>,
}

/// One pre-decoration chunk.
#[derive(Debug, Clone)]
pub struct ProtoFixtureChunk {
    pub pos: ChunkPos,
    /// Bedrock block name per voxel, index `((y - MIN_Y) * 16 + z) * 16 + x`.
    pub blocks: Vec<String>,
    /// Bedrock biome id per quart, index `(qy * 4 + qz) * 4 + qx`.
    pub biomes: Vec<u32>,
}

#[derive(Deserialize)]
struct RawProtoFixture {
    seed: i64,
    palette: Vec<String>,
    biome_palette: Vec<String>,
    chunks: Vec<RawProtoChunk>,
}

#[derive(Deserialize)]
struct RawProtoChunk {
    x: i32,
    z: i32,
    /// `[palette index, run length, ...]`.
    blocks: Vec<u32>,
    biomes: Vec<usize>,
}

impl ProtoFixture {
    /// Load a pre-decoration fixture, normalising Java names to Bedrock.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let raw: RawProtoFixture =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let registry = BlockRegistry::vanilla();
        let palette = raw
            .palette
            .iter()
            .map(|name| {
                // Fluid levels are kept as `name[level=N]`; compare block types only.
                let bare = name.split('[').next().unwrap_or(name);
                java_to_bedrock_name(registry, bare)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let biome_ids = raw
            .biome_palette
            .iter()
            .map(|name| {
                JavaBiome::from_name(name)
                    .map(|biome| biome.data().bedrock_id)
                    .ok_or_else(|| format!("unknown Java biome {name}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut chunks = Vec::with_capacity(raw.chunks.len());
        for chunk in raw.chunks {
            let mut blocks = Vec::with_capacity(16 * 16 * 384);
            for run in chunk.blocks.chunks_exact(2) {
                let name = &palette[run[0] as usize];
                blocks.extend(std::iter::repeat_n(name.clone(), run[1] as usize));
            }
            if blocks.len() != 16 * 16 * 384 || chunk.biomes.len() != 4 * 4 * 96 {
                return Err(format!("proto chunk {} {} is truncated", chunk.x, chunk.z));
            }
            chunks.push(ProtoFixtureChunk {
                pos: ChunkPos::new(chunk.x, chunk.z),
                blocks,
                biomes: chunk.biomes.iter().map(|&i| biome_ids[i]).collect(),
            });
        }
        Ok(Self {
            seed: raw.seed,
            chunks,
        })
    }
}

/// Pre-decoration voxel metrics; rates in `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProtoMetrics {
    pub voxels: u64,
    /// Same block type.
    pub block_exact: f64,
    /// Same class (air, water, lava, solid).
    pub class_exact: f64,
    /// Same quart biome.
    pub biome_exact: f64,
}

/// Metrics plus a confusion table of mismatching block types.
#[derive(Debug, Clone)]
pub struct ProtoReport {
    pub metrics: ProtoMetrics,
    /// `(reference block, generated block) -> voxels`, mismatches only.
    pub confusion: BTreeMap<(String, String), u64>,
}

fn voxel_class(name: &str) -> u8 {
    match name {
        "minecraft:air" => 0,
        "minecraft:water" | "minecraft:flowing_water" => 1,
        "minecraft:lava" | "minecraft:flowing_lava" => 2,
        _ => 3,
    }
}

/// Compare the pipeline's protos (no decoration) with a pre-decoration fixture.
pub fn measure_proto(fixture: &ProtoFixture, pipeline: &ChunkPipeline) -> ProtoReport {
    let registry = BlockRegistry::vanilla();
    let generator = pipeline.generator();
    let (mut voxels, mut exact, mut class) = (0u64, 0u64, 0u64);
    let (mut quarts, mut biome_exact) = (0u64, 0u64);
    let mut confusion: BTreeMap<(String, String), u64> = BTreeMap::new();
    for reference in &fixture.chunks {
        let proto = pipeline.proto(reference.pos);
        for y in MIN_Y..MIN_Y + 384 {
            for z in 0..16u8 {
                for x in 0..16u8 {
                    let index = (((y - MIN_Y) as usize * 16) + z as usize) * 16 + x as usize;
                    let expected = reference.blocks[index].as_str();
                    let native = proto.chunk.get_block(x, y as i16, z);
                    let generated = name_of(registry, proto.chunk.state_space().to_bedrock(native));
                    voxels += 1;
                    if expected == generated {
                        exact += 1;
                    } else {
                        *confusion
                            .entry((expected.to_string(), generated.to_string()))
                            .or_default() += 1;
                    }
                    class += u64::from(voxel_class(expected) == voxel_class(generated));
                }
            }
        }
        for (index, &expected) in reference.biomes.iter().enumerate() {
            let (qy, qz, qx) = (index / 16, (index / 4) % 4, index % 4);
            quarts += 1;
            biome_exact +=
                u64::from(generator.bedrock_biome(proto.biomes.get(qx, qy, qz)) == expected);
        }
    }
    ProtoReport {
        metrics: ProtoMetrics {
            voxels,
            block_exact: exact as f64 / voxels.max(1) as f64,
            class_exact: class as f64 / voxels.max(1) as f64,
            biome_exact: biome_exact as f64 / quarts.max(1) as f64,
        },
        confusion,
    }
}
