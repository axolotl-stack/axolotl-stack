//! `minecraft:structure_template_feature`: places a structure template
//! (compiled into `STRUCTURE_TEMPLATES` by [`crate::structures`]) after
//! checking its constraints. Keys: `structure_name`, `adjustment_radius`
//! (0..=16), `facing_direction` (`random` by default), `ground_level`
//! (clamped to the template height), `rotate_around_center`, `constraints`
//! (`grounded`, `unburied`, `leveled.max_steepness` (default 2),
//! `block_intersection` with `block_allowlist`/`block_whitelist` and
//! `only_check_intersection_for_motion_blocking_blocks`).

use crate::emit::Emitter;
use crate::error::{Error, Result};
use crate::ir::{self, Obj};
use proc_macro2::TokenStream;
use quote::quote;
use serde_json::Value;

/// An empty constraint object (`"unburied": {}`).
fn empty_object(origin: &str, v: &Value) -> Result<()> {
    Obj::new(origin, v)?.finish()
}

pub fn emit(em: &mut Emitter<'_>, o: &Obj<'_>) -> Result<TokenStream> {
    let origin = o.origin;
    let model = em.model;
    let name = o.str("structure_name")?;
    let template_index = model.structures.named_template(name).ok_or_else(|| {
        Error::data(
            origin,
            format!("structure template {name:?} was not loaded"),
        )
    })?;
    let height = model.structures.templates[template_index].size[1];
    let template = u16::try_from(template_index)
        .map_err(|_| Error::data(origin, "template index overflow"))?;
    let radius = o.i32_or("adjustment_radius", 0)?;
    if !(0..=16).contains(&radius) {
        return Err(Error::data(
            origin,
            format!("adjustment_radius {radius} outside 0..=16"),
        ));
    }
    // Quarter turns clockwise from the template's own orientation (north).
    let fixed =
        |quarter_turns: u8| quote!(features::structure_template::Facing::Fixed(#quarter_turns));
    let facing = match o.opt("facing_direction") {
        None => quote!(features::structure_template::Facing::Random),
        Some(v) => match v.as_str() {
            Some("random") => quote!(features::structure_template::Facing::Random),
            Some("north") => fixed(0),
            Some("east") => fixed(1),
            Some("south") => fixed(2),
            Some("west") => fixed(3),
            _ => return Err(Error::data(origin, format!("facing_direction {v}"))),
        },
    };
    let ground_level = o.i32_or("ground_level", 0)?;
    if ground_level < 0 {
        return Err(Error::data(
            origin,
            format!("ground_level {ground_level} is negative"),
        ));
    }
    // Values above the template height are clamped (Mojang docs).
    let ground_level = ground_level.min(height - 1);
    let around = o.bool_or("rotate_around_center", false)?;

    let mut grounded = false;
    let mut unburied = false;
    let mut leveled = quote!(None);
    let mut intersection = quote!(None);
    let mut allowed_fn = quote!();
    if let Some(c) = o.opt("constraints") {
        let c = Obj::new(origin, c)?;
        if let Some(v) = c.opt("grounded") {
            empty_object(origin, v)?;
            grounded = true;
        }
        if let Some(v) = c.opt("unburied") {
            empty_object(origin, v)?;
            unburied = true;
        }
        if let Some(v) = c.opt("leveled") {
            let l = Obj::new(origin, v)?;
            let steepness = l.i32_or("max_steepness", 2)?;
            l.finish()?;
            if steepness < 0 {
                return Err(Error::data(
                    origin,
                    format!("max_steepness {steepness} is negative"),
                ));
            }
            leveled = quote!(Some(#steepness));
        }
        if let Some(v) = c.opt("block_intersection") {
            let b = Obj::new(origin, v)?;
            let list = match (b.opt("block_allowlist"), b.opt("block_whitelist")) {
                (Some(list), None) | (None, Some(list)) => list,
                _ => {
                    return Err(Error::data(
                        origin,
                        "block_intersection needs exactly one of block_allowlist/block_whitelist",
                    ));
                }
            };
            let set = ir::block_set(&model.palette, origin, list)?;
            let motion = b.bool_or("only_check_intersection_for_motion_blocking_blocks", false)?;
            b.finish()?;
            let test = em.matches(quote!(b), &set);
            allowed_fn = quote!(fn allowed(b: u32) -> bool { #test });
            intersection = quote!(Some(features::structure_template::Intersection {
                allowed,
                motion_blocking_only: #motion,
            }));
        }
        c.finish()?;
    }
    Ok(quote!({
        #allowed_fn
        static PARAMS: features::structure_template::Params = features::structure_template::Params {
            template: #template,
            adjustment_radius: #radius,
            ground_level: #ground_level,
            facing: #facing,
            rotate_around_center: #around,
            grounded: #grounded,
            unburied: #unburied,
            leveled: #leveled,
            intersection: #intersection,
        };
        features::structure_template::place(ctx, pos, &PARAMS)
    }))
}
