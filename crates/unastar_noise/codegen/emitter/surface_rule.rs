//! Emits `overworld_surface`: the overworld surface rule compiled to straight-line Rust.
//!
//! A rule becomes statements that `return Some(block)` when it places a block and fall
//! through otherwise, so a `sequence` is its children in order and a `condition` is an
//! `if`. Conditions become expressions over [`SurfaceEnv`](crate::surface::SurfaceEnv);
//! blocks are resolved at build time to Bedrock runtime ids, vertical anchors to absolute
//! y, gradient random names to `fromHashOf` seeds.

use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::quote;
use std::collections::BTreeMap;

use crate::codegen::data::java_blocks::JavaBlocks;
use crate::codegen::data::random_hash::hash_of;
use crate::codegen::parser::biome::to_pascal_case;
use crate::codegen::parser::noise_settings::NoiseSettings;
use crate::codegen::parser::surface_rule::{
    BlockState, ConditionSource, RuleSource, VerticalAnchor,
};

struct Emitter<'a> {
    blocks: &'a JavaBlocks,
    min_y: i32,
    height: i32,
    /// Gradient random names in first-use order.
    gradients: Vec<String>,
}

fn pascal_ident(name: &str) -> Result<Ident, String> {
    let short = name
        .strip_prefix("minecraft:")
        .ok_or_else(|| format!("{name} is not namespaced"))?;
    Ok(Ident::new(&to_pascal_case(short), Span::call_site()))
}

