//! Emitter for Java decoration (`features.rs`):
//!
//! 1. the biome table (names, climate);
//! 2. one function per configured feature (`cf_*`) that calls the feature
//!    implementation in `features::kinds` with a static, fully resolved config,
//!    or inlines the selector/patch control flow;
//! 3. one function per placed feature (`pf_*`) with every placement modifier
//!    compiled to straight-line Rust: nested loops for count-like modifiers,
//!    `if` blocks for filters, local rebinding of `p` for position changes. The
//!    nesting reproduces Java's lazy `Stream.flatMap` order exactly (each
//!    position flows through the remaining modifiers before the next one is
//!    produced), so RNG draws happen in the same order;
//! 4. per-dimension step order (Java `FeatureSorter`) and per-biome membership
//!    tables for the decoration driver.
//!
//! Any data shape the emitter cannot compile is a build error.

use crate::codegen::java_blocks::{JavaBlocks, Result, write_pretty};
use crate::codegen::parser::biome::BiomeJson;
use crate::codegen::parser::feature::{
    self as ir, BlockHolderSet, BlockPredicate, BlockStateJson, ConfiguredFeature, ConfiguredRef,
    FeatureData, FloatProvider, FloatProviderObj, HeightProvider, HeightProviderObj, HeightmapKind,
    IntProvider, IntProviderObj, PlacedFeatureJson, PlacedRef, PlacementModifier, RuleTest,
    StateProvider, VerticalAnchor, VerticalDirection,
};
use proc_macro2::{Ident, Literal, TokenStream};
use quote::{format_ident, quote};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

/// Java generation steps (`GenerationStep.Decoration`), in order.
pub const STEPS: [&str; 11] = [
    "raw_generation",
    "lakes",
    "local_modifications",
    "underground_structures",
    "surface_structures",
    "strongholds",
    "underground_ores",
    "underground_decoration",
    "fluid_springs",
    "vegetal_decoration",
    "top_layer_modification",
];

/// End biome source order (`TheEndBiomeSource` possible biomes).
const END_BIOMES: [&str; 5] = [
    "minecraft:the_end",
    "minecraft:end_highlands",
    "minecraft:end_midlands",
    "minecraft:small_end_islands",
    "minecraft:end_barrens",
];

fn sanitize(name: &str) -> String {
    name.strip_prefix("minecraft:")
        .unwrap_or(name)
        .replace(['/', '-', '.'], "_")
}

struct Emitter<'a> {
    blocks: &'a mut JavaBlocks,
    data: &'a FeatureData,
    biome_index: BTreeMap<String, usize>,
    /// Top-level placed feature name -> id (sorted name order).
    placed_ids: BTreeMap<String, u16>,
    items: Vec<TokenStream>,
    inline_counter: usize,
    noise_counter: usize,
    /// Placed features referenced from configs (must not use `biome`).
    nested_refs: BTreeSet<String>,
}

impl<'a> Emitter<'a> {
    // ---- identities --------------------------------------------------------

