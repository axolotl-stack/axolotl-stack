//! Unrolled scatter loops for feature rules and `minecraft:scatter_feature`.
//!
//! Coordinate semantics (inferred from the shipped data, logged in
//! `thoughts/shared/research/bds-quirks.md`):
//! - numeric constants and distribution objects are offsets from the input
//!   position (rules use the chunk origin at y = 0);
//! - Molang strings evaluate to absolute coordinates: the eight ocean seagrass
//!   scatters place at `query.heightmap(...)` below rules that already placed
//!   them at the heightmap, which only works if the value is absolute;
//! - `project_input_to_floor` moves the base of the y offset to the heightmap
//!   of the column computed so far;
//! - `scatter_chance` is rolled once per scatter, before any iteration;
//! - grid distributions advance like an odometer over the iteration index, the
//!   first grid axis in evaluation order fastest.

use super::Emitter;
use crate::error::Result;
use crate::ir::{Axis, Coord, DistKind, Scatter, ScatterChance};
use crate::molang::{self, Env, Var};
use proc_macro2::TokenStream;
use quote::quote;

/// Molang environment inside a scatter loop: `wx/wy/wz` hold the coordinates
/// computed so far, `origin` the input position.
pub struct ScatterEnv<'a, 'm> {
    pub em: &'a Emitter<'m>,
}

impl Env for ScatterEnv<'_, '_> {
    fn var(&self, var: Var) -> Result<TokenStream> {
        Ok(match var {
            Var::WorldX => quote!((wx as f32)),
            Var::WorldY => quote!((wy as f32)),
            Var::WorldZ => quote!((wz as f32)),
            Var::OriginX => quote!((origin.x as f32)),
            Var::OriginY => quote!((origin.y as f32)),
            Var::OriginZ => quote!((origin.z as f32)),
            Var::IsLegacy => quote!(0.0f32),
            // Overworld sea level.
            Var::SeaLevel => quote!(63.0f32),
        })
    }

    fn has_biome_tag(&self, tag: &str) -> Result<TokenStream> {
        Ok(biome_tag_test(self.em, tag, quote!(ctx.biome(origin))))
    }

    fn ctx(&self) -> TokenStream {
        quote!(ctx)
    }
}

