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
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use super::cache::ShardedCache;
use super::morton;
use super::provider::{ChunkColumn, StorageError, StorageResult, WorldProvider};
use crate::world::{Chunk, ChunkPos};

/// Magic bytes at start of each chunk entry.
const MAGIC: &[u8; 4] = b"BLAZ";

/// Current format version.
const FORMAT_VERSION: u8 = 1;

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
            cache_capacity: 4096,
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
    write_tx: mpsc::UnboundedSender<WriteRequest>,
    /// Shutdown flag.
    shutdown: Arc<std::sync::atomic::AtomicBool>,
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
        std::fs::create_dir_all(&path).map_err(|e| StorageError::Io(e))?;

        let data_path = path.join("chunks.dat");
        let index_path = path.join("index.dat");

        // Open or create data file
        let data_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&data_path)
            .map_err(|e| StorageError::Io(e))?;

        let write_offset = data_file.metadata().map(|m| m.len()).unwrap_or(0);

        // Try to load index from file, or rebuild from data
        let index = if index_path.exists() {
            Self::load_index(&index_path).unwrap_or_else(|e| {
                warn!("Failed to load index, rebuilding: {}", e);
                Self::rebuild_index(&data_file).unwrap_or_default()
            })
        } else {
            Self::rebuild_index(&data_file).unwrap_or_default()
        };

        info!(
            "BlazeDB opened: {} chunks in index, {} bytes on disk",
            index.len(),
            write_offset
        );

        // Create cache
        let cache = ShardedCache::new(config.cache_capacity);

        // Create write channel
        let (write_tx, write_rx) = mpsc::unbounded_channel();
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let provider = Arc::new(Self {
            path,
            data_file: Arc::new(Mutex::new(data_file)),
            write_offset: AtomicU64::new(write_offset),
            index: RwLock::new(index),
            cache,
            config,
            write_tx,
            shutdown: shutdown.clone(),
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
        let mut file = File::open(path).map_err(|e| StorageError::Io(e))?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .map_err(|e| StorageError::Io(e))?;

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
        let mut file = file.try_clone().map_err(|e| StorageError::Io(e))?;
        let mut index = HashMap::new();

        file.seek(SeekFrom::Start(0))
            .map_err(|e| StorageError::Io(e))?;

        loop {
            let offset = file.stream_position().map_err(|e| StorageError::Io(e))?;

            // Read header
            let mut header = [0u8; 4];
            match file.read_exact(&mut header) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(StorageError::Io(e)),
            }

            if &header != MAGIC {
                warn!("Invalid magic at offset {}, stopping index rebuild", offset);
                break;
            }

            // Read size
            let mut size_buf = [0u8; 4];
            file.read_exact(&mut size_buf)
                .map_err(|e| StorageError::Io(e))?;
            let size = u32::from_le_bytes(size_buf);

            // Skip CRC
            file.seek(SeekFrom::Current(4))
                .map_err(|e| StorageError::Io(e))?;

            // Read coordinates
            let mut x_buf = [0u8; 4];
            let mut z_buf = [0u8; 4];
            let mut dim_buf = [0u8; 4];
            file.read_exact(&mut x_buf)
                .map_err(|e| StorageError::Io(e))?;
            file.read_exact(&mut z_buf)
                .map_err(|e| StorageError::Io(e))?;
            file.read_exact(&mut dim_buf)
                .map_err(|e| StorageError::Io(e))?;

            let x = i32::from_le_bytes(x_buf);
            let z = i32::from_le_bytes(z_buf);
            let dim = i32::from_le_bytes(dim_buf);

            let morton = morton::encode(x, z, dim);
            index.insert(morton, IndexEntry { offset, size });

            // Skip to next entry (size includes header, so subtract what we read)
            let remaining = size as i64 - 20; // 4 + 4 + 4 + 4 + 4 + 4 - 4 (magic already counted)
            if remaining > 0 {
                file.seek(SeekFrom::Current(remaining))
                    .map_err(|e| StorageError::Io(e))?;
            }
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

        std::fs::write(&index_path, &data).map_err(|e| StorageError::Io(e))?;
        Ok(())
    }

    /// Background write worker.
    async fn write_worker(provider: Arc<Self>, mut rx: mpsc::UnboundedReceiver<WriteRequest>) {
        let mut pending_writes: Vec<WriteRequest> = Vec::new();
        let mut last_flush = std::time::Instant::now();

        loop {
            // Try to receive with timeout for batching
            match tokio::time::timeout(
                std::time::Duration::from_millis(provider.config.flush_interval_ms),
                rx.recv(),
            )
            .await
            {
                Ok(Some(req)) => {
                    pending_writes.push(req);
                }
                Ok(None) => {
                    // Channel closed, flush and exit
                    if !pending_writes.is_empty() {
                        if let Err(e) = provider.flush_writes(&mut pending_writes) {
                            error!("Error flushing writes on shutdown: {}", e);
                        }
                    }
                    break;
                }
                Err(_) => {
                    // Timeout - flush if we have pending writes
                    if !pending_writes.is_empty()
                        && last_flush.elapsed().as_millis()
                            >= provider.config.flush_interval_ms as u128
                    {
                        if let Err(e) = provider.flush_writes(&mut pending_writes) {
                            error!("Error flushing writes: {}", e);
                        }
                        last_flush = std::time::Instant::now();
                    }
                }
            }

            if provider.shutdown.load(Ordering::Relaxed) {
                if !pending_writes.is_empty() {
                    let _ = provider.flush_writes(&mut pending_writes);
                }
                break;
            }
        }

        debug!("BlazeDB write worker shut down");
    }

    /// Flush pending writes to disk.
    fn flush_writes(&self, writes: &mut Vec<WriteRequest>) -> StorageResult<()> {
        if writes.is_empty() {
            return Ok(());
        }

        let mut file = self.data_file.lock();
        let mut index = self.index.write();

        for req in writes.drain(..) {
            let offset = self.write_offset.load(Ordering::Relaxed);

            // Build entry
            let mut entry = Vec::with_capacity(24 + req.data.len());
            entry.extend_from_slice(MAGIC);

            let total_size = 24 + req.data.len() as u32; // Header + data
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
                .map_err(|e| StorageError::Io(e))?;
            file.write_all(&entry).map_err(|e| StorageError::Io(e))?;

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

        file.flush().map_err(|e| StorageError::Io(e))?;

        Ok(())
    }

    /// Read a chunk from disk at the given offset.
    fn read_chunk_at(&self, entry: IndexEntry) -> StorageResult<ChunkColumn> {
        let mut file = self.data_file.lock();

        file.seek(SeekFrom::Start(entry.offset))
            .map_err(|e| StorageError::Io(e))?;

        let mut header = [0u8; 24];
        file.read_exact(&mut header)
            .map_err(|e| StorageError::Io(e))?;

        // Verify magic
        if &header[0..4] != MAGIC {
            return Err(StorageError::Database("Invalid magic bytes".to_string()));
        }

        let size = u32::from_le_bytes(header[4..8].try_into().unwrap());
        let stored_crc = u32::from_le_bytes(header[8..12].try_into().unwrap());
        let x = i32::from_le_bytes(header[12..16].try_into().unwrap());
        let z = i32::from_le_bytes(header[16..20].try_into().unwrap());
        let _dim = i32::from_le_bytes(header[20..24].try_into().unwrap());

        // Read data
        let data_size = size as usize - 24;
        let mut data = vec![0u8; data_size];
        file.read_exact(&mut data)
            .map_err(|e| StorageError::Io(e))?;

        // Skip compression byte and version in the data
        if data.len() < 4 {
            return Err(StorageError::Database("Data too short".to_string()));
        }

        let compression = data[0];
        let _version = data[1];
        let chunk_data = &data[4..]; // Skip compression, version, reserved

        // Verify CRC
        let computed_crc = crc32fast::hash(chunk_data);
        if computed_crc != stored_crc {
            warn!(
                "CRC mismatch for chunk ({}, {}): expected {:08x}, got {:08x}",
                x, z, stored_crc, computed_crc
            );
        }

        // Decompress if needed
        let decompressed = if compression == Compression::Lz4 as u8 {
            lz4_flex::decompress_size_prepended(chunk_data)
                .map_err(|e| StorageError::Database(format!("LZ4 decompress error: {}", e)))?
        } else {
            chunk_data.to_vec()
        };

        // Deserialize chunk
        Self::deserialize_chunk(x, z, &decompressed)
    }

    /// Serialize a chunk column for storage.
    fn serialize_chunk(col: &ChunkColumn) -> Vec<u8> {
        let biomes = col.chunk.encode_biomes();

        // Collect subchunks
        let mut subchunks = Vec::new();
        for y_index in -4..20i8 {
            if let Some(data) = col.chunk.encode_subchunk(y_index as i32) {
                subchunks.push((y_index, data));
            }
        }

        // Format: biome_len(4) + biomes + subchunk_count(1) + [y(1) + len(4) + data]...
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

        out
    }

    /// Deserialize a chunk column from storage.
    fn deserialize_chunk(x: i32, z: i32, data: &[u8]) -> StorageResult<ChunkColumn> {
        if data.len() < 5 {
            return Err(StorageError::Database("Data too short".to_string()));
        }

        let mut cursor = 0;

        // Biomes
        let biome_len = u32::from_le_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;

        if cursor + biome_len > data.len() {
            return Err(StorageError::Database("Invalid biome length".to_string()));
        }
        let _biomes = &data[cursor..cursor + biome_len];
        cursor += biome_len;

        // Create chunk
        let mut chunk = Chunk::new(x, z);

        // Subchunks
        if cursor >= data.len() {
            return Ok(ChunkColumn::new(chunk));
        }

        let subchunk_count = data[cursor] as usize;
        cursor += 1;

        for _ in 0..subchunk_count {
            if cursor + 5 > data.len() {
                break;
            }

            let y = data[cursor] as i8 as i32;
            cursor += 1;

            let len = u32::from_le_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;

            if cursor + len > data.len() {
                break;
            }

            let subchunk_data = &data[cursor..cursor + len];
            if let Err(e) = chunk.decode_subchunk(y, subchunk_data) {
                warn!("Failed to decode subchunk y={}: {}", y, e);
            }
            cursor += len;
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

        // Read from disk (blocking, so spawn_blocking)
        let provider = self.data_file.clone();
        let this_entry = entry;

        // Clone self for the blocking task
        let data_file = self.data_file.clone();
        let config_compression = self.config.compression;

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

            let size = u32::from_le_bytes(header[4..8].try_into().unwrap());
            let _stored_crc = u32::from_le_bytes(header[8..12].try_into().unwrap());
            let x = i32::from_le_bytes(header[12..16].try_into().unwrap());
            let z = i32::from_le_bytes(header[16..20].try_into().unwrap());

            // Read rest
            let data_size = size as usize - 24;
            let mut data = vec![0u8; data_size];
            file.read_exact(&mut data)?;

            Ok((x, z, data))
        })
        .await
        .map_err(|e| StorageError::Database(format!("Join error: {}", e)))?
        .map_err(|e| StorageError::Io(e))?;

        let (x, z, data) = result;

        // Decompress
        let chunk_data = if data.len() >= 4 && data[0] == Compression::Lz4 as u8 {
            lz4_flex::decompress_size_prepended(&data[4..])
                .map_err(|e| StorageError::Database(format!("LZ4 error: {}", e)))?
        } else if data.len() >= 4 {
            data[4..].to_vec()
        } else {
            return Err(StorageError::Database("Data too short".to_string()));
        };

        let col = Self::deserialize_chunk(x, z, &chunk_data)?;

        // Cache it
        self.cache.put(morton, col.clone());

        Ok(Some(col))
    }

    async fn save_column(&self, pos: ChunkPos, dim: i32, col: &ChunkColumn) -> StorageResult<()> {
        let morton = morton::encode(pos.x, pos.z, dim);

        // Update cache
        self.cache.put(morton, col.clone());

        // Serialize
        let raw_data = Self::serialize_chunk(col);

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
            .send(req)
            .map_err(|_| StorageError::Database("Write channel closed".to_string()))?;

        Ok(())
    }

    async fn flush(&self) -> StorageResult<()> {
        // Sync data file
        {
            let file = self.data_file.lock();
            file.sync_all().map_err(|e| StorageError::Io(e))?;
        }

        // Save index
        self.save_index()?;

        Ok(())
    }

    async fn close(&self) -> StorageResult<()> {
        self.shutdown.store(true, Ordering::Relaxed);
        self.flush().await
    }
}

impl Drop for BlazeDBProvider {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        // Best-effort flush
        if let Err(e) = self.save_index() {
            error!("Failed to save index on drop: {}", e);
        }
    }
}
