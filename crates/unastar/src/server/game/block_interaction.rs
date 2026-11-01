//! Authorization and committed notifications for native player block operations.
//!
//! Register native handlers in [`BlockInteractionHooks`] or observe
//! [`BlockInteraction`] with Bevy. Handlers run in registration order from a
//! snapshot, followed by observers. No registry borrow is held during callbacks.
//! Registry changes affect the next dispatch, and cancellation cannot be undone.
//! Callbacks may access the world; the operation revalidates its inputs afterward.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::IVec3;

use super::player_events::ConnectionIdentity;
use crate::entity::components::{HeldSlot, MainInventory, PlayerSession, PlayerUuid};
use crate::item::ItemStack;
use crate::world::ecs::{ChunkData, ChunkManager, world_to_chunk_coords};

/// The kind of proposed operation, independent of its packet representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockInteractionKind {
    Break,
    Place,
    Use,
}

/// An actual input face, retained only when the native operation has that fact.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockClick {
    pub position: IVec3,
    pub face: Option<u8>,
    pub offset: Option<glam::DVec3>,
}

/// One complete planned cell. Unchanged reciprocal actors remain explicit.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlannedBlockCell {
    pub position: IVec3,
    pub before: BlockStateSnapshot,
    pub proposed_runtime_id: u32,
    actor_before: Option<crate::world::block_entity::BlockEntity>,
}

/// Scoped component dispatch can run inline without reacquiring its own manager.
pub(crate) trait ScopedBlockDispatcher {
    fn proposed(&mut self, world: &mut World, event: &mut BlockInteraction);
    fn committed(&mut self, world: &mut World, event: &PlayerBlockCommitted);
}

#[derive(Resource, Default)]
struct OperationSequence(u64);

/// A block operation whose mutations, item costs and normal effects have not run.
///
/// Positions include every block affected by the operation, including upper
/// halves and adjacent targets. Use operations also include the clicked block.
/// Player, kind and positions are immutable; extensions authorize or cancel the
/// actual operation rather than redirecting it to unvalidated targets.
#[derive(Event, Clone, Debug)]
pub struct BlockInteraction {
    player: Entity,
    kind: BlockInteractionKind,
    positions: Vec<IVec3>,
    cancelled: bool,
    operation_id: u64,
    cells: Vec<PlannedBlockCell>,
    click: Option<BlockClick>,
    held: Option<HeldItemSnapshot>,
    inline: bool,
}

impl BlockInteraction {
    /// Correlate proposal and completion only within this actual world lifetime.
    pub(crate) fn operation_id(&self) -> u64 {
        self.operation_id
    }
    /// Read every captured cell and its exact planned block state.
    pub(crate) fn cells(&self) -> &[PlannedBlockCell] {
        &self.cells
    }
    /// Read native click facts; unavailable packet fields remain absent.
    pub(crate) fn click(&self) -> Option<&BlockClick> {
        self.click.as_ref()
    }
    /// Read the owned original selected item, including an empty hand.
    pub(crate) fn held(&self) -> Option<&HeldItemSnapshot> {
        self.held.as_ref()
    }
    /// Only the internal managed commit path supplies its own scoped dispatcher.
    pub(crate) fn inline(&self) -> bool {
        self.inline
    }
    /// Identify the player attempting this operation.
    pub fn player(&self) -> Entity {
        self.player
    }

    /// Read the semantic operation being authorized.
    pub fn kind(&self) -> BlockInteractionKind {
        self.kind
    }

    /// Read the exact affected block coordinates in this world.
    pub fn positions(&self) -> &[IVec3] {
        &self.positions
    }

    /// Prevent the operation and its normal mutations and item costs.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// Whether an earlier handler or observer has vetoed this operation.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// An owned loaded-cell snapshot, using server runtime IDs and exact rooted
/// Bedrock NetworkLittleEndian block-actor NBT. An absent or not-yet-converted
/// Java block actor has no Bedrock bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockStateSnapshot {
    pub runtime_id: u32,
    pub block_actor: Option<Vec<u8>>,
}

/// Before and after facts for one affected position, including unchanged cells
/// involved in a use or reciprocal container operation. `after` is absent only
/// when a synchronous callback unloaded the cell before the operation finished.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerBlockChange {
    pub position: IVec3,
    pub before: BlockStateSnapshot,
    pub after: Option<BlockStateSnapshot>,
}

