//! One gated LevelDB key space for complete column sets and recovery metadata.

use super::{
    ColumnReceipt, ColumnStamp, ColumnWrite, ColumnWriteAck, ColumnWriteError, MAX_COLUMNS,
    MAX_WRITE_BYTES, ReceiptOutcome, valid_id,
};
use crate::storage::{StorageError, StorageResult, keys};
use bleveldb::{DB, ReadOptions, WriteBatch, WriteOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

const LEDGER_KEY: &[u8] = b"unastar:column-writer:v1";
const MAX_RECEIPTS: usize = 65_536;
const MAX_LEDGER_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    version: u32,
    epoch: u64,
    receipts: BTreeMap<String, ColumnReceipt>,
    prepared: Option<Prepared>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Prepared {
    id: String,
    hash: [u8; 32],
    expected: u64,
    columns: Vec<(i32, i32, i32)>,
}
struct State {
    ledger: Ledger,
    last_sequence: u64,
}
/// A LevelDB batch, including deletions for empty subchunks and sparse actors.
pub(super) struct EncodedSet {
    entries: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
    columns: Vec<(i32, i32, i32)>,
    hash: [u8; 32],
    pub bytes: usize,
}
/// The sole read/write gate and durable metadata owner for an opened database.
pub(crate) struct LevelStore {
    db: Arc<DB>,
    state: Mutex<State>,
    epoch: AtomicU64,
    uncertain: AtomicBool,
    lease: Mutex<Option<Arc<dyn Send + Sync>>>,
    #[cfg(test)]
    fault: std::sync::atomic::AtomicU8,
    #[cfg(test)]
    pause: Mutex<Option<Arc<TestPause>>>,
}
impl LevelStore {
    /// Reconcile an interrupted prepare before any column becomes visible.
    pub(crate) fn open(db: Arc<DB>) -> StorageResult<Self> {
        let mut ledger = match db
            .get(LEDGER_KEY, &ReadOptions::new())
            .map_err(StorageError::Database)?
        {
            None => Ledger {
                version: 1,
                ..Ledger::default()
            },
            Some(bytes) => {
                if bytes.len() > MAX_LEDGER_BYTES {
                    return Err(invalid("column ledger exceeds its limit"));
                }
                serde_json::from_slice::<Ledger>(&bytes)
                    .map_err(|error| invalid(error.to_string()))?
            }
        };
        validate(&ledger)?;
        if let Some(prepared) = ledger.prepared.take() {
            // The final data and receipt share one atomic batch. A surviving
            // prepare therefore proves that batch is absent, not half applied.
            ledger.receipts.insert(
                prepared.id.clone(),
                ColumnReceipt {
                    id: prepared.id,
                    hash: prepared.hash,
                    epoch: prepared.expected,
                    outcome: ReceiptOutcome::NotApplied,
                },
            );
            put_ledger(&db, &ledger)?;
        }
        Ok(Self {
            epoch: AtomicU64::new(ledger.epoch),
            state: Mutex::new(State {
                ledger,
                last_sequence: 0,
            }),
            db,
            uncertain: AtomicBool::new(false),
            lease: Mutex::new(None),
            #[cfg(test)]
            fault: std::sync::atomic::AtomicU8::new(0),
            #[cfg(test)]
            pause: Mutex::new(None),
        })
    }
    /// Bind directory ownership to the database and every accepted worker job.
    pub(super) fn attach_lease(&self, lease: Arc<dyn Send + Sync>) -> StorageResult<()> {
        let mut slot = self
            .lease
            .lock()
            .map_err(|_| StorageError::ColumnWrite(ColumnWriteError::RecoveryRequired))?;
        if slot.is_some() {
            return Err(StorageError::ColumnWrite(ColumnWriteError::Conflict));
        }
        *slot = Some(lease);
        Ok(())
    }

    /// Pause an owned write at its real serialization boundary.
    #[cfg(test)]
    pub(super) fn pause_next(&self) -> Arc<TestPause> {
        let pause = Arc::new(TestPause::default());
        *self.pause.lock().unwrap() = Some(pause.clone());
        pause
    }

    /// Inject an interruption at an owned journal boundary for restart tests.
    #[cfg(test)]
    pub(super) fn set_fault(&self, point: u8) {
        self.fault.store(point, Ordering::Release);
    }

    /// Preserve the provider's explicit memtable/SST flush behavior. Accepted
    /// batches are already synchronous; compaction runs only on the IO worker.
    pub(super) fn flush(&self) -> Result<(), ColumnWriteError> {
        if self.uncertain() {
            return Err(ColumnWriteError::RecoveryRequired);
        }
        self.db.flush();
        Ok(())
    }

    /// Observe durable epoch without taking an IO lock.
    pub(super) fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
    /// An ambiguous write fences reads and new writes until a fresh open reconciles it.
    pub(super) fn uncertain(&self) -> bool {
        self.uncertain.load(Ordering::Acquire)
    }
    /// Perform a consistent complete load under the writer's gate.
    pub(super) fn read<T>(&self, f: impl FnOnce(&DB) -> StorageResult<T>) -> StorageResult<T> {
        let _state = self
            .state
            .lock()
            .map_err(|_| StorageError::ColumnWrite(ColumnWriteError::RecoveryRequired))?;
        if self.uncertain() {
            return Err(StorageError::ColumnWrite(
                ColumnWriteError::RecoveryRequired,
            ));
        }
        f(&self.db)
    }
    /// Query a committed or restart-proven absent effect without replaying it.
    pub(super) fn receipt(&self, id: &str) -> Result<Option<ColumnReceipt>, ColumnWriteError> {
        let state = self
            .state
            .lock()
            .map_err(|_| ColumnWriteError::RecoveryRequired)?;
        if self.uncertain() {
            return Err(ColumnWriteError::RecoveryRequired);
        }
        if !valid_id(id) {
            return Err(ColumnWriteError::Invalid);
        }
        Ok(state.ledger.receipts.get(id).cloned())
    }
    /// Check original capture order and epoch, then write one complete atomic set.
    pub(super) fn write(
        &self,
        stamp: ColumnStamp,
        set: &EncodedSet,
        effect: Option<&str>,
    ) -> Result<ColumnWriteAck, ColumnWriteError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ColumnWriteError::RecoveryRequired)?;
        if self.uncertain() {
            return Err(ColumnWriteError::RecoveryRequired);
        }
        #[cfg(test)]
        if let Some(pause) = self.pause.lock().unwrap().take() {
            pause.wait();
        }
        if let Some(id) = effect
            && let Some(receipt) = state.ledger.receipts.get(id)
        {
            if receipt.hash != set.hash {
                return Err(ColumnWriteError::Conflict);
            }
            return match receipt.outcome {
                ReceiptOutcome::Applied => Ok(ColumnWriteAck {
                    epoch: receipt.epoch,
                    receipt: Some(receipt.clone()),
                    replay: true,
                }),
                ReceiptOutcome::NotApplied => Err(ColumnWriteError::Stale),
            };
        }
        if stamp.epoch != state.ledger.epoch || stamp.sequence <= state.last_sequence {
            return Err(ColumnWriteError::Stale);
        }
        let next = if effect.is_some() {
            state
                .ledger
                .epoch
                .checked_add(1)
                .ok_or(ColumnWriteError::Invalid)?
        } else {
            state.ledger.epoch
        };
        let receipt = effect.map(|id| ColumnReceipt {
            id: id.to_owned(),
            hash: set.hash,
            epoch: next,
            outcome: ReceiptOutcome::Applied,
        });
        // Check both bounded metadata forms before attempting any IO. A quota
        // rejection is provably NotApplied and does not need a recovery fence.
        let (final_ledger, final_bytes) = if let Some(receipt) = &receipt {
            let mut final_ledger = state.ledger.clone();
            if final_ledger.receipts.len() >= MAX_RECEIPTS {
                return Err(ColumnWriteError::Busy);
            }
            final_ledger.epoch = next;
            final_ledger
                .receipts
                .insert(receipt.id.clone(), receipt.clone());
            let bytes = serde_json::to_vec(&final_ledger).map_err(|_| ColumnWriteError::Invalid)?;
            if bytes.len() > MAX_LEDGER_BYTES {
                return Err(ColumnWriteError::Busy);
            }
            (Some(final_ledger), Some(bytes))
        } else {
            (None, None)
        };
        if let Some(id) = effect {
            let mut prepared = state.ledger.clone();
            prepared.prepared = Some(Prepared {
                id: id.to_owned(),
                hash: set.hash,
                expected: stamp.epoch,
                columns: set.columns.clone(),
            });
            let bytes = serde_json::to_vec(&prepared).map_err(|_| ColumnWriteError::Invalid)?;
            if bytes.len() > MAX_LEDGER_BYTES {
                return Err(ColumnWriteError::Busy);
            }
            if put_ledger(&self.db, &prepared).is_err() {
                return self.poison();
            }
            #[cfg(test)]
            if self.fault.load(Ordering::Acquire) == 1 {
                return self.poison();
            }
        }
        let mut batch = WriteBatch::new();
        for (key, value) in &set.entries {
            match value {
                Some(value) => batch.put(key, value),
                None => batch.delete(key),
            }
        }
        if let Some(bytes) = &final_bytes {
            batch.put(LEDGER_KEY, bytes);
        }
        let options = WriteOptions::new();
        options.sync(true);
        if batch.write(&self.db, &options).is_err() {
            return self.poison();
        }
        if let Some(ledger) = final_ledger {
            state.ledger = ledger;
        }
        self.epoch.store(next, Ordering::Release);
        state.last_sequence = stamp.sequence;
        #[cfg(test)]
        if self.fault.load(Ordering::Acquire) == 2 {
            return self.poison();
        }
        Ok(ColumnWriteAck {
            epoch: next,
            receipt,
            replay: false,
        })
    }
    /// Latch uncertainty before returning any failure after an attempted write.
    fn poison<T>(&self) -> Result<T, ColumnWriteError> {
        self.uncertain.store(true, Ordering::Release);
        Err(ColumnWriteError::RecoveryRequired)
    }
}
/// Validate persisted ownership, complete epoch lineage, and bounded receipt keys.
fn validate(ledger: &Ledger) -> StorageResult<()> {
    if ledger.version != 1 || ledger.receipts.len() > MAX_RECEIPTS {
        return Err(invalid("invalid column ledger version or capacity"));
    }
    let mut epochs = BTreeSet::new();
    for (id, receipt) in &ledger.receipts {
        if !valid_id(id) || &receipt.id != id || receipt.epoch > ledger.epoch {
            return Err(invalid("invalid column receipt"));
        }
        if receipt.outcome == ReceiptOutcome::Applied
            && (receipt.epoch == 0 || !epochs.insert(receipt.epoch))
        {
            return Err(invalid("invalid column epoch lineage"));
        }
    }
    if epochs.len() as u64 != ledger.epoch {
        return Err(invalid("incomplete column epoch lineage"));
    }
    if let Some(prepared) = &ledger.prepared {
        let columns: BTreeSet<_> = prepared.columns.iter().copied().collect();
        if !valid_id(&prepared.id)
            || prepared.expected != ledger.epoch
            || ledger.receipts.contains_key(&prepared.id)
            || columns.len() != prepared.columns.len()
            || columns.is_empty()
            || columns.len() > MAX_COLUMNS
        {
            return Err(invalid("invalid prepared column ownership"));
        }
    }
    Ok(())
}
/// Persist recovery metadata with the same durable-write guarantee as the final set.
fn put_ledger(db: &DB, ledger: &Ledger) -> StorageResult<()> {
    let bytes = serde_json::to_vec(ledger)
        .map_err(|error| StorageError::Serialization(error.to_string()))?;
    if bytes.len() > MAX_LEDGER_BYTES {
        return Err(invalid("column ledger exceeds its limit"));
    }
    let options = WriteOptions::new();
    options.sync(true);
    db.put(LEDGER_KEY, &bytes, &options)
        .map_err(StorageError::Database)
}
/// Preserve a malformed metadata error instead of generating replacement terrain.
fn invalid(message: impl Into<String>) -> StorageError {
    StorageError::Deserialization(message.into())
}

