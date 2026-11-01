//! Exact-source inventory writes are staged, acknowledged and retained independently of guest handles.

use super::*;
use crate::plugin::manifest::PluginCapability;
use crate::server::game::inventory_effects as native;
use crate::storage::InventoryEffectRecord;
use crate::storage::player_writer::WriteFailureKind;
use sha2::{Digest, Sha256};
use std::sync::{
    Mutex, Weak,
    atomic::{AtomicUsize, Ordering},
};

type RequestKey = (uuid::Uuid, String);

/// Outstanding work remains counted after its guest resource is dropped.
struct PendingSlot(Arc<AtomicUsize>);
impl Drop for PendingSlot {
    fn drop(&mut self) {
        let previous = self.0.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0);
    }
}

/// Native copies and their original immutable image stay charged until a definitive result.
struct Retained {
    image: Arc<super::values::Value<super::inventory::Image>>,
    _copies: Arc<super::values::Value<()>>,
    _slot: PendingSlot,
}

/// Progress and retained bytes have the same lifetime and are changed under one lock.
struct Progress {
    status: wit::InventoryStatus,
    retained: Option<Retained>,
}

/// An owned operation contains no scoped capability and cannot resolve a replacement actor.
pub(in crate::plugin::manager) struct Operation {
    key: RequestKey,
    source: uuid::Uuid,
    player: Entity,
    world: WorldReference,
    hash: [u8; 32],
    progress: Mutex<Progress>,
}
impl Operation {
    /// Publish a final result and release pending work exactly once.
    fn finish(&self, status: wit::InventoryStatus) {
        self.finish_retaining(status, false);
    }

    /// A result can be known while the protected source still owns its frozen buffers.
    fn finish_retaining(&self, status: wit::InventoryStatus, retained_buffers: bool) {
        if let Ok(mut progress) = self.progress.lock() {
            progress.status = status;
            if !retained_buffers {
                progress.retained.take();
            }
        }
    }

    /// Release only buffer ownership, preserving the already-observed completion disposition.
    fn release_buffers(&self) {
        if let Ok(mut progress) = self.progress.lock() {
            progress.retained.take();
        }
    }

    /// Read owned progress without touching an actor, world or persistence provider.
    fn status(&self) -> Result<wit::InventoryStatus, wit::AccessError> {
        self.progress
            .lock()
            .map(|progress| progress.status.clone())
            .map_err(|_| wit::AccessError::Unavailable)
    }
}

/// The native worker and undelivered completion queue own this lease, even after world closure.
struct WorkerLease(Arc<Operation>);
impl Drop for WorkerLease {
    fn drop(&mut self) {
        if let Ok(mut progress) = self.0.progress.lock()
            && matches!(
                &progress.status,
                wit::InventoryStatus::Staged | wit::InventoryStatus::Queued
            )
        {
            // No native completion reached the world ledger. Its outcome cannot be inferred.
            progress.status = wit::InventoryStatus::Uncertain;
            progress.retained.take();
        }
    }
}

/// Only the native admission book, not the guest's description, grants initializing writes.
enum Permit {
    Ready,
    Initializing {
        hold: super::super::sessions::OwnedHold,
    },
}
struct Pending {
    operation: Arc<Operation>,
    permit: Permit,
}

/// Weak lookup entries do not retain completed work; staged entries retain accepted intents.
#[derive(Default)]
pub(in crate::plugin::manager) struct Requests {
    staged: Vec<Pending>,
    known: HashMap<RequestKey, Weak<Operation>>,
    pending: Arc<AtomicUsize>,
}

/// Completion delivery never needs to acquire the component manager lock.
#[derive(Resource, Default)]
struct Ledger {
    pending: HashMap<RequestKey, Arc<Operation>>,
    retained: HashMap<RequestKey, Arc<Operation>>,
    released: std::collections::HashSet<Entity>,
}

/// Project only the acknowledged record fields, never native provider diagnostics.
fn receipt(record: &InventoryEffectRecord) -> wit::InventoryReceipt {
    wit::InventoryReceipt {
        revision: record.revision,
        request_hash: record.request_hash.to_vec(),
    }
}

