//! Nonblocking canonical full-state writes over genuinely held native targets.

use super::*;
use crate::ecs::PacketApplySet;
use crate::storage::{
    InventoryEffectRecord,
    player_writer::{EffectWrite, EffectWriteFailure, SerializedPlayerProvider, WriteFailureKind},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, mpsc},
};

/// Typed native failures; provider diagnostics remain private to the engine log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Unavailable,
    Busy,
    Conflict,
    InvalidState,
    RecoveryRequired,
}
/// One immutable completion, including absence of the original actor.
#[derive(Event, Clone)]
pub(crate) struct Committed {
    pub player: Option<Entity>,
    pub uuid: uuid::Uuid,
    pub id: String,
    pub receipt: InventoryEffectRecord,
    pub already_committed: bool,
    pub live_applied: bool,
    pub retained_buffers: bool,
}
/// Definite rejection and ambiguous writes have different recovery obligations.
#[derive(Event, Clone)]
pub(crate) struct Failed {
    pub player: Option<Entity>,
    pub uuid: uuid::Uuid,
    pub id: String,
    pub kind: WriteFailureKind,
    pub retained_buffers: bool,
}
pub(crate) enum Submission {
    Queued,
    AlreadyCommitted(InventoryEffectRecord),
}

/// Report installed durable support without exposing writer internals to components.
pub(crate) fn available(world: &World) -> bool {
    world.contains_resource::<Coordinator>()
}

#[derive(Resource)]
struct Coordinator {
    writer: Arc<SerializedPlayerProvider>,
    tx: mpsc::Sender<Completion>,
    rx: Mutex<mpsc::Receiver<Completion>>,
    pending: BTreeMap<uuid::Uuid, (String, Option<String>)>,
}
struct Completion {
    target: quarantine::Handle,
    uuid: uuid::Uuid,
    id: String,
    desired: Arc<PlayerSnapshot>,
    history: std::collections::BTreeMap<String, InventoryEffectRecord>,
    result: Result<EffectWrite, EffectWriteFailure>,
    _retention: Arc<dyn Send + Sync>,
}
struct Reply {
    writer: Arc<SerializedPlayerProvider>,
    tx: mpsc::Sender<Completion>,
    target: quarantine::Handle,
    uuid: uuid::Uuid,
    id: String,
    desired: Arc<PlayerSnapshot>,
    history: std::collections::BTreeMap<String, InventoryEffectRecord>,
    retention: Option<Arc<dyn Send + Sync>>,
    done: bool,
}
impl Reply {
    /// A closed owner world cannot silently turn an accepted worker into a definite rejection.
    fn finish(mut self, result: Result<EffectWrite, EffectWriteFailure>) {
        self.done = true;
        if self
            .tx
            .send(Completion {
                target: self.target.clone(),
                uuid: self.uuid,
                id: self.id.clone(),
                desired: self.desired.clone(),
                history: self.history.clone(),
                result,
                _retention: self.retention.take().expect("accepted reservation"),
            })
            .is_err()
        {
            self.writer.require_recovery(self.uuid);
        }
    }
}
impl Drop for Reply {
    fn drop(&mut self) {
        if !self.done {
            self.writer.require_recovery(self.uuid);
            if let Some(retention) = self.retention.take() {
                let _ = self.tx.send(Completion {
                    target: self.target.clone(),
                    uuid: self.uuid,
                    id: self.id.clone(),
                    desired: self.desired.clone(),
                    history: self.history.clone(),
                    result: Err(EffectWriteFailure {
                        kind: WriteFailureKind::RecoveryRequired,
                        resume_source: false,
                        message: "state worker interrupted".into(),
                    }),
                    _retention: retention,
                });
            }
        }
    }
}