/// Encode all terrain, sparse block actors, actor digests and global actor records
/// before reserving IO. Duplicate/conflicting actors across a set are rejected.
pub(super) fn encode(columns: &[ColumnWrite]) -> Result<EncodedSet, ColumnWriteError> {
    if columns.is_empty() || columns.len() > MAX_COLUMNS {
        return Err(ColumnWriteError::Invalid);
    }
    let mut entries = BTreeMap::new();
    let mut nbt_keys = BTreeSet::new();
    let mut actor_bytes = 0usize;
    let mut positions = BTreeSet::new();
    let mut actors = BTreeSet::new();
    let mut removed = BTreeSet::new();
    for column in columns {
        let pos = column.position;
        let dim = column.dimension;
        let chunk = &column.column.chunk;
        if chunk.state_space() != crate::world::chunk::BlockStateSpace::Bedrock
            || chunk.x != pos.x
            || chunk.z != pos.z
            || !positions.insert((dim, pos.x, pos.z))
        {
            return Err(ColumnWriteError::Invalid);
        }
        entries.insert(keys::version_key(pos, dim), Some(vec![keys::CHUNK_VERSION]));
        entries.insert(
            keys::finalisation_key(pos, dim),
            Some(2u32.to_le_bytes().to_vec()),
        );
        entries.insert(
            keys::biome_key(pos, dim),
            Some(chunk.encode_biomes_disk_for_dimension(dim)),
        );
        // Empty sections must delete old data when replacing a nonempty column.
        for y in -4..20i8 {
            entries.insert(
                keys::subchunk_key(pos, dim, y),
                chunk.encode_subchunk_disk(y as i32),
            );
        }
        let mut block_actors = Vec::new();
        for actor in chunk.block_entities() {
            let [x, _, z] = actor.position();
            if x.div_euclid(16) != pos.x || z.div_euclid(16) != pos.z {
                return Err(ColumnWriteError::Invalid);
            }
            actor
                .write_disk_nbt(&mut block_actors)
                .map_err(|_| ColumnWriteError::Invalid)?;
            if block_actors.len() > MAX_WRITE_BYTES {
                return Err(ColumnWriteError::Invalid);
            }
        }
        nbt_keys.insert(keys::block_entities_key(pos, dim));
        entries.insert(
            keys::block_entities_key(pos, dim),
            (!block_actors.is_empty()).then_some(block_actors),
        );
        let mut digest = Vec::new();
        for actor in &column.column.actors {
            if !actors.insert(actor.unique_id) || actor.nbt.len() > MAX_WRITE_BYTES {
                return Err(ColumnWriteError::Invalid);
            }
            actor_bytes = actor_bytes
                .checked_add(actor.nbt.len())
                .filter(|bytes| *bytes <= MAX_WRITE_BYTES)
                .ok_or(ColumnWriteError::Invalid)?;
            validate_actor(actor)?;
            nbt_keys.insert(keys::entity_data_key(actor.unique_id));
            digest.extend_from_slice(&actor.unique_id.to_le_bytes());
            entries.insert(
                keys::entity_data_key(actor.unique_id),
                Some(actor.nbt.clone()),
            );
        }
        entries.insert(
            keys::entity_ids_key(pos, dim),
            (!digest.is_empty()).then_some(digest),
        );
        removed.extend(column.column.removed_actors.iter().copied());
        if entries
            .iter()
            .try_fold(0usize, |sum, (key, value)| {
                sum.checked_add(key.len() + value.as_ref().map_or(0, Vec::len))
            })
            .is_none_or(|bytes| bytes > MAX_WRITE_BYTES)
        {
            return Err(ColumnWriteError::Invalid);
        }
    }
    if !actors.is_disjoint(&removed) {
        return Err(ColumnWriteError::Invalid);
    }
    for id in removed {
        entries.insert(keys::entity_data_key(id), None);
    }
    let bytes = entries
        .iter()
        .try_fold(0usize, |sum, (key, value)| {
            sum.checked_add(key.len() + value.as_ref().map_or(0, Vec::len))
        })
        .filter(|bytes| *bytes <= MAX_WRITE_BYTES)
        .ok_or(ColumnWriteError::Invalid)?;
    let mut hash = Sha256::new();
    hash.update(b"unastar-column-set-v1");
    for (key, value) in &entries {
        hash.update((key.len() as u64).to_le_bytes());
        hash.update(key);
        match value {
            Some(value) => {
                hash.update([1]);
                if nbt_keys.contains(key) {
                    hash.update([1]);
                    super::fingerprint::nbt_stream(&mut hash, value)?;
                } else {
                    hash.update([0]);
                    hash.update((value.len() as u64).to_le_bytes());
                    hash.update(value);
                }
            }
            None => hash.update([0]),
        }
    }
    Ok(EncodedSet {
        entries,
        columns: positions.into_iter().collect(),
        bytes,
        hash: hash.finalize().into(),
    })
}