/// The source hotbar cell and its owned item, including an empty hand. The
/// post-operation snapshot uses this same cell even if a callback changed the
/// player's selection.
#[derive(Clone, Debug, PartialEq)]
pub struct HeldItemSnapshot {
    pub slot: u8,
    pub stack: ItemStack,
}

/// A completed native player operation, after block mutations, ordinary item
/// costs, durability, drops and normal effects. This is a read-only notification:
/// it cannot cancel an already accepted operation. Rejected or merely handled
/// clicks do not emit it. Identity and source items remain owned even when a
/// callback despawned the player or consumed the final item.
#[derive(Event, Clone, Debug)]
pub struct PlayerBlockCommitted {
    player: Entity,
    identity: ConnectionIdentity,
    kind: BlockInteractionKind,
    changes: Vec<PlayerBlockChange>,
    held_before: Option<HeldItemSnapshot>,
    held_after: Option<HeldItemSnapshot>,
    operation_id: u64,
    actor_uuid: Option<uuid::Uuid>,
    click: Option<BlockClick>,
    inline: bool,
}

impl PlayerBlockCommitted {
    /// Correlate with the exact accepted native proposal in this world lifetime.
    pub(crate) fn operation_id(&self) -> u64 {
        self.operation_id
    }
    /// Read the original authoritative UUID even if its actor was removed later.
    pub(crate) fn actor_uuid(&self) -> Option<uuid::Uuid> {
        self.actor_uuid
    }
    /// Read retained input facts without inventing unavailable click fields.
    pub(crate) fn click(&self) -> Option<&BlockClick> {
        self.click.as_ref()
    }
    /// Internal managed commits already notify their borrowed scoped dispatcher.
    pub(crate) fn inline(&self) -> bool {
        self.inline
    }
    /// Read the originating entity, which may have been despawned by a callback.
    pub fn player(&self) -> Entity {
        self.player
    }
    /// Read the immutable original account and transport identity.
    pub fn identity(&self) -> &ConnectionIdentity {
        &self.identity
    }
    /// Read the semantic operation that completed.
    pub fn kind(&self) -> BlockInteractionKind {
        self.kind
    }
    /// Read the owned facts for the exact affected cells.
    pub fn changes(&self) -> &[PlayerBlockChange] {
        &self.changes
    }
    /// Read the selected cell before costs, including the last consumed item.
    pub fn held_before(&self) -> Option<&HeldItemSnapshot> {
        self.held_before.as_ref()
    }
    /// Read the source cell afterward; absent if its original account or inventory
    /// is no longer present, or it was unavailable at the start.
    pub fn held_after(&self) -> Option<&HeldItemSnapshot> {
        self.held_after.as_ref()
    }
}

/// Retain pre-mutation facts without borrowing the world across engine effects.
/// Sessionless test or trusted actors still execute normally but cannot emit an
/// authenticated player notification.
pub(super) struct BlockCommit {
    world: bevy_ecs::world::WorldId,
    dimension: Option<i32>,
    player: Entity,
    identity: Option<ConnectionIdentity>,
    uuid: Option<uuid::Uuid>,
    kind: BlockInteractionKind,
    before: Vec<(IVec3, BlockStateSnapshot)>,
    held: Option<HeldItemSnapshot>,
    operation_id: u64,
    click: Option<BlockClick>,
    inline: bool,
}

impl BlockCommit {
    /// Capture only after authorization and final input revalidation.
    pub(super) fn capture(
        world: &World,
        player: Entity,
        kind: BlockInteractionKind,
        positions: impl IntoIterator<Item = IVec3>,
    ) -> Self {
        let mut before = Vec::new();
        for position in positions {
            if !before.iter().any(|(pos, _)| *pos == position)
                && let Some(state) = block_state(world, position)
            {
                before.push((position, state));
            }
        }
        Self {
            world: world.id(),
            dimension: world
                .get_resource::<ChunkManager>()
                .map(|manager| manager.dimension()),
            player,
            identity: ConnectionIdentity::capture(world, player),
            uuid: world.get::<PlayerUuid>(player).map(|uuid| uuid.0),
            kind,
            before,
            held: held_snapshot(world, player),
            operation_id: 0,
            click: None,
            inline: false,
        }
    }

