//! Emission of the Bedrock-style generator's compiled data.
//!
//! [`generate`] turns a [`Model`] into one Rust source file that the `unastar`
//! build script writes to `OUT_DIR/bedrock_worldgen.rs` and includes inside
//! `world::generator::bedrock::generated`. Layout of the emitted file:
//!
//! - `pub mod blocks`: one named `u32` constant per block state the data uses;
//! - `pub fn run_pass`: per placement pass, every active rule guarded by the
//!   bit mask of biomes it applies to, in execution order;
//! - `fn rule_*`: one function per feature rule, its scatter unrolled into
//!   straight-line Rust;
//! - `fn f_*`: one function per reachable JSON feature; composition features
//!   (scatter, aggregate, sequence, weighted_random, search, snap_to_surface,
//!   surface_relative_threshold) are inlined, block-placing feature types call
//!   their runtime algorithm with a `static` parameter literal;
//! - `pub static BIOMES`: per-biome surface builder parameters.
//!
//! Generated items reference the runtime through these paths inside the
//! `generated` module: `super::world`, `super::scatter`, `super::features`,
//! `super::builtin`, `super::surface`, `super::structures`, `super::BiomeSet`,
//! `super::Pass`.

mod biomes;
mod features;
mod rules;
mod scatter;
mod structures;
mod survival;

use crate::blocks::IdSet;
use crate::builtin::{BUILTIN_FEATURES, rust_name};
use crate::error::Result;
use crate::model::Model;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote};
use std::collections::{BTreeMap, BTreeSet};

/// Summary of one generation run (printed by the build script).
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub rules: usize,
    pub features: usize,
    pub builtins: usize,
    pub biomes: usize,
    pub block_constants: usize,
}

/// Shared emission state: block constant names and the reachable set.
pub struct Emitter<'m> {
    pub model: &'m Model,
    pub reachable: BTreeSet<String>,
    /// Runtime id -> constant name, filled on demand.
    blocks: BTreeMap<u32, Ident>,
    used_names: BTreeSet<String>,
}

impl<'m> Emitter<'m> {
    pub fn new(model: &'m Model) -> Result<Self> {
        Ok(Self {
            model,
            reachable: model.reachable_features(BUILTIN_FEATURES)?,
            blocks: BTreeMap::new(),
            used_names: BTreeSet::new(),
        })
    }

