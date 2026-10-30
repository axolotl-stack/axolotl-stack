//! Whether a block face can hold an attached block, from the block's
//! collision shape.
//!
//! A face gives **full** support when the block's collision boxes cover the
//! whole face, and **centre** support when they cover the middle of it (a
//! 2/16 square around the face centre: fence and wall tops hold a torch).
//! Blocks without collision (air, plants, liquids) support nothing.

use glam::{DVec3, IVec3};

use crate::entity::systems::BlockSource;
use crate::registry::collision::block_collision_aabbs;

/// What an attached block needs from the face it hangs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportKind {
    /// The whole face.
    Full,
    /// The middle of the face.
    Centre,
    /// What doors stand on: a full face, or the top of farmland or a dirt
    /// path (lower than a full block, still accepted).
    Edge,
}

const EPS: f64 = 1e-6;

/// Whether the face of the block at `pos` that points along `face` (a unit
/// offset, e.g. `IVec3::Y` for the top) gives `kind` support.
pub fn provides_support(
    blocks: &impl BlockSource,
    pos: IVec3,
    face: IVec3,
    kind: SupportKind,
) -> bool {
    let Some(state) = blocks.block_at(pos) else {
        return false;
    };
    if kind == SupportKind::Edge {
        let low_top = face == IVec3::Y
            && blocks
                .registry()
                .get_by_runtime_id(state)
                .is_some_and(|entry| {
                    matches!(
                        entry.string_id.as_str(),
                        "minecraft:farmland" | "minecraft:grass_path"
                    )
                });
        return low_top || provides_support(blocks, pos, face, SupportKind::Full);
    }
    let (lo, hi) = match kind {
        SupportKind::Full | SupportKind::Edge => (0.0, 1.0),
        SupportKind::Centre => (7.0 / 16.0, 9.0 / 16.0),
    };
    // The face plane and the two axes spanning it.
    let axis = if face.x != 0 {
        0
    } else if face.y != 0 {
        1
    } else {
        2
    };
    let plane = if face[axis] > 0 { 1.0 } else { 0.0 };
    let (u, v) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    // Boxes reaching the face plane (fence and wall tops rise past it),
    // projected onto it.
    let rects: Vec<[f64; 4]> = block_collision_aabbs(state)
        .filter(|b| {
            let (min, max) = (axis_of(b.min, axis), axis_of(b.max, axis));
            if plane > 0.5 {
                max > 1.0 - EPS && min < 1.0 - EPS
            } else {
                min < EPS && max > EPS
            }
        })
        .map(|b| {
            [
                axis_of(b.min, u),
                axis_of(b.max, u),
                axis_of(b.min, v),
                axis_of(b.max, v),
            ]
        })
        .collect();
    covers(&rects, lo, hi)
}

fn axis_of(v: DVec3, axis: usize) -> f64 {
    v[axis]
}

/// Whether the union of `rects` (`[u0, u1, v0, v1]`) covers the square
/// `[lo, hi]²`, by checking the cells of the grid their edges cut.
fn covers(rects: &[[f64; 4]], lo: f64, hi: f64) -> bool {
    if rects.is_empty() {
        return false;
    }
    let mut us = vec![lo, hi];
    let mut vs = vec![lo, hi];
    let inside = |value: f64| value > lo && value < hi;
    for r in rects {
        us.extend([r[0], r[1]].into_iter().filter(|v| inside(*v)));
        vs.extend([r[2], r[3]].into_iter().filter(|v| inside(*v)));
    }
    us.sort_by(f64::total_cmp);
    vs.sort_by(f64::total_cmp);
    us.windows(2).all(|u| {
        vs.windows(2).all(|v| {
            let (cu, cv) = ((u[0] + u[1]) / 2.0, (v[0] + v[1]) / 2.0);
            (u[1] - u[0]) < EPS
                || (v[1] - v[0]) < EPS
                || rects.iter().any(|r| {
                    r[0] - EPS <= cu && cu <= r[1] + EPS && r[2] - EPS <= cv && cv <= r[3] + EPS
                })
        })
    })
}