/// Stock runtimes install this alongside their shared inventory/account writer.
pub(crate) fn install(
    server: &mut super::super::GameServer,
) -> Result<(), super::super::inventory_effects::EffectError> {
    let world = server.ecs.world_mut();
    if world.contains_resource::<Coordinator>() {
        return Ok(());
    }
    let writer = super::super::inventory_effects::prepare_shared_writer(world)?;
    let (tx, rx) = mpsc::channel();
    world.insert_resource(Coordinator {
        writer,
        tx,
        rx: Mutex::new(rx),
        pending: BTreeMap::new(),
    });
    server
        .ecs
        .schedule_mut()
        .add_systems(poll.before(PacketApplySet));
    super::source_actors::install(server)?;
    Ok(())
}

/// A held account remains unavailable to concurrent admission until native acknowledgement.
pub(crate) fn account_pending(world: &World, uuid: uuid::Uuid, xuid: Option<&str>) -> bool {
    quarantine::account_pending(world, uuid, xuid)
        || super::source_actors::pending_account(world, uuid, xuid)
        || world
            .get_resource::<Coordinator>()
            .is_some_and(|coordinator| {
                coordinator.pending.iter().any(|(id, (_, pending_xuid))| {
                    *id == uuid
                        || xuid
                            .filter(|xuid| !xuid.is_empty())
                            .is_some_and(|xuid| pending_xuid.as_deref() == Some(xuid))
                })
            })
}

/// One validated complete install retains its original physical and canonical baselines.
pub(crate) struct StateSubmission {
    pub target: quarantine::Handle,
    pub id: String,
    pub hash: [u8; 32],
    pub expected: Arc<PlayerSnapshot>,
    pub desired: Arc<PlayerSnapshot>,
    pub origin: crate::storage::player_writer::StateOrigin,
    pub retention: Arc<dyn Send + Sync>,
}

/// The caller has already frozen the exact source and performed global duplicate checks.
pub(crate) fn submit(world: &mut World, request: StateSubmission) -> Result<Submission, Error> {
    let StateSubmission {
        target,
        id,
        hash,
        expected,
        desired,
        origin,
        retention,
    } = request;
    let crate::storage::player_writer::StateOrigin { source, baseline } = origin;
    if !crate::storage::valid_inventory_effect_id(&id) || !quarantine::matches(world, &target) {
        return Err(Error::Conflict);
    }
    if desired.data.uuid != expected.data.uuid || desired.xuid != expected.xuid {
        return Err(Error::Conflict);
    }
    let uuid = uuid::Uuid::parse_str(&expected.data.uuid).map_err(|_| Error::InvalidState)?;
    if let Some(receipt) = super::super::inventory_effects::receipt(world, target.player, &id) {
        return if receipt.request_hash == hash {
            Ok(Submission::AlreadyCommitted(receipt.clone()))
        } else {
            Err(Error::Conflict)
        };
    }
    let coordinator = world
        .get_resource::<Coordinator>()
        .ok_or(Error::Unavailable)?;
    if coordinator.pending.contains_key(&uuid) {
        return Err(Error::Busy);
    }
    if coordinator.writer.recovery_required(uuid) {
        return Err(Error::RecoveryRequired);
    }
    let writer = coordinator.writer.clone();
    let tx = coordinator.tx.clone();
    let runtime = tokio::runtime::Handle::try_current().map_err(|_| Error::Unavailable)?;
    let mut candidate = desired.data.clone();
    candidate.game_mode = desired.base_game_mode.unwrap_or(desired.data.game_mode);
    candidate.inventory_revision = expected.data.inventory_revision;
    candidate.inventory_effects = expected.data.inventory_effects.clone();
    if let Some(baseline) = &baseline {
        if !super::super::player_events::initial_state_pending(world, target.player)
            || baseline.uuid != expected.data.uuid
            || baseline.source_actor.is_none()
        {
            return Err(Error::Conflict);
        }
        candidate.inventory_revision = baseline.inventory_revision;
        candidate.inventory_effects = baseline.inventory_effects.clone();
    }
    candidate.restoration_checkpoint = None;
    candidate.ui = desired.ui.clone();
    candidate.cursor = desired.ui.first().cloned().ok_or(Error::InvalidState)?;
    candidate.validate(uuid).map_err(|_| Error::InvalidState)?;
    let supplement = desired.supplement();
    supplement
        .validate(&candidate)
        .map_err(|_| Error::InvalidState)?;
    world
        .resource_mut::<Coordinator>()
        .pending
        .insert(uuid, (id.clone(), expected.xuid.clone()));
    let reply = Reply {
        writer: writer.clone(),
        tx,
        target,
        uuid,
        id: id.clone(),
        desired,
        history: candidate.inventory_effects.clone(),
        retention: Some(retention),
        done: false,
    };
    runtime.spawn(async move {
        let result = writer
            .commit_state_effect(
                uuid,
                &id,
                hash,
                candidate,
                supplement,
                crate::storage::player_writer::StateOrigin { source, baseline },
            )
            .await;
        reply.finish(result);
    });
    Ok(Submission::Queued)
}

