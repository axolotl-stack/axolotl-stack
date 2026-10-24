//! `minecraft:partially_exposed_blob_feature` ->
//! `features::partially_exposed_blob::place` with a static
//! `features::partially_exposed_blob::Params` literal. `exposed_face`
//! defaults to `up` (Mojang's documented default) and is emitted as a face
//! index in `multi_face_direction_bits` order (0 down, 1 up, 2 south,
//! 3 west, 4 north, 5 east).

use crate::emit::Emitter;
use crate::error::{Error, Result};
use crate::ir::{Obj, block};
use proc_macro2::TokenStream;
use quote::quote;
use serde_json::Value;

pub fn emit(em: &mut Emitter<'_>, o: &Obj<'_>) -> Result<TokenStream> {
    let origin = o.origin;
    let id = block(&em.model.palette, origin, o.req("places_block")?)?;
    let radius = o.i32("placement_radius_around_floor")?;
    if radius < 0 {
        return Err(Error::data(origin, format!("negative placement_radius_around_floor {radius}")));
    }
    let probability = o.f32("placement_probability_per_valid_position")?;
    let face: usize = match o.opt("exposed_face") {
        None => 1,
        Some(Value::String(s)) => match s.as_str() {
            "down" => 0,
            "up" => 1,
            "south" => 2,
            "west" => 3,
            "north" => 4,
            "east" => 5,
            other => return Err(Error::data(origin, format!("unknown exposed_face {other:?}"))),
        },
        Some(other) => return Err(Error::data(origin, format!("exposed_face is not a string: {other}"))),
    };
    let places = em.block(id);
    Ok(quote!({
        static PARAMS: features::partially_exposed_blob::Params = features::partially_exposed_blob::Params {
            block: #places,
            radius: #radius,
            probability: #probability,
            exposed_face: #face,
        };
        features::partially_exposed_blob::place(ctx, pos, &PARAMS)
    }))
}
