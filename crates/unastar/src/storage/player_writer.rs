//! Serialized player writes with inventory epochs and durable idempotency records.
//!
//! Every load, ordinary save and effect write for one account shares a lock.
//! Inventory epochs advance only for durable effects, preventing an older queued
//! autosave or disconnect snapshot from replacing their results. All application
//! accesses must use this wrapper once installed; the inner provider remains an
//! implementation detail. A provider's successful save must be durable.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use super::{
    InventoryEffectRecord, PlayerData, PlayerProvider, StorageError, StorageResult,
    valid_inventory_effect_id,
};

/// The outcome of a write that either created or found an exact durable receipt.
#[derive(Clone, Debug)]
pub struct EffectWrite {
    pub record: InventoryEffectRecord,
    pub already_committed: bool,
}

/// Whether the caller can prove its effect did not run and release a reservation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteFailureKind {
    NotApplied,
    RecoveryRequired,
}

/// A persistence failure with an explicit recovery disposition.
#[derive(Clone, Debug)]
pub struct EffectWriteFailure {
    pub kind: WriteFailureKind,
    pub resume_source: bool,
    pub message: String,
}

/// Shared serialization and uncertainty state for one provider instance.
#[derive(Clone)]
pub struct SerializedPlayerProvider {
    inner: Arc<dyn PlayerProvider>,
    locks: Arc<Mutex<BTreeMap<Uuid, Arc<AsyncMutex<()>>>>>,
    uncertain: Arc<Mutex<BTreeSet<Uuid>>>,
}

impl SerializedPlayerProvider {
    /// Wrap a provider before sharing it with connection loading and game saves.
    pub fn new(inner: Arc<dyn PlayerProvider>) -> Self {
        Self {
            inner,
            locks: Arc::new(Mutex::new(BTreeMap::new())),
            uncertain: Arc::new(Mutex::new(BTreeSet::new())),
        }
    }

    /// Whether an ambiguous write/read requires recovery before admitting this account.
    pub fn recovery_required(&self, uuid: Uuid) -> bool {
        self.uncertain
            .lock()
            .map_or(true, |accounts| accounts.contains(&uuid))
    }

    /// Latch a poisoned account without waiting on its outstanding provider future.
    pub fn require_recovery(&self, uuid: Uuid) {
        if let Ok(mut accounts) = self.uncertain.lock() {
            accounts.insert(uuid);
        }
    }

    /// Get the one asynchronous writer lock associated with this account.
    fn account_lock(&self, uuid: Uuid) -> StorageResult<Arc<AsyncMutex<()>>> {
        Ok(self
            .locks
            .lock()
            .map_err(|_| StorageError::PlayerRecoveryRequired)?
            .entry(uuid)
            .or_default()
            .clone())
    }

