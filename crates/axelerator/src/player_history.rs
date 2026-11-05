//! Player history tracking for friend expiry.
//!
//! Tracks the last time each friend (by XUID) successfully transferred through
//! the server. This is used by the friend expiry system to remove inactive friends.
//!
//! One `PlayerHistory` is loaded per run and shared (it is a cheap handle):
//! the transfer server records joins and the expiry task reads and removes
//! entries through the same map, and only that map is ever written back.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// JSON-backed player history: XUID -> last-seen epoch seconds.
#[derive(Clone)]
pub struct PlayerHistory {
    inner: Arc<RwLock<PlayerHistoryInner>>,
    /// Held for a whole save, so two saves never race their renames and an
    /// older snapshot can never replace a newer file.
    saving: Arc<tokio::sync::Mutex<()>>,
}

struct PlayerHistoryInner {
    data: HashMap<String, u64>,
    path: PathBuf,
    /// Incremented by every change.
    changes: u64,
    /// `changes` as of the snapshot the last successful save wrote; the
    /// history is dirty while they differ.
    saved: u64,
}

impl PlayerHistory {
    /// Load a player history file, or start empty when it does not exist.
    ///
    /// The file is stored as JSON next to the token cache file. An unreadable
    /// or malformed file is an error and is left untouched: starting empty
    /// would overwrite it on the next save and lose every last-seen time.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let data = match std::fs::read_to_string(path) {
            Ok(contents) => serde_json::from_str(&contents).with_context(|| {
                format!(
                    "player history {} is not valid JSON; fix or remove it",
                    path.display()
                )
            })?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("failed to read player history {}", path.display()));
            }
        };

        debug!(entries = data.len(), "Loaded player history");

        Ok(Self {
            inner: Arc::new(RwLock::new(PlayerHistoryInner {
                data,
                path: path.to_owned(),
                changes: 0,
                saved: 0,
            })),
            saving: Arc::default(),
        })
    }

    /// Derive the player history path from the token cache path.
    ///
    /// Places `player_history.json` in the same directory as `token.json`.
    pub fn path_from_token_cache(token_cache_path: &str) -> PathBuf {
        let token_path = Path::new(token_cache_path);
        token_path
            .parent()
            .unwrap_or(Path::new("."))
            .join("player_history.json")
    }

    /// Update the last-seen timestamp for an XUID to now.
    pub async fn update_last_seen(&self, xuid: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut inner = self.inner.write().await;
        inner.data.insert(xuid.to_string(), now);
        inner.changes += 1;
    }

    /// Initialize all given XUIDs with the current timestamp if they don't already exist.
    ///
    /// Used on first run to seed existing friends so they aren't immediately expired.
    pub async fn initialize_friends(&self, xuids: &[String]) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut inner = self.inner.write().await;
        let mut initialized = 0;
        for xuid in xuids {
            if !inner.data.contains_key(xuid) {
                inner.data.insert(xuid.clone(), now);
                initialized += 1;
            }
        }
        if initialized > 0 {
            inner.changes += 1;
            debug!(
                initialized,
                total = inner.data.len(),
                "Initialized friend history entries"
            );
        }
    }

    /// XUIDs not seen within `expiry_days` days, each with the last-seen
    /// time read now. Pass that time to [`Self::remove_if_unchanged`] so a
    /// join recorded in the meantime is never removed.
    pub async fn get_expired(&self, expiry_days: u64) -> Vec<(String, u64)> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let cutoff = now.saturating_sub(expiry_days * 86400);

        let inner = self.inner.read().await;
        inner
            .data
            .iter()
            .filter(|&(_, &last_seen)| last_seen < cutoff)
            .map(|(xuid, &last_seen)| (xuid.clone(), last_seen))
            .collect()
    }

    /// The recorded last-seen time for `xuid`.
    pub async fn last_seen(&self, xuid: &str) -> Option<u64> {
        self.inner.read().await.data.get(xuid).copied()
    }

    /// Sets a last-seen time directly (tests age entries with it).
    #[cfg(test)]
    pub(crate) async fn set_last_seen(&self, xuid: &str, at: u64) {
        let mut inner = self.inner.write().await;
        inner.data.insert(xuid.to_string(), at);
        inner.changes += 1;
    }

    /// Removes `xuid` only if its last-seen time is still `seen` (the value
    /// [`Self::get_expired`] returned). Returns whether it was removed; false
    /// means the player joined since, or the entry is already gone.
    pub async fn remove_if_unchanged(&self, xuid: &str, seen: u64) -> bool {
        let mut inner = self.inner.write().await;
        if inner.data.get(xuid) != Some(&seen) {
            return false;
        }
        inner.data.remove(xuid);
        inner.changes += 1;
        true
    }

    /// Remove an XUID from history (after expiry/unfriend).
    pub async fn remove(&self, xuid: &str) {
        let mut inner = self.inner.write().await;
        if inner.data.remove(xuid).is_some() {
            inner.changes += 1;
        }
    }

    /// Save to disk if there are pending changes. Returns true if saved.
    ///
    /// The map is copied under the lock and written outside it on a blocking
    /// thread, so joins and expiry are not held up by the file sync. Changes
    /// made while the write runs stay pending for the next save.
    pub async fn save_if_dirty(&self) -> bool {
        let _saving = self.saving.lock().await;
        let (path, data, changes) = {
            let inner = self.inner.read().await;
            if inner.changes == inner.saved {
                return false;
            }
            (inner.path.clone(), inner.data.clone(), inner.changes)
        };
        let entries = data.len();
        let written = tokio::task::spawn_blocking(move || write_atomically(&path, &data))
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("player history writer failed: {e}")));
        match written {
            Ok(()) => {
                self.inner.write().await.saved = changes;
                debug!(entries, "Saved player history");
                true
            }
            Err(e) => {
                warn!("Failed to save player history: {:#}", e);
                false
            }
        }
    }
}

