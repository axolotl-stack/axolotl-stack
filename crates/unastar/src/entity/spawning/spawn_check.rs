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
//! brightness below 8 at its feet and no block light there.
//!
//! Raw brightness is [`crate::world::clock::sky_brightness`]. Block light
//! is not modelled, so the monster's block-light test always passes. The
//! server has no weather (see `world::clock`), so the monster's rain
//! recompute (the sky darkened by 10 while it rains) never applies. Fire
//! and effect immunity come from `minecraft:fire_immune` and
//! `minecraft:mob_effect_immunity`, which have no runtime kind: a definition
//! declaring them does not compile, so every spawnable mob takes the
//! non-immune path; the wither skeleton's native class is immune to wither.

use bevy_ecs::prelude::World;
use glam::IVec3;

use super::rules::BlockSet;
use crate::entity::mob::constants::{
    ANIMAL_GRASS_WALK_TARGET, ANIMAL_WALK_TARGET_OFFSET, MOB_WALK_TARGET,
    MONSTER_SPAWN_BRIGHTNESS_LIMIT, MONSTER_WALK_TARGET_BASE, OVERWORLD_BRIGHTNESS_RAMP,
    SPAWN_REFUSING_BLOCKS, SPAWN_UNSAFE_GROUND, SPAWN_WITHER_ROSE,
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
        let raw = sky_brightness(world, feet);
        if class == SpawnClass::Monster && raw >= MONSTER_SPAWN_BRIGHTNESS_LIMIT {
            return false;
        }
        self.walk_target(class, ground, raw) >= 0.0
    }

    /// The class's walk-target value at a block with raw brightness `raw`
    /// above the block `ground`.
    fn walk_target(&self, class: SpawnClass, ground: u32, raw: u8) -> f32 {
        let ramp = OVERWORLD_BRIGHTNESS_RAMP[usize::from(raw.min(15))];
        match class {
            SpawnClass::Mob => MOB_WALK_TARGET,
            SpawnClass::Animal if self.grass.contains(ground) => ANIMAL_GRASS_WALK_TARGET,
            SpawnClass::Animal => ramp + ANIMAL_WALK_TARGET_OFFSET,
            SpawnClass::Monster => MONSTER_WALK_TARGET_BASE - ramp,
        }
    }
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
