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
    for entry in walkdir::WalkDir::new(&json_root) {
        if let Ok(e) = entry {
            if e.path().extension().is_some_and(|ext| ext == "json") {
                println!("cargo:rerun-if-changed={}", e.path().display());
            }
        }
    }

    // Also rerun if the codegen code changes
    println!("cargo:rerun-if-changed=codegen");

    // Parse all JSON
    let noises = codegen::parser::noise::parse_all(&json_root.join("noise"))
        .expect("Failed to parse noise definitions");
    let density_functions = codegen::parser::density_function::parse_all(&json_root.join("density_function"))
        .expect("Failed to parse density functions");
    let noise_settings = codegen::parser::noise_settings::parse_all(&json_root.join("noise_settings"))
        .expect("Failed to parse noise settings");
    let biomes = codegen::parser::biome::parse_all(&json_root.join("biome"))
        .expect("Failed to parse biome definitions");

    println!("cargo:warning=Parsed {} noise definitions", noises.len());
    println!("cargo:warning=Parsed {} density functions", density_functions.len());
    println!("cargo:warning=Parsed {} noise settings", noise_settings.len());
    println!("cargo:warning=Parsed {} biome definitions", biomes.len());

    // Generate Rust code
    codegen::emitter::emit_all(&output_dir, &noises, &density_functions, &noise_settings, &biomes)
        .expect("Failed to emit generated code");

    println!("cargo:warning=Generated worldgen code in {:?}", output_dir);
}
