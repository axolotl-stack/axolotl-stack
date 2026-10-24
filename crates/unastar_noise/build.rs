//! AOT worldgen toolchain: parse data into IR, analyze, emit Rust into `OUT_DIR`.
//!
//! Inputs: `worldgen_data/` (the pinned Java version's `data/minecraft/worldgen`, unchanged)
//! and `java_data/` (Java reports and Java->Bedrock maps, see `java_data/README.md`).
//! Layout: `codegen/parser/<family>.rs` (JSON -> IR), `codegen/data/` (non-worldgen
//! tables), `codegen/analyzer/` (density graph), `codegen/emitter/<family>.rs`
//! (IR -> `OUT_DIR/<module>.rs`, registered in `emitter::emit_all`).

mod codegen;

use std::path::PathBuf;

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
    let biomes = codegen::parser::biome::parse_all(&json_root.join("biome"))
        .expect("Failed to parse biome definitions");
    let java_blocks = codegen::data::java_blocks::JavaBlocks::load(&java_data)
        .unwrap_or_else(|e| panic!("Failed to load java_data block tables: {e}"));
    let bedrock_biome_ids = codegen::emitter::java_biomes::load_bedrock_ids(&java_data)
        .unwrap_or_else(|e| panic!("Failed to load java_data biome ids: {e}"));
    let biome_parameters = codegen::data::biome_parameters::BiomeParameters::load(&java_data)
        .unwrap_or_else(|e| panic!("Failed to load java_data biome parameters: {e}"));

    codegen::emitter::emit_all(
        &output_dir,
        &codegen::emitter::Inputs {
            noises: &noises,
            density_functions: &density_functions,
            noise_settings: &noise_settings,
            biomes: &biomes,
            java_blocks: &java_blocks,
            bedrock_biome_ids: &bedrock_biome_ids,
            biome_parameters: &biome_parameters,
        },
    )
    .expect("Failed to emit generated code");
}
