//! Compare complete Java-state captures before Java-to-Bedrock conversion.
//!
//! Usage: `java_canonical_compare <expected.json> <actual.json>`.
//! Inputs must use `java_worldgen_reference --canonical`; legacy name-only fixtures
//! are rejected. Exit 1 means a real difference, exit 2 means incomparable inputs.
//! `--generate <reference.json> <actual.json>` captures the requested native Rust
//! stages for the reference's chunk coverage, without an inverse Bedrock mapping.

mod java_canonical_inputs;
#[path = "java_canonical_inputs/validation.rs"]
mod java_canonical_validation;
#[path = "java_canonical_inputs/profile.rs"]
mod java_canonical_profile;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use unastar_noise::features::{JavaBlock, JavaState};
use unastar::world::ChunkPos;
use unastar::world::chunk::BlockStateSpace;
use unastar::world::generator::{ChunkPipeline, HeightmapKind, JavaDimension, JavaGenerationOptions, JavaGenerator, JavaTerrainStage};
use unastar_noise::java_biomes::JavaBiome;

#[derive(Deserialize, Serialize)]
struct Capture {
    edition: String,
    state_encoding: String,
    version: String,
    seed: i64,
    dimension: String,
    min_y: i32,
    height: usize,
    capture: String,
    structures: bool,
    datapacks: Vec<String>,
    inputs: java_canonical_inputs::Inputs,
    #[serde(flatten)]
    profile: java_canonical_profile::Profile,
    palette: Vec<String>,
    biome_palette: Vec<String>,
    chunks: Vec<Column>,
}

#[derive(Deserialize, Serialize)]
struct Column {
    x: i32,
    z: i32,
    status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    coordinate_request_index: Option<usize>,
    blocks: Vec<usize>,
    biomes: Vec<usize>,
    heightmaps: BTreeMap<String, Vec<i32>>,
    block_entities: Vec<serde_json::Value>,
}

