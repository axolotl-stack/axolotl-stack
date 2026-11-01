//! Native ownership of asynchronous column writes, loaded pins and publication.
//! This module is an engine mechanism; component operations are separate bindings.

#[cfg(test)]
mod tests;

use super::{
    ChunkColumn,
    column_writer::{
        ColumnCompletion, ColumnReceipt, ColumnStamp, ColumnWrite, ColumnWriteAck,
        ColumnWriteError, ColumnWriter, MAX_COLUMNS,
    },
};
use crate::{
    entity::mob::persistence::{ChunkSnapshot, mark_saved, snapshot_chunks},
    world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkStateFlags, ChunkViewers},
};
use bevy_ecs::prelude::*;
use glam::IVec3;
use std::sync::{Arc, Mutex, Weak};

/// An owned observation of physical publication, not a client acknowledgement.
#[derive(Clone, Debug)]
pub enum ColumnOperationStatus {
    Pending,
    Applied(Option<ColumnReceipt>),
    Failed(ColumnWriteError),
}
/// Dropping a ticket never cancels its accepted storage work or releases pins.
#[derive(Clone)]
pub struct ColumnTicket(Arc<Mutex<ColumnOperationStatus>>);
impl ColumnTicket {
    /// Observe the last definitive outcome or a recovery-required fence.
    pub fn status(&self) -> ColumnOperationStatus {
        self.0.lock().map_or(
            ColumnOperationStatus::Failed(ColumnWriteError::RecoveryRequired),
            |status| status.clone(),
        )
    }
}
/// Captured immutable source identities plus detached candidate columns.
pub struct ColumnCapture {
    world: bevy_ecs::world::WorldId,
    writer: Arc<ColumnWriter>,
    stamp: ColumnStamp,
    snapshots: Vec<ChunkSnapshot>,
    pub columns: Vec<ColumnWrite>,
}
struct Pending {
    world: bevy_ecs::world::WorldId,
    writer: Arc<ColumnWriter>,
    snapshots: Vec<ChunkSnapshot>,
    columns: Vec<ColumnWrite>,
    receiver: Option<ColumnCompletion>,
    mode: Mode,
    ticket: ColumnTicket,
}
/// Trusted native block notifications accompanying an already-validated durable
/// terrain candidate. These run only after its whole disk-backed image is installed.
pub(crate) struct ColumnBlockChange {
    pub position: IVec3,
    pub before: u32,
    pub after: u32,
}

/// Publication ownership is independent of guest ticket lifetime. Once IO is
/// accepted, loss of its component owner requires receipt-backed reconciliation.
#[derive(Default)]
struct Mode {
    changes: Vec<ColumnBlockChange>,
    durable: bool,
    unload: bool,
    id: Option<String>,
    intent: Option<[u8; 32]>,
    owner: Option<Weak<()>>,
    identity: Option<uuid::Uuid>,
}

/// Pending writes also pin chunks with zero viewers. Uncertain operations retain
/// these reservations until world teardown and provider restart reconciliation.
#[derive(Resource, Default)]
struct ColumnPersistence {
    pending: Vec<Pending>,
}

impl Drop for ColumnPersistence {
    fn drop(&mut self) {
        for pending in &self.pending {
            pending.writer.orphan();
            set_status(
                &pending.ticket,
                ColumnOperationStatus::Failed(ColumnWriteError::RecoveryRequired),
            );
        }
    }
}

/// Native write exclusion for a column owned by an accepted durable operation.
/// Its private construction prevents a loaded-column pin from being confused
/// with a viewer or a generated chunk.
#[derive(Component)]
pub struct ColumnWriteFence(());

/// Whether any column currently has a durable mutation owner.
pub(crate) fn any_reserved(world: &World) -> bool {
    world
        .get_resource::<ColumnPersistence>()
        .is_some_and(|state| state.pending.iter().any(|job| job.mode.durable))
}

