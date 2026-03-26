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
    let structure_root = PathBuf::from(&manifest_dir).join("structure_data");
    let output_dir = PathBuf::from(&out_dir);

    // Emit rerun-if-changed for the worldgen_data directory
    println!("cargo:rerun-if-changed=worldgen_data");
    println!("cargo:rerun-if-changed=structure_data");

    // Also emit rerun-if-changed for individual JSON files
    for e in walkdir::WalkDir::new(&json_root).into_iter().flatten() {
        if e.path().extension().is_some_and(|ext| ext == "json") {
            println!("cargo:rerun-if-changed={}", e.path().display());
        }
    }
    for e in walkdir::WalkDir::new(&structure_root).into_iter().flatten() {
        if e.path().extension().is_some_and(|ext| ext == "nbt") {
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
    let configured_carvers =
        codegen::parser::configured_carver::parse_all(&json_root.join("configured_carver"))
            .expect("Failed to parse configured carvers");
    let configured_features =
        codegen::parser::configured_feature::parse_all(&json_root.join("configured_feature"))
            .expect("Failed to parse configured features");
    let placed_features =
        codegen::parser::placed_feature::parse_all(&json_root.join("placed_feature"))
            .expect("Failed to parse placed features");
    let biome_source_parameter_lists =
        codegen::parser::multi_noise_biome_source_parameter_list::parse_all(
            &json_root.join("multi_noise_biome_source_parameter_list"),
        )
        .expect("Failed to parse multi-noise biome source parameter lists");
    let processor_lists =
        codegen::parser::processor_list::parse_all(&json_root.join("processor_list"))
            .expect("Failed to parse processor lists");
    let structures = codegen::parser::structure::parse_all(&json_root.join("structure"))
        .expect("Failed to parse structure definitions");
    let structure_sets =
        codegen::parser::structure_set::parse_all(&json_root.join("structure_set"))
            .expect("Failed to parse structure sets");
    let template_pools =
        codegen::parser::template_pool::parse_all(&json_root.join("template_pool"))
            .expect("Failed to parse template pools");
    let structure_templates = codegen::parser::structure_template::parse_all(&structure_root)
        .expect("Failed to collect structure templates");

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
    println!(
        "cargo:warning=Parsed {} configured carvers",
        configured_carvers.len()
    );
    println!(
        "cargo:warning=Parsed {} configured features",
        configured_features.len()
    );
    println!(
        "cargo:warning=Parsed {} placed features",
        placed_features.len()
    );
    println!(
        "cargo:warning=Parsed {} biome source parameter lists",
        biome_source_parameter_lists.len()
    );
    println!(
        "cargo:warning=Parsed {} processor lists",
        processor_lists.len()
    );
    println!("cargo:warning=Parsed {} structures", structures.len());
    println!(
        "cargo:warning=Parsed {} structure sets",
        structure_sets.len()
    );
    println!(
        "cargo:warning=Parsed {} template pools",
        template_pools.len()
    );
    println!(
        "cargo:warning=Collected {} structure templates",
        structure_templates.len()
    );

    // Generate Rust code
    codegen::emitter::emit_all(
        &output_dir,
        &noises,
        &density_functions,
        &noise_settings,
        &biomes,
        &configured_carvers,
        &configured_features,
        &placed_features,
        &biome_source_parameter_lists,
        &processor_lists,
        &structures,
        &structure_sets,
        &template_pools,
        &structure_templates,
    )
    .expect("Failed to emit generated code");

    if let Ok(go_out_dir) = std::env::var("UNASTAR_NOISE_GO_OUT") {
        let go_package =
            std::env::var("UNASTAR_NOISE_GO_PACKAGE").unwrap_or_else(|_| "gen".to_string());
        codegen::emitter::emit_all_go(
            &PathBuf::from(go_out_dir),
            &go_package,
            &noises,
            &density_functions,
            &noise_settings,
            &biomes,
            &configured_carvers,
            &configured_features,
            &placed_features,
        )
        .expect("Failed to emit Go worldgen code");
        println!("cargo:warning=Generated Go worldgen code");
    }

    println!("cargo:warning=Generated worldgen code in {:?}", output_dir);
}
