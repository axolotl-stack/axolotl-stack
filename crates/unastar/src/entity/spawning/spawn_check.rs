//! The check a mob's native actor class runs when the natural cycle spawns
//! it, after the spawn rule's conditions and the spawn position passed.
//!
//! Every class refuses a mob whose feet are in a block that refuses spawns
//! (rails, buttons, pressure plates, redstone components, campfires, and a
//! wither rose unless the mob is immune to wither), that stands on magma, a
//! campfire or a soul campfire, or whose walk-target value at its feet is
//! negative. The walk-target value depends on the class
//! ([`SpawnClass`], a native trait of the archetype): 0 for a plain mob;
//! for an animal 10 with a grass block
//! below, elsewhere the brightness ramp at the feet minus 0.5 (raw
//! brightness 12 or more passes); for a monster 0.5 minus the ramp (raw
//! brightness 11 or less passes). A monster must also be dark enough: raw
//! brightness below 8 at its feet and no block light there. A slime spawns
//! only in a slime chunk (`super::slime_chunk`), below
//! `SLIME_CHUNK_MAX_FEET_Y`; its swamp-surface spawning is not modelled.
//!
//! Raw brightness is [`crate::world::clock::sky_brightness`]. Block light
//! is not modelled, so the monster's block-light test always passes. The
//! server has no weather (see `world::clock`), so the monster's rain
//! recompute (the sky darkened by 10 while it rains) never applies. The
//! spawn check refuses magma ground before it reads the walk-target value,
//! so only [`walk_target_value`] (goals ranking blocks, such as
//! `behavior.flee_sun`) reaches the class rules' magma value (-0.5 above
//! magma for a mob that is not fire immune). The wither skeleton's native
//! class is immune to wither.

use bevy_ecs::prelude::World;
use glam::IVec3;

use super::rules::BlockSet;
use super::slime_chunk::is_slime_chunk;
use crate::entity::mob::constants::{
    ANIMAL_GRASS_WALK_TARGET, ANIMAL_WALK_TARGET_OFFSET, MAGMA_BLOCK, MAGMA_WALK_TARGET,
    MOB_WALK_TARGET, MONSTER_SPAWN_BRIGHTNESS_LIMIT, MONSTER_WALK_TARGET_BASE,
    OVERWORLD_BRIGHTNESS_RAMP, SLIME_CHUNK_MAX_FEET_Y, SPAWN_REFUSING_BLOCKS, SPAWN_UNSAFE_GROUND,
    SPAWN_WITHER_ROSE,
};
use crate::entity::mob::native::SpawnClass;
use crate::entity::systems::physics::BlockSource;
use crate::registry::BlockRegistry;
use crate::world::clock::sky_brightness;

/// The blocks the native spawn checks test, resolved against the block
/// registry once.
#[derive(Debug, Clone, Default)]
pub struct NativeSpawnBlocks {
    grass: BlockSet,
    unsafe_ground: BlockSet,
    refusing: BlockSet,
    wither_rose: BlockSet,
}

impl NativeSpawnBlocks {
    pub fn resolve(blocks: &BlockRegistry) -> Self {
        Self {
            grass: BlockSet::named(blocks, &["minecraft:grass_block"]),
            unsafe_ground: BlockSet::named(blocks, &SPAWN_UNSAFE_GROUND),
            refusing: BlockSet::named(blocks, &SPAWN_REFUSING_BLOCKS),
            wither_rose: BlockSet::named(blocks, &[SPAWN_WITHER_ROSE]),
        }
    }

    /// Whether a mob of spawn class `class` with its feet in `feet` passes
    /// its native spawn check (module docs); `immune_to_wither` from its
    /// native traits.
    pub fn allows(
        &self,
        world: &World,
        class: SpawnClass,
        immune_to_wither: bool,
        feet: IVec3,
    ) -> bool {
        let (Some(inside), Some(ground)) = (world.block_at(feet), world.block_at(feet - IVec3::Y))
        else {
            return false;
        };
        if self.unsafe_ground.contains(ground) || self.refusing.contains(inside) {
            return false;
        }
        if self.wither_rose.contains(inside) && !immune_to_wither {
            return false;
        }
        if class == SpawnClass::Slime
            && !(feet.y < SLIME_CHUNK_MAX_FEET_Y
                && is_slime_chunk(feet.x.div_euclid(16), feet.z.div_euclid(16)))
        {
            return false;
        }
        let raw = sky_brightness(world, feet);
        if class == SpawnClass::Monster && raw >= MONSTER_SPAWN_BRIGHTNESS_LIMIT {
            return false;
        }
        self.walk_target(class, ground, raw) >= 0.0
    }

    /// The class's walk-target value at a block with raw brightness `raw`
    /// above the block `ground` (never magma here: it refused the spawn).
    fn walk_target(&self, class: SpawnClass, ground: u32, raw: u8) -> f32 {
        class_walk_target(class, self.grass.contains(ground), false, raw)
    }
}

/// The native walk-target value of a class at a block with raw brightness
/// `raw`, `grass` when a grass block is below it and `magma` when magma is
/// below it and the mob is not fire immune.
fn class_walk_target(class: SpawnClass, grass: bool, magma: bool, raw: u8) -> f32 {
    let ramp = OVERWORLD_BRIGHTNESS_RAMP[usize::from(raw.min(15))];
    match class {
        SpawnClass::Mob | SpawnClass::Slime => MOB_WALK_TARGET,
        SpawnClass::Animal if grass => ANIMAL_GRASS_WALK_TARGET,
        SpawnClass::Animal | SpawnClass::Monster if magma => MAGMA_WALK_TARGET,
        SpawnClass::Animal => ramp + ANIMAL_WALK_TARGET_OFFSET,
        SpawnClass::Monster => MONSTER_WALK_TARGET_BASE - ramp,
    }
}

/// The native walk-target value of a mob of class `class` at the block
/// `pos`, for goals that rank blocks by it (`behavior.flee_sun`).
pub fn walk_target_value(world: &World, class: SpawnClass, fire_immune: bool, pos: IVec3) -> f32 {
    let below = world.block_at(pos - IVec3::Y);
    let is = |name: &str| {
        below.is_some_and(|id| BlockSet::named(world.registry(), &[name]).contains(id))
    };
    class_walk_target(
        class,
        is("minecraft:grass_block"),
        !fire_immune && is(MAGMA_BLOCK),
        sky_brightness(world, pos),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_targets_split_the_brightness_ramp_at_twelve() {
        let blocks = NativeSpawnBlocks::default();
        for raw in 0..=15u8 {
            let monster = blocks.walk_target(SpawnClass::Monster, 0, raw) >= 0.0;
            let animal = blocks.walk_target(SpawnClass::Animal, 0, raw) >= 0.0;
            assert_eq!(monster, raw <= 11, "monster at raw {raw}");
            assert_eq!(animal, raw >= 12, "animal at raw {raw}");
            assert!(blocks.walk_target(SpawnClass::Mob, 0, raw) >= 0.0);
        }
    }
}
