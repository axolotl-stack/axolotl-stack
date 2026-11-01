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

use super::source::{SourceActorRecord, SourcePhase, SourceProof};
use super::{
    InventoryEffectRecord, PlayerData, PlayerProvider, StorageError, StorageResult,
    valid_inventory_effect_id,
};

/// A source transition returns the exact row acknowledged under the account lock.
pub(crate) struct SourceWrite {
    pub effect: EffectWrite,
    pub data: PlayerData,
}

/// Native source proof and exact optional initialization baseline for one full-state install.
#[derive(Default)]
pub(crate) struct StateOrigin {
    pub source: Option<SourceProof>,
    pub baseline: Option<Arc<PlayerData>>,
}

enum SourceMutation {
    Ordinary,
    Handoff(StateOrigin),
    Transition {
        expected: Option<SourceProof>,
        desired: Box<SourceActorRecord>,
    },
}

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
        candidate: PlayerData,
    ) -> Result<EffectWrite, EffectWriteFailure> {
        self.commit_full_effect(
            uuid,
            id,
            request_hash,
            candidate,
            None,
            SourceMutation::Ordinary,
        )
        .await
        .map(|(effect, _)| effect)
    }

    /// Atomically preserve canonical history while installing one full-state handoff.
    pub(crate) async fn commit_state_effect(
        &self,
        uuid: Uuid,
        id: &str,
        request_hash: [u8; 32],
        candidate: PlayerData,
        supplement: super::RestorationSupplement,
        origin: StateOrigin,
    ) -> Result<EffectWrite, EffectWriteFailure> {
        self.commit_full_effect(
            uuid,
            id,
            request_hash,
            candidate,
            Some(supplement),
            SourceMutation::Handoff(origin),
        )
        .await
        .map(|(effect, _)| effect)
    }

    /// Source ownership and inventory/receipt lineage change in the same durable account row.
    pub(crate) async fn commit_source(
        &self,
        uuid: Uuid,
        id: &str,
        hash: [u8; 32],
        candidate: PlayerData,
        expected: Option<SourceProof>,
        desired: SourceActorRecord,
    ) -> Result<SourceWrite, EffectWriteFailure> {
        self.commit_full_effect(
            uuid,
            id,
            hash,
            candidate,
            None,
            SourceMutation::Transition {
                expected,
                desired: Box::new(desired),
            },
        )
        .await
        .map(|(effect, data)| SourceWrite { effect, data })
    }

    /// Inventory and full-state effects share one account lock, epoch and proof ledger.
    async fn commit_full_effect(
        &self,
        uuid: Uuid,
        id: &str,
        request_hash: [u8; 32],
        mut candidate: PlayerData,
        supplement: Option<super::RestorationSupplement>,
        source: SourceMutation,
    ) -> Result<(EffectWrite, PlayerData), EffectWriteFailure> {
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
                    Ok((
                        EffectWrite {
                            record: record.clone(),
                            already_committed: true,
                        },
                        current.clone(),
                    ))
                } else {
                    Err(uncertain(
                        "effect identifier already belongs to another payload",
                    ))
                };
            }
        }
        let revision = current.as_ref().map_or(0, |data| data.inventory_revision);
        if let SourceMutation::Handoff(StateOrigin {
            baseline: Some(expected),
            ..
        }) = &source
            && current.as_ref() != Some(expected.as_ref())
        {
            let mut error = rejected("canonical initialization source changed");
            error.resume_source = false;
            return Err(error);
        }
        if candidate.inventory_revision != revision {
            let mut error = rejected("inventory revision changed before the effect write");
            error.resume_source = false;
            return Err(error);
        }
        if supplement.is_some()
            && current
                .as_ref()
                .map_or_else(BTreeMap::new, |data| data.inventory_effects.clone())
                != candidate.inventory_effects
        {
            let mut error = rejected("state receipt lineage changed before the effect write");
            error.resume_source = false;
            return Err(error);
        }
        if current
            .as_ref()
            .is_some_and(|data| data.inventory_effects.len() >= 65_536)
        {
            return Err(rejected("inventory receipt history is full"));
        }
        candidate.source_actor =
            source_update(current.as_ref(), &candidate, source, id, request_hash)?;
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
        candidate.restoration_checkpoint =
            supplement.map(|supplement| super::RestorationCheckpoint {
                id: id.into(),
                revision: next,
                request_hash,
                supplement,
            });
        candidate
            .validate(uuid)
            .map_err(|error| rejected(error.to_string()))?;
        if let Err(error) = self.inner.save(uuid, &candidate).await {
            self.require_recovery(uuid);
            return Err(uncertain(error.to_string()));
        }
        Ok((
            EffectWrite {
                record,
                already_committed: false,
            },
            candidate,
        ))
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
        candidate.source_actor = ordinary_source(current.as_ref(), data)?;
        // This ordinary-save boundary retires the one-shot raw handoff. Keeping
        // old extras beside newly advanced life/position would replay stale state.
        candidate.restoration_checkpoint = None;
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

