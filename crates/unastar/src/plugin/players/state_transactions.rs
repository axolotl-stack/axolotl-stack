//! Full-state intents keep exact target/source authority through durable native completion.

use super::*;
use crate::plugin::manifest::PluginCapability;
use crate::server::game::{
    inventory_effects,
    player_state::{self, quarantine, restoration as native},
};
use crate::storage::player_writer::WriteFailureKind;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::sync::{
    Weak,
    atomic::{AtomicUsize, Ordering},
};

type Key = (uuid::Uuid, String);
struct Slot(Arc<AtomicUsize>);
impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
struct Retained {
    expected: Arc<values::Value<super::state::Image>>,
    desired: Arc<values::Value<super::state::Image>>,
    source: Option<Arc<super::state_sources::Fence>>,
    _copies: Arc<values::Value<()>>,
    _slot: Slot,
}
struct Progress {
    status: wit::StateStatus,
    retained: Option<Arc<Retained>>,
}
pub(super) struct Operation {
    key: Key,
    hash: [u8; 32],
    reference: PlayerLineage,
    world: WorldReference,
    player: Entity,
    progress: Mutex<Progress>,
}
impl Operation {
    /// Only real native completion supplies a receipt or actual live-application fact.
    fn finish(&self, status: wit::StateStatus, keep: bool) {
        let mut progress = self.progress.lock();
        progress.status = status;
        if !keep {
            progress.retained.take();
        }
    }
    /// Tickets remain readable after actor loss, reconnect and guest resource drop.
    fn status(&self) -> wit::StateStatus {
        self.progress.lock().status.clone()
    }
}
/// Worker replies and frozen buffers keep an independent copy lease after status publication.
struct WorkerLease {
    operation: Arc<Operation>,
    _retained: Arc<Retained>,
}
impl Drop for WorkerLease {
    fn drop(&mut self) {
        let mut progress = self.operation.progress.lock();
        if matches!(
            progress.status,
            wit::StateStatus::Staged | wit::StateStatus::Queued
        ) {
            progress.status = wit::StateStatus::Uncertain;
            progress.retained.take();
        }
    }
}
enum Permit {
    Ready,
    Initializing(super::super::sessions::OwnedHold),
}
struct Staged {
    operation: Arc<Operation>,
    permit: Permit,
}
#[derive(Default)]
pub(in crate::plugin::manager) struct Requests {
    staged: Vec<Staged>,
    known: HashMap<Key, Weak<Operation>>,
    pending: Arc<AtomicUsize>,
}
#[derive(Resource, Default)]
struct Ledger {
    pending: HashMap<Key, Arc<Operation>>,
}

