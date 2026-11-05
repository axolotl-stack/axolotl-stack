//! Startup with a world database another process holds.
//!
//! `UnastarServer::new` opens `worlds/default/db` relative to the working
//! directory, so the parent test changes the process's working directory. No
//! other test in this binary may rely on it.
//!
//! The lock must be held by a different process. LevelDB's LOCK file uses
//! POSIX `fcntl` locks on Linux, which are per process, so a second open from
//! the same process isn't refused by the OS. LevelDB's own in-process check only
//! catches it when the path string is identical. The test binary therefore
//! re-runs itself as the holder (see [`hold_world_database_for_parent`]).

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use unastar::config::UnastarConfig;
use unastar::server::UnastarServer;
use unastar::storage::LevelDBWorldProvider;
use unastar::world::WorldConfig;

/// Set on the child: the database path it opens and holds.
const HOLD_ENV: &str = "UNASTAR_TEST_HOLD_WORLD_DB";
/// Printed by the child once the database is open.
const HELD: &str = "UNASTAR_TEST_WORLD_DB_HELD";

/// Child half of [`startup_fails_when_another_process_holds_the_world_database`].
/// Does nothing unless the parent set [`HOLD_ENV`]. Then it opens the database,
/// reports that it holds it, and keeps it open until the parent closes its stdin.
#[test]
fn hold_world_database_for_parent() {
    let Ok(path) = std::env::var(HOLD_ENV) else {
        return;
    };
    let provider = LevelDBWorldProvider::open(&path).expect("child opens the world database");
    println!("{HELD}");
    let mut rest = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut rest);
    drop(provider);
}

/// The holder process. Killed on drop if the parent test panics first.
struct Holder(Child);

impl Holder {
    /// Re-run this test binary as the holder of `path` and wait until it has
    /// the database open.
    fn spawn(path: &Path) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "hold_world_database_for_parent",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(HOLD_ENV, path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn the holder process");
        let stdout = child.stdout.take().unwrap();
        let mut holder = Self(child);
        // Drain the child's stdout to the end on a thread: if the pipe closed
        // early, the child's later test-harness output would fail to write and
        // the child would exit with an error.
        let (held_tx, held_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if line.contains(HELD) {
                    let _ = held_tx.send(());
                }
            }
        });
        // The sender drops at EOF, so a child that exits without opening the
        // database ends the wait too.
        let held = held_rx.recv().is_ok();
        if !held {
            let status = holder.0.wait();
            panic!("holder exited before opening the database: {status:?}");
        }
        holder
    }

    /// Let the holder close the database and exit.
    fn release(mut self) {
        drop(self.0.stdin.take());
        let status = self.0.wait().expect("wait for the holder");
        assert!(status.success(), "holder failed: {status}");
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        if self.0.stdin.is_some() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// A world database another instance holds stops startup instead of leaving
/// the server running with chunk persistence off.
#[tokio::test(flavor = "multi_thread")]
async fn startup_fails_when_another_process_holds_the_world_database() {
    let directory = std::env::temp_dir().join(format!("unastar-startup-{}", uuid::Uuid::new_v4()));
    let db = directory.join("worlds").join("default").join("db");
    std::fs::create_dir_all(&db).unwrap();
    let holder = Holder::spawn(&db);
    std::env::set_current_dir(&directory).unwrap();

    let config = UnastarConfig {
        world: WorldConfig::void_spawn_platform(1),
        ..UnastarConfig::default()
    };
    let result = UnastarServer::new(Arc::new(config)).await;
    std::env::set_current_dir(std::env::temp_dir()).unwrap();
    let error = match result {
        Ok(_) => panic!("startup must fail while another process holds the world database"),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("Failed to open world storage"),
        "unexpected error: {error}"
    );

    holder.release();
    let _ = std::fs::remove_dir_all(&directory);
}
