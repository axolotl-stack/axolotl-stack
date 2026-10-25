//! Per-biome generation data: surface builder parameters, material
//! adjustments and mountain parameters, as one `static` table indexed by
//! numeric biome id (`biome(id)` is a compiled `match`).

use super::Emitter;
use crate::error::{Error, Result};
use crate::ir::{Biome, MaterialAdjustment, Materials, SurfaceBuilder};
use crate::molang::{self, Var};
use proc_macro2::TokenStream;
use quote::quote;

fn materials(em: &mut Emitter<'_>, m: &Materials) -> TokenStream {
    let (top, mid, sea_floor, foundation, sea) = (
        em.block(m.top),
        em.block(m.mid),
        em.block(m.sea_floor),
        em.block(m.foundation),
        em.block(m.sea),
    );
    let depth = m.sea_floor_depth;
    quote!(surface::Materials { top: #top, mid: #mid, sea_floor: #sea_floor, foundation: #foundation, sea: #sea, sea_floor_depth: #depth })
}

fn builder(em: &mut Emitter<'_>, b: &SurfaceBuilder) -> TokenStream {
    match b {
        SurfaceBuilder::Overworld(m) => {
            let m = materials(em, m);
            quote!(surface::Builder::Overworld(#m))
        }
        SurfaceBuilder::FrozenOcean(m) => {
            let m = materials(em, m);
            quote!(surface::Builder::FrozenOcean(#m))
        }
        SurfaceBuilder::Swamp {
            materials: m,
            max_puddle_depth_below_sea_level,
        } => {
            let m = materials(em, m);
            quote!(surface::Builder::Swamp { materials: #m, max_puddle_depth_below_sea_level: #max_puddle_depth_below_sea_level })
        }
        SurfaceBuilder::Mesa {
            materials: m,
            clay,
            hard_clay,
            bryce_pillars,
            has_forest,
        } => {
            let m = materials(em, m);
            let (clay, hard_clay) = (em.block(*clay), em.block(*hard_clay));
            quote!(surface::Builder::Mesa { materials: #m, clay: #clay, hard_clay: #hard_clay, bryce_pillars: #bryce_pillars, has_forest: #has_forest })
        }
        SurfaceBuilder::Capped {
            floor,
            ceiling,
            sea,
            foundation,
            beach,
        } => {
            let floor: Vec<_> = floor.iter().map(|b| em.block(*b)).collect();
            let ceiling: Vec<_> = ceiling.iter().map(|b| em.block(*b)).collect();
            let (sea, foundation) = (em.block(*sea), em.block(*foundation));
            let beach = match beach {
                Some(b) => {
                    let b = em.block(*b);
                    quote!(Some(#b))
                }
                None => quote!(None),
            };
            quote!(surface::Builder::Capped { floor: &[#(#floor),*], ceiling: &[#(#ceiling),*], sea: #sea, foundation: #foundation, beach: #beach })
        }
        SurfaceBuilder::TheEnd => quote!(surface::Builder::TheEnd),
        SurfaceBuilder::NoiseGradient {
            noise_name,
            first_octave,
            amplitudes,
            non_replaceable,
            specifiers,
        } => {
            let test = em.matches(quote!(b), non_replaceable);
            let specs = specifiers.iter().map(|(lo, hi, b)| {
                let b = em.block(*b);
                quote!((#lo, #hi, #b))
            });
            let specs: Vec<_> = specs.collect();
            quote!(surface::Builder::NoiseGradient(surface::NoiseGradient {
                noise_name: #noise_name,
                first_octave: #first_octave,
                amplitudes: &[#(#amplitudes),*],
                non_replaceable: { fn test(b: u32) -> bool { #test } test },
                specifiers: &[#(#specs),*],
            }))
        }
    }
}

fn adjustment(em: &mut Emitter<'_>, biome: &Biome, a: &MaterialAdjustment) -> Result<TokenStream> {
    let mut opt = |b: Option<u32>| match b {
        Some(b) => {
            let b = em.block(b);
            quote!(Some(#b))
        }
        None => quote!(None),
    };
    let (top, mid, sea_floor, foundation, sea) = (
        opt(a.top),
        opt(a.mid),
        opt(a.sea_floor),
        opt(a.foundation),
        opt(a.sea),
    );
    let scale = a.noise_frequency_scale;
    let [lo, hi] = a.noise_range;
    // `variable.sea_level` is the dimension's sea level: 32 in the nether, 63 elsewhere.
    let sea_level = if biome.tags.contains("nether") {
        32.0
    } else {
        63.0
    };
    let height = match &a.height_range {
        None => quote!(None),
        Some([min, max]) => {
            let eval = |e: &molang::Expr| {
                molang::substitute(e, Var::SeaLevel, sea_level)
                    .as_const()
                    .map(|v| v.floor() as i32)
                    .ok_or_else(|| {
                        Error::data(
                            &biome.source,
                            "height_range must be constant once sea_level is known",
                        )
                    })
            };
            let (min, max) = (eval(min)?, eval(max)?);
            quote!(Some((#min, #max)))
        }
    };
    Ok(quote!(surface::Adjustment {
        top: #top, mid: #mid, sea_floor: #sea_floor, foundation: #foundation, sea: #sea,
        noise_frequency_scale: #scale, noise_range: (#lo, #hi), height_range: #height,
    }))
}

pub fn emit(em: &mut Emitter<'_>) -> Result<TokenStream> {
    let biomes: Vec<Biome> = em.model.biomes.clone();
    let mut rows = Vec::new();
    let mut arms = Vec::new();
    for (index, b) in biomes.iter().enumerate() {
        let id = b.id;
        let identifier = &b.identifier;
        let (temperature, downfall) = (b.temperature, b.downfall);
        let surface = match &b.surface {
            Some(s) => {
                let s = builder(em, s);
                quote!(Some(#s))
            }
            None => quote!(None),
        };
        let subsurface = match &b.subsurface {
            Some(s) => {
                let s = builder(em, s);
                quote!(Some(#s))
            }
            None => quote!(None),
        };
        let adjustments = b
            .adjustments
            .iter()
            .map(|a| adjustment(em, b, a))
            .collect::<Result<Vec<_>>>()?;
        let mountain = match &b.mountain {
            None => quote!(None),
            Some(m) => {
                let steep = match m.steep_material {
                    Some((block, [n, s, w, e])) => {
                        let block = em.block(block);
                        quote!(Some((#block, [#n, #s, #w, #e])))
                    }
                    None => quote!(None),
                };
                let slide = match m.top_slide_enabled {
                    Some(v) => quote!(Some(#v)),
                    None => quote!(None),
                };
                quote!(Some(surface::Mountain { steep_material: #steep, top_slide_enabled: #slide }))
            }
        };
        let frozen = b.tags.contains("frozen");
        let overworld = crate::model::OVERWORLD_TAGS
            .iter()
            .any(|t| b.tags.contains(*t));
        rows.push(quote!(surface::BiomeData {
            id: #id,
            identifier: #identifier,
            temperature: #temperature,
            downfall: #downfall,
            frozen: #frozen,
            overworld: #overworld,
            surface: #surface,
            subsurface: #subsurface,
            adjustments: &[#(#adjustments),*],
            mountain: #mountain,
        }));
        arms.push(quote!(#id => Some(&BIOMES[#index]),));
    }
    Ok(quote! {
        /// Every biome of the pack stack, sorted by identifier.
        pub static BIOMES: &[surface::BiomeData] = &[#(#rows),*];

        /// Generation data of a numeric biome id.
        pub fn biome(id: u16) -> Option<&'static surface::BiomeData> {
            match id {
                #(#arms)*
                _ => None,
            }
        }
    })
}
