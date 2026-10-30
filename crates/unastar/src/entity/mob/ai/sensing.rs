//! Line of sight: whether a mob's eye sees a point (the native sensing
//! check behind `must_see` targets and item pickup).

use glam::{DVec3, IVec3};

use crate::entity::systems::physics::{BlockSource, collect_block_boxes};
use crate::registry::collision::Aabb;

/// Whether the segment `from`–`to` passes no block collision box. Blocks
/// are visited along the segment (voxel traversal) and each block's boxes
/// are tested against it; an unloaded block blocks sight.
pub fn can_see(blocks: &impl BlockSource, from: DVec3, to: DVec3) -> bool {
    let delta = to - from;
    let length = delta.length();
    if !length.is_finite() {
        return false;
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
    loop {
        if blocks.block_at(cell).is_none() {
            return false;
        }
        let min = cell.as_dvec3();
        boxes.clear();
        collect_block_boxes(blocks, Aabb::new(min, min + DVec3::ONE), &mut boxes);
        if boxes.iter().any(|b| segment_hits(from, delta, b)) {
            return false;
        }
        if cell == end {
            return true;
        }
        if next.x <= next.y && next.x <= next.z {
            if next.x > 1.0 {
                return true;
            }
            cell.x += step.x;
            next.x += width.x;
        } else if next.y <= next.z {
            if next.y > 1.0 {
                return true;
            }
            cell.y += step.y;
            next.y += width.y;
        } else {
            if next.z > 1.0 {
                return true;
            }
            cell.z += step.z;
            next.z += width.z;
        }
    }
}

/// Whether `from + t·delta`, `t ∈ [0, 1]`, enters `aabb` (slab test).
fn segment_hits(from: DVec3, delta: DVec3, aabb: &Aabb) -> bool {
    let mut enter = 0.0f64;
    let mut exit = 1.0f64;
    for axis in 0..3 {
        let (origin, dir, min, max) = (from[axis], delta[axis], aabb.min[axis], aabb.max[axis]);
        if dir.abs() < 1.0e-12 {
            if origin <= min || origin >= max {
                return false;
            }
            continue;
        }
        let (a, b) = ((min - origin) / dir, (max - origin) / dir);
        let (near, far) = if a < b { (a, b) } else { (b, a) };
        enter = enter.max(near);
        exit = exit.min(far);
        if enter >= exit {
            return false;
        }
    }
    true
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
