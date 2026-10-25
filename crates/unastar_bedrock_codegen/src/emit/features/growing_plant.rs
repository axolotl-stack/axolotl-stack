//! `minecraft:growing_plant_feature` -> `features::growing_plant::place` with
//! a static `features::growing_plant::Params` literal.
//!
//! `height_distribution`, `body_blocks` and `head_blocks` are `[value,
//! weight]` pairs. Height ranges are inclusive (`pale_hanging_moss_feature`
//! lists single heights as `{1, 1}`, `{2, 2}`, ...). The head `age` range is
//! half-open: `cave_vine_feature` gives `17..26` while `growing_plant_age`
//! stops at 25, the same one-above upper bound as other pack ranges with an
//! exclusive maximum. Each head candidate with a `growing_plant_age` state is
//! expanded to one state per age in the range (the runtime picks one
//! uniformly); heads without that state, or features without `age`, keep
//! their single state.

use crate::blocks::{BlockPalette, StateValue};
use crate::emit::Emitter;
use crate::emit::features::range_tokens;
use crate::error::{Error, Result};
use crate::ir::{Obj, block, int, int_range};
use proc_macro2::TokenStream;
use quote::quote;
use serde_json::Value;

/// Age state of cave vines.
const AGE_STATE: &str = "growing_plant_age";

pub fn emit(em: &mut Emitter<'_>, o: &Obj<'_>) -> Result<TokenStream> {
    let origin = o.origin;
    let model = em.model;
    let palette = &model.palette;
    let heights = weighted(origin, o.array("height_distribution")?)?
        .into_iter()
        .map(|(v, w)| {
            let range = range_tokens(origin, v)?;
            Ok(quote!((#range, #w)))
        })
        .collect::<Result<Vec<TokenStream>>>()?;
    let step: i32 = match o.str("growth_direction")? {
        "UP" | "up" => 1,
        "DOWN" | "down" => -1,
        other => {
            return Err(Error::data(
                origin,
                format!("unknown growth_direction {other:?}"),
            ));
        }
    };
    let allow_water = o.bool_or("allow_water", false)?;
    let age = o.opt("age").map(|v| int_range(origin, v)).transpose()?;
    if let Some(age) = age
        && age.max <= age.min
    {
        return Err(Error::data(
            origin,
            format!("empty age range {}..{}", age.min, age.max),
        ));
    }
    let mut body = Vec::new();
    for (v, w) in weighted(origin, o.array("body_blocks")?)? {
        let id = block(palette, origin, v)?;
        let c = em.block(id);
        body.push(quote!((#c, #w)));
    }
    let mut head = Vec::new();
    for (v, w) in weighted(origin, o.array("head_blocks")?)? {
        let id = block(palette, origin, v)?;
        let states = match age {
            Some(age) if has_state(palette, id, AGE_STATE) => (age.min..age.max)
                .map(|a| with_state(palette, origin, id, AGE_STATE, &StateValue::Int(a)))
                .collect::<Result<Vec<u32>>>()?,
            _ => vec![id],
        };
        let consts: Vec<TokenStream> = states.into_iter().map(|s| em.block(s)).collect();
        head.push(quote!((&[#(#consts),*], #w)));
    }
    Ok(quote!({
        static PARAMS: features::growing_plant::Params = features::growing_plant::Params {
            heights: &[#(#heights),*],
            step: #step,
            allow_water: #allow_water,
            body: &[#(#body),*],
            head: &[#(#head),*],
        };
        features::growing_plant::place(ctx, pos, &PARAMS)
    }))
}

/// A non-empty list of `[value, weight]` pairs with positive weights.
fn weighted<'v>(origin: &str, items: &'v [Value]) -> Result<Vec<(&'v Value, i32)>> {
    if items.is_empty() {
        return Err(Error::data(origin, "empty weighted list"));
    }
    items
        .iter()
        .map(|item| match item.as_array().map(Vec::as_slice) {
            Some([value, weight]) => {
                let w = int(origin, "weight", weight)?;
                if w <= 0 {
                    return Err(Error::data(origin, format!("weight {w} is not positive")));
                }
                Ok((value, w))
            }
            _ => Err(Error::data(
                origin,
                format!("not a [value, weight] pair: {item}"),
            )),
        })
        .collect()
}

fn has_state(palette: &BlockPalette, id: u32, key: &str) -> bool {
    palette
        .describe(id)
        .is_some_and(|(_, _, state)| state.contains_key(key))
}

/// The state of `id`'s block equal to `id` except `key = value`.
pub(super) fn with_state(
    palette: &BlockPalette,
    origin: &str,
    id: u32,
    key: &str,
    value: &StateValue,
) -> Result<u32> {
    let (name, _, state) = palette
        .describe(id)
        .ok_or_else(|| Error::data(origin, format!("runtime id {id} is not in the palette")))?;
    palette
        .states_of(name)
        .into_iter()
        .flatten()
        .find(|(_, s)| {
            s.get(key) == Some(value) && s.iter().all(|(k, v)| k == key || state.get(k) == Some(v))
        })
        .map(|(i, _)| i)
        .ok_or_else(|| Error::data(origin, format!("{name} has no state {key}={value}")))
}
