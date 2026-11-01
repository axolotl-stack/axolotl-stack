//! Receipt-backed loaded terrain intents, with no live world borrow retained by work.

use super::*;
use crate::storage::{
    column_runtime as native,
    column_writer::{ColumnReceipt, ColumnWriteError, ReceiptOutcome},
};
use crate::world::WorldIdentity;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Weak,
};

type Failure = wit::DurableBlockError;
type Status = wit::DurableBlockStatus;

/// An owned observation of staging, disk and native publication, never client ACK.
pub(in crate::plugin::manager) struct State(Mutex<Status>);
impl State {
    /// New requests do not yet own native IO.
    fn new() -> Self {
        Self(Mutex::new(Status::Staged))
    }
    /// Status resources remain readable after their source scope expires.
    fn get(&self) -> Status {
        self.0.lock().clone()
    }
    /// Update only the request's own historical observation.
    fn set(&self, value: Status) {
        *self.0.lock() = value;
    }
    /// Cancellation is definitive only before IO acceptance.
    fn cancel(&self) -> bool {
        let mut status = self.0.lock();
        if matches!(*status, Status::Staged) {
            *status = Status::NotApplied(Failure::Access(wit::AccessError::Cancelled));
            true
        } else {
            false
        }
    }
}
pub(in crate::plugin::manager) struct Work {
    state: Arc<State>,
    world: WorldReference,
    identity: uuid::Uuid,
    key: String,
    id: String,
    digest: [u8; 32],
    cells: Vec<OwnedReplacement>,
    owner: Weak<()>,
    ticket: Option<native::ColumnTicket>,
    _count: publication::Count,
}
/// Callback staging is separate from accepted native storage ownership.
pub(in crate::plugin::manager) enum Staged {
    New(Work),
    Resume {
        state: Arc<State>,
        world: WorldReference,
        id: String,
        owner: Weak<()>,
        count: publication::Count,
    },
}
impl Staged {
    /// Shared status reference independent of the guest resource handle.
    fn state(&self) -> &Arc<State> {
        match self {
            Self::New(work) => &work.state,
            Self::Resume { state, .. } => state,
        }
    }
    /// Validate source world and owner before any native submission.
    fn current(&self, world: &World) -> bool {
        match self {
            Self::New(work) => {
                work.world.matches(world)
                    && world
                        .get_resource::<WorldIdentity>()
                        .is_some_and(|identity| identity.id == work.identity)
                    && work.owner.upgrade().is_some()
            }
            Self::Resume {
                world: source,
                owner,
                ..
            } => source.matches(world) && owner.upgrade().is_some(),
        }
    }
    /// A key is scoped by the trusted module identity and configured world UUID.
    fn id(&self) -> &str {
        match self {
            Self::New(work) => &work.id,
            Self::Resume { id, .. } => id,
        }
    }
}
#[derive(Resource, Default)]
struct Book {
    queued: Vec<Staged>,
    active: BTreeMap<String, Work>,
}
impl Drop for Book {
    fn drop(&mut self) {
        for queued in &self.queued {
            queued.state().set(Status::NotApplied(Failure::Access(
                wit::AccessError::WorldClosed,
            )));
        }
        for work in self.active.values() {
            work.state.set(Status::RecoveryRequired);
        }
    }
}

