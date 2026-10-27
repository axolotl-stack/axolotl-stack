//! `BlockState.canSurvive(level, pos)` for the blocks Java decoration places
//! or tests with `would_survive`.
//!
//! Rules are keyed by the block type from the Java block report (`kind`), with
//! per-block overrides where one type covers blocks with different supports.
//! Two inputs are approximated because the light engine does not run during
//! decoration in unastar: mushrooms treat positions at or above the
//! `WORLD_SURFACE` heightmap as fully sky-lit (brightness 15) and everything
//! below as dark; `spore_blossom` and other "center support" checks use the
//! sturdy-face flag of the supporting block.

use super::blocks::{BlockPos, Direction, FluidKind, Heightmap, JavaState};
use super::eval::FeatureWorld;
use super::java_blocks::{blocks as b, tags};

/// `VegetationBlock.mayPlaceOn` default: dirt-tagged blocks and farmland.
#[inline]
fn vegetation_ground(s: JavaState) -> bool {
    s.is_in(tags::DIRT) || s.is(b::FARMLAND)
}

/// Lower half / upper half of a double block (`half` = `lower` / `upper`).
#[inline]
fn is_upper(s: JavaState) -> bool {
    s.value("half") == Some("upper")
}

/// `canSurvive` for `state` placed at `pos`.
pub fn can_survive<W: FeatureWorld + ?Sized>(
    world: &mut W,
    state: JavaState,
    pos: BlockPos,
) -> bool {
    let block = state.block();
    let below_pos = pos.below();
    match block.def().kind {
        "minecraft:flower"
        | "minecraft:tall_grass"
        | "minecraft:bush"
        | "minecraft:sapling"
        | "minecraft:sweet_berry_bush"
        | "minecraft:flower_bed"
        | "minecraft:firefly_bush"
        | "minecraft:eyeblossom" => vegetation_ground(world.get_state(below_pos)),
        "minecraft:wither_rose" => {
            let below = world.get_state(below_pos);
            vegetation_ground(below)
                || below.is(b::NETHERRACK)
                || below.is(b::SOUL_SAND)
                || below.is(b::SOUL_SOIL)
        }
        "minecraft:crop"
        | "minecraft:beetroot"
        | "minecraft:carrot"
        | "minecraft:potato"
        | "minecraft:torchflower_crop"
        | "minecraft:pitcher_crop"
        | "minecraft:stem"
        | "minecraft:attached_stem" => world.get_state(below_pos).is(b::FARMLAND),
        "minecraft:double_plant" | "minecraft:tall_flower" => {
            if is_upper(state) {
                let below = world.get_state(below_pos);
                below.is(block) && !is_upper(below)
            } else {
                vegetation_ground(world.get_state(below_pos))
            }
        }
        "minecraft:dry_vegetation" | "minecraft:short_dry_grass" | "minecraft:tall_dry_grass" => {
            world
                .get_state(below_pos)
                .is_in(tags::DRY_VEGETATION_MAY_PLACE_ON)
        }
        "minecraft:leaf_litter" => world.get_state(below_pos).is_face_sturdy(Direction::Up),
        "minecraft:azalea" => {
            let below = world.get_state(below_pos);
            below.is(b::CLAY) || vegetation_ground(below)
        }
        "minecraft:mangrove_propagule" => {
            if state.value("hanging") == Some("true") {
                world.get_state(pos.above()).is(b::MANGROVE_LEAVES)
            } else {
                let below = world.get_state(below_pos);
                below.is(b::CLAY) || vegetation_ground(below)
            }
        }
        "minecraft:fungus" => {
            let below = world.get_state(below_pos);
            below.is_in(tags::NYLIUM)
                || below.is(b::MYCELIUM)
                || below.is(b::SOUL_SOIL)
                || vegetation_ground(below)
        }
        "minecraft:roots" | "minecraft:nether_sprouts" => {
            let below = world.get_state(below_pos);
            below.is_in(tags::NYLIUM) || below.is(b::SOUL_SOIL) || vegetation_ground(below)
        }
        "minecraft:mushroom" => {
            let below = world.get_state(below_pos);
            if below.is_in(tags::MUSHROOM_GROW_BLOCK) {
                return true;
            }
            let sky_lit = world.has_skylight()
                && pos.y >= world.get_height(Heightmap::WorldSurface, pos.x, pos.z);
            !sky_lit && below.is_collision_full_cube() && below.is_solid()
        }
        "minecraft:waterlily" => {
            let below = world.get_state(below_pos);
            let below_fluid = below.fluid();
            let water_source = below_fluid.kind == FluidKind::Water && below_fluid.source;
            (water_source || below.is(b::ICE) || below.is(b::FROSTED_ICE))
                && world.get_state(pos.above()).fluid().is_empty()
        }
        "minecraft:sugar_cane" => {
            let below = world.get_state(below_pos);
            if below.is(b::SUGAR_CANE) {
                return true;
            }
            if below.is_in(tags::DIRT) || below.is_in(tags::SAND) {
                for dir in Direction::HORIZONTAL {
                    let n = world.get_state(below_pos.rel(dir));
                    if n.fluid().is_water() || n.is(b::FROSTED_ICE) {
                        return true;
                    }
                }
            }
            false
        }
        "minecraft:cactus" => {
            for dir in Direction::HORIZONTAL {
                let n = world.get_state(pos.rel(dir));
                if n.is_solid() || n.fluid().is_lava() {
                    return false;
                }
            }
            let below = world.get_state(below_pos);
            (below.is(b::CACTUS) || below.is_in(tags::SAND))
                && world.get_state(pos.above()).fluid().is_empty()
        }
        "minecraft:cactus_flower" => {
            let below = world.get_state(below_pos);
            below.is(b::CACTUS) || below.is(b::FARMLAND) || below.is_face_sturdy(Direction::Up)
        }
        "minecraft:bamboo_sapling" | "minecraft:bamboo_stalk" => {
            world.get_state(below_pos).is_in(tags::BAMBOO_PLANTABLE_ON)
        }
        "minecraft:snow_layer" => {
            let below = world.get_state(below_pos);
            if below.is_in(tags::SNOW_LAYER_CANNOT_SURVIVE_ON) {
                false
            } else if below.is_in(tags::SNOW_LAYER_CAN_SURVIVE_ON) {
                true
            } else {
                below.has_full_top_collision_face()
                    || (below.is(b::SNOW) && below.int_value("layers") == Some(8))
            }
        }
        "minecraft:amethyst_cluster" => {
            let facing = facing_of(state).unwrap_or(Direction::Up);
            let attached = pos.rel(facing.opposite());
            world.get_state(attached).is_face_sturdy(facing)
        }
        "minecraft:big_dripleaf" => {
            let below = world.get_state(below_pos);
            below.is(b::BIG_DRIPLEAF)
                || below.is(b::BIG_DRIPLEAF_STEM)
                || below.is_in(tags::BIG_DRIPLEAF_PLACEABLE)
        }
        "minecraft:big_dripleaf_stem" => {
            let below = world.get_state(below_pos);
            let above = world.get_state(pos.above());
            (below.is(b::BIG_DRIPLEAF_STEM) || below.is_in(tags::BIG_DRIPLEAF_PLACEABLE))
                && (above.is(b::BIG_DRIPLEAF_STEM) || above.is(b::BIG_DRIPLEAF))
        }
        "minecraft:small_dripleaf" => {
            let below = world.get_state(below_pos);
            if is_upper(state) {
                below.is(b::SMALL_DRIPLEAF) && !is_upper(below)
            } else {
                let fluid = world.get_state(pos).fluid();
                below.is_in(tags::SMALL_DRIPLEAF_PLACEABLE)
                    || (fluid.kind == FluidKind::Water && fluid.source && vegetation_ground(below))
            }
        }
        "minecraft:cave_vines" | "minecraft:cave_vines_plant" => {
            let above = world.get_state(pos.above());
            above.is(b::CAVE_VINES)
                || above.is(b::CAVE_VINES_PLANT)
                || above.is_face_sturdy(Direction::Down)
        }
        "minecraft:weeping_vines" | "minecraft:weeping_vines_plant" => {
            let above = world.get_state(pos.above());
            above.is(b::WEEPING_VINES)
                || above.is(b::WEEPING_VINES_PLANT)
                || above.is_face_sturdy(Direction::Down)
        }
        "minecraft:twisting_vines" | "minecraft:twisting_vines_plant" => {
            let below = world.get_state(below_pos);
            below.is(b::TWISTING_VINES)
                || below.is(b::TWISTING_VINES_PLANT)
                || below.is_face_sturdy(Direction::Up)
        }
        "minecraft:kelp" | "minecraft:kelp_plant" => {
            let below = world.get_state(below_pos);
            !below.is(b::MAGMA_BLOCK)
                && (below.is(b::KELP)
                    || below.is(b::KELP_PLANT)
                    || below.is_face_sturdy(Direction::Up))
        }
        "minecraft:seagrass" => {
            let below = world.get_state(below_pos);
            below.is_face_sturdy(Direction::Up) && !below.is(b::MAGMA_BLOCK)
        }
        "minecraft:tall_seagrass" => {
            let below = world.get_state(below_pos);
            if is_upper(state) {
                below.is(b::TALL_SEAGRASS) && !is_upper(below)
            } else {
                below.is_face_sturdy(Direction::Up) && !below.is(b::MAGMA_BLOCK)
            }
        }
        "minecraft:sea_pickle" => {
            let below = world.get_state(below_pos);
            below.is_face_sturdy(Direction::Up) || below.is_solid()
        }
        "minecraft:spore_blossom" => {
            world.get_state(pos.above()).is_face_sturdy(Direction::Down)
                && !world.get_state(pos).fluid().is_water()
        }
        "minecraft:hanging_roots" => world.get_state(pos.above()).is_face_sturdy(Direction::Down),
        "minecraft:carpet" | "minecraft:wool_carpet" => !world.get_state(below_pos).is_air(),
        "minecraft:mossy_carpet" => {
            let below = world.get_state(below_pos);
            if state.value("bottom") == Some("true") {
                !below.is_air()
            } else {
                below.is(block) && below.value("bottom") == Some("true")
            }
        }
        "minecraft:fire" => world.get_state(below_pos).is_face_sturdy(Direction::Up),
        "minecraft:soul_fire" => world
            .get_state(below_pos)
            .is_in(tags::SOUL_FIRE_BASE_BLOCKS),
        "minecraft:coral_plant"
        | "minecraft:base_coral_plant"
        | "minecraft:coral_fan"
        | "minecraft:base_coral_fan" => world.get_state(below_pos).is_face_sturdy(Direction::Up),
        "minecraft:coral_wall_fan" | "minecraft:base_coral_wall_fan" => {
            let facing = facing_of(state).unwrap_or(Direction::North);
            let attached = pos.rel(facing.opposite());
            world.get_state(attached).is_face_sturdy(facing)
        }
        _ => true,
    }
}

/// `facing` property as a [`Direction`].
pub fn facing_of(state: JavaState) -> Option<Direction> {
    Some(match state.value("facing")? {
        "down" => Direction::Down,
        "up" => Direction::Up,
        "north" => Direction::North,
        "south" => Direction::South,
        "west" => Direction::West,
        "east" => Direction::East,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::{java_blocks::states, test_world::TestWorld};

    #[test]
    fn dimension_light_controls_mushrooms_above_the_noise_ceiling() {
        // Native Nether holdout captures retain mushrooms on the bedrock roof:
        // y=128 is valid storage and has no sky light, despite 128 noise blocks.
        let pos = BlockPos::new(0, 128, 0);
        let mut world = TestWorld {
            min_y: 0, height: 256, gen_depth: 128, fill_y: 128,
            fill: states::BEDROCK, has_skylight: false, ..Default::default()
        };
        assert!(can_survive(&mut world, states::BROWN_MUSHROOM, pos));
        world.has_skylight = true;
        assert!(!can_survive(&mut world, states::BROWN_MUSHROOM, pos));
        world.blocks.insert(pos.below(), states::MYCELIUM);
        assert!(can_survive(&mut world, states::BROWN_MUSHROOM, pos));
    }
}
