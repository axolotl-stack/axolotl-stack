//! BlazeDB world provider - high-performance storage with spatial indexing.
//!
//! Features:
//! - Z-Order (Morton) encoding for spatial locality
//! - Append-only data file for high write throughput
//! - Sharded LRU cache for O(1) reads
//! - Async background writes via tokio channel
//! - LZ4 compression for efficient storage

use async_trait::async_trait;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

use super::cache::ShardedCache;
use super::morton;
use super::provider::{ChunkColumn, StorageError, StorageResult, WorldProvider};
use crate::world::block_entity::BlockEntity;
use crate::world::{Chunk, ChunkPos};

/// Magic bytes at start of each chunk entry.
const MAGIC: &[u8; 4] = b"BLAZ";
const ENTRY_HEADER_LEN: usize = 24;
const ENTRY_DATA_PREFIX_LEN: usize = 4;
const MIN_ENTRY_SIZE: u32 = (ENTRY_HEADER_LEN + ENTRY_DATA_PREFIX_LEN) as u32;

/// Version 3 appends length-prefixed concatenated fixed-LE block-entity NBT.
/// Version 2 stores Data3D biomes; version 1's network biomes are skipped.
const FORMAT_VERSION: u8 = 3;

/// First format version whose biome field is a Data3D value.
const DATA3D_BIOMES_VERSION: u8 = 2;

/// Compression types.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None = 0,
    Lz4 = 1,
}

/// Index entry for a chunk in the data file.
#[derive(Debug, Clone, Copy)]
struct IndexEntry {
    offset: u64,
    size: u32,
}

/// Configuration for BlazeDB.
#[derive(Debug, Clone)]
pub struct BlazeConfig {
    /// Maximum cache entries (default: 4096 chunks ~= 512MB at 128KB/chunk)
    pub cache_capacity: usize,
    /// Compression type (default: LZ4)
    pub compression: Compression,
    /// Flush interval in milliseconds (default: 100)
    pub flush_interval_ms: u64,
}

impl Default for BlazeConfig {
    fn default() -> Self {
        Self {
            cache_capacity: 512,
            compression: Compression::Lz4,
            flush_interval_ms: 100,
        }
    }
}

/// Write request for the background worker.
struct WriteRequest {
    morton: u64,
    data: Vec<u8>,
    x: i32,
    z: i32,
    dim: i32,
}

enum WriteCommand {
    Save(WriteRequest),
    Flush(oneshot::Sender<StorageResult<()>>),
    Close(oneshot::Sender<StorageResult<()>>),
}

/// BlazeDB world provider.
pub struct BlazeDBProvider {
    /// Path to the database directory.
    path: PathBuf,
    /// Data file (append-only).
    data_file: Arc<Mutex<File>>,
    /// Current write offset.
    write_offset: AtomicU64,
    /// Spatial index: Morton code -> (offset, size).
    index: RwLock<HashMap<u64, IndexEntry>>,
    /// Sharded LRU cache.
    cache: ShardedCache,
    /// Configuration.
    config: BlazeConfig,
    /// Write channel sender (for async writes).
    write_tx: mpsc::UnboundedSender<WriteCommand>,
}

impl BlazeDBProvider {
    /// Open or create a BlazeDB database.
    ///
    /// # Arguments
    /// * `path` - Path to the database directory
    /// * `config` - Optional configuration (uses defaults if None)
    pub fn open<P: AsRef<Path>>(path: P, config: Option<BlazeConfig>) -> StorageResult<Arc<Self>> {
        let path = path.as_ref().to_path_buf();
        let config = config.unwrap_or_default();

        // Ensure directory exists
        std::fs::create_dir_all(&path).map_err(StorageError::Io)?;

        let data_path = path.join("chunks.dat");
        let index_path = path.join("index.dat");

        // Open or create data file
        let data_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&data_path)
            .map_err(StorageError::Io)?;

        let write_offset = data_file.metadata().map_err(StorageError::Io)?.len();

        // Try to load index from file, or rebuild from data
        let mut index = if index_path.exists() {
            match Self::load_index(&index_path) {
                Ok(index) => index,
                Err(e) => {
                    warn!("Failed to load index, rebuilding: {}", e);
                    Self::rebuild_index(&data_file)?
                }
            }
        } else {
            Self::rebuild_index(&data_file)?
        };
        Self::repair_legacy_index_sizes(&data_file, &mut index)?;

        info!(
            "BlazeDB opened: {} chunks in index, {} bytes on disk",
            index.len(),
            write_offset
        );

        // Create cache
        let cache = ShardedCache::new(config.cache_capacity);

        // Create write channel
        let (write_tx, write_rx) = mpsc::unbounded_channel();

        let provider = Arc::new(Self {
            path,
            data_file: Arc::new(Mutex::new(data_file)),
            write_offset: AtomicU64::new(write_offset),
            index: RwLock::new(index),
            cache,
            config,
            write_tx,
        });

        // Start background write worker
        let worker_provider = provider.clone();
        tokio::spawn(async move {
            Self::write_worker(worker_provider, write_rx).await;
        });

