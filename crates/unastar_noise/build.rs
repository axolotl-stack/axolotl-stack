//! Build script that generates density function code from worldgen JSON.
//!
//! This script runs at build time to parse worldgen JSON files and generate
//! Rust code that is included via include!() in lib.rs.

mod codegen;

use std::path::PathBuf;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = std::env::var("OUT_DIR").unwrap();

    let json_root = PathBuf::from(&manifest_dir).join("worldgen_data");
    let output_dir = PathBuf::from(&out_dir);

    // Emit rerun-if-changed for the worldgen_data directory
    println!("cargo:rerun-if-changed=worldgen_data");

    // Also emit rerun-if-changed for individual JSON files
    for e in walkdir::WalkDir::new(&json_root).into_iter().flatten() {
        if e.path().extension().is_some_and(|ext| ext == "json") {
            println!("cargo:rerun-if-changed={}", e.path().display());
        }
    }

    // Also rerun if the codegen code changes
    println!("cargo:rerun-if-changed=codegen");

    // Parse all JSON
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
    let features = codegen::parser::feature::parse_all(&json_root)
        .unwrap_or_else(|e| panic!("Failed to parse feature definitions: {e}"));
    println!(
        "cargo:warning=Parsed {} configured / {} placed features",
        features.configured.len(),
        features.placed.len()
    );
    let structures = codegen::parser::structure::parse_all(&json_root)
        .unwrap_or_else(|e| panic!("Failed to parse structure definitions: {e}"));
    println!(
        "cargo:warning=Parsed {} structure sets / {} structures / {} pools / {} processor lists",
        structures.sets.len(),
        structures.structures.len(),
        structures.pools.len(),
        structures.processor_lists.len()
    );

    println!("cargo:warning=Parsed {} noise definitions", noises.len());
    println!(
        "cargo:warning=Parsed {} density functions",
        density_functions.len()
    );
    println!(
        "cargo:warning=Parsed {} noise settings",
        noise_settings.len()
    );
    println!("cargo:warning=Parsed {} biome definitions", biomes.len());

    // Generate Rust code
    codegen::emitter::emit_all(
        &output_dir,
        &noises,
        &density_functions,
        &noise_settings,
        &biomes,
    )
    .expect("Failed to emit generated code");

    // Java block registry for decoration (features/structures).
    println!("cargo:rerun-if-changed=java_data");
    let mut java_blocks = codegen::java_blocks::JavaBlocks::load(
        &PathBuf::from(&manifest_dir).join("java_data/blocks.json"),
        &json_root.join("tags/block"),
    )
    .unwrap_or_else(|e| panic!("Failed to load Java block data: {e}"));
    // Features intern block states and sets, so they are emitted first.
    codegen::emitter::feature::emit_features(
        &output_dir,
        &json_root,
        &biomes,
        &features,
        &mut java_blocks,
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
    let template_names = codegen::templates::emit(&output_dir, &java_blocks, jar.as_deref())
        .unwrap_or_else(|e| panic!("Failed to embed structure templates: {e}"));
    println!(
        "cargo:warning=Embedded {} structure templates",
        template_names.len()
    );
    codegen::emitter::structure::emit_structures(&output_dir, &structures, &mut java_blocks)
        .unwrap_or_else(|e| panic!("Failed to emit structures: {e}"));
    java_blocks
        .emit(&output_dir)
        .unwrap_or_else(|e| panic!("Failed to emit Java block data: {e}"));

    println!("cargo:warning=Generated worldgen code in {:?}", output_dir);
}
