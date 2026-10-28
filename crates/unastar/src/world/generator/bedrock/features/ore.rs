//! `minecraft:ore_feature`: a chain of `count` overlapping spheres.
//! Bedrock's internal horizontal center is input + (8, 0, 8), as observed in
//! isolated native coal controls; `count` is not a resulting block budget.
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

/// Place a vein with Bedrock's internal (+8, 0, +8) translation; return the input on success.
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
    let origin_x = pos.x as f32 + 8.0;
    let origin_z = pos.z as f32 + 8.0;
    let (x0, x1) = (origin_x + sin * reach, origin_x - sin * reach);
    let (z0, z1) = (origin_z + cos * reach, origin_z - cos * reach);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::BlockRegistry;
    use crate::world::ChunkPos;
    use crate::world::generator::bedrock::{molang_noise::MolangNoise, rng::BedrockRandom};
    use crate::world::generator::proto::ProtoChunk;
    use std::sync::LazyLock;

    static IDS: LazyLock<(u32, u32, u32)> = LazyLock::new(|| {
        let registry = BlockRegistry::vanilla();
        (
            registry
                .default_state_id_by_name("minecraft:stone")
                .unwrap(),
            registry
                .default_state_id_by_name("minecraft:coal_ore")
                .unwrap(),
            registry.default_state_id_by_name("minecraft:air").unwrap(),
        )
    });

    fn replace_stone(id: u32) -> Option<u32> {
        (id == IDS.0).then_some(IDS.1)
    }

    fn is_air(id: u32) -> bool {
        id == IDS.2
    }

    #[test]
    fn count_17_ore_uses_the_observed_positive_horizontal_offset_at_signed_origins() {
        // BDS 1.26.51: 32 full stone-box controls, then three controls each at
        // input X=-128,0,128. All count-17 coal cells have X/Z offsets 5..10;
        // voxel-center means are approximately +8/+8, not the input position.
        // Feature RNG seeds are unmeasured: test the coordinate convention,
        // not a fabricated exact replay of one of those native voxel sets.
        let noise = MolangNoise;
        for chunk_coordinate in [-1, 0] {
            for seed in 0..16 {
                let mut proto =
                    ProtoChunk::empty(ChunkPos::new(chunk_coordinate, chunk_coordinate), 0);
                proto.chunk.fill_subchunk_solid(16, IDS.0); // Y=192..207.
                let input =
                    BlockPos::new(chunk_coordinate * 16 + 4, 204, chunk_coordinate * 16 + 4);
                let mut rng = BedrockRandom::new(seed);
                assert_eq!(
                    place(
                        &mut Ctx {
                            world: &mut proto,
                            rng: &mut rng,
                            noise: &noise,
                            seed: 12345
                        },
                        input,
                        17,
                        0.0,
                        replace_stone,
                        is_air,
                    ),
                    Some(input)
                );
                let mut ore_cells = 0;
                for x in 0..16 {
                    for z in 0..16 {
                        for y in 192..208 {
                            if proto.chunk.get_block(x, y, z) != IDS.1 {
                                continue;
                            }
                            let dx = chunk_coordinate * 16 + i32::from(x) - input.x;
                            let dz = chunk_coordinate * 16 + i32::from(z) - input.z;
                            assert!((5..=10).contains(&dx), "ore x offset {dx}, seed {seed}");
                            assert!((5..=10).contains(&dz), "ore z offset {dz}, seed {seed}");
                            ore_cells += 1;
                        }
                    }
                }
                assert!(ore_cells > 0, "full stone target must produce a vein");
            }
        }
    }
}
