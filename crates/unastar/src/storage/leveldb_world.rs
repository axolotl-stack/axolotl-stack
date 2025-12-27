//! LevelDB-based world provider.
//!
//! Implements `WorldProvider` using `bleveldb` for Bedrock-compatible storage.

use async_trait::async_trait;
use bleveldb::{DB, Options, ReadOptions, WriteBatch, WriteOptions};
use std::path::Path;
use std::sync::Arc;
use tokio::task;

use crate::storage::keys;
use crate::storage::provider::{ChunkColumn, StorageError, StorageResult, WorldProvider};
use crate::world::{Chunk, ChunkPos};

/// LevelDB-based world provider.
///
/// Each world should have its own instance pointing to `<world>/db`.
pub struct LevelDBWorldProvider {
    /// Thread-safe database handle.
    db: Arc<DB>,
    /// Dimension ID for key encoding.
    dimension: i32,
}

impl LevelDBWorldProvider {
    /// Open or create a world database.
    ///
    /// # Arguments
    /// * `path` - Path to the `db` directory (e.g., `worlds/main/db`)
    /// * `dimension` - Dimension ID (0=overworld, 1=nether, 2=end)
    pub fn open<P: AsRef<Path>>(path: P, dimension: i32) -> StorageResult<Self> {
        // Ensure parent directories exist
        std::fs::create_dir_all(path.as_ref())
            .map_err(|e| StorageError::Database(format!("Failed to create directory: {e}")))?;

        let options = Options::new();
        options.create_if_missing(true);

        let db =
            DB::open(path.as_ref(), &options).map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(Self {
            db: Arc::new(db),
            dimension,
        })
    }

    /// Get the dimension ID.
    pub fn dimension(&self) -> i32 {
        self.dimension
    }
}

#[async_trait]
impl WorldProvider for LevelDBWorldProvider {
    async fn load_column(&self, pos: ChunkPos, dim: i32) -> StorageResult<Option<ChunkColumn>> {
        let db = self.db.clone();
        let version_key = keys::version_key(pos, dim);

        task::spawn_blocking(move || {
            let read_opts = ReadOptions::new();

            // Check if chunk exists by reading version
            match db.get(&version_key, &read_opts) {
                Ok(Some(_version_data)) => {
                    // Chunk exists - load subchunk data
                    let mut chunk = Chunk::new(pos.x, pos.z);

                    // Load subchunks for standard overworld range (-4 to 19, corresponding to Y -64 to 319)
                    for y_index in -4i8..20i8 {
                        let subchunk_key = keys::subchunk_key(pos, dim, y_index);
                        if let Ok(Some(data)) = db.get(&subchunk_key, &read_opts) {
                            // Decode subchunk data into the chunk
                            if let Err(e) = chunk.decode_subchunk(y_index as i32, &data) {
                                tracing::warn!(
                                    chunk = ?(pos.x, pos.z),
                                    y = y_index,
                                    error = %e,
                                    "Failed to decode subchunk, using empty"
                                );
                            }
                        }
                        // Missing subchunks are fine - they're just empty (air)
                    }

                    // TODO: Load biome data from key3DData
                    // TODO: Load block entities

                    Ok(Some(ChunkColumn::new(chunk)))
                }
                Ok(None) => Ok(None),
                Err(e) => Err(StorageError::Database(e)),
            }
        })
        .await
        .map_err(|e| StorageError::Database(format!("Join error: {e}")))?
    }

    async fn save_column(&self, pos: ChunkPos, dim: i32, col: &ChunkColumn) -> StorageResult<()> {
        let db = self.db.clone();

        // Build all key-value pairs to write
        let version_key = keys::version_key(pos, dim);
        let finalisation_key = keys::finalisation_key(pos, dim);

        // Encode chunk data
        let biome_data = col.chunk.encode_biomes();
        let biome_key = keys::biome_key(pos, dim);

        // Collect subchunk data
        let mut subchunk_entries: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        for y_index in -4..20i8 {
            // Standard overworld range
            if let Some(data) = col.chunk.encode_subchunk(y_index as i32) {
                let key = keys::subchunk_key(pos, dim, y_index);
                subchunk_entries.push((key, data));
            }
        }

        task::spawn_blocking(move || {
            let write_opts = WriteOptions::new();
            let mut batch = WriteBatch::new();

            // Version
            batch.put(&version_key, &[keys::CHUNK_VERSION]);

            // Finalisation (2 = populated)
            batch.put(&finalisation_key, &2u32.to_le_bytes());

            // Biomes
            batch.put(&biome_key, &biome_data);

            // Subchunks
            for (key, data) in subchunk_entries {
                batch.put(&key, &data);
            }

            // TODO: Block entities, entities

            // Write batch atomically - method is on WriteBatch, takes &db
            batch
                .write(&db, &write_opts)
                .map_err(|e| StorageError::Database(e))?;

            Ok(())
        })
        .await
        .map_err(|e| StorageError::Database(format!("Join error: {e}")))?
    }

    async fn flush(&self) -> StorageResult<()> {
        let db = self.db.clone();
        task::spawn_blocking(move || {
            db.flush();
            Ok(())
        })
        .await
        .map_err(|e| StorageError::Database(format!("Join error: {e}")))?
    }

    async fn close(&self) -> StorageResult<()> {
        self.flush().await
    }
}
