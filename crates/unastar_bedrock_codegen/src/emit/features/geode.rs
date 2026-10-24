//! `minecraft:geode_feature` -> `features::geode::place` with a static
//! `features::geode::Params` literal.
//!
//! The pack's `max_*` bounds of the three integer ranges are exclusive (the
//! vanilla 1.26 values `4..7`, `3..5`, `1..3` are one above the inclusive
//! ranges both editions generate), so they are emitted as inclusive
//! `IntRange`s with `max - 1`. Each `inner_placements` block is expanded to
//! its six `minecraft:block_face` states (down, up, north, south, west,
//! east), so a cluster can face away from the shell it grows on.

use crate::blocks::{BlockPalette, StateValue};
use crate::emit::Emitter;
use crate::error::{Error, Result};
use crate::ir::{Obj, block};
use proc_macro2::TokenStream;
use quote::quote;
use serde_json::Value;

/// Facing state of amethyst buds and clusters.
const FACE_STATE: &str = "minecraft:block_face";
/// Face order of the runtime's `inner_placements` rows.
const FACES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

pub fn emit(em: &mut Emitter<'_>, o: &Obj<'_>) -> Result<TokenStream> {
    let origin = o.origin;
    let model = em.model;
    let palette = &model.palette;
    let layer = |key: &'static str| -> Result<u32> { block(palette, origin, o.req(key)?) };
    let filler = layer("filler")?;
    let inner = layer("inner_layer")?;
    let alternate = layer("alternate_inner_layer")?;
    let middle = layer("middle_layer")?;
    let outer = layer("outer_layer")?;
    let mut placements = Vec::new();
    for v in o.array("inner_placements")? {
        placements.push(facings(palette, origin, block(palette, origin, v)?)?);
    }
    let (wall_min, wall_max) = exclusive_range(o, "min_outer_wall_distance", "max_outer_wall_distance")?;
    let (points_min, points_max) = exclusive_range(o, "min_distribution_points", "max_distribution_points")?;
    let (offset_min, offset_max) = exclusive_range(o, "min_point_offset", "max_point_offset")?;
    let max_radius = o.i32("max_radius")?;
    let crack_point_offset = o.f32("crack_point_offset")?;
    let generate_crack_chance = o.f32("generate_crack_chance")?;
    let base_crack_size = o.f32("base_crack_size")?;
    let noise_multiplier = o.f32("noise_multiplier")?;
    let use_potential_placements_chance = o.f32("use_potential_placements_chance")?;
    let use_alternate_layer0_chance = o.f32("use_alternate_layer0_chance")?;
    let placements_require_layer0_alternate = match o.req("placements_require_layer0_alternate")? {
        Value::Bool(b) => *b,
        other => {
            return Err(Error::data(
                origin,
                format!("placements_require_layer0_alternate is not a bool: {other}"),
            ));
        }
    };
    let invalid_blocks_threshold = o.i32("invalid_blocks_threshold")?;
    if max_radius <= 0 {
        return Err(Error::data(origin, format!("max_radius must be positive, got {max_radius}")));
    }

    let mut rows = Vec::with_capacity(placements.len());
    for row in &placements {
        let cells: Vec<TokenStream> = row.iter().map(|&id| em.block(id)).collect();
        rows.push(quote!([#(#cells),*]));
    }
    let count = rows.len();
    let [filler, inner, alternate, middle, outer] = [filler, inner, alternate, middle, outer].map(|id| em.block(id));
    Ok(quote! {{
        static INNER_PLACEMENTS: [[u32; 6]; #count] = [#(#rows),*];
        static PARAMS: features::geode::Params = features::geode::Params {
            filler: #filler,
            inner_layer: #inner,
            alternate_inner_layer: #alternate,
            middle_layer: #middle,
            outer_layer: #outer,
            inner_placements: &INNER_PLACEMENTS,
            outer_wall_distance: features::IntRange { min: #wall_min, max: #wall_max },
            distribution_points: features::IntRange { min: #points_min, max: #points_max },
            point_offset: features::IntRange { min: #offset_min, max: #offset_max },
            max_radius: #max_radius,
            crack_point_offset: #crack_point_offset,
            generate_crack_chance: #generate_crack_chance,
            base_crack_size: #base_crack_size,
            noise_multiplier: #noise_multiplier,
            use_potential_placements_chance: #use_potential_placements_chance,
            use_alternate_layer0_chance: #use_alternate_layer0_chance,
            placements_require_layer0_alternate: #placements_require_layer0_alternate,
            invalid_blocks_threshold: #invalid_blocks_threshold,
        };
        features::geode::place(ctx, pos, &PARAMS)
    }})
}

/// `[min, max)` from the pack as an inclusive `(min, max - 1)`.
fn exclusive_range(o: &Obj<'_>, min_key: &'static str, max_key: &'static str) -> Result<(i32, i32)> {
    let (min, max) = (o.i32(min_key)?, o.i32(max_key)?);
    if max <= min {
        return Err(Error::data(o.origin, format!("{max_key} ({max}) must exceed {min_key} ({min})")));
    }
    Ok((min, max - 1))
}

/// The six `minecraft:block_face` states of `id` (other states kept), in
/// [`FACES`] order; `[id; 6]` for blocks without that state.
fn facings(palette: &BlockPalette, origin: &str, id: u32) -> Result<[u32; 6]> {
    let (name, _, state) = palette
        .describe(id)
        .ok_or_else(|| Error::data(origin, format!("runtime id {id} is not in the palette")))?;
    let mut out = [id; 6];
    if !state.contains_key(FACE_STATE) {
        return Ok(out);
    }
    for (slot, face) in out.iter_mut().zip(FACES) {
        let found = palette.states_of(name).into_iter().flatten().find(|(_, s)| {
            s.iter().all(|(k, v)| {
                if k == FACE_STATE {
                    matches!(v, StateValue::Str(f) if f == face)
                } else {
                    state.get(k) == Some(v)
                }
            })
        });
        *slot = found
            .map(|(i, _)| i)
            .ok_or_else(|| Error::data(origin, format!("{name} has no {FACE_STATE}={face} state")))?;
    }
    Ok(out)
}
