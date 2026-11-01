//! Serialized, bounded whole-column persistence and durable effect receipts.
//!
//! Capture stamps belong to one provider instance. Ordinary snapshots retain
//! their original epoch and ordering; they cannot adopt a later durable epoch.
//! The native runtime owns publication and pins separately from disk completion.

mod fingerprint;
pub(crate) mod leveldb;
#[cfg(test)]
mod tests;

use super::{ChunkColumn, StorageError, StorageResult};
use crate::world::ChunkPos;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    mpsc,
};
use tokio::sync::oneshot;

pub(crate) use leveldb::LevelStore;

/// Maximum columns and encoded bytes in one atomic write.
pub const MAX_COLUMNS: usize = 16;
pub const MAX_WRITE_BYTES: usize = 16 * 1024 * 1024;
const MAX_PENDING_BYTES: usize = 32 * 1024 * 1024;
const MAX_PENDING_WRITES: usize = 16;

/// Immutable provider-scoped witness captured before copying native state.
#[derive(Clone, Copy, Debug)]
pub struct ColumnStamp {
    owner: uuid::Uuid,
    epoch: u64,
    sequence: u64,
}
impl ColumnStamp {
    /// The durable-effect epoch observed by this capture.
    pub fn epoch(self) -> u64 {
        self.epoch
    }
}

/// One whole column in an atomic set; dimensions share the provider's epoch.
#[derive(Clone, Debug)]
pub struct ColumnWrite {
    pub position: ChunkPos,
    pub dimension: i32,
    pub column: ChunkColumn,
}

/// A durable effect's immutable binding and restart-visible result.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnReceipt {
    pub id: String,
    pub hash: [u8; 32],
    pub epoch: u64,
    pub outcome: ReceiptOutcome,
}
/// Whether restart reconciliation proves a final atomic write was present.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ReceiptOutcome {
    Applied,
    NotApplied,
}

/// A known rejection is distinct from an ambiguous storage result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ColumnWriteError {
    #[error("provider does not support atomic column writes")]
    Unsupported,
    #[error("column write exceeds its bounded limits or is malformed")]
    Invalid,
    #[error("column snapshot is stale")]
    Stale,
    #[error("column writer is reserved or full")]
    Busy,
    #[error("effect identifier conflicts with its durable receipt")]
    Conflict,
    #[error("column persistence requires recovery")]
    RecoveryRequired,
    #[error("column writer has closed")]
    Closed,
}
/// A successful disk write, not a client or native-publication acknowledgement.
#[derive(Clone, Debug)]
pub struct ColumnWriteAck {
    pub epoch: u64,
    pub receipt: Option<ColumnReceipt>,
    pub replay: bool,
}
/// Completion receiver remains optional: the owned worker finishes accepted IO.
pub type ColumnCompletion = oneshot::Receiver<Result<ColumnWriteAck, ColumnWriteError>>;

