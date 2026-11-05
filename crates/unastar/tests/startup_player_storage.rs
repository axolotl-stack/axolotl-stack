//! Startup with a player database that can't be opened.
//!
//! `UnastarServer::new` opens the world database at `worlds/default/db`
//! relative to the working directory, so this file holds a single test: it
//! changes the process's working directory to a temporary one, which no other
//! test in this binary may rely on. (The world-database case lives in its own
//! binary, `startup_storage.rs`, for the same reason.)

use std::path::PathBuf;
use std::sync::Arc;

use unastar::config::UnastarConfig;
use unastar::server::UnastarServer;
use unastar::world::WorldConfig;

/// A temporary working directory. Dropping it, even during a panic, restores
/// the previous working directory and removes the temporary one.
struct WorkingDirectory {
    previous: PathBuf,
    directory: PathBuf,
}

impl WorkingDirectory {
    fn enter_temp() -> Self {
        let directory =
            std::env::temp_dir().join(format!("unastar-startup-players-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(&directory).unwrap();
        Self {
            previous,
            directory,
        }
    }
}

impl Drop for WorkingDirectory {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.previous);
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// A configured player database that can't be opened stops startup instead
/// of letting everyone join with first-join state.
#[tokio::test(flavor = "multi_thread")]
async fn startup_fails_when_the_player_database_cannot_be_opened() {
    let cwd = WorkingDirectory::enter_temp();
    let data_dir = cwd.directory.join("playerdata");
    std::fs::create_dir_all(&data_dir).unwrap();
    // A regular file where the database directory belongs.
    std::fs::write(data_dir.join("db"), b"not a database").unwrap();

    let mut config = UnastarConfig {
        world: WorldConfig::void_spawn_platform(1),
        ..UnastarConfig::default()
    };
    config.players.leveldb_enabled = true;
    config.players.data_dir = data_dir;
    let error = match UnastarServer::new(Arc::new(config)).await {
        Ok(_) => panic!("startup must fail when the player database can't be opened"),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("Failed to open player storage"),
        "unexpected error: {error}"
    );
}
