//! Emitters: IR -> Rust modules in `OUT_DIR`.
//!
//! Each data family has one emitter module here that turns its IR into a `TokenStream`.
//! [`GeneratedModules`] pretty-prints it to `OUT_DIR/<module>.rs` and records how
//! `OUT_DIR/mod.rs` (included by `src/lib.rs`) exposes it. New families add one
//! `modules.public(..)` line to [`emit_all`] (a named module, so generated items stay easy
//! to find); older families still glob re-export modules they write themselves.

pub mod biome_features;
pub mod emitter_quote;
pub mod java_blocks;
pub mod noise;
pub mod surface_rule;

use super::analyzer::DependencyGraph;
use super::data::java_blocks::JavaBlocks;
use super::parser;
use proc_macro2::TokenStream;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Everything the emitters read.
pub struct Inputs<'a> {
    pub noises: &'a HashMap<String, parser::noise::NoiseParams>,
    pub density_functions: &'a HashMap<String, parser::density_function::DensityFunctionArg>,
    pub noise_settings: &'a HashMap<String, parser::noise_settings::NoiseSettings>,
    pub biomes: &'a HashMap<String, parser::biome::BiomeJson>,
    pub java_blocks: &'a JavaBlocks,
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

    noise::emit_noise_params(output_dir, inputs.noises)?;
    modules.glob_existing("noise_params");

    let overworld = inputs
        .noise_settings
        .get("minecraft:overworld")
        .ok_or("noise_settings/overworld.json is missing")?;

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
    let mut emitter = emitter_quote::AotEmitter::new(&graph);
    std::fs::write(
        output_dir.join("overworld_compiled.rs"),
        emitter.emit_module(),
    )?;
    modules.glob_existing("overworld_compiled");

    biome_features::emit_biome_features(output_dir, inputs.biomes)?;
    modules.glob_existing("biome_features");

    surface_rule::emit_surface_rules(output_dir, &overworld.surface_rule)?;
    modules.glob_existing("surface_rules");

    modules.public("java_blocks", java_blocks::emit(inputs.java_blocks))?;

    modules.finish()?;
    Ok(())
}