/// Keep mechanical failures typed without exposing provider paths or native messages.
fn error(error: native::EffectError) -> wit::InventoryError {
    match error {
        native::EffectError::NotInstalled | native::EffectError::MissingProvider => {
            wit::InventoryError::NotInstalled
        }
        native::EffectError::UnsupportedProvider => wit::InventoryError::UnsupportedProvider,
        native::EffectError::MissingRuntime => {
            wit::InventoryError::Access(wit::AccessError::Unavailable)
        }
        native::EffectError::MissingPlayer => {
            wit::InventoryError::Access(wit::AccessError::StalePlayer)
        }
        native::EffectError::Busy => wit::InventoryError::Busy,
        native::EffectError::Conflict => wit::InventoryError::Conflict,
        native::EffectError::RecoveryRequired => wit::InventoryError::RecoveryRequired,
        native::EffectError::InvalidIdentifier => wit::InventoryError::InvalidIdentifier,
        native::EffectError::InvalidInventory(_) => wit::InventoryError::InvalidImage,
    }
}

/// Frame both variable-length identifiers before hashing into the engine's receipt namespace.
fn request_key(plugin: &str, request: &str) -> Result<String, wit::InventoryError> {
    if request.is_empty() || request.len() > 256 || request.chars().any(char::is_control) {
        return Err(wit::InventoryError::InvalidIdentifier);
    }
    let mut hash = Sha256::new();
    hash.update(b"unastar.inventory.v1\0");
    hash.update((plugin.len() as u64).to_be_bytes());
    hash.update(plugin.as_bytes());
    hash.update((request.len() as u64).to_be_bytes());
    hash.update(request.as_bytes());
    Ok(format!("{:x}", hash.finalize()))
}

/// Register once in each real world, independently of component instance reloads.
fn install_ledger(world: &mut World) {
    if world.contains_resource::<Ledger>() {
        return;
    }
    world.init_resource::<Ledger>();
    world.add_observer(
        |event: On<native::InventoryEffectCommitted>, mut ledger: ResMut<Ledger>| {
            if let Some(operation) = ledger.pending.remove(&(event.uuid, event.id.clone())) {
                let released = ledger.released.remove(&operation.player);
                let retained = event.retained_buffers && !released;
                let status = if event.record.request_hash == operation.hash {
                    wit::InventoryStatus::Committed(wit::InventoryCommit {
                        receipt: receipt(&event.record),
                        already_committed: event.already_committed,
                        live_applied: event.live_applied,
                    })
                } else {
                    wit::InventoryStatus::Uncertain
                };
                operation.finish_retaining(status, retained);
                if retained {
                    ledger.retained.insert(operation.key.clone(), operation);
                }
            }
        },
    );
    world.add_observer(
        |event: On<native::InventoryEffectFailed>, mut ledger: ResMut<Ledger>| {
            if let Some(operation) = ledger.pending.remove(&(event.uuid, event.id.clone())) {
                let released = ledger.released.remove(&operation.player);
                let retained = event.retained_buffers && !released;
                let status = match event.kind {
                    WriteFailureKind::NotApplied => {
                        wit::InventoryStatus::Rejected(wit::InventoryError::NotApplied)
                    }
                    WriteFailureKind::RecoveryRequired => wit::InventoryStatus::Uncertain,
                };
                operation.finish_retaining(status, retained);
                if retained {
                    ledger.retained.insert(operation.key.clone(), operation);
                }
            }
        },
    );
    world.add_observer(
        |event: On<native::InventoryBuffersReleased>, mut ledger: ResMut<Ledger>| {
            if ledger
                .pending
                .values()
                .any(|operation| operation.player == event.player)
            {
                ledger.released.insert(event.player);
            }
            ledger.retained.retain(|_, operation| {
                if operation.player == event.player {
                    operation.release_buffers();
                    false
                } else {
                    true
                }
            });
        },
    );
}

impl HostContext {
    /// Observe accepted work without releasing or duplicating any reservation.
    #[cfg(test)]
    pub(in crate::plugin::manager) fn inventory_pending_count(&self) -> usize {
        self.inventories.pending.load(Ordering::Acquire)
    }

    /// Charge a new resource reference even when it refers to an existing accepted operation.
    fn own_inventory_operation(
        &mut self,
        operation: Arc<Operation>,
    ) -> Result<Resource<wit::InventoryOperation>, wit::InventoryError> {
        self.resources
            .allocate(
                Entry::InventoryOperation(operation),
                self.api_limits.max_owned_resources,
            )
            .map(Resource::new_own)
            .map_err(wit::InventoryError::Access)
    }