    /// Prevent post-mutation item effects from charging a replacement account.
    pub(super) fn same_actor(&self, world: &World) -> bool {
        world.id() == self.world
            && world
                .get_resource::<ChunkManager>()
                .map(|manager| manager.dimension())
                == self.dimension
            && world.get_entity(self.player).is_ok()
            && match &self.identity {
                Some(identity) => identity.matches(world, self.player),
                None => {
                    world.get::<PlayerSession>(self.player).is_none()
                        && world.get::<PlayerUuid>(self.player).map(|uuid| uuid.0) == self.uuid
                }
            }
    }

    /// Require the original source item before an ordinary post-mutation cost.
    pub(super) fn source_item_unchanged(&self, world: &World) -> bool {
        self.same_actor(world)
            && self.held.as_ref().is_some_and(|held| {
                world
                    .get::<MainInventory>(self.player)
                    .and_then(|inv| inv.0.item(usize::from(held.slot)))
                    == Some(&held.stack)
            })
    }

    /// Read the owned source hotbar snapshot for ordinary engine costs.
    pub(super) fn held(&self) -> Option<&HeldItemSnapshot> {
        self.held.as_ref()
    }

    /// Apply durability to the original item rather than a new selection.
    pub(super) fn wear_source(&self, world: &mut World, points: i32) {
        if self.source_item_unchanged(world)
            && let Some(held) = &self.held
        {
            super::durability::wear_slot(
                world,
                self.player,
                usize::from(held.slot),
                &held.stack,
                points,
            );
        }
    }

    /// Replace the original source cell without overwriting callback changes.
    pub(super) fn replace_source(&self, world: &mut World, stack: ItemStack) -> bool {
        if !self.source_item_unchanged(world) {
            return false;
        }
        let Some(held) = &self.held else {
            return false;
        };
        super::item_use::set_slot(world, self.player, held.slot, stack);
        true
    }

    /// Dispatch after all ordinary effects; registry mutations affect the next
    /// dispatch, and no registry borrow remains while callbacks run.
    pub(super) fn finish(self, world: &mut World) {
        self.finish_with(world, None);
    }

    /// Deliver an actual completion through the caller's existing manager borrow when required.
    pub(super) fn finish_with(
        self,
        world: &mut World,
        inline: Option<&mut dyn ScopedBlockDispatcher>,
    ) {
        let held_after = self
            .held
            .as_ref()
            .filter(|_| self.same_actor(world))
            .and_then(|held| {
                Some(HeldItemSnapshot {
                    slot: held.slot,
                    stack: world
                        .get::<MainInventory>(self.player)?
                        .0
                        .item(usize::from(held.slot))?
                        .clone(),
                })
            });
        let Some(identity) = self.identity else {
            return;
        };
        let mut event = PlayerBlockCommitted {
            player: self.player,
            identity,
            kind: self.kind,
            changes: self
                .before
                .into_iter()
                .map(|(position, before)| PlayerBlockChange {
                    position,
                    before,
                    after: block_state(world, position),
                })
                .collect(),
            held_before: self.held,
            held_after,
            operation_id: self.operation_id,
            actor_uuid: self.uuid,
            click: self.click,
            inline: self.inline,
        };
        let handlers = world
            .get_resource::<BlockInteractionHooks>()
            .map_or_else(Vec::new, |hooks| hooks.handlers.clone());
        for handler in handlers {
            handler.on_committed(world, &event);
        }
        if let Some(inline) = inline {
            inline.committed(world, &event);
        }
        world.trigger_ref(&mut event);
    }
}

/// Snapshot the original selection without discarding an empty hand.
fn held_snapshot(world: &World, player: Entity) -> Option<HeldItemSnapshot> {
    let slot = world.get::<HeldSlot>(player)?.0;
    Some(HeldItemSnapshot {
        slot,
        stack: world
            .get::<MainInventory>(player)?
            .0
            .item(usize::from(slot))?
            .clone(),
    })
}

