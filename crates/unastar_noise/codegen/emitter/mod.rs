//! Emitters: IR -> Rust modules in `OUT_DIR`.
//!
//! Each data family has one emitter module here that turns its IR into a `TokenStream`.
//! [`GeneratedModules`] pretty-prints it to `OUT_DIR/<module>.rs` and records how
//! `OUT_DIR/mod.rs` (included by `src/lib.rs`) exposes it. New families add one
//! `modules.public(..)` line to [`emit_all`] (a named module, so generated items stay easy
//! to find); older families still glob re-export modules they write themselves.

pub mod biome_features;
pub mod biome_source;
pub mod carver;
pub mod emitter_quote;
pub mod feature;
pub mod java_biomes;
pub mod java_blocks;
pub mod noise;
pub mod settings;
pub mod structure;
pub mod surface_rule;

use super::analyzer::DependencyGraph;
use super::data::java_blocks::JavaBlocks;
use super::parser;
use proc_macro2::TokenStream;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// Everything the emitters read.
pub struct Inputs<'a> {
    pub noises: &'a HashMap<String, parser::noise::NoiseParams>,
    pub density_functions: &'a HashMap<String, parser::density_function::DensityFunctionArg>,
    pub noise_settings: &'a HashMap<String, parser::noise_settings::NoiseSettings>,
    pub biomes: &'a HashMap<String, parser::biome::BiomeJson>,
    pub java_blocks: &'a JavaBlocks,
    /// `java_data/bedrock_biomes.json`.
    pub bedrock_biome_ids: &'a BTreeMap<String, u32>,
    /// `java_data/biome_parameters_overworld.json`.
    pub biome_parameters: &'a super::data::biome_parameters::BiomeParameters,
    /// `worldgen_data/configured_carver`.
    pub carvers: &'a BTreeMap<String, parser::carver::ConfiguredCarver>,
    /// `java_data/tags/block`.
    pub block_tags: &'a super::data::block_tags::BlockTags,
}

/// Generated modules written to `OUT_DIR` and their `mod.rs` declarations.
pub struct GeneratedModules {
    out_dir: PathBuf,
    declarations: Vec<String>,
}

impl GeneratedModules {
    pub fn new(out_dir: &Path) -> Self {
        Self {
            out_dir: out_dir.to_path_buf(),
            declarations: Vec::new(),
        }
    }

    /// Write `tokens` to `<name>.rs` and expose it as `pub mod <name>`.
    pub fn public(&mut self, name: &str, tokens: TokenStream) -> Result<(), String> {
        self.write(name, tokens)?;
        self.declarations.push(format!("pub mod {name};"));
        Ok(())
    }

    /// Write `tokens` to `<name>.rs` and glob re-export it at the crate root (for modules
    /// the router code refers to unqualified).
    pub fn glob(&mut self, name: &str, tokens: TokenStream) -> Result<(), String> {
        self.write(name, tokens)?;
        self.glob_existing(name);
        Ok(())
    }

    /// Glob re-export a module an older emitter wrote itself.
    pub fn glob_existing(&mut self, name: &str) {
        self.declarations
            .push(format!("mod {name};\npub use {name}::*;"));
    }

    fn write(&self, name: &str, tokens: TokenStream) -> Result<(), String> {
        let file: syn::File = syn::parse2(tokens)
            .map_err(|e| format!("generated module {name} does not parse: {e}"))?;
        std::fs::write(
            self.out_dir.join(format!("{name}.rs")),
            prettyplease::unparse(&file),
        )
        .map_err(|e| format!("write {name}.rs: {e}"))
    }

    /// Write `mod.rs`.
    pub fn finish(self) -> Result<(), String> {
        let mut out =
            String::from("// Generated worldgen code; regenerated at build time. Do not edit.\n\n");
        for declaration in &self.declarations {
            out.push_str(declaration);
            out.push('\n');
        }
        std::fs::write(self.out_dir.join("mod.rs"), out).map_err(|e| format!("write mod.rs: {e}"))
    }
}

pub fn emit_all(output_dir: &Path, inputs: &Inputs<'_>) -> Result<(), Box<dyn std::error::Error>> {
    let mut modules = GeneratedModules::new(output_dir);

    modules.glob("noise_params", noise::emit_noise_params(inputs.noises)?)?;
    modules.public("random_hashes", noise::emit_random_hashes())?;

    let overworld = inputs
        .noise_settings
        .get("minecraft:overworld")
        .ok_or("noise_settings/overworld.json is missing")?;
    modules.public(
        "overworld_settings",
        settings::emit(overworld, inputs.density_functions, inputs.java_blocks)?,
    )?;

    // Overworld router, AOT compiled.
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
    let graph = DependencyGraph::build(&router_fields, inputs.density_functions);
    let mut emitter = emitter_quote::AotEmitter::new(&graph).with_geometry(
        overworld.noise.min_y,
        overworld.noise.height,
        overworld.noise.size_horizontal * 4,
        overworld.noise.size_vertical * 4,
    );
    modules.glob("overworld_compiled", emitter.emit_module())?;

    biome_features::emit_biome_features(output_dir, inputs.biomes)?;
    modules.glob_existing("biome_features");

    modules.public(
        "overworld_surface",
        surface_rule::emit(overworld, inputs.java_blocks)?,
    )?;

    modules.public("java_blocks", java_blocks::emit(inputs.java_blocks))?;
    modules.public(
        "java_biomes",
        java_biomes::emit(inputs.biomes, inputs.bedrock_biome_ids)?,
    )?;
    modules.public(
        "overworld_biome_source",
        biome_source::emit(inputs.biome_parameters)?,
    )?;
    modules.public(
        "carvers",
        carver::emit(
            inputs.carvers,
            inputs.biomes,
            inputs.biome_parameters,
            inputs.block_tags,
            inputs.java_blocks,
            overworld.noise.min_y,
            overworld.noise.height,
        )?,
    )?;

    modules.finish()?;
    Ok(())
}