/// Writes `data` to a new file beside `path`, syncs it and renames it over
/// `path`, so a crash mid-save leaves the complete old or new JSON, never a
/// truncated file. The temporary file is removed when anything fails.
fn write_atomically(path: &Path, data: &HashMap<String, u64>) -> anyhow::Result<()> {
    use std::io::Write;

    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temporary = parent.join(format!(".player_history-{}.tmp", uuid::Uuid::new_v4()));
    struct RemoveTemporary(PathBuf);
    impl Drop for RemoveTemporary {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .context("create temporary player history")?;
    let _cleanup = RemoveTemporary(temporary.clone());
    serde_json::to_writer_pretty(&mut file, data).context("serialize player history")?;
    file.flush().context("flush player history")?;
    file.sync_all().context("sync player history")?;
    drop(file);
    std::fs::rename(&temporary, path).context("replace player history")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A malformed file used to be replaced by an empty map, which the next
    /// save wrote over the old data.
    #[test]
    fn corrupt_history_is_an_error_and_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("player_history.json");
        std::fs::write(&path, b"{\"123\": 1").unwrap();
        assert!(PlayerHistory::load(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"123\": 1");
    }

    #[test]
    fn missing_history_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let history = PlayerHistory::load(&dir.path().join("absent.json")).unwrap();
        assert!(history.inner.try_read().unwrap().data.is_empty());
    }

    #[tokio::test]
    async fn save_replaces_the_file_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("player_history.json");
        std::fs::write(&path, b"{\"1\": 5}").unwrap();
        let history = PlayerHistory::load(&path).unwrap();
        history.update_last_seen("2").await;
        assert!(history.save_if_dirty().await);
        let saved: HashMap<String, u64> =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["1"], 5);
        assert!(saved.contains_key("2"));
        // Only the history file remains: no temporary is left behind.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn failed_save_keeps_the_destination_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        // A directory cannot be replaced by a file rename.
        let path = dir.path().join("player_history.json");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("marker"), b"unchanged").unwrap();
        let history = PlayerHistory {
            inner: Arc::new(RwLock::new(PlayerHistoryInner {
                data: HashMap::from([("1".to_string(), 1)]),
                path: path.clone(),
                changes: 1,
                saved: 0,
            })),
            saving: Arc::default(),
        };
        assert!(!history.save_if_dirty().await);
        assert_eq!(std::fs::read(path.join("marker")).unwrap(), b"unchanged");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    /// A change recorded after a save took its snapshot is not marked saved
    /// by that save, so the next save writes it.
    #[tokio::test]
    async fn change_during_a_save_stays_pending() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("player_history.json");
        let history = PlayerHistory::load(&path).unwrap();
        history.update_last_seen("1").await;
        // A save snapshots now; "2" joins before that save records success.
        let snapshot = history.inner.read().await.changes;
        history.update_last_seen("2").await;
        history.inner.write().await.saved = snapshot;
        assert!(
            history.save_if_dirty().await,
            "the later join was treated as saved"
        );
        let saved: HashMap<String, u64> =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(saved.contains_key("2"));
        assert!(!history.save_if_dirty().await);
    }

    /// Concurrent joins and saves: whatever the interleaving, a final save
    /// writes every join and no temporary file is left behind.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_saves_keep_every_join() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("player_history.json");
        let history = PlayerHistory::load(&path).unwrap();
        let mut tasks = Vec::new();
        for i in 0..32 {
            let history = history.clone();
            tasks.push(tokio::spawn(async move {
                history.update_last_seen(&i.to_string()).await;
                history.save_if_dirty().await;
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        history.save_if_dirty().await;
        let saved: HashMap<String, u64> =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.len(), 32);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    /// The expiry task and the transfer server used to load separate
    /// copies, so removals and new joins never met. Clones share one map.
    #[tokio::test]
    async fn clones_share_one_history() {
        let dir = tempfile::tempdir().unwrap();
        let history = PlayerHistory::load(&dir.path().join("h.json")).unwrap();
        let transfer_side = history.clone();
        transfer_side.update_last_seen("42").await;
        assert!(history.get_expired(0).await.is_empty());
        history.remove("42").await;
        assert!(transfer_side.inner.read().await.data.is_empty());
    }
}