/// Frame both identities so a guest identifier can never dispatch another module's work.
fn request_key(module: &str, request: &str) -> Result<String, wit::StateError> {
    if request.is_empty() || request.len() > 256 || request.chars().any(char::is_control) {
        return Err(wit::StateError::InvalidIdentifier);
    }
    let mut hash = Sha256::new();
    hash.update(b"unastar.state.restore.v1\0");
    for value in [module, request] {
        hash.update((value.len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    Ok(format!("{:x}", hash.finalize()))
}
/// Only canonical native records produce guest-visible durable proof.
fn receipt(record: &crate::storage::InventoryEffectRecord) -> wit::InventoryReceipt {
    wit::InventoryReceipt {
        revision: record.revision,
        request_hash: record.request_hash.to_vec(),
    }
}
/// Keep worker diagnostics internal while preserving typed recovery obligations.
fn error(error: native::Error) -> wit::StateError {
    match error {
        native::Error::Unavailable => wit::StateError::NotInstalled,
        native::Error::Busy => wit::StateError::Busy,
        native::Error::Conflict => wit::StateError::Conflict,
        native::Error::InvalidState => wit::StateError::InvalidImage,
        native::Error::RecoveryRequired => wit::StateError::RecoveryRequired,
    }
}
/// Observer snapshots carry owned facts and never acquire the component manager lock.
fn install_ledger(world: &mut World) {
    if world.contains_resource::<Ledger>() {
        return;
    }
    world.init_resource::<Ledger>();
    world.add_observer(|event: On<native::Committed>, mut ledger: ResMut<Ledger>| {
        if let Some(operation) = ledger.pending.remove(&(event.uuid, event.id.clone())) {
            let status = if event.receipt.request_hash == operation.hash
                && event.player.is_none_or(|player| player == operation.player)
                && (!event.live_applied || event.player == Some(operation.player))
            {
                wit::StateStatus::Committed(wit::StateCommit {
                    receipt: receipt(&event.receipt),
                    already_committed: event.already_committed,
                    live_applied: event.live_applied,
                })
            } else {
                wit::StateStatus::Uncertain
            };
            operation.finish(status, event.retained_buffers);
        }
    });
    world.add_observer(|event: On<native::Failed>, mut ledger: ResMut<Ledger>| {
        if let Some(operation) = ledger.pending.remove(&(event.uuid, event.id.clone())) {
            let status = match event.kind {
                _ if event
                    .player
                    .is_some_and(|player| player != operation.player) =>
                {
                    wit::StateStatus::Uncertain
                }
                WriteFailureKind::NotApplied => {
                    wit::StateStatus::Rejected(wit::StateError::NotApplied)
                }
                WriteFailureKind::RecoveryRequired => wit::StateStatus::Uncertain,
            };
            operation.finish(status, event.retained_buffers);
        }
    });
}

impl HostContext {
    /// Bound resources separately from the native operation's retained asynchronous lifetime.
    fn own_state_operation(
        &mut self,
        operation: Arc<Operation>,
    ) -> Result<Resource<wit::StateOperation>, wit::StateError> {
        self.resources
            .allocate(
                Entry::StateOperation(operation),
                self.api_limits.max_owned_resources,
            )
            .map(Resource::new_own)
            .map_err(wit::StateError::Access)
    }
    /// Resolve account/state authority before reserving any native work or freezing a target.
    fn stage_state(
        &mut self,
        target: (Entity, PlayerLineage),
        request: String,
        expected_rep: u32,
        desired_rep: u32,
        source_rep: Option<u32>,
        permit: Permit,
    ) -> Result<Resource<wit::StateOperation>, wit::StateError> {
        let (player, reference) = target;
        self.require(PluginCapability::RestorePlayerState)
            .map_err(wit::StateError::Access)?;
        let expected = self
            .state_image(expected_rep)
            .map_err(wit::StateError::Access)?;
        let desired = self
            .state_image(desired_rep)
            .map_err(wit::StateError::Access)?;
        let (original, world) = expected.live_binding().map_err(wit::StateError::Access)?;
        if original.id != reference.id
            || !world.matches(unsafe { self.world() })
            || expected.expected != expected.candidate
        {
            return Err(wit::StateError::Conflict);
        }
        if desired.owner.uuid != reference.identity.uuid
            || desired.owner.authenticated_xuid != reference.identity.authenticated_xuid
        {
            return Err(wit::StateError::WrongAccount);
        }
        let actual = unsafe { self.world() };
        let key = (reference.uuid, request_key(&self.plugin_id.0, &request)?);
        let hash = desired.fingerprint().map_err(wit::StateError::Access)?;
        // Canonical proof wins over an old retained operation ticket on reconnect.
        if let Some(record) = inventory_effects::receipt(actual, player, &key.1) {
            if record.request_hash != hash {
                return Err(wit::StateError::Conflict);
            }
            let operation = Arc::new(Operation {
                key,
                hash,
                reference,
                world,
                player,
                progress: Mutex::new(Progress {
                    status: wit::StateStatus::Committed(wit::StateCommit {
                        receipt: receipt(record),
                        already_committed: true,
                        live_applied: false,
                    }),
                    retained: None,
                }),
            });
            return self.own_state_operation(operation);
        }
        let identity = actual
            .get_resource::<crate::world::WorldIdentity>()
            .ok_or(wit::StateError::NotInstalled)?;
        if desired
            .destination
            .is_some_and(|destination| !destination.matches(actual))
            || desired.world.uuid != identity.id.to_string()
            || desired.candidate.data.dimension != world.dimension
        {
            return Err(wit::StateError::Access(wit::AccessError::WrongWorld));
        }
        if !native::available(actual) {
            return Err(wit::StateError::NotInstalled);
        }
        self.states.known.retain(|_, weak| weak.strong_count() > 0);
        if let Some(old) = self.states.known.get(&key).and_then(Weak::upgrade) {
            match old.status() {
                wit::StateStatus::Staged | wit::StateStatus::Queued => {
                    if old.hash != hash {
                        return Err(wit::StateError::Conflict);
                    }
                    if old.reference.id != reference.id || old.world != world {
                        return Err(wit::StateError::Access(wit::AccessError::StalePlayer));
                    }
                    return self.own_state_operation(old);
                }
                wit::StateStatus::Committed(_) | wit::StateStatus::Uncertain => {
                    return Err(wit::StateError::RecoveryRequired);
                }
                _ => (),
            }
        }
        if matches!(permit, Permit::Ready)
            && !matches!(
                availability(unsafe { self.world() }, player),
                wit::PlayerAvailability::Ready
            )
        {
            return Err(wit::StateError::Access(wit::AccessError::Unavailable));
        }
        let source = source_rep
            .map(|rep| self.state_fence(rep))
            .transpose()
            .map_err(wit::StateError::Access)?;
        if let Some(source) = &source
            && (source.reference.uuid != reference.uuid
                || source.reference.identity.authenticated_xuid
                    != reference.identity.authenticated_xuid
                || source.image.expected != desired.expected)
        {
            return Err(wit::StateError::Conflict);
        }
        let counter = self.states.pending.clone();
        counter
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1).filter(|value| {
                    *value <= self.api_limits.max_pending_inventory_operations as usize
                })
            })
            .map_err(|_| wit::StateError::Access(wit::AccessError::LimitExceeded))?;
        let slot = Slot(counter);
        let bytes = expected
            .bytes()
            .checked_add(desired.bytes())
            .and_then(|bytes| bytes.checked_mul(4))
            .ok_or(wit::StateError::Access(wit::AccessError::LimitExceeded))?;
        let copies = self
            .resources
            .value_budget
            .own((), bytes)
            .map_err(wit::StateError::Access)?;
        let retained = Arc::new(Retained {
            expected,
            desired,
            source,
            _copies: copies,
            _slot: slot,
        });
        let operation = Arc::new(Operation {
            key: key.clone(),
            hash,
            reference,
            world,
            player,
            progress: Mutex::new(Progress {
                status: wit::StateStatus::Staged,
                retained: Some(retained),
            }),
        });
        let resource = self.own_state_operation(operation.clone())?;
        self.states.known.insert(key, Arc::downgrade(&operation));
        self.states.staged.push(Staged { operation, permit });
        Ok(resource)
    }
    /// Published targets use a genuine scoped player; initialization requires its owned hold.
    pub(super) fn submit_player_state(
        &mut self,
        rep: u32,
        request: String,
        expected: u32,
        desired: u32,
        source: Option<u32>,
    ) -> Result<Resource<wit::StateOperation>, wit::StateError> {
        let (reference, player) = self.live_player(rep).map_err(wit::StateError::Access)?;
        self.stage_state(
            (player, reference),
            request,
            expected,
            desired,
            source,
            Permit::Ready,
        )
    }
    /// An initializing plan is valid only after aggregate Deferred, in a fresh owner task.
    pub(super) fn submit_initial_state(
        &mut self,
        hold_rep: u32,
        tx: u32,
        request: String,
        expected: u32,
        desired: u32,
        source: Option<u32>,
    ) -> Result<Resource<wit::StateOperation>, wit::StateError> {
        let hold = match self.resources.entries.get(&hold_rep) {
            Some(Entry::AdmissionHold(hold)) => hold.clone(),
            _ => return Err(wit::StateError::Access(wit::AccessError::StalePlayer)),
        };
        self.initialization_owner(tx, hold.player, hold_rep)
            .map_err(wit::StateError::Access)?;
        if self.active_admission.is_some() {
            return Err(wit::StateError::Access(wit::AccessError::Unavailable));
        }
        let reference =
            ensure_identity(self.world_mut(), hold.player).map_err(wit::StateError::Access)?;
        self.stage_state(
            (hold.player, reference),
            request,
            expected,
            desired,
            source,
            Permit::Initializing(hold),
        )
    }
    /// Only this module's loaded or acknowledged canonical receipt is observable.
    pub(super) fn state_receipt(
        &mut self,
        rep: u32,
        request: String,
    ) -> Result<Option<wit::InventoryReceipt>, wit::StateError> {
        self.require(PluginCapability::RestorePlayerState)
            .map_err(wit::StateError::Access)?;
        let (_, player) = self.live_player(rep).map_err(wit::StateError::Access)?;
        let key = request_key(&self.plugin_id.0, &request)?;
        Ok(inventory_effects::receipt(unsafe { self.world() }, player, &key).map(receipt))
    }
}