/// Whether native terrain/container writers must defer mutation at this cell.
pub(crate) fn cell_reserved(world: &World, pos: IVec3) -> bool {
    let chunk = world
        .get_resource::<ChunkManager>()
        .and_then(|manager| manager.get_by_coords(pos.x >> 4, pos.z >> 4));
    chunk.is_some_and(|chunk| {
        if world.get::<ColumnWriteFence>(chunk).is_some() {
            return true;
        }
        world
            .get_resource::<ColumnPersistence>()
            .is_some_and(|state| {
                state.pending.iter().any(|job| {
                    job.mode.durable && job.snapshots.iter().any(|snapshot| snapshot.chunk == chunk)
                })
            })
    })
}
/// An accepted ordinary or durable write owns a real loaded-column pin.
pub(crate) fn pinned(world: &World, entity: Entity) -> bool {
    world
        .get_resource::<ColumnPersistence>()
        .is_some_and(|state| {
            state.pending.iter().any(|job| {
                job.snapshots
                    .iter()
                    .any(|snapshot| snapshot.chunk == entity)
            })
        })
}
/// Capture one bounded loaded set before producing a detached candidate. All
/// snapshots keep their original provider epoch and cannot be restamped later.
pub fn capture(world: &mut World, chunks: &[Entity]) -> Result<ColumnCapture, ColumnWriteError> {
    capture_inner(world, chunks, true)
}

/// Capture ordinary dirty terrain or actor ownership without rewriting clean,
/// actor-free stored columns. The original witness is still taken before copying.
pub(crate) fn capture_changed(
    world: &mut World,
    chunks: &[Entity],
) -> Result<Option<ColumnCapture>, ColumnWriteError> {
    let captured = capture_inner(world, chunks, false)?;
    Ok((!captured.columns.is_empty()).then_some(captured))
}

/// Share immutable stamps and loaded ownership checks across both capture paths.
fn capture_inner(
    world: &mut World,
    chunks: &[Entity],
    include_clean: bool,
) -> Result<ColumnCapture, ColumnWriteError> {
    if world
        .get_resource::<ColumnPersistence>()
        .is_some_and(|state| {
            state.pending.len() >= 16 || state.pending.iter().any(|pending| pending.mode.durable)
        })
    {
        return Err(ColumnWriteError::Busy);
    }
    if chunks.is_empty() || chunks.len() > MAX_COLUMNS {
        return Err(ColumnWriteError::Invalid);
    }
    let manager = world
        .get_resource::<ChunkManager>()
        .ok_or(ColumnWriteError::Closed)?;
    let provider = manager.provider().ok_or(ColumnWriteError::Unsupported)?;
    let writer = provider
        .column_writer()
        .ok_or(ColumnWriteError::Unsupported)?;
    let dimension = manager.dimension();
    let stamp = writer.capture()?;
    let mut unique = std::collections::HashSet::new();
    for &chunk in chunks {
        let pos = world
            .get::<ChunkPosition>(chunk)
            .ok_or(ColumnWriteError::Stale)?;
        if !unique.insert(chunk)
            || manager.get_by_coords(pos.x, pos.z) != Some(chunk)
            || world.get::<ChunkData>(chunk).is_none()
            || pinned(world, chunk)
        {
            return Err(ColumnWriteError::Busy);
        }
    }
    let mut snapshots = snapshot_chunks(world, chunks, false);
    // Explicit sets also capture currently clean columns; saving an empty set is
    // never an implicit deletion or generation request.
    for snapshot in &mut snapshots {
        if include_clean && snapshot.column.is_none() {
            let terrain = &world
                .get::<ChunkData>(snapshot.chunk)
                .ok_or(ColumnWriteError::Stale)?
                .inner;
            snapshot.terrain_witness = Some(terrain.mutation_witness());
            snapshot.column = Some(ChunkColumn::new(terrain.clone()));
        }
    }
    snapshots.retain(|snapshot| snapshot.column.is_some());
    let columns = snapshots
        .iter()
        .map(|snapshot| ColumnWrite {
            position: snapshot.pos,
            dimension,
            column: snapshot.column.as_ref().expect("captured column").clone(),
        })
        .collect();
    Ok(ColumnCapture {
        world: world.id(),
        writer,
        stamp,
        snapshots,
        columns,
    })
}
/// Reserve bounded native ownership only at actual acceptance, including
/// captures retained before other requests acquired their pins.
fn admission_capacity(world: &World, capture: &ColumnCapture) -> Result<(), ColumnWriteError> {
    if world
        .get_resource::<ColumnPersistence>()
        .is_some_and(|state| {
            state.pending.len() >= 16 || state.pending.iter().any(|pending| pending.mode.durable)
        })
        || capture
            .snapshots
            .iter()
            .any(|snapshot| pinned(world, snapshot.chunk))
    {
        return Err(ColumnWriteError::Busy);
    }
    Ok(())
}