    fn state(&self, s: &BlockStateJson) -> Result<TokenStream> {
        let id = self.blocks.state(&s.name, &s.properties)?;
        Ok(quote! { JavaState(#id) })
    }

    fn states(&self, list: &[BlockStateJson]) -> Result<TokenStream> {
        let items = list
            .iter()
            .map(|s| self.state(s))
            .collect::<Result<Vec<_>>>()?;
        Ok(quote! { &[#(#items),*] })
    }

    fn block(&self, name: &str) -> Result<TokenStream> {
        let id = self.blocks.block(name)? as u16;
        Ok(quote! { JavaBlock(#id) })
    }

    fn holder_set(&mut self, set: &BlockHolderSet) -> Result<TokenStream> {
        let idx = match set {
            BlockHolderSet::Tag(t) => self.blocks.tag(t)?,
            BlockHolderSet::Blocks(list) => self.blocks.intern_blocks(list)?,
        };
        Ok(quote! { BlockSet(#idx) })
    }

    fn tag(&self, tag: &ir::TagRef) -> Result<TokenStream> {
        let idx = self.blocks.tag(&tag.0)?;
        Ok(quote! { BlockSet(#idx) })
    }

    fn fluid_set(fluids: &[String]) -> Result<TokenStream> {
        let mut bits = 0u8;
        for f in fluids {
            bits |= match f.as_str() {
                "minecraft:empty" => 1,
                "minecraft:water" => 2,
                "minecraft:flowing_water" => 4,
                "minecraft:lava" => 8,
                "minecraft:flowing_lava" => 16,
                other => return Err(format!("unknown fluid {other}").into()),
            };
        }
        Ok(quote! { FluidSet(#bits) })
    }

    // ---- value providers ---------------------------------------------------

    fn int_provider(&self, p: &IntProvider) -> Result<TokenStream> {
        Ok(match p {
            IntProvider::Constant(v) => quote! { IntProvider::Constant(#v) },
            IntProvider::Provider(obj) => match obj {
                IntProviderObj::Constant { value } => quote! { IntProvider::Constant(#value) },
                IntProviderObj::Uniform {
                    min_inclusive,
                    max_inclusive,
                } => quote! { IntProvider::Uniform { min: #min_inclusive, max: #max_inclusive } },
                IntProviderObj::BiasedToBottom {
                    min_inclusive,
                    max_inclusive,
                } => {
                    quote! { IntProvider::BiasedToBottom { min: #min_inclusive, max: #max_inclusive } }
                }
                IntProviderObj::Clamped {
                    source,
                    min_inclusive,
                    max_inclusive,
                } => {
                    let s = self.int_provider(source)?;
                    quote! { IntProvider::Clamped { source: &#s, min: #min_inclusive, max: #max_inclusive } }
                }
                IntProviderObj::ClampedNormal {
                    mean,
                    deviation,
                    min_inclusive,
                    max_inclusive,
                } => quote! {
                    IntProvider::ClampedNormal { mean: #mean, deviation: #deviation, min: #min_inclusive, max: #max_inclusive }
                },
                IntProviderObj::WeightedList { distribution } => {
                    let total: i32 = distribution.iter().map(|e| e.weight).sum();
                    if total <= 0 {
                        return Err("weighted_list with zero total weight".into());
                    }
                    let entries = distribution
                        .iter()
                        .map(|e| {
                            let d = self.int_provider(&e.data)?;
                            let w = e.weight;
                            Ok(quote! { (#d, #w) })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    quote! { IntProvider::WeightedList { entries: &[#(#entries),*], total_weight: #total } }
                }
            },
        })
    }

    /// Expression sampling an int provider with `rng`.
    fn int_sample(&self, p: &IntProvider) -> Result<TokenStream> {
        Ok(match p {
            IntProvider::Constant(v) => quote! { #v },
            IntProvider::Provider(IntProviderObj::Constant { value }) => quote! { #value },
            IntProvider::Provider(IntProviderObj::Uniform {
                min_inclusive,
                max_inclusive,
            }) => quote! { rng.next_int_between_inclusive(#min_inclusive, #max_inclusive) },
            _ => {
                let ip = self.int_provider(p)?;
                quote! { { const P: IntProvider = #ip; P.sample(rng) } }
            }
        })
    }

    fn float_provider(&self, p: &FloatProvider) -> TokenStream {
        match p {
            FloatProvider::Constant(v) => quote! { FloatProvider::Constant(#v) },
            FloatProvider::Provider(obj) => match obj {
                FloatProviderObj::Constant { value } => quote! { FloatProvider::Constant(#value) },
                FloatProviderObj::Uniform {
                    min_inclusive,
                    max_exclusive,
                } => {
                    quote! { FloatProvider::Uniform { min: #min_inclusive, max_exclusive: #max_exclusive } }
                }
                FloatProviderObj::ClampedNormal {
                    mean,
                    deviation,
                    min,
                    max,
                } => {
                    quote! { FloatProvider::ClampedNormal { mean: #mean, deviation: #deviation, min: #min, max: #max } }
                }
                FloatProviderObj::Trapezoid { min, max, plateau } => {
                    quote! { FloatProvider::Trapezoid { min: #min, max: #max, plateau: #plateau } }
                }
            },
        }
    }

    fn anchor(a: &VerticalAnchor) -> TokenStream {
        match *a {
            VerticalAnchor::Absolute(y) => quote! { #y },
            VerticalAnchor::AboveBottom(n) => quote! { (world.min_y() + #n) },
            // PlacementContext inherits WorldGenerationContext, whose height is
            // min(level height, generator depth), not the dimension build height.
            VerticalAnchor::BelowTop(n) => {
                quote! { (world.min_y() + world.height().min(world.gen_depth()) - 1 - #n) }
            }
        }
    }

    fn height_sample(h: &HeightProvider) -> TokenStream {
        match h {
            HeightProvider::Constant(a) => Self::anchor(a),
            HeightProvider::Provider(obj) => match obj {
                HeightProviderObj::Constant { value } => Self::anchor(value),
                HeightProviderObj::Uniform {
                    min_inclusive,
                    max_inclusive,
                } => {
                    let (lo, hi) = (Self::anchor(min_inclusive), Self::anchor(max_inclusive));
                    quote! { height::uniform(rng, #lo, #hi) }
                }
                HeightProviderObj::BiasedToBottom {
                    min_inclusive,
                    max_inclusive,
                    inner,
                } => {
                    let (lo, hi) = (Self::anchor(min_inclusive), Self::anchor(max_inclusive));
                    quote! { height::biased_to_bottom(rng, #lo, #hi, #inner) }
                }
                HeightProviderObj::VeryBiasedToBottom {
                    min_inclusive,
                    max_inclusive,
                    inner,
                } => {
                    let (lo, hi) = (Self::anchor(min_inclusive), Self::anchor(max_inclusive));
                    quote! { height::very_biased_to_bottom(rng, #lo, #hi, #inner) }
                }
                HeightProviderObj::Trapezoid {
                    min_inclusive,
                    max_inclusive,
                    plateau,
                } => {
                    let (lo, hi) = (Self::anchor(min_inclusive), Self::anchor(max_inclusive));
                    quote! { height::trapezoid(rng, #lo, #hi, #plateau) }
                }
            },
        }
    }

    // ---- predicates ----------------------------------------------------------

    /// Data form (`BlockPredicate` value) for configs.
    fn predicate_data(&mut self, p: &BlockPredicate) -> Result<TokenStream> {
        Ok(match p {
            BlockPredicate::MatchingBlocks { blocks, offset } => {
                let set = self.holder_set(blocks)?;
                quote! { BlockPredicate::MatchingBlocks { set: #set, offset: [#(#offset),*] } }
            }
            BlockPredicate::MatchingBlockTag { tag, offset } => {
                let idx = self.blocks.tag(tag)?;
                quote! { BlockPredicate::MatchingBlocks { set: BlockSet(#idx), offset: [#(#offset),*] } }
            }
            BlockPredicate::MatchingFluids { fluids, offset } => {
                let f = Self::fluid_set(&fluids.0)?;
                quote! { BlockPredicate::MatchingFluids { fluids: #f, offset: [#(#offset),*] } }
            }
            BlockPredicate::Solid { offset } => {
                quote! { BlockPredicate::Solid { offset: [#(#offset),*] } }
            }
            BlockPredicate::Replaceable { offset } => {
                quote! { BlockPredicate::Replaceable { offset: [#(#offset),*] } }
            }
            BlockPredicate::WouldSurvive { state, offset } => {
                let s = self.state(state)?;
                quote! { BlockPredicate::WouldSurvive { state: #s, offset: [#(#offset),*] } }
            }
            BlockPredicate::HasSturdyFace { direction, offset } => {
                let d = direction_tokens(*direction);
                quote! { BlockPredicate::HasSturdyFace { direction: #d, offset: [#(#offset),*] } }
            }
            BlockPredicate::InsideWorldBounds { offset } => {
                quote! { BlockPredicate::InsideWorldBounds { offset: [#(#offset),*] } }
            }
            BlockPredicate::AllOf { predicates } => {
                let ps = predicates
                    .iter()
                    .map(|p| self.predicate_data(p))
                    .collect::<Result<Vec<_>>>()?;
                quote! { BlockPredicate::AllOf(&[#(#ps),*]) }
            }
            BlockPredicate::AnyOf { predicates } => {
                let ps = predicates
                    .iter()
                    .map(|p| self.predicate_data(p))
                    .collect::<Result<Vec<_>>>()?;
                quote! { BlockPredicate::AnyOf(&[#(#ps),*]) }
            }
            BlockPredicate::Not { predicate } => {
                let p = self.predicate_data(predicate)?;
                quote! { BlockPredicate::Not(&#p) }
            }
            BlockPredicate::True {} => quote! { BlockPredicate::True },
        })
    }

    /// Inline boolean expression of a predicate at `p`.
    fn predicate_expr(&mut self, pred: &BlockPredicate) -> Result<TokenStream> {
        fn at(offset: &[i32; 3]) -> TokenStream {
            if *offset == [0, 0, 0] {
                quote! { p }
            } else {
                let [x, y, z] = *offset;
                quote! { p.offset(#x, #y, #z) }
            }
        }
        Ok(match pred {
            BlockPredicate::MatchingBlocks { blocks, offset } => {
                let set = self.holder_set(blocks)?;
                let a = at(offset);
                quote! { world.get_state(#a).is_in(#set) }
            }
            BlockPredicate::MatchingBlockTag { tag, offset } => {
                let idx = self.blocks.tag(tag)?;
                let a = at(offset);
                quote! { world.get_state(#a).is_in(BlockSet(#idx)) }
            }
            BlockPredicate::MatchingFluids { fluids, offset } => {
                let f = Self::fluid_set(&fluids.0)?;
                let a = at(offset);
                quote! { #f.contains(world.get_state(#a).fluid()) }
            }
            BlockPredicate::Solid { offset } => {
                let a = at(offset);
                quote! { world.get_state(#a).is_solid() }
            }
            BlockPredicate::Replaceable { offset } => {
                let a = at(offset);
                quote! { world.get_state(#a).can_be_replaced() }
            }
            BlockPredicate::WouldSurvive { state, offset } => {
                let s = self.state(state)?;
                let a = at(offset);
                quote! { survival::can_survive(world, #s, #a) }
            }
            BlockPredicate::HasSturdyFace { direction, offset } => {
                let d = direction_tokens(*direction);
                let a = at(offset);
                quote! { world.get_state(#a).is_face_sturdy(#d) }
            }
            BlockPredicate::InsideWorldBounds { offset } => {
                let a = at(offset);
                quote! { !world.is_outside_build_height(#a.y) }
            }
            BlockPredicate::AllOf { predicates } => {
                let ps = predicates
                    .iter()
                    .map(|p| self.predicate_expr(p))
                    .collect::<Result<Vec<_>>>()?;
                quote! { (#((#ps))&&*) }
            }
            BlockPredicate::AnyOf { predicates } => {
                let ps = predicates
                    .iter()
                    .map(|p| self.predicate_expr(p))
                    .collect::<Result<Vec<_>>>()?;
                quote! { (#((#ps))||*) }
            }
            BlockPredicate::Not { predicate } => {
                let e = self.predicate_expr(predicate)?;
                quote! { !(#e) }
            }
            BlockPredicate::True {} => quote! { true },
        })
    }

    fn rule_test(&self, r: &RuleTest) -> Result<TokenStream> {
        Ok(match r {
            RuleTest::AlwaysTrue {} => quote! { RuleTest::AlwaysTrue },
            RuleTest::BlockMatch { block } => {
                let b = self.block(block)?;
                quote! { RuleTest::BlockMatch(#b) }
            }
            RuleTest::BlockStateMatch { block_state } => {
                let s = self.state(block_state)?;
                quote! { RuleTest::BlockStateMatch(#s) }
            }
            RuleTest::TagMatch { tag } => {
                let idx = self.blocks.tag(tag)?;
                quote! { RuleTest::TagMatch(BlockSet(#idx)) }
            }
            RuleTest::RandomBlockMatch { block, probability } => {
                let b = self.block(block)?;
                quote! { RuleTest::RandomBlockMatch(#b, #probability) }
            }
            RuleTest::RandomBlockStateMatch {
                block_state,
                probability,
            } => {
                let s = self.state(block_state)?;
                quote! { RuleTest::RandomBlockStateMatch(#s, #probability) }
            }
        })
    }

    fn noise_static(&mut self, seed: i64, noise: &ir::NoiseParamsJson) -> Ident {
        let id = format_ident!("PROVIDER_NOISE_{}", self.noise_counter);
        self.noise_counter += 1;
        let first = noise.first_octave;
        let amps = &noise.amplitudes;
        self.items.push(quote! {
            static #id: LazyLock<LegacyNoise> = LazyLock::new(|| {
                LegacyNoise::new(#seed, NoiseSpec { first_octave: #first, amplitudes: &[#(#amps),*] })
            });
        });
        id
    }

    fn state_provider(&mut self, p: &StateProvider) -> Result<TokenStream> {
        Ok(match p {
            StateProvider::Simple { state } => {
                let s = self.state(state)?;
                quote! { StateProvider::Simple(#s) }
            }
            StateProvider::Rotated { state } => {
                let block = self.blocks.default_state(&state.name)?;
                let check = self.state(state)?;
                // Java rotates the block's default state; the listed state only
                // names the block (its properties are ignored).
                let _ = check;
                quote! { StateProvider::Rotated(JavaState(#block)) }
            }
            StateProvider::Weighted { entries } => {
                let total: i32 = entries.iter().map(|e| e.weight).sum();
                if total <= 0 {
                    return Err("weighted_state_provider with zero total weight".into());
                }
                let es = entries
                    .iter()
                    .map(|e| {
                        let s = self.state(&e.data)?;
                        let w = e.weight;
                        Ok(quote! { (#s, #w) })
                    })
                    .collect::<Result<Vec<_>>>()?;
                quote! { StateProvider::Weighted { entries: &[#(#es),*], total_weight: #total } }
            }
            StateProvider::RandomizedInt {
                source,
                property,
                values,
            } => {
                let s = self.state_provider(source)?;
                let v = self.int_provider(values)?;
                quote! { StateProvider::RandomizedInt { source: &#s, property: #property, values: #v } }
            }
            StateProvider::Noise {
                seed,
                noise,
                scale,
                states,
            } => {
                let n = self.noise_static(*seed, noise);
                let st = self.states(states)?;
                quote! { StateProvider::Noise { noise: &#n, scale: #scale, states: #st } }
            }
            StateProvider::DualNoise {
                seed,
                noise,
                scale,
                states,
                variety,
                slow_noise,
                slow_scale,
            } => {
                if variety.1 + 1 > 32 || variety.0 < 0 {
                    return Err("dual_noise_provider variety outside 0..32".into());
                }
                let n = self.noise_static(*seed, noise);
                let sn = self.noise_static(*seed, slow_noise);
                let st = self.states(states)?;
                let (a, b) = (variety.0, variety.1);
                quote! {
                    StateProvider::DualNoise {
                        noise: &#n, slow_noise: &#sn, scale: #scale, slow_scale: #slow_scale,
                        variety: (#a, #b), states: #st,
                    }
                }
            }
            StateProvider::NoiseThreshold {
                seed,
                noise,
                scale,
                threshold,
                high_chance,
                default_state,
                low_states,
                high_states,
            } => {
                let n = self.noise_static(*seed, noise);
                let d = self.state(default_state)?;
                let lo = self.states(low_states)?;
                let hi = self.states(high_states)?;
                quote! {
                    StateProvider::NoiseThreshold {
                        noise: &#n, scale: #scale, threshold: #threshold, high_chance: #high_chance,
                        default_state: #d, low_states: #lo, high_states: #hi,
                    }
                }
            }
        })
    }

    // ---- feature references ----------------------------------------------------

    fn configured_ref(&mut self, r: &ConfiguredRef) -> Result<Ident> {
        match r {
            ConfiguredRef::Ref(name) => {
                if !self.data.configured.contains_key(name) {
                    return Err(format!("unknown configured feature {name}").into());
                }
                Ok(format_ident!("cf_{}", sanitize(name)))
            }
            ConfiguredRef::Inline(cf) => {
                let id = format_ident!("cf_inline_{}", self.inline_counter);
                self.inline_counter += 1;
                self.emit_configured(&id, cf)?;
                Ok(id)
            }
        }
    }

    fn placed_ref(&mut self, r: &PlacedRef) -> Result<Ident> {
        match r {
            PlacedRef::Ref(name) => {
                if !self.placed_ids.contains_key(name) {
                    return Err(format!("unknown placed feature {name}").into());
                }
                self.nested_refs.insert(name.clone());
                Ok(format_ident!("pf_{}", sanitize(name)))
            }
            PlacedRef::Inline(pf) => {
                let id = format_ident!("pf_inline_{}", self.inline_counter);
                self.inline_counter += 1;
                if pf
                    .placement
                    .iter()
                    .any(|m| matches!(m, PlacementModifier::Biome {}))
                {
                    return Err("nested placed feature uses the biome filter".into());
                }
                self.emit_placed(&id, pf, None)?;
                Ok(id)
            }
        }
    }

    // ---- placed features ----------------------------------------------------

    fn emit_placed(
        &mut self,
        id: &Ident,
        pf: &PlacedFeatureJson,
        placed_id: Option<u16>,
    ) -> Result<()> {
        let cf = self.configured_ref(&pf.feature)?;
        let mut cont = quote! {
            if #cf(world, rng, p) {
                placed = true;
            }
        };
        for m in pf.placement.iter().rev() {
            cont = self.emit_modifier(m, cont, placed_id)?;
        }
        self.items.push(quote! {
            #[allow(unused_variables, unused_mut, clippy::all)]
            pub fn #id<W: FeatureWorld + ?Sized>(world: &mut W, rng: &mut FeatureRandom, origin: BlockPos) -> bool {
                let mut placed = false;
                let p = origin;
                #cont
                placed
            }
        });
        Ok(())
    }

    fn emit_modifier(
        &mut self,
        m: &PlacementModifier,
        cont: TokenStream,
        placed_id: Option<u16>,
    ) -> Result<TokenStream> {
        Ok(match m {
            PlacementModifier::Biome {} => {
                let id = placed_id.ok_or("biome filter outside a top-level placed feature")?;
                quote! {
                    if PLACED_BIOMES[#id as usize] & (1u128 << world.biome(p)) != 0 {
                        #cont
                    }
                }
            }
            PlacementModifier::BlockPredicateFilter { predicate } => {
                let e = self.predicate_expr(predicate)?;
                quote! { if #e { #cont } }
            }
            PlacementModifier::Count { count } => {
                let n = self.int_sample(count)?;
                quote! {
                    {
                        let n: i32 = #n;
                        for _ in 0..n {
                            #cont
                        }
                    }
                }
            }
            PlacementModifier::CountOnEveryLayer { count } => {
                let n = self.int_sample(count)?;
                // Java collects every position before passing any downstream.
                quote! {
                    {
                        let mut found: Vec<BlockPos> = Vec::new();
                        let mut layer = 0;
                        loop {
                            let mut any = false;
                            let mut j = 0;
                            while j < #n {
                                let x = rng.next_int_bounded(16) + p.x;
                                let z = rng.next_int_bounded(16) + p.z;
                                let top = world.get_height(Heightmap::MotionBlocking, x, z);
                                if let Some(y) = placement::on_ground_y(world, x, top, z, layer) {
                                    found.push(BlockPos::new(x, y, z));
                                    any = true;
                                }
                                j += 1;
                            }
                            layer += 1;
                            if !any {
                                break;
                            }
                        }
                        for p in found {
                            #cont
                        }
                    }
                }
            }
            PlacementModifier::EnvironmentScan {
                direction_of_search,
                target_condition,
                allowed_search_condition,
                max_steps,
            } => {
                let target = self.predicate_expr(target_condition)?;
                let allowed = match allowed_search_condition {
                    Some(a) => self.predicate_expr(a)?,
                    None => quote! { true },
                };
                let dy: i32 = match direction_of_search {
                    VerticalDirection::Up => 1,
                    VerticalDirection::Down => -1,
                };
                quote! {
                    {
                        let found: Option<BlockPos> = 'scan: {
                            let mut p = p;
                            if !(#allowed) {
                                break 'scan None;
                            }
                            for _ in 0..#max_steps {
                                if #target {
                                    break 'scan Some(p);
                                }
                                p = p.offset(0, #dy, 0);
                                if world.is_outside_build_height(p.y) {
                                    break 'scan None;
                                }
                                if !(#allowed) {
                                    break;
                                }
                            }
                            if #target { Some(p) } else { None }
                        };
                        if let Some(p) = found {
                            #cont
                        }
                    }
                }
            }
            PlacementModifier::FixedPlacement { positions } => {
                let arms = positions.iter().map(|[x, y, z]| {
                    quote! {
                        if (#x >> 4) == (p.x >> 4) && (#z >> 4) == (p.z >> 4) {
                            let p = BlockPos::new(#x, #y, #z);
                            #cont
                        }
                    }
                });
                quote! { { #(#arms)* } }
            }
            PlacementModifier::HeightRange { height } => {
                let y = Self::height_sample(height);
                quote! {
                    {
                        let p = p.with_y(#y);
                        #cont
                    }
                }
            }
            PlacementModifier::Heightmap { heightmap } => {
                let hm = heightmap_tokens(*heightmap);
                quote! {
                    {
                        let y = world.get_height(#hm, p.x, p.z);
                        if y > world.min_y() {
                            let p = p.with_y(y);
                            #cont
                        }
                    }
                }
            }
            PlacementModifier::InSquare {} => quote! {
                {
                    let x = rng.next_int_bounded(16) + p.x;
                    let z = rng.next_int_bounded(16) + p.z;
                    let p = BlockPos::new(x, p.y, z);
                    #cont
                }
            },
            PlacementModifier::NoiseBasedCount {
                noise_to_count_ratio,
                noise_factor,
                noise_offset,
            } => quote! {
                {
                    let d = noise::biome_info_noise(p.x as f64 / #noise_factor, p.z as f64 / #noise_factor);
                    let n = ((d + #noise_offset) * #noise_to_count_ratio as f64).ceil() as i32;
                    for _ in 0..n {
                        #cont
                    }
                }
            },
            PlacementModifier::NoiseThresholdCount {
                noise_level,
                below_noise,
                above_noise,
            } => quote! {
                {
                    let d = noise::biome_info_noise(p.x as f64 / 200.0, p.z as f64 / 200.0);
                    let n: i32 = if d < #noise_level { #below_noise } else { #above_noise };
                    for _ in 0..n {
                        #cont
                    }
                }
            },
            PlacementModifier::RandomOffset {
                xz_spread,
                y_spread,
            } => {
                let xz = self.int_sample(xz_spread)?;
                let y = self.int_sample(y_spread)?;
                quote! {
                    {
                        let x = p.x + #xz;
                        let y = p.y + #y;
                        let z = p.z + #xz;
                        let p = BlockPos::new(x, y, z);
                        #cont
                    }
                }
            }
            PlacementModifier::RarityFilter { chance } => {
                let c = *chance as f32;
                quote! {
                    if rng.next_float() < 1.0f32 / #c {
                        #cont
                    }
                }
            }
            PlacementModifier::SurfaceRelativeThresholdFilter {
                heightmap,
                min_inclusive,
                max_inclusive,
            } => {
                let hm = heightmap_tokens(*heightmap);
                let lo = min_inclusive.unwrap_or(i32::MIN) as i64;
                let hi = max_inclusive.unwrap_or(i32::MAX) as i64;
                quote! {
                    {
                        let h = world.get_height(#hm, p.x, p.z) as i64;
                        let y = p.y as i64;
                        if h + #lo <= y && y <= h + #hi {
                            #cont
                        }
                    }
                }
            }
            PlacementModifier::SurfaceWaterDepthFilter { max_water_depth } => quote! {
                {
                    let floor = world.get_height(Heightmap::OceanFloor, p.x, p.z);
                    let surface = world.get_height(Heightmap::WorldSurface, p.x, p.z);
                    if surface - floor <= #max_water_depth {
                        #cont
                    }
                }
            },
        })
    }

    // ---- configured features -------------------------------------------------

    fn wrap_cfg(&mut self, id: &Ident, ty: TokenStream, value: TokenStream, call: TokenStream) {
        self.items.push(quote! {
            #[allow(unused_variables, clippy::all)]
            pub fn #id<W: FeatureWorld + ?Sized>(world: &mut W, rng: &mut FeatureRandom, origin: BlockPos) -> bool {
                static CFG: #ty = #value;
                #call
            }
        });
    }

    fn wrap_call(&mut self, id: &Ident, call: TokenStream) {
        self.items.push(quote! {
            #[allow(unused_variables, clippy::all)]
            pub fn #id<W: FeatureWorld + ?Sized>(world: &mut W, rng: &mut FeatureRandom, origin: BlockPos) -> bool {
                #call
            }
        });
    }

    fn ore_config(&mut self, c: &ir::OreConfig) -> Result<TokenStream> {
        let targets = c
            .targets
            .iter()
            .map(|t| {
                let r = self.rule_test(&t.target)?;
                let s = self.state(&t.state)?;
                Ok(quote! { OreTarget { target: #r, state: #s } })
            })
            .collect::<Result<Vec<_>>>()?;
        let size = c.size;
        let d = c.discard_chance_on_air_exposure;
        Ok(
            quote! { OreConfig { targets: &[#(#targets),*], size: #size, discard_chance_on_air_exposure: #d } },
        )
    }

    fn random_patch(&mut self, id: &Ident, c: &ir::RandomPatchConfig) -> Result<()> {
        let nested = self.placed_ref(&c.feature)?;
        let tries = c.tries;
        let xz = c.xz_spread + 1;
        let y = c.y_spread + 1;
        self.wrap_call(
            id,
            quote! {
                let mut count = 0;
                for _ in 0..#tries {
                    let dx = rng.next_int_bounded(#xz) - rng.next_int_bounded(#xz);
                    let dy = rng.next_int_bounded(#y) - rng.next_int_bounded(#y);
                    let dz = rng.next_int_bounded(#xz) - rng.next_int_bounded(#xz);
                    if #nested(world, rng, origin.offset(dx, dy, dz)) {
                        count += 1;
                    }
                }
                count > 0
            },
        );
        Ok(())
    }

    fn vegetation_patch(
        &mut self,
        id: &Ident,
        c: &ir::VegetationPatchConfig,
        waterlogged: bool,
    ) -> Result<()> {
        let nested = self.placed_ref(&c.vegetation_feature)?;
        let replaceable = self.tag(&c.replaceable)?;
        let ground = self.state_provider(&c.ground_state)?;
        let surface = match c.surface {
            ir::CaveSurface::Ceiling => quote! { CaveSurface::Ceiling },
            ir::CaveSurface::Floor => quote! { CaveSurface::Floor },
        };
        let depth = self.int_provider(&c.depth)?;
        let ebc = c.extra_bottom_block_chance;
        let vr = c.vertical_range;
        let vc = c.vegetation_chance;
        let xz = self.int_provider(&c.xz_radius)?;
        let eec = c.extra_edge_column_chance;
        let f = if waterlogged {
            quote! { kinds::vegetation_patch::place_waterlogged }
        } else {
            quote! { kinds::vegetation_patch::place }
        };
        self.wrap_cfg(
            id,
            quote! { VegetationPatchConfig },
            quote! {
                VegetationPatchConfig {
                    replaceable: #replaceable, ground_state: #ground, surface: #surface, depth: #depth,
                    extra_bottom_block_chance: #ebc, vertical_range: #vr, vegetation_chance: #vc,
                    xz_radius: #xz, extra_edge_column_chance: #eec,
                }
            },
            quote! { #f(world, rng, &CFG, origin, |w, r, p| #nested(w, r, p)) },
        );
        Ok(())
    }

    fn emit_configured(&mut self, id: &Ident, cf: &ConfiguredFeature) -> Result<()> {
        use ConfiguredFeature as C;
        match cf {
            C::Ore(c) => {
                let v = self.ore_config(c)?;
                self.wrap_cfg(
                    id,
                    quote! { OreConfig },
                    v,
                    quote! { kinds::ore::place(world, rng, &CFG, origin) },
                );
            }
            C::ScatteredOre(c) => {
                let v = self.ore_config(c)?;
                self.wrap_cfg(
                    id,
                    quote! { OreConfig },
                    v,
                    quote! { kinds::ore::place_scattered(world, rng, &CFG, origin) },
                );
            }
            C::RandomPatch(c) | C::Flower(c) | C::NoBonemealFlower(c) => {
                self.random_patch(id, c)?
            }
            C::RandomSelector(c) => {
                let mut arms = Vec::new();
                for w in &c.features {
                    let f = self.placed_ref(&w.feature)?;
                    let chance = w.chance;
                    arms.push(quote! {
                        if rng.next_float() < #chance {
                            return #f(world, rng, origin);
                        }
                    });
                }
                let d = self.placed_ref(&c.default)?;
                self.wrap_call(id, quote! { #(#arms)* #d(world, rng, origin) });
            }
            C::SimpleRandomSelector(c) => {
                let fs = c
                    .features
                    .iter()
                    .map(|f| self.placed_ref(f))
                    .collect::<Result<Vec<_>>>()?;
                let n = fs.len() as i32;
                let arms = fs.iter().enumerate().map(|(i, f)| {
                    let i = i as i32;
                    quote! { #i => #f(world, rng, origin), }
                });
                self.wrap_call(
                    id,
                    quote! {
                        match rng.next_int_bounded(#n) {
                            #(#arms)*
                            _ => unreachable!(),
                        }
                    },
                );
            }
            C::RandomBooleanSelector(c) => {
                let t = self.placed_ref(&c.feature_true)?;
                let f = self.placed_ref(&c.feature_false)?;
                self.wrap_call(
                    id,
                    quote! {
                        if rng.next_bool() { #t(world, rng, origin) } else { #f(world, rng, origin) }
                    },
                );
            }
            C::SimpleBlock(c) => {
                let p = self.state_provider(&c.to_place)?;
                let st = c.schedule_tick;
                self.wrap_cfg(
                    id,
                    quote! { SimpleBlockConfig },
                    quote! { SimpleBlockConfig { to_place: #p, schedule_tick: #st } },
                    quote! { kinds::simple_block::place(world, rng, &CFG, origin) },
                );
            }
            C::Disk(c) => {
                let fallback = self.state_provider(&c.state_provider.fallback)?;
                let rules = c
                    .state_provider
                    .rules
                    .iter()
                    .map(|r| {
                        let pr = self.predicate_data(&r.if_true)?;
                        let t = self.state_provider(&r.then)?;
                        Ok(quote! { (#pr, #t) })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let target = self.predicate_data(&c.target)?;
                let radius = self.int_provider(&c.radius)?;
                let hh = c.half_height;
                self.wrap_cfg(
                    id,
                    quote! { DiskConfig },
                    quote! {
                        DiskConfig {
                            state_provider: RuleBasedStateProvider { fallback: #fallback, rules: &[#(#rules),*] },
                            target: #target, radius: #radius, half_height: #hh,
                        }
                    },
                    quote! { kinds::disk::place(world, rng, &CFG, origin) },
                );
            }
            C::Lake(c) => {
                let fluid = self.state_provider(&c.fluid)?;
                let barrier = self.state_provider(&c.barrier)?;
                self.wrap_cfg(
                    id,
                    quote! { LakeConfig },
                    quote! { LakeConfig { fluid: #fluid, barrier: #barrier } },
                    quote! { kinds::lake::place(world, rng, &CFG, origin) },
                );
            }
            C::Spring(c) => {
                // `state` is a FluidState; Java places `createLegacyBlock()`,
                // which for a source fluid is the fluid block at level 0.
                if !matches!(c.state.name.as_str(), "minecraft:water" | "minecraft:lava") {
                    return Err(
                        format!("spring fluid {} is not a source fluid", c.state.name).into(),
                    );
                }
                let mut level = BTreeMap::new();
                level.insert("level".to_string(), "0".to_string());
                let s = self.state(&BlockStateJson {
                    name: c.state.name.clone(),
                    properties: level,
                })?;
                let rbb = c.requires_block_below;
                let rc = c.rock_count;
                let hc = c.hole_count;
                let vb = self.holder_set(&c.valid_blocks)?;
                self.wrap_cfg(
                    id,
                    quote! { SpringConfig },
                    quote! {
                        SpringConfig { state: #s, requires_block_below: #rbb, rock_count: #rc, hole_count: #hc, valid_blocks: #vb }
                    },
                    quote! { kinds::spring::place(world, rng, &CFG, origin) },
                );
            }
            C::BlockColumn(c) => {
                let layers = c
                    .layers
                    .iter()
                    .map(|l| {
                        let h = self.int_provider(&l.height)?;
                        let p = self.state_provider(&l.provider)?;
                        Ok(quote! { ColumnLayer { height: #h, provider: #p } })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let dir = direction_tokens(c.direction);
                let ap = self.predicate_data(&c.allowed_placement)?;
                let pt = c.prioritize_tip;
                self.wrap_cfg(
                    id,
                    quote! { BlockColumnConfig },
                    quote! {
                        BlockColumnConfig { layers: &[#(#layers),*], direction: #dir, allowed_placement: #ap, prioritize_tip: #pt }
                    },
                    quote! { kinds::block_column::place(world, rng, &CFG, origin) },
                );
            }
            C::BlockPile(c) => {
                let p = self.state_provider(&c.state_provider)?;
                self.wrap_cfg(
                    id,
                    quote! { BlockPileConfig },
                    quote! { BlockPileConfig { state_provider: #p } },
                    quote! { kinds::block_pile::place(world, rng, &CFG, origin) },
                );
            }
            C::Bamboo(c) => {
                let p = c.probability;
                self.wrap_call(id, quote! { kinds::bamboo::place(world, rng, #p, origin) });
            }
            C::UnderwaterMagma(c) => {
                let a = c.floor_search_range;
                let b = c.placement_radius_around_floor;
                let d = c.placement_probability_per_valid_position;
                self.wrap_cfg(
                    id,
                    quote! { UnderwaterMagmaConfig },
                    quote! {
                        UnderwaterMagmaConfig {
                            floor_search_range: #a, placement_radius_around_floor: #b,
                            placement_probability_per_valid_position: #d,
                        }
                    },
                    quote! { kinds::underwater_magma::place(world, rng, &CFG, origin) },
                );
            }
            C::ForestRock(c) => {
                let s = self.state(&c.state)?;
                self.wrap_call(
                    id,
                    quote! { kinds::forest_rock::place(world, rng, #s, origin) },
                );
            }
            C::IceSpike(_) => self.wrap_call(
                id,
                quote! { kinds::ice::place_ice_spike(world, rng, origin) },
            ),
            C::Iceberg(c) => {
                let s = self.state(&c.state)?;
                self.wrap_call(
                    id,
                    quote! { kinds::ice::place_iceberg(world, rng, #s, origin) },
                );
            }
            C::BlueIce(_) => self.wrap_call(
                id,
                quote! { kinds::ice::place_blue_ice(world, rng, origin) },
            ),
            C::FreezeTopLayer(_) => self.wrap_call(
                id,
                quote! { kinds::ice::place_freeze_top_layer(world, rng, origin) },
            ),
            C::DesertWell(_) => {
                self.wrap_call(id, quote! { kinds::desert_well::place(world, rng, origin) })
            }
            C::MonsterRoom(_) => self.wrap_call(
                id,
                quote! { kinds::monster_room::place(world, rng, origin) },
            ),
            C::BonusChest(_) => {
                self.wrap_call(id, quote! { kinds::bonus_chest::place(world, rng, origin) })
            }
            C::Kelp(_) => {
                self.wrap_call(id, quote! { kinds::ocean::place_kelp(world, rng, origin) })
            }
            C::Seagrass(c) => {
                let p = c.probability;
                self.wrap_call(
                    id,
                    quote! { kinds::ocean::place_seagrass(world, rng, #p, origin) },
                );
            }
            C::SeaPickle(c) => {
                let n = self.int_provider(&c.count)?;
                self.wrap_cfg(
                    id,
                    quote! { IntProvider },
                    n,
                    quote! { kinds::ocean::place_sea_pickle(world, rng, &CFG, origin) },
                );
            }
            C::CoralClaw(_) => self.wrap_call(
                id,
                quote! { kinds::ocean::place_coral_claw(world, rng, origin) },
            ),
            C::CoralMushroom(_) => self.wrap_call(
                id,
                quote! { kinds::ocean::place_coral_mushroom(world, rng, origin) },
            ),
            C::CoralTree(_) => self.wrap_call(
                id,
                quote! { kinds::ocean::place_coral_tree(world, rng, origin) },
            ),
            C::Vines(_) => self.wrap_call(id, quote! { kinds::vines::place(world, rng, origin) }),
            C::VegetationPatch(c) => self.vegetation_patch(id, c, false)?,
            C::WaterloggedVegetationPatch(c) => self.vegetation_patch(id, c, true)?,
            C::MultifaceGrowth(c) => {
                let block = self.block(&c.block)?;
                let mut dirs = Vec::new();
                if c.can_place_on_ceiling {
                    dirs.push(quote! { Direction::Up });
                }
                if c.can_place_on_floor {
                    dirs.push(quote! { Direction::Down });
                }
                if c.can_place_on_wall {
                    dirs.extend([
                        quote! { Direction::North },
                        quote! { Direction::East },
                        quote! { Direction::South },
                        quote! { Direction::West },
                    ]);
                }
                let set = self.holder_set(&c.can_be_placed_on)?;
                let (sr, f, ce, w, ch) = (
                    c.search_range,
                    c.can_place_on_floor,
                    c.can_place_on_ceiling,
                    c.can_place_on_wall,
                    c.chance_of_spreading,
                );
                self.wrap_cfg(
                    id,
                    quote! { MultifaceGrowthConfig },
                    quote! {
                        MultifaceGrowthConfig {
                            block: #block, search_range: #sr, can_place_on_floor: #f, can_place_on_ceiling: #ce,
                            can_place_on_wall: #w, chance_of_spreading: #ch, can_be_placed_on: #set,
                            valid_directions: &[#(#dirs),*],
                        }
                    },
                    quote! { kinds::multiface_growth::place(world, rng, &CFG, origin) },
                );
            }
            C::SculkPatch(c) => {
                let erg = self.int_provider(&c.extra_rare_growths)?;
                let (a, b, s, g, r, cc) = (
                    c.charge_count,
                    c.amount_per_charge,
                    c.spread_attempts,
                    c.growth_rounds,
                    c.spread_rounds,
                    c.catalyst_chance,
                );
                self.wrap_cfg(
                    id,
                    quote! { SculkPatchConfig },
                    quote! {
                        SculkPatchConfig {
                            charge_count: #a, amount_per_charge: #b, spread_attempts: #s, growth_rounds: #g,
                            spread_rounds: #r, extra_rare_growths: #erg, catalyst_chance: #cc,
                        }
                    },
                    quote! { kinds::sculk_patch::place(world, rng, &CFG, origin) },
                );
            }
            C::RootSystem(c) => {
                let tree = self.placed_ref(&c.feature)?;
                let rr = self.tag(&c.root_replaceable)?;
                let rsp = self.state_provider(&c.root_state_provider)?;
                let hsp = self.state_provider(&c.hanging_root_state_provider)?;
                let atp = self.predicate_data(&c.allowed_tree_position)?;
                let (a, b, d, e, f, g, h, i) = (
                    c.required_vertical_space_for_tree,
                    c.root_radius,
                    c.root_placement_attempts,
                    c.root_column_max_height,
                    c.hanging_root_radius,
                    c.hanging_roots_vertical_span,
                    c.hanging_root_placement_attempts,
                    c.allowed_vertical_water_for_tree,
                );
                self.wrap_cfg(
                    id,
                    quote! { RootSystemConfig },
                    quote! {
                        RootSystemConfig {
                            required_vertical_space_for_tree: #a, root_radius: #b, root_replaceable: #rr,
                            root_state_provider: #rsp, root_placement_attempts: #d, root_column_max_height: #e,
                            hanging_root_radius: #f, hanging_roots_vertical_span: #g,
                            hanging_root_state_provider: #hsp, hanging_root_placement_attempts: #h,
                            allowed_vertical_water_for_tree: #i, allowed_tree_position: #atp,
                        }
                    },
                    quote! { kinds::root_system::place(world, rng, &CFG, origin, |w, r, p| #tree(w, r, p)) },
                );
            }
            C::HugeBrownMushroom(c) | C::HugeRedMushroom(c) => {
                let cap = self.state_provider(&c.cap_provider)?;
                let stem = self.state_provider(&c.stem_provider)?;
                let fr = c.foliage_radius;
                let f = if matches!(cf, C::HugeBrownMushroom(_)) {
                    quote! { kinds::huge_mushroom::place_brown }
                } else {
                    quote! { kinds::huge_mushroom::place_red }
                };
                self.wrap_cfg(
                    id,
                    quote! { HugeMushroomConfig },
                    quote! { HugeMushroomConfig { cap_provider: #cap, stem_provider: #stem, foliage_radius: #fr } },
                    quote! { #f(world, rng, &CFG, origin) },
                );
            }
            C::Tree(c) => {
                let v = self.tree_config(c)?;
                let moss = self.pale_moss_closure(&c.decorators)?;
                self.wrap_cfg(
                    id,
                    quote! { TreeConfig },
                    v,
                    quote! { kinds::tree::place(world, rng, &CFG, origin, #moss) },
                );
            }
            C::FallenTree(c) => {
                let tp = self.state_provider(&c.trunk_provider)?;
                let ll = self.int_provider(&c.log_length)?;
                let sd = self.decorators(&c.stump_decorators)?;
                let ld = self.decorators(&c.log_decorators)?;
                self.wrap_cfg(
                    id,
                    quote! { FallenTreeConfig },
                    quote! {
                        FallenTreeConfig { trunk_provider: #tp, log_length: #ll, stump_decorators: #sd, log_decorators: #ld }
                    },
                    {
                        let moss = self.pale_moss_closure(
                            &[c.stump_decorators.as_slice(), c.log_decorators.as_slice()].concat(),
                        )?;
                        quote! { kinds::fallen_tree::place(world, rng, &CFG, origin, #moss) }
                    },
                );
            }
            C::Geode(c) => {
                let v = self.geode_config(c)?;
                self.wrap_cfg(
                    id,
                    quote! { GeodeConfig },
                    v,
                    quote! { kinds::geode::place(world, rng, &CFG, origin) },
                );
            }
            C::DripstoneCluster(c) => {
                let h = self.int_provider(&c.height)?;
                let r = self.int_provider(&c.radius)?;
                let t = self.int_provider(&c.dripstone_block_layer_thickness)?;
                let d = self.float_provider(&c.density);
                let w = self.float_provider(&c.wetness);
                let (a, b, e, f, g, i) = (
                    c.floor_to_ceiling_search_range,
                    c.max_stalagmite_stalactite_height_diff,
                    c.height_deviation,
                    c.chance_of_dripstone_column_at_max_distance_from_center,
                    c.max_distance_from_edge_affecting_chance_of_dripstone_column,
                    c.max_distance_from_center_affecting_height_bias,
                );
                self.wrap_cfg(
                    id,
                    quote! { DripstoneClusterConfig },
                    quote! {
                        DripstoneClusterConfig {
                            floor_to_ceiling_search_range: #a, height: #h, radius: #r,
                            max_stalagmite_stalactite_height_diff: #b, height_deviation: #e,
                            dripstone_block_layer_thickness: #t, density: #d, wetness: #w,
                            chance_of_dripstone_column_at_max_distance_from_center: #f,
                            max_distance_from_edge_affecting_chance_of_dripstone_column: #g,
                            max_distance_from_center_affecting_height_bias: #i,
                        }
                    },
                    quote! { kinds::dripstone::place_cluster(world, rng, &CFG, origin) },
                );
            }
            C::LargeDripstone(c) => {
                let cr = self.int_provider(&c.column_radius)?;
                let hs = self.float_provider(&c.height_scale);
                let sb = self.float_provider(&c.stalactite_bluntness);
                let gb = self.float_provider(&c.stalagmite_bluntness);
                let ws = self.float_provider(&c.wind_speed);
                let (a, b, d, e) = (
                    c.floor_to_ceiling_search_range,
                    c.max_column_radius_to_cave_height_ratio,
                    c.min_radius_for_wind,
                    c.min_bluntness_for_wind,
                );
                self.wrap_cfg(
                    id,
                    quote! { LargeDripstoneConfig },
                    quote! {
                        LargeDripstoneConfig {
                            floor_to_ceiling_search_range: #a, column_radius: #cr, height_scale: #hs,
                            max_column_radius_to_cave_height_ratio: #b, stalactite_bluntness: #sb,
                            stalagmite_bluntness: #gb, wind_speed: #ws, min_radius_for_wind: #d,
                            min_bluntness_for_wind: #e,
                        }
                    },
                    quote! { kinds::dripstone::place_large(world, rng, &CFG, origin) },
                );
            }
            C::PointedDripstone(c) => {
                let (a, b, d, e) = (
                    c.chance_of_taller_dripstone,
                    c.chance_of_directional_spread,
                    c.chance_of_spread_radius2,
                    c.chance_of_spread_radius3,
                );
                self.wrap_cfg(
                    id,
                    quote! { PointedDripstoneConfig },
                    quote! {
                        PointedDripstoneConfig {
                            chance_of_taller_dripstone: #a, chance_of_directional_spread: #b,
                            chance_of_spread_radius2: #d, chance_of_spread_radius3: #e,
                        }
                    },
                    quote! { kinds::dripstone::place_pointed(world, rng, &CFG, origin) },
                );
            }
            C::Fossil(c) => {
                let fs = c
                    .fossil_structures
                    .iter()
                    .map(|s| quote! { #s })
                    .collect::<Vec<_>>();
                let os = c
                    .overlay_structures
                    .iter()
                    .map(|s| quote! { #s })
                    .collect::<Vec<_>>();
                let pl = |name: &str| {
                    let id = format_ident!("PL_{}", crate::codegen::java_blocks::const_ident(name));
                    quote! { &crate::structures::generated::#id }
                };
                let (fp, op, me) = (
                    pl(&c.fossil_processors),
                    pl(&c.overlay_processors),
                    c.max_empty_corners_allowed,
                );
                self.wrap_cfg(
                    id,
                    quote! { FossilConfig },
                    quote! {
                        FossilConfig {
                            fossil_structures: &[#(#fs),*], overlay_structures: &[#(#os),*],
                            fossil_processors: #fp, overlay_processors: #op, max_empty_corners_allowed: #me,
                        }
                    },
                    quote! { kinds::fossil::place(world, rng, &CFG, origin) },
                );
            }
            C::HugeFungus(c) => {
                let (vb, ss, hs, ds) = (
                    self.state(&c.valid_base_block)?,
                    self.state(&c.stem_state)?,
                    self.state(&c.hat_state)?,
                    self.state(&c.decor_state)?,
                );
                let rb = self.predicate_data(&c.replaceable_blocks)?;
                let pl = c.planted;
                self.wrap_cfg(
                    id,
                    quote! { HugeFungusConfig },
                    quote! {
                        HugeFungusConfig {
                            valid_base_block: #vb, stem_state: #ss, hat_state: #hs, decor_state: #ds,
                            replaceable_blocks: #rb, planted: #pl,
                        }
                    },
                    quote! { kinds::nether::place_huge_fungus(world, rng, &CFG, origin) },
                );
            }
            C::NetherForestVegetation(c) => {
                let sp = self.state_provider(&c.state_provider)?;
                let (w, h) = (c.spread_width, c.spread_height);
                self.wrap_cfg(
                    id,
                    quote! { NetherForestVegetationConfig },
                    quote! { NetherForestVegetationConfig { state_provider: #sp, spread_width: #w, spread_height: #h } },
                    quote! { kinds::nether::place_nether_forest_vegetation(world, rng, &CFG, origin) },
                );
            }
            C::TwistingVines(c) => {
                let (w, h, m) = (c.spread_width, c.spread_height, c.max_height);
                self.wrap_cfg(
                    id,
                    quote! { TwistingVinesConfig },
                    quote! { TwistingVinesConfig { spread_width: #w, spread_height: #h, max_height: #m } },
                    quote! { kinds::nether::place_twisting_vines(world, rng, &CFG, origin) },
                );
            }
            C::WeepingVines(_) => self.wrap_call(
                id,
                quote! { kinds::nether::place_weeping_vines(world, rng, origin) },
            ),
            C::NetherrackReplaceBlobs(c) => {
                let (t, s) = (self.state(&c.target)?, self.state(&c.state)?);
                let r = self.int_provider(&c.radius)?;
                self.wrap_cfg(
                    id,
                    quote! { ReplaceBlobsConfig },
                    quote! { ReplaceBlobsConfig { target: #t, state: #s, radius: #r } },
                    quote! { kinds::nether::place_netherrack_replace_blobs(world, rng, &CFG, origin) },
                );
            }
            C::BasaltColumns(c) => {
                let (r, h) = (self.int_provider(&c.reach)?, self.int_provider(&c.height)?);
                self.wrap_cfg(
                    id,
                    quote! { ColumnConfig },
                    quote! { ColumnConfig { reach: #r, height: #h } },
                    quote! { kinds::nether::place_basalt_columns(world, rng, &CFG, origin) },
                );
            }
            C::BasaltPillar(_) => self.wrap_call(
                id,
                quote! { kinds::nether::place_basalt_pillar(world, rng, origin) },
            ),
            C::Delta(c) => {
                let (ct, rm) = (self.state(&c.contents)?, self.state(&c.rim)?);
                let (sz, rs) = (self.int_provider(&c.size)?, self.int_provider(&c.rim_size)?);
                self.wrap_cfg(
                    id,
                    quote! { DeltaConfig },
                    quote! { DeltaConfig { contents: #ct, rim: #rm, size: #sz, rim_size: #rs } },
                    quote! { kinds::nether::place_delta(world, rng, &CFG, origin) },
                );
            }
            C::GlowstoneBlob(_) => self.wrap_call(
                id,
                quote! { kinds::nether::place_glowstone_blob(world, rng, origin) },
            ),
            C::ChorusPlant(_) => self.wrap_call(
                id,
                quote! { kinds::end::place_chorus_plant(world, rng, origin) },
            ),
            C::EndGateway(c) => {
                let exit = match c.exit {
                    Some([x, y, z]) => quote! { Some([#x, #y, #z]) },
                    None => quote! { None },
                };
                let exact = c.exact;
                self.wrap_cfg(
                    id,
                    quote! { EndGatewayConfig },
                    quote! { EndGatewayConfig { exit: #exit, exact: #exact } },
                    quote! { kinds::end::place_end_gateway(world, rng, &CFG, origin) },
                );
            }
            C::EndIsland(_) => self.wrap_call(
                id,
                quote! { kinds::end::place_end_island(world, rng, origin) },
            ),
            C::EndPlatform(_) => self.wrap_call(
                id,
                quote! { kinds::end::place_end_platform(world, rng, origin) },
            ),
            C::EndSpike(c) => {
                let spikes = c.spikes.iter().map(|s| {
                    let (x, z, r, h, g) = (s.center_x, s.center_z, s.radius, s.height, s.guarded);
                    quote! { EndSpike { center_x: #x, center_z: #z, radius: #r, height: #h, guarded: #g } }
                });
                let ci = c.crystal_invulnerable;
                let target = match c.crystal_beam_target {
                    Some([x, y, z]) => quote! { Some([#x, #y, #z]) },
                    None => quote! { None },
                };
                self.wrap_cfg(
                    id,
                    quote! { EndSpikeConfig },
                    quote! { EndSpikeConfig { crystal_invulnerable: #ci, spikes: &[#(#spikes),*], crystal_beam_target: #target } },
                    quote! { kinds::end::place_end_spike(world, rng, &CFG, origin) },
                );
            }
            C::VoidStartPlatform(_) => self.wrap_call(
                id,
                quote! { kinds::end::place_void_start_platform(world, rng, origin) },
            ),
        }
        Ok(())
    }

    /// Closure for the `pale_moss` decorator's nested `minecraft:pale_moss_patch`
    /// configured feature, or a closure that places nothing.
    fn pale_moss_closure(&self, list: &[ir::TreeDecorator]) -> Result<TokenStream> {
        if list
            .iter()
            .any(|d| matches!(d, ir::TreeDecorator::PaleMoss { .. }))
        {
            if !self
                .data
                .configured
                .contains_key("minecraft:pale_moss_patch")
            {
                return Err("pale_moss decorator without minecraft:pale_moss_patch".into());
            }
            Ok(
                quote! { |w: &mut W, r: &mut FeatureRandom, p: BlockPos| cf_pale_moss_patch(w, r, p) },
            )
        } else {
            Ok(quote! { |_: &mut W, _: &mut FeatureRandom, _: BlockPos| false })
        }
    }

    fn decorators(&mut self, list: &[ir::TreeDecorator]) -> Result<TokenStream> {
        use ir::TreeDecorator as D;
        let mut out = Vec::new();
        for d in list {
            out.push(match d {
                D::TrunkVine {} => quote! { TreeDecorator::TrunkVine },
                D::LeaveVine { probability } => quote! { TreeDecorator::LeaveVine { probability: #probability } },
                D::Cocoa { probability } => quote! { TreeDecorator::Cocoa { probability: #probability } },
                D::Beehive { probability } => quote! { TreeDecorator::Beehive { probability: #probability } },
                D::AlterGround { provider } => {
                    let p = self.state_provider(provider)?;
                    quote! { TreeDecorator::AlterGround { provider: #p } }
                }
                D::AttachedToLeaves {
                    probability,
                    exclusion_radius_xz,
                    exclusion_radius_y,
                    block_provider,
                    required_empty_blocks,
                    directions,
                } => {
                    let p = self.state_provider(block_provider)?;
                    let ds = directions.iter().map(|d| direction_tokens(*d));
                    quote! {
                        TreeDecorator::AttachedToLeaves {
                            probability: #probability, exclusion_radius_xz: #exclusion_radius_xz,
                            exclusion_radius_y: #exclusion_radius_y, block_provider: #p,
                            required_empty_blocks: #required_empty_blocks, directions: &[#(#ds),*],
                        }
                    }
                }
                D::AttachedToLogs {
                    probability,
                    block_provider,
                    directions,
                } => {
                    let p = self.state_provider(block_provider)?;
                    let ds = directions.iter().map(|d| direction_tokens(*d));
                    quote! {
                        TreeDecorator::AttachedToLogs { probability: #probability, block_provider: #p, directions: &[#(#ds),*] }
                    }
                }
                D::PlaceOnGround {
                    tries,
                    radius,
                    height,
                    block_state_provider,
                } => {
                    let p = self.state_provider(block_state_provider)?;
                    quote! {
                        TreeDecorator::PlaceOnGround { tries: #tries, radius: #radius, height: #height, block_state_provider: #p }
                    }
                }
                D::CreakingHeart { probability } => {
                    quote! { TreeDecorator::CreakingHeart { probability: #probability } }
                }
                D::PaleMoss {
                    leaves_probability,
                    trunk_probability,
                    ground_probability,
                } => quote! {
                    TreeDecorator::PaleMoss {
                        leaves_probability: #leaves_probability, trunk_probability: #trunk_probability,
                        ground_probability: #ground_probability,
                    }
                },
            });
        }
        Ok(quote! { &[#(#out),*] })
    }

    fn tree_config(&mut self, c: &ir::TreeConfig) -> Result<TokenStream> {
        use ir::{FeatureSize as S, FoliagePlacer as F, TrunkPlacer as T};
        let trunk_provider = self.state_provider(&c.trunk_provider)?;
        let foliage_provider = self.state_provider(&c.foliage_provider)?;
        let dirt_provider = self.state_provider(&c.dirt_provider)?;
        let heights = |b: i32, a: i32, r: i32| {
            quote! { TrunkHeights { base_height: #b, height_rand_a: #a, height_rand_b: #r } }
        };
        let trunk = match &c.trunk_placer {
            T::Straight {
                base_height,
                height_rand_a,
                height_rand_b,
            } => {
                let h = heights(*base_height, *height_rand_a, *height_rand_b);
                quote! { TrunkPlacer::Straight(#h) }
            }
            T::Forking {
                base_height,
                height_rand_a,
                height_rand_b,
            } => {
                let h = heights(*base_height, *height_rand_a, *height_rand_b);
                quote! { TrunkPlacer::Forking(#h) }
            }
            T::Giant {
                base_height,
                height_rand_a,
                height_rand_b,
            } => {
                let h = heights(*base_height, *height_rand_a, *height_rand_b);
                quote! { TrunkPlacer::Giant(#h) }
            }
            T::MegaJungle {
                base_height,
                height_rand_a,
                height_rand_b,
            } => {
                let h = heights(*base_height, *height_rand_a, *height_rand_b);
                quote! { TrunkPlacer::MegaJungle(#h) }
            }
            T::DarkOak {
                base_height,
                height_rand_a,
                height_rand_b,
            } => {
                let h = heights(*base_height, *height_rand_a, *height_rand_b);
                quote! { TrunkPlacer::DarkOak(#h) }
            }
            T::Fancy {
                base_height,
                height_rand_a,
                height_rand_b,
            } => {
                let h = heights(*base_height, *height_rand_a, *height_rand_b);
                quote! { TrunkPlacer::Fancy(#h) }
            }
            T::Bending {
                base_height,
                height_rand_a,
                height_rand_b,
                min_height_for_leaves,
                bend_length,
            } => {
                let h = heights(*base_height, *height_rand_a, *height_rand_b);
                let bl = self.int_provider(bend_length)?;
                quote! { TrunkPlacer::Bending { heights: #h, min_height_for_leaves: #min_height_for_leaves, bend_length: #bl } }
            }
            T::UpwardsBranching {
                base_height,
                height_rand_a,
                height_rand_b,
                extra_branch_steps,
                place_branch_per_log_probability,
                extra_branch_length,
                can_grow_through,
            } => {
                let h = heights(*base_height, *height_rand_a, *height_rand_b);
                let s = self.int_provider(extra_branch_steps)?;
                let l = self.int_provider(extra_branch_length)?;
                let g = self.tag(can_grow_through)?;
                quote! {
                    TrunkPlacer::UpwardsBranching {
                        heights: #h, extra_branch_steps: #s,
                        place_branch_per_log_probability: #place_branch_per_log_probability,
                        extra_branch_length: #l, can_grow_through: #g,
                    }
                }
            }
            T::Cherry {
                base_height,
                height_rand_a,
                height_rand_b,
                branch_count,
                branch_horizontal_length,
                branch_start_offset_from_top,
                branch_end_offset_from_top,
            } => {
                let h = heights(*base_height, *height_rand_a, *height_rand_b);
                let bc = self.int_provider(branch_count)?;
                let bh = self.int_provider(branch_horizontal_length)?;
                let (s0, s1) = (
                    branch_start_offset_from_top.min_inclusive,
                    branch_start_offset_from_top.max_inclusive,
                );
                let be = self.int_provider(branch_end_offset_from_top)?;
                quote! {
                    TrunkPlacer::Cherry {
                        heights: #h, branch_count: #bc, branch_horizontal_length: #bh,
                        branch_start_offset_from_top: (#s0, #s1), branch_end_offset_from_top: #be,
                    }
                }
            }
        };
        let foliage = match &c.foliage_placer {
            F::Blob {
                radius,
                offset,
                height,
            }
            | F::Bush {
                radius,
                offset,
                height,
            }
            | F::Fancy {
                radius,
                offset,
                height,
            }
            | F::Jungle {
                radius,
                offset,
                height,
            } => {
                let (r, o) = (self.int_provider(radius)?, self.int_provider(offset)?);
                let variant = match &c.foliage_placer {
                    F::Blob { .. } => quote! { Blob },
                    F::Bush { .. } => quote! { Bush },
                    F::Fancy { .. } => quote! { Fancy },
                    _ => quote! { Jungle },
                };
                quote! { FoliagePlacer::#variant { radius: #r, offset: #o, height: #height } }
            }
            F::Spruce {
                radius,
                offset,
                trunk_height,
            } => {
                let (r, o, t) = (
                    self.int_provider(radius)?,
                    self.int_provider(offset)?,
                    self.int_provider(trunk_height)?,
                );
                quote! { FoliagePlacer::Spruce { radius: #r, offset: #o, trunk_height: #t } }
            }
            F::Pine {
                radius,
                offset,
                height,
            } => {
                let (r, o, h) = (
                    self.int_provider(radius)?,
                    self.int_provider(offset)?,
                    self.int_provider(height)?,
                );
                quote! { FoliagePlacer::Pine { radius: #r, offset: #o, height: #h } }
            }
            F::Acacia { radius, offset } => {
                let (r, o) = (self.int_provider(radius)?, self.int_provider(offset)?);
                quote! { FoliagePlacer::Acacia { radius: #r, offset: #o } }
            }
            F::DarkOak { radius, offset } => {
                let (r, o) = (self.int_provider(radius)?, self.int_provider(offset)?);
                quote! { FoliagePlacer::DarkOak { radius: #r, offset: #o } }
            }
            F::MegaPine {
                radius,
                offset,
                crown_height,
            } => {
                let (r, o, h) = (
                    self.int_provider(radius)?,
                    self.int_provider(offset)?,
                    self.int_provider(crown_height)?,
                );
                quote! { FoliagePlacer::MegaPine { radius: #r, offset: #o, crown_height: #h } }
            }
            F::RandomSpread {
                radius,
                offset,
                foliage_height,
                leaf_placement_attempts,
            } => {
                let (r, o, h) = (
                    self.int_provider(radius)?,
                    self.int_provider(offset)?,
                    self.int_provider(foliage_height)?,
                );
                quote! {
                    FoliagePlacer::RandomSpread { radius: #r, offset: #o, foliage_height: #h, leaf_placement_attempts: #leaf_placement_attempts }
                }
            }
            F::Cherry {
                radius,
                offset,
                height,
                wide_bottom_layer_hole_chance,
                corner_hole_chance,
                hanging_leaves_chance,
                hanging_leaves_extension_chance,
            } => {
                let (r, o, h) = (
                    self.int_provider(radius)?,
                    self.int_provider(offset)?,
                    self.int_provider(height)?,
                );
                quote! {
                    FoliagePlacer::Cherry {
                        radius: #r, offset: #o, height: #h,
                        wide_bottom_layer_hole_chance: #wide_bottom_layer_hole_chance,
                        corner_hole_chance: #corner_hole_chance,
                        hanging_leaves_chance: #hanging_leaves_chance,
                        hanging_leaves_extension_chance: #hanging_leaves_extension_chance,
                    }
                }
            }
        };
        let root = match &c.root_placer {
            None => quote! { None },
            Some(ir::RootPlacer::Mangrove {
                trunk_offset_y,
                root_provider,
                above_root_placement,
                mangrove_root_placement: m,
            }) => {
                let t = self.int_provider(trunk_offset_y)?;
                let rp = self.state_provider(root_provider)?;
                let above = match above_root_placement {
                    Some(a) => {
                        let p = self.state_provider(&a.above_root_provider)?;
                        let ch = a.above_root_placement_chance;
                        quote! { Some((#p, #ch)) }
                    }
                    None => quote! { None },
                };
                let cg = self.tag(&m.can_grow_through)?;
                let mi = self.holder_set(&m.muddy_roots_in)?;
                let mp = self.state_provider(&m.muddy_roots_provider)?;
                let (w, l, s) = (m.max_root_width, m.max_root_length, m.random_skew_chance);
                quote! {
                    Some(RootPlacer::Mangrove {
                        trunk_offset_y: #t, root_provider: #rp, above_root_placement: #above,
                        can_grow_through: #cg, muddy_roots_in: #mi, muddy_roots_provider: #mp,
                        max_root_width: #w, max_root_length: #l, random_skew_chance: #s,
                    })
                }
            }
        };
        let opt = |v: Option<i32>| match v {
            Some(x) => quote! { Some(#x) },
            None => quote! { None },
        };
        let size = match &c.minimum_size {
            S::TwoLayers {
                limit,
                lower_size,
                upper_size,
                min_clipped_height,
            } => {
                let m = opt(*min_clipped_height);
                quote! { FeatureSize::TwoLayers { limit: #limit, lower_size: #lower_size, upper_size: #upper_size, min_clipped_height: #m } }
            }
            S::ThreeLayers {
                limit,
                upper_limit,
                lower_size,
                middle_size,
                upper_size,
                min_clipped_height,
            } => {
                let m = opt(*min_clipped_height);
                quote! {
                    FeatureSize::ThreeLayers {
                        limit: #limit, upper_limit: #upper_limit, lower_size: #lower_size,
                        middle_size: #middle_size, upper_size: #upper_size, min_clipped_height: #m,
                    }
                }
            }
        };
        let decorators = self.decorators(&c.decorators)?;
        let (iv, fd) = (c.ignore_vines, c.force_dirt);
        Ok(quote! {
            TreeConfig {
                trunk_provider: #trunk_provider, trunk_placer: #trunk, foliage_provider: #foliage_provider,
                foliage_placer: #foliage, root_placer: #root, dirt_provider: #dirt_provider,
                minimum_size: #size, decorators: #decorators, ignore_vines: #iv, force_dirt: #fd,
            }
        })
    }

    fn geode_config(&mut self, c: &ir::GeodeConfig) -> Result<TokenStream> {
        let b = &c.blocks;
        let fp = self.state_provider(&b.filling_provider)?;
        let ip = self.state_provider(&b.inner_layer_provider)?;
        let ap = self.state_provider(&b.alternate_inner_layer_provider)?;
        let mp = self.state_provider(&b.middle_layer_provider)?;
        let op = self.state_provider(&b.outer_layer_provider)?;
        let pl = self.states(&b.inner_placements)?;
        let cr = self.tag(&b.cannot_replace)?;
        let ib = self.tag(&b.invalid_blocks)?;
        let l = &c.layers;
        let (lf, li, lm, lo) = (l.filling, l.inner_layer, l.middle_layer, l.outer_layer);
        let k = &c.crack;
        let (gc, bc, co) = (
            k.generate_crack_chance,
            k.base_crack_size,
            k.crack_point_offset,
        );
        let owd = self.int_provider(&c.outer_wall_distance)?;
        let dp = self.int_provider(&c.distribution_points)?;
        let po = self.int_provider(&c.point_offset)?;
        let (upc, ualc, prla, mino, maxo, nm, ibt) = (
            c.use_potential_placements_chance,
            c.use_alternate_layer0_chance,
            c.placements_require_layer0_alternate,
            c.min_gen_offset,
            c.max_gen_offset,
            c.noise_multiplier,
            c.invalid_blocks_threshold,
        );
        Ok(quote! {
            GeodeConfig {
                filling_provider: #fp, inner_layer_provider: #ip, alternate_inner_layer_provider: #ap,
                middle_layer_provider: #mp, outer_layer_provider: #op, inner_placements: #pl,
                cannot_replace: #cr, invalid_blocks: #ib,
                filling: #lf, inner_layer: #li, middle_layer: #lm, outer_layer: #lo,
                generate_crack_chance: #gc, base_crack_size: #bc, crack_point_offset: #co,
                use_potential_placements_chance: #upc, use_alternate_layer0_chance: #ualc,
                placements_require_layer0_alternate: #prla, outer_wall_distance: #owd,
                distribution_points: #dp, point_offset: #po, min_gen_offset: #mino, max_gen_offset: #maxo,
                noise_multiplier: #nm, invalid_blocks_threshold: #ibt,
            }
        })
    }
}

fn direction_tokens(d: ir::Direction) -> TokenStream {
    match d {
        ir::Direction::Down => quote! { Direction::Down },
        ir::Direction::Up => quote! { Direction::Up },
        ir::Direction::North => quote! { Direction::North },
        ir::Direction::South => quote! { Direction::South },
        ir::Direction::West => quote! { Direction::West },
        ir::Direction::East => quote! { Direction::East },
    }
}

fn heightmap_tokens(h: HeightmapKind) -> TokenStream {
    match h {
        HeightmapKind::WorldSurfaceWg => quote! { Heightmap::WorldSurfaceWg },
        HeightmapKind::WorldSurface => quote! { Heightmap::WorldSurface },
        HeightmapKind::OceanFloorWg => quote! { Heightmap::OceanFloorWg },
        HeightmapKind::OceanFloor => quote! { Heightmap::OceanFloor },
        HeightmapKind::MotionBlocking => quote! { Heightmap::MotionBlocking },
        HeightmapKind::MotionBlockingNoLeaves => quote! { Heightmap::MotionBlockingNoLeaves },
    }
}

/// Java `FeatureSorter.buildFeaturesPerStep`: a stable topological order of
/// placed features per step, consistent with every biome's list order.
///
/// Nodes are `(step, first-seen index)`; edges join consecutive entries of each
/// biome's flattened step lists; a depth-first search over nodes in key order
/// emits post-order, which is reversed.
fn sort_features(biome_lists: &[&Vec<Vec<String>>]) -> Result<Vec<Vec<String>>> {
    let mut first_seen: HashMap<String, usize> = HashMap::new();
    let mut names: Vec<String> = Vec::new();
    type Node = (usize, usize); // (step, feature index)
    let mut graph: BTreeMap<Node, BTreeSet<Node>> = BTreeMap::new();
    let mut max_steps = 0;
    for lists in biome_lists {
        max_steps = max_steps.max(lists.len());
        let mut flat: Vec<Node> = Vec::new();
        for (step, list) in lists.iter().enumerate() {
            for f in list {
                let idx = *first_seen.entry(f.clone()).or_insert_with(|| {
                    names.push(f.clone());
                    names.len() - 1
                });
                flat.push((step, idx));
            }
        }
        for k in 0..flat.len() {
            let set = graph.entry(flat[k]).or_default();
            if k + 1 < flat.len() {
                set.insert(flat[k + 1]);
            }
        }
    }
    fn dfs(
        graph: &BTreeMap<Node, BTreeSet<Node>>,
        done: &mut BTreeSet<Node>,
        path: &mut BTreeSet<Node>,
        out: &mut Vec<Node>,
        node: Node,
    ) -> bool {
        if done.contains(&node) {
            return false;
        }
        if path.contains(&node) {
            return true;
        }
        path.insert(node);
        if let Some(next) = graph.get(&node) {
            for n in next {
                if dfs(graph, done, path, out, *n) {
                    return true;
                }
            }
        }
        path.remove(&node);
        done.insert(node);
        out.push(node);
        false
    }
    let mut done = BTreeSet::new();
    let mut path = BTreeSet::new();
    let mut order = Vec::new();
    for node in graph.keys() {
        if !done.contains(node) && dfs(&graph, &mut done, &mut path, &mut order, *node) {
            return Err("feature order cycle between biomes".into());
        }
    }
    order.reverse();
    let mut steps = vec![Vec::new(); max_steps];
    for (step, idx) in order {
        steps[step].push(names[idx].clone());
    }
    Ok(steps)
}

/// Biome order of a multi-noise parameter list (first appearance): either the
/// data generator report (`biomes: [{biome, parameters}]`) or the compact
/// `java_data` form whose `biomes` is already that ordered list.
fn parameter_list_order(path: &Path) -> Result<Vec<String>> {
    #[derive(serde::Deserialize)]
    struct Report {
        biomes: Vec<serde_json::Value>,
    }
    let r: Report = serde_json::from_str(&std::fs::read_to_string(path)?)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for e in r.biomes {
        let name = match &e {
            serde_json::Value::String(s) => s.clone(),
            other => other
                .get("biome")
                .and_then(|b| b.as_str())
                .ok_or_else(|| format!("{}: bad biome entry {other}", path.display()))?
                .to_string(),
        };
        if seen.insert(name.clone()) {
            out.push(name);
        }
    }
    Ok(out)
}

/// Emit `features.rs` into `out_dir`.
pub fn emit_features(
    out_dir: &Path,
    java_data: &Path,
    biomes: &HashMap<String, BiomeJson>,
    data: &FeatureData,
    blocks: &mut JavaBlocks,
) -> Result<()> {
    let mut biome_names: Vec<String> = biomes.keys().map(|s| format!("minecraft:{s}")).collect();
    biome_names.sort();
    if biome_names.len() > 128 {
        return Err("more than 128 biomes do not fit the u128 biome sets".into());
    }
    let biome_index: BTreeMap<String, usize> = biome_names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i))
        .collect();
    let biome_json = |name: &str| &biomes[name.strip_prefix("minecraft:").unwrap_or(name)];

    let placed_ids: BTreeMap<String, u16> = data
        .placed
        .keys()
        .enumerate()
        .map(|(i, n)| (n.clone(), i as u16))
        .collect();

    // Java-equal placed features (same configured reference, modifier lists of
    // only singleton modifiers) would share one sorter node; vanilla has none
    // among biome-listed features, and the emitter assumes that.
    {
        let mut seen: HashMap<String, &String> = HashMap::new();
        let listed: BTreeSet<&String> = biomes
            .values()
            .flat_map(|b| b.features.iter().flatten())
            .collect();
        for name in listed {
            let pf = data
                .placed
                .get(name)
                .ok_or_else(|| format!("biome lists unknown placed feature {name}"))?;
            let singleton_only = pf.placement.iter().all(|m| {
                matches!(
                    m,
                    PlacementModifier::InSquare {} | PlacementModifier::Biome {}
                )
            });
            if let (true, ConfiguredRef::Ref(cf)) = (singleton_only, &pf.feature) {
                let key = format!("{cf}|{:?}", pf.placement);
                if let Some(other) = seen.insert(key, name) {
                    return Err(format!("placed features {other} and {name} are Java-equal").into());
                }
            }
        }
    }

    let mut em = Emitter {
        blocks,
        data,
        biome_index,
        placed_ids: placed_ids.clone(),
        items: Vec::new(),
        inline_counter: 0,
        noise_counter: 0,
        nested_refs: BTreeSet::new(),
    };

    for (name, cf) in &data.configured {
        let id = format_ident!("cf_{}", sanitize(name));
        em.emit_configured(&id, cf)
            .map_err(|e| format!("configured feature {name}: {e}"))?;
    }
    for (name, pf) in &data.placed {
        let id = format_ident!("pf_{}", sanitize(name));
        em.emit_placed(&id, pf, Some(placed_ids[name]))
            .map_err(|e| format!("placed feature {name}: {e}"))?;
    }
    for name in &em.nested_refs {
        if data.placed[name]
            .placement
            .iter()
            .any(|m| matches!(m, PlacementModifier::Biome {}))
        {
            return Err(format!("{name} uses the biome filter but is placed from a config").into());
        }
    }

    // Biome membership (any step) per placed feature.
    let mut placed_biomes = vec![0u128; placed_ids.len()];
    for (bname, &bi) in &em.biome_index {
        for step in &biome_json(bname).features {
            for f in step {
                placed_biomes[placed_ids[f] as usize] |= 1u128 << bi;
            }
        }
    }

    // Per-dimension step order and per-biome step membership.
    let overworld = parameter_list_order(&java_data.join("biome_parameters_overworld.json"))?;
    let nether = parameter_list_order(&java_data.join("biome_parameters_nether.json"))?;
    let end: Vec<String> = END_BIOMES.iter().map(|s| s.to_string()).collect();
    let mut dims = Vec::new();
    for (dim_name, order) in [
        ("OVERWORLD", &overworld),
        ("NETHER", &nether),
        ("END", &end),
    ] {
        let lists: Vec<&Vec<Vec<String>>> = order
            .iter()
            .map(|b| {
                if !em.biome_index.contains_key(b) {
                    return Err(format!("{dim_name}: unknown biome {b}"));
                }
                Ok(&biome_json(b).features)
            })
            .collect::<std::result::Result<_, _>>()?;
        let steps = sort_features(&lists)?;
        if steps.len() > STEPS.len() {
            return Err(format!("{dim_name}: more than 11 generation steps").into());
        }
        let step_index: Vec<HashMap<&String, u16>> = steps
            .iter()
            .map(|s| s.iter().enumerate().map(|(i, n)| (n, i as u16)).collect())
            .collect();
        let step_tokens = (0..STEPS.len()).map(|s| {
            let ids = steps
                .get(s)
                .map(|l| l.iter().map(|n| placed_ids[n]).collect::<Vec<_>>())
                .unwrap_or_default();
            quote! { &[#(#ids),*] }
        });
        let biome_tokens = biome_names.iter().map(|bname| {
            let rows = (0..STEPS.len()).map(|s| {
                let mut idx: Vec<u16> = Vec::new();
                if order.contains(bname)
                    && let Some(list) = biome_json(bname).features.get(s)
                {
                    for f in list {
                        idx.push(step_index[s][f]);
                    }
                }
                idx.sort_unstable();
                idx.dedup();
                quote! { &[#(#idx),*] }
            });
            quote! { [#(#rows),*] }
        });
        let biomes_ident = format_ident!("{}_BIOMES", dim_name);
        let mut bits = 0u128;
        for b in order.iter() {
            bits |= 1u128 << em.biome_index[b];
        }
        let bits = Literal::u128_suffixed(bits);
        dims.push(quote! {
            /// Possible biomes of the dimension's biome source (bit per biome index).
            pub static #biomes_ident: u128 = #bits;
        });
        let ident = format_ident!("{}_FEATURES", dim_name);
        dims.push(quote! {
            pub static #ident: DimensionFeatures = DimensionFeatures {
                steps: [#(#step_tokens),*],
                biome_steps: [#(#biome_tokens),*],
            };
        });
    }

    let n_biomes = biome_names.len();
    let climates = biome_names
        .iter()
        .map(|name| {
            let b = biome_json(name);
            let has_precipitation = b.has_precipitation;
            let temperature = b.temperature;
            let downfall = b.downfall;
            let frozen = match b.temperature_modifier.as_deref() {
                None | Some("none") => false,
                Some("frozen") => true,
                Some(other) => {
                    return Err(format!(
                        "biome {name}: unknown temperature_modifier {other}"
                    ));
                }
            };
            Ok(quote! {
                BiomeClimate {
                    has_precipitation: #has_precipitation,
                    temperature: #temperature,
                    frozen: #frozen,
                    downfall: #downfall,
                }
            })
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let n_placed = placed_ids.len();
    let placed_names = placed_ids.keys();
    let dispatch = placed_ids.iter().map(|(name, id)| {
        let f = format_ident!("pf_{}", sanitize(name));
        quote! { #id => #f(world, rng, origin), }
    });
    let biome_bits = placed_biomes.iter().map(|b| Literal::u128_suffixed(*b));
    let items = &em.items;
    let tokens = quote! {
        use super::climate::BiomeClimate;

        /// Number of Java biomes (all dimensions).
        pub const BIOME_COUNT: usize = #n_biomes;
        /// Java biome ids in index order (sorted by name); `FeatureWorld::biome`
        /// returns indices into this list.
        pub static BIOME_NAMES: [&str; #n_biomes] = [#(#biome_names),*];
        /// Climate settings per biome index.
        pub static BIOME_CLIMATE: [BiomeClimate; #n_biomes] = [#(#climates),*];

        /// Biome index by id (`minecraft:plains`).
        pub fn biome_index(name: &str) -> Option<u8> {
            BIOME_NAMES.binary_search(&name).ok().map(|i| i as u8)
        }

        /// Number of placed features.
        pub const PLACED_COUNT: usize = #n_placed;
        /// Placed feature ids in id order.
        pub static PLACED_NAMES: [&str; #n_placed] = [#(#placed_names),*];
        /// Biomes (bit per biome index) whose feature lists contain each placed feature.
        pub static PLACED_BIOMES: [u128; #n_placed] = [#(#biome_bits),*];

        /// Decoration order of one dimension.
        pub struct DimensionFeatures {
            /// Placed feature ids per generation step, in Java's sorted order;
            /// the position in this list is the feature index used for seeding.
            pub steps: [&'static [u16]; 11],
            /// Per biome and step: sorted positions into `steps[step]`.
            pub biome_steps: [[&'static [u16]; 11]; #n_biomes],
        }

        #(#dims)*
    };
    write_pretty(&out_dir.join("features.rs"), tokens)?;

    // Compiled placed/configured features (`features_code.rs`), included by
    // `features::generated` next to the tables above.
    let code = quote! {
        use crate::features::blocks::{
            BlockPos, BlockPredicate, BlockSet, Direction, FluidSet, Heightmap, JavaBlock, JavaState,
            NoiseSpec, RuleBasedStateProvider, RuleTest, StateProvider,
        };
        use crate::features::config::*;
        use crate::features::eval::FeatureWorld;
        use crate::features::kinds;
        use crate::features::noise::{self, LegacyNoise};
        use crate::features::placement;
        use crate::features::random::FeatureRandom;
        use crate::features::survival;
        use crate::features::values::{height, FloatProvider, IntProvider};
        use std::sync::LazyLock;

        /// Place a placed feature by id (`PlacedFeature.placeWithBiomeCheck`).
        pub fn place_placed<W: FeatureWorld + ?Sized>(id: u16, world: &mut W, rng: &mut FeatureRandom, origin: BlockPos) -> bool {
            match id {
                #(#dispatch)*
                _ => panic!("unknown placed feature id {id}"),
            }
        }

        #(#items)*
    };
    write_pretty(&out_dir.join("features_code.rs"), code)
}