/// Ordinary callers can advance genuine active state, but cannot create or reopen source authority.
fn ordinary_source(
    current: Option<&PlayerData>,
    candidate: &PlayerData,
) -> StorageResult<Option<SourceActorRecord>> {
    let old = current.and_then(|data| data.source_actor.as_ref());
    match old {
        Some(old) if old.phase.unresolved() => {
            let next = candidate
                .source_actor
                .as_ref()
                .ok_or(StorageError::PlayerRevisionConflict)?;
            let mut comparable = next.clone();
            comparable.sample = old.sample;
            comparable.supplement = old.supplement.clone();
            if old.phase != SourcePhase::Active
                || next.phase != SourcePhase::Active
                || next.sample <= old.sample
                || comparable != *old
            {
                return Err(StorageError::PlayerRevisionConflict);
            }
            next.validate(candidate)?;
            Ok(Some(next.clone()))
        }
        Some(old) => {
            if candidate.source_actor.is_some() {
                return Err(StorageError::PlayerRevisionConflict);
            }
            let mut tombstone = old.clone();
            tombstone.phase = SourcePhase::Retired;
            tombstone.supplement = None;
            Ok(Some(tombstone))
        }
        None if candidate.source_actor.is_some() => Err(StorageError::PlayerRevisionConflict),
        None => Ok(None),
    }
}