        Ok(provider)
    }

    /// Load index from file.
    fn load_index(path: &Path) -> StorageResult<HashMap<u64, IndexEntry>> {
        let mut file = File::open(path).map_err(StorageError::Io)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data).map_err(StorageError::Io)?;
        if data.len() % 20 != 0 {
            return Err(StorageError::Deserialization(
                "Truncated BlazeDB index".into(),
            ));
        }

        let mut index = HashMap::new();
        let mut cursor = 0;

        while cursor + 20 <= data.len() {
            let morton = u64::from_le_bytes(data[cursor..cursor + 8].try_into().unwrap());
            let offset = u64::from_le_bytes(data[cursor + 8..cursor + 16].try_into().unwrap());
            let size = u32::from_le_bytes(data[cursor + 16..cursor + 20].try_into().unwrap());

            index.insert(morton, IndexEntry { offset, size });
            cursor += 20;
        }

        Ok(index)
    }

    /// Rebuild index by scanning the data file.
    fn rebuild_index(file: &File) -> StorageResult<HashMap<u64, IndexEntry>> {
        let mut file = file.try_clone().map_err(StorageError::Io)?;
        let mut index = HashMap::new();
        let file_len = file.metadata().map_err(StorageError::Io)?.len();

        file.seek(SeekFrom::Start(0)).map_err(StorageError::Io)?;

        loop {
            let offset = file.stream_position().map_err(StorageError::Io)?;
            if offset == file_len {
                break;
            }

            // Read header
            let mut header = [0u8; 4];
            file.read_exact(&mut header).map_err(StorageError::Io)?;

            if &header != MAGIC {
                return Err(StorageError::Deserialization(format!(
                    "Invalid BlazeDB magic at offset {offset}"
                )));
            }

            // Read size
            let mut size_buf = [0u8; 4];
            file.read_exact(&mut size_buf).map_err(StorageError::Io)?;
            let size = u32::from_le_bytes(size_buf);
            if size < MIN_ENTRY_SIZE {
                return Err(StorageError::Deserialization(format!(
                    "Invalid BlazeDB entry size {size} at offset {offset}"
                )));
            }

            // Skip CRC
            file.seek(SeekFrom::Current(4)).map_err(StorageError::Io)?;

            // Read coordinates
            let mut x_buf = [0u8; 4];
            let mut z_buf = [0u8; 4];
            let mut dim_buf = [0u8; 4];
            file.read_exact(&mut x_buf).map_err(StorageError::Io)?;
            file.read_exact(&mut z_buf).map_err(StorageError::Io)?;
            file.read_exact(&mut dim_buf).map_err(StorageError::Io)?;

            let x = i32::from_le_bytes(x_buf);
            let z = i32::from_le_bytes(z_buf);
            let dim = i32::from_le_bytes(dim_buf);
            let mut prefix = [0u8; ENTRY_DATA_PREFIX_LEN];
            file.read_exact(&mut prefix).map_err(StorageError::Io)?;

            let mut entry_size = size as u64;
            let declared_end = offset
                .checked_add(entry_size)
                .ok_or_else(|| StorageError::Deserialization("Chunk entry size overflow".into()))?;
            let legacy_end = declared_end.checked_add(ENTRY_DATA_PREFIX_LEN as u64);
            if let Some(legacy_end) = legacy_end
                && matches!(prefix[1], 1 | 2)
                && declared_end < file_len
                && legacy_end <= file_len
                && !Self::file_has_magic_at(&mut file, declared_end, file_len)?
                && Self::entry_boundary_is_valid(&mut file, legacy_end, file_len)?
            {
                warn!(
                    "Repairing legacy short BlazeDB entry size at offset {} from {} to {}",
                    offset,
                    entry_size,
                    entry_size + ENTRY_DATA_PREFIX_LEN as u64
                );
                entry_size += ENTRY_DATA_PREFIX_LEN as u64;
            }
            let entry_end = offset
                .checked_add(entry_size)
                .ok_or_else(|| StorageError::Deserialization("Chunk entry size overflow".into()))?;
            if entry_end > file_len {
                return Err(StorageError::Deserialization(format!(
                    "BlazeDB entry at {offset} ends at {entry_end}, beyond file length {file_len}"
                )));
            }

            let entry_size = u32::try_from(entry_size)
                .map_err(|_| StorageError::Database("BlazeDB entry too large".to_string()))?;
            let morton = morton::encode(x, z, dim);
            index.insert(
                morton,
                IndexEntry {
                    offset,
                    size: entry_size,
                },
            );

            // Skip to next entry (size includes the full fixed header).
            file.seek(SeekFrom::Start(offset + entry_size as u64))
                .map_err(StorageError::Io)?;
        }

        Ok(index)
    }

    /// Save index to file.
    fn save_index(&self) -> StorageResult<()> {
        let index_path = self.path.join("index.dat");
        let index = self.index.read();

        let mut data = Vec::with_capacity(index.len() * 20);
        for (&morton, entry) in index.iter() {
            data.extend_from_slice(&morton.to_le_bytes());
            data.extend_from_slice(&entry.offset.to_le_bytes());
            data.extend_from_slice(&entry.size.to_le_bytes());
        }

        std::fs::write(&index_path, &data).map_err(StorageError::Io)?;
        Ok(())
    }

    /// Repair indexes written by the initial BlazeDB format bug.
    ///
    /// Older entries stored `size = 24 + payload_len` but still wrote the
    /// 4-byte compression/version/reserved prefix between the fixed header and
    /// payload. Boundaries are checked against the actual data file, including
    /// obsolete entries between the latest indexed chunk versions.
    fn repair_legacy_index_sizes(
        file: &File,
        index: &mut HashMap<u64, IndexEntry>,
    ) -> StorageResult<()> {
        let file_len = file.metadata().map_err(StorageError::Io)?.len();
        let mut file = file.try_clone().map_err(StorageError::Io)?;

        let mut repairs = Vec::new();
        let mut removals = Vec::new();
        for (&morton, entry) in index.iter() {
            if entry.size < MIN_ENTRY_SIZE {
                warn!(
                    "Dropping too-small BlazeDB index entry at offset {} with size {}",
                    entry.offset, entry.size
                );
                removals.push(morton);
                continue;
            }
            if entry.offset >= file_len {
                warn!(
                    "Dropping BlazeDB index entry at offset {} beyond file length {}",
                    entry.offset, file_len
                );
                removals.push(morton);
                continue;
            }

            let Some(declared_end) = entry.offset.checked_add(entry.size as u64) else {
                warn!(
                    "Dropping BlazeDB index entry at offset {} with overflowing size {}",
                    entry.offset, entry.size
                );
                removals.push(morton);
                continue;
            };
            if declared_end <= file_len
                && Self::entry_boundary_is_valid(&mut file, declared_end, file_len)?
            {
                continue;
            }

            let legacy_end = declared_end.checked_add(ENTRY_DATA_PREFIX_LEN as u64);
            let legacy_boundary = match legacy_end {
                Some(end) if end <= file_len => {
                    Self::entry_boundary_is_valid(&mut file, end, file_len)?
                }
                _ => false,
            };
            let legacy_version = if legacy_boundary {
                file.seek(SeekFrom::Start(entry.offset + ENTRY_HEADER_LEN as u64))
                    .map_err(StorageError::Io)?;
                let mut prefix = [0u8; ENTRY_DATA_PREFIX_LEN];
                file.read_exact(&mut prefix).map_err(StorageError::Io)?;
                matches!(prefix[1], 1 | 2)
            } else {
                false
            };
            if legacy_version {
                repairs.push(morton);
            } else {
                warn!(
                    "Dropping BlazeDB index entry at offset {} with invalid declared end {}",
                    entry.offset, declared_end
                );
                removals.push(morton);
            }
        }

        for morton in repairs {
            if let Some(entry) = index.get_mut(&morton) {
                entry.size += ENTRY_DATA_PREFIX_LEN as u32;
                warn!(
                    "Repaired legacy short BlazeDB index entry at offset {} to size {}",
                    entry.offset, entry.size
                );
            }
        }
        for morton in removals {
            index.remove(&morton);
        }

        Ok(())
    }

    fn entry_boundary_is_valid(file: &mut File, offset: u64, file_len: u64) -> StorageResult<bool> {
        if offset == file_len {
            return Ok(true);
        }
        Self::file_has_magic_at(file, offset, file_len)
    }

    fn file_has_magic_at(file: &mut File, offset: u64, file_len: u64) -> StorageResult<bool> {
        if offset + MAGIC.len() as u64 > file_len {
            return Ok(false);
        }
        let current = file.stream_position().map_err(StorageError::Io)?;
        file.seek(SeekFrom::Start(offset))
            .map_err(StorageError::Io)?;
        let mut magic = [0u8; 4];
        let result = file.read_exact(&mut magic).map(|_| &magic == MAGIC);
        file.seek(SeekFrom::Start(current))
            .map_err(StorageError::Io)?;
        result.map_err(StorageError::Io)
    }

    /// Background write worker.
    async fn write_worker(provider: Arc<Self>, mut rx: mpsc::UnboundedReceiver<WriteCommand>) {
        let mut pending_writes = Vec::new();
        loop {
            match tokio::time::timeout(
                std::time::Duration::from_millis(provider.config.flush_interval_ms),
                rx.recv(),
            )
            .await
            {
                Ok(Some(WriteCommand::Save(req))) => pending_writes.push(req),
                Ok(Some(WriteCommand::Flush(reply))) => {
                    let _ = reply.send(provider.flush_pending(&mut pending_writes));
                }
                Ok(Some(WriteCommand::Close(reply))) => {
                    rx.close();
                    let result = provider.flush_pending(&mut pending_writes);
                    drop(provider);
                    let _ = reply.send(result);
                    return;
                }
                Ok(None) => {
                    if let Err(e) = provider.flush_pending(&mut pending_writes) {
                        error!("Error flushing writes on shutdown: {}", e);
                    }
                    break;
                }
                Err(_) => {
                    if let Err(e) = provider.flush_writes(&mut pending_writes) {
                        error!("Error flushing writes: {}", e);
                    }
                }
            }
        }
        debug!("BlazeDB write worker shut down");
    }

    fn flush_pending(&self, writes: &mut Vec<WriteRequest>) -> StorageResult<()> {
        self.flush_writes(writes)?;
        self.data_file.lock().sync_all().map_err(StorageError::Io)?;
        self.save_index()
    }

    /// Flush pending writes to disk.
    fn flush_writes(&self, writes: &mut Vec<WriteRequest>) -> StorageResult<()> {
        if writes.is_empty() {
            return Ok(());
        }

        let mut file = self.data_file.lock();
        let mut index = self.index.write();

        for req in writes.iter() {
            let offset = self.write_offset.load(Ordering::Relaxed);

            // Build entry
            let total_size_usize = ENTRY_HEADER_LEN
                .checked_add(ENTRY_DATA_PREFIX_LEN)
                .and_then(|len| len.checked_add(req.data.len()))
                .ok_or_else(|| StorageError::Database("BlazeDB entry too large".to_string()))?;
            let total_size = u32::try_from(total_size_usize)
                .map_err(|_| StorageError::Database("BlazeDB entry too large".to_string()))?;

            let mut entry = Vec::with_capacity(total_size_usize);
            entry.extend_from_slice(MAGIC);
            entry.extend_from_slice(&total_size.to_le_bytes());

            // CRC32 (placeholder - compute over data)
            let crc = crc32fast::hash(&req.data);
            entry.extend_from_slice(&crc.to_le_bytes());

            // Coordinates
            entry.extend_from_slice(&req.x.to_le_bytes());
            entry.extend_from_slice(&req.z.to_le_bytes());
            entry.extend_from_slice(&req.dim.to_le_bytes());

            // Compression type
            entry.push(self.config.compression as u8);

            // Version
            entry.push(FORMAT_VERSION);

            // Reserved
            entry.extend_from_slice(&[0u8; 2]);

            // Data
            entry.extend_from_slice(&req.data);

            // Write to file
            file.seek(SeekFrom::Start(offset))
                .map_err(StorageError::Io)?;
            file.write_all(&entry).map_err(StorageError::Io)?;

            // Update offset and index
            let new_offset = offset + entry.len() as u64;
            self.write_offset.store(new_offset, Ordering::Relaxed);

            index.insert(
                req.morton,
                IndexEntry {
                    offset,
                    size: total_size,
                },
            );
        }

        file.flush().map_err(StorageError::Io)?;
        writes.clear();

        Ok(())
    }

    /// Serialize a chunk column for storage.
    fn serialize_chunk(col: &ChunkColumn) -> StorageResult<Vec<u8>> {
        let biomes = col.chunk.encode_biomes_disk();

        // Collect subchunks
        let mut subchunks = Vec::new();
        for y_index in -4..20i8 {
            if let Some(data) = col.chunk.encode_subchunk_disk(y_index as i32) {
                subchunks.push((y_index, data));
            }
        }

        // Format: biome_len(4) + Data3D biomes + subchunk_count(1) + [y(1) + len(4) + data]...
        let mut out = Vec::new();

        // Biomes
        out.extend_from_slice(&(biomes.len() as u32).to_le_bytes());
        out.extend_from_slice(&biomes);

        // Subchunks
        out.push(subchunks.len() as u8);
        for (y, data) in subchunks {
            out.push(y as u8);
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&data);
        }
        let length_offset = out.len();
        out.extend_from_slice(&0u32.to_le_bytes());
        for entity in col.chunk.block_entities() {
            entity
                .write_disk_nbt(&mut out)
                .map_err(|e| StorageError::Serialization(format!("block entity: {e}")))?;
        }
        let length = u32::try_from(out.len() - length_offset - 4)
            .map_err(|_| StorageError::Serialization("block entities too large".into()))?;
        out[length_offset..length_offset + 4].copy_from_slice(&length.to_le_bytes());

        Ok(out)
    }

    /// Deserialize a chunk column stored with format `version`.
    fn deserialize_chunk(x: i32, z: i32, version: u8, data: &[u8]) -> StorageResult<ChunkColumn> {
        if !(1..=FORMAT_VERSION).contains(&version) {
            return Err(StorageError::Deserialization(format!(
                "Unsupported BlazeDB chunk version {version}"
            )));
        }
        if data.len() < 5 {
            return Err(StorageError::Deserialization("Data too short".into()));
        }

        let mut cursor = 0;

        // Biomes
        let biome_len = u32::from_le_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;

        if biome_len > data.len() - cursor {
            return Err(StorageError::Database("Invalid biome length".to_string()));
        }
        let biomes = &data[cursor..cursor + biome_len];
        cursor += biome_len;

        // Create chunk
        let mut chunk = Chunk::new(x, z);
        if version >= DATA3D_BIOMES_VERSION {
            chunk.decode_biomes_disk(biomes).map_err(|e| {
                StorageError::Deserialization(format!("chunk ({x}, {z}) biomes: {e}"))
            })?;
        }

        // Subchunks

        if cursor == data.len() {
            return Err(StorageError::Deserialization(
                "Missing subchunk count".into(),
            ));
        }
        let subchunk_count = data[cursor] as usize;
        cursor += 1;

        for _ in 0..subchunk_count {
            if data.len() - cursor < 5 {
                return Err(StorageError::Deserialization(
                    "Truncated subchunk header".into(),
                ));
            }

            let y = data[cursor] as i8 as i32;
            cursor += 1;

            let len = u32::from_le_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;

            if len > data.len() - cursor {
                return Err(StorageError::Deserialization(
                    "Truncated subchunk data".into(),
                ));
            }

            chunk
                .decode_subchunk(y, &data[cursor..cursor + len])
                .map_err(|e| StorageError::Deserialization(format!("subchunk y={y}: {e}")))?;
            cursor += len;
        }
        if version >= 3 {
            if data.len() - cursor < 4 {
                return Err(StorageError::Deserialization(
                    "Missing block-entity length".into(),
                ));
            }
            let len = u32::from_le_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            if len != data.len() - cursor {
                return Err(StorageError::Deserialization(
                    "Invalid block-entity length".into(),
                ));
            }
            let mut input = &data[cursor..];
            while !input.is_empty() {
                let entity = BlockEntity::read_disk_nbt(&mut input)
                    .map_err(|e| StorageError::Deserialization(format!("block entity: {e}")))?;
                let position = entity.position();
                if !chunk.set_block_entity(entity) {
                    return Err(StorageError::Deserialization(format!(
                        "block entity {position:?} is outside chunk ({x}, {z})"
                    )));
                }
            }
            cursor += len;
        }
        if cursor != data.len() {
            return Err(StorageError::Deserialization("Trailing chunk data".into()));
        }

        Ok(ChunkColumn::new(chunk))
    }
}

