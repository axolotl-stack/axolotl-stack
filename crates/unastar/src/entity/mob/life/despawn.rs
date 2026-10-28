//! Natural despawning (`minecraft:despawn` with `despawn_from_distance`;
//! the component is `mob::components::despawn`).
//!
//! Despawning is only considered while at least one player is in the world,
//! and never for [`Persistent`] or dying mobs. Measured against the nearest
//! player:
//!
//! - beyond `max_distance` (capped at
//!   [`DESPAWN_SHORT_TICK_RANGE_DISTANCE`] when the chunk tick range is
//!   below [`DESPAWN_SHORT_TICK_RANGE`]) the mob despawns at once;
//! - beyond `min_distance` it despawns at random: once it has been inactive
//!   (no player within `min_distance`) for `min_range_inactivity_timer`
//!   seconds, each tick rolls 1 in `min_range_random_chance`. A player within
//!   `min_distance` resets the inactivity time.
//!
//! Unloading a chunk does not despawn its mobs; they are saved with it.

use bevy_ecs::prelude::*;
use rand::Rng;

use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{Player, Position};
use crate::entity::mob::MobKind;
use crate::entity::mob::components::DespawnConfig;
use crate::entity::mob::constants::{
    DESPAWN_SHORT_TICK_RANGE, DESPAWN_SHORT_TICK_RANGE_DISTANCE, TICKS_PER_SECOND,
};
use crate::entity::mob::state::{Dead, Persistent};
use crate::world::ecs::ChunkLoadConfig;

/// The last server tick a player was within the mob's despawn
/// `min_distance`; the mob has been inactive since. Inserted and removed
/// with [`DespawnConfig`].
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Inactivity {
    pub last_near_tick: u64,
}

impl Inactivity {
    /// Inactive from the current tick on.
    pub fn starting_now(world: &World) -> Self {
        Self {
            last_near_tick: world.get_resource::<TickCounter>().map_or(0, |t| t.current),
        }
    }
}