    /// `blocks::NAME` for a runtime id, registering a constant on first use.
    pub fn block(&mut self, id: u32) -> TokenStream {
        if let Some(name) = self.blocks.get(&id) {
            return quote!(blocks::#name);
        }
        let mut base = self.block_name(id);
        if self.used_names.contains(&base) {
            base = format!("{base}_{id}");
        }
        self.used_names.insert(base.clone());
        let ident = Ident::new(&base, Span::call_site());
        self.blocks.insert(id, ident.clone());
        quote!(blocks::#ident)
    }

    /// `OAK_LOG` for a block's first (default) state, `OAK_LOG__PILLAR_AXIS_X`
    /// for other states (only the properties that differ are named).
    fn block_name(&self, id: u32) -> String {
        let palette = &self.model.palette;
        let Some((name, first, state)) = palette.describe(id) else {
            return format!("STATE_{id}");
        };
        let mut out = sanitize(name.strip_prefix("minecraft:").unwrap_or(name)).to_ascii_uppercase();
        if id != first
            && let Some((_, _, default)) = palette.describe(first)
        {
            for (k, v) in state {
                if default.get(k) != Some(v) {
                    let key = k.strip_prefix("minecraft:").unwrap_or(k);
                    out.push_str(&format!("__{}_{}", sanitize(key), sanitize(&v.to_string())).to_ascii_uppercase());
                }
            }
        }
        out
    }

    /// A pattern matching every id of `set`, or `None` for the empty set.
    pub fn pattern(&self, set: &IdSet) -> Option<TokenStream> {
        let arms: Vec<TokenStream> = set
            .ranges()
            .iter()
            .map(|r| {
                let (a, b) = (*r.start(), *r.end());
                if a == b { quote!(#a) } else { quote!(#a..=#b) }
            })
            .collect();
        (!arms.is_empty()).then(|| quote!(#(#arms)|*))
    }

    /// A `bool` expression testing `id` against `set`.
    pub fn matches(&self, id: TokenStream, set: &IdSet) -> TokenStream {
        match self.pattern(set) {
            Some(p) => quote!(matches!(#id, #p)),
            None => quote!(false),
        }
    }

    /// A call placing feature `id` at `pos`, returning `Option<BlockPos>`.
    pub fn call(&self, id: &str, pos: TokenStream) -> TokenStream {
        if self.model.features.contains_key(id) {
            let f = feature_fn(id);
            quote!(#f(ctx, #pos))
        } else {
            let f = format_ident!("{}", rust_name(id));
            quote!(builtin::#f(ctx, #pos))
        }
    }

    fn block_items(&self) -> TokenStream {
        let consts = self.blocks.iter().map(|(id, name)| quote!(pub const #name: u32 = #id;));
        quote!(
            /// Block states used by the compiled data (canonical runtime ids).
            pub mod blocks { #(#consts)* }
        )
    }
}

/// Lower-case identifier fragment from arbitrary text.
pub fn sanitize(text: &str) -> String {
    let mut out: String = text
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// Function name of a JSON feature (`minecraft:coal_ore_feature` -> `f_coal_ore_feature`).
pub fn feature_fn(id: &str) -> Ident {
    format_ident!("f_{}", sanitize(id.strip_prefix("minecraft:").unwrap_or(id)))
}

/// The `bb` module: blocks declared in [`crate::builtin_blocks`].
fn builtin_blocks(em: &Emitter<'_>) -> Result<TokenStream> {
    use crate::blocks::{BlockDescriptor, BlockPredicate, StateValue};
    use crate::builtin_blocks::{BUILTIN_BLOCKS, Spec, Val};
    let palette = &em.model.palette;
    let origin = "builtin_blocks.rs";
    let mut items = Vec::new();
    for (name, spec) in BUILTIN_BLOCKS {
        let ident = format_ident!("{}", name);
        match spec {
            Spec::State(block, states) => {
                let mut desc = BlockDescriptor::named(block);
                for (k, v) in *states {
                    let v = match v {
                        Val::I(i) => StateValue::Int(*i),
                        Val::S(s) => StateValue::Str((*s).to_owned()),
                        Val::B(b) => StateValue::Int(i32::from(*b)),
                    };
                    desc.states.insert((*k).to_owned(), v);
                }
                let id = palette.resolve(origin, &desc)?;
                let doc = format!(" `{block}` {states:?}");
                items.push(quote!(#[doc = #doc] pub const #ident: u32 = #id;));
            }
            Spec::Blocks(names) => {
                let set = palette.all_states(origin, names)?;
                let test = em.matches(quote!(id), &set);
                items.push(quote!(#[inline] pub fn #ident(id: u32) -> bool { #test }));
            }
            Spec::Tags(tags) => {
                let set = palette.matching(origin, &BlockPredicate::AnyTag(tags.iter().map(|t| t.to_string()).collect()))?;
                let test = em.matches(quote!(id), &set);
                items.push(quote!(#[inline] pub fn #ident(id: u32) -> bool { #test }));
            }
        }
    }
    Ok(quote!(
        /// Blocks the hand-written runtime code uses (see `builtin_blocks.rs`).
        pub mod bb { #(#items)* }
    ))
}

/// The `props` module: per-state physical flags as bitsets over the palette.
fn props(model: &Model) -> TokenStream {
    let palette = &model.palette;
    let words = palette.len().div_ceil(64) as usize;
    let bitset = |pred: &dyn Fn(crate::blocks::BlockFlags) -> bool| {
        let mut bits = vec![0u64; words];
        for id in 0..palette.len() {
            if pred(palette.flags(id)) {
                bits[(id / 64) as usize] |= 1 << (id % 64);
            }
        }
        bits
    };
    let solid = bitset(&|f| f.solid);
    let liquid = bitset(&|f| f.liquid);
    let motion = bitset(&|f| f.blocks_motion);
    let opaque = bitset(&|f| f.opaque_full_block);
    quote!(
        /// Physical flags per runtime id (protocolgen native BDS measurements).
        pub mod props {
            const WORDS: usize = #words;
            static SOLID: [u64; WORDS] = [#(#solid),*];
            static LIQUID: [u64; WORDS] = [#(#liquid),*];
            static BLOCKS_MOTION: [u64; WORDS] = [#(#motion),*];
            static OPAQUE: [u64; WORDS] = [#(#opaque),*];
            #[inline]
            fn bit(set: &[u64; WORDS], id: u32) -> bool {
                set.get((id / 64) as usize).is_some_and(|w| w >> (id % 64) & 1 == 1)
            }
            /// State `solid` flag.
            #[inline] pub fn is_solid(id: u32) -> bool { bit(&SOLID, id) }
            /// Material is a liquid.
            #[inline] pub fn is_liquid(id: u32) -> bool { bit(&LIQUID, id) }
            /// Material blocks motion.
            #[inline] pub fn blocks_motion(id: u32) -> bool { bit(&BLOCKS_MOTION, id) }
            /// Opaque full block.
            #[inline] pub fn is_opaque(id: u32) -> bool { bit(&OPAQUE, id) }
        }
    )
}

/// Generate the whole compiled module as formatted Rust source.
pub fn generate(model: &Model) -> Result<(String, Report)> {
    let mut em = Emitter::new(model)?;
    let rules = rules::emit(&mut em)?;
    let features = features::emit_all(&mut em)?;
    let biomes = biomes::emit(&mut em)?;
    let structures = structures::emit(&mut em)?;
    let blocks = em.block_items();
    let bb = builtin_blocks(&em)?;
    let props = props(model);
    let report = Report {
        rules: model.active_rules().len(),
        features: em.reachable.iter().filter(|f| model.features.contains_key(*f)).count(),
        builtins: em.reachable.iter().filter(|f| !model.features.contains_key(*f)).count(),
        biomes: model.overworld_biomes().count(),
        block_constants: em.blocks.len(),
    };
    let file = quote! {
        use super::world::{BlockPos, BlockWorld, Ctx};
        #[allow(unused_imports)]
        use super::{builtin, features, scatter, structures, surface};

        #blocks
        #bb
        #props
        #rules
        #features
        #biomes
        #structures
    };
    let parsed: syn::File = syn::parse2(file)
        .map_err(|e| crate::error::Error::data("emitter", format!("generated code does not parse: {e}")))?;
    let header = "// @generated by unastar_bedrock_codegen from the local BDS 1.26.51 install.\n// Do not edit: change the generator instead.\n";
    Ok((format!("{header}{}", prettyplease::unparse(&parsed)), report))
}