#[async_trait]
impl WorldProvider for BlazeDBProvider {
    async fn load_column(&self, pos: ChunkPos, dim: i32) -> StorageResult<Option<ChunkColumn>> {
        let morton = morton::encode(pos.x, pos.z, dim);

        // Check cache first
        if let Some(col) = self.cache.get(morton) {
            return Ok(Some(col));
        }

        // Check index
        let entry = {
            let index = self.index.read();
            index.get(&morton).copied()
        };

        let Some(entry) = entry else {
            return Ok(None);
        };

        let this_entry = entry;

        // Clone self for the blocking task
        let data_file = self.data_file.clone();

        let result = tokio::task::spawn_blocking(move || {
            let mut file = data_file.lock();

            file.seek(SeekFrom::Start(this_entry.offset))?;

            let mut header = [0u8; 24];
            file.read_exact(&mut header)?;

            // Verify magic
            if &header[0..4] != MAGIC {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Invalid magic bytes",
                ));
            }

            let header_size = u32::from_le_bytes(header[4..8].try_into().unwrap());
            let effective_size = this_entry.size.max(header_size);
            if effective_size < MIN_ENTRY_SIZE {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Invalid chunk entry size {}", effective_size),
                ));
            }
            let file_len = file.metadata()?.len();
            let entry_end = this_entry
                .offset
                .checked_add(effective_size as u64)
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Chunk entry size overflow",
                    )
                })?;
            if entry_end > file_len {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Chunk entry end {} exceeds file length {}",
                        entry_end, file_len
                    ),
                ));
            }
            let stored_crc = u32::from_le_bytes(header[8..12].try_into().unwrap());
            let x = i32::from_le_bytes(header[12..16].try_into().unwrap());
            let z = i32::from_le_bytes(header[16..20].try_into().unwrap());
            let stored_dim = i32::from_le_bytes(header[20..24].try_into().unwrap());
            if x != pos.x || z != pos.z || stored_dim != dim {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Chunk entry coordinates do not match index",
                ));
            }

            // Read rest
            let data_size = effective_size as usize - ENTRY_HEADER_LEN;
            let mut data = vec![0u8; data_size];
            file.read_exact(&mut data)?;

            Ok((x, z, stored_crc, data))
        })
        .await
        .map_err(|e| StorageError::Database(format!("Join error: {}", e)))?
        .map_err(StorageError::Io)?;

        let (x, z, stored_crc, data) = result;
        if data.len() < 4 {
            return Err(StorageError::Database("Data too short".to_string()));
        }
        let version = data[1];
        if crc32fast::hash(&data[4..]) != stored_crc {
            return Err(StorageError::Deserialization(
                "BlazeDB chunk CRC mismatch".into(),
            ));
        }

        // Decompress
        let chunk_data = match data[0] {
            value if value == Compression::Lz4 as u8 => {
                lz4_flex::decompress_size_prepended(&data[4..])
                    .map_err(|e| StorageError::Deserialization(format!("LZ4 error: {e}")))?
            }
            value if value == Compression::None as u8 => data[4..].to_vec(),
            value => {
                return Err(StorageError::Deserialization(format!(
                    "Unsupported BlazeDB compression {value}"
                )));
            }
        };

        let col = Self::deserialize_chunk(x, z, version, &chunk_data)?;

        // Cache it
        self.cache.put(morton, col.clone());

        Ok(Some(col))
    }

    async fn save_column(&self, pos: ChunkPos, dim: i32, col: &ChunkColumn) -> StorageResult<()> {
        let morton = morton::encode(pos.x, pos.z, dim);

        for entity in col.chunk.block_entities() {
            let [x, _, z] = entity.position();
            if x.div_euclid(16) != pos.x || z.div_euclid(16) != pos.z {
                return Err(StorageError::Serialization(format!(
                    "block entity {:?} is outside chunk ({}, {})",
                    entity.position(),
                    pos.x,
                    pos.z
                )));
            }
        }
        let raw_data = Self::serialize_chunk(col)?;

        // Compress
        let compressed = if self.config.compression == Compression::Lz4 {
            lz4_flex::compress_prepend_size(&raw_data)
        } else {
            raw_data
        };

        // Send to write worker
        let req = WriteRequest {
            morton,
            data: compressed,
            x: pos.x,
            z: pos.z,
            dim,
        };

        self.write_tx
            .send(WriteCommand::Save(req))
            .map_err(|_| StorageError::Database("Write channel closed".to_string()))?;
        self.cache.put(morton, col.clone());

        Ok(())
    }

    async fn flush(&self) -> StorageResult<()> {
        let (reply, result) = oneshot::channel();
        self.write_tx
            .send(WriteCommand::Flush(reply))
            .map_err(|_| StorageError::Database("Write channel closed".into()))?;
        result
            .await
            .map_err(|_| StorageError::Database("Write worker stopped".into()))?
    }

    async fn close(&self) -> StorageResult<()> {
        let (reply, result) = oneshot::channel();
        self.write_tx
            .send(WriteCommand::Close(reply))
            .map_err(|_| StorageError::Database("Write channel closed".into()))?;
        result
            .await
            .map_err(|_| StorageError::Database("Write worker stopped".into()))?
    }
}

