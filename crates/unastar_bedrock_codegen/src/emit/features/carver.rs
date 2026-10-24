//! The three worm carver types -> `features::carver::place` with a static
//! `features::carver::Params` literal. They share every key; [`Kind`] picks
//! the runtime variant (replaceable set, liquid rules, tunnel shape).
//!
//! The feature identifier is hashed (FNV-1a 64) into the parameters' `salt`
//! so two carvers of one type (`underground_cave_carver_feature` and
//! `underground_extra_cave_carver_feature`) start caves in different source
//! chunks. The hash only depends on the identifier, so worlds are identical
//! across machines.

use crate::emit::Emitter;
use crate::error::{Error, Result};
use crate::ir::{Obj, block, expr, num};
use proc_macro2::TokenStream;
use quote::quote;
use serde_json::Value;

/// Which carver feature type is being emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `minecraft:cave_carver_feature`.
    Cave,
    /// `minecraft:underwater_cave_carver_feature`.
    UnderwaterCave,
    /// `minecraft:nether_cave_carver_feature`.
    NetherCave,
}

pub fn emit(em: &mut Emitter<'_>, o: &Obj<'_>, kind: Kind) -> Result<TokenStream> {
    let origin = o.origin;
    let model = em.model;
    let identifier = o
        .opt("description")
        .and_then(|d| d.get("identifier"))
        .and_then(Value::as_str)
        .ok_or_else(|| Error::data(origin, "carver feature without description.identifier"))?;
    let salt = fnv1a(identifier);
    let fill_with = block(&model.palette, origin, o.req("fill_with")?)?;
    let replace_air_with = o
        .opt("replace_air_with")
        .map(|v| block(&model.palette, origin, v))
        .transpose()?;
    let width_modifier = expr(origin, o.req("width_modifier")?)?
        .as_const()
        .ok_or_else(|| Error::data(origin, "width_modifier must be a constant"))?;
    let skip_carve_chance = o.i32("skip_carve_chance")?;
    let height_limit = o.i32("height_limit")?;
    if skip_carve_chance < 0 || height_limit <= 0 {
        return Err(Error::data(
            origin,
            format!("skip_carve_chance ({skip_carve_chance}) must be >= 0 and height_limit ({height_limit}) > 0"),
        ));
    }
    let y_scale = float_range(origin, "y_scale", o.req("y_scale")?)?;
    let horizontal = float_range(origin, "horizontal_radius_multiplier", o.req("horizontal_radius_multiplier")?)?;
    let vertical = float_range(origin, "vertical_radius_multiplier", o.req("vertical_radius_multiplier")?)?;
    let floor_level = float_range(origin, "floor_level", o.req("floor_level")?)?;

    let kind = match kind {
        Kind::Cave => quote!(features::carver::Kind::Cave),
        Kind::UnderwaterCave => quote!(features::carver::Kind::UnderwaterCave),
        Kind::NetherCave => quote!(features::carver::Kind::NetherCave),
    };
    let fill_with = em.block(fill_with);
    let replace_air_with = match replace_air_with {
        Some(id) => {
            let id = em.block(id);
            quote!(Some(#id))
        }
        None => quote!(None),
    };
    Ok(quote! {{
        static PARAMS: features::carver::Params = features::carver::Params {
            kind: #kind,
            salt: #salt,
            fill_with: #fill_with,
            replace_air_with: #replace_air_with,
            width_modifier: #width_modifier,
            skip_carve_chance: #skip_carve_chance,
            height_limit: #height_limit,
            y_scale: #y_scale,
            horizontal_radius_multiplier: #horizontal,
            vertical_radius_multiplier: #vertical,
            floor_level: #floor_level,
        };
        features::carver::place(ctx, pos, &PARAMS)
    }})
}

/// `[min, max]` (or a single number) as a `features::carver::FloatRange`.
fn float_range(origin: &str, key: &str, v: &Value) -> Result<TokenStream> {
    let (min, max) = match v {
        Value::Array(items) if items.len() == 2 => (num(origin, key, &items[0])?, num(origin, key, &items[1])?),
        Value::Number(_) => {
            let n = num(origin, key, v)?;
            (n, n)
        }
        other => return Err(Error::data(origin, format!("{key} is not a [min, max] range: {other}"))),
    };
    if max < min {
        return Err(Error::data(origin, format!("{key} range [{min}, {max}] is reversed")));
    }
    Ok(quote!(features::carver::FloatRange { min: #min, max: #max }))
}

/// FNV-1a 64 of `text`.
fn fnv1a(text: &str) -> u64 {
    text.bytes()
        .fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_reference_values() {
        assert_eq!(fnv1a(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a("a"), 0xaf63_dc4c_8601_ec8c);
        assert_ne!(
            fnv1a("minecraft:underground_cave_carver_feature"),
            fnv1a("minecraft:underground_extra_cave_carver_feature")
        );
    }
}