/// Drop of a queued, pre-write owner world is definite; accepted workers retain their own lease.
struct QueueGuard {
    operation: Arc<Operation>,
    target: quarantine::Handle,
    retained: Arc<Retained>,
    remaining: u16,
    armed: bool,
}
impl Drop for QueueGuard {
    fn drop(&mut self) {
        if self.armed {
            self.operation.finish(
                wit::StateStatus::Rejected(wit::StateError::SourceGone),
                false,
            );
        }
    }
}

/// A full pre-write queue is a definite rejection and reinstalls only the original target.
fn enqueue(world: &mut World, queued: QueueGuard) {
    let operation = queued.operation.clone();
    let target = queued.target.clone();
    if !crate::server::worlds::queue_router_request(world, move |router, _| run(router, queued)) {
        operation.finish(wit::StateStatus::Rejected(wit::StateError::Busy), false);
        quarantine::reject(world, &target);
    }
}
/// Compare every actual same-account actor, including withheld and genuinely retained bodies.
fn duplicates_allowed(
    router: &mut crate::server::worlds::DimensionRouter,
    operation: &Operation,
    source: Option<&Arc<super::state_sources::Fence>>,
) -> bool {
    for (_, server) in router.iter_mut() {
        let world = server.ecs.world_mut();
        let mut query=world.query_filtered::<(Entity,&crate::entity::components::PlayerUuid),(With<crate::entity::components::Player>,)>();
        let others: Vec<_> = query
            .iter(world)
            .filter_map(|(actor, uuid)| {
                let same = uuid.0 == operation.reference.uuid
                    || operation
                        .reference
                        .identity
                        .authenticated_xuid
                        .is_some_and(|xuid| {
                            crate::server::game::player_state::account_xuid(world, actor)
                                .and_then(|text| text.parse::<u64>().ok())
                                == Some(xuid)
                        });
                (same && !(operation.world.matches(world) && actor == operation.player))
                    .then_some(actor)
            })
            .collect();
        for actor in others {
            if !source.is_some_and(|source| {
                source.world.matches(world)
                    && resolve(world, &source.reference) == Ok(actor)
                    && source.matches(world)
            }) {
                return false;
            }
        }
    }
    true
}
/// Run loading and native submission only after all local scopes and manager locks were released.
fn run(router: &mut crate::server::worlds::DimensionRouter, mut queued: QueueGuard) {
    let operation = queued.operation.clone();
    let rejected =
        |queued: &mut QueueGuard, router: &mut crate::server::worlds::DimensionRouter, error| {
            if let Some(server) = router.world_mut(operation.world.dimension)
                && operation.world.matches(server.ecs.world())
            {
                quarantine::reject(server.ecs.world_mut(), &queued.target);
            }
            queued.armed = false;
            operation.finish(wit::StateStatus::Rejected(error), false);
        };
    if !router
        .world(operation.world.dimension)
        .is_some_and(|server| {
            operation.world.matches(server.ecs.world())
                && operation
                    .reference
                    .matches(server.ecs.world(), operation.player)
                && quarantine::matches(server.ecs.world(), &queued.target)
        })
    {
        rejected(&mut queued, router, wit::StateError::SourceGone);
        return;
    }
    if !duplicates_allowed(router, &operation, queued.retained.source.as_ref()) {
        rejected(&mut queued, router, wit::StateError::Conflict);
        return;
    }
    if let Some(source) = &queued.retained.source
        && !source.permits(&operation.key.1, operation.hash)
    {
        rejected(&mut queued, router, wit::StateError::Conflict);
        return;
    }
    let feet = glam::DVec3::from_array(queued.retained.desired.candidate.data.position)
        - glam::DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
    match router.prepare_state_position(operation.world.dimension, feet) {
        Ok(true) => (),
        Ok(false) if queued.remaining > 0 => {
            queued.remaining -= 1;
            let world = router
                .world_mut(operation.world.dimension)
                .expect("validated owner world")
                .ecs
                .world_mut();
            enqueue(world, queued);
            return;
        }
        Ok(false) => {
            rejected(&mut queued, router, wit::StateError::TimedOut);
            return;
        }
        Err(_) => {
            rejected(&mut queued, router, wit::StateError::UnsafePosition);
            return;
        }
    }
    let world = router
        .world_mut(operation.world.dimension)
        .expect("validated owner world")
        .ecs
        .world_mut();
    install_ledger(world);
    let lease = Arc::new(WorkerLease {
        operation: operation.clone(),
        _retained: queued.retained.clone(),
    });
    match native::submit(
        world,
        queued.target.clone(),
        operation.key.1.clone(),
        operation.hash,
        queued.retained.expected.expected.clone(),
        queued.retained.desired.candidate.clone(),
        lease,
    ) {
        Ok(native::Submission::Queued) => {
            if let Some(source) = &queued.retained.source {
                source.consume(operation.key.1.clone(), operation.hash);
            }
            world
                .resource_mut::<Ledger>()
                .pending
                .insert(operation.key.clone(), operation.clone());
            operation.progress.lock().status = wit::StateStatus::Queued;
            queued.armed = false;
        }
        Ok(native::Submission::AlreadyCommitted(record)) => {
            quarantine::reject(world, &queued.target);
            queued.armed = false;
            operation.finish(
                wit::StateStatus::Committed(wit::StateCommit {
                    receipt: receipt(&record),
                    already_committed: true,
                    live_applied: false,
                }),
                false,
            );
        }
        Err(error) => {
            quarantine::reject(world, &queued.target);
            queued.armed = false;
            operation.finish(wit::StateStatus::Rejected(self::error(error)), false);
        }
    }
}

