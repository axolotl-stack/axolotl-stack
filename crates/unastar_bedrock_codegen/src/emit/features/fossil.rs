//! `minecraft:fossil_feature`: a random template of `structures/fossils/*`
//! (loaded by [`crate::structures`]) buried below the surface; `ore_block`
//! replaces part of the bones, `max_empty_corners` rejects exposed
//! placements.

use crate::emit::Emitter;
use crate::error::{Error, Result};
use crate::ir::{self, Obj};
use proc_macro2::TokenStream;
use quote::quote;

pub fn emit(em: &mut Emitter<'_>, o: &Obj<'_>) -> Result<TokenStream> {
    let origin = o.origin;
    let model = em.model;
    let ore = ir::block(&model.palette, origin, o.req("ore_block")?)?;
    let max_empty_corners = o.i32("max_empty_corners")?;
    if !(0..=8).contains(&max_empty_corners) {
        return Err(Error::data(
            origin,
            format!("max_empty_corners {max_empty_corners} outside 0..=8"),
        ));
    }
    if model.structures.fossils.is_empty() {
        return Err(Error::data(
            origin,
            "no structures/fossils templates were loaded",
        ));
    }
    let templates = model
        .structures
        .fossils
        .iter()
        .map(|&i| u16::try_from(i).map_err(|_| Error::data(origin, "template index overflow")))
        .collect::<Result<Vec<u16>>>()?;
    let ore = em.block(ore);
    Ok(quote!({
        static PARAMS: features::fossil::Params = features::fossil::Params {
            templates: &[#(#templates),*],
            ore_block: #ore,
            max_empty_corners: #max_empty_corners,
        };
        features::fossil::place(ctx, pos, &PARAMS)
    }))
}
