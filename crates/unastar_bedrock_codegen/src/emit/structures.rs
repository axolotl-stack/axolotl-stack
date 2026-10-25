//! Emission of the structure tables (`STRUCTURE_TEMPLATES`, `TEMPLATE_POOLS`,
//! `PROCESSOR_LISTS`, `JIGSAW_STRUCTURES`, `STRUCTURE_SETS`), one compiled
//! biome filter per jigsaw structure and `place_pool_feature`, the
//! dispatcher of feature pool elements. The runtime types live in
//! `unastar::world::generator::bedrock::structures`.
//!
//! Template cells are packed into byte strings (`x, y, z` as `u8`, palette
//! index as little-endian `u16`) to keep the generated source compact;
//! jigsaw names and targets are interned to `u16` (only equality matters).

use super::Emitter;
use super::scatter::biome_tag_test;
use crate::error::{Error, Result};
use crate::ir::BiomeFilter;
use crate::structures::{
    Adaptation, Anchor, Dir6, EMPTY, ElementKind, Processor, ProcessorRule, StructureData, Template,
};
use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};
use std::collections::BTreeMap;

/// `u16` index of a table entry ("structures" tables use `u16::MAX` as none).
fn index(origin: &str, i: usize) -> Result<u16> {
    u16::try_from(i)
        .ok()
        .filter(|&v| v != u16::MAX)
        .ok_or_else(|| Error::data(origin, format!("table index {i} does not fit u16")))
}

fn none() -> u16 {
    u16::MAX
}

fn dir(d: Dir6) -> TokenStream {
    match d {
        Dir6::Down => quote!(structures::Dir6::Down),
        Dir6::Up => quote!(structures::Dir6::Up),
        Dir6::North => quote!(structures::Dir6::North),
        Dir6::South => quote!(structures::Dir6::South),
        Dir6::West => quote!(structures::Dir6::West),
        Dir6::East => quote!(structures::Dir6::East),
    }
}

