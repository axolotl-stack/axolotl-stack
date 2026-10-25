//! `minecraft:partially_exposed_blob_feature` (underwater magma): converts
//! blocks embedded in the floor into `places_block`.
//!
//! The input is the open cell above the floor (`snap_to_surface`), so the
//! blob is centred on the floor block below it. Every cell of the cube of
//! half-extent `placement_radius_around_floor` around that block is rolled
//! with `placement_probability_per_valid_position` (vanilla rolls before
//! validating, x fastest, then y, then z); a cell is converted when neither it
//! nor any neighbour except the one across `exposed_face` is air or water
//! (Mojang documents `exposed_face` as the one face that may be exposed to
//! air and/or water).

use super::super::generated::bb;
use super::super::world::{BlockPos, BlockWorld, Ctx};
use super::multiface::step;

/// Parameters of one `minecraft:partially_exposed_blob_feature`.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// `places_block`.
    pub block: u32,
    /// `placement_radius_around_floor`.
    pub radius: i32,
    /// `placement_probability_per_valid_position`.
    pub probability: f32,
    /// `exposed_face`, as a face index of [`super::multiface::FACES`].
    pub exposed_face: usize,
}

#[inline]
fn is_open(id: u32) -> bool {
    bb::air(id) || bb::water(id)
}

/// Whether `at` is solid and enclosed on every face but `exposed`.
fn is_embedded<W: BlockWorld>(ctx: &Ctx<'_, W>, at: BlockPos, exposed: usize) -> bool {
    !is_open(ctx.get(at)) && (0..6).all(|face| face == exposed || !is_open(ctx.get(step(at, face))))
}

/// Place one blob under `pos`; `Some(pos)` when any block was converted.
pub fn place<W: BlockWorld>(ctx: &mut Ctx<'_, W>, pos: BlockPos, p: &Params) -> Option<BlockPos> {
    let floor = pos.below();
    let r = p.radius;
    let mut placed = false;
    for z in -r..=r {
        for y in -r..=r {
            for x in -r..=r {
                let at = floor.offset(x, y, z);
                if ctx.rng.next_float() < p.probability && is_embedded(ctx, at, p.exposed_face) {
                    placed |= ctx.set(at, p.block);
                }
            }
        }
    }
    placed.then_some(pos)
}

#[cfg(test)]
mod tests {
    use super::super::super::rng::BedrockRandom;
    use super::super::multiface::UP;
    use super::super::multiface::test_world::{Cube, with_ctx};
    use super::*;

    #[test]
    fn blob_converts_only_embedded_floor_blocks() {
        // Stone below y = 0, water above; the input is the water cell on the floor.
        let mut world = Cube::filled(8, bb::WATER);
        world.fill((-8, -8, -8), (7, -1, 7), bb::STONE);
        let p = Params {
            block: bb::MAGMA,
            radius: 1,
            probability: 1.0,
            exposed_face: UP,
        };
        let mut rng = BedrockRandom::new(4);
        let placed = with_ctx(&mut world, &mut rng, |ctx| {
            place(ctx, BlockPos::new(0, 0, 0), &p)
        });
        assert_eq!(placed, Some(BlockPos::new(0, 0, 0)));
        // The 3x3x3 cube round the floor block (0, -1, 0): its two stone
        // layers convert, the water layer does not.
        let magma = world.positions(|id| id == bb::MAGMA);
        assert_eq!(magma.len(), 18);
        assert!(
            magma
                .iter()
                .all(|at| (-2..=-1).contains(&at.y) && at.x.abs() <= 1 && at.z.abs() <= 1)
        );
    }

    #[test]
    fn side_exposure_prevents_placement() {
        // A one-block trench: the floor block's west side is open water.
        let mut world = Cube::filled(8, bb::WATER);
        world.fill((-8, -8, -8), (7, -1, 7), bb::STONE);
        world.set(BlockPos::new(-1, -1, 0), bb::WATER);
        let p = Params {
            block: bb::MAGMA,
            radius: 0,
            probability: 1.0,
            exposed_face: UP,
        };
        let mut rng = BedrockRandom::new(4);
        assert_eq!(
            with_ctx(&mut world, &mut rng, |ctx| place(
                ctx,
                BlockPos::new(0, 0, 0),
                &p
            )),
            None
        );
        assert_eq!(world.count(bb::MAGMA), 0);
    }
}
