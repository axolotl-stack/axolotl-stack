//! Emits dimension-specific configured carver tables, with replaceable block tags
//! compiled to canonical Java state-id bitsets.
//!
//! Every biome a dimension's native biome source can produce must agree on its carver
//! list. Differing lists fail the build rather than silently selecting the wrong list.

use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::quote;
use std::collections::{BTreeMap, HashMap};

use crate::codegen::data::block_tags::BlockTags;
use crate::codegen::data::java_blocks::JavaBlocks;
use crate::codegen::parser::biome::BiomeJson;
use crate::codegen::parser::carver::{
    ConfiguredCarver, FloatProvider, HeightProvider, TypedFloatProvider,
};
use crate::codegen::parser::surface_rule::VerticalAnchor;
pub struct Preset<'a> {
    pub name: &'a str,
    pub biomes: &'a [String],
    pub min_y: i32,
    pub height: i32,
}


struct Context<'a> {
    min_y: i32,
    height: i32,
    tags: &'a BlockTags,
    blocks: &'a JavaBlocks,
    /// Replaceable tag -> static ident of its bitset.
    bitsets: BTreeMap<String, (Ident, Vec<u64>)>,
}

impl Context<'_> {
    fn anchor(&self, anchor: &VerticalAnchor) -> i32 {
        match anchor {
            VerticalAnchor::Absolute { absolute } => *absolute,
            VerticalAnchor::AboveBottom { above_bottom } => self.min_y + above_bottom,
            VerticalAnchor::BelowTop { below_top } => self.min_y + self.height - 1 - below_top,
        }
    }

    fn float(&self, provider: &FloatProvider) -> TokenStream {
        match provider {
            FloatProvider::Constant(v)
            | FloatProvider::Typed(TypedFloatProvider::Constant { value: v }) => {
                quote! { FloatProvider::Constant(#v) }
            }
            FloatProvider::Typed(TypedFloatProvider::Uniform {
                min_inclusive,
                max_exclusive,
            }) => quote! { FloatProvider::Uniform { min: #min_inclusive, max: #max_exclusive } },
            FloatProvider::Typed(TypedFloatProvider::Trapezoid { min, max, plateau }) => {
                quote! { FloatProvider::Trapezoid { min: #min, max: #max, plateau: #plateau } }
            }
        }
    }

    fn height(&self, provider: &HeightProvider) -> TokenStream {
        match provider {
            HeightProvider::Uniform {
                min_inclusive,
                max_inclusive,
            } => {
                let (min, max) = (self.anchor(min_inclusive), self.anchor(max_inclusive));
                quote! { HeightProvider::Uniform { min: #min, max: #max } }
            }
        }
    }

    fn replaceable(&mut self, tag: &str) -> Result<Ident, String> {
        if !self.bitsets.contains_key(tag) {
            let mut bits = vec![0u64; self.blocks.state_count().div_ceil(64)];
            for name in self.tags.resolve(tag)? {
                for state in self.blocks.state_ids(&name)? {
                    let state = state as usize;
                    bits[state / 64] |= 1 << (state % 64);
                }
            }
            let ident = Ident::new(
                &format!(
                    "REPLACEABLE_{}",
                    tag.trim_start_matches("#minecraft:").to_uppercase()
                ),
                Span::call_site(),
            );
            self.bitsets.insert(tag.to_string(), (ident, bits));
        }
        Ok(self.bitsets[tag].0.clone())
    }

    fn carver(&mut self, carver: &ConfiguredCarver) -> Result<TokenStream, String> {
        Ok(match carver {
            ConfiguredCarver::Cave(c) | ConfiguredCarver::NetherCave(c) => {
                let variant = if matches!(carver, ConfiguredCarver::Cave(_)) {
                    quote! { Cave }
                } else {
                    quote! { NetherCave }
                };
                let probability = c.probability;
                let y = self.height(&c.y);
                let y_scale = self.float(&c.y_scale);
                let lava = self.anchor(&c.lava_level);
                let replaceable = self.replaceable(&c.replaceable)?;
                let horizontal = self.float(&c.horizontal_radius_multiplier);
                let vertical = self.float(&c.vertical_radius_multiplier);
                let floor = self.float(&c.floor_level);
                quote! {
                    ConfiguredCarver::#variant(CaveConfig {
                        probability: #probability,
                        y: #y,
                        y_scale: #y_scale,
                        lava_level: #lava,
                        replaceable: &#replaceable,
                        horizontal_radius_multiplier: #horizontal,
                        vertical_radius_multiplier: #vertical,
                        floor_level: #floor,
                    })
                }
            }
            ConfiguredCarver::Canyon(c) => {
                let probability = c.probability;
                let y = self.height(&c.y);
                let y_scale = self.float(&c.y_scale);
                let lava = self.anchor(&c.lava_level);
                let replaceable = self.replaceable(&c.replaceable)?;
                let rotation = self.float(&c.vertical_rotation);
                let shape = &c.shape;
                let distance = self.float(&shape.distance_factor);
                let thickness = self.float(&shape.thickness);
                let smoothness = shape.width_smoothness;
                let horizontal = self.float(&shape.horizontal_radius_factor);
                let default_factor = shape.vertical_radius_default_factor;
                let center_factor = shape.vertical_radius_center_factor;
                quote! {
                    ConfiguredCarver::Canyon(CanyonConfig {
                        probability: #probability,
                        y: #y,
                        y_scale: #y_scale,
                        lava_level: #lava,
                        replaceable: &#replaceable,
                        vertical_rotation: #rotation,
                        distance_factor: #distance,
                        thickness: #thickness,
                        width_smoothness: #smoothness,
                        horizontal_radius_factor: #horizontal,
                        vertical_radius_default_factor: #default_factor,
                        vertical_radius_center_factor: #center_factor,
                    })
                }
            }
        })
    }
}

pub fn emit(
    carvers: &BTreeMap<String, ConfiguredCarver>,
    biomes: &HashMap<String, BiomeJson>,
    presets: &[Preset<'_>],
    tags: &BlockTags,
    blocks: &JavaBlocks,
) -> Result<TokenStream, String> {
    let first = presets.first().ok_or("no carver presets supplied")?;
    let mut ctx = Context {
        min_y: first.min_y,
        height: first.height,
        tags,
        blocks,
        bitsets: BTreeMap::new(),
    };
    let mut tables = Vec::new();
    for preset in presets {
        let mut list: Option<(&str, Vec<String>)> = None;
        for name in preset.biomes {
            let short = name
                .strip_prefix("minecraft:")
                .ok_or_else(|| format!("biome {name} is not namespaced"))?;
            let biome = biomes
                .get(short)
                .ok_or_else(|| format!("{} source names unknown biome {name}", preset.name))?;
            let carvers = biome.carvers.clone().into_vec();
            match &list {
                None => list = Some((name, carvers)),
                Some((first, expected)) if *expected != carvers => {
                    return Err(format!(
                        "{} biomes {first} and {name} list different carvers; carver starts \
                         need their biome looked up per start chunk",
                        preset.name,
                    ));
                }
                Some(_) => {}
            }
        }
        let (_, names) =
            list.ok_or_else(|| format!("{} biome source has no biomes", preset.name))?;
        ctx.min_y = preset.min_y;
        ctx.height = preset.height;
        let mut entries = Vec::new();
        for name in &names {
            let carver = carvers
                .get(name)
                .ok_or_else(|| format!("unknown configured carver {name}"))?;
            entries.push(ctx.carver(carver)?);
        }
        let count = entries.len();
        let ident = Ident::new(
            &format!("{}_CARVERS", preset.name.to_uppercase()),
            Span::call_site(),
        );
        let names_ident = Ident::new(
            &format!("{}_CARVER_NAMES", preset.name.to_uppercase()),
            Span::call_site(),
        );
        let doc = format!(
            "The {} biomes' native carvers in list order (the index salts the start seed).",
            preset.name,
        );
        tables.push(quote! {
            #[doc = #doc]
            pub static #ident: [ConfiguredCarver; #count] = [#(#entries),*];
            /// Registry ids of the carvers above, in the same order.
            pub static #names_ident: [&str; #count] = [#(#names),*];
        });
    }
    let bitsets = ctx.bitsets.values().map(|(ident, bits)| {
        let len = bits.len();
        let words = bits.iter().map(|&w| Literal::u64_suffixed(w));
        quote! { static #ident: [u64; #len] = [#(#words),*]; }
    });

    Ok(quote! {
        //! Native dimension carver tables (`configured_carver/*.json`).
        //! Generated by `codegen/emitter/carver.rs`.

        use crate::carver::{
            CanyonConfig, CaveConfig, ConfiguredCarver, FloatProvider, HeightProvider,
        };

        #(#bitsets)*
        #(#tables)*
    })
}