    /// Ordinary writes require fully admitted, connected physical gameplay state.
    fn ready_inventory_source(
        &self,
        world: &World,
        player: Entity,
    ) -> Result<(), wit::InventoryError> {
        match availability(world, player) {
            wit::PlayerAvailability::Ready => Ok(()),
            wit::PlayerAvailability::Quarantined => Err(wit::InventoryError::Busy),
            wit::PlayerAvailability::Recovering => Err(wit::InventoryError::RecoveryRequired),
            _ => Err(wit::InventoryError::Access(wit::AccessError::Unavailable)),
        }
    }

    /// Retain a candidate and its capacity before returning a staged operation to the guest.
    fn stage_inventory(
        &mut self,
        player: Entity,
        reference: PlayerLineage,
        image_rep: u32,
        request: String,
        permit: Permit,
    ) -> Result<Resource<wit::InventoryOperation>, wit::InventoryError> {
        self.require(PluginCapability::WriteInventory)
            .map_err(wit::InventoryError::Access)?;
        let image = self
            .inventory_image(image_rep)
            .map_err(wit::InventoryError::Access)?;
        let (captured, origin) = image
            .captured_binding()
            .map_err(wit::InventoryError::Access)?;
        let world = unsafe { self.world() };
        if captured.id != reference.id || !captured.matches(world, player) {
            return Err(wit::InventoryError::Access(wit::AccessError::StalePlayer));
        }
        if !origin.matches(world) {
            return Err(wit::InventoryError::Access(wit::AccessError::WrongWorld));
        }
        native::available(world).map_err(error)?;
        let id = request_key(&self.plugin_id.0, &request)?;
        let key = (reference.uuid, id);
        let hash = native::request_hash(&image.candidate).map_err(error)?;
        self.inventories
            .known
            .retain(|_, operation| operation.strong_count() > 0);
        if let Some(record) = native::receipt(unsafe { self.world() }, player, &key.1).cloned() {
            if record.request_hash != hash {
                return Err(wit::InventoryError::Conflict);
            }
            // A canonical receipt is read-only proof, even if an old connection's ticket survives.
            let operation = Arc::new(Operation {
                key: key.clone(),
                source: reference.id,
                player,
                world: origin,
                hash,
                progress: Mutex::new(Progress {
                    status: wit::InventoryStatus::Committed(wit::InventoryCommit {
                        receipt: receipt(&record),
                        already_committed: true,
                        live_applied: false,
                    }),
                    retained: None,
                }),
            });
            let resource = self.own_inventory_operation(operation.clone())?;
            self.inventories
                .known
                .insert(key, Arc::downgrade(&operation));
            return Ok(resource);
        }
        let previous = self
            .inventories
            .known
            .get(&key)
            .and_then(Weak::upgrade)
            .or_else(|| {
                unsafe { self.world() }
                    .get_resource::<Ledger>()
                    .and_then(|ledger| ledger.pending.get(&key).cloned())
            });
        if let Some(previous) = previous {
            match previous.status().map_err(wit::InventoryError::Access)? {
                wit::InventoryStatus::Staged | wit::InventoryStatus::Queued => {
                    if previous.hash != hash {
                        return Err(wit::InventoryError::Conflict);
                    }
                    if previous.source != reference.id || previous.world != origin {
                        return Err(wit::InventoryError::Access(wit::AccessError::StalePlayer));
                    }
                    return self.own_inventory_operation(previous);
                }
                wit::InventoryStatus::Committed(_) | wit::InventoryStatus::Uncertain => {
                    return Err(wit::InventoryError::RecoveryRequired);
                }
                wit::InventoryStatus::Rejected(_) | wit::InventoryStatus::Cancelled => (),
            }
        }
        if let Permit::Ready = &permit {
            self.ready_inventory_source(unsafe { self.world() }, player)?;
        }
        let pending = self.inventories.pending.clone();
        pending
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                count.checked_add(1).filter(|count| {
                    *count <= self.api_limits.max_pending_inventory_operations as usize
                })
            })
            .map_err(|_| wit::InventoryError::Access(wit::AccessError::LimitExceeded))?;
        let slot = PendingSlot(pending);
        let copies = image
            .bytes()
            .checked_mul(4)
            .and_then(|bytes| bytes.checked_add(1024))
            .ok_or(wit::InventoryError::Access(wit::AccessError::LimitExceeded))?;
        let copies = self
            .resources
            .value_budget
            .own((), copies)
            .map_err(wit::InventoryError::Access)?;
        let operation = Arc::new(Operation {
            key: key.clone(),
            source: reference.id,
            player,
            world: origin,
            hash,
            progress: Mutex::new(Progress {
                status: wit::InventoryStatus::Staged,
                retained: Some(Retained {
                    image,
                    _copies: copies,
                    _slot: slot,
                }),
            }),
        });
        let resource = self.own_inventory_operation(operation.clone())?;
        self.inventories
            .known
            .insert(key, Arc::downgrade(&operation));
        self.inventories.staged.push(Pending { operation, permit });
        Ok(resource)
    }

    /// Bind a normal submission to the current scoped player, never a caller-supplied account.
    pub(super) fn submit_player_inventory(
        &mut self,
        rep: u32,
        request: String,
        image: u32,
    ) -> Result<Resource<wit::InventoryOperation>, wit::InventoryError> {
        let (reference, player) = self.live_player(rep).map_err(wit::InventoryError::Access)?;
        self.stage_inventory(player, reference, image, request, Permit::Ready)
    }

    /// Validate the owned initialization hold before retaining a request for its exact actor.
    pub(super) fn submit_initial_inventory(
        &mut self,
        hold_rep: u32,
        tx_rep: u32,
        request: String,
        image: u32,
    ) -> Result<Resource<wit::InventoryOperation>, wit::InventoryError> {
        let hold = match self.resources.entries.get(&hold_rep) {
            Some(Entry::AdmissionHold(hold)) => hold.clone(),
            _ => return Err(wit::InventoryError::Access(wit::AccessError::StalePlayer)),
        };
        self.initialization_owner(tx_rep, hold.player, hold_rep)
            .map_err(wit::InventoryError::Access)?;
        let reference =
            ensure_identity(self.world_mut(), hold.player).map_err(wit::InventoryError::Access)?;
        let active = self
            .active_admission
            .as_ref()
            .filter(|active| active.book.same(&hold.book) && active.player == hold.player)
            .map(|active| active.id);
        if active.is_some() {
            // Initialization mutations run in a fresh task after aggregate admission decisions.
            return Err(wit::InventoryError::Access(wit::AccessError::Unavailable));
        }
        self.stage_inventory(
            hold.player,
            reference,
            image,
            request,
            Permit::Initializing { hold },
        )
    }

    /// Read only a receipt already loaded or acknowledged for this exact actor and component ID.
    pub(super) fn player_inventory_receipt(
        &mut self,
        rep: u32,
        request: String,
    ) -> Result<Option<wit::InventoryReceipt>, wit::InventoryError> {
        let (_, player) = self.live_player(rep).map_err(wit::InventoryError::Access)?;
        self.require(PluginCapability::ReadInventory)
            .map_err(wit::InventoryError::Access)?;
        let id = request_key(&self.plugin_id.0, &request)?;
        Ok(native::receipt(unsafe { self.world() }, player, &id).map(receipt))
    }
}

