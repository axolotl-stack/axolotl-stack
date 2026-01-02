pub mod biome_features;
pub mod emitter_quote;
pub mod noise;
pub mod surface_rule;

use super::analyzer::DependencyGraph;
use super::parser;
use std::collections::HashMap;
use std::path::Path;

pub fn emit_all(
    output_dir: &Path,
    noises: &HashMap<String, parser::noise::NoiseParams>,
    density_functions: &HashMap<String, parser::density_function::DensityFunctionArg>,
    noise_settings: &HashMap<String, parser::noise_settings::NoiseSettings>,
    biomes: &HashMap<String, parser::biome::BiomeJson>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Generate noise_params.rs (dynamic - from JSON)
    noise::emit_noise_params(output_dir, noises)?;

    // Generate overworld_compiled.rs (AOT compiled - this is the only thing we need now)
    if let Some(overworld) = noise_settings.get("minecraft:overworld") {
        let router = &overworld.noise_router;
        let router_fields: Vec<(&str, &parser::density_function::DensityFunctionArg)> = vec![
            ("barrier", &router.barrier),
            ("continents", &router.continents),
            ("depth", &router.depth),
            ("erosion", &router.erosion),
            ("final_density", &router.final_density),
            ("fluid_level_floodedness", &router.fluid_level_floodedness),
            ("fluid_level_spread", &router.fluid_level_spread),
            ("lava", &router.lava),
            // preliminary_surface_level is a find_top_surface that computes surface Y level
            ("preliminary_surface_level", &router.preliminary_surface_level),
            ("ridges", &router.ridges),
            ("temperature", &router.temperature),
            ("vegetation", &router.vegetation),
            ("vein_gap", &router.vein_gap),
            ("vein_ridged", &router.vein_ridged),
            ("vein_toggle", &router.vein_toggle),
        ];

        let graph = DependencyGraph::build(&router_fields, density_functions);

        // Use the quote-based emitter for clean, type-safe code generation
        let mut emitter = emitter_quote::AotEmitter::new(&graph);
        let compiled_code = emitter.emit_module();

        std::fs::write(output_dir.join("overworld_compiled.rs"), compiled_code)?;
    }

    // Generate biome_features.rs
    biome_features::emit_biome_features(output_dir, biomes)?;

    // Generate surface_rules.rs from overworld surface_rule
    if let Some(overworld) = noise_settings.get("minecraft:overworld") {
        surface_rule::emit_surface_rules(output_dir, &overworld.surface_rule)?;
    }

    // Generate mod.rs - use regular comments instead of doc comments for include!() compatibility
    let mod_content = r#"// Generated worldgen code.
// Do not edit manually - regenerated at build time from worldgen JSON.

mod biome_features;
mod noise_params;
mod overworld_compiled;
mod surface_rules;

pub use biome_features::*;
pub use noise_params::*;
pub use overworld_compiled::*;
pub use surface_rules::*;
"#;
    std::fs::write(output_dir.join("mod.rs"), mod_content)?;

    Ok(())
}
