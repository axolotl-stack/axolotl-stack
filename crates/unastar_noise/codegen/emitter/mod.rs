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
use quote::quote;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// Everything the emitters read.
pub struct Inputs<'a> {
    pub noises: &'a HashMap<String, parser::noise::NoiseParams>,
    pub density_functions: &'a HashMap<String, parser::density_function::DensityFunctionArg>,
    pub noise_settings: &'a HashMap<String, parser::noise_settings::NoiseSettings>,
    pub dimension_types: &'a HashMap<String, parser::noise_settings::DimensionType>,
    pub biomes: &'a HashMap<String, parser::biome::BiomeJson>,
    pub java_blocks: &'a JavaBlocks,
    /// `java_data/bedrock_biomes.json`.
    pub bedrock_biome_ids: &'a BTreeMap<String, u32>,
    /// `java_data/biome_parameters_overworld.json`.
    pub biome_parameters: &'a super::data::biome_parameters::BiomeParameters,
    /// `java_data/biome_parameters_nether.json`.
    pub nether_biome_parameters: &'a super::data::biome_parameters::BiomeParameters,
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

    let preset = |name: &str| {
        inputs
            .noise_settings
            .get(&format!("minecraft:{name}"))
            .ok_or_else(|| format!("noise_settings/{name}.json is missing"))
    };
    let overworld = preset("overworld")?;
    let nether = preset("nether")?;
    let end = preset("end")?;
    for (name, settings) in [("overworld", overworld), ("nether", nether), ("end", end)] {
        let dimension = inputs
            .dimension_types
            .get(&format!("minecraft:{name}"))
            .ok_or_else(|| format!("dimension type for noise settings {name} is missing"))?;
        modules.public(
            &format!("{name}_settings"),
            settings::emit(
                name,
                settings,
                dimension,
                inputs.density_functions,
                inputs.java_blocks,
            )?,
        )?;

        let router = &settings.noise_router;
        let router_fields = [
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
        let graph = DependencyGraph::build(&router_fields, inputs.density_functions);
        let mut emitter = emitter_quote::AotEmitter::new(&graph).with_geometry(
            settings.noise.min_y,
            settings.noise.height,
            settings.noise.size_horizontal * 4,
            settings.noise.size_vertical * 4,
        );
        let mut compiled: syn::File = syn::parse2(emitter.emit_module())?;
        let module_doc = format!(
            "AOT-compiled {name} noise router (regenerated at build time from worldgen JSON)."
        );
        compiled.attrs.retain(|attr| !attr.path().is_ident("doc"));
        compiled
            .attrs
            .insert(0, syn::parse_quote!(#![doc = #module_doc]));
        let compiled = quote! { #compiled };
        if name == "overworld" {
            // Preserve the existing root API; other dimensions are explicitly scoped.
            modules.glob("overworld_compiled", compiled)?;
        } else {
            modules.public(&format!("{name}_compiled"), compiled)?;
        }
        modules.public(
            &format!("{name}_surface"),
            surface_rule::emit(name, settings, inputs.java_blocks)?,
        )?;
    }

    biome_features::emit_biome_features(output_dir, inputs.biomes)?;
    modules.glob_existing("biome_features");

    modules.public("java_blocks", java_blocks::emit(inputs.java_blocks))?;
    modules.public(
        "java_biomes",
        java_biomes::emit(inputs.biomes, inputs.bedrock_biome_ids)?,
    )?;
    modules.public(
        "overworld_biome_source",
        biome_source::emit("overworld", inputs.biome_parameters)?,
    )?;
    modules.public(
        "nether_biome_source",
        biome_source::emit("nether", inputs.nether_biome_parameters)?,
    )?;

    // TheEndBiomeSource has five fixed biome slots, not multi-noise parameter points.
    let end_biomes = [
        "minecraft:the_end",
        "minecraft:end_highlands",
        "minecraft:end_midlands",
        "minecraft:small_end_islands",
        "minecraft:end_barrens",
    ]
    .map(str::to_owned);
    modules.public(
        "carvers",
        carver::emit(
            inputs.carvers,
            inputs.biomes,
            &[
                carver::Preset {
                    name: "overworld",
                    biomes: &inputs.biome_parameters.biomes,
                    min_y: overworld.noise.min_y,
                    height: overworld.noise.height,
                },
                carver::Preset {
                    name: "nether",
                    biomes: &inputs.nether_biome_parameters.biomes,
                    min_y: nether.noise.min_y,
                    height: nether.noise.height,
                },
                carver::Preset {
                    name: "end",
                    biomes: &end_biomes,
                    min_y: end.noise.min_y,
                    height: end.noise.height,
                },
            ],
            inputs.block_tags,
            inputs.java_blocks,
        )?,
    )?;

    modules.finish()?;
    Ok(())
}