impl Drop for BlazeDBProvider {
    fn drop(&mut self) {
        // Best-effort flush
        if let Err(e) = self.save_index() {
            error!("Failed to save index on drop: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db_dir(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "unastar-blazedb-{}-{}-{}",
            test_name,
            std::process::id(),
            unique
        ))
    }

    fn test_config() -> BlazeConfig {
        BlazeConfig {
            cache_capacity: 16,
            compression: Compression::None,
            flush_interval_ms: 1,
        }
    }

    fn test_column(pos: ChunkPos) -> ChunkColumn {
        let mut chunk = Chunk::new(pos.x, pos.z);
        chunk
            .set_block(1, 0, 2, 42)
            .expect("test block position is in bounds");
        ChunkColumn::new(chunk)
    }

    fn write_legacy_short_entry(dir: &Path, pos: ChunkPos, dim: i32) -> Vec<u8> {
        std::fs::create_dir_all(dir).expect("create legacy db dir");
        let mut raw_data =
            BlazeDBProvider::serialize_chunk(&test_column(pos)).expect("serialize chunk");
        raw_data.truncate(raw_data.len() - 4);
        let morton = morton::encode(pos.x, pos.z, dim);
        let legacy_size = (ENTRY_HEADER_LEN + raw_data.len()) as u32;

        let mut entry =
            Vec::with_capacity(ENTRY_HEADER_LEN + ENTRY_DATA_PREFIX_LEN + raw_data.len());
        entry.extend_from_slice(MAGIC);
        entry.extend_from_slice(&legacy_size.to_le_bytes());
        entry.extend_from_slice(&crc32fast::hash(&raw_data).to_le_bytes());
        entry.extend_from_slice(&pos.x.to_le_bytes());
        entry.extend_from_slice(&pos.z.to_le_bytes());
        entry.extend_from_slice(&dim.to_le_bytes());
        entry.push(Compression::None as u8);
        entry.push(DATA3D_BIOMES_VERSION);
        entry.extend_from_slice(&[0u8; 2]);
        entry.extend_from_slice(&raw_data);
        std::fs::write(dir.join("chunks.dat"), entry).expect("write legacy chunks.dat");

        let mut index = Vec::new();
        index.extend_from_slice(&morton.to_le_bytes());
        index.extend_from_slice(&0u64.to_le_bytes());
        index.extend_from_slice(&legacy_size.to_le_bytes());
        std::fs::write(dir.join("index.dat"), index).expect("write legacy index.dat");

        raw_data
    }

