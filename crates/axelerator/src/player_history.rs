//! Player history tracking for friend expiry.
//!
//! Tracks the last time each friend (by XUID) successfully transferred through
//! the server. This is used by the friend expiry system to remove inactive friends.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::RwLock;
use tracing::{debug, warn};

/// JSON-backed player history: XUID -> last-seen epoch seconds.
#[derive(Clone)]
pub struct PlayerHistory {
    inner: Arc<RwLock<PlayerHistoryInner>>,
}

struct PlayerHistoryInner {
    data: HashMap<String, u64>,
    path: PathBuf,
    dirty: bool,
}

impl PlayerHistory {
    /// Load or create a player history file.
    ///
    /// The file is stored as JSON next to the token cache file.
    /// If the file doesn't exist or is invalid, starts with an empty map.
    pub fn load(path: &Path) -> Self {
        let data = match std::fs::read_to_string(path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_else(|e| {
                warn!("Failed to parse player history, starting fresh: {}", e);
                HashMap::new()
            }),
            Err(_) => HashMap::new(),
        };

        debug!(entries = data.len(), "Loaded player history");

        Self {
            inner: Arc::new(RwLock::new(PlayerHistoryInner {
                data,
                path: path.to_owned(),
                dirty: false,
            })),
        }
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
        inner.dirty = true;
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
            inner.dirty = true;
            debug!(initialized, total = inner.data.len(), "Initialized friend history entries");
        }
    }

    /// Get XUIDs that haven't been seen within `expiry_days` days.
    pub async fn get_expired(&self, expiry_days: u64) -> Vec<String> {
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
            .map(|(xuid, _)| xuid.clone())
            .collect()
    }

    /// Remove an XUID from history (after expiry/unfriend).
    pub async fn remove(&self, xuid: &str) {
        let mut inner = self.inner.write().await;
        if inner.data.remove(xuid).is_some() {
            inner.dirty = true;
        }
    }

    /// Save to disk if there are pending changes. Returns true if saved.
    pub async fn save_if_dirty(&self) -> bool {
        let mut inner = self.inner.write().await;
        if !inner.dirty {
            return false;
        }

        match serde_json::to_string_pretty(&inner.data) {
            Ok(json) => match std::fs::write(&inner.path, json) {
                Ok(_) => {
                    inner.dirty = false;
                    debug!(entries = inner.data.len(), "Saved player history");
                    true
                }
                Err(e) => {
                    warn!("Failed to save player history: {}", e);
                    false
                }
            },
            Err(e) => {
                warn!("Failed to serialize player history: {}", e);
                false
            }
        }
    }
}