#[derive(Serialize, Debug)]
struct Difference {
    chunk: [i32; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    coordinate_request_index: Option<usize>,
    block: [i64; 3],
    stage: String,
    expected: String,
    actual: String,
}

#[derive(Serialize, Debug)]
struct BiomeDifference {
    chunk: [i32; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    coordinate_request_index: Option<usize>,
    quart: [i64; 3],
    stage: String,
    expected: String,
    actual: String,
}

#[derive(Serialize, Debug)]
struct Report {
    dimension: String,
    seed: i64,
    chunks: usize,
    observations: usize,
    expected_capture: String,
    actual_capture: String,
    stages: Vec<String>,
    voxels: u64,
    differing_states: u64,
    differing_quart_biomes: u64,
    differing_heightmap_columns: u64,
    chunks_with_differing_block_entities: u64,
    first_state_difference: Option<Difference>,
    first_quart_biome_difference: Option<BiomeDifference>,
}

/// Reject incomplete properties rather than quietly filling them from defaults.
fn parse_state(text: &str) -> Result<JavaState, String> {
    let (name, properties) = if let Some((name, suffix)) = text.split_once('[') {
        (name, suffix.strip_suffix(']').ok_or_else(|| format!("unclosed state {text}"))?)
    } else {
        (text, "")
    };
    let block = JavaBlock::by_name(name).ok_or_else(|| format!("unknown Java block {name}"))?;
    let def = block.def();
    let mut state = block.default_state();
    let mut seen = BTreeMap::new();
    if !properties.is_empty() {
        for property in properties.split(',') {
            let (key, value) = property.split_once('=').ok_or_else(|| format!("bad property {property}"))?;
            let prop = def.props.iter().find(|p| p.name == key)
                .ok_or_else(|| format!("unknown property {key} in {text}"))?;
            if !prop.values.contains(&value) || seen.insert(key, value).is_some() {
                return Err(format!("invalid or repeated property {key} in {text}"));
            }
            state = state.with(key, value);
        }
    }
    if seen.len() != def.props.len() {
        return Err(format!("incomplete Java state {text}: expected {} properties, found {}", def.props.len(), seen.len()));
    }
    Ok(state)
}

fn load(path: &Path) -> Result<Capture, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let capture: Capture = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    validate(&capture)?;
    Ok(capture)
}

/// A chunk's actor container is a map by absolute position, not an ordered NBT
/// ListTag. Compare every typed tag unchanged; only outer map enumeration differs.
fn actors_by_position(column: &Column) -> Result<BTreeMap<[i32; 3], &serde_json::Value>, String> {
    let mut actors = BTreeMap::new();
    for actor in &column.block_entities {
        let fields = actor.get("value").and_then(serde_json::Value::as_array)
            .filter(|_| actor.get("type").and_then(serde_json::Value::as_str) == Some("Compound"))
            .ok_or("actor root must be a typed NBT compound")?;
        let mut position = [0; 3];
        for (axis, name) in ["x", "y", "z"].into_iter().enumerate() {
            let tag = fields.iter().find_map(|entry| {
                let pair = entry.as_array()?;
                (pair.len() == 2 && pair[0].as_str() == Some(name)).then(|| &pair[1])
            }).ok_or_else(|| format!("actor has no {name} position tag"))?;
            if tag.get("type").and_then(serde_json::Value::as_str) != Some("Int") {
                return Err(format!("actor position {name} must retain its NBT Int type"));
            }
            position[axis] = i32::try_from(tag.get("value").and_then(serde_json::Value::as_i64)
                .ok_or("invalid actor position value")?).map_err(|_| "actor position exceeds NBT Int")?;
        }
        if actors.insert(position, actor).is_some() {
            return Err(format!("duplicate actor position {position:?} in chunk {},{}", column.x, column.z));
        }
    }
    Ok(actors)
}

fn validate(capture: &Capture) -> Result<(), String> {
    if capture.edition != "java" || capture.state_encoding != "java-canonical-properties-v1" || capture.version != "1.21.11" {
        return Err("requires pinned Java 1.21.11 complete-property capture, not a legacy collapsed fixture".into());
    }
    capture.inputs.validate_metadata(capture.seed, capture.structures, &capture.dimension)?;
    let bounds = match capture.dimension.as_str() {
        "minecraft:overworld" => (-64, 384),
        "minecraft:the_nether" | "minecraft:the_end" => (0, 256),
        dimension => return Err(format!("unsupported dimension {dimension}")),
    };
    if (capture.min_y, capture.height) != bounds || capture.capture.trim().is_empty() || capture.chunks.is_empty() {
        return Err("invalid dimension bounds, missing capture controls, or empty corpus".into());
    }
    for state in &capture.palette {
        parse_state(state)?;
    }
    let mut coordinates = BTreeMap::new();
    for column in &capture.chunks {
        capture.profile.validate_observation(ChunkPos::new(column.x, column.z), column.coordinate_request_index)?;
        let observation = capture.profile.observation_index(column.coordinate_request_index);
        if coordinates.insert((column.x, column.z, observation), ()).is_some() {
            return Err(format!("duplicate chunk observation {},{} at {observation:?}", column.x, column.z));
        }
        if column.status.is_empty() || column.blocks.len() % 2 != 0 {
            return Err(format!("missing status or incomplete RLE in {},{}", column.x, column.z));
        }
        let mut volume = 0usize;
        for run in column.blocks.chunks_exact(2) {
            if run[0] >= capture.palette.len() || run[1] == 0 {
                return Err("invalid state index or zero-length run".into());
            }
            volume = volume.checked_add(run[1]).ok_or("RLE volume overflow")?;
        }
        if volume != 256 * capture.height || column.biomes.len() != 4 * capture.height {
            return Err(format!("incomplete volume in {},{}", column.x, column.z));
        }
        if column.biomes.iter().any(|&b| b >= capture.biome_palette.len()) || column.heightmaps.values().any(|h| h.len() != 256) {
            return Err("invalid biome index or heightmap volume".into());
        }
        actors_by_position(column)?;
    }
    Ok(())
}

fn compare(expected: &Capture, actual: &Capture) -> Result<Report, String> {
    if (expected.seed, &expected.dimension, expected.min_y, expected.height, expected.structures, &expected.datapacks, &expected.inputs)
        != (actual.seed, &actual.dimension, actual.min_y, actual.height, actual.structures, &actual.datapacks, &actual.inputs) {
        return Err("seed, dimension, bounds, native generator settings, structure setting or datapack contents differ".into());
    }
    if expected.profile != actual.profile {
        return Err("native request/observation histories differ".into());
    }
    let expected_chunks: BTreeMap<_, _> = expected.chunks.iter().map(|c| {
        ((c.x, c.z, expected.profile.observation_index(c.coordinate_request_index)), c)
    }).collect();
    let actual_chunks: BTreeMap<_, _> = actual.chunks.iter().map(|c| {
        ((c.x, c.z, actual.profile.observation_index(c.coordinate_request_index)), c)
    }).collect();
    if !expected_chunks.keys().eq(actual_chunks.keys()) {
        return Err("chunk coverage differs (missing/extra chunks cannot count as parity)".into());
    }
    let expected_states = expected.palette.iter().map(|s| parse_state(s)).collect::<Result<Vec<_>, _>>()?;
    let actual_states = actual.palette.iter().map(|s| parse_state(s)).collect::<Result<Vec<_>, _>>()?;
    let mut report = Report {
        dimension: expected.dimension.clone(), seed: expected.seed,
        chunks: expected.chunks.iter().map(|c| (c.x, c.z)).collect::<std::collections::BTreeSet<_>>().len(),
        observations: expected_chunks.len(),
        expected_capture: expected.capture.clone(), actual_capture: actual.capture.clone(),
        stages: expected_chunks.values().map(|c| c.status.clone()).collect::<std::collections::BTreeSet<_>>().into_iter().collect(),
        voxels: 0, differing_states: 0, differing_quart_biomes: 0,
        differing_heightmap_columns: 0, chunks_with_differing_block_entities: 0, first_state_difference: None,
        first_quart_biome_difference: None,
    };
    for (pos, reference) in expected_chunks {
        let generated = actual_chunks[&pos];
        if reference.status != generated.status {
            return Err(format!("chunk {pos:?}: unequal generation stages {} / {}", reference.status, generated.status));
        }
        let reference_voxels = reference.blocks.chunks_exact(2).flat_map(|r| std::iter::repeat_n(r[0], r[1]));
        let generated_voxels = generated.blocks.chunks_exact(2).flat_map(|r| std::iter::repeat_n(r[0], r[1]));
        for (index, (e, a)) in reference_voxels.zip(generated_voxels).enumerate() {
            report.voxels += 1;
            if expected_states[e] != actual_states[a] {
                report.differing_states += 1;
                report.first_state_difference.get_or_insert_with(|| Difference {
                    chunk: [pos.0, pos.1],
                    coordinate_request_index: pos.2,
                    block: [i64::from(pos.0) * 16 + (index % 16) as i64,
                        i64::from(expected.min_y) + (index / 256) as i64,
                        i64::from(pos.1) * 16 + ((index / 16) % 16) as i64],
                    stage: reference.status.clone(), expected: expected.palette[e].clone(), actual: actual.palette[a].clone(),
                });
            }
        }
        for (index, (&e, &a)) in reference.biomes.iter().zip(&generated.biomes).enumerate() {
            if expected.biome_palette[e] != actual.biome_palette[a] {
                report.differing_quart_biomes += 1;
                report.first_quart_biome_difference.get_or_insert_with(|| BiomeDifference {
                    chunk: [pos.0, pos.1],
                    coordinate_request_index: pos.2,
                    quart: [i64::from(pos.0) * 4 + (index % 4) as i64,
                        i64::from(expected.min_y / 4) + (index / 16) as i64,
                        i64::from(pos.1) * 4 + ((index / 4) % 4) as i64],
                    stage: reference.status.clone(),
                    expected: expected.biome_palette[e].clone(),
                    actual: actual.biome_palette[a].clone(),
                });
            }
        }
        if !reference.heightmaps.keys().eq(generated.heightmaps.keys()) {
            return Err(format!("chunk {pos:?}: heightmap coverage differs"));
        }
        for (kind, heights) in &reference.heightmaps {
            report.differing_heightmap_columns += heights.iter().zip(&generated.heightmaps[kind]).filter(|(e, a)| e != a).count() as u64;
        }
        report.chunks_with_differing_block_entities += u64::from(
            actors_by_position(reference)? != actors_by_position(generated)?,
        );
    }
    Ok(report)
}

fn generate(reference: &Capture) -> Result<Capture, String> {
    reference.profile.validate_runtime_history()?;
    let dimension = match reference.dimension.as_str() {
        "minecraft:overworld" => JavaDimension::Overworld,
        "minecraft:the_nether" => JavaDimension::Nether,
        "minecraft:the_end" => JavaDimension::End,
        other => return Err(format!("unsupported native Java dimension {other}")),
    };
    let bounds = dimension.block_bounds();
    if reference.min_y != bounds.start || reference.height != (bounds.end - bounds.start) as usize {
        return Err(format!(
            "native generation requires complete {} storage volume {}..{}, capture is {}..{}",
            reference.dimension, bounds.start, bounds.end,
            reference.min_y, reference.min_y + reference.height as i32,
        ));
    }
    let biome_features = reference.inputs.validate_generation(
        reference.seed, reference.structures, &reference.dimension, &reference.datapacks,
    )?;
    let generator = Arc::new(JavaGenerator::with_options(reference.seed, JavaGenerationOptions {
        dimension,
        generate_structures: reference.structures,
        biome_features,
    }));
    let pipeline = ChunkPipeline::new(generator.clone());
    let mut actual = Capture {
        edition: "java".into(), state_encoding: "java-canonical-properties-v1".into(),
        version: unastar_noise::java_blocks::JAVA_VERSION.into(),
        seed: reference.seed, dimension: reference.dimension.clone(),
        min_y: reference.min_y, height: reference.height,
        capture: "Rust source-native in-memory generation using captured native world options/datapack recipe; no ticking or Bedrock block conversion".into(),
        structures: reference.structures, datapacks: reference.datapacks.clone(),
        inputs: reference.inputs.clone(),
        profile: reference.profile.clone(),
        palette: Vec::new(), biome_palette: Vec::new(), chunks: Vec::new(),
    };
    let mut states = BTreeMap::new();
    let mut biome_ids = BTreeMap::new();
    let mut columns: Vec<_> = reference.chunks.iter().collect();
    columns.sort_by_key(|column| reference.profile.observation_index(column.coordinate_request_index));
    for column in columns {
        let pos = ChunkPos::new(column.x, column.z);
        let stage = match column.status.as_str() {
            "minecraft:empty" => Some(JavaTerrainStage::Empty),
            "minecraft:structure_starts" => Some(JavaTerrainStage::StructureStarts),
            "minecraft:structure_references" => Some(JavaTerrainStage::StructureReferences),
            "minecraft:biomes" => Some(JavaTerrainStage::Biomes),
            "minecraft:noise" => Some(JavaTerrainStage::Noise),
            "minecraft:surface" => Some(JavaTerrainStage::Surface),
            "minecraft:carvers" => Some(JavaTerrainStage::Carvers),
            // FEATURES and every later status observe the mutable native world.
            // Later statuses write no blocks (native write radius -1); FULL
            // additionally promotes pending actors and drops the WG maps.
            "minecraft:features" | "minecraft:initialize_light" | "minecraft:light"
            | "minecraft:spawn" | "minecraft:full" => {
                    if (reference.structures || biome_features) && !cfg!(feature = "java-worldgen") {
                        return Err("FEATURES captures require --features java-worldgen with the pinned structure templates".into());
                    }
                    None
                }
            status => return Err(format!("cannot capture unimplemented native generation status {status}")),
        };
        // Native LIGHT depends on INITIALIZE_LIGHT, hence FEATURES, at radius
        // one; SPAWN and FULL inherit that. Those neighbor FEATURES writes run
        // in a scheduler order a capture must record; never invent it here.
        let closes_halo = matches!(column.status.as_str(), "minecraft:light" | "minecraft:spawn" | "minecraft:full");
        if closes_halo && !reference.profile.closes_features_halo() {
            return Err(format!("{} capture does not record its neighbor FEATURES request order", column.status));
        }
        let full = column.status == "minecraft:full";
        let owned = stage.map(|stage| generator.generate_stage(pos, stage));
        let finished = if stage.is_none() {
            reference.profile.decorate(&pipeline, &reference.dimension, pos, column.coordinate_request_index)?;
            let snapshot = if full { pipeline.native_full_snapshot(pos) } else { pipeline.native_snapshot(pos) };
            Some(snapshot.ok_or("native FEATURES request produced no source snapshot")?)
        } else {
            None
        };
        let chunk = finished.as_ref().or_else(|| owned.as_ref().map(|proto| &proto.chunk))
            .expect("one native chunk source");
        if chunk.state_space() != BlockStateSpace::Java {
            return Err(format!("chunk {pos:?} lost its canonical Java identity"));
        }
        let mut actors = chunk.block_entities().iter().map(|actor| {
            let nbt = if full { actor.canonical_java_level_nbt() } else { actor.canonical_java_nbt() };
            nbt.map(|nbt| (actor.position(), nbt))
                .map_err(|error| format!("chunk {pos:?}: native actor export failed: {error}"))
        }).collect::<Result<Vec<_>, _>>()?;
        actors.sort_by_key(|(position, _)| *position);
        // Retained containers belong to the requested native stage, not to the
        // reference's chosen keys. Otherwise a corrupt reference can manufacture
        // an EMPTY heightmap, or hide a missing runtime container.
        let stage_maps: &[(&str, HeightmapKind)] = match stage {
            Some(stage) if stage < JavaTerrainStage::Noise => &[],
            Some(_) => &[
                ("OCEAN_FLOOR_WG", HeightmapKind::OceanFloorWg),
                ("WORLD_SURFACE_WG", HeightmapKind::WorldSurfaceWg),
            ],
            // A LevelChunk retains only the four final maps.
            None if full => &[
                ("MOTION_BLOCKING", HeightmapKind::MotionBlocking),
                ("MOTION_BLOCKING_NO_LEAVES", HeightmapKind::MotionBlockingNoLeaves),
                ("OCEAN_FLOOR", HeightmapKind::OceanFloor),
                ("WORLD_SURFACE", HeightmapKind::WorldSurface),
            ],
            None => &[
                ("MOTION_BLOCKING", HeightmapKind::MotionBlocking),
                ("MOTION_BLOCKING_NO_LEAVES", HeightmapKind::MotionBlockingNoLeaves),
                ("OCEAN_FLOOR", HeightmapKind::OceanFloor),
                ("OCEAN_FLOOR_WG", HeightmapKind::OceanFloorWg),
                ("WORLD_SURFACE", HeightmapKind::WorldSurface),
                ("WORLD_SURFACE_WG", HeightmapKind::WorldSurfaceWg),
            ],
        };
        let mut maps: Vec<_> = stage_maps.iter().map(|&(name, kind)| {
            (name.to_owned(), kind, vec![reference.min_y; 256])
        }).collect();
        let mut blocks: Vec<usize> = Vec::new();
        for y in reference.min_y..reference.min_y + reference.height as i32 {
            for z in 0..16u8 {
                for x in 0..16u8 {
                    let id = chunk.get_block(x, y as i16, z);
                    let state = JavaState(u16::try_from(id).map_err(|_| "invalid native Java state")?);
                    let palette_id = *states.entry(state).or_insert_with(|| {
                        let index = actual.palette.len();
                        actual.palette.push(state.to_string());
                        index
                    });
                    if blocks.len() >= 2 && blocks[blocks.len() - 2] == palette_id {
                        let last = blocks.len() - 1;
                        blocks[last] += 1;
                    } else {
                        blocks.extend([palette_id, 1]);
                    }
                    let flags = unastar_noise::java_blocks::JAVA_STATE_CLASSES[id as usize];
                    for (_, kind, heights) in &mut maps {
                        if kind.counts(flags) {
                            heights[z as usize * 16 + x as usize] = y + 1;
                        }
                    }
                }
            }
        }
        let mut biomes = Vec::with_capacity(reference.height * 4);
        for qy in 0..reference.height / 4 {
            for qz in 0..4 {
                for qx in 0..4 {
                    let id = if let Some(proto) = &owned {
                        let source_y = (reference.min_y - unastar::world::chunk::MIN_Y) as usize / 4 + qy;
                        proto.biomes.get(qx, source_y, qz)
                    } else {
                        u16::try_from(chunk.biome_at(
                            (qx * 4) as u8, (reference.min_y + qy as i32 * 4) as i16, (qz * 4) as u8,
                        ).ok_or("native biome outside chunk storage")?).map_err(|_| "invalid native biome id")?
                    };
                    let palette_id = *biome_ids.entry(id).or_insert_with(|| {
                        let index = actual.biome_palette.len();
                        actual.biome_palette.push(JavaBiome::from_index(id)
                            .expect("native Java biome id").data().name.into());
                        index
                    });
                    biomes.push(palette_id);
                }
            }
        }
        if stage.is_none() {
            let frozen = pipeline.native_worldgen_heightmaps(pos)
                .ok_or("native FEATURES snapshot lost its frozen worldgen heightmaps")?;
            for (_, kind, heights) in &mut maps {
                let index = match kind {
                    HeightmapKind::WorldSurfaceWg => Some(0),
                    HeightmapKind::OceanFloorWg => Some(1),
                    _ => None,
                };
                if let Some(index) = index {
                    heights.iter_mut().zip(frozen[index]).for_each(|(height, native)| {
                        *height = i32::from(native);
                    });
                }
            }
        }
        actual.chunks.push(Column {
            x: column.x, z: column.z, status: column.status.clone(), blocks, biomes,
            coordinate_request_index: column.coordinate_request_index,
            heightmaps: maps.into_iter().map(|(name, _, heights)| (name, heights)).collect(),
            block_entities: actors.into_iter().map(|(_, nbt)| nbt).collect(),
        });
    }
    validate(&actual)?;
    Ok(actual)
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--generate") {
        if args.len() != 3 {
            eprintln!("usage: java_canonical_compare --generate <reference.json> <actual.json>");
            std::process::exit(2);
        }
        let result = std::thread::Builder::new().stack_size(64 << 20).spawn(move || {
            let reference = load(Path::new(&args[1]))?;
            // Only writes into compared chunks count as exercised coverage.
            #[cfg(feature = "worldgen-coverage")]
            unastar_noise::coverage::set_targets(reference.chunks.iter().map(|c| (c.x, c.z)));
            let actual = generate(&reference)?;
            let text = serde_json::to_string(&actual).map_err(|error| error.to_string())?;
            std::fs::write(&args[2], text).map_err(|error| error.to_string())?;
            #[cfg(feature = "worldgen-coverage")]
            {
                let mut coverage: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
                for (kind, name) in unastar_noise::coverage::take_hits() {
                    coverage.entry(kind.key()).or_default().push(name);
                }
                let mut path = args[2].clone();
                path.push(".coverage.json");
                let text = serde_json::to_string(&coverage).map_err(|error| error.to_string())?;
                std::fs::write(path, text).map_err(|error| error.to_string())?;
            }
            println!("captured {} native chunks, {} full-volume voxels", actual.chunks.len(),
                actual.chunks.len() * actual.height * 256);
            Ok::<_, String>(())
        }).expect("spawn native generation thread").join().expect("native generation panicked");
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(2);
        }
        return;
    }
    let result = if args.len() == 2 {
        load(Path::new(&args[0])).and_then(|expected| load(Path::new(&args[1])).and_then(|actual| compare(&expected, &actual)))
    } else {
        Err("usage: java_canonical_compare <expected.json> <actual.json>".into())
    };
    match result {
        Ok(report) => {
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
            if report.differing_states + report.differing_quart_biomes + report.differing_heightmap_columns + report.chunks_with_differing_block_entities != 0 {
                std::process::exit(1);
            }
        }
        Err(error) => { eprintln!("{error}"); std::process::exit(2); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_states_preserve_properties_and_air_identity() {
        assert_ne!(parse_state("minecraft:water[level=0]").unwrap(), parse_state("minecraft:water[level=1]").unwrap());
        assert_ne!(parse_state("minecraft:air").unwrap(), parse_state("minecraft:cave_air").unwrap());
        assert_ne!(parse_state("minecraft:grass_block[snowy=false]").unwrap(), parse_state("minecraft:grass_block[snowy=true]").unwrap());
        assert!(parse_state("minecraft:water").is_err());
        assert!(parse_state("minecraft:water[level=16]").is_err());
        assert!(parse_state("minecraft:water[level=0,level=1]").is_err());
    }
}

#[cfg(test)]
mod volume_tests {
    use super::*;

    fn capture() -> Capture {
        serde_json::from_value(serde_json::json!({
            "edition":"java", "state_encoding":"java-canonical-properties-v1", "version":"1.21.11",
            "seed":i64::MIN, "dimension":"minecraft:overworld", "min_y":-64, "height":384,
            "capture":"synthetic comparator regression, not native parity", "structures":false, "datapacks":["vanilla"],
            "inputs": {
                "worldgen_settings":{"type":"Compound","value":[
                    ["bonus_chest",{"type":"Byte","value":0}],
                    ["dimensions",{"type":"Compound","value":[
                        ["minecraft:overworld",java_canonical_validation::vanilla_dimension("minecraft:overworld").unwrap()]
                    ]}],
                    ["generate_features",{"type":"Byte","value":0}],
                    ["seed",{"type":"Long","value":i64::MIN}]
                ]},
                "enabled_features":["minecraft:vanilla"],
                "data_profile":{"kind":"vanilla"}
            },
            "palette":["minecraft:air","minecraft:cave_air"], "biome_palette":["minecraft:plains"],
            "chunks":[{"x":-2,"z":-1,"status":"minecraft:carvers","blocks":[0,98304],
                "biomes":vec![0;1536],"heightmaps":{},"block_entities":[]}]
        })).unwrap()
    }

    #[test]
    fn repeated_chunk_observations_keep_earlier_differences_and_require_full_history() {
        let mut document = serde_json::to_value(capture()).unwrap();
        document["coordinate_request_order"] = serde_json::json!(["-2:-1", "-2:-1"]);
        let mut first = document["chunks"][0].clone();
        first["status"] = serde_json::json!("minecraft:features");
        first["coordinate_request_index"] = serde_json::json!(0);
        let mut second = first.clone();
        second["coordinate_request_index"] = serde_json::json!(1);
        document["chunks"] = serde_json::json!([first, second]);
        let expected: Capture = serde_json::from_value(document.clone()).unwrap();
        validate(&expected).unwrap();
        document["chunks"][0]["blocks"] = serde_json::json!([0, 98303, 1, 1]);
        let actual: Capture = serde_json::from_value(document.clone()).unwrap();
        let report = compare(&expected, &actual).unwrap();
        assert_eq!((report.chunks, report.observations, report.voxels), (1, 2, 196608));
        assert_eq!(report.differing_states, 1);
        assert_eq!(report.first_state_difference.unwrap().coordinate_request_index, Some(0));
        document["chunks"].as_array_mut().unwrap().remove(0);
        let missing: Capture = serde_json::from_value(document.clone()).unwrap();
        assert!(compare(&expected, &missing).is_err());
        document["chunks"] = serde_json::json!([
            document["chunks"][0].clone(), document["chunks"][0].clone(),
        ]);
        let ambiguous: Capture = serde_json::from_value(document).unwrap();
        assert!(validate(&ambiguous).is_err());
    }

    #[test]
    fn actor_container_order_is_not_payload_identity_or_duplicate_permission() {
        let actor = |x, seed| serde_json::json!({"type":"Compound","value":[
            ["LootTableSeed",{"type":"Long","value":seed}],
            ["id",{"type":"Str","value":"minecraft:chest"}],
            ["x",{"type":"Int","value":x}],
            ["y",{"type":"Int","value":-64}],
            ["z",{"type":"Int","value":-16}]
        ]});
        let mut expected = capture();
        expected.chunks[0].block_entities = vec![actor(-32, i64::MIN), actor(-31, i64::MAX)];
        let mut actual = capture();
        actual.chunks[0].block_entities = expected.chunks[0].block_entities.iter().rev().cloned().collect();
        assert_eq!(compare(&expected, &actual).unwrap().chunks_with_differing_block_entities, 0);
        actual.chunks[0].block_entities[1]["value"][0][1]["value"] = serde_json::json!(-1);
        assert_eq!(compare(&expected, &actual).unwrap().chunks_with_differing_block_entities, 1);
        let duplicate = actual.chunks[0].block_entities[0].clone();
        actual.chunks[0].block_entities.push(duplicate);
        assert!(validate(&actual).is_err());
    }

    #[test]
    fn source_world_options_must_match_the_declared_seed() {
        let mut actual = capture();
        actual.seed += 1;
        assert!(validate(&actual).is_err());
    }

    #[test]
    fn last_voxel_and_negative_chunk_coordinates_are_compared() {
        let expected = capture();
        let mut actual = capture();
        actual.chunks[0].blocks = vec![0, 98303, 1, 1];
        validate(&actual).unwrap();
        let report = compare(&expected, &actual).unwrap();
        assert_eq!(report.voxels, 98304);
        assert_eq!(report.differing_states, 1);
        let first = report.first_state_difference.unwrap();
        assert_eq!(first.block, [-17, 319, -1]);
        assert_eq!(first.expected, "minecraft:air");
        assert_eq!(first.actual, "minecraft:cave_air");
    }

    #[test]
    fn last_quart_reports_native_coordinates_and_names() {
        let expected = capture();
        let mut actual = capture();
        actual.biome_palette.push("minecraft:forest".into());
        *actual.chunks[0].biomes.last_mut().unwrap() = 1;
        let report = compare(&expected, &actual).unwrap();
        assert_eq!(report.differing_quart_biomes, 1);
        assert_eq!(report.differing_states, 0);
        let first = report.first_quart_biome_difference.unwrap();
        assert_eq!(first.quart, [-5, 79, -1]);
        assert_eq!(first.expected, "minecraft:plains");
        assert_eq!(first.actual, "minecraft:forest");
    }

    #[test]
    fn malformed_runs_and_unequal_stage_or_coverage_are_rejected() {
        let expected = capture();
        let mut actual = capture();
        actual.chunks[0].blocks.push(0);
        assert!(validate(&actual).is_err());
        actual.chunks[0].blocks = vec![0, 98305];
        assert!(validate(&actual).is_err());
        actual.chunks[0].blocks = vec![0, 98304];
        actual.chunks[0].status = "minecraft:full".into();
        assert!(compare(&expected, &actual).is_err());
        actual.chunks.clear();
        assert!(compare(&expected, &actual).is_err());
    }
}
