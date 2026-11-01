//! LevelDB-based world provider.
//!
//! Implements `WorldProvider` using `bleveldb` for Bedrock-compatible storage.

use async_trait::async_trait;
#[cfg(test)]
use bleveldb::WriteOptions;
use bleveldb::options::Compression;
use bleveldb::{DB, Options, ReadOptions};
use std::path::Path;
use std::sync::Arc;
use tokio::task;

use crate::storage::keys;
use crate::storage::provider::{
    ChunkColumn, StorageError, StorageResult, StoredActor, WorldProvider,
};
use crate::world::block_entity::BlockEntity;
use crate::world::chunk::DATA3D_HEIGHTMAP_LEN;
use crate::world::{Chunk, ChunkPos};

/// LevelDB-based world provider.
///
/// Each world should have its own instance pointing to `<world>/db`.
pub struct LevelDBWorldProvider {
    /// Thread-safe database handle.
    #[cfg(test)]
    db: Arc<DB>,
    writer: Arc<super::column_writer::ColumnWriter>,
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
        // BDS stores compressible SST blocks as raw deflate, format id 4.
        options.compression(Compression::ZlibRaw);

        let db =
            DB::open(path.as_ref(), &options).map_err(|e| StorageError::Database(e.to_string()))?;

        let db = Arc::new(db);
        let writer = super::column_writer::ColumnWriter::open(db.clone())?;
        Ok(Self {
            #[cfg(test)]
            db,
            writer,
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
        let writer = self.writer.clone();
        let version_key = keys::version_key(pos, dim);

        task::spawn_blocking(move || writer.read(|db| {
            let read_opts = ReadOptions::new();

            // Check if chunk exists by reading version
            match db.get(&version_key, &read_opts) {
                Ok(Some(_version_data)) => {
                    // Chunk exists - load subchunk data
                    let mut chunk = Chunk::new(pos.x, pos.z);

                    // Load subchunks for standard overworld range (-4 to 19, corresponding to Y -64 to 319)
                    for y_index in -4i8..20i8 {
                        let subchunk_key = keys::subchunk_key(pos, dim, y_index);
                        if let Some(data) = db.get(&subchunk_key, &read_opts).map_err(StorageError::Database)? {
                            chunk.decode_subchunk(y_index as i32, &data).map_err(|error|
                                StorageError::Deserialization(format!("chunk ({}, {}) section {y_index}: {error}", pos.x, pos.z)))?;
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
                            chunk.decode_biomes_disk_for_dimension(&data, dim)
                        };
                        decoded.map_err(|e| {
                            StorageError::Deserialization(format!(
                                "chunk ({}, {}) Data3D: {e}",
                                pos.x, pos.z
                            ))
                        })?;
                    }

                    if let Some(data) = db
                        .get(&keys::block_entities_key(pos, dim), &read_opts)
                        .map_err(StorageError::Database)?
                    {
                        let mut input = data.as_slice();
                        while !input.is_empty() {
                            let entity = BlockEntity::read_disk_nbt(&mut input).map_err(|e| {
                                StorageError::Deserialization(format!(
                                    "chunk ({}, {}) block entity: {e}", pos.x, pos.z
                                ))
                            })?;
                            let position = entity.position();
                            if !chunk.set_block_entity(entity) {
                                return Err(StorageError::Deserialization(format!(
                                    "block entity {position:?} is outside chunk ({}, {})",
                                    pos.x, pos.z
                                )));
                            }
                        }
                    }

                    let mut column = ChunkColumn::new(chunk);
                    // Actors live under `actorprefix<id>` records listed by the
                    // chunk's `digp` digest of concatenated 8-byte IDs.
                    if let Some(digest) = db
                        .get(&keys::entity_ids_key(pos, dim), &read_opts)
                        .map_err(StorageError::Database)?
                    {
                        if digest.len() % 8 != 0 {
                            return Err(StorageError::Deserialization(format!(
                                "chunk ({}, {}) actor digest is {} bytes, not a list of 8-byte IDs",
                                pos.x,
                                pos.z,
                                digest.len()
                            )));
                        }
                        for id in digest.chunks_exact(8) {
                            let unique_id = i64::from_le_bytes(id.try_into().expect("8 bytes"));
                            match db
                                .get(&keys::entity_data_key(unique_id), &read_opts)
                                .map_err(StorageError::Database)?
                            {
                                Some(nbt) => column.actors.push(StoredActor { unique_id, nbt }),
                                // A global actor deletion also tombstones old
                                // digests left behind by its previous chunks.
                                None => tracing::debug!(unique_id, "Obsolete chunk actor reference"),
                            }
                        }
                    }

                    Ok(Some(column))
                }
                Ok(None) => Ok(None),
                Err(e) => Err(StorageError::Database(e)),
            }
        }))
        .await
        .map_err(|e| StorageError::Database(format!("Join error: {e}")))?
    }

    async fn save_column(&self, pos: ChunkPos, dim: i32, col: &ChunkColumn) -> StorageResult<()> {
        self.writer
            .legacy_save(super::column_writer::ColumnWrite {
                position: pos,
                dimension: dim,
                column: col.clone(),
            })
            .await
    }

    fn column_writer(&self) -> Option<Arc<super::column_writer::ColumnWriter>> {
        Some(self.writer.clone())
    }

    async fn flush(&self) -> StorageResult<()> {
        self.writer.flush().await.map_err(StorageError::ColumnWrite)
    }

    async fn close(&self) -> StorageResult<()> {
        self.writer.close().await.map_err(StorageError::ColumnWrite)
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

    #[tokio::test]
    async fn actors_are_stored_under_a_chunk_digest_and_removed_records_are_deleted() {
        let dir = std::env::temp_dir().join(format!("unastar-leveldb-{}", uuid::Uuid::new_v4()));
        let provider = LevelDBWorldProvider::open(&dir, 0).expect("open leveldb");
        let pos = ChunkPos::new(-3, 4);
        let actor = |unique_id: i64| {
            use zuri_nbt::{NBTTag, encoding::LittleEndian, tag};
            let mut nbt = Vec::new();
            NBTTag::Compound(
                tag::Compound::builder()
                    .with_long("UniqueID", unique_id)
                    .build(),
            )
            .write(&mut nbt, LittleEndian)
            .unwrap();
            StoredActor { unique_id, nbt }
        };
        let mut column = ChunkColumn::new(Chunk::new(pos.x, pos.z));
        column.actors = vec![actor(-4294967295), actor(7)];
        provider.save_column(pos, 0, &column).await.unwrap();

        let digest = provider
            .db
            .get(&keys::entity_ids_key(pos, 0), &ReadOptions::new())
            .unwrap()
            .expect("digp record");
        let mut expected = (-4294967295i64).to_le_bytes().to_vec();
        expected.extend_from_slice(&7i64.to_le_bytes());
        assert_eq!(digest, expected);
        let loaded = provider.load_column(pos, 0).await.unwrap().unwrap();
        assert_eq!(loaded.actors, column.actors);

        column.actors = vec![actor(7)];
        column.removed_actors = vec![-4294967295];
        provider.save_column(pos, 0, &column).await.unwrap();
        let loaded = provider.load_column(pos, 0).await.unwrap().unwrap();
        assert_eq!(loaded.actors, [actor(7)]);
        assert!(
            provider
                .db
                .get(&keys::entity_data_key(-4294967295), &ReadOptions::new())
                .unwrap()
                .is_none()
        );

        column.actors.clear();
        column.removed_actors = vec![7];
        provider.save_column(pos, 0, &column).await.unwrap();
        assert!(
            provider
                .db
                .get(&keys::entity_ids_key(pos, 0), &ReadOptions::new())
                .unwrap()
                .is_none()
        );
        drop(provider);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