    #[tokio::test]
    async fn entry_size_includes_compression_prefix() {
        let dir = temp_db_dir("entry-size");
        let provider = BlazeDBProvider::open(&dir, Some(test_config())).expect("open blazedb");
        let pos = ChunkPos { x: 3, z: -7 };
        let dim = 0;
        let raw_data =
            BlazeDBProvider::serialize_chunk(&test_column(pos)).expect("serialize chunk");
        let morton = morton::encode(pos.x, pos.z, dim);
        let mut writes = vec![WriteRequest {
            morton,
            data: raw_data.clone(),
            x: pos.x,
            z: pos.z,
            dim,
        }];

        provider.flush_writes(&mut writes).expect("flush write");

        let data = std::fs::read(dir.join("chunks.dat")).expect("read chunks.dat");
        let stored_size = u32::from_le_bytes(data[4..8].try_into().expect("size bytes")) as usize;

        assert_eq!(stored_size, data.len());
        assert_eq!(
            stored_size,
            ENTRY_HEADER_LEN + ENTRY_DATA_PREFIX_LEN + raw_data.len()
        );
        assert_eq!(
            provider
                .index
                .read()
                .get(&morton)
                .expect("index entry")
                .size as usize,
            data.len()
        );

        drop(provider);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn saved_column_loads_after_reopen() {
        let dir = temp_db_dir("reopen-load");
        let pos = ChunkPos { x: -2, z: 5 };
        let dim = 0;
        {
            let provider = BlazeDBProvider::open(&dir, Some(test_config())).expect("open blazedb");
            let raw_data =
                BlazeDBProvider::serialize_chunk(&test_column(pos)).expect("serialize chunk");
            let mut writes = vec![WriteRequest {
                morton: morton::encode(pos.x, pos.z, dim),
                data: raw_data,
                x: pos.x,
                z: pos.z,
                dim,
            }];
            provider.flush_writes(&mut writes).expect("flush write");
            provider.save_index().expect("save index");
        }

        let provider = BlazeDBProvider::open(&dir, Some(test_config())).expect("reopen blazedb");
        let loaded = provider
            .load_column(pos, dim)
            .await
            .expect("load column")
            .expect("saved column exists");

        assert_eq!(loaded.chunk.x, pos.x);
        assert_eq!(loaded.chunk.z, pos.z);
        assert_eq!(loaded.chunk.get_block(1, 0, 2), 42);

        drop(provider);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn version_1_network_biomes_are_skipped_on_load() {
        let mut chunk = Chunk::new(0, 0);
        chunk.set_biome(7);
        let network = chunk.encode_biomes();
        let mut data = (network.len() as u32).to_le_bytes().to_vec();
        data.extend_from_slice(&network);
        data.push(0);

        let col = BlazeDBProvider::deserialize_chunk(0, 0, 1, &data).expect("version 1 entry");
        assert_eq!(
            col.chunk.biome_at(0, 0, 0),
            Some(crate::world::chunk::DEFAULT_BIOME_ID)
        );
        assert!(BlazeDBProvider::deserialize_chunk(0, 0, FORMAT_VERSION, &data).is_err());
    }

    #[test]
    fn version_2_data3d_entries_remain_readable_without_metadata() {
        let mut column = test_column(ChunkPos::new(-2, 5));
        column.chunk.set_biome(7);
        let mut data = BlazeDBProvider::serialize_chunk(&column).expect("serialize");
        data.truncate(data.len() - 4); // Version 2 ends after the subchunk records.
        let restored = BlazeDBProvider::deserialize_chunk(-2, 5, 2, &data).expect("version 2");
        assert_eq!(restored.chunk.get_block(1, 0, 2), 42);
        assert_eq!(restored.chunk.biome_at(0, 0, 0), Some(7));
        assert!(restored.chunk.block_entities().is_empty());
        data.push(0);
        assert!(BlazeDBProvider::deserialize_chunk(-2, 5, 2, &data).is_err());
    }

    #[test]
    fn malformed_block_entity_payloads_fail_without_partial_chunk_fallback() {
        let column = test_column(ChunkPos::new(0, 0));
        let empty = BlazeDBProvider::serialize_chunk(&column).expect("serialize");
        let length_offset = empty.len() - 4;
        let actor = BlockEntity::generated([1, 70, 2], "Chest", &[], None, None);
        let mut nbt = Vec::new();
        actor.write_disk_nbt(&mut nbt).expect("disk nbt");
        let payload = |bytes: &[u8]| {
            let mut data = empty[..length_offset].to_vec();
            data.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            data.extend_from_slice(bytes);
            data
        };
        let valid = payload(&nbt);
        let restored =
            BlazeDBProvider::deserialize_chunk(0, 0, FORMAT_VERSION, &valid).expect("valid actor");
        assert_eq!(restored.chunk.block_entities()[0].position(), [1, 70, 2]);
        for bytes in [&nbt[..nbt.len() - 1], &[0xff][..]] {
            assert!(
                BlazeDBProvider::deserialize_chunk(0, 0, FORMAT_VERSION, &payload(bytes)).is_err()
            );
        }
        for position in [[16, 70, 2], [1, 320, 2], [1, -65, 2]] {
            let actor = BlockEntity::generated(position, "Chest", &[], None, None);
            let mut bytes = Vec::new();
            actor.write_disk_nbt(&mut bytes).expect("disk nbt");
            assert!(
                BlazeDBProvider::deserialize_chunk(0, 0, FORMAT_VERSION, &payload(&bytes)).is_err()
            );
        }
        let mut trailing = valid.clone();
        trailing.push(0);
        assert!(BlazeDBProvider::deserialize_chunk(0, 0, FORMAT_VERSION, &trailing).is_err());
        for len in [length_offset, length_offset + 1, valid.len() - 1] {
            assert!(
                BlazeDBProvider::deserialize_chunk(0, 0, FORMAT_VERSION, &valid[..len]).is_err()
            );
        }
        assert!(BlazeDBProvider::deserialize_chunk(0, 0, FORMAT_VERSION + 1, &valid).is_err());
    }

    #[tokio::test]
    async fn legacy_short_size_index_is_repaired_on_open() {
        let dir = temp_db_dir("legacy-short-index");
        let pos = ChunkPos { x: 8, z: -3 };
        let dim = 0;
        let raw_data = write_legacy_short_entry(&dir, pos, dim);

        let provider = BlazeDBProvider::open(&dir, Some(test_config())).expect("open blazedb");
        let morton = morton::encode(pos.x, pos.z, dim);
        assert_eq!(
            provider
                .index
                .read()
                .get(&morton)
                .expect("index entry")
                .size as usize,
            ENTRY_HEADER_LEN + ENTRY_DATA_PREFIX_LEN + raw_data.len()
        );
        let loaded = provider
            .load_column(pos, dim)
            .await
            .expect("load legacy column")
            .expect("legacy column exists");

        assert_eq!(loaded.chunk.x, pos.x);
        assert_eq!(loaded.chunk.z, pos.z);
        assert_eq!(loaded.chunk.get_block(1, 0, 2), 42);

        drop(provider);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn legacy_short_size_entry_is_repaired_during_index_rebuild() {
        let dir = temp_db_dir("legacy-short-rebuild");
        let pos = ChunkPos { x: -9, z: 4 };
        let dim = 0;
        let raw_data = write_legacy_short_entry(&dir, pos, dim);
        std::fs::remove_file(dir.join("index.dat")).expect("remove stale index");

        let provider = BlazeDBProvider::open(&dir, Some(test_config())).expect("open blazedb");
        let morton = morton::encode(pos.x, pos.z, dim);
        assert_eq!(
            provider
                .index
                .read()
                .get(&morton)
                .expect("rebuilt index entry")
                .size as usize,
            ENTRY_HEADER_LEN + ENTRY_DATA_PREFIX_LEN + raw_data.len()
        );
        let loaded = provider
            .load_column(pos, dim)
            .await
            .expect("load rebuilt legacy column")
            .expect("legacy column exists");

        assert_eq!(loaded.chunk.x, pos.x);
        assert_eq!(loaded.chunk.z, pos.z);
        assert_eq!(loaded.chunk.get_block(1, 0, 2), 42);

        drop(provider);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn oversized_index_entry_is_dropped_before_read_allocation() {
        let dir = temp_db_dir("oversized-index");
        let pos = ChunkPos { x: 6, z: 6 };
        let dim = 0;
        write_legacy_short_entry(&dir, pos, dim);

        let morton = morton::encode(pos.x, pos.z, dim);
        let mut index = Vec::new();
        index.extend_from_slice(&morton.to_le_bytes());
        index.extend_from_slice(&0u64.to_le_bytes());
        index.extend_from_slice(&u32::MAX.to_le_bytes());
        std::fs::write(dir.join("index.dat"), index).expect("write poisoned index.dat");

        let provider = BlazeDBProvider::open(&dir, Some(test_config())).expect("open blazedb");

        assert!(!provider.index.read().contains_key(&morton));
        assert!(
            provider
                .load_column(pos, dim)
                .await
                .expect("oversized entry should be dropped")
                .is_none()
        );

        drop(provider);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn public_lz4_save_column_loads_after_reopen() {
        let dir = temp_db_dir("public-lz4-roundtrip");
        let pos = ChunkPos { x: 11, z: 12 };
        let dim = 0;
        let config = BlazeConfig {
            cache_capacity: 16,
            compression: Compression::Lz4,
            flush_interval_ms: 1,
        };
        {
            let provider = BlazeDBProvider::open(&dir, Some(config.clone())).expect("open blazedb");
            provider
                .save_column(pos, dim, &test_column(pos))
                .await
                .expect("save column");

            let morton = morton::encode(pos.x, pos.z, dim);
            for _ in 0..50 {
                if provider.index.read().contains_key(&morton) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            assert!(
                provider.index.read().contains_key(&morton),
                "background writer should flush saved column"
            );
            provider.flush().await.expect("flush provider");
        }

        let provider = BlazeDBProvider::open(&dir, Some(config)).expect("reopen blazedb");
        let loaded = provider
            .load_column(pos, dim)
            .await
            .expect("load lz4 column")
            .expect("saved lz4 column exists");

        assert_eq!(loaded.chunk.x, pos.x);
        assert_eq!(loaded.chunk.z, pos.z);
        assert_eq!(loaded.chunk.get_block(1, 0, 2), 42);

        drop(provider);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
