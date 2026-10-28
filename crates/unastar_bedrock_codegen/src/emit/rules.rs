//! Feature rules: one function per active rule plus the per-pass dispatcher.
//!
//! A chunk runs, per placement pass, every rule whose biome filter matches
//! any biome present in the chunk, each once, in identifier order. Each
//! distributed placement is then checked again against the biome at its own
//! position: a rule for an underground biome (lush caves) must not decorate
//! the whole chunk volume just because one pocket of that biome exists. A
//! chunk-wide check scattered lush-cave clay ore through mountain stone up to
//! y 145 and generated about twice the BDS fixture's clay, moss and cave vines
//! (see the codegen README). Biome filters are resolved at build time into
//! 256-bit masks over numeric biome ids.

use super::{Emitter, sanitize, scatter};
use crate::error::{Error, Result};
use crate::ir::{FeatureRule, Pass};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

fn rule_fn(rule: &FeatureRule) -> Ident {
    format_ident!(
        "rule_{}",
        sanitize(
            rule.identifier
                .strip_prefix("minecraft:")
                .unwrap_or(&rule.identifier)
        )
    )
}

fn mask(em: &Emitter<'_>, rule: &FeatureRule) -> Result<[u64; 4]> {
    let mut words = [0u64; 4];
    for biome in em.model.overworld_biomes() {
        if rule
            .biome_filter
            .as_ref()
            .is_none_or(|f| f.matches(&biome.tags))
        {
            let id = biome.id as usize;
            if id >= 256 {
                return Err(Error::data(
                    &biome.source,
                    format!("biome id {id} does not fit the 256-bit mask"),
                ));
            }
            words[id / 64] |= 1 << (id % 64);
        }
    }
    Ok(words)
}

pub fn emit(em: &mut Emitter<'_>) -> Result<TokenStream> {
    let active: Vec<FeatureRule> = em.model.active_rules().into_iter().cloned().collect();
    let mut fns = Vec::new();
    let mut arms = Vec::new();
    for pass in Pass::ALL {
        let mut calls = Vec::new();
        for rule in active.iter().filter(|r| r.pass == pass) {
            let name = rule_fn(rule);
            let [a, b, c, d] = mask(em, rule)?;
            calls.push(quote!(if biomes.intersects(&[#a, #b, #c, #d]) { #name(ctx, origin); }));
        }
        if !calls.is_empty() {
            let variant = format_ident!("{}", pass.variant());
            arms.push(quote!(super::Pass::#variant => { #(#calls)* }));
        }
    }
    for rule in &active {
        let name = rule_fn(rule);
        let [a, b, c, d] = mask(em, rule)?;
        let call = |pos: TokenStream| {
            let place = em.call(&rule.places_feature, quote!(p));
            quote!({
                let p = #pos;
                if biome_allowed(&[#a, #b, #c, #d], ctx.world.biome(p)) { #place } else { None }
            })
        };
        let body = scatter::emit(em, &rule.source, &rule.scatter, call)?;
        let doc = format!(
            " `{}` from `{}`: places `{}`.",
            rule.identifier, rule.source, rule.places_feature
        );
        fns.push(quote! {
            #[doc = #doc]
            #[inline(never)]
            fn #name<W: BlockWorld>(ctx: &mut Ctx<'_, W>, origin: BlockPos) -> Option<BlockPos> #body
        });
    }
    Ok(quote! {
        /// Run every rule of `pass` that applies to a biome in `biomes`, for the
        /// chunk whose origin (minimum x/z corner, y = 0) is `origin`.
        #[allow(unreachable_patterns, clippy::match_single_binding)]
        pub fn run_pass<W: BlockWorld>(pass: super::Pass, ctx: &mut Ctx<'_, W>, origin: BlockPos, biomes: &super::BiomeSet) {
            match pass {
                #(#arms)*
                _ => {}
            }
        }
        /// Whether numeric biome `id` is in the 256-bit `mask`.
        #[inline]
        fn biome_allowed(mask: &[u64; 4], id: u16) -> bool {
            mask.get(usize::from(id) / 64).is_some_and(|w| w >> (id % 64) & 1 == 1)
        }
        #(#fns)*
    })
}
