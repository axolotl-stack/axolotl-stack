//! Automatic step height follows the current definition and ground block.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_flags;

use super::MobKind;
use super::components::movement::VariableMaxAutoStep;
use crate::entity::components::{PhysicsBody, Position};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::systems::physics::{BlockSource, WorldBlocks};

/// A honey support block prevents an ordinary ground jump. Unloaded ground
/// is conservative; it must not permit a tall automatic step into that area.
fn jump_prevented(blocks: &impl BlockSource, feet: DVec3) -> bool {
    let support = (feet - DVec3::Y * 0.001).floor().as_ivec3();
    blocks.block_at(support).is_none_or(|state| {
        blocks
            .registry()
            .get_by_runtime_id(state)
            .is_some_and(|block| block.name == "minecraft:honey_block")
    })
}

/// Refresh the collision/pathfinding step height before AI and physics.
/// Removing the authored component restores the ordinary living height.
pub fn update_step_heights(
    blocks: WorldBlocks,
    mut mobs: Query<
        (
            &Position,
            Option<&VariableMaxAutoStep>,
            &ActorFlags,
            &mut PhysicsBody,
        ),
        With<MobKind>,
    >,
) {
    for (position, config, flags, mut body) in &mut mobs {
        let height = config.map_or(0.5625, |config| {
            if jump_prevented(&blocks, position.0) {
                config.jump_prevented_value
            } else if flags.get(actor_flags::WASD_CONTROLLED).unwrap_or(false) {
                config.controlled_value
            } else {
                config.base_value
            }
        });
        if body.step_height != height {
            body.step_height = height;
        }
    }
}