/// Keep uncertainty physically withheld, including a genuinely retained disconnected target.
fn uncertain(
    world: &mut World,
    target: &quarantine::Handle,
    writer: &SerializedPlayerProvider,
    uuid: uuid::Uuid,
) {
    writer.require_recovery(uuid);
    if world.get_entity(target.player).is_ok()
        && !quarantine::is_held(world, target.player)
        && let Ok(source) =
            super::super::inventory_effects::RecoverySource::capture(world, target.player)
    {
        let _ = super::super::inventory_effects::fence_for_recovery(world, &source);
    }
}

/// Publish complete raw state only after its canonical record and receipt are acknowledged.
pub(crate) fn poll(world: &mut World) {
    let completions: Vec<_> =
        world
            .get_resource::<Coordinator>()
            .map_or_else(Vec::new, |coordinator| {
                coordinator
                    .rx
                    .lock()
                    .map(|rx| rx.try_iter().collect())
                    .unwrap_or_default()
            });
    for completion in completions {
        let writer = world.resource::<Coordinator>().writer.clone();
        world
            .resource_mut::<Coordinator>()
            .pending
            .remove(&completion.uuid);
        match completion.result {
            Ok(write) => {
                let mut snapshot = (*completion.desired).clone();
                snapshot.data.inventory_revision = write.record.revision;
                snapshot.data.inventory_effects = completion.history;
                snapshot
                    .data
                    .inventory_effects
                    .insert(completion.id.clone(), write.record.clone());
                snapshot.data.restoration_checkpoint = None;
                let live_applied = if !write.already_committed {
                    match quarantine::apply(world, &completion.target, &snapshot) {
                        Ok(applied) => applied,
                        Err(_) => {
                            uncertain(world, &completion.target, &writer, completion.uuid);
                            false
                        }
                    }
                } else {
                    false
                };
                if live_applied {
                    super::source_actors::release_admission(world, completion.target.player);
                    world.entity_mut(completion.target.player).insert(
                        super::super::inventory_effects::InventoryPersistence::from_record(Some(
                            &snapshot.data,
                        )),
                    );
                }
                world.trigger(Committed {
                    player: world
                        .get_entity(completion.target.player)
                        .is_ok()
                        .then_some(completion.target.player),
                    uuid: completion.uuid,
                    id: completion.id,
                    receipt: write.record,
                    already_committed: write.already_committed,
                    live_applied,
                    retained_buffers: quarantine::is_held(world, completion.target.player),
                });
            }
            Err(error) => {
                if error.kind == WriteFailureKind::NotApplied && error.resume_source {
                    quarantine::reject(world, &completion.target);
                } else {
                    uncertain(world, &completion.target, &writer, completion.uuid);
                }
                world.trigger(Failed {
                    player: world
                        .get_entity(completion.target.player)
                        .is_ok()
                        .then_some(completion.target.player),
                    uuid: completion.uuid,
                    id: completion.id,
                    kind: error.kind,
                    retained_buffers: quarantine::is_held(world, completion.target.player),
                });
            }
        }
    }
}
