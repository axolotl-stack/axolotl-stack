//! Level data: everything a world keeps outside its chunks and players, in
//! BDS 1.26.51's layout so worlds move between BDS and unastar.
//!
//! - [`dat`]: the `level.dat` file in the world directory (settings, clock,
//!   difficulty, game rules, world spawn, weather).
//! - [`records`]: the world database's level records (`scoreboard`,
//!   `mobevents`, `tickingarea_*`, `structuretemplate_*`, `WorldClocks`).
//! - [`LevelRecordStore`]: reads and writes those records through the
//!   world's provider (LevelDB keys, as BDS; BlazeDB keeps them in a
//!   checked side file).
//! - [`LevelFiles`]: `level.dat`, `level.dat_old` and `levelname.txt` in the
//!   world directory.
//!
//! Which ECS state each value feeds is in `crate::world::level_data`.

pub mod dat;
pub mod nbt;
pub mod records;

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use bleveldb::{ReadOptions, WriteBatch, WriteOptions};
use tokio::task;

use super::provider::{StorageError, StorageResult};
use super::{BlazeDBProvider, LevelDBWorldProvider};
use dat::{LevelDat, LevelDatError};

/// A raw level record: key and value.
pub type RawRecord = (Vec<u8>, Vec<u8>);

/// Level-data records of a world database.
#[async_trait]
pub trait LevelRecordStore: Send + Sync + 'static {
    /// Every record [`records::is_level_record`] names, in key order.
    async fn load_level_records(&self) -> StorageResult<Vec<RawRecord>>;

    /// Writes `puts` and removes `deletes`, all at once.
    async fn save_level_records(
        &self,
        puts: Vec<RawRecord>,
        deletes: Vec<Vec<u8>>,
    ) -> StorageResult<()>;
}

fn join_error(error: task::JoinError) -> StorageError {
    StorageError::Database(format!("Join error: {error}"))
}

#[async_trait]
impl LevelRecordStore for LevelDBWorldProvider {
    async fn load_level_records(&self) -> StorageResult<Vec<RawRecord>> {
        let db = self.database();
        task::spawn_blocking(move || {
            let read = ReadOptions::new();
            let mut out = Vec::new();
            for key in records::SINGLE_KEYS {
                if let Some(value) = db.get(key, &read).map_err(StorageError::Database)? {
                    out.push((key.to_vec(), value));
                }
            }
            for prefix in records::PREFIXES {
                let mut iter = db.iter(&read);
                iter.seek(prefix);
                while iter.valid() {
                    let (Some(key), Some(value)) = (iter.key(), iter.value()) else {
                        break;
                    };
                    if !key.starts_with(prefix) {
                        break;
                    }
                    out.push((key, value));
                    iter.next_native();
                }
            }
            out.sort();
            Ok(out)
        })
        .await
        .map_err(join_error)?
    }

    async fn save_level_records(
        &self,
        puts: Vec<RawRecord>,
        deletes: Vec<Vec<u8>>,
    ) -> StorageResult<()> {
        let db = self.database();
        task::spawn_blocking(move || {
            let mut batch = WriteBatch::new();
            for key in &deletes {
                batch.delete(key);
            }
            for (key, value) in &puts {
                batch.put(key, value);
            }
            batch
                .write(&db, &WriteOptions::new())
                .map_err(StorageError::Database)
        })
        .await
        .map_err(join_error)?
    }
}

/// BlazeDB has no key-value records, so its level records live in one side
/// file in the database directory: `ULVL`, a version, the record count, each
/// record as `u32` key length, key, `u32` value length, value, and a CRC32 of
/// everything before it. A file that fails its checks is an error, never an
/// empty set of records.
const SIDE_FILE: &str = "level_records.dat";
const SIDE_MAGIC: &[u8; 4] = b"ULVL";
const SIDE_VERSION: u32 = 1;

fn side_error(text: &str) -> StorageError {
    StorageError::Deserialization(format!("{SIDE_FILE}: {text}"))
}

fn decode_side_file(bytes: &[u8]) -> StorageResult<Vec<RawRecord>> {
    if bytes.len() < 16 || &bytes[..4] != SIDE_MAGIC {
        return Err(side_error("not a level record file"));
    }
    let (body, crc) = bytes.split_at(bytes.len() - 4);
    if crc32fast::hash(body).to_le_bytes() != crc {
        return Err(side_error("checksum mismatch"));
    }
    let mut at = 4;
    let take = |at: &mut usize, len: usize| -> StorageResult<&[u8]> {
        let end = at
            .checked_add(len)
            .filter(|end| *end <= body.len())
            .ok_or_else(|| side_error("truncated"))?;
        let value = &body[*at..end];
        *at = end;
        Ok(value)
    };
    let u32_at = |at: &mut usize| -> StorageResult<u32> {
        Ok(u32::from_le_bytes(
            take(at, 4)?.try_into().expect("4 bytes"),
        ))
    };
    if u32_at(&mut at)? != SIDE_VERSION {
        return Err(side_error("unknown version"));
    }
    let count = u32_at(&mut at)?;
    let mut out = Vec::new();
    for _ in 0..count {
        let len = u32_at(&mut at)? as usize;
        let key = take(&mut at, len)?.to_vec();
        let len = u32_at(&mut at)? as usize;
        let value = take(&mut at, len)?.to_vec();
        out.push((key, value));
    }
    if at != body.len() {
        return Err(side_error("trailing bytes"));
    }
    Ok(out)
}

