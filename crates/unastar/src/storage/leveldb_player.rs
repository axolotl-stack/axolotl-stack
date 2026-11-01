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
    items: Arc<std::sync::Mutex<super::item_grants::ItemLedger>>,
    items_uncertain: Arc<std::sync::atomic::AtomicBool>,
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

        let items = super::item_grants::ItemLedger::open(&db)?;
        Ok(Self {
            db: Arc::new(db),
            items: Arc::new(std::sync::Mutex::new(items)),
            items_uncertain: Arc::default(),
        })
    }
}

#[async_trait]
impl PlayerProvider for LevelDBPlayerProvider {
    fn atomic_durable_items(&self) -> bool {
        true
    }

    async fn world_items(
        &self,
        world: Uuid,
        dimension: i32,
    ) -> StorageResult<Vec<super::item_grants::PersistentItem>> {
        if self
            .items_uncertain
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(StorageError::PlayerRecoveryRequired);
        }
        self.items
            .lock()
            .map(|items| items.world(world, dimension))
            .map_err(|_| StorageError::PlayerRecoveryRequired)
    }

    async fn retire_world_item(
        &self,
        expected: &super::item_grants::PersistentItem,
    ) -> StorageResult<()> {
        let db = self.db.clone();
        let items = self.items.clone();
        let uncertain = self.items_uncertain.clone();
        let mutation = super::item_grants::ItemMutation::Take {
            expected: expected.clone(),
            remaining: 0,
        };
        task::spawn_blocking(move || {
            let mut ledger = items
                .lock()
                .map_err(|_| StorageError::PlayerRecoveryRequired)?;
            if uncertain.load(std::sync::atomic::Ordering::Acquire) {
                return Err(StorageError::PlayerRecoveryRequired);
            }
            let prepared = ledger.prepare(&mutation)?;
            let mut batch = bleveldb::WriteBatch::new();
            prepared.append(&mut batch);
            let options = WriteOptions::new();
            options.sync(true);
            if let Err(error) = batch.write(&db, &options) {
                uncertain.store(true, std::sync::atomic::Ordering::Release);
                return Err(StorageError::Database(error));
            }
            ledger.acknowledge(prepared);
            Ok(())
        })
        .await
        .map_err(|error| StorageError::Database(error.to_string()))?
    }

    async fn save_with_items(
        &self,
        uuid: Uuid,
        data: &PlayerData,
        mutation: &super::item_grants::ItemMutation,
    ) -> StorageResult<()> {
        data.validate(uuid)?;
        let db = self.db.clone();
        let items = self.items.clone();
        let uncertain = self.items_uncertain.clone();
        let mutation = mutation.clone();
        let value = serde_json::to_vec(data)
            .map_err(|error| StorageError::Serialization(error.to_string()))?;
        task::spawn_blocking(move || {
            let mut ledger = items
                .lock()
                .map_err(|_| StorageError::PlayerRecoveryRequired)?;
            if uncertain.load(std::sync::atomic::Ordering::Acquire) {
                return Err(StorageError::PlayerRecoveryRequired);
            }
            let prepared = ledger.prepare(&mutation)?;
            let mut batch = bleveldb::WriteBatch::new();
            batch.put(uuid.as_bytes(), &value);
            prepared.append(&mut batch);
            let options = WriteOptions::new();
            options.sync(true);
            if let Err(error) = batch.write(&db, &options) {
                uncertain.store(true, std::sync::atomic::Ordering::Release);
                return Err(StorageError::Database(error));
            }
            ledger.acknowledge(prepared);
            Ok(())
        })
        .await
        .map_err(|error| StorageError::Database(error.to_string()))?
    }
    fn atomic_durable_records(&self) -> bool {
        true
    }

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
            write_opts.sync(true);
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
