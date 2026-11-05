//! Startup refuses a world whose resolved spawn lies outside `world.bounds`, instead
//! of dropping every player into the all-air chunks there.

use std::sync::Arc;

use unastar::config::{SpawnRule, UnastarConfig};
use unastar::world::{WorldBounds, WorldConfig};

#[test]
fn startup_fails_when_the_world_spawn_is_outside_the_bounds() {
    let directory = tempfile::tempdir().unwrap();
    // The runtime keeps worlds, players and plugins below the working directory.
    std::env::set_current_dir(directory.path()).unwrap();
    let mut config = UnastarConfig::default();
    config.server.bind_address = "127.0.0.1:0".into();
    config.server.online_mode = false;
    // The void world's template spawn is at the origin, outside this rect.
    config.world = WorldConfig {
        bounds: WorldBounds::Rect {
            min_x: 10,
            max_x: 12,
            min_z: 10,
            max_z: 12,
        },
        ..WorldConfig::void_spawn_platform(1)
    };
    config.players.data_dir = directory.path().join("players");
    config.spawn_rules = vec![SpawnRule {
        previous_position: false,
        location: None,
        ..SpawnRule::default()
    }];
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_stack_size(64 * 1024 * 1024)
        .build()
        .unwrap();
    runtime.block_on(async move {
        let mut server = unastar::server::UnastarServer::new(Arc::new(config))
            .await
            .unwrap();
        let error = tokio::time::timeout(std::time::Duration::from_secs(30), server.run())
            .await
            .expect("startup returns instead of serving")
            .expect_err("a spawn outside the bounds fails startup");
        assert!(error.to_string().contains("world.bounds"), "{error}");
    });
}