/// `bool` expression: does the biome id `biome` carry `tag`?
pub fn biome_tag_test(em: &Emitter<'_>, tag: &str, biome: TokenStream) -> TokenStream {
    let ids: Vec<u16> = em
        .model
        .biomes
        .iter()
        .filter(|b| b.tags.contains(tag))
        .map(|b| b.id)
        .collect();
    if ids.is_empty() {
        quote!(false)
    } else {
        quote!(matches!(#biome, #(#ids)|*))
    }
}

fn int_expr(origin: &str, env: &ScatterEnv<'_, '_>, e: &molang::Expr) -> Result<TokenStream> {
    if let Some(v) = e.as_const() {
        let v = v.floor() as i32;
        return Ok(quote!(#v));
    }
    let t = molang::emit(origin, e, env)?;
    Ok(quote!(scatter::coord(#t)))
}

/// Emit a block evaluating to `Option<BlockPos>`: the last successful
/// placement of `place(pos)` over every scattered position, or `None`.
///
/// Expects `ctx` and `origin: BlockPos` in scope.
pub fn emit(
    em: &Emitter<'_>,
    origin_doc: &str,
    s: &Scatter,
    place: impl Fn(TokenStream) -> TokenStream,
) -> Result<TokenStream> {
    let env = ScatterEnv { em };
    let chance = match &s.chance {
        None => quote!(),
        Some(ScatterChance::Fraction {
            numerator,
            denominator,
        }) => {
            if numerator >= denominator {
                quote!()
            } else {
                quote!(if !ctx.rng.chance(#numerator, #denominator) { return None; })
            }
        }
        Some(ScatterChance::Percent(e)) => match e.as_const() {
            Some(p) if p >= 100.0 => quote!(),
            _ => {
                let p = molang::emit(origin_doc, e, &env)?;
                quote!(if !(ctx.rng.next_float() * 100.0f32 < #p) { return None; })
            }
        },
    };

    // Grid axes in evaluation order, for the odometer.
    let grid_axes: Vec<Axis> = s
        .order
        .iter()
        .copied()
        .filter(|a| {
            matches!(
                coord_of(s, *a),
                Coord::Dist {
                    kind: DistKind::FixedGrid | DistKind::JitteredGrid,
                    ..
                }
            )
        })
        .collect();

    // Each computed axis becomes `let wN = value;` in evaluation order. An axis
    // that is never computed, or that an earlier value reads (`variable.worldx`,
    // the projected floor), is first bound to the input coordinate.
    let mut steps = Vec::new();
    let mut prelude = Vec::new();
    let mut defined = [false; 3];
    let bind = |i: usize, defined: &mut [bool; 3], prelude: &mut Vec<TokenStream>| {
        if !defined[i] {
            defined[i] = true;
            prelude.push(match i {
                0 => quote!(let wx = origin.x;),
                1 => quote!(let wy = origin.y;),
                _ => quote!(let wz = origin.z;),
            });
        }
    };
    let mut grid_divisor = quote!(1i32);
    for axis in s.order {
        let (var, base, index) = match axis {
            Axis::X => (quote!(wx), quote!(origin.x), 0),
            Axis::Y => (
                quote!(wy),
                if s.project_input_to_floor {
                    quote!(ctx.heightmap(wx, wz))
                } else {
                    quote!(origin.y)
                },
                1,
            ),
            Axis::Z => (quote!(wz), quote!(origin.z), 2),
        };
        let coord = coord_of(s, axis);
        if matches!(coord, Coord::Offset(0)) && !(axis == Axis::Y && s.project_input_to_floor) {
            continue;
        }
        let mut reads = coord_reads(coord);
        if axis == Axis::Y && s.project_input_to_floor {
            reads[0] = true;
            reads[2] = true;
        }
        for (i, read) in reads.into_iter().enumerate() {
            if read {
                bind(i, &mut defined, &mut prelude);
            }
        }
        defined[index] = true;
        let value = match coord {
            Coord::Offset(n) => quote!(#base + #n),
            Coord::Expr(e) => int_expr(origin_doc, &env, e)?,
            Coord::Dist {
                kind,
                extent,
                step_size,
                grid_offset,
            } => {
                let a = int_expr(origin_doc, &env, &extent[0])?;
                let b = int_expr(origin_doc, &env, &extent[1])?;
                // Bind the extent first: it may query `ctx` while the sample borrows `ctx.rng`.
                let sample = match kind {
                    DistKind::Uniform => quote!(scatter::uniform(ctx.rng, lo, hi)),
                    DistKind::Triangle => quote!(scatter::triangle(ctx.rng, lo, hi)),
                    DistKind::Gaussian => quote!(scatter::gaussian(ctx.rng, lo, hi)),
                    DistKind::InverseGaussian => quote!(scatter::inverse_gaussian(ctx.rng, lo, hi)),
                    DistKind::FixedGrid | DistKind::JitteredGrid => {
                        let index = quote!((i / (#grid_divisor)));
                        grid_divisor = quote!((#grid_divisor) * scatter::grid_cells(#a, #b, #step_size, #grid_offset));
                        if *kind == DistKind::FixedGrid {
                            quote!(scatter::fixed_grid(lo, hi, #step_size, #grid_offset, #index))
                        } else {
                            quote!(scatter::jittered_grid(ctx.rng, lo, hi, #step_size, #grid_offset, #index))
                        }
                    }
                };
                quote!({ let lo = #a; let hi = #b; #base + #sample })
            }
        };
        steps.push(quote!(let #var = #value;));
    }
    for i in 0..3 {
        bind(i, &mut defined, &mut prelude);
    }

    let iterations = match s.iterations.as_const() {
        Some(n) => {
            let n = n.floor() as i32;
            quote!(#n)
        }
        None => {
            let e = molang::emit(origin_doc, &s.iterations, &env)?;
            quote!(scatter::coord(#e))
        }
    };
    let loop_var = if grid_axes.is_empty() {
        quote!(_)
    } else {
        quote!(i)
    };
    let place = place(quote!(BlockPos::new(wx, wy, wz)));
    Ok(quote! {{
        #chance
        let mut last = None;
        for #loop_var in 0..#iterations {
            #(#prelude)*
            #(#steps)*
            if let Some(p) = #place {
                last = Some(p);
            }
        }
        last
    }})
}

fn coord_of(s: &Scatter, axis: Axis) -> &Coord {
    match axis {
        Axis::X => &s.coords[0],
        Axis::Y => &s.coords[1],
        Axis::Z => &s.coords[2],
    }
}

/// Which of `variable.worldx/worldy/worldz` a coordinate's value reads.
fn coord_reads(coord: &Coord) -> [bool; 3] {
    let exprs: Vec<&molang::Expr> = match coord {
        Coord::Offset(_) => Vec::new(),
        Coord::Expr(e) => vec![e],
        Coord::Dist { extent, .. } => extent.iter().collect(),
    };
    [Var::WorldX, Var::WorldY, Var::WorldZ].map(|v| exprs.iter().any(|e| e.reads(v)))
}
