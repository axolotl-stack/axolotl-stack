use std::sync::{Arc, Mutex};

use bevy_ecs::prelude::*;
use glam::DVec3;
use unastar::ecs::TickCounter;
use unastar::entity::components::{GameMode, Health, Player, Position};
use unastar::entity::mob::combat::Hit;
use unastar::entity::mob::components::KnockbackPower;
use unastar::entity::player_life::{self, hurt_player};
use unastar::plugin::PluginManager;
use unastar::world::rules::{Difficulty, GameRules};
use unastar_data::actor_components::body::ActorDamageCause;

#[tokio::test]
async fn custom_runtime_can_attach_synchronous_component_hooks() {
    let mut world = World::new();
    world.init_resource::<TickCounter>();
    world.init_resource::<GameRules>();
    world.insert_resource(Difficulty::Normal);
    player_life::install(&mut world, DVec3::ZERO).unwrap();
    let manager = Arc::new(Mutex::new(PluginManager::new().unwrap()));
    PluginManager::attach_to_world(Arc::clone(&manager), &mut world);
    let player = world
        .spawn((
            Player,
            GameMode::Survival,
            Health::default(),
            Position(DVec3::ZERO),
        ))
        .id();
    let hit = Hit {
        damage: 2.0,
        cause: ActorDamageCause::Fall,
        attacker: None,
        by_player: false,
        knockback_from: None,
        knockback_power: KnockbackPower::DEFAULT,
    };

    // Contention proves the public attachment installed the synchronous bridge.
    let guard = manager.lock().unwrap();
    assert_eq!(hurt_player(&mut world, player, &hit), 0.0);
    assert_eq!(world.get::<Health>(player).unwrap().current, 20.0);
    drop(guard);

    assert_eq!(hurt_player(&mut world, player, &hit), 2.0);
    assert_eq!(world.get::<Health>(player).unwrap().current, 18.0);
}
