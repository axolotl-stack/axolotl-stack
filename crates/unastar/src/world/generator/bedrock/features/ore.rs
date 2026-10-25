//! `minecraft:ore_feature`: a blob of `count` blocks stretched along a random
//! horizontal line, the classic vanilla vein shape shared by both editions
//! (a chain of spheres whose radius peaks mid-way).
//!
//! `replace` maps a target block to the replacement of the first matching
//! `replace_rules` entry; `discard_chance_on_air_exposure` skips a candidate
//! touching air with that probability.

use super::super::world::{BlockPos, BlockWorld, Ctx};
use std::f32::consts::PI;

const NEIGHBOURS: [(i32, i32, i32); 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

/// Place one ore blob centred on `pos`; `Some(pos)` when any block changed.
pub fn place<W: BlockWorld>(
    ctx: &mut Ctx<'_, W>,
    pos: BlockPos,
    count: i32,
    discard_chance_on_air_exposure: f32,
    replace: fn(u32) -> Option<u32>,
    is_air: fn(u32) -> bool,
) -> Option<BlockPos> {
    if count <= 0 {
        return None;
    }
    let size = count as f32;
    let angle = ctx.rng.next_float() * PI;
    let reach = size / 8.0;
    let (sin, cos) = angle.sin_cos();
    let (x0, x1) = (pos.x as f32 + sin * reach, pos.x as f32 - sin * reach);
    let (z0, z1) = (pos.z as f32 + cos * reach, pos.z as f32 - cos * reach);
    let y0 = (pos.y + ctx.rng.next_int(3) - 2) as f32;
    let y1 = (pos.y + ctx.rng.next_int(3) - 2) as f32;
    let mut placed = false;
    for i in 0..count {
        let t = i as f32 / size;
        let cx = x0 + (x1 - x0) * t;
        let cy = y0 + (y1 - y0) * t;
        let cz = z0 + (z1 - z0) * t;
        let scale = ctx.rng.next_float() * size / 16.0;
        let radius = (((PI * t).sin() + 1.0) * scale + 1.0) / 2.0;
        let (lo_x, hi_x) = ((cx - radius).floor() as i32, (cx + radius).floor() as i32);
        let (lo_y, hi_y) = ((cy - radius).floor() as i32, (cy + radius).floor() as i32);
        let (lo_z, hi_z) = ((cz - radius).floor() as i32, (cz + radius).floor() as i32);
        for x in lo_x..=hi_x {
            let dx = (x as f32 + 0.5 - cx) / radius;
            if dx * dx >= 1.0 {
                continue;
            }
            for y in lo_y..=hi_y {
                let dy = (y as f32 + 0.5 - cy) / radius;
                if dx * dx + dy * dy >= 1.0 || !ctx.in_height(y) {
                    continue;
                }
                for z in lo_z..=hi_z {
                    let dz = (z as f32 + 0.5 - cz) / radius;
                    if dx * dx + dy * dy + dz * dz >= 1.0 {
                        continue;
                    }
                    let at = BlockPos::new(x, y, z);
                    let Some(ore) = replace(ctx.get(at)) else {
                        continue;
                    };
                    if discard_chance_on_air_exposure > 0.0
                        && NEIGHBOURS
                            .iter()
                            .any(|&(nx, ny, nz)| is_air(ctx.get(at.offset(nx, ny, nz))))
                        && ctx.rng.probability(discard_chance_on_air_exposure)
                    {
                        continue;
                    }
                    placed |= ctx.set(at, ore);
                }
            }
        }
    }
    placed.then_some(pos)
}
