//! `minecraft:fossil`: a random fossil template sunk below the ocean floor,
//! placed with its processors, then the matching ore overlay template.

use crate::features::config::FossilConfig;
use crate::features::java_blocks::blocks as b;
use crate::features::{BlockPos, FeatureRandom, FeatureWorld, Heightmap};
use crate::structures::template::{
    PlaceSettings, place_in_world, rotated_size, template_box, zero_position,
};
use crate::structures::template_data::template;
use crate::structures::transform::{BoundingBox, Mirror, Rotation};

fn lookup(name: &str) -> &'static crate::structures::template_data::Template {
    template(name)
        .unwrap_or_else(|| panic!("fossil template {name} is missing from the embedded table"))
}

/// `FossilFeature.place`.
///
/// Block entities in fossil templates (none in vanilla) would be reported to
/// nothing: fossils are plain blocks.
pub fn place<W: FeatureWorld + ?Sized>(
    world: &mut W,
    rng: &mut FeatureRandom,
    cfg: &FossilConfig,
    origin: BlockPos,
) -> bool {
    let rotation = Rotation::random(rng);
    let i = rng.next_int_bounded(cfg.fossil_structures.len() as i32) as usize;
    let fossil = lookup(cfg.fossil_structures[i]);
    let overlay = lookup(cfg.overlay_structures[i]);
    let cx = origin.x >> 4;
    let cz = origin.z >> 4;
    let bbox = BoundingBox::new(
        cx * 16 - 16,
        world.min_y(),
        cz * 16 - 16,
        cx * 16 + 15 + 16,
        world.max_y() - 1,
        cz * 16 + 15 + 16,
    );
    let size = rotated_size(fossil, rotation);
    let corner = origin.offset(-size[0] / 2, 0, -size[2] / 2);
    let mut y = origin.y;
    for k in 0..size[0] {
        for l in 0..size[2] {
            y = y.min(world.get_height(Heightmap::OceanFloorWg, corner.x + k, corner.z + l));
        }
    }
    let base_y = (y - 15 - rng.next_int_bounded(10)).max(world.min_y() + 10);
    let start = zero_position(fossil, corner.with_y(base_y), Mirror::None, rotation);
    let pivot = BlockPos::new(0, 0, 0);
    let bx = template_box(fossil, start, rotation, pivot, Mirror::None);
    let mut empty = 0;
    for x in [bx.min_x, bx.max_x] {
        for yy in [bx.min_y, bx.max_y] {
            for z in [bx.min_z, bx.max_z] {
                let s = world.get_state(BlockPos::new(x, yy, z));
                if s.is_air() || s.is(b::LAVA) || s.is(b::WATER) {
                    empty += 1;
                }
            }
        }
    }
    if empty > cfg.max_empty_corners_allowed {
        return false;
    }
    for (t, processors) in [
        (fossil, cfg.fossil_processors),
        (overlay, cfg.overlay_processors),
    ] {
        let mut settings = PlaceSettings::new(t);
        settings.rotation = rotation;
        settings.bounding_box = Some(bbox);
        settings.shared_random = true;
        settings.processors = processors.iter().collect();
        place_in_world(world, start, start, &mut settings, rng, &mut |_| {});
    }
    true
}