/// System: despawn mobs far from every player.
#[allow(clippy::type_complexity)]
pub fn despawn_mobs(
    mut commands: Commands,
    config: Res<ChunkLoadConfig>,
    tick: Res<TickCounter>,
    mut random: ResMut<EntityRandom>,
    players: Query<&Position, With<Player>>,
    mut mobs: Query<
        (Entity, &Position, &DespawnConfig, &mut Inactivity),
        (With<MobKind>, Without<Persistent>, Without<Dead>),
    >,
) {
    if players.is_empty() {
        return;
    }
    let now = tick.current;
    let short_tick_range = config.simulation_distance < DESPAWN_SHORT_TICK_RANGE;
    for (mob, position, despawn, mut inactivity) in &mut mobs {
        let Some((min_distance, max_distance)) = despawn.from_distance else {
            continue;
        };
        let nearest_sq = players
            .iter()
            .map(|player| player.0.distance_squared(position.0))
            .fold(f64::INFINITY, f64::min);

        let mut max_distance = f64::from(max_distance);
        if short_tick_range {
            max_distance = max_distance.min(DESPAWN_SHORT_TICK_RANGE_DISTANCE);
        }
        if nearest_sq > max_distance * max_distance {
            commands.entity(mob).despawn();
            continue;
        }

        let min_distance = f64::from(min_distance);
        if nearest_sq <= min_distance * min_distance {
            inactivity.last_near_tick = now;
            continue;
        }
        if !(despawn.from_inactivity || despawn.from_chance) {
            continue;
        }
        let inactive_ticks = now.saturating_sub(inactivity.last_near_tick);
        if despawn.from_inactivity
            && (inactive_ticks as f32) < despawn.inactivity_seconds as f32 * TICKS_PER_SECOND
        {
            continue;
        }
        if despawn.from_chance && random.0.gen_range(0..despawn.random_chance.max(1)) != 0 {
            continue;
        }
        commands.entity(mob).despawn();
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::*;
    use crate::entity::mob::{SpawnCause, spawn_mob};
    use crate::server::game::{GameServer, ServerConfig};
    use crate::world::ecs::{ChunkManagerWorldExt, ChunkViewers};

    /// Feet position in the middle of chunk (0, 0), on the void world's
    /// stone platform (top at y = 16).
    const COW_FEET: DVec3 = DVec3::new(8.5, 16.0, 8.5);

    struct Scene {
        server: GameServer,
        cow: Entity,
        player: Entity,
    }

    impl Scene {
        /// A cow on the platform and one player, who views the platform's
        /// chunks so they stay loaded.
        fn new(simulation_distance: i32) -> Self {
            let mut server = GameServer::with_config(ServerConfig {
                world: crate::world::WorldConfig::void_spawn_platform(1),
                simulation_distance,
                ..ServerConfig::default()
            });
            let world = server.ecs.world_mut();
            world.insert_resource(EntityRandom::seeded(11));
            let player = world.spawn((Player, Position(COW_FEET))).id();
            for x in -1..=1 {
                for z in -1..=1 {
                    let chunk = world.get_or_create_chunk(x, z);
                    world.get_mut::<ChunkViewers>(chunk).unwrap().insert(player);
                }
            }
            let cow =
                spawn_mob(world, "minecraft:cow", COW_FEET, 0.0, SpawnCause::Spawned).unwrap();
            Self {
                server,
                cow,
                player,
            }
        }

        /// Keep the player `offset` away from the cow, then tick.
        fn tick_with_player_at(&mut self, offset: DVec3) {
            let world = self.server.ecs.world_mut();
            if let Some(cow) = world.get::<Position>(self.cow).map(|p| p.0) {
                world.get_mut::<Position>(self.player).unwrap().0 = cow + offset;
            }
            self.server.tick();
        }

        fn cow_exists(&self) -> bool {
            self.server.ecs.world().get_entity(self.cow).is_ok()
        }
    }

    #[test]
    fn a_cow_far_from_the_only_player_despawns_at_once() {
        let mut scene = Scene::new(6);
        scene.tick_with_player_at(DVec3::new(200.0, 0.0, 0.0));
        assert!(!scene.cow_exists());
    }

    #[test]
    fn a_persistent_cow_never_despawns() {
        let mut scene = Scene::new(6);
        scene
            .server
            .ecs
            .world_mut()
            .entity_mut(scene.cow)
            .insert(Persistent);
        for _ in 0..20 {
            scene.tick_with_player_at(DVec3::new(200.0, 0.0, 0.0));
        }
        assert!(scene.cow_exists());
    }

    #[test]
    fn a_cow_within_32_blocks_of_a_player_never_despawns() {
        let mut scene = Scene::new(6);
        for _ in 0..2000 {
            scene.tick_with_player_at(DVec3::new(20.0, 0.0, 20.0));
        }
        assert!(scene.cow_exists());
    }

    #[test]
    fn a_short_tick_range_caps_the_instant_despawn_distance_at_44() {
        let mut scene = Scene::new(DESPAWN_SHORT_TICK_RANGE);
        scene.tick_with_player_at(DVec3::new(60.0, 0.0, 0.0));
        assert!(scene.cow_exists(), "60 blocks is within 128");

        let mut scene = Scene::new(DESPAWN_SHORT_TICK_RANGE - 1);
        scene.tick_with_player_at(DVec3::new(60.0, 0.0, 0.0));
        assert!(!scene.cow_exists(), "60 blocks is beyond 44");
    }

    #[test]
    fn a_cow_between_32_and_128_blocks_despawns_at_random_after_30_inactive_seconds() {
        let mut scene = Scene::new(6);
        let offset = DVec3::new(64.0, 0.0, 0.0);
        for _ in 0..599 {
            scene.tick_with_player_at(offset);
        }
        assert!(scene.cow_exists(), "not inactive for 600 ticks yet");
        let mut ticks = 0;
        while scene.cow_exists() && ticks < 10_000 {
            scene.tick_with_player_at(offset);
            ticks += 1;
        }
        assert!(!scene.cow_exists(), "1-in-800 per tick despawns it");
    }
}
