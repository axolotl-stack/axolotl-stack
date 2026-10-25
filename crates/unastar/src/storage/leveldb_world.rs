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
use crate::world::chunk::DATA3D_HEIGHTMAP_LEN;
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

                    // Missing Data3D keeps the default biomes; an unreadable or
                    // malformed value fails the load rather than losing biomes.
                    let biome_key = keys::biome_key(pos, dim);
                    if let Some(data) = db
                        .get(&biome_key, &read_opts)
                        .map_err(StorageError::Database)?
                    {
                        // Legacy: unastar used to store `Chunk::encode_biomes()` (the
                        // LevelChunk network layout) under Data3D. A real Data3D value
                        // starts with a 512-byte height map, so a shorter value is read
                        // as that legacy layout, and only if it parses exactly (24
                        // storages, a zero border count, no other bytes).
                        let decoded = if data.len() < DATA3D_HEIGHTMAP_LEN {
                            chunk.decode_biomes_network(&data).map_err(|e| {
                                format!(
                                    "{} bytes is shorter than the {DATA3D_HEIGHTMAP_LEN}-byte height map and not unastar's legacy network layout: {e}",
                                    data.len()
                                )
                            })
                        } else {
                            chunk.decode_biomes_disk(&data)
                        };
                        decoded.map_err(|e| {
                            StorageError::Deserialization(format!(
                                "chunk ({}, {}) Data3D: {e}",
                                pos.x, pos.z
                            ))
                        })?;
                    }

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
        let biome_data = col.chunk.encode_biomes_disk();
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
                .map_err(StorageError::Database)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::generator::QuartBiomes;

    /// Save an empty column, replace its Data3D value with `data3d` and load it back.
    async fn load_with_data3d(data3d: &[u8]) -> StorageResult<Option<ChunkColumn>> {
        let dir = std::env::temp_dir().join(format!("unastar-leveldb-{}", uuid::Uuid::new_v4()));
        let provider = LevelDBWorldProvider::open(&dir, 0).expect("open leveldb");
        let pos = ChunkPos::new(1, 2);
        provider
            .save_column(pos, 0, &ChunkColumn::new(Chunk::new(pos.x, pos.z)))
            .await
            .expect("save column");
        provider
            .db
            .put(&keys::biome_key(pos, 0), data3d, &WriteOptions::new())
            .expect("overwrite Data3D");
        let loaded = provider.load_column(pos, 0).await;
        drop(provider);
        let _ = std::fs::remove_dir_all(&dir);
        loaded
    }

    #[tokio::test]
    async fn legacy_network_data3d_loads_with_its_biomes() {
        let mut quarts = QuartBiomes::filled(1);
        for qz in 0..4 {
            for qx in 0..4 {
                for qy in 40..44 {
                    quarts.set(qx, qy, qz, 2);
                }
            }
        }
        let mut chunk = Chunk::new(1, 2);
        chunk.set_biomes_from_quarts(&quarts, u32::from);
        let network = chunk.encode_biomes();
        assert!(network.len() < DATA3D_HEIGHTMAP_LEN);

        let loaded = load_with_data3d(&network)
            .await
            .expect("legacy Data3D loads")
            .expect("column exists");
        assert_eq!(loaded.chunk.biome_at(0, 95, 0), Some(1));
        assert_eq!(loaded.chunk.biome_at(0, 96, 0), Some(2));
        assert_eq!(loaded.chunk.biome_at(15, 111, 15), Some(2));
        assert_eq!(loaded.chunk.biome_at(15, 112, 15), Some(1));
    }

    #[tokio::test]
    async fn short_garbage_data3d_is_an_error() {
        // 15 single-value network storages: parses until it runs out of sections.
        let garbage = [1u8, 2].repeat(15);
        assert!(matches!(
            load_with_data3d(&garbage).await,
            Err(StorageError::Deserialization(_))
        ));
    }
}
