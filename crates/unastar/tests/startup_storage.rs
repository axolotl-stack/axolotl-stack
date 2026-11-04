//! Startup with a world database that can't be opened.
//!
//! `UnastarServer::new` opens `worlds/default/db` relative to the working
//! directory, so this file holds a single test: it changes the process's
//! working directory, which no other test in this binary may rely on.

use std::sync::Arc;

use unastar::config::UnastarConfig;
use unastar::server::UnastarServer;
use unastar::storage::LevelDBWorldProvider;
use unastar::world::WorldConfig;

/// A world database another instance holds stops startup instead of leaving
/// the server running with chunk persistence off.
#[tokio::test(flavor = "multi_thread")]
async fn startup_fails_when_the_world_database_is_locked() {
    let directory = std::env::temp_dir().join(format!("unastar-startup-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    std::env::set_current_dir(&directory).unwrap();

    // Another instance holding the world database.
    let holder = LevelDBWorldProvider::open(directory.join("worlds").join("default").join("db"))
        .expect("first open of the world database");

    let config = UnastarConfig {
        world: WorldConfig::void_spawn_platform(1),
        ..UnastarConfig::default()
    };
    let result = UnastarServer::new(Arc::new(config)).await;
    let error = match result {
        Ok(_) => panic!("startup must fail while the world database is locked"),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("Failed to open world storage"),
        "unexpected error: {error}"
    );

    drop(holder);
    std::env::set_current_dir(std::env::temp_dir()).unwrap();
    let _ = std::fs::remove_dir_all(&directory);
}
