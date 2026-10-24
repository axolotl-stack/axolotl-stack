//! `minecraft:tree_feature` -> `features::tree::place` with a static
//! `features::tree::Params` literal (algorithms in the runtime module).
//!
//! Conventions fixed here:
//! - `{"range_min": a, "range_max": b}` is the half-open range `[a, b)` and is
//!   emitted as the inclusive `IntRange { a, b - 1 }` (`{a, a}` when empty,
//!   e.g. the poplar `4..4`). With this reading every vanilla tree range
//!   reproduces the classic generator formulas (oak trunks 4..=6, spruce
//!   trunk shortening 0..=2, mega spruce canopies 13..=17 layers, mangrove
//!   trunk offsets 1..=3, savanna lean 1..=4 layers). `{"min", "max"}` is
//!   inclusive; a number is a constant.
//! - Numeric `*_chance` values are percentages (`decoration_chance: 75.0`,
//!   `branch_chance: 100.0`, the cherry chances `25`, `50`, `16.6666`);
//!   `poplar_canopy.side_hole_chance` (`0.15`) is a probability.
//! - Log blocks expand to their three `pillar_axis` states (branches and
//!   fallen logs lie along their direction); decoration blocks expand to the
//!   four `vine_direction_bits` faces (bit 1 south .. 8 east = side of the
//!   support).
//! - `log_decoration_feature` becomes the closure argument of `place`
//!   (statics cannot hold generic functions).
//! - Defaults of keys absent from some documents reproduce the classic oak:
//!   `canopy.min_width` 1, `canopy_slope` rise 1 per 2 layers, no corner
//!   variation without `variation_chance`, `core_width` 1, `simplify_canopy`
//!   false, `height_modifier` 0.

use crate::blocks::{BlockPalette, StateValue};
use crate::emit::Emitter;
use crate::error::{Error, Result};
use crate::ir::{Obj, block, block_set, int, num};
use proc_macro2::TokenStream;
use quote::quote;
use serde_json::Value;

const TRUNKS: [&str; 8] = [
    "trunk",
    "fancy_trunk",
    "mega_trunk",
    "acacia_trunk",
    "mangrove_trunk",
    "cherry_trunk",
    "poplar_trunk",
    "fallen_trunk",
];

const CANOPIES: [&str; 12] = [
    "canopy",
    "fancy_canopy",
    "mega_canopy",
    "mega_pine_canopy",
    "pine_canopy",
    "spruce_canopy",
    "roofed_canopy",
    "acacia_canopy",
    "random_spread_canopy",
    "mangrove_canopy",
    "cherry_canopy",
    "poplar_canopy",
];