/// All source generations use the same canonical lock and atomic receipt boundary.
fn source_update(
    current: Option<&PlayerData>,
    candidate: &PlayerData,
    mutation: SourceMutation,
    id: &str,
    request_hash: [u8; 32],
) -> Result<Option<SourceActorRecord>, EffectWriteFailure> {
    let conflict = || {
        let mut error = rejected("canonical source ownership changed");
        error.resume_source = false;
        error
    };
    let old = current.and_then(|data| data.source_actor.as_ref());
    match mutation {
        SourceMutation::Ordinary => ordinary_source(current, candidate).map_err(|_| conflict()),
        SourceMutation::Handoff(StateOrigin { source: proof, .. }) => match (old, proof) {
            (Some(old), Some(proof)) if old.phase.unresolved() && old.matches(&proof) => {
                let mut next = old.clone();
                next.generation = old.generation.checked_add(1).ok_or_else(conflict)?;
                next.sample = 0;
                next.phase = SourcePhase::Transferred;
                next.supplement = None;
                next.handoff = Some(super::source::SourceHandoff {
                    id: id.into(),
                    request_hash,
                });
                Ok(Some(next))
            }
            (Some(old), None) if !old.phase.unresolved() => {
                let mut next = old.clone();
                next.phase = SourcePhase::Transferred;
                next.supplement = None;
                Ok(Some(next))
            }
            (None, None) => Ok(None),
            _ => Err(conflict()),
        },
        SourceMutation::Transition { expected, desired } => {
            let generation = old
                .map_or(Some(1), |old| old.generation.checked_add(1))
                .ok_or_else(conflict)?;
            let valid_old = match (old, expected) {
                (Some(old), Some(proof)) => {
                    old.matches(&proof)
                        && old.module == desired.module
                        && old.nonce == desired.nonce
                        && old.world == desired.world
                        && old.name == desired.name
                        && (old.phase.unresolved() || desired.phase == SourcePhase::Retired)
                }
                (Some(old), None) => {
                    !old.phase.unresolved()
                        && old.nonce != desired.nonce
                        && desired.phase == SourcePhase::Frozen
                }
                (None, None) => desired.phase == SourcePhase::Frozen,
                _ => false,
            };
            if !valid_old
                || desired.generation != generation
                || desired.sample != 0
                || desired.phase == SourcePhase::Transferred
            {
                return Err(conflict());
            }
            desired.validate(candidate).map_err(|_| conflict())?;
            Ok(Some(*desired))
        }
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
#[path = "player_writer/source_tests.rs"]
mod source_tests;

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

    #[tokio::test]
    async fn inventory_effect_after_state_restore_retires_the_checkpoint_without_losing_either_receipt()
     {
        let directory = tempfile::tempdir().unwrap();
        let uuid = Uuid::new_v4();
        let supplement = super::super::RestorationSupplement {
            version: 1,
            xuid: Some("42".into()),
            effective_mode: 0,
            base_mode: None,
            flight_override: None,
            food_policy: None,
            food_ticks: 0,
            food_regeneration: 0,
            velocity: [0.0; 3],
            head_yaw: 0.0,
            absorption: 0.0,
            air_current: 17,
            air_max: 300,
            on_fire_ticks: 0,
            burning_remaining: 0,
            burning_next: 0,
            effects: Vec::new(),
            dead: false,
        };
        {
            let provider = SerializedPlayerProvider::new(Arc::new(
                LevelDBPlayerProvider::open(directory.path()).unwrap(),
            ));
            let original = data(uuid);
            provider.save(uuid, &original).await.unwrap();
            provider
                .commit_state_effect(
                    uuid,
                    "state:1",
                    [4; 32],
                    original.clone(),
                    supplement.clone(),
                    StateOrigin::default(),
                )
                .await
                .unwrap();
            let mut candidate = provider.load(uuid).await.unwrap().unwrap();
            assert!(candidate.restoration_checkpoint.is_some());
            candidate.food = 15;
            candidate.inventory.resize_with(36, Default::default);
            candidate.inventory[0] = super::super::StoredItemStack {
                item_id: "minecraft:stone".into(),
                runtime_id: 0,
                count: 3,
                damage: 0,
                nbt: None,
            };
            provider
                .commit_effect(uuid, "items:1", [5; 32], candidate)
                .await
                .unwrap();
            let advanced = provider.load(uuid).await.unwrap().unwrap();
            assert!(advanced.restoration_checkpoint.is_none());
            assert_eq!(advanced.inventory_revision, 2);
            assert_eq!(advanced.inventory_effects.len(), 2);
            let replay = provider
                .commit_state_effect(
                    uuid,
                    "state:1",
                    [4; 32],
                    original,
                    supplement,
                    StateOrigin::default(),
                )
                .await
                .unwrap();
            assert!(replay.already_committed);
            assert_eq!(provider.load(uuid).await.unwrap().unwrap(), advanced);
        }
        let provider = SerializedPlayerProvider::new(Arc::new(
            LevelDBPlayerProvider::open(directory.path()).unwrap(),
        ));
        let stored = provider.load(uuid).await.unwrap().unwrap();
        assert!(stored.restoration_checkpoint.is_none());
        assert_eq!(stored.inventory_revision, 2);
        assert_eq!(stored.food, 15);
        assert_eq!(stored.inventory[0].count, 3);
        assert_eq!(stored.inventory_effects["state:1"].request_hash, [4; 32]);
        assert_eq!(stored.inventory_effects["items:1"].request_hash, [5; 32]);
    }
}
