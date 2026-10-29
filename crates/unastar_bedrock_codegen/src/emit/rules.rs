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
//!
//! Every rule runs on its own random streams: the scatter and the placed
//! feature each start from `feature_rule_seed(population_seed, hash)`, where
//! `hash` is [`rule_hash`] of the identifier, folded here at build time. A
//! rule's placements therefore do not depend on which other rules ran before
//! it in the chunk (BDS probe rules, see the codegen README).

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

/// Low 32 bits of the 64-bit FNV-1 hash (multiply, then xor) of a rule
/// identifier, the per-rule term of its random seed.
fn rule_hash(identifier: &str) -> u32 {
    identifier.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        h.wrapping_mul(0x0000_0100_0000_01b3) ^ u64::from(b)
    }) as u32
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
            calls.push(quote!(if biomes.intersects(&[#a, #b, #c, #d]) {
                #name(world, noise, world_seed, origin, population_seed);
            }));
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
                if biome_allowed(&[#a, #b, #c, #d], ctx.world.biome(p)) {
                    let ctx = &mut Ctx {
                        world: &mut *ctx.world,
                        rng: &mut feature_rng,
                        noise: ctx.noise,
                        seed: ctx.seed,
                    };
                    #place
                } else {
                    None
                }
            })
        };
        let body = scatter::emit(em, &rule.source, &rule.scatter, call)?;
        let hash = rule_hash(&rule.identifier);
        let doc = format!(
            " `{}` from `{}`: places `{}`.",
            rule.identifier, rule.source, rule.places_feature
        );
        fns.push(quote! {
            #[doc = #doc]
            #[inline(never)]
            fn #name<W: BlockWorld>(
                world: &mut W,
                noise: &dyn NoiseSource,
                world_seed: i64,
                origin: BlockPos,
                population_seed: u32,
            ) -> Option<BlockPos> {
                let seed = feature_rule_seed(population_seed, #hash);
                let mut scatter_rng = BedrockRandom::new(seed);
                let mut feature_rng = BedrockRandom::new(seed);
                let ctx = &mut Ctx { world, rng: &mut scatter_rng, noise, seed: world_seed };
                #body
            }
        });
    }
    Ok(quote! {
        /// Run every rule of `pass` that applies to a biome in `biomes`, for the
        /// chunk whose origin (minimum x/z corner, y = 0) is `origin` and whose
        /// population seed is `population_seed`.
        #[allow(unreachable_patterns, clippy::match_single_binding, unused_variables)]
        pub fn run_pass<W: BlockWorld>(
            pass: super::Pass,
            world: &mut W,
            noise: &dyn NoiseSource,
            world_seed: i64,
            origin: BlockPos,
            biomes: &super::BiomeSet,
            population_seed: u32,
        ) {
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

#[cfg(test)]
mod tests {
    use super::rule_hash;

    #[test]
    fn rule_hash_is_low_word_of_fnv1_64() {
        // Full 64-bit FNV-1 values: 0xc094_ccdc_da69_9877, 0x1d59_dd83_86a6_f51c.
        assert_eq!(rule_hash("wg_seed:c_probe"), 0xda69_9877);
        assert_eq!(rule_hash("wg_seed:d_probe"), 0x86a6_f51c);
        assert_eq!(rule_hash(""), 0x8422_2325);
    }
}