pub fn emit(em: &mut Emitter<'_>, o: &Obj<'_>) -> Result<TokenStream> {
    let origin = o.origin;
    let model = em.model;
    let palette = &model.palette;
    let mut items = Vec::new();

    let grow_on = em.matches(quote!(b), &block_set(palette, origin, o.req("may_grow_on")?)?);
    let replace = em.matches(quote!(b), &block_set(palette, origin, o.req("may_replace")?)?);
    items.push(quote! {
        fn may_grow_on(b: u32) -> bool { #grow_on }
        fn may_replace(b: u32) -> bool { #replace }
    });
    items.push(match o.opt("may_grow_through") {
        Some(v) => {
            let test = em.matches(quote!(b), &block_set(palette, origin, v)?);
            quote!(fn may_grow_through(b: u32) -> bool { #test })
        }
        None => quote!(fn may_grow_through(_: u32) -> bool { false }),
    });

    let base_list = match o.req("base_block")? {
        Value::Array(list) => list.as_slice(),
        single => std::slice::from_ref(single),
    };
    let mut base = Vec::new();
    for v in base_list {
        base.push(em.block(block(palette, origin, v)?));
    }
    if base.is_empty() {
        return Err(Error::data(origin, "base_block is empty"));
    }

    let base_cluster = match o.opt("base_cluster") {
        Some(v) => {
            let c = Obj::new(origin, v)?;
            let set = block_set(palette, origin, c.req("may_replace")?)?;
            let count = c.i32("num_clusters")?;
            let radius = c.i32("cluster_radius")?;
            c.finish()?;
            let test = em.matches(quote!(b), &set);
            items.push(quote!(fn cluster_may_replace(b: u32) -> bool { #test }));
            quote!(Some(features::tree::BaseCluster {
                may_replace: cluster_may_replace,
                num_clusters: #count,
                radius: #radius,
            }))
        }
        None => quote!(None),
    };
    let roots = match o.opt("mangrove_roots") {
        Some(v) => mangrove_roots(em, origin, v, &mut items)?,
        None => quote!(None),
    };

    let canopy_keys: Vec<&'static str> = CANOPIES.iter().copied().filter(|&k| o.opt(k).is_some()).collect();
    let (canopy, fancy_height) = match canopy_keys.as_slice() {
        [] => (quote!(None), None),
        &[key] => {
            let (tokens, fancy_height) = parse_canopy(em, origin, key, o.req(key)?)?;
            (quote!(Some(#tokens)), fancy_height)
        }
        many => return Err(Error::data(origin, format!("tree with several canopies: {many:?}"))),
    };

    let trunk_keys: Vec<&'static str> = TRUNKS.iter().copied().filter(|&k| o.opt(k).is_some()).collect();
    let &[trunk_key] = trunk_keys.as_slice() else {
        return Err(Error::data(origin, format!("tree needs exactly one trunk, found {trunk_keys:?}")));
    };
    let (trunk, log_feature) = parse_trunk(em, origin, trunk_key, o.req(trunk_key)?, fancy_height, &mut items)?;

    let log_decoration = log_feature.is_some();
    let decorate = match &log_feature {
        Some(id) => {
            let call = em.call(id, quote!(p));
            quote!(|ctx, p| #call)
        }
        None => quote!(|_, _| None),
    };
    Ok(quote! {
        #(#items)*
        static PARAMS: features::tree::Params = features::tree::Params {
            may_grow_on,
            may_replace,
            may_grow_through,
            base_block: &[#(#base),*],
            base_cluster: #base_cluster,
            roots: #roots,
            trunk: #trunk,
            canopy: #canopy,
            log_decoration: #log_decoration,
        };
        features::tree::place(ctx, pos, &PARAMS, #decorate)
    })
}

// ---------------------------------------------------------------------------
// Value helpers
// ---------------------------------------------------------------------------

/// Inclusive bounds of a range value (see the module docs).
fn range(origin: &str, v: &Value) -> Result<(i32, i32)> {
    if v.is_number() {
        let n = int(origin, "range", v)?;
        return Ok((n, n));
    }
    let o = Obj::new(origin, v)?;
    let bounds = if let (Some(a), Some(b)) = (o.opt("range_min"), o.opt("range_max")) {
        let (a, b) = (int(origin, "range_min", a)?, int(origin, "range_max", b)?);
        (a, (b - 1).max(a))
    } else if let (Some(a), Some(b)) = (o.opt("min"), o.opt("max")) {
        (int(origin, "min", a)?, int(origin, "max", b)?)
    } else {
        return Err(Error::data(origin, format!("not an int range: {v}")));
    };
    o.finish()?;
    Ok(bounds)
}

fn range_tokens(origin: &str, v: &Value) -> Result<TokenStream> {
    let (a, b) = range(origin, v)?;
    Ok(quote!(features::IntRange { min: #a, max: #b }))
}

/// An optional range; absent is `0`.
fn range_or_zero(origin: &str, v: Option<&Value>) -> Result<TokenStream> {
    match v {
        Some(v) => range_tokens(origin, v),
        None => Ok(quote!(features::IntRange { min: 0i32, max: 0i32 })),
    }
}

/// `{"numerator", "denominator"}` or a percentage.
fn percent_chance(origin: &str, v: &Value) -> Result<TokenStream> {
    if v.is_number() {
        let p = num(origin, "chance", v)? / 100.0;
        return Ok(quote!(features::Chance::Probability(#p)));
    }
    let o = Obj::new(origin, v)?;
    let n = o.i32("numerator")?;
    let d = o.i32("denominator")?;
    o.finish()?;
    Ok(quote!(features::Chance::Fraction(#n, #d)))
}

/// `id` with state `key` set to `value` (other states kept); `id` itself for
/// blocks without that state.
fn with_state(palette: &BlockPalette, id: u32, key: &str, value: &StateValue) -> u32 {
    let Some((name, _, state)) = palette.describe(id) else {
        return id;
    };
    if !state.contains_key(key) {
        return id;
    }
    palette
        .states_of(name)
        .into_iter()
        .flatten()
        .find(|(_, s)| s.iter().all(|(k, v)| if k == key { v == value } else { state.get(k) == Some(v) }))
        .map_or(id, |(i, _)| i)
}

/// `features::tree::Log` with the three `pillar_axis` states of a block.
fn log_tokens(em: &mut Emitter<'_>, palette: &BlockPalette, origin: &str, v: &Value) -> Result<TokenStream> {
    let id = block(palette, origin, v)?;
    let [x, y, z] = ["x", "y", "z"].map(|axis| with_state(palette, id, "pillar_axis", &StateValue::Str(axis.to_owned())));
    let (x, y, z) = (em.block(x), em.block(y), em.block(z));
    Ok(quote!(features::tree::Log { x: #x, y: #y, z: #z }))
}

/// `[south, west, north, east]` support-side states of a decoration block.
fn side_tokens(em: &mut Emitter<'_>, palette: &BlockPalette, id: u32) -> TokenStream {
    let states: Vec<TokenStream> = [1, 2, 4, 8]
        .into_iter()
        .map(|bit| em.block(with_state(palette, id, "vine_direction_bits", &StateValue::Int(bit))))
        .collect();
    quote!([#(#states),*])
}

fn leaf_tokens(em: &mut Emitter<'_>, palette: &BlockPalette, origin: &str, v: &Value) -> Result<TokenStream> {
    Ok(em.block(block(palette, origin, v)?))
}

/// `trunk_decoration` -> `Option<TrunkDecoration>`.
fn trunk_decoration(em: &mut Emitter<'_>, palette: &BlockPalette, origin: &str, v: Option<&Value>) -> Result<TokenStream> {
    let Some(v) = v else {
        return Ok(quote!(None));
    };
    let o = Obj::new(origin, v)?;
    let id = block(palette, origin, o.req("decoration_block")?)?;
    let chance = percent_chance(origin, o.req("decoration_chance")?)?;
    o.finish()?;
    let block = side_tokens(em, palette, id);
    Ok(quote!(Some(features::tree::TrunkDecoration { block: #block, chance: #chance })))
}

/// `canopy_decoration` -> `Option<CanopyDecoration>`.
fn canopy_decoration(em: &mut Emitter<'_>, palette: &BlockPalette, origin: &str, v: Option<&Value>) -> Result<TokenStream> {
    let Some(v) = v else {
        return Ok(quote!(None));
    };
    let o = Obj::new(origin, v)?;
    let id = block(palette, origin, o.req("decoration_block")?)?;
    let chance = percent_chance(origin, o.req("decoration_chance")?)?;
    let num_steps = o.i32("num_steps")?;
    let step: i32 = match o.str("step_direction")? {
        "down" => -1,
        "up" => 1,
        other => return Err(Error::data(origin, format!("unknown step_direction {other:?}"))),
    };
    o.finish()?;
    let block = side_tokens(em, palette, id);
    Ok(quote!(Some(features::tree::CanopyDecoration {
        block: #block,
        chance: #chance,
        num_steps: #num_steps,
        step: #step,
    })))
}

/// `trunk_height` -> `Height` plus `min_height_for_canopy`.
fn height(origin: &str, v: &Value) -> Result<(TokenStream, Option<i32>)> {
    if v.get("base").is_none() {
        let r = range_tokens(origin, v)?;
        return Ok((quote!(features::tree::Height::Range(#r)), None));
    }
    let o = Obj::new(origin, v)?;
    let base = o.i32("base")?;
    let min_canopy = o
        .opt("min_height_for_canopy")
        .map(|m| int(origin, "min_height_for_canopy", m))
        .transpose()?;
    let tokens = if let Some(list) = o.opt("intervals") {
        let list = list
            .as_array()
            .ok_or_else(|| Error::data(origin, "trunk_height.intervals is not an array"))?
            .iter()
            .map(|n| int(origin, "intervals", n))
            .collect::<Result<Vec<i32>>>()?;
        quote!(features::tree::Height::Intervals { base: #base, intervals: &[#(#list),*] })
    } else if let Some(a) = o.opt("height_rand_a") {
        let a = int(origin, "height_rand_a", a)?;
        let b = o.i32("height_rand_b")?;
        quote!(features::tree::Height::Rand { base: #base, a: #a, b: #b })
    } else {
        return Err(Error::data(origin, format!("unsupported trunk_height {v}")));
    };
    o.finish()?;
    Ok((tokens, min_canopy))
}

/// `trunk_height` of trunks without per-log canopies.
fn plain_height(origin: &str, v: &Value) -> Result<TokenStream> {
    match height(origin, v)? {
        (tokens, None) => Ok(tokens),
        (_, Some(_)) => Err(Error::data(origin, "min_height_for_canopy is only supported on acacia_trunk")),
    }
}

// ---------------------------------------------------------------------------
// Roots, trunks, canopies
// ---------------------------------------------------------------------------

fn mangrove_roots(em: &mut Emitter<'_>, origin: &str, v: &Value, items: &mut Vec<TokenStream>) -> Result<TokenStream> {
    let model = em.model;
    let palette = &model.palette;
    let r = Obj::new(origin, v)?;
    let max_width = r.i32("max_root_width")?;
    let max_length = r.i32("max_root_length")?;
    let y_offset = range_tokens(origin, r.req("y_offset")?)?;
    let root = leaf_tokens(em, palette, origin, r.req("root_block")?)?;
    let muddy = leaf_tokens(em, palette, origin, r.req("muddy_root_block")?)?;
    let mud = leaf_tokens(em, palette, origin, r.req("mud_block")?)?;
    let above = match r.opt("above_root") {
        Some(v) => {
            let a = Obj::new(origin, v)?;
            let b = leaf_tokens(em, palette, origin, a.req("above_root_block")?)?;
            let c = percent_chance(origin, a.req("above_root_chance")?)?;
            a.finish()?;
            quote!(Some((#b, #c)))
        }
        None => quote!(None),
    };
    let through = block_set(palette, origin, r.req("roots_may_grow_through")?)?;
    r.finish()?;
    let test = em.matches(quote!(b), &through);
    items.push(quote!(fn roots_may_grow_through(b: u32) -> bool { #test }));
    Ok(quote!(Some(features::tree::MangroveRoots {
        max_width: #max_width,
        max_length: #max_length,
        y_offset: #y_offset,
        root: #root,
        muddy_root: #muddy,
        mud: #mud,
        above_root: #above,
        may_grow_through: roots_may_grow_through,
    })))
}

/// `branch_canopy: {"<kind>": {...}}` -> `&BRANCH_CANOPY` (a sibling static).
fn branch_canopy(em: &mut Emitter<'_>, origin: &str, v: &Value, items: &mut Vec<TokenStream>) -> Result<TokenStream> {
    let o = Obj::new(origin, v)?;
    let keys: Vec<&'static str> = CANOPIES.iter().copied().filter(|&k| o.opt(k).is_some()).collect();
    let &[key] = keys.as_slice() else {
        return Err(Error::data(origin, format!("branch_canopy needs exactly one canopy, found {keys:?}")));
    };
    let (tokens, _) = parse_canopy(em, origin, key, o.req(key)?)?;
    o.finish()?;
    items.push(quote!(static BRANCH_CANOPY: features::tree::Canopy = #tokens;));
    Ok(quote!(&BRANCH_CANOPY))
}

/// One trunk -> `features::tree::Trunk` and its `log_decoration_feature`.
fn parse_trunk(
    em: &mut Emitter<'_>,
    origin: &str,
    key: &str,
    v: &Value,
    fancy_height: Option<i32>,
    items: &mut Vec<TokenStream>,
) -> Result<(TokenStream, Option<String>)> {
    let model = em.model;
    let palette = &model.palette;
    let o = Obj::new(origin, v)?;
    let log = log_tokens(em, palette, origin, o.req("trunk_block")?)?;
    let feature = o.opt("log_decoration_feature").map(|f| {
        f.as_str()
            .map(str::to_owned)
            .ok_or_else(|| Error::data(origin, "log_decoration_feature is not a string"))
    });
    let feature = feature.transpose()?;
    if feature.is_some() && !matches!(key, "poplar_trunk" | "fallen_trunk") {
        return Err(Error::data(origin, format!("log_decoration_feature is not supported on {key}")));
    }
    let tokens = match key {
        "trunk" => {
            let h = plain_height(origin, o.req("trunk_height")?)?;
            let modifier = range_or_zero(origin, o.opt("height_modifier"))?;
            let deco = trunk_decoration(em, palette, origin, o.opt("trunk_decoration"))?;
            let depth = match o.opt("can_be_submerged") {
                Some(v) => {
                    let s = Obj::new(origin, v)?;
                    let d = s.i32("max_depth")?;
                    s.finish()?;
                    quote!(Some(#d))
                }
                None => quote!(None),
            };
            quote!(features::tree::Trunk::Straight(features::tree::StraightTrunk {
                height: #h,
                height_modifier: #modifier,
                log: #log,
                decoration: #deco,
                max_water_depth: #depth,
            }))
        }
        "fancy_trunk" => {
            let th = Obj::new(origin, o.req("trunk_height")?)?;
            let base = th.i32("base")?;
            let variance = th.i32("variance")?;
            let scale = th.f32("scale")?;
            th.finish()?;
            let b = Obj::new(origin, o.req("branches")?)?;
            let slope = b.f32("slope")?;
            let density = b.f32("density")?;
            let min_altitude = b.f32("min_altitude_factor")?;
            b.finish()?;
            let width = o.i32("trunk_width")?;
            let width_scale = o.f32("width_scale")?;
            let foliage_altitude = o.f32("foliage_altitude_factor")?;
            let foliage_height = fancy_height
                .ok_or_else(|| Error::data(origin, "fancy_trunk needs a fancy_canopy (its height spaces the clusters)"))?;
            quote!(features::tree::Trunk::Fancy(features::tree::FancyTrunk {
                base: #base,
                variance: #variance,
                scale: #scale,
                width: #width,
                log: #log,
                branch_slope: #slope,
                branch_density: #density,
                min_altitude_factor: #min_altitude,
                width_scale: #width_scale,
                foliage_altitude_factor: #foliage_altitude,
                foliage_height: #foliage_height,
            }))
        }
        "mega_trunk" => {
            let width = o.i32("trunk_width")?;
            let h = plain_height(origin, o.req("trunk_height")?)?;
            let deco = trunk_decoration(em, palette, origin, o.opt("trunk_decoration"))?;
            let branches = match o.opt("branches") {
                Some(v) => {
                    let b = Obj::new(origin, v)?;
                    let length = b.i32("branch_length")?;
                    let slope = b.f32("branch_slope")?;
                    let interval = range_tokens(origin, b.req("branch_interval")?)?;
                    let alt = Obj::new(origin, b.req("branch_altitude_factor")?)?;
                    let altitude_min = alt.f32("min")?;
                    let altitude_max = alt.f32("max")?;
                    alt.finish()?;
                    let canopy = branch_canopy(em, origin, b.req("branch_canopy")?, items)?;
                    b.finish()?;
                    quote!(Some(features::tree::MegaBranches {
                        length: #length,
                        slope: #slope,
                        interval: #interval,
                        altitude_min: #altitude_min,
                        altitude_max: #altitude_max,
                        canopy: #canopy,
                    }))
                }
                None => quote!(None),
            };
            quote!(features::tree::Trunk::Mega(features::tree::MegaTrunk {
                height: #h,
                width: #width,
                log: #log,
                decoration: #deco,
                branches: #branches,
            }))
        }
        "acacia_trunk" => {
            let width = o.i32("trunk_width")?;
            let (h, min_canopy) = height(origin, o.req("trunk_height")?)?;
            let min_canopy = match min_canopy {
                Some(m) => quote!(Some(#m)),
                None => quote!(None),
            };
            let lean = match o.opt("trunk_lean") {
                Some(v) => {
                    let l = Obj::new(origin, v)?;
                    let diagonal = match l.req("allow_diagonal_growth")? {
                        Value::Bool(b) => *b,
                        other => return Err(Error::data(origin, format!("allow_diagonal_growth is not a bool: {other}"))),
                    };
                    let lean_height = range_tokens(origin, l.req("lean_height")?)?;
                    let steps = range_tokens(origin, l.req("lean_steps")?)?;
                    let length = match l.opt("lean_length") {
                        Some(v) => {
                            let r = range_tokens(origin, v)?;
                            quote!(Some(#r))
                        }
                        None => quote!(None),
                    };
                    l.finish()?;
                    quote!(Some(features::tree::Lean {
                        allow_diagonal: #diagonal,
                        height: #lean_height,
                        steps: #steps,
                        length: #length,
                    }))
                }
                None => quote!(None),
            };
            let deco = trunk_decoration(em, palette, origin, o.opt("trunk_decoration"))?;
            let branches = match o.opt("branches") {
                Some(v) => {
                    let b = Obj::new(origin, v)?;
                    let chance = percent_chance(origin, b.req("branch_chance")?)?;
                    let length = range_tokens(origin, b.req("branch_length")?)?;
                    let position = range_tokens(origin, b.req("branch_position")?)?;
                    let canopy = branch_canopy(em, origin, b.req("branch_canopy")?, items)?;
                    b.finish()?;
                    quote!(Some(features::tree::AcaciaBranches {
                        chance: #chance,
                        length: #length,
                        position: #position,
                        canopy: #canopy,
                    }))
                }
                None => quote!(None),
            };
            quote!(features::tree::Trunk::Acacia(features::tree::AcaciaTrunk {
                height: #h,
                min_height_for_canopy: #min_canopy,
                width: #width,
                log: #log,
                lean: #lean,
                decoration: #deco,
                branches: #branches,
            }))
        }
        "mangrove_trunk" => {
            let width = o.i32("trunk_width")?;
            let h = plain_height(origin, o.req("trunk_height")?)?;
            let branches = match o.opt("branches") {
                Some(v) => {
                    let b = Obj::new(origin, v)?;
                    let chance = percent_chance(origin, b.req("branch_chance")?)?;
                    let length = range_tokens(origin, b.req("branch_length")?)?;
                    let steps = range_tokens(origin, b.req("branch_steps")?)?;
                    b.finish()?;
                    quote!(Some(features::tree::MangroveBranches { chance: #chance, length: #length, steps: #steps }))
                }
                None => quote!(None),
            };
            quote!(features::tree::Trunk::Mangrove(features::tree::MangroveTrunk {
                height: #h,
                width: #width,
                log: #log,
                branches: #branches,
            }))
        }
        "cherry_trunk" => {
            let h = plain_height(origin, o.req("trunk_height")?)?;
            let b = Obj::new(origin, o.req("branches")?)?;
            let w = Obj::new(origin, b.req("tree_type_weights")?)?;
            let weights = vec![w.i32("one_branch")?, w.i32("two_branches")?, w.i32("two_branches_and_trunk")?];
            w.finish()?;
            let horizontal = range_tokens(origin, b.req("branch_horizontal_length")?)?;
            let start = range_tokens(origin, b.req("branch_start_offset_from_top")?)?;
            let end = range_tokens(origin, b.req("branch_end_offset_from_top")?)?;
            let canopy = branch_canopy(em, origin, b.req("branch_canopy")?, items)?;
            b.finish()?;
            quote!(features::tree::Trunk::Cherry(features::tree::CherryTrunk {
                height: #h,
                log: #log,
                branches: features::tree::CherryBranches {
                    weights: [#(#weights),*],
                    horizontal_length: #horizontal,
                    start_offset: #start,
                    end_offset: #end,
                    canopy: #canopy,
                },
            }))
        }
        "poplar_trunk" => {
            let h = plain_height(origin, o.req("trunk_height")?)?;
            let remaining = range_tokens(origin, o.req("remaining_trunk_height_above_branches")?)?;
            let support = range_tokens(origin, o.req("amount_of_foliage_support_branches")?)?;
            quote!(features::tree::Trunk::Poplar(features::tree::PoplarTrunk {
                height: #h,
                log: #log,
                remaining_above_branches: #remaining,
                support_branches: #support,
            }))
        }
        "fallen_trunk" => {
            let length = range_tokens(origin, o.req("log_length")?)?;
            let modifier = range_or_zero(origin, o.opt("height_modifier"))?;
            let deco = trunk_decoration(em, palette, origin, o.opt("trunk_decoration"))?;
            quote!(features::tree::Trunk::Fallen(features::tree::FallenTrunk {
                length: #length,
                height_modifier: #modifier,
                log: #log,
                decoration: #deco,
            }))
        }
        other => return Err(Error::data(origin, format!("unknown trunk kind {other}"))),
    };
    o.finish()?;
    Ok((tokens, feature))
}

/// One canopy -> `features::tree::Canopy`, plus the cluster height of a
/// `fancy_canopy` (the fancy trunk spaces its clusters by it).
fn parse_canopy(em: &mut Emitter<'_>, origin: &str, key: &str, v: &Value) -> Result<(TokenStream, Option<i32>)> {
    let model = em.model;
    let palette = &model.palette;
    let o = Obj::new(origin, v)?;
    let mut fancy_height = None;
    let tokens = match key {
        "canopy" => {
            let offset = Obj::new(origin, o.req("canopy_offset")?)?;
            let (lo, hi) = (offset.i32("min")?, offset.i32("max")?);
            offset.finish()?;
            let min_width = o.i32_or("min_width", 1)?;
            let (rise, run) = match o.opt("canopy_slope") {
                Some(v) => {
                    let s = Obj::new(origin, v)?;
                    let slope = (s.i32("rise")?, s.i32("run")?);
                    s.finish()?;
                    slope
                }
                None => (1, 2),
            };
            let variation = match o.opt("variation_chance") {
                None => Vec::new(),
                Some(Value::Array(list)) => list.iter().map(|c| percent_chance(origin, c)).collect::<Result<Vec<_>>>()?,
                Some(single) => vec![percent_chance(origin, single)?],
            };
            let leaf = leaf_tokens(em, palette, origin, o.req("leaf_block")?)?;
            let decoration = canopy_decoration(em, palette, origin, o.opt("canopy_decoration"))?;
            quote!(features::tree::Canopy::Blob(features::tree::BlobCanopy {
                offset_min: #lo,
                offset_max: #hi,
                min_width: #min_width,
                rise: #rise,
                run: #run,
                variation: &[#(#variation),*],
                leaf: #leaf,
                decoration: #decoration,
            }))
        }
        "fancy_canopy" => {
            let height = o.i32("height")?;
            let radius = o.i32("radius")?;
            let leaf = leaf_tokens(em, palette, origin, o.req("leaf_block")?)?;
            fancy_height = Some(height);
            quote!(features::tree::Canopy::Fancy(features::tree::FancyCanopy { height: #height, radius: #radius, leaf: #leaf }))
        }
        "mega_canopy" => {
            let height = range_tokens(origin, o.req("canopy_height")?)?;
            let base_radius = o.i32("base_radius")?;
            let core_width = o.i32_or("core_width", 1)?;
            let simplify = o.bool_or("simplify_canopy", false)?;
            let leaf = leaf_tokens(em, palette, origin, o.req("leaf_block")?)?;
            quote!(features::tree::Canopy::Mega(features::tree::MegaCanopy {
                height: #height,
                base_radius: #base_radius,
                core_width: #core_width,
                simplify: #simplify,
                leaf: #leaf,
            }))
        }
        "mega_pine_canopy" => {
            let height = range_tokens(origin, o.req("canopy_height")?)?;
            let base_radius = o.i32("base_radius")?;
            let step = o.f32("radius_step_modifier")?;
            let core_width = o.i32_or("core_width", 1)?;
            let leaf = leaf_tokens(em, palette, origin, o.req("leaf_block")?)?;
            quote!(features::tree::Canopy::MegaPine(features::tree::MegaPineCanopy {
                height: #height,
                base_radius: #base_radius,
                radius_step: #step,
                core_width: #core_width,
                leaf: #leaf,
            }))
        }
        "pine_canopy" => {
            let base_radius = o.i32("base_radius")?;
            let height = range_tokens(origin, o.req("canopy_height")?)?;
            let leaf = leaf_tokens(em, palette, origin, o.req("leaf_block")?)?;
            quote!(features::tree::Canopy::Pine(features::tree::PineCanopy {
                base_radius: #base_radius,
                height: #height,
                leaf: #leaf,
            }))
        }
        "spruce_canopy" => {
            let lower = range_tokens(origin, o.req("lower_offset")?)?;
            let upper = range_tokens(origin, o.req("upper_offset")?)?;
            let max_radius = range_tokens(origin, o.req("max_radius")?)?;
            let leaf = leaf_tokens(em, palette, origin, o.req("leaf_block")?)?;
            quote!(features::tree::Canopy::Spruce(features::tree::SpruceCanopy {
                lower_offset: #lower,
                upper_offset: #upper,
                max_radius: #max_radius,
                leaf: #leaf,
            }))
        }
        "roofed_canopy" => {
            let height = o.i32("canopy_height")?;
            let core_width = o.i32("core_width")?;
            let outer = o.i32("outer_radius")?;
            let inner = o.i32("inner_radius")?;
            let leaf = leaf_tokens(em, palette, origin, o.req("leaf_block")?)?;
            quote!(features::tree::Canopy::Roofed(features::tree::RoofedCanopy {
                height: #height,
                core_width: #core_width,
                outer_radius: #outer,
                inner_radius: #inner,
                leaf: #leaf,
            }))
        }
        "acacia_canopy" => {
            let size = o.i32("canopy_size")?;
            let simplify = o.bool_or("simplify_canopy", false)?;
            let leaf = leaf_tokens(em, palette, origin, o.req("leaf_block")?)?;
            quote!(features::tree::Canopy::Acacia(features::tree::AcaciaCanopy { size: #size, simplify: #simplify, leaf: #leaf }))
        }
        "random_spread_canopy" | "mangrove_canopy" => {
            let height = o.i32("canopy_height")?;
            let radius = o.i32("canopy_radius")?;
            let attempts = o.i32("leaf_placement_attempts")?;
            let mut leaves = Vec::new();
            for entry in o.array("leaf_blocks")? {
                let pair = entry
                    .as_array()
                    .filter(|p| p.len() == 2)
                    .ok_or_else(|| Error::data(origin, format!("leaf_blocks entry is not [block, weight]: {entry}")))?;
                let leaf = leaf_tokens(em, palette, origin, &pair[0])?;
                let weight = int(origin, "leaf_blocks weight", &pair[1])?;
                leaves.push(quote!((#leaf, #weight)));
            }
            if leaves.is_empty() {
                return Err(Error::data(origin, "leaf_blocks is empty"));
            }
            let decoration = canopy_decoration(em, palette, origin, o.opt("canopy_decoration"))?;
            let hanging = match (o.opt("hanging_block"), o.opt("hanging_block_placement_chance")) {
                (Some(b), Some(c)) => {
                    let b = leaf_tokens(em, palette, origin, b)?;
                    let c = percent_chance(origin, c)?;
                    quote!(Some((#b, #c)))
                }
                (None, None) => quote!(None),
                _ => return Err(Error::data(origin, "hanging_block and hanging_block_placement_chance come together")),
            };
            quote!(features::tree::Canopy::RandomSpread(features::tree::RandomSpreadCanopy {
                height: #height,
                radius: #radius,
                attempts: #attempts,
                leaves: &[#(#leaves),*],
                decoration: #decoration,
                hanging: #hanging,
            }))
        }
        "cherry_canopy" => {
            let leaf = leaf_tokens(em, palette, origin, o.req("leaf_block")?)?;
            let height = o.i32("height")?;
            let radius = o.i32("radius")?;
            let wide = o.f32("wide_bottom_layer_hole_chance")? / 100.0;
            let corner = o.f32("corner_hole_chance")? / 100.0;
            let hanging = o.f32("hanging_leaves_chance")? / 100.0;
            let extension = o.f32("hanging_leaves_extension_chance")? / 100.0;
            quote!(features::tree::Canopy::Cherry(features::tree::CherryCanopy {
                height: #height,
                radius: #radius,
                wide_bottom_hole: #wide,
                corner_hole: #corner,
                hanging: #hanging,
                hanging_extension: #extension,
                leaf: #leaf,
            }))
        }
        "poplar_canopy" => {
            let mut radius = Vec::new();
            for entry in o.array("radius")? {
                let e = Obj::new(origin, entry)?;
                let value = e.i32("value")?;
                let weight = e.i32("weight")?;
                e.finish()?;
                radius.push(quote!((#value, #weight)));
            }
            if radius.is_empty() {
                return Err(Error::data(origin, "poplar_canopy.radius is empty"));
            }
            let height = range_tokens(origin, o.req("height")?)?;
            let side_hole = o.f32("side_hole_chance")?;
            let leaf = leaf_tokens(em, palette, origin, o.req("leaf_block")?)?;
            let branch = log_tokens(em, palette, origin, o.req("branch_block")?)?;
            quote!(features::tree::Canopy::Poplar(features::tree::PoplarCanopy {
                radius: &[#(#radius),*],
                height: #height,
                side_hole: #side_hole,
                leaf: #leaf,
                branch: #branch,
            }))
        }
        other => return Err(Error::data(origin, format!("unknown canopy kind {other}"))),
    };
    o.finish()?;
    Ok((tokens, fancy_height))
}
