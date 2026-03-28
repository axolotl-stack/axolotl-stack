pub mod biome_features;
pub mod emitter_quote;
pub mod go;
pub mod noise;
pub mod registries;
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
    configured_carvers: &HashMap<String, parser::configured_carver::ConfiguredCarverJson>,
    configured_features: &HashMap<String, parser::configured_feature::ConfiguredFeatureJson>,
    placed_features: &HashMap<String, parser::placed_feature::PlacedFeatureJson>,
    biome_source_parameter_lists: &HashMap<
        String,
        parser::multi_noise_biome_source_parameter_list::MultiNoiseBiomeSourceParameterListJson,
    >,
    processor_lists: &HashMap<String, parser::processor_list::ProcessorListJson>,
    structures: &HashMap<String, parser::structure::StructureJson>,
    structure_sets: &HashMap<String, parser::structure_set::StructureSetJson>,
    template_pools: &HashMap<String, parser::template_pool::TemplatePoolJson>,
    structure_templates: &HashMap<String, parser::structure_template::StructureTemplateAsset>,
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
            (
                "preliminary_surface_level",
                &router.preliminary_surface_level,
            ),
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

    registries::emit_worldgen_registries(
        output_dir,
        registries::RegistryAssets {
            configured_carvers,
            configured_features,
            placed_features,
            biome_source_parameter_lists,
            processor_lists,
            structures,
            structure_sets,
            template_pools,
            structure_templates,
        },
    )?;

    // Generate mod.rs - use regular comments instead of doc comments for include!() compatibility
    let mod_content = r#"// Generated worldgen code.
// Do not edit manually - regenerated at build time from worldgen JSON.

mod biome_features;
mod noise_params;
mod overworld_compiled;
mod surface_rules;
mod worldgen_registries;

pub use biome_features::*;
pub use noise_params::*;
pub use overworld_compiled::*;
pub use surface_rules::*;
pub use worldgen_registries::*;
"#;
    std::fs::write(output_dir.join("mod.rs"), mod_content)?;

    Ok(())
}

pub fn emit_all_go(
    output_dir: &Path,
    package: &str,
    noises: &HashMap<String, parser::noise::NoiseParams>,
    density_functions: &HashMap<String, parser::density_function::DensityFunctionArg>,
    noise_settings: &HashMap<String, parser::noise_settings::NoiseSettings>,
    biomes: &HashMap<String, parser::biome::BiomeJson>,
    configured_carvers: &HashMap<String, parser::configured_carver::ConfiguredCarverJson>,
    configured_features: &HashMap<String, parser::configured_feature::ConfiguredFeatureJson>,
    placed_features: &HashMap<String, parser::placed_feature::PlacedFeatureJson>,
    biome_source_parameter_lists: &HashMap<
        String,
        parser::multi_noise_biome_source_parameter_list::MultiNoiseBiomeSourceParameterListJson,
    >,
    processor_lists: &HashMap<String, parser::processor_list::ProcessorListJson>,
    structures: &HashMap<String, parser::structure::StructureJson>,
    structure_sets: &HashMap<String, parser::structure_set::StructureSetJson>,
    template_pools: &HashMap<String, parser::template_pool::TemplatePoolJson>,
    structure_templates: &HashMap<String, parser::structure_template::StructureTemplateAsset>,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(output_dir)?;

    go::emit_noise_params(output_dir, package, noises)?;
    go::emit_biome_features(output_dir, package, biomes)?;
    go::emit_feature_data(
        output_dir,
        package,
        configured_carvers,
        configured_features,
        placed_features,
    )?;
    go::emit_registry_data(
        output_dir,
        package,
        biome_source_parameter_lists,
        processor_lists,
        structures,
        structure_sets,
        template_pools,
        structure_templates,
    )?;
    go::emit_dimension_metadata(output_dir, package, noise_settings)?;

    let mut surface_rules = Vec::new();
    for (dimension, settings) in [
        ("overworld", noise_settings.get("minecraft:overworld")),
        ("nether", noise_settings.get("minecraft:nether")),
        ("end", noise_settings.get("minecraft:end")),
    ] {
        let Some(settings) = settings else {
            continue;
        };

        let router = &settings.noise_router;
        let router_fields: Vec<(&str, &parser::density_function::DensityFunctionArg)> = vec![
            ("barrier", &router.barrier),
            ("continents", &router.continents),
            ("depth", &router.depth),
            ("erosion", &router.erosion),
            ("final_density", &router.final_density),
            ("fluid_level_floodedness", &router.fluid_level_floodedness),
            ("fluid_level_spread", &router.fluid_level_spread),
            ("lava", &router.lava),
            (
                "preliminary_surface_level",
                &router.preliminary_surface_level,
            ),
            ("ridges", &router.ridges),
            ("temperature", &router.temperature),
            ("vegetation", &router.vegetation),
            ("vein_gap", &router.vein_gap),
            ("vein_ridged", &router.vein_ridged),
            ("vein_toggle", &router.vein_toggle),
        ];

        let graph = DependencyGraph::build(&router_fields, density_functions);
        go::emit_dimension_graph(output_dir, package, dimension, &graph)?;
        if dimension == "overworld" {
            go::emit_overworld_compiled(output_dir, package, &graph)?;
        }
        surface_rules.push((dimension, &settings.surface_rule));
    }

    go::emit_surface_rules(output_dir, package, &surface_rules)?;

    Ok(())
}
