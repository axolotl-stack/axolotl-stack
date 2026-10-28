//! `minecraft:fossil_feature`: a buried bone structure, following the
//! vanilla fossil algorithm:
//!
//! 1. pick a random rotation and a random template of `structures/fossils`;
//! 2. centre the rotated footprint on the input column and sink it to
//!    `min(input y, lowest heightmap over the footprint) - 15 - [0, 10)`
//!    (never below `min_y + 10`), so fossils stay below the surface;
//! 3. reject the placement when more than `max_empty_corners` of the 8
//!    corners of its box are air or liquid;
//! 4. place every bone with integrity 0.9, then overlay `ore_block` on the
//!    bone cells with integrity 0.1. Vanilla keeps a separate ore overlay
//!    template per fossil with the same shape; BDS ships one template per
//!    fossil, so the overlay reuses the bone cells. Bedrock is never
//!    replaced.

use super::super::generated::{self, bb, props};
use super::super::structures::{place_state, world_pos};
use super::super::world::{BlockPos, BlockWorld, Ctx, OUT_OF_BOUNDS};

/// Compiled feature parameters.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// Indices into `generated::STRUCTURE_TEMPLATES`.
    pub templates: &'static [u16],
    pub ore_block: u32,
    pub max_empty_corners: i32,
}

/// Bone integrity.
const BONE_INTEGRITY: f32 = 0.9;
/// Ore overlay integrity.
const ORE_INTEGRITY: f32 = 0.1;

/// Place one fossil around `pos`; `Some(pos)` when it was placed.
pub fn place<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, p: &Params) -> Option<BlockPos> {
    if p.templates.is_empty() {
        return None;
    }
    let rotation = ctx.rng.next_int(4) as u8;
    let pick = ctx.rng.next_int(p.templates.len() as i32) as usize;
    let t = generated::STRUCTURE_TEMPLATES.get(p.templates[pick] as usize)?;
    let local = t.bounding_box(BlockPos::new(0, 0, 0), rotation);
    let width = local.max.x - local.min.x + 1;
    let depth = local.max.z - local.min.z + 1;
    let corner_x = pos.x - width / 2;
    let corner_z = pos.z - depth / 2;
    let mut ground = pos.y;
    for x in corner_x..corner_x + width {
        for z in corner_z..corner_z + depth {
            ground = ground.min(ctx.heightmap(x, z));
        }
    }
    let sink = ctx.rng.next_int(10);
    let y = (ground - 15 - sink).max(ctx.world.min_y() + 10);
    let origin = BlockPos::new(corner_x - local.min.x, y, corner_z - local.min.z);
    let bbox = t.bounding_box(origin, rotation);
    let mut empty = 0;
    for x in [bbox.min.x, bbox.max.x] {
        for cy in [bbox.min.y, bbox.max.y] {
            for z in [bbox.min.z, bbox.max.z] {
                let id = ctx.get(BlockPos::new(x, cy, z));
                if id == bb::AIR || id == OUT_OF_BOUNDS || props::is_liquid(id) {
                    empty += 1;
                }
            }
        }
    }
    if empty > p.max_empty_corners {
        return None;
    }
    for (_, cell, entry) in t.cells() {
        let at = world_pos(origin, cell, rotation);
        // Both rolls are drawn for every cell so the stream does not depend
        // on the world contents.
        let bone = ctx.rng.probability(BONE_INTEGRITY);
        let ore = ctx.rng.probability(ORE_INTEGRITY);
        if !ctx.in_height(at.y) || ctx.get(at) == bb::BEDROCK {
            continue;
        }
        if ore {
            ctx.set(at, p.ore_block);
        } else if bone {
            let _ = place_state(ctx, at, t.state(entry, rotation));
        }
    }
    Some(pos)
}
