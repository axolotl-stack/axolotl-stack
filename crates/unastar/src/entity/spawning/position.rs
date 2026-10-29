//! Where the spawn cycle may spawn: the spawn block search down a column
//! and the checks on a drawn entry's spawn position.

use bevy_ecs::prelude::World;
use glam::{DVec3, IVec3};
use unastar_data::blocks::{BlockData, StateValue};

use super::rules::SpawnEntry;
use crate::entity::mob::constants::{
    MATERIAL_ICE, MATERIAL_LAVA, MATERIAL_WATER, SPAWN_FULL_TICK_RANGE, SPAWN_OPEN_MATERIALS,
    SPAWN_SHORT_TICK_RANGE, SPAWN_SHORT_TICK_RANGE_DISTANCE,
};
use crate::entity::systems::physics::BlockSource;
use crate::registry::BlockRegistry;
use crate::world::chunk::MIN_Y;
use crate::world::ecs::systems::world_to_chunk_coords;
use crate::world::ecs::{ChunkManager, ChunkStateFlags};

/// Move `pos` down to the next spawn block: the first motion-blocking block
/// below open space that is solid, ice or a top slab. On the surface search
/// a solid block above the open space or a bottom slab ends the search.
pub(super) fn find_spawn_block(
    world: &World,
    blocks: &BlockRegistry,
    pos: &mut IVec3,
    surface: bool,
) -> bool {
    loop {
        if world.block_at(*pos).is_none() {
            return false;
        }
        while pos.y >= MIN_Y {
            let data = block_data(world, blocks, *pos);
            if data.is_none_or(|d| SPAWN_OPEN_MATERIALS.contains(&d.material)) {
                break;
            }
            if surface && data.is_some_and(|d| d.solid) {
                return false;
            }
            pos.y -= 1;
        }
        let mut candidate = None;
        while pos.y >= MIN_Y {
            if let Some((id, data)) = block(world, blocks, *pos)
                && data.blocks_motion
            {
                candidate = Some((id, data));
                break;
            }
            pos.y -= 1;
        }
        let Some((id, data)) = candidate else {
            return false;
        };
        match slab_half(blocks, id) {
            Some(true) => return true,
            Some(false) if surface => return false,
            Some(false) => continue,
            None => {}
        }
        if data.material == MATERIAL_ICE || data.solid {
            return true;
        }
    }
}

/// `Some(true)` for a top slab, `Some(false)` for a bottom slab.
pub(super) fn slab_half(blocks: &BlockRegistry, id: u32) -> Option<bool> {
    match blocks
        .get_by_runtime_id(id)?
        .state_value(id, "minecraft:vertical_half")?
    {
        StateValue::Str(half) => Some(half == "top"),
        _ => None,
    }
}

/// Player distance, spawn blocks, and room at the feet. `tick_range` is the
/// chunk tick range; `players` the positions spawns can happen around.
pub(super) fn spawn_position_ok(
    world: &World,
    blocks: &BlockRegistry,
    entry: &SpawnEntry,
    feet: IVec3,
    tick_range: i32,
    players: &[DVec3],
) -> bool {
    let max = if tick_range >= SPAWN_FULL_TICK_RANGE {
        if !neighbourhood_ticking(world, feet) {
            return false;
        }
        entry.distance.1
    } else if tick_range == SPAWN_SHORT_TICK_RANGE {
        entry.distance.1.min(SPAWN_SHORT_TICK_RANGE_DISTANCE)
    } else {
        return false;
    };
    let max_sq = if max >= 0 {
        f64::from(max * max)
    } else {
        f64::MAX
    };
    let at = feet.as_dvec3();
    let Some(nearest) = players
        .iter()
        .map(|player| player.distance_squared(at))
        .filter(|&distance| distance < max_sq)
        .min_by(f64::total_cmp)
    else {
        return false;
    };
    if nearest < f64::from(entry.distance.0 * entry.distance.0) {
        return false;
    }
    let below = feet - IVec3::Y;
    let Some((ground, _)) = block(world, blocks, below) else {
        return false;
    };
    if !entry.on_blocks.is_empty() && !entry.on_blocks.contains(ground) {
        return false;
    }
    if entry.prevented_blocks.contains(ground) {
        return false;
    }
    if let Some((above, distance)) = &entry.above_blocks {
        let mut found = false;
        for depth in 0..(*distance).max(0) {
            let Some((id, data)) = block(world, blocks, below - IVec3::new(0, depth, 0)) else {
                return false;
            };
            if above.contains(id) {
                found = true;
                break;
            }
            let is_air = blocks
                .get_by_runtime_id(id)
                .is_some_and(|entry| entry.string_id == "minecraft:air");
            if data.material != MATERIAL_WATER && !is_air {
                break;
            }
        }
        if !found {
            return false;
        }
    }
    if solid_blocking(world, blocks, feet + IVec3::Y) {
        return false;
    }
    let feet_material = material(world, blocks, feet);
    if (feet_material == MATERIAL_WATER) != entry.underwater
        || (feet_material == MATERIAL_LAVA) != entry.lava
    {
        return false;
    }
    let ground_name = blocks
        .get_by_runtime_id(ground)
        .map(|entry| entry.string_id.as_str());
    if matches!(
        ground_name,
        Some("minecraft:bedrock" | "minecraft:invisible_bedrock" | "minecraft:barrier")
    ) {
        return false;
    }
    !solid_blocking(world, blocks, feet)
}

/// The chunk of `feet` and the 8 around it are loaded and ticking.
fn neighbourhood_ticking(world: &World, feet: IVec3) -> bool {
    let (cx, cz) = world_to_chunk_coords(feet.x, feet.z);
    let manager = world.resource::<ChunkManager>();
    (-1..=1).all(|dx| {
        (-1..=1).all(|dz| {
            manager
                .get_by_coords(cx + dx, cz + dz)
                .and_then(|entity| world.get::<ChunkStateFlags>(entity))
                .is_some_and(ChunkStateFlags::is_ticking)
        })
    })
}

pub(super) fn block(
    world: &World,
    blocks: &BlockRegistry,
    pos: IVec3,
) -> Option<(u32, &'static BlockData)> {
    let id = world.block_at(pos)?;
    Some((id, blocks.get_by_runtime_id(id)?.data))
}

fn block_data(world: &World, blocks: &BlockRegistry, pos: IVec3) -> Option<&'static BlockData> {
    block(world, blocks, pos).map(|(_, data)| data)
}

/// Material ordinal at `pos` (air when unloaded or unknown).
pub(super) fn material(world: &World, blocks: &BlockRegistry, pos: IVec3) -> u8 {
    block_data(world, blocks, pos).map_or(0, |data| data.material)
}

fn solid_blocking(world: &World, blocks: &BlockRegistry, pos: IVec3) -> bool {
    block_data(world, blocks, pos).is_some_and(|data| data.solid && data.blocks_motion)
}
