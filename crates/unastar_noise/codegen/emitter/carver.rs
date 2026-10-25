//! Emits `carvers`: the overworld's configured carvers (`configured_carver/*.json`) as typed
//! constants, with replaceable block tags compiled to Bedrock runtime-id bitsets.
//!
//! Java picks a carver-start chunk's carvers from its biome. Every biome the overworld
//! biome source can produce lists the same carvers, so the build checks that and emits one
//! list; a data change that makes the lists differ fails the build instead of silently
//! using the wrong carvers.

use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::quote;
use std::collections::{BTreeMap, HashMap};

use crate::codegen::data::biome_parameters::BiomeParameters;
use crate::codegen::data::block_tags::BlockTags;
use crate::codegen::data::java_blocks::JavaBlocks;
use crate::codegen::parser::biome::BiomeJson;
use crate::codegen::parser::carver::{
    ConfiguredCarver, FloatProvider, HeightProvider, TypedFloatProvider,
};
use crate::codegen::parser::surface_rule::VerticalAnchor;

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
            let bits = self.tags.bedrock_bitset(tag, self.blocks)?;
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

#[allow(clippy::too_many_arguments)]
pub fn emit(
    carvers: &BTreeMap<String, ConfiguredCarver>,
    biomes: &HashMap<String, BiomeJson>,
    parameters: &BiomeParameters,
    tags: &BlockTags,
    blocks: &JavaBlocks,
    min_y: i32,
    height: i32,
) -> Result<TokenStream, String> {
    // The carvers of every biome the overworld biome source can place.
    let mut list: Option<(String, Vec<String>)> = None;
    for name in &parameters.biomes {
        let short = name.trim_start_matches("minecraft:");
        let biome = biomes
            .get(short)
            .ok_or_else(|| format!("biome parameter list names unknown biome {name}"))?;
        let carvers = biome.carvers.clone().into_vec();
        match &list {
            None => list = Some((name.clone(), carvers)),
            Some((first, expected)) if *expected != carvers => {
                return Err(format!(
                    "overworld biomes {first} and {name} list different carvers; carver starts \
                     would need their biome looked up per start chunk"
                ));
            }
            Some(_) => {}
        }
    }
    let (_, names) = list.ok_or("the overworld biome parameter list is empty")?;

    let mut ctx = Context {
        min_y,
        height,
        tags,
        blocks,
        bitsets: BTreeMap::new(),
    };
    let mut entries = Vec::new();
    for name in &names {
        let carver = carvers
            .get(name)
            .ok_or_else(|| format!("unknown configured carver {name}"))?;
        entries.push(ctx.carver(carver)?);
    }
    let count = entries.len();
    let bitsets = ctx.bitsets.values().map(|(ident, bits)| {
        let len = bits.len();
        let words = bits.iter().map(|&w| Literal::u64_suffixed(w));
        quote! { static #ident: [u64; #len] = [#(#words),*]; }
    });

    Ok(quote! {
        //! Overworld configured carvers (`configured_carver/*.json`), in the order every
        //! overworld biome lists them. Generated by `codegen/emitter/carver.rs`.

        use crate::carver::{
            CanyonConfig, CaveConfig, ConfiguredCarver, FloatProvider, HeightProvider,
        };

        #(#bitsets)*

        /// The overworld biomes' carvers, in list order (the index salts the start seed).
        pub static OVERWORLD_CARVERS: [ConfiguredCarver; #count] = [#(#entries),*];
    })
}
