//! `minecraft:multiface_feature` -> `features::multiface::place` with a
//! static `features::multiface::Params` literal.
//!
//! `places_block` must be a multiface block given without faces; the
//! runtime addresses its states as `base + face mask`, which
//! [`face_states_base`] verifies against the palette. Sculk veins use the
//! runtime's vein spreading rules (`features::sculk_patch::VEIN`); other
//! blocks attach to any sturdy face. The allowed faces come from
//! `can_place_on_floor` (down), `can_place_on_ceiling` (up) and
//! `can_place_on_wall` (the four sides); all default to false, like
//! `chance_of_spreading` defaults to 0.

use crate::blocks::{BlockPalette, StateValue};
use crate::emit::Emitter;
use crate::error::{Error, Result};
use crate::ir::{Obj, block, block_set};
use proc_macro2::TokenStream;
use quote::quote;

/// Face mask state of multiface blocks.
const FACE_BITS: &str = "multi_face_direction_bits";

pub fn emit(em: &mut Emitter<'_>, o: &Obj<'_>) -> Result<TokenStream> {
    let origin = o.origin;
    let model = em.model;
    let palette = &model.palette;
    let id = block(palette, origin, o.req("places_block")?)?;
    let (name, _, _) = palette
        .describe(id)
        .ok_or_else(|| Error::data(origin, format!("runtime id {id} is not in the palette")))?;
    let base = face_states_base(palette, origin, name)?;
    if id != base {
        return Err(Error::data(
            origin,
            format!("places_block {name} must be given without faces"),
        ));
    }
    let search_range = o.i32("search_range")?;
    // Face bits: 0 down, 1 up, 2..=5 south, west, north, east.
    let mut faces: u8 = 0;
    if o.bool_or("can_place_on_floor", false)? {
        faces |= 1;
    }
    if o.bool_or("can_place_on_ceiling", false)? {
        faces |= 1 << 1;
    }
    if o.bool_or("can_place_on_wall", false)? {
        faces |= 0b11_1100;
    }
    let chance = o.f32_or("chance_of_spreading", 0.0)?;
    let can_place_on = block_set(palette, origin, o.req("can_place_on")?)?;
    let test = em.matches(quote!(b), &can_place_on);
    let spreader = if name == "minecraft:sculk_vein" {
        quote!(features::sculk_patch::VEIN)
    } else {
        let base = em.block(base);
        quote!(features::multiface::Spreader {
            base: #base,
            can_attach: features::multiface::is_sturdy,
            wrap_needs_open_corner: false,
        })
    };
    Ok(quote!({
        fn can_place_on(b: u32) -> bool { #test }
        static PARAMS: features::multiface::Params = features::multiface::Params {
            spreader: #spreader,
            search_range: #search_range,
            faces: #faces,
            chance_of_spreading: #chance,
            can_place_on,
        };
        features::multiface::place(ctx, pos, &PARAMS)
    }))
}

/// Runtime id of `name`'s faceless state, after checking that its states are
/// exactly `multi_face_direction_bits` 0..=63 at consecutive ids.
pub(super) fn face_states_base(palette: &BlockPalette, origin: &str, name: &str) -> Result<u32> {
    let states = palette
        .states_of(name)
        .ok_or_else(|| Error::data(origin, format!("unknown block {name}")))?;
    let mut base = None;
    let mut count: u32 = 0;
    for (id, state) in states {
        let first = *base.get_or_insert(id);
        let expected = StateValue::Int(count as i32);
        if id != first + count || state.len() != 1 || state.get(FACE_BITS) != Some(&expected) {
            return Err(Error::data(
                origin,
                format!("{name}: states are not {FACE_BITS} 0..=63 in palette order"),
            ));
        }
        count += 1;
    }
    match base {
        Some(base) if count == 64 => Ok(base),
        _ => Err(Error::data(
            origin,
            format!("{name}: expected 64 {FACE_BITS} states, found {count}"),
        )),
    }
}