/// Read authoritative loaded block data without decoding, repairing or loading
/// its actor and without triggering loot generation.
pub(super) fn block_state(world: &World, position: IVec3) -> Option<BlockStateSnapshot> {
    let runtime_id = super::blocks::block_at(world, (position.x, position.y, position.z))?;
    let (cx, cz) = world_to_chunk_coords(position.x, position.z);
    let chunk = world
        .get_resource::<ChunkManager>()?
        .get_by_coords(cx, cz)?;
    let data = &world.get::<ChunkData>(chunk)?.inner;
    let block_actor = data
        .block_entities()
        .iter()
        .find(|actor| actor.position() == position.to_array())
        .and_then(|actor| actor.bedrock_nbt())
        .map(|nbt| nbt.0.to_vec());
    Some(BlockStateSnapshot {
        runtime_id,
        block_actor,
    })
}

/// Compare complete actors, including source recipes not yet converted to network NBT.
fn block_actor(world: &World, position: IVec3) -> Option<crate::world::block_entity::BlockEntity> {
    let (cx, cz) = world_to_chunk_coords(position.x, position.z);
    let chunk = world
        .get_resource::<ChunkManager>()?
        .get_by_coords(cx, cz)?;
    world
        .get::<ChunkData>(chunk)?
        .inner
        .block_entities()
        .iter()
        .find(|actor| actor.position() == position.to_array())
        .cloned()
}

/// Native synchronous callbacks with no engine lock or borrowed registry held.
///
/// Callbacks may cause nested operations. Handlers with internal locks must allow
/// reentrant calls or avoid initiating nested operations while holding a lock.
pub trait BlockInteractionHandler: Send + Sync + 'static {
    /// Inspect or cancel one proposed block operation before any ordinary effects.
    fn on_interaction(&self, _world: &mut World, _event: &mut BlockInteraction) {}
    /// Observe an accepted operation after its normal mutations and item effects.
    fn on_committed(&self, _world: &mut World, _event: &PlayerBlockCommitted) {}
}

/// Ordered native block authorization callbacks for an embedding application.
#[derive(Resource, Default, Clone)]
pub struct BlockInteractionHooks {
    handlers: Vec<Arc<dyn BlockInteractionHandler>>,
}

impl BlockInteractionHooks {
    /// Add a handler to subsequent dispatch snapshots.
    pub fn register(&mut self, handler: impl BlockInteractionHandler) {
        self.handlers.push(Arc::new(handler));
    }
}

/// Dispatch a decision without holding the callback registry resource borrowed.
#[cfg(test)]
pub(crate) fn authorize(
    world: &mut World,
    player: Entity,
    kind: BlockInteractionKind,
    positions: Vec<IVec3>,
) -> bool {
    let updates: Vec<_> = positions
        .iter()
        .filter_map(|position| Some((*position, block_state(world, *position)?.runtime_id)))
        .collect();
    if updates.len() != positions.len() {
        return false;
    }
    plan(world, player, kind, &updates, None, None).is_some()
}