fn template_tokens(
    t: &Template,
    names: &BTreeMap<&str, u16>,
    data: &StructureData,
) -> Result<TokenStream> {
    let [sx, sy, sz] = t.size;
    let palette = t
        .palette
        .iter()
        .map(|[a, b, c, d]| quote!([#a, #b, #c, #d]));
    let mut bytes = Vec::with_capacity(t.cells.len() * 5);
    for ([x, y, z], entry) in &t.cells {
        bytes.extend_from_slice(&[*x, *y, *z]);
        bytes.extend_from_slice(&entry.to_le_bytes());
    }
    let cells = Literal::byte_string(&bytes);
    let mut connectors = Vec::new();
    for c in &t.connectors {
        let [x, y, z] = c.pos;
        let (front, top) = (dir(c.front), dir(c.top));
        let name = names[c.name.as_str()];
        let target = names[c.target.as_str()];
        let pool = if c.pool == EMPTY {
            none()
        } else {
            let i = *data.pool_index.get(&c.pool).ok_or_else(|| {
                Error::data(&t.origin, format!("jigsaw names unknown pool {}", c.pool))
            })?;
            index(&t.origin, i)?
        };
        let (rollable, selection, placement) =
            (c.rollable, c.selection_priority, c.placement_priority);
        connectors.push(quote!(structures::Connector {
            pos: [#x, #y, #z],
            front: #front,
            top: #top,
            name: #name,
            target: #target,
            pool: #pool,
            rollable: #rollable,
            selection_priority: #selection,
            placement_priority: #placement,
        }));
    }
    Ok(quote!(structures::Template {
        size: [#sx, #sy, #sz],
        palette: &[#(#palette),*],
        cells: #cells,
        connectors: &[#(#connectors),*],
    }))
}

fn rules_tokens(rules: &[ProcessorRule]) -> TokenStream {
    let items = rules.iter().map(|r| {
        let (first, last) = r.input;
        let (probability, output) = (r.probability, r.output);
        quote!(structures::ProcessorRule {
            input_first: #first,
            input_last: #last,
            probability: #probability,
            output: #output,
        })
    });
    quote!(&[#(#items),*])
}

/// `bool` expression of a biome filter over `biome: u16`.
fn filter_tokens(em: &Emitter<'_>, f: &BiomeFilter) -> TokenStream {
    match f {
        BiomeFilter::All(items) => {
            let parts = items.iter().map(|i| filter_tokens(em, i));
            quote!((true #(&& #parts)*))
        }
        BiomeFilter::Any(items) => {
            let parts = items.iter().map(|i| filter_tokens(em, i));
            quote!((false #(|| #parts)*))
        }
        BiomeFilter::HasTag { tag, negate } => {
            let test = biome_tag_test(em, tag, quote!(biome));
            if *negate { quote!(!(#test)) } else { test }
        }
    }
}

/// Emit every structure table and `place_pool_feature`.
pub fn emit(em: &mut Emitter<'_>) -> Result<TokenStream> {
    let model = em.model;
    let data = &model.structures;

    let mut names: BTreeMap<&str, u16> = BTreeMap::new();
    for t in &data.templates {
        for c in &t.connectors {
            for n in [c.name.as_str(), c.target.as_str()] {
                let next = index(&t.origin, names.len())?;
                names.entry(n).or_insert(next);
            }
        }
    }

    let templates = data
        .templates
        .iter()
        .map(|t| template_tokens(t, &names, data))
        .collect::<Result<Vec<_>>>()?;

    let mut pools = Vec::new();
    for pool in &data.pools {
        let mut elements = Vec::new();
        for e in &pool.elements {
            let (template, processors, feature) = match e.kind {
                ElementKind::Template {
                    template,
                    processors,
                } => (
                    index(&pool.source, template)?,
                    match processors {
                        Some(p) => index(&pool.source, p)?,
                        None => none(),
                    },
                    none(),
                ),
                ElementKind::Feature { template, feature } => (
                    index(&pool.source, template)?,
                    none(),
                    index(&pool.source, feature)?,
                ),
                ElementKind::Empty => (none(), none(), none()),
            };
            let weight = u16::try_from(e.weight)
                .map_err(|_| Error::data(&pool.source, "pool weight overflow"))?;
            elements.push(quote!(structures::Element {
                weight: #weight,
                template: #template,
                processors: #processors,
                feature: #feature,
            }));
        }
        let fallback = match pool.fallback {
            Some(f) => index(&pool.source, f)?,
            None => none(),
        };
        pools.push(quote!(structures::Pool {
            elements: &[#(#elements),*],
            fallback: #fallback,
        }));
    }

    let processors = data.processors.iter().map(|list| {
        let items = list.processors.iter().map(|p| match p {
            Processor::Rules(rules) => {
                let rules = rules_tokens(rules);
                quote!(structures::Processor::Rules(#rules))
            }
            Processor::Capped { limit, rules } => {
                let limit = *limit as u16;
                let rules = rules_tokens(rules);
                quote!(structures::Processor::Capped { limit: #limit, rules: #rules })
            }
        });
        quote!(structures::ProcessorList { processors: &[#(#items),*] })
    });
    let processors: Vec<TokenStream> = processors.collect();

    let extent = data.max_template_extent();
    let mut structures = Vec::new();
    let mut filters = Vec::new();
    for (i, s) in data.structures.iter().enumerate() {
        let f = format_ident!("structure_biome_{}", i);
        let test = filter_tokens(em, &s.biome_filter);
        let doc = format!(" `biome_filters` of `{}` ({}).", s.identifier, s.source);
        filters.push(quote! {
            #[doc = #doc]
            #[allow(unused_variables)]
            fn #f(biome: u16) -> bool { #test }
        });
        let start_pool = index(&s.source, s.start_pool)?;
        let max_depth =
            u8::try_from(s.max_depth).map_err(|_| Error::data(&s.source, "max_depth overflow"))?;
        let (horizontal, vertical) = (s.horizontal, s.vertical);
        let start_height = match s.start_height {
            Anchor::Absolute(y) => quote!(structures::Anchor::Absolute(#y)),
            Anchor::AboveBottom(y) => quote!(structures::Anchor::AboveBottom(#y)),
            Anchor::BelowTop(y) => quote!(structures::Anchor::BelowTop(#y)),
        };
        let project = s.project_to_surface;
        let (padding_bottom, padding_top) = (s.padding_bottom, s.padding_top);
        let adaptation = match s.adaptation {
            Adaptation::None => quote!(structures::Adaptation::None),
            Adaptation::Bury => quote!(structures::Adaptation::Bury),
            Adaptation::BeardThin => quote!(structures::Adaptation::BeardThin),
            Adaptation::BeardBox => quote!(structures::Adaptation::BeardBox),
            Adaptation::Encapsulate => quote!(structures::Adaptation::Encapsulate),
        };
        let step = s.step;
        // Every piece lies within `horizontal` of the start piece's centre,
        // which lies within one template extent of the start chunk corner.
        let reach_blocks = if s.max_depth > 0 {
            s.horizontal + extent
        } else {
            2 * extent
        };
        let reach = (reach_blocks + 15) / 16 + 1;
        structures.push(quote!(structures::Structure {
            start_pool: #start_pool,
            max_depth: #max_depth,
            horizontal: #horizontal,
            vertical: #vertical,
            start_height: #start_height,
            project_to_surface: #project,
            padding_bottom: #padding_bottom,
            padding_top: #padding_top,
            adaptation: #adaptation,
            step: #step,
            biome: #f,
            reach: #reach,
        }));
    }

    let mut sets = Vec::new();
    for set in &data.sets {
        let mut entries = Vec::new();
        for &(s, w) in &set.structures {
            let s = index(&set.source, s)?;
            let w = u16::try_from(w)
                .map_err(|_| Error::data(&set.source, "structure weight overflow"))?;
            entries.push(quote!((#s, #w)));
        }
        let (spacing, separation, salt, triangular) =
            (set.spacing, set.separation, set.salt, set.triangular);
        sets.push(quote!(structures::StructureSet {
            structures: &[#(#entries),*],
            spacing: #spacing,
            separation: #separation,
            salt: #salt,
            triangular: #triangular,
        }));
    }

    let mut arms = Vec::new();
    for (i, id) in data.pool_features.iter().enumerate() {
        let i = index("worldgen/template_pools", i)?;
        let call = em.call(id, quote!(pos));
        arms.push(quote!(#i => #call,));
    }

    let (nt, np, nl, ns, nset) = (
        templates.len(),
        pools.len(),
        processors.len(),
        structures.len(),
        sets.len(),
    );
    Ok(quote! {
        /// Structure templates: jigsaw pieces, feature-element shapes, fossils
        /// and `structure_template_feature` templates.
        pub static STRUCTURE_TEMPLATES: [structures::Template; #nt] = [#(#templates),*];
        /// `worldgen/template_pools`.
        pub static TEMPLATE_POOLS: [structures::Pool; #np] = [#(#pools),*];
        /// `worldgen/processors`.
        pub static PROCESSOR_LISTS: [structures::ProcessorList; #nl] = [#(#processors),*];
        /// `worldgen/structures` (jigsaw structures).
        pub static JIGSAW_STRUCTURES: [structures::Structure; #ns] = [#(#structures),*];
        /// `worldgen/structure_sets`.
        pub static STRUCTURE_SETS: [structures::StructureSet; #nset] = [#(#sets),*];

        #(#filters)*

        /// Place the feature of a feature pool element (`Element::feature`).
        #[allow(unused_variables)]
        pub fn place_pool_feature<W: BlockWorld>(ctx: &mut Ctx<'_, W>, index: u16, pos: BlockPos) -> Option<BlockPos> {
            match index {
                #(#arms)*
                _ => None,
            }
        }
    })
}