/// Check unchanged live ownership immediately before accepting a captured set.
fn valid_capture(world: &World, capture: &ColumnCapture) -> bool {
    let Some(manager) = world.get_resource::<ChunkManager>() else {
        return false;
    };
    if capture.world != world.id()
        || manager
            .provider()
            .and_then(|provider| provider.column_writer())
            .is_none_or(|writer| !Arc::ptr_eq(&writer, &capture.writer))
    {
        return false;
    }
    capture.snapshots.iter().all(|snapshot| {
        manager.get_by_coords(snapshot.pos.x, snapshot.pos.z) == Some(snapshot.chunk)
            && world
                .get::<ChunkData>(snapshot.chunk)
                .map(|data| data.inner.mutation_witness())
                == snapshot.terrain_witness
            && world
                .get::<ChunkStateFlags>(snapshot.chunk)
                .is_none_or(|flags| flags.dirty_generation() == snapshot.dirty_generation)
    })
}
/// Submit an ordinary save/unload. Completion clears only the exact captured
/// dirty generation; failed writes leave terrain and actors loaded for retry.
pub fn save(
    world: &mut World,
    capture: ColumnCapture,
    unload: bool,
) -> Result<ColumnTicket, ColumnWriteError> {
    admission_capacity(world, &capture)?;
    if !valid_capture(world, &capture) {
        return Err(ColumnWriteError::Stale);
    }
    if capture.columns.len() != capture.snapshots.len()
        || capture
            .columns
            .iter()
            .zip(&capture.snapshots)
            .any(|(candidate, source)| {
                source.column.as_ref().is_none_or(|original| {
                    candidate.position != source.pos
                        || candidate.dimension != world.resource::<ChunkManager>().dimension()
                        || candidate.column.chunk.mutation_witness()
                            != original.chunk.mutation_witness()
                        || candidate.column.actors != original.actors
                        || candidate.column.removed_actors != original.removed_actors
                })
            })
    {
        return Err(ColumnWriteError::Invalid);
    }
    let receiver = capture.writer.ordinary(capture.stamp, &capture.columns)?;
    Ok(track(
        world,
        capture,
        receiver,
        Mode {
            unload,
            ..Mode::default()
        },
    ))
}
/// Submit a durable set after acquiring its provider-wide publication fence.
/// Candidate actor ownership is captured from live state and cannot be replaced
/// through this terrain writer; later actor mutations continue in live ECS.
pub fn commit(
    world: &mut World,
    capture: ColumnCapture,
    id: &str,
) -> Result<ColumnTicket, ColumnWriteError> {
    commit_inner(world, capture, id, None, None, Vec::new())
}

/// Bind accepted IO to the validated component owner and a stable typed intent.
pub(crate) fn commit_scoped(
    world: &mut World,
    capture: ColumnCapture,
    id: &str,
    intent: [u8; 32],
    owner: Weak<()>,
    changes: Vec<ColumnBlockChange>,
) -> Result<ColumnTicket, ColumnWriteError> {
    commit_inner(world, capture, id, Some(intent), Some(owner), changes)
}

