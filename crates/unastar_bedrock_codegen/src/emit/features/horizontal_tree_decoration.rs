//! `minecraft:horizontal_tree_decoration_feature` ->
//! `features::horizontal_tree_decoration::place` with a static
//! `features::horizontal_tree_decoration::Params` literal.
//!
//! `places_block` is expanded to its four `minecraft:cardinal_direction`
//! states (south, west, north, east = the runtime's `Direction` order); the
//! runtime gives a decoration the direction of the log side it sits on.
//! `bark_side_only` becomes a per-log side mask computed from the palette:
//! every side of wood and hyphae blocks, all four sides of an upright log or
//! stem, north/south of one lying along x, west/east of one lying along z,
//! and no side of stripped logs (they have no bark). Without
//! `bark_side_only` every side is allowed.

use crate::blocks::{IdSet, StateValue};
use crate::emit::Emitter;
use crate::error::{Error, Result};
use crate::ir::{Obj, block};
use proc_macro2::TokenStream;
use quote::quote;

use super::growing_plant::with_state;

/// Rotation state of the decoration.
const DIRECTION_STATE: &str = "minecraft:cardinal_direction";
/// `minecraft:cardinal_direction` values in the runtime's `Direction` order.
const DIRECTIONS: [&str; 4] = ["south", "west", "north", "east"];

pub fn emit(em: &mut Emitter<'_>, o: &Obj<'_>) -> Result<TokenStream> {
    let origin = o.origin;
    let model = em.model;
    let palette = &model.palette;
    let id = block(palette, origin, o.req("places_block")?)?;
    let (name, _, _) = palette
        .describe(id)
        .ok_or_else(|| Error::data(origin, format!("runtime id {id} is not in the palette")))?;
    let bark_side_only = required_bool(o, "bark_side_only")?;
    let allow_adjacent = required_bool(o, "allow_adjacent")?;
    let mut rotations = Vec::new();
    for dir in DIRECTIONS {
        let state = with_state(
            palette,
            origin,
            id,
            DIRECTION_STATE,
            &StateValue::Str(dir.to_owned()),
        )?;
        rotations.push(em.block(state));
    }
    let decoration = palette.all_states(origin, &[name])?;
    let is_decoration = em.matches(quote!(b), &decoration);
    let sides = if bark_side_only {
        let (all, along_x, along_z) = bark_sides(palette);
        let all = em.matches(quote!(b), &all);
        let along_x = em.matches(quote!(b), &along_x);
        let along_z = em.matches(quote!(b), &along_z);
        // Masks over Direction: bit 0 south, 1 west, 2 north, 3 east.
        quote!(if #all { 0b1111 } else if #along_x { 0b0101 } else if #along_z { 0b1010 } else { 0 })
    } else {
        quote!({
            let _ = b;
            0b1111
        })
    };
    Ok(quote!({
        fn is_decoration(b: u32) -> bool { #is_decoration }
        fn sides(b: u32) -> u8 { #sides }
        static PARAMS: features::horizontal_tree_decoration::Params = features::horizontal_tree_decoration::Params {
            rotations: [#(#rotations),*],
            is_decoration,
            sides,
            allow_adjacent: #allow_adjacent,
        };
        features::horizontal_tree_decoration::place(ctx, pos, &PARAMS)
    }))
}

fn required_bool(o: &Obj<'_>, key: &'static str) -> Result<bool> {
    o.req(key)?
        .as_bool()
        .ok_or_else(|| Error::data(o.origin, format!("{key} is not a bool")))
}

/// `(bark on all four sides, lying along x, lying along z)` log states.
fn bark_sides(palette: &crate::blocks::BlockPalette) -> (IdSet, IdSet, IdSet) {
    let (mut all, mut along_x, mut along_z) = (Vec::new(), Vec::new(), Vec::new());
    for name in palette.names() {
        if name.starts_with("minecraft:stripped_") {
            continue;
        }
        let wood = name.ends_with("_wood") || name.ends_with("_hyphae");
        let log = name.ends_with("_log") || name.ends_with("_stem");
        if !wood && !log {
            continue;
        }
        for (id, state) in palette.states_of(name).into_iter().flatten() {
            match state.get("pillar_axis") {
                _ if wood => all.push(id),
                Some(StateValue::Str(axis)) if axis == "x" => along_x.push(id),
                Some(StateValue::Str(axis)) if axis == "z" => along_z.push(id),
                Some(_) => all.push(id),
                // Melon/pumpkin/mushroom stems have no axis and are no logs.
                None => {}
            }
        }
    }
    (
        IdSet::from_ids(all),
        IdSet::from_ids(along_x),
        IdSet::from_ids(along_z),
    )
}
