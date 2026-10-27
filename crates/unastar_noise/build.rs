//! AOT worldgen toolchain: parse data into IR, analyze, emit Rust into `OUT_DIR`.
//!
//! Inputs: `worldgen_data/` (the pinned Java version's `data/minecraft/worldgen`, unchanged)
//! and `java_data/` (Java reports and Java->Bedrock maps, see `java_data/README.md`).
//! Layout: `codegen/parser/<family>.rs` (JSON -> IR), `codegen/data/` (non-worldgen
//! tables), `codegen/analyzer/` (density graph), `codegen/emitter/<family>.rs`
//! (IR -> `OUT_DIR/<module>.rs`, registered in `emitter::emit_all`).

mod codegen;

use std::path::PathBuf;

fn load_nether_biome_parameters(
    java_data: &std::path::Path,
) -> Result<codegen::data::biome_parameters::BiomeParameters, String> {
    use codegen::data::biome_parameters::{BiomeParameters, Point};
    use codegen::parser::noise_settings::SpawnTargetPoint;

    #[derive(serde::Deserialize)]
    struct Entry {
        biome: String,
        parameters: SpawnTargetPoint,
    }
    #[derive(serde::Deserialize)]
    struct Parameters {
        biomes: Vec<Entry>,
    }

    let path = java_data.join("biome_parameters_nether.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file: Parameters =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let targets: Vec<_> = file.biomes.iter().map(|entry| entry.parameters.clone()).collect();
    let spaces = codegen::emitter::settings::spawn_target(&targets)?;
    Ok(BiomeParameters {
        biomes: file.biomes.into_iter().map(|entry| entry.biome).collect(),
        points: spaces
            .into_iter()
            .enumerate()
            .map(|(biome, space)| Point {
                space: space.map(|[min, max]| (min, max)),
                biome,
            })
            .collect(),
    })
}

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let output_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let json_root = manifest_dir.join("worldgen_data");
    let java_data = manifest_dir.join("java_data");

    for dir in ["worldgen_data", "java_data", "codegen"] {
        println!("cargo:rerun-if-changed={dir}");
        for e in walkdir::WalkDir::new(manifest_dir.join(dir))
            .into_iter()
            .flatten()
        {
            println!("cargo:rerun-if-changed={}", e.path().display());
        }
    }

    let noises = codegen::parser::noise::parse_all(&json_root.join("noise"))
        .expect("Failed to parse noise definitions");
    let density_functions =
        codegen::parser::density_function::parse_all(&json_root.join("density_function"))
            .expect("Failed to parse density functions");
    let noise_settings =
        codegen::parser::noise_settings::parse_all(&json_root.join("noise_settings"))
            .expect("Failed to parse noise settings");
    let dimension_types = codegen::parser::noise_settings::parse_dimension_types(
        &json_root.join("world_preset/normal.json"),
        &java_data.join("dimension_type"),
    )
    .expect("Failed to parse dimension storage geometry");
    let biomes = codegen::parser::biome::parse_all(&json_root.join("biome"))
        .expect("Failed to parse biome definitions");
    let features = codegen::parser::feature::parse_all(&json_root)
        .unwrap_or_else(|e| panic!("Failed to parse feature definitions: {e}"));
    println!(
        "cargo:warning=Parsed {} configured / {} placed features",
        features.configured.len(),
        features.placed.len()
    );
    let structures = codegen::parser::structure::parse_all(&json_root, &java_data)
        .unwrap_or_else(|e| panic!("Failed to parse structure definitions: {e}"));
    println!(
        "cargo:warning=Parsed {} structure sets / {} structures / {} pools / {} processor lists",
        structures.sets.len(),
        structures.structures.len(),
        structures.pools.len(),
        structures.processor_lists.len()
    );
    let java_blocks = codegen::data::java_blocks::JavaBlocks::load(&java_data)
        .unwrap_or_else(|e| panic!("Failed to load java_data block tables: {e}"));
    let bedrock_biome_ids = codegen::emitter::java_biomes::load_bedrock_ids(&java_data)
        .unwrap_or_else(|e| panic!("Failed to load java_data biome ids: {e}"));
    let biome_parameters = codegen::data::biome_parameters::BiomeParameters::load(&java_data)
        .unwrap_or_else(|e| panic!("Failed to load java_data biome parameters: {e}"));
    let nether_biome_parameters = load_nether_biome_parameters(&java_data)
        .unwrap_or_else(|e| panic!("Failed to load Nether biome parameters: {e}"));
    let carvers = codegen::parser::carver::parse_all(&json_root.join("configured_carver"))
        .unwrap_or_else(|e| panic!("Failed to parse configured carvers: {e}"));
    let block_tags = codegen::data::block_tags::BlockTags::load(&java_data.join("tags/block"))
        .unwrap_or_else(|e| panic!("Failed to load java_data block tags: {e}"));

    codegen::emitter::emit_all(
        &output_dir,
        &codegen::emitter::Inputs {
            noises: &noises,
            density_functions: &density_functions,
            noise_settings: &noise_settings,
            dimension_types: &dimension_types,
            biomes: &biomes,
            java_blocks: &java_blocks,
            bedrock_biome_ids: &bedrock_biome_ids,
            biome_parameters: &biome_parameters,
            nether_biome_parameters: &nether_biome_parameters,
            carvers: &carvers,
            block_tags: &block_tags,
        },
    )
    .expect("Failed to emit generated code");

    // Java block registry for decoration (features/structures).
    let mut decor_blocks = codegen::java_blocks::JavaBlocks::load(
        &java_data, java_blocks.shape_support_table(), java_blocks.state_physics_table(),
    )
        .unwrap_or_else(|e| panic!("Failed to load Java block data: {e}"));
    // Features intern block states and sets, so they are emitted first.
    codegen::emitter::feature::emit_features(
        &output_dir,
        &java_data,
        &biomes,
        &features,
        &mut decor_blocks,
    )
    .unwrap_or_else(|e| panic!("Failed to emit features: {e}"));

    // Structure templates from the local Java server jar (feature `java-templates`).
    println!("cargo:rerun-if-env-changed=UNASTAR_JAVA_SERVER_JAR");
    let jar = if std::env::var_os("CARGO_FEATURE_JAVA_TEMPLATES").is_some() {
        let path = std::env::var("UNASTAR_JAVA_SERVER_JAR").unwrap_or_else(|_| {
            panic!(
                "feature `java-templates` needs UNASTAR_JAVA_SERVER_JAR = path to the Java Edition \
                 1.21.11 server.jar (structure templates are read from it at build time)"
            )
        });
        println!("cargo:rerun-if-changed={path}");
        Some(PathBuf::from(path))
    } else {
        None
    };
    let template_names = codegen::templates::emit(&output_dir, &decor_blocks, jar.as_deref())
        .unwrap_or_else(|e| panic!("Failed to embed structure templates: {e}"));
    println!(
        "cargo:warning=Embedded {} structure templates",
        template_names.len()
    );
    let mut biome_names: Vec<String> = biomes.keys().map(|b| format!("minecraft:{b}")).collect();
    biome_names.sort();
    let placed_names: Vec<String> = features.placed.keys().cloned().collect();
    codegen::emitter::structure::emit_structures(
        &output_dir,
        &structures,
        &mut decor_blocks,
        &biome_names,
        &placed_names,
    )
    .unwrap_or_else(|e| panic!("Failed to emit structures: {e}"));
    decor_blocks
        .emit(&output_dir)
        .unwrap_or_else(|e| panic!("Failed to emit Java block data: {e}"));

    println!("cargo:warning=Generated worldgen code in {:?}", output_dir);
}