/// Capture the entire plan and revalidate its full before-state after every authorization callback.
pub(super) fn plan(
    world: &mut World,
    player: Entity,
    kind: BlockInteractionKind,
    updates: &[(IVec3, u32)],
    click: Option<BlockClick>,
    mut inline: Option<&mut dyn ScopedBlockDispatcher>,
) -> Option<BlockCommit> {
    if updates.len() > 512 {
        return None;
    }
    let mut cells = Vec::new();
    for &(position, proposed_runtime_id) in updates {
        if let Some(previous) = cells
            .iter()
            .find(|cell: &&PlannedBlockCell| cell.position == position)
        {
            if previous.proposed_runtime_id != proposed_runtime_id {
                return None;
            }
            continue;
        }
        cells.push(PlannedBlockCell {
            position,
            before: block_state(world, position)?,
            proposed_runtime_id,
            actor_before: block_actor(world, position),
        });
    }
    let operation_id = {
        let mut sequence = world.get_resource_or_init::<OperationSequence>();
        sequence.0 = sequence.0.checked_add(1)?;
        sequence.0
    };
    let mut committed =
        BlockCommit::capture(world, player, kind, cells.iter().map(|cell| cell.position));
    committed.operation_id = operation_id;
    committed.click = click.clone();
    committed.inline = inline.is_some();
    let handlers = world
        .get_resource::<BlockInteractionHooks>()
        .map_or_else(Vec::new, |hooks| hooks.handlers.clone());
    let mut event = BlockInteraction {
        player,
        kind,
        positions: cells.iter().map(|cell| cell.position).collect(),
        cancelled: false,
        operation_id,
        cells,
        click,
        held: committed.held.clone(),
        inline: inline.is_some(),
    };
    for handler in handlers {
        handler.on_interaction(world, &mut event);
    }
    if !event.is_cancelled()
        && let Some(inline) = inline.as_mut()
    {
        inline.proposed(world, &mut event);
    }
    world.trigger_ref(&mut event);
    (!event.is_cancelled()
        && committed.same_actor(world)
        && held_snapshot(world, player) == committed.held
        && event.cells.iter().all(|cell| {
            block_state(world, cell.position).as_ref() == Some(&cell.before)
                && block_actor(world, cell.position) == cell.actor_before
        }))
    .then_some(committed)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Record callback order while trying to add another handler during dispatch.
    struct RegisterDuringDispatch(Arc<Mutex<Vec<u8>>>);

    impl BlockInteractionHandler for RegisterDuringDispatch {
        fn on_interaction(&self, world: &mut World, event: &mut BlockInteraction) {
            self.0.lock().unwrap().push(1);
            world
                .resource_mut::<BlockInteractionHooks>()
                .register(Observe(self.0.clone()));
            event.cancel();
        }
    }

    /// Record a later callback and verify cancellation remains visible.
    struct Observe(Arc<Mutex<Vec<u8>>>);

    impl BlockInteractionHandler for Observe {
        fn on_interaction(&self, _world: &mut World, event: &mut BlockInteraction) {
            assert!(event.is_cancelled());
            self.0.lock().unwrap().push(2);
        }
    }

    #[test]
    fn callback_snapshot_allows_registry_mutation_and_sticky_cancellation() {
        let mut world = World::new();
        world.init_resource::<BlockInteractionHooks>();
        let calls = Arc::new(Mutex::new(Vec::new()));
        world
            .resource_mut::<BlockInteractionHooks>()
            .register(RegisterDuringDispatch(calls.clone()));
        let player = world.spawn_empty().id();
        assert!(!authorize(
            &mut world,
            player,
            BlockInteractionKind::Use,
            Vec::new()
        ));
        assert_eq!(*calls.lock().unwrap(), [1]);
        assert!(!authorize(
            &mut world,
            player,
            BlockInteractionKind::Use,
            Vec::new()
        ));
        assert_eq!(*calls.lock().unwrap(), [1, 1, 2]);
    }

    /// Register a handler from a committed callback and start a nested dispatch.
    struct NestedCommitted {
        calls: Arc<Mutex<Vec<u8>>>,
        entered: std::sync::atomic::AtomicBool,
    }
    impl BlockInteractionHandler for NestedCommitted {
        fn on_committed(&self, world: &mut World, event: &PlayerBlockCommitted) {
            self.calls.lock().unwrap().push(1);
            if !self.entered.swap(true, std::sync::atomic::Ordering::SeqCst) {
                world
                    .resource_mut::<BlockInteractionHooks>()
                    .register(LateCommitted(self.calls.clone()));
                BlockCommit::capture(world, event.player(), BlockInteractionKind::Use, [])
                    .finish(world);
            }
        }
    }

    /// Record a handler that was added during an outer committed dispatch.
    struct LateCommitted(Arc<Mutex<Vec<u8>>>);
    impl BlockInteractionHandler for LateCommitted {
        fn on_committed(&self, _world: &mut World, _event: &PlayerBlockCommitted) {
            self.0.lock().unwrap().push(2);
        }
    }

    #[test]
    fn committed_registry_snapshot_allows_registration_and_nested_notification() {
        let mut world = World::new();
        world.init_resource::<BlockInteractionHooks>();
        let calls = Arc::new(Mutex::new(Vec::new()));
        world
            .resource_mut::<BlockInteractionHooks>()
            .register(NestedCommitted {
                calls: calls.clone(),
                entered: std::sync::atomic::AtomicBool::new(false),
            });
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let player = world
            .spawn(PlayerSession::new(1, "actor".into(), None, None, tx))
            .id();
        BlockCommit::capture(&world, player, BlockInteractionKind::Use, []).finish(&mut world);
        assert_eq!(*calls.lock().unwrap(), [1, 1, 2]);
    }
}
