//! `minecraft:vegetation_patch_feature` -> `features::vegetation_patch::place`
//! with a static `features::vegetation_patch::Params` literal and the
//! compiled `vegetation_feature` passed as a closure (a `static` cannot hold
//! the generic feature function).
//!
//! Optional keys default to vanilla's neutral values: chances `0`,
//! `waterlogged` false. `depth` and `horizontal_radius` accept a constant or
//! an inclusive range.

use crate::emit::Emitter;
use crate::emit::features::range_tokens;
use crate::error::{Error, Result};
use crate::ir::{Obj, block, block_set, int_range};
use proc_macro2::TokenStream;
use quote::quote;

/// `features::vegetation_patch::MAX_RADIUS` of the runtime (the bound of its
/// on-stack cell buffer).
const MAX_RADIUS: i32 = 11;

pub fn emit(em: &mut Emitter<'_>, o: &Obj<'_>) -> Result<TokenStream> {
    let origin = o.origin;
    let model = em.model;
    let palette = &model.palette;
    let replaceable = block_set(palette, origin, o.req("replaceable_blocks")?)?;
    let ground = block(palette, origin, o.req("ground_block")?)?;
    let vegetation = o.str("vegetation_feature")?;
    let ceiling = match o.str("surface")? {
        "floor" => false,
        "ceiling" => true,
        other => return Err(Error::data(origin, format!("unknown vegetation patch surface {other:?}"))),
    };
    let depth = range_tokens(origin, o.req("depth")?)?;
    let vertical_range = o.i32("vertical_range")?;
    let vegetation_chance = o.f32_or("vegetation_chance", 0.0)?;
    let radius = int_range(origin, o.req("horizontal_radius")?)?;
    if radius.min < 0 || radius.max < radius.min || radius.max > MAX_RADIUS {
        return Err(Error::data(
            origin,
            format!("horizontal_radius {}..={} outside 0..={MAX_RADIUS}", radius.min, radius.max),
        ));
    }
    let extra_deep = o.f32_or("extra_deep_block_chance", 0.0)?;
    let extra_edge = o.f32_or("extra_edge_column_chance", 0.0)?;
    let waterlogged = o.bool_or("waterlogged", false)?;
    if waterlogged && ceiling {
        // The pool test looks at the cell's sides and bottom: floor only.
        return Err(Error::data(origin, "waterlogged ceiling patches are not supported"));
    }
    let test = em.matches(quote!(b), &replaceable);
    let ground = em.block(ground);
    let call = em.call(vegetation, quote!(p));
    let (rmin, rmax) = (radius.min, radius.max);
    Ok(quote!({
        fn replaceable(b: u32) -> bool { #test }
        static PARAMS: features::vegetation_patch::Params = features::vegetation_patch::Params {
            replaceable,
            ground: #ground,
            ceiling: #ceiling,
            depth: #depth,
            vertical_range: #vertical_range,
            vegetation_chance: #vegetation_chance,
            horizontal_radius: features::IntRange { min: #rmin, max: #rmax },
            extra_deep_block_chance: #extra_deep,
            extra_edge_column_chance: #extra_edge,
            waterlogged: #waterlogged,
        };
        features::vegetation_patch::place(ctx, pos, &PARAMS, |ctx, p| #call)
    }))
}
