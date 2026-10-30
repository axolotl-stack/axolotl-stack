//! Gravity blocks' block-side rules. Falling itself lives in
//! `server::game::falling` (sand, gravel, concrete powder and the other
//! blocks with the native `falling` flag turn into falling-block actors two
//! ticks after a change and re-place themselves through the change path).
//!
//! Concrete powder hardens into the concrete of its colour when water
//! touches it: on placement, on landing and whenever a neighbour changes,
//! water in the cell itself or on any side but below hardens it
//! (provisional: Minecraft Wiki). Liquids themselves belong to an outside
//! contributor; this module only reads whether a cell holds water
//! ([`touches_water`]).

use bevy_ecs::prelude::*;
use glam::IVec3;

use crate::entity::systems::BlockSource;
use crate::registry::block::LiquidKind;
use crate::server::game::types::BlockRegistryResource;

/// Whether the cell at `pos` holds water, as a water block or as water held
/// by a waterlogged block.
pub fn holds_water(world: &World, pos: IVec3) -> bool {
    let registry = &world.resource::<BlockRegistryResource>().0;
    let water = |state: u32| {
        registry
            .liquid_state(state)
            .is_some_and(|liquid| liquid.kind == LiquidKind::Water)
    };
    world.block_at(pos).is_some_and(water) || waterlogged(world, pos).is_some_and(water)
}

/// The liquid layer of the cell at `pos` (`None` when unloaded).
fn waterlogged(world: &World, pos: IVec3) -> Option<u32> {
    let (cx, cz) = crate::world::ecs::world_to_chunk_coords(pos.x, pos.z);
    let chunk = world
        .get_resource::<crate::world::ecs::ChunkManager>()?
        .get_by_coords(cx, cz)?;
    let (x, y, z) = crate::world::ecs::world_to_local_coords(pos.x, pos.y, pos.z);
    Some(
        world
            .get::<crate::world::ecs::ChunkData>(chunk)?
            .inner
            .get_block_layer(x, y, z, 1),
    )
}

/// Whether water touches the block at `pos` on any side but below.
pub fn touches_water(world: &World, pos: IVec3) -> bool {
    super::NEIGHBOURS
        .iter()
        .filter(|offset| **offset != IVec3::NEG_Y)
        .any(|offset| holds_water(world, pos + *offset))
}

/// Concrete powder `state` at `pos` touching water becomes concrete.
pub fn harden_concrete_powder(world: &mut World, pos: IVec3, state: u32) {
    if !touches_water(world, pos) {
        return;
    }
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let Some(concrete) = registry
        .get_by_runtime_id(state)
        .and_then(|entry| entry.string_id.strip_suffix("_powder"))
        .and_then(|name| registry.default_state_id_by_name(name))
    else {
        return;
    };
    crate::server::game::blocks::set_block(world, pos, concrete, false);
}

#[cfg(test)]
mod tests {
    use glam::IVec3;

    use super::super::tests::{FLOOR, Harness, state};

    const POWDER: IVec3 = IVec3::new(2, FLOOR + 1, 2);

    #[test]
    fn concrete_powder_hardens_when_water_touches_a_side_or_the_top() {
        for side in [IVec3::X, IVec3::NEG_Z, IVec3::Y] {
            let mut h = Harness::new();
            h.put(POWDER + side, state("minecraft:water", &[]));
            h.set(POWDER, state("minecraft:red_concrete_powder", &[]));
            assert_eq!(h.name(POWDER), "minecraft:red_concrete", "water at {side}");
        }
    }

    #[test]
    fn concrete_powder_stays_dry_over_water_and_hardens_when_water_arrives() {
        let mut h = Harness::new();
        h.put(POWDER - IVec3::Y, state("minecraft:water", &[]));
        h.set(POWDER, state("minecraft:blue_concrete_powder", &[]));
        assert_eq!(h.name(POWDER), "minecraft:blue_concrete_powder");
        h.set(POWDER + IVec3::Z, state("minecraft:water", &[]));
        assert_eq!(h.name(POWDER), "minecraft:blue_concrete");
    }
}
