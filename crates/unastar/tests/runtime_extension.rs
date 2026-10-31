use std::process::Command;
use std::sync::Arc;

use bevy_ecs::prelude::*;
use unastar::config::PlayerStorageConfig;
use unastar::ecs::{EntityLogicSet, TickCounter};
use unastar::world::{StorageProvider, WorldConfig, WorldGenerator};
use unastar::{UnastarConfig, UnastarServer};

/// Records the core tick seen by an externally registered system.
#[derive(Resource, Default)]
struct ObservedTicks(Vec<u64>);

/// Observe ticks before the core cleanup phase increments its counter.
fn observe_tick(tick: Res<TickCounter>, mut observed: ResMut<ObservedTicks>) {
    observed.0.push(tick.current);
}

/// Exercise runtime setup from a consumer crate without opening a listener.
async fn assert_extension_runs() {
    let config = UnastarConfig {
        world: WorldConfig {
            generator: WorldGenerator::SuperFlat,
            storage_provider: StorageProvider::BlazeDb,
            generation_threads: 1,
            ..WorldConfig::default()
        },
        players: PlayerStorageConfig {
            data_dir: "players".into(),
            save_previous_position: false,
            leveldb_enabled: false,
            ..PlayerStorageConfig::default()
        },
        ..UnastarConfig::default()
    };
    let mut runtime = UnastarServer::new(Arc::new(config)).await.unwrap();
    assert!(
        runtime
            .game_server()
            .ecs
            .world()
            .get_resource::<ObservedTicks>()
            .is_none()
    );

    let game = runtime.game_server_mut();
    game.ecs
        .world_mut()
        .insert_resource(ObservedTicks::default());
    game.ecs
        .schedule_mut()
        .add_systems(observe_tick.in_set(EntityLogicSet));
    game.tick();
    game.tick();

    let world = runtime.game_server().ecs.world();
    assert_eq!(world.resource::<ObservedTicks>().0, [0, 1]);
    assert_eq!(world.resource::<TickCounter>().current, 2);
}

#[test]
fn native_extensions_share_the_initialized_runtime_and_tick_schedule() {
    const CHILD_ENV: &str = "UNASTAR_RUNTIME_EXTENSION_TEST_CHILD";
    if std::env::var_os(CHILD_ENV).is_some() {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
            .block_on(assert_extension_runs());
        return;
    }

    // Runtime storage and plugin discovery are relative to the working directory.
    // Give this test its own process and directory without changing the test
    // harness's global working directory or sharing an open world database.
    let directory = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "native_extensions_share_the_initialized_runtime_and_tick_schedule",
            "--nocapture",
        ])
        .env(CHILD_ENV, "1")
        .current_dir(directory.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "runtime extension consumer failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