/// Reject malformed actor payloads and conflicting embedded identities before
/// preparing ownership. Unknown actor kinds remain opaque and preserved.
fn validate_actor(actor: &crate::storage::StoredActor) -> Result<(), ColumnWriteError> {
    use zuri_nbt::{NBTTag, encoding::LittleEndian};
    let mut input = actor.nbt.as_slice();
    let root = NBTTag::read(&mut input, LittleEndian).map_err(|_| ColumnWriteError::Invalid)?;
    let NBTTag::Compound(root) = root else {
        return Err(ColumnWriteError::Invalid);
    };
    if !input.is_empty() {
        return Err(ColumnWriteError::Invalid);
    }
    if let Some(id) = root.0.get("UniqueID")
        && !matches!(id, NBTTag::Long(id) if id.0 == actor.unique_id)
    {
        return Err(ColumnWriteError::Invalid);
    }
    if let Some(pos) = root.0.get("Pos") {
        let NBTTag::List(pos) = pos else {
            return Err(ColumnWriteError::Invalid);
        };
        if pos.0.len() != 3 {
            return Err(ColumnWriteError::Invalid);
        }
        for value in &pos.0 {
            let NBTTag::Float(value) = value else {
                return Err(ColumnWriteError::Invalid);
            };
            if !value.0.is_finite() {
                return Err(ColumnWriteError::Invalid);
            }
        }
    }
    Ok(())
}

/// Deterministic test-only IO gate; production uses the same serialized write.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestPause {
    pub(crate) entered: AtomicBool,
    released: Mutex<bool>,
    wake: std::sync::Condvar,
}
#[cfg(test)]
impl TestPause {
    /// Signal entry and await only the test's release.
    fn wait(&self) {
        let mut released = self.released.lock().unwrap();
        self.entered.store(true, Ordering::Release);
        while !*released {
            released = self.wake.wait(released).unwrap();
        }
    }
    /// Resume the exact owned operation.
    pub(crate) fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.wake.notify_all();
    }
}