    /// Atomically store candidate inventory and receipt after checking its loaded epoch.
    /// An error after calling the underlying save is always conservatively uncertain.
    pub async fn commit_effect(
        &self,
        uuid: Uuid,
        id: &str,
        request_hash: [u8; 32],
        mut candidate: PlayerData,
    ) -> Result<EffectWrite, EffectWriteFailure> {
        if !self.inner.atomic_durable_records() {
            return Err(uncertain(
                "provider does not guarantee atomic durable records",
            ));
        }
        if !valid_inventory_effect_id(id) {
            return Err(rejected("invalid effect identifier"));
        }
        if self.recovery_required(uuid) {
            return Err(uncertain("account requires recovery"));
        }
        let lock = self
            .account_lock(uuid)
            .map_err(|error| uncertain(error.to_string()))?;
        let _guard = lock.lock().await;
        if self.recovery_required(uuid) {
            return Err(uncertain("account requires recovery"));
        }
        let current = match self.inner.load(uuid).await {
            Ok(current) => current,
            Err(error) => {
                self.require_recovery(uuid);
                return Err(uncertain(error.to_string()));
            }
        };
        if let Some(current) = &current {
            if let Err(error) = current.validate(uuid) {
                self.require_recovery(uuid);
                return Err(uncertain(error.to_string()));
            }
            if let Some(record) = current.inventory_effects.get(id) {
                return if record.request_hash == request_hash {
                    Ok(EffectWrite {
                        record: record.clone(),
                        already_committed: true,
                    })
                } else {
                    Err(uncertain(
                        "effect identifier already belongs to another payload",
                    ))
                };
            }
        }
        let revision = current.as_ref().map_or(0, |data| data.inventory_revision);
        if candidate.inventory_revision != revision {
            let mut error = rejected("inventory revision changed before the effect write");
            error.resume_source = false;
            return Err(error);
        }
        if current
            .as_ref()
            .is_some_and(|data| data.inventory_effects.len() >= 65_536)
        {
            return Err(rejected("inventory receipt history is full"));
        }
        candidate.inventory_effects =
            current.map_or_else(BTreeMap::new, |data| data.inventory_effects);
        let next = revision
            .checked_add(1)
            .ok_or_else(|| rejected("inventory revision exhausted"))?;
        let record = InventoryEffectRecord {
            revision: next,
            request_hash,
        };
        candidate.inventory_revision = next;
        candidate
            .inventory_effects
            .insert(id.into(), record.clone());
        candidate
            .validate(uuid)
            .map_err(|error| rejected(error.to_string()))?;
        if let Err(error) = self.inner.save(uuid, &candidate).await {
            self.require_recovery(uuid);
            return Err(uncertain(error.to_string()));
        }
        Ok(EffectWrite {
            record,
            already_committed: false,
        })
    }
}

#[async_trait]
impl PlayerProvider for SerializedPlayerProvider {
    fn atomic_durable_records(&self) -> bool {
        self.inner.atomic_durable_records()
    }

    async fn load(&self, uuid: Uuid) -> StorageResult<Option<PlayerData>> {
        if self.recovery_required(uuid) {
            return Err(StorageError::PlayerRecoveryRequired);
        }
        let lock = self.account_lock(uuid)?;
        let _guard = lock.lock().await;
        if self.recovery_required(uuid) {
            return Err(StorageError::PlayerRecoveryRequired);
        }
        match self.inner.load(uuid).await {
            Ok(Some(data)) => match data.validate(uuid) {
                Ok(()) => Ok(Some(data)),
                Err(error) => {
                    self.require_recovery(uuid);
                    Err(error)
                }
            },
            Ok(None) => Ok(None),
            Err(error) => {
                self.require_recovery(uuid);
                Err(error)
            }
        }
    }

    async fn save(&self, uuid: Uuid, data: &PlayerData) -> StorageResult<()> {
        data.validate(uuid)?;
        if self.recovery_required(uuid) {
            return Err(StorageError::PlayerRecoveryRequired);
        }
        let lock = self.account_lock(uuid)?;
        let _guard = lock.lock().await;
        if self.recovery_required(uuid) {
            return Err(StorageError::PlayerRecoveryRequired);
        }
        let current = self.inner.load(uuid).await.inspect_err(|_| {
            self.require_recovery(uuid);
        })?;
        if current.as_ref().map_or(0, |data| data.inventory_revision) != data.inventory_revision {
            return Err(StorageError::PlayerRevisionConflict);
        }
        let mut candidate = data.clone();
        // Ordinary saves retain history from storage; callers cannot erase or mint receipts.
        candidate.inventory_effects =
            current.map_or_else(BTreeMap::new, |data| data.inventory_effects);
        candidate.validate(uuid)?;
        self.inner.save(uuid, &candidate).await.inspect_err(|_| {
            self.require_recovery(uuid);
        })
    }

    async fn delete(&self, uuid: Uuid) -> StorageResult<()> {
        let lock = self.account_lock(uuid)?;
        let _guard = lock.lock().await;
        self.inner.delete(uuid).await
    }

