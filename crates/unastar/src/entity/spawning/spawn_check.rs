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
//! brightness below 8 at its feet and no block light there. The slime's
//! class runs the plain mob check, but in a superflat world it first passes
//! only when a draw in `0..4` is 1 (1 spawn in 4).
//!
//! Raw brightness is [`crate::world::light::brightness`] (the darkened sky
//! light or the block light, whichever is brighter). The server has no
//! weather (see `world::clock`), so the monster's rain recompute (the sky
//! darkened by 10 while it rains) never applies. Fire immunity comes from
//! `minecraft:fire_immune`, which has no runtime kind: a definition
//! declaring it does not compile, so every spawnable mob takes the
//! non-fire-immune path. Wither immunity is the native class's (the wither
//! skeleton's): `minecraft:mob_effect_immunity` has a runtime kind, but no
//! vanilla definition lists wither in it and the check does not read it.

use bevy_ecs::prelude::World;
use glam::IVec3;
use rand::Rng;

use super::rules::BlockSet;
use crate::entity::mob::constants::{
    ANIMAL_GRASS_WALK_TARGET, ANIMAL_WALK_TARGET_OFFSET, GRASS_BLOCK, MOB_WALK_TARGET,
    MONSTER_SPAWN_BRIGHTNESS_LIMIT, MONSTER_WALK_TARGET_BASE, OVERWORLD_BRIGHTNESS_RAMP,
    SLIME_SUPERFLAT_SPAWN_CHANCE, SPAWN_REFUSING_BLOCKS, SPAWN_UNSAFE_GROUND, SPAWN_WITHER_ROSE,
};
use crate::entity::mob::native::SpawnClass;
use crate::entity::systems::physics::BlockSource;
use crate::registry::BlockRegistry;
use crate::world::WorldGenerator;
use crate::world::ecs::ChunkManager;
use crate::world::light::{block_light, brightness};

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
            grass: BlockSet::named(blocks, &[GRASS_BLOCK]),
            unsafe_ground: BlockSet::named(blocks, &SPAWN_UNSAFE_GROUND),
            refusing: BlockSet::named(blocks, &SPAWN_REFUSING_BLOCKS),
            wither_rose: BlockSet::named(blocks, &[SPAWN_WITHER_ROSE]),
        }
    }

    /// Whether a mob of spawn class `class` with its feet in `feet` passes
    /// its native spawn check (module docs); `immune_to_wither` from its
    /// native traits. The slime's superflat draw comes from `random`.
    pub fn allows(
        &self,
        world: &World,
        class: SpawnClass,
        immune_to_wither: bool,
        feet: IVec3,
        random: &mut impl Rng,
    ) -> bool {
        if class == SpawnClass::Slime
            && superflat(world)
            && random.gen_range(0..SLIME_SUPERFLAT_SPAWN_CHANCE) != 1
        {
            return false;
        }
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
        let raw = brightness(world, feet);
        if class == SpawnClass::Monster
            && (block_light(world, feet) > 0 || raw >= MONSTER_SPAWN_BRIGHTNESS_LIMIT)
        {
            return false;
        }
        self.walk_target(class, ground, raw) >= 0.0
    }

    /// The class's walk-target value at a block with raw brightness `raw`
    /// above the block `ground`.
    fn walk_target(&self, class: SpawnClass, ground: u32, raw: u8) -> f32 {
        walk_target_value(class, self.grass.contains(ground), raw)
    }
}

/// The world is superflat (its generator is [`WorldGenerator::SuperFlat`]).
fn superflat(world: &World) -> bool {
    world
        .get_resource::<ChunkManager>()
        .is_some_and(|manager| manager.world_config().generator == WorldGenerator::SuperFlat)
}

/// A class's native walk-target value at a block with raw brightness
/// `raw`, standing on grass when `on_grass`: natural spawning refuses a
/// negative value, and `behavior.flee_sun` looks for one.
pub(crate) fn walk_target_value(class: SpawnClass, on_grass: bool, raw: u8) -> f32 {
    let ramp = OVERWORLD_BRIGHTNESS_RAMP[usize::from(raw.min(15))];
    match class {
        SpawnClass::Mob | SpawnClass::Slime => MOB_WALK_TARGET,
        SpawnClass::Animal if on_grass => ANIMAL_GRASS_WALK_TARGET,
        SpawnClass::Animal => ramp + ANIMAL_WALK_TARGET_OFFSET,
        SpawnClass::Monster => MONSTER_WALK_TARGET_BASE - ramp,
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
