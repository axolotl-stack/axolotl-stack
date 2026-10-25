//! `minecraft:structure_template_feature`: place a compiled structure
//! template after checking its constraints (Mojang's feature documentation).
//!
//! - The template layer `ground_level` is aligned with the input position
//!   (the template is lowered by `ground_level`).
//! - `facing_direction` picks the rotation (random by default). With
//!   `rotate_around_center` the rotated footprint is centred on the input;
//!   otherwise the rotated footprint starts at the input (its minimum corner).
//! - `adjustment_radius` searches horizontal offsets in rings of growing
//!   distance (dx ascending inside a ring) and places at the first valid one.
//! - Constraints: `block_intersection` requires every non-void template cell
//!   (only motion-blocking ones with
//!   `only_check_intersection_for_motion_blocking_blocks`) to cover an
//!   allowlisted world block; `unburied` requires air directly above every
//!   footprint column; `grounded` requires a solid block under every
//!   footprint column at the ground level; `leveled` samples the terrain
//!   halfway along each side of the footprint: a solid block with a
//!   non-solid block above must exist within `max_steepness` of the input y.

use super::super::generated::{self, bb, props};
use super::super::structures::{BoundingBox, Template, world_pos};
use super::super::world::{BlockPos, BlockWorld, Ctx};

/// Rotation choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facing {
    Random,
    /// Clockwise quarter turns.
    Fixed(u8),
}

/// `block_intersection` constraint.
#[derive(Debug, Clone, Copy)]
pub struct Intersection {
    pub allowed: fn(u32) -> bool,
    pub motion_blocking_only: bool,
}

/// Compiled feature parameters.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// Index into `generated::STRUCTURE_TEMPLATES`.
    pub template: u16,
    pub adjustment_radius: i32,
    pub ground_level: i32,
    pub facing: Facing,
    pub rotate_around_center: bool,
    pub grounded: bool,
    pub unburied: bool,
    /// `leveled.max_steepness`, when the constraint is present.
    pub leveled: Option<i32>,
    pub intersection: Option<Intersection>,
}

/// Integer square root of a non-negative value.
fn isqrt(v: i32) -> i32 {
    let mut r = (v as f64).sqrt() as i32;
    while r * r > v {
        r -= 1;
    }
    while (r + 1) * (r + 1) <= v {
        r += 1;
    }
    r
}

/// Place the template; `Some(position)` of the accepted placement.
pub fn place<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, p: &Params) -> Option<BlockPos> {
    let t = generated::STRUCTURE_TEMPLATES.get(p.template as usize)?;
    let rotation = match p.facing {
        Facing::Random => ctx.rng.next_int(4) as u8,
        Facing::Fixed(r) => r % 4,
    };
    let local = t.bounding_box(BlockPos::new(0, 0, 0), rotation);
    let (shift_x, shift_z) = if p.rotate_around_center {
        let (cx, cz) = local.center_xz();
        (-cx, -cz)
    } else {
        (-local.min.x, -local.min.z)
    };
    let r = p.adjustment_radius.max(0);
    for ring in 0..=r * r {
        for dx in -r..=r {
            let rest = ring - dx * dx;
            if rest < 0 {
                continue;
            }
            let dz = isqrt(rest);
            if dz * dz != rest {
                continue;
            }
            let options = if dz == 0 { 1 } else { 2 };
            for dz in [-dz, dz].into_iter().take(options) {
                let at = pos.offset(dx, 0, dz);
                let origin = BlockPos::new(at.x + shift_x, at.y - p.ground_level, at.z + shift_z);
                if fits(ctx, t, origin, rotation, at, p) {
                    t.place(ctx, origin, rotation);
                    return Some(at);
                }
            }
        }
    }
    None
}

/// Terrain surface sample at column `(x, z)`: a solid block with a non-solid
/// block above, searched within `steepness` of `y`.
fn level_sample<W: BlockWorld>(ctx: &Ctx<'_, W>, x: i32, y: i32, z: i32, steepness: i32) -> bool {
    (y - steepness..=y + steepness).any(|top| {
        props::is_solid(ctx.get(BlockPos::new(x, top - 1, z)))
            && !props::is_solid(ctx.get(BlockPos::new(x, top, z)))
    })
}

fn fits<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    t: &Template,
    origin: BlockPos,
    rotation: u8,
    at: BlockPos,
    p: &Params,
) -> bool {
    let bbox: BoundingBox = t.bounding_box(origin, rotation);
    if !ctx.in_height(bbox.min.y) || !ctx.in_height(bbox.max.y) {
        return false;
    }
    if let Some(steepness) = p.leveled {
        let (cx, cz) = bbox.center_xz();
        let samples = [
            (bbox.min.x, cz),
            (bbox.max.x, cz),
            (cx, bbox.min.z),
            (cx, bbox.max.z),
        ];
        if !samples
            .iter()
            .all(|&(x, z)| level_sample(ctx, x, at.y, z, steepness))
        {
            return false;
        }
    }
    if p.unburied {
        let y = bbox.max.y + 1;
        for x in bbox.min.x..=bbox.max.x {
            for z in bbox.min.z..=bbox.max.z {
                if ctx.get(BlockPos::new(x, y, z)) != bb::AIR {
                    return false;
                }
            }
        }
    }
    if p.grounded {
        for x in bbox.min.x..=bbox.max.x {
            for z in bbox.min.z..=bbox.max.z {
                if !props::is_solid(ctx.get(BlockPos::new(x, at.y - 1, z))) {
                    return false;
                }
            }
        }
    }
    if let Some(i) = p.intersection {
        for (_, cell, entry) in t.cells() {
            if i.motion_blocking_only && !props::blocks_motion(t.state(entry, rotation)) {
                continue;
            }
            if !(i.allowed)(ctx.get(world_pos(origin, cell, rotation))) {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::isqrt;

    #[test]
    fn integer_square_roots() {
        for v in 0..2000 {
            let r = isqrt(v);
            assert!(r * r <= v && (r + 1) * (r + 1) > v, "{v}");
        }
    }
}
