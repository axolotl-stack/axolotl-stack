//! Whole-batch publication runs without a manager guard and keeps native ownership bounded.

use super::*;
use crate::world::ecs::{BlockBroadcastEvent, ChunkStateFlags};
use parking_lot::Mutex;
use std::sync::{
    Weak,
    atomic::{AtomicU32, Ordering},
};

const MAX_QUEUED_REQUESTS: u32 = 4096;
const MAX_QUEUED_CELLS: u32 = 8192;
const MAX_APPLIED_REQUESTS: usize = 128;
const MAX_APPLIED_CELLS: usize = 1024;

/// Owned status survives source changes and never claims a storage/client acknowledgement.
pub(in crate::plugin::manager) struct RequestState(Mutex<wit::BlockEditStatus>);
impl RequestState {
    /// New guest tickets describe callback staging only.
    pub(super) fn new() -> Self {
        Self(Mutex::new(wit::BlockEditStatus::Staged))
    }
    /// Observations remain available without resolving a live actor or world.
    pub(super) fn status(&self) -> wit::BlockEditStatus {
        *self.0.lock()
    }
    /// Cancel before publication; physical changes are terminal.
    pub(super) fn cancel(&self) -> bool {
        let mut state = self.0.lock();
        if matches!(
            *state,
            wit::BlockEditStatus::Staged | wit::BlockEditStatus::Pending
        ) {
            *state = wit::BlockEditStatus::Cancelled;
            true
        } else {
            false
        }
    }
    /// Never overwrite a physical result or an explicit cancellation.
    fn fail(&self, error: Error) {
        let mut state = self.0.lock();
        if matches!(
            *state,
            wit::BlockEditStatus::Staged | wit::BlockEditStatus::Pending
        ) {
            *state = wit::BlockEditStatus::Failed(error);
        }
    }
    /// Set a pending or physical result while publication owns the request.
    fn set(&self, state: wit::BlockEditStatus) {
        *self.0.lock() = state;
    }
}

/// Reservations follow owned work rather than the guest's status resource.
pub(in crate::plugin::manager) struct Count {
    counter: Arc<AtomicU32>,
    amount: u32,
}
impl Count {
    /// Acquire before exposing a new retained request or queue cell batch.
    pub(super) fn acquire(
        counter: &Arc<AtomicU32>,
        amount: u32,
        maximum: u32,
    ) -> Result<Self, wit::AccessError> {
        counter
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(amount).filter(|next| *next <= maximum)
            })
            .map_err(|_| wit::AccessError::LimitExceeded)?;
        Ok(Self {
            counter: counter.clone(),
            amount,
        })
    }
}
impl Drop for Count {
    fn drop(&mut self) {
        self.counter.fetch_sub(self.amount, Ordering::AcqRel);
    }
}

/// No world borrow or live player survives callback completion.
pub(in crate::plugin::manager) struct Request {
    state: Arc<RequestState>,
    world: WorldReference,
    cells: Vec<OwnedReplacement>,
    owner: Weak<()>,
    _component_count: Count,
    queue_counts: Option<(Count, Count)>,
}
impl Request {
    /// Keep immutable witnesses and quota leases while accepted work awaits publication.
    pub(super) fn new(
        state: Arc<RequestState>,
        world: WorldReference,
        cells: Vec<OwnedReplacement>,
        owner: Weak<()>,
        component_count: Count,
    ) -> Self {
        Self {
            state,
            world,
            cells,
            owner,
            _component_count: component_count,
            queue_counts: None,
        }
    }
    /// Reject disappeared owners and every changed or newly reserved physical cell.
    fn current(&self, world: &World) -> Result<(), Error> {
        if self.owner.upgrade().is_none() {
            return Err(Error::Access(wit::AccessError::Cancelled));
        }
        if !self.world.matches(world) {
            return Err(Error::Access(wit::AccessError::WrongWorld));
        }
        for cell in &self.cells {
            cell.expected.current(world)?;
        }
        Ok(())
    }
    /// Revalidate the complete batch before the first raw write or publication effect.
    fn apply(&self, world: &mut World) -> Result<(), Error> {
        self.current(world)?;
        if !world.contains_resource::<Messages<BlockBroadcastEvent>>()
            || self
                .cells
                .iter()
                .any(|cell| world.get::<ChunkStateFlags>(cell.expected.chunk).is_none())
        {
            return Err(Error::Access(wit::AccessError::Unavailable));
        }
        let mut changes = Vec::new();
        for replacement in &self.cells {
            let cell = &replacement.expected;
            if replacement.proposed == cell.layers {
                continue;
            }
            let (x, y, z) =
                world_to_local_coords(cell.position.x, cell.position.y, cell.position.z);
            world
                .get_mut::<ChunkData>(cell.chunk)
                .expect("validated chunk lifetime")
                .inner
                .set_block_layers(
                    x,
                    y,
                    z,
                    replacement.proposed.primary,
                    Some(replacement.proposed.secondary),
                )
                .expect("validated native height and state space");
            let mut flags = world
                .get_mut::<ChunkStateFlags>(cell.chunk)
                .expect("validated chunk flags");
            flags.mark_dirty();
            flags.mark_needs_rebroadcast();
            changes.push((
                cell.chunk,
                cell.position,
                cell.layers.primary,
                replacement.proposed.primary,
            ));
        }
        self.state
            .set(wit::BlockEditStatus::Applied(wit::BlockEditApplied {
                checked: self.cells.len() as u32,
                changed: changes.len() as u32,
            }));
        // Publish the whole installed batch before an observer can author a later state.
        for &(chunk, position, _, current) in &changes {
            world.write_message(BlockBroadcastEvent {
                chunk_entity: chunk,
                block_pos: position,
                new_block: current,
            });
            crate::server::game::blocks::broadcast_block_state(
                world, chunk, position, current, false,
            );
        }
        for (chunk, position, previous, current) in changes {
            crate::server::game::blocks::notify_block_change(
                world, chunk, position, previous, current,
            );
        }
        Ok(())
    }
}
impl Drop for Request {
    fn drop(&mut self) {
        self.state.fail(Error::Access(wit::AccessError::Cancelled));
    }
}

