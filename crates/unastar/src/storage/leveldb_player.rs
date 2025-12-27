//! LevelDB-based player provider.
//!
//! Implements `PlayerProvider` using `bleveldb` for player data storage.
//! Uses a separate database from world data.

use async_trait::async_trait;
use bleveldb::{DB, Options, ReadOptions, WriteOptions};
use std::path::Path;
use std::sync::Arc;
use tokio::task;
use uuid::Uuid;

use crate::storage::provider::{PlayerData, PlayerProvider, StorageError, StorageResult};

/// LevelDB-based player provider.
///
/// Stores player data in a separate database at `players/db`.
/// Key = UUID bytes (16 bytes), Value = JSON-encoded PlayerData.
pub struct LevelDBPlayerProvider {
    /// Thread-safe database handle.
    db: Arc<DB>,
}

impl LevelDBPlayerProvider {
    /// Open or create a player database.
    ///
    /// # Arguments
    /// * `path` - Path to the `db` directory (e.g., `players/db`)
    pub fn open<P: AsRef<Path>>(path: P) -> StorageResult<Self> {
        // Ensure parent directories exist
        std::fs::create_dir_all(path.as_ref())
            .map_err(|e| StorageError::Database(format!("Failed to create directory: {e}")))?;

        let options = Options::new();
        options.create_if_missing(true);

        let db =
            DB::open(path.as_ref(), &options).map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(Self { db: Arc::new(db) })
    }
}

#[async_trait]
impl PlayerProvider for LevelDBPlayerProvider {
    async fn load(&self, uuid: Uuid) -> StorageResult<Option<PlayerData>> {
        let db = self.db.clone();
        let key = uuid.as_bytes().to_vec();

        task::spawn_blocking(move || {
            let read_opts = ReadOptions::new();
            match db.get(&key, &read_opts) {
                Ok(Some(data)) => {
                    let player_data: PlayerData = serde_json::from_slice(&data)
                        .map_err(|e| StorageError::Deserialization(e.to_string()))?;
                    Ok(Some(player_data))
                }
                Ok(None) => Ok(None),
                Err(e) => Err(StorageError::Database(e.to_string())),
            }
        })
        .await
        .map_err(|e| StorageError::Database(format!("Join error: {e}")))?
    }

    async fn save(&self, uuid: Uuid, data: &PlayerData) -> StorageResult<()> {
        let db = self.db.clone();
        let key = uuid.as_bytes().to_vec();
        let value =
            serde_json::to_vec(data).map_err(|e| StorageError::Serialization(e.to_string()))?;

        task::spawn_blocking(move || {
            let write_opts = WriteOptions::new();
            db.put(&key, &value, &write_opts)
                .map_err(|e| StorageError::Database(e.to_string()))?;
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Database(format!("Join error: {e}")))?
    }

    async fn delete(&self, uuid: Uuid) -> StorageResult<()> {
        let db = self.db.clone();
        let key = uuid.as_bytes().to_vec();

        task::spawn_blocking(move || {
            let write_opts = WriteOptions::new();
            db.delete(&key, &write_opts)
                .map_err(|e| StorageError::Database(e.to_string()))?;
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Database(format!("Join error: {e}")))?
    }

    async fn close(&self) -> StorageResult<()> {
        // Database is closed when Arc is dropped
        Ok(())
    }
}
