//! `minecraft:sculk_patch_feature` -> `features::sculk_patch::place` with a
//! static `features::sculk_patch::Params` literal.
//!
//! `extra_growth_chance` is a count range of extra shriekers (absent: none).
//! `can_place_sculk_patch_on` is empty in every vanilla document; empty
//! selects the runtime's default sculk-replaceable set, a non-empty list
//! replaces it. The runtime addresses sculk vein states as `base + face
//! mask`, verified here against the palette.

use super::multiface::face_states_base;
use crate::emit::Emitter;
use crate::error::{Error, Result};
use crate::ir::{Obj, block, block_set, int_range};
use proc_macro2::TokenStream;
use quote::quote;

pub fn emit(em: &mut Emitter<'_>, o: &Obj<'_>) -> Result<TokenStream> {
    let origin = o.origin;
    let model = em.model;
    let palette = &model.palette;
    face_states_base(palette, origin, "minecraft:sculk_vein")?;
    let on = block_set(palette, origin, o.req("can_place_sculk_patch_on")?)?;
    let central = block(palette, origin, o.req("central_block")?)?;
    let central_chance = o.f32("central_block_placement_chance")?;
    let count = |key: &'static str| -> Result<i32> {
        let n = o.i32(key)?;
        if n < 0 {
            return Err(Error::data(origin, format!("negative {key} {n}")));
        }
        Ok(n)
    };
    let cursor_count = count("cursor_count")?;
    let charge_amount = count("charge_amount")?;
    let spread_attempts = count("spread_attempts")?;
    let growth_rounds = count("growth_rounds")?;
    let spread_rounds = count("spread_rounds")?;
    let extra = match o.opt("extra_growth_chance") {
        None => quote!(None),
        Some(v) => {
            let r = int_range(origin, v)?;
            let (a, b) = (r.min, r.max);
            quote!(Some(features::IntRange { min: #a, max: #b }))
        }
    };
    let replaceable = if on.is_empty() {
        quote!(features::sculk_patch::default_replaceable(b))
    } else {
        em.matches(quote!(b), &on)
    };
    let central = em.block(central);
    Ok(quote!({
        fn replaceable(b: u32) -> bool { #replaceable }
        static PARAMS: features::sculk_patch::Params = features::sculk_patch::Params {
            cursor_count: #cursor_count,
            charge_amount: #charge_amount,
            spread_attempts: #spread_attempts,
            growth_rounds: #growth_rounds,
            spread_rounds: #spread_rounds,
            extra_growths: #extra,
            central_block: #central,
            central_block_chance: #central_chance,
            replaceable,
        };
        features::sculk_patch::place(ctx, pos, &PARAMS)
    }))
}