/// In-flight counts remain charged while callbacks triggered by a batch stage more work.
#[derive(Resource, Default)]
struct Queue {
    requests: Vec<Request>,
    request_count: Arc<AtomicU32>,
    cell_count: Arc<AtomicU32>,
}
impl Queue {
    /// Reserve both queue dimensions before accepting any part of a batch.
    fn push(&mut self, mut request: Request) -> Result<(), (Error, Request)> {
        let counts = (|| {
            let requests = Count::acquire(&self.request_count, 1, MAX_QUEUED_REQUESTS)?;
            let cells = Count::acquire(
                &self.cell_count,
                request.cells.len() as u32,
                MAX_QUEUED_CELLS,
            )?;
            Ok::<_, wit::AccessError>((requests, cells))
        })();
        match counts {
            Ok(counts) => {
                request.queue_counts = Some(counts);
                request.state.set(wit::BlockEditStatus::Pending);
                self.requests.push(request);
                Ok(())
            }
            Err(error) => Err((Error::Access(error), request)),
        }
    }
}
impl Drop for Queue {
    fn drop(&mut self) {
        for request in &self.requests {
            request
                .state
                .fail(Error::Access(wit::AccessError::WorldClosed));
        }
    }
}

/// Callback traps/malformed results discard staging; valid default-action cancellation preserves it.
pub(in crate::plugin::manager) fn finish(
    context: &mut HostContext,
    world: &mut World,
    succeeded: bool,
) {
    let requests = std::mem::take(&mut context.pending_block_edits);
    for request in requests {
        if !succeeded {
            request
                .state
                .fail(Error::Access(wit::AccessError::CallbackFailed));
            continue;
        }
        if matches!(request.state.status(), wit::BlockEditStatus::Cancelled) {
            continue;
        }
        if let Err(error) = request.current(world) {
            request.state.fail(error);
            continue;
        }
        world.init_resource::<Queue>();
        if let Err((error, request)) = world.resource_mut::<Queue>().push(request) {
            request.state.fail(error);
        }
    }
}

/// Apply bounded whole batches with no active manager guard and no guest export running.
pub(crate) fn apply_pending(world: &mut World) {
    let Some(mut queue) = world.get_resource_mut::<Queue>() else {
        return;
    };
    let requests = std::mem::take(&mut queue.requests);
    let mut pending = Vec::new();
    let mut remaining_cells = MAX_APPLIED_CELLS;
    let mut remaining_requests = MAX_APPLIED_REQUESTS;
    let mut deferred = false;
    for request in requests {
        if matches!(request.state.status(), wit::BlockEditStatus::Cancelled) {
            continue;
        }
        if !crate::plugin::manager::readiness::admission_allowed(world) {
            request
                .state
                .fail(Error::Access(wit::AccessError::Unavailable));
            continue;
        }
        if deferred || remaining_requests == 0 || request.cells.len() > remaining_cells {
            deferred = true;
            pending.push(request);
            continue;
        }
        remaining_requests -= 1;
        remaining_cells -= request.cells.len();
        if let Err(error) = request.apply(world) {
            request.state.fail(error);
        }
    }
    world.init_resource::<Queue>();
    let mut queue = world.resource_mut::<Queue>();
    pending.append(&mut queue.requests);
    queue.requests = pending;
}