/// Freeze before simulation can advance the expected image; storage work starts later.
pub(in crate::plugin::manager) fn finish(
    context: &mut HostContext,
    world: &mut World,
    succeeded: bool,
) {
    for staged in std::mem::take(&mut context.states.staged) {
        let operation = staged.operation;
        if !succeeded {
            operation.finish(wit::StateStatus::Cancelled, false);
            continue;
        }
        let retained = operation
            .progress
            .lock()
            .retained
            .clone()
            .expect("staged data lease");
        let result = (|| {
            if !operation.world.matches(world)
                || !operation.reference.matches(world, operation.player)
                || world.resource::<crate::ecs::TickCounter>().current
                    != retained.expected.captured_tick
            {
                return Err(wit::StateError::Conflict);
            }
            match &staged.permit {
                Permit::Ready => {
                    if !matches!(
                        availability(world, operation.player),
                        wit::PlayerAvailability::Ready
                    ) {
                        return Err(wit::StateError::Access(wit::AccessError::Unavailable));
                    }
                }
                Permit::Initializing(hold) => {
                    hold.book
                        .validate_initialization(world, hold.id, &hold.owner, hold.player, None)
                        .map_err(|_| wit::StateError::Access(wit::AccessError::StalePlayer))?;
                    if context.pending_admissions.iter().any(|resolution| {
                        resolution.hold.player == operation.player
                            && matches!(
                                resolution.decision,
                                super::super::sessions::holds::Decision::Reject(_)
                            )
                    }) {
                        return Err(wit::StateError::Access(wit::AccessError::Cancelled));
                    }
                }
            }
            player_state::preflight_restore(world, operation.player, &retained.desired.candidate)
                .map_err(|_| wit::StateError::InvalidImage)?;
            let lease = Arc::new(WorkerLease {
                operation: operation.clone(),
                _retained: retained.clone(),
            });
            quarantine::hold(
                world,
                operation.player,
                retained.expected.expected.clone(),
                quarantine::Role::Target,
                lease,
            )
            .map_err(|_| wit::StateError::Conflict)
        })();
        match result {
            Ok(target) => {
                let queued = QueueGuard {
                    operation: operation.clone(),
                    target,
                    retained,
                    remaining: 200,
                    armed: true,
                };
                enqueue(world, queued);
            }
            Err(error) => operation.finish(wit::StateStatus::Rejected(error), false),
        }
    }
}

impl super::super::wit_bindings::unastar::plugin::types::HostStateOperation for HostContext {
    fn status(
        &mut self,
        resource: Resource<wit::StateOperation>,
    ) -> Result<wit::StateStatus, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::RestorePlayerState)?;
        match self.resources.entries.get(&resource.rep()) {
            Some(Entry::StateOperation(operation)) => Ok(operation.status()),
            _ => Err(wit::AccessError::InvalidArgument),
        }
    }
    fn drop(&mut self, resource: Resource<wit::StateOperation>) -> wasmtime::Result<()> {
        self.resources.entries.remove(&resource.rep());
        Ok(())
    }
}
