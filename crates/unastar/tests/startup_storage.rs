//! Startup with a world database that can't be opened.
//!
//! `UnastarServer::new` opens `worlds/default/db` relative to the working
//! directory, so this file holds a single test: it changes the process's
//! working directory, which no other test in this binary may rely on.
//!
//! The open is made to fail with a regular file where the database directory
//! belongs, not with a held lock. LevelDB's in-process lock table is keyed by
//! the path string, so the same process holding the database by its absolute
//! path doesn't block the server's relative open on Linux, where the file lock
//! itself is per process. The lock is covered by the unit test
//! `server::runtime::tests::a_locked_world_leveldb_fails_to_open`.

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
            std::env::temp_dir().join(format!("unastar-startup-{}", uuid::Uuid::new_v4()));
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

/// A world database that can't be opened stops startup instead of leaving the
/// server running with chunk persistence off.
#[tokio::test(flavor = "multi_thread")]
async fn startup_fails_when_the_world_database_cannot_be_opened() {
    let cwd = WorkingDirectory::enter_temp();
    // A regular file where the database directory belongs.
    std::fs::create_dir_all(cwd.directory.join("worlds").join("default")).unwrap();
    std::fs::write(
        cwd.directory.join("worlds").join("default").join("db"),
        b"not a database",
    )
    .unwrap();

    let config = UnastarConfig {
        world: WorldConfig::void_spawn_platform(1),
        ..UnastarConfig::default()
    };
    let error = match UnastarServer::new(Arc::new(config)).await {
        Ok(_) => panic!("startup must fail when the world database can't be opened"),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("Failed to open world storage"),
        "unexpected error: {error}"
    );
}
