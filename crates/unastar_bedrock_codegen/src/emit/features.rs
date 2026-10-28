//! JSON features: one `f_*` function per reachable feature.
//!
//! Composition types are compiled inline (their children become direct calls):
//! - `scatter_feature`: an unrolled scatter loop (see [`super::scatter`]);
//! - `aggregate_feature`: every child at the same input; `early_out`
//!   `first_failure` stops at the first child that places nothing,
//!   `first_success` at the first that places something;
//! - `sequence_feature`: each child receives the previous child's output
//!   position, stopping at the first failure;
//! - `weighted_random_feature`: one child picked by weight;
//! - `search_feature`: sweeps `search_volume` along `search_axis` (outer loop)
//!   and stops once `required_successes` placements succeeded;
//! - `snap_to_surface_feature`: walks from the input through open blocks
//!   towards the floor/ceiling for up to `vertical_search_range` blocks;
//! - `surface_relative_threshold_feature`: places only at least
//!   `minimum_distance_below_surface` blocks below the heightmap surface.
//!
//! Block-placing types call `features::<type>::place(ctx, pos, &PARAMS)`
//! with a `static` parameter literal; `single_block_feature` is compiled
//! inline (its checks are a handful of compiled block tests).

use super::survival::{Survival, classify};
use super::{Emitter, feature_fn, scatter};
use crate::blocks::IdSet;
use crate::error::{Error, Result};
use crate::ir::{self, Feature, Obj, Scatter, block, block_set, int, num};
use proc_macro2::TokenStream;
use quote::quote;
use serde_json::Value;

pub mod complex;

