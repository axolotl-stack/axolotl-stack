//! Line of sight: whether a mob's eye sees a point (the native sensing
//! check behind `must_see` targets and item pickup), and where a segment
//! first meets a block (kinetic weapon hits, `mob::combat::kinetic`).

use glam::{DVec3, IVec3};

use crate::entity::systems::physics::{BlockSource, collect_block_boxes};
use crate::registry::collision::Aabb;

/// Whether the segment `from`–`to` passes no block collision box. Blocks
/// are visited along the segment (voxel traversal) and each block's boxes
/// are tested against it; an unloaded block blocks sight.
pub fn can_see(blocks: &impl BlockSource, from: DVec3, to: DVec3) -> bool {
    trace(blocks, from, to, true).is_none()
}

/// The fraction of the segment `from`–`to` at which it first enters a block
/// collision box (an unloaded block from where the segment enters it);
/// `None` when it passes none.
pub fn first_block_hit(blocks: &impl BlockSource, from: DVec3, to: DVec3) -> Option<f64> {
    trace(blocks, from, to, true)
}

/// Whether `from` sees the block at `cell`: the segment to the block's
/// centre passes no other block's collision box.
pub fn can_see_block(blocks: &impl BlockSource, from: DVec3, cell: IVec3) -> bool {
    trace(blocks, from, cell.as_dvec3() + DVec3::splat(0.5), false).is_none()
}

/// [`first_block_hit`]; the boxes of the cell `to` lies in are tested only
/// when `test_end`.
fn trace(blocks: &impl BlockSource, from: DVec3, to: DVec3, test_end: bool) -> Option<f64> {
    let delta = to - from;
    let length = delta.length();
    if !length.is_finite() {
        return Some(0.0);
    }
    let mut cell = from.floor().as_ivec3();
    let end = to.floor().as_ivec3();
    let step = IVec3::new(
        delta.x.signum() as i32,
        delta.y.signum() as i32,
        delta.z.signum() as i32,
    );
    // Segment parameter at which the ray crosses the next cell boundary on
    // each axis, and the parameter width of one cell.
    let boundary = |origin: f64, cell: i32, dir: f64| {
        if dir > 0.0 {
            (f64::from(cell + 1) - origin) / dir
        } else if dir < 0.0 {
            (f64::from(cell) - origin) / dir
        } else {
            f64::INFINITY
        }
    };
    let mut next = DVec3::new(
        boundary(from.x, cell.x, delta.x),
        boundary(from.y, cell.y, delta.y),
        boundary(from.z, cell.z, delta.z),
    );
    let width = DVec3::new(
        (1.0 / delta.x).abs(),
        (1.0 / delta.y).abs(),
        (1.0 / delta.z).abs(),
    );
    let mut boxes = Vec::new();
    // Segment parameter at which the ray entered `cell`.
    let mut entered = 0.0;
    loop {
        if blocks.block_at(cell).is_none() {
            return Some(entered);
        }
        if cell == end && !test_end {
            return None;
        }
        let min = cell.as_dvec3();
        boxes.clear();
        collect_block_boxes(blocks, Aabb::new(min, min + DVec3::ONE), &mut boxes);
        if let Some(hit) = boxes
            .iter()
            .filter_map(|b| segment_entry(from, delta, b))
            .min_by(f64::total_cmp)
        {
            return Some(hit);
        }
        if cell == end {
            return None;
        }
        if next.x <= next.y && next.x <= next.z {
            if next.x > 1.0 {
                return None;
            }
            entered = next.x;
            cell.x += step.x;
            next.x += width.x;
        } else if next.y <= next.z {
            if next.y > 1.0 {
                return None;
            }
            entered = next.y;
            cell.y += step.y;
            next.y += width.y;
        } else {
            if next.z > 1.0 {
                return None;
            }
            entered = next.z;
            cell.z += step.z;
            next.z += width.z;
        }
    }
}

/// The smallest `t ∈ [0, 1]` at which `from + t·delta` is inside `aabb`
/// (slab test); `None` when the segment misses it or only grazes it.
pub(crate) fn segment_entry(from: DVec3, delta: DVec3, aabb: &Aabb) -> Option<f64> {
    let mut enter = 0.0f64;
    let mut exit = 1.0f64;
    for axis in 0..3 {
        let (origin, dir, min, max) = (from[axis], delta[axis], aabb.min[axis], aabb.max[axis]);
        if dir.abs() < 1.0e-12 {
            if origin <= min || origin >= max {
                return None;
            }
            continue;
        }
        let (a, b) = ((min - origin) / dir, (max - origin) / dir);
        let (near, far) = if a < b { (a, b) } else { (b, a) };
        enter = enter.max(near);
        exit = exit.min(far);
        if enter >= exit {
            return None;
        }
    }
    Some(enter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::BlockRegistry;

    struct Wall;

    impl BlockSource for Wall {
        fn block_at(&self, pos: IVec3) -> Option<u32> {
            let registry = BlockRegistry::vanilla();
            // Ground below y = 64 and a wall at x = 5.
            let name = if pos.y < 64 || pos.x == 5 {
                "minecraft:stone"
            } else {
                "minecraft:air"
            };
            Some(registry.get_by_name(name).unwrap().default_state_id)
        }

        fn registry(&self) -> &BlockRegistry {
            BlockRegistry::vanilla()
        }
    }

    #[test]
    fn a_wall_blocks_sight_and_open_air_does_not() {
        let eye = DVec3::new(0.5, 65.5, 0.5);
        assert!(!can_see(&Wall, eye, DVec3::new(9.5, 65.5, 0.5)));
        assert!(can_see(&Wall, eye, DVec3::new(4.5, 65.5, 3.5)));
        assert!(can_see(&Wall, eye, DVec3::new(0.5, 65.5, -8.5)));
        // Grazing the floor top from above does not count as a hit.
        assert!(can_see(&Wall, eye, DVec3::new(-3.5, 64.0, 0.5)));
    }
}