/// Preserve existing native transactions while scoped callers add ownership guards.
fn commit_inner(
    world: &mut World,
    capture: ColumnCapture,
    id: &str,
    intent: Option<[u8; 32]>,
    owner: Option<Weak<()>>,
    changes: Vec<ColumnBlockChange>,
) -> Result<ColumnTicket, ColumnWriteError> {
    admission_capacity(world, &capture)?;
    if !valid_capture(world, &capture) || capture.columns.len() != capture.snapshots.len() {
        return Err(ColumnWriteError::Stale);
    }
    for (candidate, source) in capture.columns.iter().zip(&capture.snapshots) {
        let original = source.column.as_ref().ok_or(ColumnWriteError::Stale)?;
        if candidate.position != source.pos
            || candidate.column.chunk.block_bounds() != original.chunk.block_bounds()
            || candidate.dimension != world.resource::<ChunkManager>().dimension()
            || candidate.column.chunk.block_entities() != original.chunk.block_entities()
            || candidate.column.actors != original.actors
            || candidate.column.removed_actors != original.removed_actors
        {
            return Err(ColumnWriteError::Invalid);
        }
    }
    if !changes.is_empty() {
        if !world.contains_resource::<Messages<crate::world::ecs::BlockBroadcastEvent>>() {
            return Err(ColumnWriteError::Invalid);
        }
        let mut seen = std::collections::HashSet::new();
        for change in &changes {
            if !seen.insert(change.position) {
                return Err(ColumnWriteError::Invalid);
            }
            let coordinates =
                crate::world::ChunkPos::new(change.position.x >> 4, change.position.z >> 4);
            let Some((candidate, source)) = capture
                .columns
                .iter()
                .zip(&capture.snapshots)
                .find(|(column, _)| column.position == coordinates)
            else {
                return Err(ColumnWriteError::Invalid);
            };
            let original = source.column.as_ref().ok_or(ColumnWriteError::Invalid)?;
            let (x, y, z) = crate::world::ecs::world_to_local_coords(
                change.position.x,
                change.position.y,
                change.position.z,
            );
            if original.chunk.get_block(x, y, z) != change.before
                || candidate.column.chunk.get_block(x, y, z) != change.after
            {
                return Err(ColumnWriteError::Invalid);
            }
        }
    }
    capture.writer.reserve()?;
    let queued = match intent {
        Some(intent) => capture
            .writer
            .durable_intent(capture.stamp, &capture.columns, id, intent),
        None => capture.writer.durable(capture.stamp, &capture.columns, id),
    };
    let identity = owner.as_ref().and_then(|_| {
        world
            .get_resource::<crate::world::WorldIdentity>()
            .map(|identity| identity.id)
    });
    match queued {
        Ok(receiver) => Ok(track(
            world,
            capture,
            receiver,
            Mode {
                durable: true,
                id: Some(id.to_owned()),
                intent,
                identity,
                owner,
                changes,
                ..Mode::default()
            },
        )),
        Err(error) => {
            capture.writer.release();
            Err(error)
        }
    }
}
/// Transfer pins and completion ownership into the world before returning a ticket.
fn track(
    world: &mut World,
    capture: ColumnCapture,
    receiver: ColumnCompletion,
    mode: Mode,
) -> ColumnTicket {
    let ticket = ColumnTicket(Arc::new(Mutex::new(ColumnOperationStatus::Pending)));
    if mode.durable {
        for snapshot in &capture.snapshots {
            world
                .entity_mut(snapshot.chunk)
                .insert(ColumnWriteFence(()));
        }
    }
    world.init_resource::<ColumnPersistence>();
    world
        .resource_mut::<ColumnPersistence>()
        .pending
        .push(Pending {
            world: capture.world,
            writer: capture.writer,
            snapshots: capture.snapshots,
            columns: capture.columns,
            receiver: Some(receiver),
            mode,
            ticket: ticket.clone(),
        });
    ticket
}
/// Await already accepted work during autosave/shutdown, then publish its
/// acknowledgements before capturing a newer ordinary snapshot. Normal ticks
/// use `poll` and never wait for storage futures.
pub async fn settle(world: &mut World) -> Result<(), ColumnWriteError> {
    loop {
        poll(world);
        let Some(state) = world.get_resource::<ColumnPersistence>() else {
            return Ok(());
        };
        if state.pending.is_empty() {
            return Ok(());
        }
        if state
            .pending
            .iter()
            .any(|pending| pending.receiver.is_none())
        {
            return Err(ColumnWriteError::RecoveryRequired);
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
}

/// Poll completions without waiting on IO. Native publication runs without a
/// component-manager lock and revalidates original chunk lifetimes before apply.
pub(crate) fn poll(world: &mut World) {
    let Some(mut state) = world.remove_resource::<ColumnPersistence>() else {
        return;
    };
    let mut keep = Vec::new();
    for mut pending in state.pending.drain(..) {
        let Some(receiver) = &mut pending.receiver else {
            keep.push(pending);
            continue;
        };
        let outcome = match receiver.try_recv() {
            Ok(result) => result,
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                keep.push(pending);
                continue;
            }
            Err(_) => Err(ColumnWriteError::RecoveryRequired),
        };
        match outcome {
            Ok(ack) => {
                if publish(world, &pending, &ack) {
                    if pending.mode.durable {
                        clear_fences(world, &pending);
                        pending.writer.release();
                    }
                    set_status(&pending.ticket, ColumnOperationStatus::Applied(ack.receipt));
                } else {
                    set_status(
                        &pending.ticket,
                        ColumnOperationStatus::Failed(ColumnWriteError::RecoveryRequired),
                    );
                    pending.receiver = None;
                    keep.push(pending);
                }
            }
            Err(error) => {
                set_status(&pending.ticket, ColumnOperationStatus::Failed(error));
                if error == ColumnWriteError::RecoveryRequired {
                    pending.receiver = None;
                    keep.push(pending);
                } else if pending.mode.durable {
                    clear_fences(world, &pending);
                    pending.writer.release();
                }
            }
        }
    }
    state.pending = keep;
    world.insert_resource(state);
}
/// Publish a matching disk ACK, preserving new terrain and active actor state.
fn publish(world: &mut World, pending: &Pending, ack: &ColumnWriteAck) -> bool {
    if pending.mode.identity.is_some_and(|expected| {
        world
            .get_resource::<crate::world::WorldIdentity>()
            .is_none_or(|identity| identity.id != expected)
    }) {
        return false;
    }
    if pending
        .mode
        .owner
        .as_ref()
        .is_some_and(|owner| owner.upgrade().is_none())
    {
        return false;
    }
    if pending.world != world.id()
        || world
            .get_resource::<ChunkManager>()
            .and_then(|manager| manager.provider())
            .and_then(|provider| provider.column_writer())
            .is_none_or(|writer| !Arc::ptr_eq(&writer, &pending.writer))
    {
        return false;
    }
    if pending.snapshots.iter().any(|snapshot| {
        world
            .get_resource::<ChunkManager>()
            .and_then(|manager| manager.get_by_coords(snapshot.pos.x, snapshot.pos.z))
            != Some(snapshot.chunk)
    }) {
        return false;
    }
    // A replay receipt is storage evidence only. Never replay a historical
    // candidate over an already-running native world.
    if pending.mode.durable
        && (ack.replay
            || pending.snapshots.iter().any(|snapshot| {
                world
                    .get::<ChunkData>(snapshot.chunk)
                    .map(|data| data.inner.mutation_witness())
                    != snapshot.terrain_witness
            }))
    {
        return false;
    }
    for (snapshot, candidate) in pending.snapshots.iter().zip(&pending.columns) {
        if pending.mode.durable {
            mark_saved(world, snapshot);
            let Some(mut data) = world.get_mut::<ChunkData>(snapshot.chunk) else {
                return false;
            };
            data.inner = candidate.column.chunk.clone();
            if let Some(mut flags) = world.get_mut::<ChunkStateFlags>(snapshot.chunk) {
                flags.mark_needs_rebroadcast();
            }
        } else {
            mark_saved(world, snapshot);
        }
    }
    // Every terrain buffer is installed before native observers or packets
    // can expose a partial batch. Mutation fences remain held through this phase.
    for change in &pending.mode.changes {
        let Some(chunk) = world
            .resource::<ChunkManager>()
            .get_by_coords(change.position.x >> 4, change.position.z >> 4)
        else {
            return false;
        };
        world.write_message(crate::world::ecs::BlockBroadcastEvent {
            chunk_entity: chunk,
            block_pos: change.position,
            new_block: change.after,
        });
        crate::server::game::blocks::broadcast_block_state(
            world,
            chunk,
            change.position,
            change.after,
            false,
        );
    }
    for change in &pending.mode.changes {
        let Some(chunk) = world
            .resource::<ChunkManager>()
            .get_by_coords(change.position.x >> 4, change.position.z >> 4)
        else {
            return false;
        };
        crate::server::game::blocks::notify_block_change(
            world,
            chunk,
            change.position,
            change.before,
            change.after,
        );
    }
    if pending.mode.unload {
        for snapshot in &pending.snapshots {
            let still_clean = world
                .get::<ChunkStateFlags>(snapshot.chunk)
                .is_none_or(|flags| !flags.is_dirty());
            let no_viewers = world
                .get::<ChunkViewers>(snapshot.chunk)
                .is_none_or(ChunkViewers::is_empty)
                && world.get_resource::<ChunkManager>().is_none_or(|manager| {
                    manager
                        .pending_viewers
                        .get(&(snapshot.pos.x, snapshot.pos.z))
                        .is_none_or(Vec::is_empty)
                });
            // Mobs remain active while IO is pending. A newer actor snapshot
            // prevents unload just like a newer terrain generation.
            let current = crate::entity::mob::persistence::snapshot_at_tick(
                world,
                &[snapshot.chunk],
                false,
                snapshot.captured_tick,
            );
            let same_actors =
                current
                    .first()
                    .and_then(|s| s.column.as_ref())
                    .is_none_or(|column| {
                        same_actor_records(
                            &column.actors,
                            &snapshot.column.as_ref().expect("column").actors,
                        )
                    });
            if still_clean && no_viewers && same_actors {
                for &mob in &snapshot.mobs {
                    world.despawn(mob);
                }
                world
                    .resource_mut::<ChunkManager>()
                    .remove_by_coords(snapshot.pos.x, snapshot.pos.z);
                world.despawn(snapshot.chunk);
            }
        }
    }
    true
}
/// Compound field ordering and ECS iteration order do not change actor state.
/// Values and absolute timer state still have to match before an unload.
fn same_actor_records(current: &[super::StoredActor], saved: &[super::StoredActor]) -> bool {
    use zuri_nbt::{NBTTag, encoding::LittleEndian};
    if current.len() != saved.len() {
        return false;
    }
    let decode = |bytes: &[u8]| {
        let mut input = bytes;
        let value = NBTTag::read(&mut input, LittleEndian).ok()?;
        input.is_empty().then_some(value)
    };
    let saved: std::collections::HashMap<_, _> =
        saved.iter().map(|actor| (actor.unique_id, actor)).collect();
    if saved.len() != current.len() {
        return false;
    }
    current.iter().all(|actor| {
        saved.get(&actor.unique_id).is_some_and(|other| {
            actor.nbt == other.nbt
                || decode(&actor.nbt)
                    .zip(decode(&other.nbt))
                    .is_some_and(|(current, saved)| current == saved)
        })
    })
}

/// Recover an owner-lost acknowledgement using its actual stored receipt and
/// original detached payload. No moving-actor recapture or guessed success is allowed.
pub(crate) fn reconcile_scoped(
    world: &mut World,
    id: &str,
    intent: [u8; 32],
    owner: Weak<()>,
) -> Result<ColumnTicket, ColumnWriteError> {
    if owner.upgrade().is_none() {
        return Err(ColumnWriteError::Closed);
    }
    let mut state = world
        .remove_resource::<ColumnPersistence>()
        .ok_or(ColumnWriteError::RecoveryRequired)?;
    let result = (|| {
        let index = state
            .pending
            .iter()
            .position(|pending| pending.mode.id.as_deref() == Some(id))
            .ok_or(ColumnWriteError::RecoveryRequired)?;
        let pending = &mut state.pending[index];
        if pending.mode.intent != Some(intent) {
            return Err(ColumnWriteError::Conflict);
        }
        if pending.receiver.is_some() {
            return Err(ColumnWriteError::Busy);
        }
        let receipt = pending
            .writer
            .peek_receipt(id)?
            .ok_or(ColumnWriteError::RecoveryRequired)?;
        if receipt.intent != Some(intent)
            || receipt.outcome != super::column_writer::ReceiptOutcome::Applied
            || receipt.hash != ColumnWriter::payload_hash(&pending.columns)?
        {
            return Err(ColumnWriteError::RecoveryRequired);
        }
        pending.mode.owner = Some(owner);
        let ack = ColumnWriteAck {
            epoch: receipt.epoch,
            receipt: Some(receipt.clone()),
            replay: false,
        };
        if !publish(world, pending, &ack) {
            return Err(ColumnWriteError::RecoveryRequired);
        }
        clear_fences(world, pending);
        pending.writer.release();
        set_status(
            &pending.ticket,
            ColumnOperationStatus::Applied(Some(receipt)),
        );
        let pending = state.pending.remove(index);
        Ok(pending.ticket)
    })();
    world.insert_resource(state);
    result
}

/// Remove only the original source lifetimes' native mutation exclusions.
fn clear_fences(world: &mut World, pending: &Pending) {
    for snapshot in &pending.snapshots {
        if let Ok(mut entity) = world.get_entity_mut(snapshot.chunk) {
            entity.remove::<ColumnWriteFence>();
        }
    }
}

/// Tickets may disappear while work remains owned; status updates are best effort.
fn set_status(ticket: &ColumnTicket, status: ColumnOperationStatus) {
    if let Ok(mut current) = ticket.0.lock() {
        *current = status;
    }
}