enum Job {
    Write {
        store: Arc<LevelStore>,
        stamp: ColumnStamp,
        encoded: leveldb::EncodedSet,
        effect: Option<String>,
        reply: oneshot::Sender<Result<ColumnWriteAck, ColumnWriteError>>,
        _bytes: ByteLease,
    },
    Barrier(
        Arc<LevelStore>,
        oneshot::Sender<Result<(), ColumnWriteError>>,
    ),
    Shutdown(
        Arc<LevelStore>,
        oneshot::Sender<Result<(), ColumnWriteError>>,
    ),
}
struct ByteLease {
    bytes: usize,
    used: Arc<AtomicUsize>,
}
impl Drop for ByteLease {
    fn drop(&mut self) {
        self.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

/// Shared provider writer. Accepted jobs retain their bounded ownership even
/// when callers drop their completion receiver or the native world closes.
pub struct ColumnWriter {
    store: Arc<LevelStore>,
    owner: uuid::Uuid,
    sequence: AtomicU64,
    queue: mpsc::SyncSender<Job>,
    enqueue_gate: std::sync::Mutex<()>,
    queued_bytes: Arc<AtomicUsize>,
    publication_pending: AtomicBool,
    closed: AtomicBool,
    orphaned: AtomicBool,
    worker: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
    close_gate: tokio::sync::Mutex<()>,
    close_result: std::sync::Mutex<Option<Result<(), ColumnWriteError>>>,
}
impl ColumnWriter {
    /// Install one worker before exposing a database or any loaded column.
    pub(crate) fn open(db: Arc<bleveldb::DB>) -> StorageResult<Arc<Self>> {
        let store = Arc::new(LevelStore::open(db)?);
        let (queue, jobs) = mpsc::sync_channel::<Job>(MAX_PENDING_WRITES);
        let worker_thread = std::thread::Builder::new()
            .name("column-writer".into())
            .spawn(move || {
                while let Ok(job) = jobs.recv() {
                    match job {
                        Job::Write {
                            store,
                            stamp,
                            encoded,
                            effect,
                            reply,
                            _bytes,
                        } => {
                            let result = store.write(stamp, &encoded, effect.as_deref());
                            let _ = reply.send(result);
                        }
                        Job::Barrier(store, reply) => {
                            let _ = reply.send(store.flush());
                        }
                        Job::Shutdown(store, reply) => {
                            let _ = reply.send(store.flush());
                            break;
                        }
                    }
                }
            })
            .map_err(StorageError::Io)?;
        Ok(Arc::new(Self {
            store,
            owner: uuid::Uuid::new_v4(),
            sequence: AtomicU64::new(0),
            queue,
            enqueue_gate: std::sync::Mutex::new(()),
            queued_bytes: Arc::default(),
            publication_pending: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            orphaned: AtomicBool::new(false),
            worker: std::sync::Mutex::new(Some(worker_thread)),
            close_gate: tokio::sync::Mutex::new(()),
            close_result: std::sync::Mutex::new(None),
        }))
    }
    /// Keep a configured storage-directory lease alive through worker teardown.
    pub(crate) fn attach_lease(&self, lease: Arc<dyn Send + Sync>) -> StorageResult<()> {
        self.store.attach_lease(lease)
    }

    /// Install a deterministic pause for native ownership regression tests.
    #[cfg(test)]
    pub(crate) fn pause_next(&self) -> Arc<leveldb::TestPause> {
        self.store.pause_next()
    }
    /// Inject a journal-boundary failure for native recovery regression tests.
    #[cfg(test)]
    pub(crate) fn set_fault(&self, point: u8) {
        self.store.set_fault(point);
    }

    /// Capture the current epoch without waiting for storage IO. A durable
    /// operation blocks ordinary captures until its native publication finishes.
    pub fn capture(&self) -> Result<ColumnStamp, ColumnWriteError> {
        if self.orphaned.load(Ordering::Acquire) || self.store.uncertain() {
            return Err(ColumnWriteError::RecoveryRequired);
        }
        if self.closed.load(Ordering::Acquire) {
            return Err(ColumnWriteError::Closed);
        }
        if self.publication_pending.load(Ordering::Acquire) {
            return Err(ColumnWriteError::Busy);
        }
        self.capture_reserved()
    }
    /// Preserve the same provider witness for a caller already holding a fence.
    fn capture_reserved(&self) -> Result<ColumnStamp, ColumnWriteError> {
        if self.store.uncertain() {
            return Err(ColumnWriteError::RecoveryRequired);
        }
        let sequence = self
            .sequence
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| v.checked_add(1))
            .map_err(|_| ColumnWriteError::Closed)?
            + 1;
        Ok(ColumnStamp {
            owner: self.owner,
            epoch: self.store.epoch(),
            sequence,
        })
    }
    /// Acquire the provider-wide publication fence before validating a durable
    /// native candidate. Existing ordinary stamps remain immutable and stale.
    pub(crate) fn reserve(&self) -> Result<(), ColumnWriteError> {
        self.publication_pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| ColumnWriteError::Busy)
    }
    /// Release only a definitively rejected operation or an applied matching ACK.
    pub(crate) fn release(&self) {
        self.publication_pending.store(false, Ordering::Release);
    }
    /// Whether new native captures must wait for publication or recovery.
    pub fn reserved(&self) -> bool {
        self.publication_pending.load(Ordering::Acquire)
            || self.store.uncertain()
            || self.orphaned.load(Ordering::Acquire)
    }
    /// Detach a closing world's source ownership without cancelling accepted IO.
    /// Reusing this provider for loading requires a fresh open/reconciliation.
    pub(crate) fn orphan(&self) {
        self.orphaned.store(true, Ordering::Release);
    }
    /// Read an immutable receipt under the same database gate as column loads.
    pub async fn receipt(&self, id: &str) -> Result<Option<ColumnReceipt>, ColumnWriteError> {
        let store = self.store.clone();
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || store.receipt(&id))
            .await
            .map_err(|_| ColumnWriteError::RecoveryRequired)?
    }
    /// Queue a bounded ordinary snapshot with its original witness.
    pub fn ordinary(
        &self,
        stamp: ColumnStamp,
        columns: &[ColumnWrite],
    ) -> Result<ColumnCompletion, ColumnWriteError> {
        if self.reserved() {
            return Err(ColumnWriteError::Busy);
        }
        self.submit(stamp, columns, None)
    }
    /// Queue a fenced durable candidate. The fence outlives the completion and
    /// must be released by the native coordinator after physical publication.
    pub(crate) fn durable(
        &self,
        stamp: ColumnStamp,
        columns: &[ColumnWrite],
        id: &str,
    ) -> Result<ColumnCompletion, ColumnWriteError> {
        if !self.publication_pending.load(Ordering::Acquire) {
            return Err(ColumnWriteError::Busy);
        }
        if !valid_id(id) {
            return Err(ColumnWriteError::Invalid);
        }
        self.submit(stamp, columns, Some(id.to_owned()))
    }
    /// Encode before queueing; no worker or byte reservation is created on rejection.
    fn submit(
        &self,
        stamp: ColumnStamp,
        columns: &[ColumnWrite],
        effect: Option<String>,
    ) -> Result<ColumnCompletion, ColumnWriteError> {
        if stamp.owner != self.owner {
            return Err(ColumnWriteError::Stale);
        }
        let encoded = leveldb::encode(columns)?;
        let bytes = encoded.bytes;
        self.queued_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes)
                    .filter(|next| *next <= MAX_PENDING_BYTES)
            })
            .map_err(|_| ColumnWriteError::Busy)?;
        let lease = ByteLease {
            bytes,
            used: self.queued_bytes.clone(),
        };
        let (reply, receiver) = oneshot::channel();
        let _gate = self
            .enqueue_gate
            .lock()
            .map_err(|_| ColumnWriteError::RecoveryRequired)?;
        if self.closed.load(Ordering::Acquire) {
            return Err(ColumnWriteError::Closed);
        }
        self.queue
            .try_send(Job::Write {
                store: self.store.clone(),
                stamp,
                encoded,
                effect,
                reply,
                _bytes: lease,
            })
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => ColumnWriteError::Busy,
                mpsc::TrySendError::Disconnected(_) => ColumnWriteError::Closed,
            })?;
        Ok(receiver)
    }
    /// Compatibility saves may bootstrap a database before its first durable
    /// epoch. They must never infer a newer epoch for an unstamped snapshot.
    pub(crate) async fn legacy_save(&self, column: ColumnWrite) -> StorageResult<()> {
        let stamp = self.capture().map_err(StorageError::ColumnWrite)?;
        if stamp.epoch != 0 {
            return Err(StorageError::ColumnWrite(ColumnWriteError::Stale));
        }
        self.ordinary(stamp, &[column])
            .map_err(StorageError::ColumnWrite)?
            .await
            .map_err(|_| StorageError::ColumnWrite(ColumnWriteError::RecoveryRequired))?
            .map_err(StorageError::ColumnWrite)?;
        Ok(())
    }
    /// Drain prior accepted writes through the same queue, without blocking an
    /// ECS tick or silently acknowledging a poisoned storage state.
    pub async fn flush(&self) -> Result<(), ColumnWriteError> {
        if let Some(result) = *self
            .close_result
            .lock()
            .map_err(|_| ColumnWriteError::RecoveryRequired)?
        {
            return result;
        }
        let (reply, receiver) = oneshot::channel();
        let queue = self.queue.clone();
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || queue.send(Job::Barrier(store, reply)).map_err(|_| ()))
            .await
            .map_err(|_| ColumnWriteError::Closed)?
            .map_err(|_| ColumnWriteError::Closed)?;
        receiver
            .await
            .map_err(|_| ColumnWriteError::RecoveryRequired)?
    }
    /// Reject new captures, then await every job already owned by the writer.
    pub async fn close(&self) -> Result<(), ColumnWriteError> {
        let _closing = self.close_gate.lock().await;
        if let Some(result) = *self
            .close_result
            .lock()
            .map_err(|_| ColumnWriteError::RecoveryRequired)?
        {
            return result;
        }
        {
            let _gate = self
                .enqueue_gate
                .lock()
                .map_err(|_| ColumnWriteError::RecoveryRequired)?;
            self.closed.store(true, Ordering::Release);
        }
        let handle = self
            .worker
            .lock()
            .map_err(|_| ColumnWriteError::RecoveryRequired)?
            .take()
            .ok_or(ColumnWriteError::RecoveryRequired)?;
        let (reply, receiver) = oneshot::channel();
        let queue = self.queue.clone();
        let store = self.store.clone();
        let sent = tokio::task::spawn_blocking(move || {
            queue.send(Job::Shutdown(store, reply)).map_err(|_| ())
        })
        .await;
        let result = if matches!(sent, Ok(Ok(()))) {
            receiver
                .await
                .unwrap_or(Err(ColumnWriteError::RecoveryRequired))
        } else {
            Err(ColumnWriteError::RecoveryRequired)
        };
        let joined = tokio::task::spawn_blocking(move || handle.join()).await;
        let result = if matches!(joined, Ok(Ok(()))) {
            result
        } else {
            Err(ColumnWriteError::RecoveryRequired)
        };
        *self
            .close_result
            .lock()
            .map_err(|_| ColumnWriteError::RecoveryRequired)? = Some(result);
        result
    }
    /// Keep a complete load on one version of the column/actor key space.
    pub(crate) fn read<T>(
        &self,
        f: impl FnOnce(&bleveldb::DB) -> StorageResult<T>,
    ) -> StorageResult<T> {
        if self.orphaned.load(Ordering::Acquire) {
            return Err(StorageError::ColumnWrite(
                ColumnWriteError::RecoveryRequired,
            ));
        }
        self.store.read(f)
    }
}
/// Effect identifiers are bounded opaque keys, never executable commands.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_:./".contains(&b))
}