impl super::super::wit_bindings::unastar::plugin::types::HostInventoryOperation for HostContext {
    fn status(
        &mut self,
        resource: Resource<wit::InventoryOperation>,
    ) -> Result<wit::InventoryStatus, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::ReadInventory)?;
        match self.resources.entries.get(&resource.rep()) {
            Some(Entry::InventoryOperation(operation)) => operation.status(),
            _ => Err(wit::AccessError::InvalidArgument),
        }
    }
    fn drop(&mut self, resource: Resource<wit::InventoryOperation>) -> wasmtime::Result<()> {
        if !matches!(
            self.resources.entries.get(&resource.rep()),
            Some(Entry::InventoryOperation(_))
        ) {
            anyhow::bail!("invalid inventory operation resource");
        }
        self.resources.entries.remove(&resource.rep());
        Ok(())
    }
}

/// Submit successful callback intents before releasing initialization holds or messages.
pub(in crate::plugin::manager) fn finish(
    context: &mut HostContext,
    world: &mut World,
    succeeded: bool,
) {
    for pending in std::mem::take(&mut context.inventories.staged) {
        if !succeeded {
            pending.operation.finish(wit::InventoryStatus::Cancelled);
            continue;
        }
        let result = (|| -> Result<native::Submission, wit::InventoryError> {
            let image = pending
                .operation
                .progress
                .lock()
                .map_err(|_| wit::InventoryError::Access(wit::AccessError::Unavailable))?
                .retained
                .as_ref()
                .ok_or(wit::InventoryError::Conflict)?
                .image
                .clone();
            let (source, origin) = image
                .captured_binding()
                .map_err(wit::InventoryError::Access)?;
            if !origin.matches(world) {
                return Err(wit::InventoryError::Access(wit::AccessError::WrongWorld));
            }
            let player = resolve(world, &source).map_err(wit::InventoryError::Access)?;
            match &pending.permit {
                Permit::Ready => context.ready_inventory_source(world, player)?,
                Permit::Initializing { hold } => {
                    if context.pending_admissions.iter().any(|resolution| {
                        resolution.hold.player == player
                            && resolution.hold.book.same(&hold.book)
                            && matches!(
                                &resolution.decision,
                                super::super::sessions::holds::Decision::Reject(_)
                            )
                    }) {
                        return Err(wit::InventoryError::Access(wit::AccessError::Cancelled));
                    }
                    hold.book
                        .validate_initialization(world, hold.id, &hold.owner, player, None)
                        .map_err(|_| wit::InventoryError::Access(wit::AccessError::StalePlayer))?;
                }
            }
            install_ledger(world);
            if world
                .resource::<Ledger>()
                .pending
                .contains_key(&pending.operation.key)
            {
                return Err(wit::InventoryError::Busy);
            }
            let submission = native::submit_retained(
                world,
                player,
                native::InventoryEffect {
                    id: pending.operation.key.1.clone(),
                    expected: (*image.expected).clone(),
                    replacement: (*image.candidate).clone(),
                },
                Arc::new(WorkerLease(pending.operation.clone())),
            )
            .map_err(error)?;
            Ok(submission)
        })();
        match result {
            Ok(native::Submission::Queued) => {
                if let Ok(mut progress) = pending.operation.progress.lock() {
                    progress.status = wit::InventoryStatus::Queued;
                }
                world
                    .resource_mut::<Ledger>()
                    .pending
                    .insert(pending.operation.key.clone(), pending.operation);
            }
            Ok(native::Submission::AlreadyCommitted { record }) => {
                pending
                    .operation
                    .finish(wit::InventoryStatus::Committed(wit::InventoryCommit {
                        receipt: receipt(&record),
                        already_committed: true,
                        live_applied: false,
                    }))
            }
            Err(error) => pending
                .operation
                .finish(wit::InventoryStatus::Rejected(error)),
        }
    }
}