impl Emitter<'_> {
    fn block(&self, state: &BlockState) -> Result<Literal, String> {
        let properties: BTreeMap<String, String> = state
            .properties
            .as_ref()
            .map(|p| p.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();
        let id = self.blocks.resolve(&state.name, &properties)?;
        Ok(Literal::u32_suffixed(self.blocks.bedrock(id)))
    }

    fn anchor(&self, anchor: &VerticalAnchor) -> i32 {
        match anchor {
            VerticalAnchor::Absolute { absolute } => *absolute,
            VerticalAnchor::AboveBottom { above_bottom } => self.min_y + above_bottom,
            VerticalAnchor::BelowTop { below_top } => self.min_y + self.height - 1 - below_top,
        }
    }

    fn rule(&mut self, rule: &RuleSource) -> Result<TokenStream, String> {
        Ok(match rule {
            RuleSource::Sequence { sequence } => {
                let parts = sequence
                    .iter()
                    .map(|r| self.rule(r))
                    .collect::<Result<Vec<_>, _>>()?;
                quote! { #(#parts)* }
            }
            RuleSource::Condition { if_true, then_run } => {
                let condition = self.condition(if_true)?;
                let then = self.rule(then_run)?;
                quote! { if #condition { #then } }
            }
            RuleSource::Block { result_state } => {
                let block = self.block(result_state)?;
                quote! { return Some(#block); }
            }
            RuleSource::Bandlands {} => quote! { return Some(env.band()); },
        })
    }

    fn condition(&mut self, condition: &ConditionSource) -> Result<TokenStream, String> {
        Ok(match condition {
            ConditionSource::Biome { biome_is } => {
                let biomes = biome_is
                    .iter()
                    .map(|b| pascal_ident(b))
                    .collect::<Result<Vec<_>, _>>()?;
                quote! { matches!(env.biome(), #(JavaBiome::#biomes)|*) }
            }
            ConditionSource::StoneDepth {
                offset,
                add_surface_depth,
                secondary_depth_range,
                surface_type,
            } => {
                let ceiling = match surface_type.as_str() {
                    "floor" => false,
                    "ceiling" => true,
                    other => return Err(format!("unknown stone_depth surface_type {other}")),
                };
                quote! {
                    stone_depth(env, #ceiling, #offset, #add_surface_depth, #secondary_depth_range)
                }
            }
            ConditionSource::YAbove {
                anchor,
                surface_depth_multiplier,
                add_stone_depth,
            } => {
                let y = self.anchor(anchor);
                quote! { y_above(env, #y, #surface_depth_multiplier, #add_stone_depth) }
            }
            ConditionSource::Water {
                offset,
                surface_depth_multiplier,
                add_stone_depth,
            } => {
                quote! { water(env, #offset, #surface_depth_multiplier, #add_stone_depth) }
            }
            ConditionSource::NoiseThreshold {
                noise,
                min_threshold,
                max_threshold,
            } => {
                let noise = pascal_ident(noise)?;
                let (min, max) = (*min_threshold, *max_threshold);
                quote! { noise_threshold(env, NoiseRef::#noise, #min, #max) }
            }
            ConditionSource::VerticalGradient {
                random_name,
                true_at_and_below,
                false_at_and_above,
            } => {
                let index = match self.gradients.iter().position(|g| g == random_name) {
                    Some(index) => index,
                    None => {
                        self.gradients.push(random_name.clone());
                        self.gradients.len() - 1
                    }
                };
                let (low, high) = (
                    self.anchor(true_at_and_below),
                    self.anchor(false_at_and_above),
                );
                quote! { vertical_gradient(env, #index, #low, #high) }
            }
            ConditionSource::Steep {} => quote! { env.steep() },
            ConditionSource::Hole {} => quote! { env.surface_depth() <= 0 },
            ConditionSource::AbovePreliminarySurface {} => {
                quote! { env.block_y() >= env.min_surface_level() }
            }
            ConditionSource::Temperature {} => quote! { env.cold_enough_to_snow() },
            ConditionSource::Not { invert } => {
                let inner = self.condition(invert)?;
                quote! { !(#inner) }
            }
        })
    }
}

/// Bedrock runtime id of a block's default Java state.
fn default_block(blocks: &JavaBlocks, name: &str) -> Result<Literal, String> {
    let id = blocks.resolve(name, &BTreeMap::new())?;
    Ok(Literal::u32_suffixed(blocks.bedrock(id)))
}

pub fn emit(settings: &NoiseSettings, blocks: &JavaBlocks) -> Result<TokenStream, String> {
    let mut emitter = Emitter {
        blocks,
        min_y: settings.noise.min_y,
        height: settings.noise.height,
        gradients: Vec::new(),
    };
    let body = emitter.rule(&settings.surface_rule)?;
    let gradient_count = emitter.gradients.len();
    let gradient_hashes = emitter.gradients.iter().map(|name| {
        let (lo, hi) = hash_of(name);
        let (lo, hi) = (Literal::u64_suffixed(lo), Literal::u64_suffixed(hi));
        quote! { (#lo, #hi) }
    });
    let gradient_names = emitter.gradients.iter();

    let band = |name: &str| default_block(blocks, name);
    let [terracotta, orange, yellow, brown, red, white, light_gray] = [
        band("minecraft:terracotta")?,
        band("minecraft:orange_terracotta")?,
        band("minecraft:yellow_terracotta")?,
        band("minecraft:brown_terracotta")?,
        band("minecraft:red_terracotta")?,
        band("minecraft:white_terracotta")?,
        band("minecraft:light_gray_terracotta")?,
    ];
    let packed_ice = default_block(blocks, "minecraft:packed_ice")?;
    let snow_block = default_block(blocks, "minecraft:snow_block")?;

    Ok(quote! {
        //! Overworld surface rule (`noise_settings/overworld.json`), compiled.
        //! Generated by `codegen/emitter/surface_rule.rs`.

        #![allow(clippy::nonminimal_bool, clippy::if_same_then_else, clippy::collapsible_if)]

        use crate::NoiseRef;
        use crate::java_biomes::JavaBiome;
        use crate::surface::{
            BandBlocks, SurfaceEnv, noise_threshold, stone_depth, vertical_gradient, water,
            y_above,
        };

        /// `fromHashOf` seeds of the vertical-gradient random names, by gradient index.
        pub static GRADIENT_HASHES: [(u64, u64); #gradient_count] = [#(#gradient_hashes),*];

        /// Vertical-gradient random names, by gradient index.
        pub static GRADIENT_NAMES: [&str; #gradient_count] = [#(#gradient_names),*];

        /// Terracotta colours of the badlands clay bands.
        pub const BAND_BLOCKS: BandBlocks = BandBlocks {
            terracotta: #terracotta,
            orange: #orange,
            yellow: #yellow,
            brown: #brown,
            red: #red,
            white: #white,
            light_gray: #light_gray,
        };

        /// Packed ice (frozen ocean icebergs).
        pub const PACKED_ICE: u32 = #packed_ice;
        /// Snow block (frozen ocean iceberg caps).
        pub const SNOW_BLOCK: u32 = #snow_block;

        /// The block the surface rule places at the environment's position, if any.
        pub fn apply<E: SurfaceEnv>(env: &mut E) -> Option<u32> {
            #body
            None
        }
    })
}