pub fn emit_all(em: &mut Emitter<'_>) -> Result<TokenStream> {
    let ids: Vec<String> = em
        .reachable
        .iter()
        .filter(|id| em.model.features.contains_key(*id))
        .cloned()
        .collect();
    let mut fns = Vec::new();
    for id in ids {
        let feature = em.model.features[&id].clone();
        let body = emit_feature(em, &feature)?;
        let name = feature_fn(&id);
        let doc = format!(
            " `{}` (`{}`) from `{}`.",
            feature.identifier, feature.kind, feature.source
        );
        fns.push(quote! {
            #[doc = #doc]
            #[inline(never)]
            #[allow(unused_variables)]
            pub fn #name<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos) -> Option<BlockPos> {
                #body
            }
        });
    }
    Ok(quote!(#(#fns)*))
}

fn emit_feature(em: &mut Emitter<'_>, f: &Feature) -> Result<TokenStream> {
    let origin = f.source.as_str();
    let o = Obj::new(origin, &f.body)?;
    o.ignore(&["description"]);
    let body = match f.kind.as_str() {
        "minecraft:scatter_feature" => scatter_feature(em, &o)?,
        "minecraft:aggregate_feature" => aggregate(em, &o)?,
        "minecraft:sequence_feature" => sequence(em, &o)?,
        "minecraft:weighted_random_feature" => weighted_random(em, &o)?,
        "minecraft:search_feature" => search(em, &o)?,
        "minecraft:snap_to_surface_feature" => snap_to_surface(em, &o)?,
        "minecraft:surface_relative_threshold_feature" => surface_relative_threshold(em, &o)?,
        "minecraft:single_block_feature" => single_block(em, &o)?,
        "minecraft:ore_feature" => ore(em, &o)?,
        other => complex::emit(em, other, &o)?,
    };
    o.finish()?;
    Ok(body)
}

fn scatter_feature(em: &mut Emitter<'_>, o: &Obj<'_>) -> Result<TokenStream> {
    let target = o.str("places_feature")?.to_owned();
    // Newer documents nest the scatter keys under `distribution`.
    let s: Scatter = match o.opt("distribution") {
        Some(d) => {
            let d = Obj::new(o.origin, d)?;
            let s = Scatter::parse_keys(&d)?;
            d.finish()?;
            s
        }
        None => Scatter::parse_keys(o)?,
    };
    let call = |p: TokenStream| em.call(&target, p);
    let body = scatter::emit(em, o.origin, &s, call)?;
    Ok(quote!(let origin = pos; #body))
}

fn children(o: &Obj<'_>) -> Result<Vec<String>> {
    o.array("features")?
        .iter()
        .map(|v| {
            v.as_str().map(str::to_owned).ok_or_else(|| {
                Error::data(o.origin, format!("feature list entry is not a string: {v}"))
            })
        })
        .collect()
}

fn aggregate(em: &mut Emitter<'_>, o: &Obj<'_>) -> Result<TokenStream> {
    let kids = children(o)?;
    let calls: Vec<TokenStream> = kids.iter().map(|k| em.call(k, quote!(pos))).collect();
    let Some((last, init)) = calls.split_last() else {
        return Err(Error::data(o.origin, "aggregate_feature without features"));
    };
    Ok(
        match o.opt("early_out").and_then(Value::as_str).unwrap_or("none") {
            // Every child must place; the result is the last child's output.
            "first_failure" => quote! {
                #( #init?; )*
                #last
            },
            "first_success" => quote! {
                #( if let Some(p) = #init { return Some(p); } )*
                #last
            },
            // Every child runs; the result is the last successful output.
            "none" => {
                let names: Vec<_> = (0..calls.len())
                    .map(|i| quote::format_ident!("r{}", i))
                    .collect();
                let rev: Vec<_> = names.iter().rev().collect();
                let (first, rest) = rev.split_first().expect("non-empty");
                quote! {
                    #( let #names = #calls; )*
                    #first #( .or(#rest) )*
                }
            }
            other => {
                return Err(Error::data(
                    o.origin,
                    format!("unknown early_out {other:?}"),
                ));
            }
        },
    )
}

fn sequence(em: &mut Emitter<'_>, o: &Obj<'_>) -> Result<TokenStream> {
    let kids = children(o)?;
    let calls: Vec<TokenStream> = kids.iter().map(|k| em.call(k, quote!(pos))).collect();
    Ok(quote! {
        #( let pos = #calls?; )*
        Some(pos)
    })
}

fn weighted_random(em: &mut Emitter<'_>, o: &Obj<'_>) -> Result<TokenStream> {
    let mut entries = Vec::new();
    for item in o.array("features")? {
        let pair = item.as_array().filter(|p| p.len() == 2);
        let (Some(id), Some(weight)) = (
            pair.and_then(|p| p[0].as_str()),
            pair.map(|p| num(o.origin, "weight", &p[1])).transpose()?,
        ) else {
            return Err(Error::data(
                o.origin,
                format!("weighted entry must be [feature, weight]: {item}"),
            ));
        };
        entries.push((id.to_owned(), weight));
    }
    let total: f32 = entries.iter().map(|(_, w)| w).sum();
    if entries.is_empty() || total <= 0.0 {
        return Err(Error::data(
            o.origin,
            "weighted_random_feature with no positive weight",
        ));
    }
    let integral = entries.iter().all(|(_, w)| w.fract() == 0.0);
    let mut arms = Vec::new();
    let mut acc = 0.0f32;
    for (id, w) in &entries {
        acc += w;
        let call = em.call(id, quote!(pos));
        if integral {
            let bound = acc as i32;
            arms.push(quote!(if r < #bound { return #call; }));
        } else {
            arms.push(quote!(if r < #acc { return #call; }));
        }
    }
    let draw = if integral {
        let t = total as i32;
        quote!(let r = ctx.rng.next_int(#t);)
    } else {
        quote!(let r = ctx.rng.next_float() * #total;)
    };
    Ok(quote!( #draw #(#arms)* None ))
}

fn vec3(o: &Obj<'_>, v: &Value) -> Result<[i32; 3]> {
    match v.as_array().map(Vec::as_slice) {
        Some([x, y, z]) => Ok([
            int(o.origin, "x", x)?,
            int(o.origin, "y", y)?,
            int(o.origin, "z", z)?,
        ]),
        _ => Err(Error::data(o.origin, format!("expected [x, y, z]: {v}"))),
    }
}

fn search(em: &mut Emitter<'_>, o: &Obj<'_>) -> Result<TokenStream> {
    let target = o.str("places_feature")?.to_owned();
    let volume = Obj::new(o.origin, o.req("search_volume")?)?;
    let min = vec3(o, volume.req("min")?)?;
    let max = vec3(o, volume.req("max")?)?;
    volume.finish()?;
    let required = o.i32("required_successes")?.max(1);
    let axis = o.str("search_axis")?;
    let (axis_index, positive) = match axis {
        "+x" => (0, true),
        "-x" => (0, false),
        "+y" => (1, true),
        "-y" => (1, false),
        "+z" => (2, true),
        "-z" => (2, false),
        other => {
            return Err(Error::data(
                o.origin,
                format!("unknown search_axis {other:?}"),
            ));
        }
    };
    // Loop order: the search axis outermost, then the remaining axes in x, y, z order.
    let mut order = vec![axis_index];
    order.extend((0..3).filter(|a| *a != axis_index));
    let vars = [quote!(dx), quote!(dy), quote!(dz)];
    let call = em.call(&target, quote!(pos.offset(dx, dy, dz)));
    let mut body = quote! {
        if let Some(p) = #call {
            successes += 1;
            if successes >= #required { return Some(p); }
        }
    };
    for &a in order.iter().rev() {
        let (lo, hi) = (min[a], max[a]);
        let var = &vars[a];
        let range = if a == axis_index && !positive {
            quote!((#lo..=#hi).rev())
        } else {
            quote!(#lo..=#hi)
        };
        body = quote!(for #var in #range { #body });
    }
    Ok(quote! {
        let mut successes = 0i32;
        #body
        None
    })
}

fn snap_to_surface(em: &mut Emitter<'_>, o: &Obj<'_>) -> Result<TokenStream> {
    let target = o.str("feature_to_snap")?.to_owned();
    let range = o.i32("vertical_search_range")?;
    let surface = o.str("surface")?;
    let allow_air = o.bool_or("allow_air_placement", true)?;
    let allow_water = o.bool_or("allow_underwater_placement", false)?;
    let embed = o.bool_or("embed_in_surface", false)?;
    let allowed = o
        .opt("allowed_surface_blocks")
        .map(|v| block_set(&em.model.palette, o.origin, v))
        .transpose()?;
    let palette = &em.model.palette;
    let solid = palette.ids_where(|f| f.solid);
    let water = palette.all_states(o.origin, &["minecraft:water", "minecraft:flowing_water"])?;
    let air = palette.all_states(o.origin, &["minecraft:air"])?;
    let mut open = IdSet::default();
    if allow_air {
        open = open.union(&air);
    }
    if allow_water {
        open = open.union(&water);
    }
    let is_open = em.matches(quote!(at), &open);
    let surface_set = allowed.unwrap_or(solid);
    let is_surface = em.matches(quote!(support), &surface_set);
    let call = em.call(&target, quote!(snapped));
    // `step`: walking direction; `support`: -1 floor (support below), +1 ceiling.
    // The walk starts at the input and stays inside open blocks: an input
    // embedded in rock places nothing instead of snapping through the rock
    // onto a nearby cave or ocean floor. Searching through solid blocks (and
    // also away from the surface) doubled lush-cave decoration and put
    // underground magma on open ocean floors; see the codegen README.
    let scan = |step: i32, support: i32| -> TokenStream {
        let embed_offset = if embed { support } else { 0 };
        quote! {{
            let mut found = None;
            for k in 0..=#range {
                let y = pos.y + #step * k;
                let at = ctx.get(BlockPos::new(pos.x, y, pos.z));
                if !(#is_open) {
                    break;
                }
                let support = ctx.get(BlockPos::new(pos.x, y + #support, pos.z));
                if #is_surface {
                    found = Some(BlockPos::new(pos.x, y + #embed_offset, pos.z));
                    break;
                }
            }
            found
        }}
    };
    let found = match surface {
        "floor" => scan(-1, -1),
        "ceiling" => scan(1, 1),
        // Snap sideways to a wall in a random horizontal direction.
        "random_horizontal" => {
            let embed_step = i32::from(embed);
            quote!({
                let (sx, sz) = [(0i32, 1i32), (-1, 0), (0, -1), (1, 0)][ctx.rng.next_int(4) as usize];
                let mut found = None;
                for k in 0..=#range {
                    let at = ctx.get(pos.offset(sx * k, 0, sz * k));
                    let support = ctx.get(pos.offset(sx * (k + 1), 0, sz * (k + 1)));
                    if #is_open && #is_surface {
                        found = Some(pos.offset(sx * (k + #embed_step), 0, sz * (k + #embed_step)));
                        break;
                    }
                }
                found
            })
        }
        other => {
            return Err(Error::data(
                o.origin,
                format!("unknown snap surface {other:?}"),
            ));
        }
    };
    Ok(quote! {
        let found: Option<BlockPos> = #found;
        let snapped = found?;
        #call
    })
}

fn surface_relative_threshold(em: &mut Emitter<'_>, o: &Obj<'_>) -> Result<TokenStream> {
    let target = o.str("feature_to_place")?.to_owned();
    let min = o.i32("minimum_distance_below_surface")?;
    let call = em.call(&target, quote!(pos));
    Ok(quote! {
        let surface = ctx.heightmap(pos.x, pos.z) - 1;
        if surface - pos.y < #min { return None; }
        #call
    })
}

/// Placement candidates of `places_block`: one block or a weighted list.
fn placed_blocks(em: &mut Emitter<'_>, o: &Obj<'_>) -> Result<Vec<(u32, i32)>> {
    let palette = &em.model.palette;
    let v = o.req("places_block")?;
    match v {
        Value::Array(items) => items
            .iter()
            .map(|item| {
                let e = Obj::new(o.origin, item)?;
                let id = block(palette, o.origin, e.req("block")?)?;
                let w = e.i32("weight")?;
                e.finish()?;
                Ok((id, w))
            })
            .collect(),
        single => Ok(vec![(block(palette, o.origin, single)?, 1)]),
    }
}

fn single_block(em: &mut Emitter<'_>, o: &Obj<'_>) -> Result<TokenStream> {
    let origin = o.origin;
    let candidates = placed_blocks(em, o)?;
    let enforce = o.bool_or("enforce_survivability_rules", false)?
        | o.bool_or("enforce_placement_rules", false)?;
    let randomize = o.bool_or("randomize_rotation", false)?;
    let may_replace = o
        .opt("may_replace")
        .map(|v| block_set(&em.model.palette, origin, v))
        .transpose()?;

    let mut checks = Vec::new();
    if let Some(set) = &may_replace {
        let test = em.matches(quote!(ctx.get(pos)), set);
        checks.push(quote!(if !#test { return None; }));
    }
    // Attachment: every listed side whose neighbour matches counts.
    if let Some(att) = o.opt("may_attach_to") {
        let (sides, min, auto_rotate) = attach_sides(em, origin, att)?;
        // Only unreachable vanilla features (cocoa) use auto_rotate; reaching
        // one needs the attached-side rotation implemented first.
        if auto_rotate {
            return Err(Error::data(
                origin,
                "may_attach_to.auto_rotate is not supported",
            ));
        }
        let tests: Vec<TokenStream> = sides
            .iter()
            .map(|(offset, set)| {
                let (dx, dy, dz) = *offset;
                let t = em.matches(quote!(ctx.get(pos.offset(#dx, #dy, #dz))), set);
                quote!(attached += i32::from(#t);)
            })
            .collect();
        checks.push(quote! {
            let mut attached = 0i32;
            #(#tests)*
            if attached < #min { return None; }
        });
    }
    if let Some(att) = o.opt("may_not_attach_to") {
        let (sides, _, _) = attach_sides(em, origin, att)?;
        for ((dx, dy, dz), set) in &sides {
            let t = em.matches(quote!(ctx.get(pos.offset(#dx, #dy, #dz))), set);
            checks.push(quote!(if #t { return None; }));
        }
    }

    // Choose the block (weighted) and, if requested, a random rotation.
    let mut pick = Vec::new();
    let total: i32 = candidates.iter().map(|(_, w)| *w).sum();
    let mut acc = 0;
    let palette = &em.model.palette;
    let mut survival_checks = Vec::new();
    for (i, (id, w)) in candidates.iter().enumerate() {
        acc += w;
        let variants = if randomize {
            rotations(palette, *id)
        } else {
            vec![*id]
        };
        let consts: Vec<TokenStream> = variants.iter().map(|v| em.block(*v)).collect();
        let n = consts.len() as i32;
        let value = if n == 1 {
            let c = &consts[0];
            quote!(#c)
        } else {
            let arms = consts.iter().enumerate().map(|(k, c)| {
                let k = k as i32;
                quote!(#k => #c,)
            });
            quote!(match ctx.rng.next_int(#n) { #(#arms)* _ => unreachable!() })
        };
        let last = i + 1 == candidates.len();
        pick.push(if last {
            quote!({ #value })
        } else {
            quote!(if r < #acc { #value } else)
        });
        if enforce {
            let survival = classify(palette, origin, *id)?;
            survival_checks.push(survival);
        }
    }
    // All candidates of one feature share a survival class in vanilla data;
    // a feature mixing classes would need per-candidate checks.
    if let Some(first) = survival_checks.first()
        && survival_checks.iter().any(|s| s != first)
    {
        return Err(Error::data(
            origin,
            "places_block candidates with different survival rules",
        ));
    }
    match survival_checks.first() {
        Some(Survival::Below(set)) => {
            let t = em.matches(quote!(ctx.get(pos.below())), set);
            checks.push(quote!(if !#t { return None; }));
        }
        Some(Survival::Above(set)) => {
            let t = em.matches(quote!(ctx.get(pos.above())), set);
            checks.push(quote!(if !#t { return None; }));
        }
        Some(Survival::Any) | None => {}
    }
    let draw = if candidates.len() > 1 {
        quote!(let r = ctx.rng.next_int(#total);)
    } else {
        quote!()
    };
    Ok(quote! {
        #(#checks)*
        #draw
        let block = #(#pick)*;
        if ctx.set(pos, block) { Some(pos) } else { None }
    })
}

/// `may_attach_to` / `may_not_attach_to`: per-side neighbour sets.
#[allow(clippy::type_complexity)]
fn attach_sides(
    em: &mut Emitter<'_>,
    origin: &str,
    v: &Value,
) -> Result<(Vec<((i32, i32, i32), IdSet)>, i32, bool)> {
    let o = Obj::new(origin, v)?;
    let palette = &em.model.palette;
    let mut sides = Vec::new();
    let mut add = |key: &'static str, offsets: &[(i32, i32, i32)]| -> Result<()> {
        if let Some(v) = o.opt(key) {
            let set = block_set(palette, origin, v)?;
            for off in offsets {
                sides.push((*off, set.clone()));
            }
        }
        Ok(())
    };
    const HORIZONTAL: [(i32, i32, i32); 4] = [(0, 0, 1), (-1, 0, 0), (0, 0, -1), (1, 0, 0)];
    add("top", &[(0, 1, 0)])?;
    add("bottom", &[(0, -1, 0)])?;
    add("north", &[(0, 0, -1)])?;
    add("south", &[(0, 0, 1)])?;
    add("east", &[(1, 0, 0)])?;
    add("west", &[(-1, 0, 0)])?;
    add("sides", &HORIZONTAL)?;
    add(
        "all",
        &[
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (-1, 0, 0),
            (0, 0, -1),
            (1, 0, 0),
        ],
    )?;
    add(
        "diagonal",
        &[(1, 0, 1), (1, 0, -1), (-1, 0, 1), (-1, 0, -1)],
    )?;
    let min = o.i32_or("min_sides_must_attach", 1)?;
    let auto_rotate = o.bool_or("auto_rotate", false)?;
    o.finish()?;
    Ok((sides, min, auto_rotate))
}

/// Every horizontal rotation of a state (`direction`, `minecraft:cardinal_direction`,
/// `facing_direction` 2..=5), other properties unchanged.
fn rotations(palette: &crate::blocks::BlockPalette, id: u32) -> Vec<u32> {
    let Some((name, _, state)) = palette.describe(id) else {
        return vec![id];
    };
    let key = [
        "direction",
        "minecraft:cardinal_direction",
        "facing_direction",
    ]
    .into_iter()
    .find(|k| state.contains_key(*k));
    let Some(key) = key else {
        return vec![id];
    };
    let mut out: Vec<u32> = palette
        .states_of(name)
        .into_iter()
        .flatten()
        .filter(|(_, s)| {
            s.iter().all(|(k, v)| k == key || state.get(k) == Some(v))
                && !(key == "facing_direction"
                    && matches!(s.get(key), Some(crate::blocks::StateValue::Int(0 | 1))))
        })
        .map(|(i, _)| i)
        .collect();
    out.sort_unstable();
    out
}

fn ore(em: &mut Emitter<'_>, o: &Obj<'_>) -> Result<TokenStream> {
    let origin = o.origin;
    let count = o.i32("count")?;
    let discard = o.f32_or("discard_chance_on_air_exposure", 0.0)?;
    let mut rules = Vec::new();
    for r in o.array("replace_rules")? {
        let r = Obj::new(origin, r)?;
        let places = block(&em.model.palette, origin, r.req("places_block")?)?;
        let may = r
            .opt("may_replace")
            .map(|v| block_set(&em.model.palette, origin, v))
            .transpose()?;
        r.finish()?;
        let places = em.block(places);
        let test = match &may {
            Some(set) => em.matches(quote!(b), set),
            // Without may_replace every block may be replaced.
            None => quote!(true),
        };
        rules.push(quote!(if #test { return Some(#places); }));
    }
    let air = em.model.palette.all_states(origin, &["minecraft:air"])?;
    let is_air = em.matches(quote!(b), &air);
    Ok(quote! {
        fn replace(b: u32) -> Option<u32> {
            #(#rules)*
            None
        }
        fn is_air(b: u32) -> bool { #is_air }
        features::ore::place(ctx, pos, #count, #discard, replace, is_air)
    })
}

/// Parse helper shared by complex emitters: an `IntRange` literal.
pub fn range_tokens(origin: &str, v: &Value) -> Result<TokenStream> {
    let r = ir::int_range(origin, v)?;
    let (a, b) = (r.min, r.max);
    Ok(quote!(features::IntRange { min: #a, max: #b }))
}
