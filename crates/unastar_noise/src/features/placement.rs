//! Helpers for placement modifiers that are too long to inline.

use super::blocks::{BlockPos, JavaState};
use super::eval::FeatureWorld;
use super::java_blocks::blocks as b;

/// `CountOnEveryLayerPlacement.isEmpty`: air, water or lava.
#[inline]
fn layer_empty(s: JavaState) -> bool {
    s.is_air() || s.is(b::WATER) || s.is(b::LAVA)
}

/// `CountOnEveryLayerPlacement.findOnGroundYPosition`: scanning down from
/// `top`, the Y above the `layer`-th (0-based) transition from an empty block
/// to a non-empty, non-bedrock block below it.
pub fn on_ground_y<W: FeatureWorld + ?Sized>(
    world: &mut W,
    x: i32,
    top: i32,
    z: i32,
    layer: i32,
) -> Option<i32> {
    let mut seen = 0;
    let mut above = world.get_state(BlockPos::new(x, top, z));
    let mut y = top;
    while y > world.min_y() {
        let below = world.get_state(BlockPos::new(x, y - 1, z));
        if !layer_empty(below) && layer_empty(above) && !below.is(b::BEDROCK) {
            if seen == layer {
                return Some(y);
            }
            seen += 1;
        }
        above = below;
        y -= 1;
    }
    None
}