/// Never interpret a key as a path, command or dispatch selector.
fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}
/// Resolve only the provider already installed in this exact transaction world.
fn writer(world: &World) -> Result<Arc<crate::storage::column_writer::ColumnWriter>, Failure> {
    world
        .get_resource::<ChunkManager>()
        .and_then(ChunkManager::provider)
        .and_then(|provider| provider.column_writer())
        .ok_or(Failure::UnsupportedProvider)
}
/// Build a bounded host-owned namespace; the guest controls only the final key.
fn namespace(context: &HostContext, world: &World, key: &str) -> Result<String, Failure> {
    if !valid_key(key) {
        return Err(Failure::InvalidKey);
    }
    let identity = world
        .get_resource::<WorldIdentity>()
        .ok_or(Failure::Access(wit::AccessError::Unavailable))?;
    if identity.id.is_nil()
        || world
            .get_resource::<ChunkManager>()
            .is_none_or(|manager| manager.dimension() != identity.dimension)
    {
        return Err(Failure::Access(wit::AccessError::WrongWorld));
    }
    let mut hash = Sha256::new();
    hash.update(b"unastar-world-write-namespace-v1");
    framed(&mut hash, context.plugin_id.0.as_bytes());
    hash.update(identity.id.as_bytes());
    let digest = hash.finalize();
    let mut id = String::from("wb/");
    use std::fmt::Write;
    for byte in &digest[..16] {
        write!(&mut id, "{byte:02x}").expect("string write");
    }
    id.push('/');
    id.push_str(key);
    Ok(id)
}
/// Hash portable typed state rather than ephemeral registry IDs or actor payloads.
fn value(hash: &mut Sha256, registry: &BlockRegistry, runtime: u32) -> Result<(), Failure> {
    let mut value = crate::plugin::manager::blocks::registry_value(registry, runtime)
        .map_err(Failure::Access)?;
    framed(hash, value.identifier.as_bytes());
    value.properties.sort_by(|a, b| a.name.cmp(&b.name));
    hash.update((value.properties.len() as u64).to_le_bytes());
    for property in value.properties {
        framed(hash, property.name.as_bytes());
        match property.value {
            wit::BlockPropertyValue::Byte(value) => hash.update([1, value]),
            wit::BlockPropertyValue::Integer(value) => {
                hash.update([2]);
                hash.update(value.to_le_bytes());
            }
            wit::BlockPropertyValue::Text(value) => {
                hash.update([3]);
                framed(hash, value.as_bytes());
            }
        }
    }
    Ok(())
}
/// Frame adjacent values without allocating a serialized command/data envelope.
fn framed(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}
/// Compute a stable precondition/change digest before the guest persists its intent.
fn digest(
    context: &HostContext,
    world: &World,
    cells: &[OwnedReplacement],
) -> Result<[u8; 32], Failure> {
    let identity = world
        .get_resource::<WorldIdentity>()
        .ok_or(Failure::Access(wit::AccessError::Unavailable))?;
    let mut sorted: Vec<_> = cells.iter().collect();
    sorted.sort_by_key(|cell| cell.expected.position.to_array());
    let mut hash = Sha256::new();
    hash.update(b"unastar-durable-block-intent-v1");
    framed(&mut hash, context.plugin_id.0.as_bytes());
    hash.update(identity.id.as_bytes());
    hash.update(identity.dimension.to_le_bytes());
    hash.update((sorted.len() as u64).to_le_bytes());
    for replacement in sorted {
        let expected = &replacement.expected;
        for coordinate in expected.position.to_array() {
            hash.update(coordinate.to_le_bytes());
        }
        for state in [
            expected.layers.primary,
            expected.layers.secondary,
            replacement.proposed.primary,
            replacement.proposed.secondary,
        ] {
            value(&mut hash, &expected.registry, state)?;
        }
    }
    Ok(hash.finalize().into())
}
/// Validate the borrowed builder and scope without consuming its detached values.
fn candidate(context: &mut HostContext, tx: u32, edit: u32) -> Result<(), Failure> {
    let scope = context.transaction(tx).map_err(Failure::Access)?;
    context
        .require(PluginCapability::WriteDurableBlocks)
        .map_err(Failure::Access)?;
    let Some(Entry::BlockEdit(edit)) = context.resources.entries.get(&edit) else {
        return Err(Failure::Access(wit::AccessError::InvalidArgument));
    };
    if edit.world != scope.world {
        return Err(Failure::Access(wit::AccessError::WrongWorld));
    }
    if edit.staged {
        return Err(Failure::Block(Error::AlreadyStaged));
    }
    if edit.cells.is_empty() {
        return Err(Failure::Block(Error::EmptyBatch));
    }
    let columns: HashSet<_> = edit.cells.iter().map(|cell| cell.expected.chunk).collect();
    if columns.len() > crate::storage::column_writer::MAX_COLUMNS {
        return Err(Failure::Capacity);
    }
    Ok(())
}
/// Scope-only digest access does not acquire IO, pins or mutation reservations.
pub(in crate::plugin::manager) fn intent(
    context: &mut HostContext,
    tx: u32,
    edit: u32,
) -> Result<Vec<u8>, Failure> {
    candidate(context, tx, edit)?;
    let Some(Entry::BlockEdit(edit)) = context.resources.entries.get(&edit) else {
        unreachable!()
    };
    digest(context, unsafe { context.world() }, &edit.cells).map(Vec::from)
}
/// Convert a validated cached record into its caller-owned scoped key.
fn receipt(key: &str, record: &ColumnReceipt) -> Result<wit::DurableBlockReceipt, Failure> {
    let intent = record.intent.ok_or(Failure::Conflict)?;
    Ok(wit::DurableBlockReceipt {
        key: key.into(),
        epoch: record.epoch,
        intent: intent.to_vec(),
    })
}
/// Read a real receipt or retained native state without waiting on IO.
pub(in crate::plugin::manager) fn lookup(
    context: &mut HostContext,
    tx: u32,
    key: &str,
) -> Result<Option<Status>, Failure> {
    context.transaction(tx).map_err(Failure::Access)?;
    context
        .require(PluginCapability::WriteDurableBlocks)
        .map_err(Failure::Access)?;
    let world = unsafe { context.world() };
    let id = namespace(context, world, key)?;
    if let Some(book) = world.get_resource::<Book>() {
        if let Some(work) = book.active.get(&id) {
            return Ok(Some(work.state.get()));
        }
        if let Some(work) = book.queued.iter().find(|work| work.id() == id) {
            return Ok(Some(work.state().get()));
        }
    }
    match writer(world)?.peek_receipt(&id).map_err(failure)? {
        None => Ok(None),
        Some(record) if record.outcome == ReceiptOutcome::Applied => {
            Ok(Some(Status::Committed(receipt(key, &record)?)))
        }
        Some(_) => Ok(Some(Status::NotApplied(Failure::Conflict))),
    }
}
/// Acquire resource/count ownership before exposing a staged request.
fn own(
    context: &mut HostContext,
    state: Arc<State>,
) -> Result<Resource<wit::DurableBlockRequest>, Failure> {
    context
        .resources
        .allocate(
            Entry::DurableBlockRequest(state),
            context.api_limits.max_owned_resources,
        )
        .map(Resource::new_own)
        .map_err(Failure::Access)
}
/// Stable intent and existing receipt checks precede new live-state revalidation.
pub(in crate::plugin::manager) fn stage(
    context: &mut HostContext,
    tx: u32,
    key: &str,
    edit_rep: u32,
) -> Result<Resource<wit::DurableBlockRequest>, Failure> {
    candidate(context, tx, edit_rep)?;
    let Some(Entry::BlockEdit(edit)) = context.resources.entries.get(&edit_rep) else {
        unreachable!()
    };
    let world = unsafe { context.world() };
    let id = namespace(context, world, key)?;
    let digest = digest(context, world, &edit.cells)?;
    if let Some(record) = writer(world)?.peek_receipt(&id).map_err(failure)? {
        if record.intent != Some(digest) {
            return Err(Failure::Conflict);
        }
        // An active native owner must publish/reconcile before a disk-only
        // receipt can be interpreted as a committed in-memory result.
        if world
            .get_resource::<Book>()
            .is_some_and(|book| book.active.contains_key(&id))
        {
            return Err(Failure::RecoveryRequired);
        }
        let state = Arc::new(State::new());
        state.set(if record.outcome == ReceiptOutcome::Applied {
            Status::Committed(receipt(key, &record)?)
        } else {
            Status::NotApplied(Failure::Conflict)
        });
        return own(context, state);
    }
    for cell in &edit.cells {
        cell.expected.current(world).map_err(Failure::Block)?;
    }
    let scope_world = edit.world;
    let identity = world
        .get_resource::<WorldIdentity>()
        .ok_or(Failure::Access(wit::AccessError::Unavailable))?
        .id;
    if context.resources.durable_blocks
        >= context.api_limits.max_durable_block_writes_per_tick.min(32)
    {
        return Err(Failure::Access(wit::AccessError::LimitExceeded));
    }
    let count = publication::Count::acquire(
        &context.durable_block_count,
        1,
        context.api_limits.max_pending_durable_block_writes.min(128),
    )
    .map_err(Failure::Access)?;
    let state = Arc::new(State::new());
    let resource = own(context, state.clone())?;
    let Some(Entry::BlockEdit(edit)) = context.resources.entries.get_mut(&edit_rep) else {
        unreachable!()
    };
    edit.staged = true;
    let cells = std::mem::take(&mut edit.cells);
    context.resources.durable_blocks += 1;
    context.pending_durable_blocks.push(Staged::New(Work {
        state,
        world: scope_world,
        identity,
        key: key.into(),
        id,
        digest,
        cells,
        owner: Arc::downgrade(&context.command_owner),
        ticket: None,
        _count: count,
    }));
    Ok(resource)
}
/// Resume only an operation still retained in this actual world, never a guessed
/// reconstruction from newly moving actors or current cells.
pub(in crate::plugin::manager) fn resume(
    context: &mut HostContext,
    tx: u32,
    key: &str,
) -> Result<Resource<wit::DurableBlockRequest>, Failure> {
    let scope = context.transaction(tx).map_err(Failure::Access)?;
    context
        .require(PluginCapability::WriteDurableBlocks)
        .map_err(Failure::Access)?;
    let world = unsafe { context.world() };
    let id = namespace(context, world, key)?;
    let recoverable = world
        .get_resource::<Book>()
        .and_then(|book| book.active.get(&id))
        .is_some_and(|work| matches!(work.state.get(), Status::RecoveryRequired));
    if !recoverable {
        return Err(Failure::RecoveryRequired);
    }
    if context.resources.durable_blocks
        >= context.api_limits.max_durable_block_writes_per_tick.min(32)
    {
        return Err(Failure::Access(wit::AccessError::LimitExceeded));
    }
    let count = publication::Count::acquire(
        &context.durable_block_count,
        1,
        context.api_limits.max_pending_durable_block_writes.min(128),
    )
    .map_err(Failure::Access)?;
    let state = Arc::new(State::new());
    let resource = own(context, state.clone())?;
    context.resources.durable_blocks += 1;
    context.pending_durable_blocks.push(Staged::Resume {
        state,
        world: scope.world,
        id,
        owner: Arc::downgrade(&context.command_owner),
        count,
    });
    Ok(resource)
}
/// Callback errors discard requests before native IO is submitted.
pub(in crate::plugin::manager) fn finish(
    context: &mut HostContext,
    world: &mut World,
    succeeded: bool,
) {
    for request in std::mem::take(&mut context.pending_durable_blocks) {
        if !succeeded {
            request.state().set(Status::NotApplied(Failure::Access(
                wit::AccessError::CallbackFailed,
            )));
            continue;
        }
        if !matches!(request.state().get(), Status::Staged) {
            continue;
        }
        if !request.current(world) {
            request.state().set(Status::NotApplied(Failure::Access(
                wit::AccessError::WrongWorld,
            )));
            continue;
        }
        world.init_resource::<Book>();
        let mut book = world.resource_mut::<Book>();
        if book.queued.len() + book.active.len() >= 128 {
            request.state().set(Status::NotApplied(Failure::Capacity));
            continue;
        }
        if matches!(&request, Staged::New(_))
            && (book.active.contains_key(request.id())
                || book.queued.iter().any(|queued| queued.id() == request.id()))
        {
            request.state().set(Status::NotApplied(Failure::Conflict));
            continue;
        }
        request.state().set(Status::Pending);
        book.queued.push(request);
    }
}
/// Map exact native rejection classes without turning uncertainty into rollback.
fn failure(error: ColumnWriteError) -> Failure {
    match error {
        ColumnWriteError::Unsupported => Failure::UnsupportedProvider,
        ColumnWriteError::Invalid => Failure::Block(Error::Unsupported),
        ColumnWriteError::Stale | ColumnWriteError::Conflict => Failure::Conflict,
        ColumnWriteError::Busy => Failure::Capacity,
        ColumnWriteError::Closed | ColumnWriteError::RecoveryRequired => Failure::RecoveryRequired,
    }
}
/// Build source-column candidates only after the complete original cell set validates.
fn submit(world: &mut World, work: &Work) -> Result<native::ColumnTicket, Failure> {
    if !work.world.matches(world)
        || world
            .get_resource::<WorldIdentity>()
            .is_none_or(|identity| identity.id != work.identity)
        || work.owner.upgrade().is_none()
    {
        return Err(Failure::Access(wit::AccessError::WorldClosed));
    }
    let mut chunks = Vec::new();
    let mut seen = HashSet::new();
    for replacement in &work.cells {
        replacement
            .expected
            .current(world)
            .map_err(Failure::Block)?;
        if world
            .get::<crate::world::ecs::ChunkStateFlags>(replacement.expected.chunk)
            .is_none()
        {
            return Err(Failure::Access(wit::AccessError::Unavailable));
        }
        if seen.insert(replacement.expected.chunk) {
            chunks.push(replacement.expected.chunk);
        }
    }
    if !world.contains_resource::<Messages<crate::world::ecs::BlockBroadcastEvent>>() {
        return Err(Failure::Access(wit::AccessError::Unavailable));
    }
    let mut captured = native::capture(world, &chunks).map_err(failure)?;
    let mut changes = Vec::new();
    for replacement in &work.cells {
        let cell = &replacement.expected;
        let position = crate::world::ChunkPos::new(cell.position.x >> 4, cell.position.z >> 4);
        let column = captured
            .columns
            .iter_mut()
            .find(|column| column.position == position)
            .ok_or(Failure::Conflict)?;
        let (x, y, z) = world_to_local_coords(cell.position.x, cell.position.y, cell.position.z);
        column
            .column
            .chunk
            .set_block_layers(
                x,
                y,
                z,
                replacement.proposed.primary,
                Some(replacement.proposed.secondary),
            )
            .ok_or(Failure::Conflict)?;
        if replacement.proposed != cell.layers {
            changes.push(native::ColumnBlockChange {
                position: cell.position,
                before: cell.layers.primary,
                after: replacement.proposed.primary,
            });
        }
    }
    native::commit_scoped(
        world,
        captured,
        &work.id,
        work.digest,
        work.owner.clone(),
        changes,
    )
    .map_err(failure)
}
/// Observe disk-backed physical completion before exposing committed status.
fn completed(work: &Work) -> Option<Status> {
    match work.ticket.as_ref()?.status() {
        native::ColumnOperationStatus::Pending => None,
        native::ColumnOperationStatus::Applied(Some(record))
            if record.intent == Some(work.digest) =>
        {
            Some(match receipt(&work.key, &record) {
                Ok(receipt) => Status::Committed(receipt),
                Err(_) => Status::RecoveryRequired,
            })
        }
        native::ColumnOperationStatus::Applied(_) => Some(Status::RecoveryRequired),
        native::ColumnOperationStatus::Failed(ColumnWriteError::RecoveryRequired) => {
            Some(Status::RecoveryRequired)
        }
        native::ColumnOperationStatus::Failed(error) => Some(Status::NotApplied(failure(error))),
    }
}
/// Run outside manager locks; no guest call or borrowed WorldTx crosses this phase.
pub(crate) fn apply_pending(world: &mut World) {
    let Some(mut book) = world.remove_resource::<Book>() else {
        return;
    };
    let active: Vec<_> = book.active.keys().cloned().collect();
    for id in active {
        let work = &book.active[&id];
        if let Some(status) = completed(work) {
            let terminal = matches!(status, Status::Committed(_) | Status::NotApplied(_));
            work.state.set(status);
            if terminal {
                book.active.remove(&id);
            }
        }
    }
    for request in std::mem::take(&mut book.queued) {
        if !crate::plugin::manager::readiness::admission_allowed(world) {
            request.state().set(Status::NotApplied(Failure::Access(
                wit::AccessError::Unavailable,
            )));
            continue;
        }
        if !request.current(world) {
            request.state().set(Status::NotApplied(Failure::Access(
                wit::AccessError::Cancelled,
            )));
            continue;
        }
        match request {
            Staged::New(mut work) => match submit(world, &work) {
                Ok(ticket) => {
                    work.ticket = Some(ticket);
                    book.active.insert(work.id.clone(), work);
                }
                Err(Failure::RecoveryRequired) => work.state.set(Status::RecoveryRequired),
                Err(error) => work.state.set(Status::NotApplied(error)),
            },
            Staged::Resume {
                state,
                id,
                owner,
                count,
                ..
            } => {
                let Some(work) = book.active.get_mut(&id) else {
                    state.set(Status::RecoveryRequired);
                    continue;
                };
                match native::reconcile_scoped(world, &id, work.digest, owner.clone()) {
                    Ok(ticket) => {
                        work.owner = owner;
                        work.ticket = Some(ticket);
                        work.state = state;
                        work._count = count;
                        if let Some(status) = completed(work) {
                            work.state.set(status);
                        }
                    }
                    Err(_) => state.set(Status::RecoveryRequired),
                }
            }
        }
    }
    world.insert_resource(book);
}

impl wit::HostDurableBlockRequest for HostContext {
    fn status(
        &mut self,
        request: Resource<wit::DurableBlockRequest>,
    ) -> Result<Status, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::WriteDurableBlocks)?;
        match self.resources.entries.get(&request.rep()) {
            Some(Entry::DurableBlockRequest(state)) => Ok(state.get()),
            _ => Err(wit::AccessError::InvalidArgument),
        }
    }
    fn cancel(
        &mut self,
        request: Resource<wit::DurableBlockRequest>,
    ) -> Result<bool, wit::AccessError> {
        self.operation()?;
        self.require(PluginCapability::WriteDurableBlocks)?;
        match self.resources.entries.get(&request.rep()) {
            Some(Entry::DurableBlockRequest(state)) => Ok(state.cancel()),
            _ => Err(wit::AccessError::InvalidArgument),
        }
    }
    fn drop(&mut self, request: Resource<wit::DurableBlockRequest>) -> wasmtime::Result<()> {
        self.resources.entries.remove(&request.rep());
        Ok(())
    }
}