impl super::super::PluginManager {
    /// Stock runtimes prepare durable mechanics in every world before running required guests.
    pub(crate) fn prepare_inventory_support(
        &mut self,
        server: &mut crate::server::game::GameServer,
    ) -> Result<(), String> {
        let manifests = self
            .pending
            .iter()
            .map(|plugin| &plugin.manifest)
            .chain(self.plugins.values().map(|plugin| &plugin.manifest))
            .filter(|manifest| {
                manifest
                    .capabilities
                    .contains(&PluginCapability::WriteInventory)
            })
            .map(|manifest| {
                (
                    manifest.id.0.clone(),
                    manifest.required || self.readiness.is_required(&manifest.id.0),
                )
            })
            .collect::<Vec<_>>();
        if manifests.is_empty() {
            return Ok(());
        }
        if let Err(error) = native::install(server) {
            let mut required = false;
            for (id, essential) in manifests {
                if essential {
                    required = true;
                    self.readiness.require(&id);
                    self.readiness.fail(
                        &id,
                        &format!("inventory persistence is unavailable: {error}"),
                    );
                }
            }
            if required {
                return self.readiness.check();
            }
            tracing::warn!(%error, "Optional component inventory persistence is unavailable");
            return Ok(());
        }
        install_ledger(server.ecs.world_mut());
        Ok(())
    }
}