fn encode_side_file(records: &[RawRecord]) -> Vec<u8> {
    let mut out = SIDE_MAGIC.to_vec();
    out.extend_from_slice(&SIDE_VERSION.to_le_bytes());
    out.extend_from_slice(&(records.len() as u32).to_le_bytes());
    for (key, value) in records {
        out.extend_from_slice(&(key.len() as u32).to_le_bytes());
        out.extend_from_slice(key);
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value);
    }
    let crc = crc32fast::hash(&out);
    out.extend_from_slice(&crc.to_le_bytes());
    out
}

/// Replaces `path` with `bytes` through a temporary file in the same
/// directory, so a crash leaves the old file or the new one.
fn replace_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, path)
}

fn read_side_file(path: &Path) -> StorageResult<Vec<RawRecord>> {
    match std::fs::read(path) {
        Ok(bytes) => decode_side_file(&bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(StorageError::Io(error)),
    }
}

#[async_trait]
impl LevelRecordStore for BlazeDBProvider {
    async fn load_level_records(&self) -> StorageResult<Vec<RawRecord>> {
        let path = self.directory().join(SIDE_FILE);
        task::spawn_blocking(move || read_side_file(&path))
            .await
            .map_err(join_error)?
    }

    async fn save_level_records(
        &self,
        puts: Vec<RawRecord>,
        deletes: Vec<Vec<u8>>,
    ) -> StorageResult<()> {
        let path = self.directory().join(SIDE_FILE);
        task::spawn_blocking(move || {
            let mut records = read_side_file(&path)?;
            records.retain(|(key, _)| {
                !deletes.contains(key) && !puts.iter().any(|(put, _)| put == key)
            });
            records.extend(puts);
            records.sort();
            replace_file(&path, &encode_side_file(&records)).map_err(StorageError::Io)
        })
        .await
        .map_err(join_error)?
    }
}

/// The world directory's level files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelFiles {
    pub directory: PathBuf,
}

/// Why the world's level.dat could not be read.
#[derive(Debug, thiserror::Error)]
pub enum LevelFileError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Format {
        path: PathBuf,
        source: LevelDatError,
    },
}

impl LevelFiles {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    pub fn level_dat(&self) -> PathBuf {
        self.directory.join("level.dat")
    }

    /// The world's level.dat, `Ok(None)` for a world without one (a new
    /// world). An unreadable file is an error; it is never replaced.
    pub fn read(&self) -> Result<Option<LevelDat>, LevelFileError> {
        let path = self.level_dat();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(LevelFileError::Io { path, source }),
        };
        LevelDat::read(&bytes)
            .map(Some)
            .map_err(|source| LevelFileError::Format { path, source })
    }

    /// Writes level.dat as BDS keeps it: the previous file becomes
    /// `level.dat_old`, the name goes to `levelname.txt`.
    pub fn write(&self, dat: &LevelDat, level_name: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.directory)?;
        let path = self.level_dat();
        if path.exists() {
            std::fs::copy(&path, self.directory.join("level.dat_old"))?;
        }
        replace_file(&path, &dat.write())?;
        std::fs::write(self.directory.join("levelname.txt"), level_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_files_round_trip_and_fail_their_checks() {
        let records = vec![
            (b"scoreboard".to_vec(), vec![1, 2, 3]),
            (b"tickingarea_x".to_vec(), Vec::new()),
        ];
        let bytes = encode_side_file(&records);
        assert_eq!(decode_side_file(&bytes).unwrap(), records);
        let mut flipped = bytes.clone();
        flipped[12] ^= 1;
        assert!(decode_side_file(&flipped).is_err());
        assert!(decode_side_file(&bytes[..10]).is_err());
    }

    #[tokio::test]
    async fn leveldb_keeps_only_level_records() {
        let dir = tempfile::tempdir().unwrap();
        let provider = LevelDBWorldProvider::open(dir.path(), 0).unwrap();
        provider
            .save_level_records(
                vec![
                    (b"scoreboard".to_vec(), vec![1]),
                    (b"tickingarea_b".to_vec(), vec![2]),
                    (b"tickingarea_a".to_vec(), vec![3]),
                    (b"structuretemplate_mystructure:s".to_vec(), vec![4]),
                    (b"schedulerWT".to_vec(), vec![5]),
                    (b"tickingareb".to_vec(), vec![6]),
                ],
                Vec::new(),
            )
            .await
            .unwrap();
        let keys: Vec<Vec<u8>> = provider
            .load_level_records()
            .await
            .unwrap()
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        assert_eq!(
            keys,
            [
                b"scoreboard".to_vec(),
                b"structuretemplate_mystructure:s".to_vec(),
                b"tickingarea_a".to_vec(),
                b"tickingarea_b".to_vec(),
            ]
        );
        provider
            .save_level_records(Vec::new(), vec![b"tickingarea_a".to_vec()])
            .await
            .unwrap();
        assert_eq!(provider.load_level_records().await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn blazedb_keeps_level_records_in_a_side_file() {
        let dir = tempfile::tempdir().unwrap();
        let provider = BlazeDBProvider::open(dir.path(), None).unwrap();
        provider
            .save_level_records(
                vec![
                    (b"mobevents".to_vec(), vec![7]),
                    (b"scoreboard".to_vec(), vec![8]),
                ],
                Vec::new(),
            )
            .await
            .unwrap();
        provider
            .save_level_records(
                vec![(b"scoreboard".to_vec(), vec![9])],
                vec![b"mobevents".to_vec()],
            )
            .await
            .unwrap();
        assert_eq!(
            provider.load_level_records().await.unwrap(),
            [(b"scoreboard".to_vec(), vec![9])]
        );
        std::fs::write(dir.path().join(SIDE_FILE), b"garbage").unwrap();
        assert!(provider.load_level_records().await.is_err());
    }
}