    async fn close(&self) -> StorageResult<()> {
        self.inner.close().await
    }
}

/// A successful read proved this requested effect was absent before any write attempt.
fn rejected(message: impl Into<String>) -> EffectWriteFailure {
    EffectWriteFailure {
        kind: WriteFailureKind::NotApplied,
        resume_source: true,
        message: message.into(),
    }
}

/// The write may have committed or its durable identity could not be established.
fn uncertain(message: impl Into<String>) -> EffectWriteFailure {
    EffectWriteFailure {
        kind: WriteFailureKind::RecoveryRequired,
        resume_source: false,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::LevelDBPlayerProvider;

    /// A structurally valid record in the initial inventory epoch.
    fn data(uuid: Uuid) -> PlayerData {
        PlayerData {
            uuid: uuid.to_string(),
            game_mode: 0,
            ..PlayerData::default()
        }
    }

    #[tokio::test]
    async fn durable_effects_survive_reopen_and_reject_stale_autosaves_without_reapplying() {
        let directory = tempfile::tempdir().unwrap();
        let uuid = Uuid::new_v4();
        let before = data(uuid);
        {
            let provider = SerializedPlayerProvider::new(Arc::new(
                LevelDBPlayerProvider::open(directory.path()).unwrap(),
            ));
            provider.save(uuid, &before).await.unwrap();
            let mut candidate = before.clone();
            candidate.food = 17;
            candidate.inventory = vec![super::super::StoredItemStack {
                item_id: "minecraft:stone".into(),
                runtime_id: 0,
                count: 17,
                damage: 0,
                nbt: None,
            }];
            candidate.inventory.resize_with(36, Default::default);
            let applied = provider
                .commit_effect(uuid, "request:1", [7; 32], candidate.clone())
                .await
                .unwrap();
            assert_eq!(applied.record.revision, 1);
            assert!(!applied.already_committed);
            assert!(matches!(
                provider.save(uuid, &before).await,
                Err(StorageError::PlayerRevisionConflict)
            ));
            let duplicate = provider
                .commit_effect(uuid, "request:1", [7; 32], candidate)
                .await
                .unwrap();
            assert!(duplicate.already_committed);
            assert_eq!(provider.load(uuid).await.unwrap().unwrap().food, 17);
        }
        let provider = SerializedPlayerProvider::new(Arc::new(
            LevelDBPlayerProvider::open(directory.path()).unwrap(),
        ));
        let stored = provider.load(uuid).await.unwrap().unwrap();
        assert_eq!(stored.inventory_effects["request:1"].request_hash, [7; 32]);
        assert_eq!(stored.inventory_revision, 1);
        assert_eq!(stored.food, 17);
        assert_eq!(stored.inventory[0].count, 17);
    }

    #[tokio::test]
    async fn receipt_payload_collisions_and_missing_epoch_do_not_mutate_the_record() {
        let directory = tempfile::tempdir().unwrap();
        let uuid = Uuid::new_v4();
        let provider = SerializedPlayerProvider::new(Arc::new(
            LevelDBPlayerProvider::open(directory.path()).unwrap(),
        ));
        provider
            .commit_effect(uuid, "one", [1; 32], data(uuid))
            .await
            .unwrap();
        let before = provider.load(uuid).await.unwrap().unwrap();
        let collision = provider
            .commit_effect(uuid, "one", [2; 32], data(uuid))
            .await
            .unwrap_err();
        assert_eq!(collision.kind, WriteFailureKind::RecoveryRequired);
        let stale = provider
            .commit_effect(uuid, "two", [3; 32], data(uuid))
            .await
            .unwrap_err();
        assert_eq!(stale.kind, WriteFailureKind::NotApplied);
        assert_eq!(provider.load(uuid).await.unwrap().unwrap(), before);
    }
}
