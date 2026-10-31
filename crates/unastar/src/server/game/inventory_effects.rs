//! Durable, idempotent inventory replacement without blocking the ECS tick.
//!
//! A submission validates a complete candidate, quarantines the source's mutable
//! inventory and input, and saves inventory plus its receipt on a provider worker.
//! Only the durable acknowledgement publishes slots and packets. An uncertain
//! result keeps the actor quarantined and requests disconnect; its account must
//! be recovered before further admission. All provider users share one writer.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, mpsc};

use bevy_ecs::prelude::*;
use jolyne::valentine::{
    DisconnectPacket, DisconnectPacketMessages, DisconnectPacketMessagesUnion,
    EnumsConnectionDisconnectFailReason,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use super::GameServer;
use super::types::PlayerProviderResource;
use crate::ecs::PacketApplySet;
use crate::entity::components::{
    ArmourInventory, BreakingState, EnderChestInventory, Health, HeldSlot, MainInventory,
    OffhandSlot, Player, PlayerInput, PlayerSession, PlayerUi, PlayerUuid, Position, Rotation,
    RuntimeEntityId,
};
use crate::storage::player_writer::{
    EffectWrite, EffectWriteFailure, SerializedPlayerProvider, WriteFailureKind,
};
use crate::storage::{
    InventoryEffectRecord, PlayerData, StorageError, StoredItemStack, valid_inventory_effect_id,
};

mod recovery;
pub use recovery::{
    RecoveryFence, RecoverySource, fence_for_recovery, is_recovery_fenced, recovery_account_fenced,
};

/// A complete portable inventory image; network stack IDs are deliberately absent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryState {
    pub revision: u64,
    pub main: Vec<StoredItemStack>,
    pub armour: [StoredItemStack; 4],
    pub offhand: StoredItemStack,
    pub ui: Vec<StoredItemStack>,
    pub ender_chest: Vec<StoredItemStack>,
    pub held_slot: u8,
}

impl InventoryState {
    /// Capture the current inventory epoch and every authoritative player slot.
    pub fn capture(world: &World, player: Entity) -> Result<Self, EffectError> {
        if is_quarantined(world, player) {
            return Err(EffectError::Busy);
        }
        let state = Self {
            revision: world
                .get::<InventoryPersistence>(player)
                .map_or(0, |state| state.revision),
            main: world
                .get::<MainInventory>(player)
                .ok_or(EffectError::MissingPlayer)?
                .0
                .slots()
                .iter()
                .map(Into::into)
                .collect(),
            armour: std::array::from_fn(|slot| {
                world
                    .get::<ArmourInventory>(player)
                    .map(|armour| (&armour.slots()[slot]).into())
                    .unwrap_or_default()
            }),
            offhand: (&world
                .get::<OffhandSlot>(player)
                .ok_or(EffectError::MissingPlayer)?
                .0)
                .into(),
            ui: world
                .get::<PlayerUi>(player)
                .ok_or(EffectError::MissingPlayer)?
                .0
                .slots()
                .iter()
                .map(Into::into)
                .collect(),
            ender_chest: world
                .get::<EnderChestInventory>(player)
                .ok_or(EffectError::MissingPlayer)?
                .0
                .slots()
                .iter()
                .map(Into::into)
                .collect(),
            held_slot: world
                .get::<HeldSlot>(player)
                .ok_or(EffectError::MissingPlayer)?
                .0,
        };
        if world.get::<ArmourInventory>(player).is_none() {
            return Err(EffectError::MissingPlayer);
        }
        state.validate()?;
        Ok(state)
    }

    /// Reject incomplete images before any provider request or live mutation.
    pub fn validate(&self) -> Result<(), EffectError> {
        if self.main.len() != 36
            || self.ui.len() != crate::entity::components::PLAYER_UI_SIZE
            || self.ender_chest.len() != crate::entity::components::ENDER_CHEST_SIZE
            || self.held_slot > 8
        {
            return Err(EffectError::InvalidInventory(
                "invalid inventory dimensions".into(),
            ));
        }
        Ok(())
    }

    /// Copy only inventory fields into a freshly captured player record.
    fn put_into(&self, data: &mut PlayerData) {
        data.inventory = self.main.clone();
        data.armour = self.armour.clone();
        data.offhand = self.offhand.clone();
        data.ui = self.ui.clone();
        data.cursor = self.ui[0].clone();
        data.ender_chest = self.ender_chest.clone();
        data.held_slot = self.held_slot;
        data.inventory_revision = self.revision;
    }
}

/// A stable request ID bound to an expected image and a complete replacement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryEffect {
    pub id: String,
    pub expected: InventoryState,
    pub replacement: InventoryState,
}

/// Synchronous submission never represents an unacknowledged effect as committed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Submission {
    Queued,
    AlreadyCommitted { record: InventoryEffectRecord },
}

/// Preflight failures occur before quarantine or any write attempt.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum EffectError {
    #[error("durable inventory effects are not installed")]
    NotInstalled,
    #[error("a player persistence provider is required")]
    MissingProvider,
    #[error("player provider does not guarantee atomic durable records")]
    UnsupportedProvider,
    #[error("an asynchronous runtime is required for inventory persistence")]
    MissingRuntime,
    #[error("target is not a complete connected live player")]
    MissingPlayer,
    #[error("another inventory operation is pending")]
    Busy,
    #[error("inventory receipt identity or revision conflicts")]
    Conflict,
    #[error("player state requires recovery")]
    RecoveryRequired,
    #[error("inventory effect identifier is invalid")]
    InvalidIdentifier,
    #[error("invalid inventory effect: {0}")]
    InvalidInventory(String),
}

/// Durable notification; a disconnected retained actor may still receive live slots.
#[derive(Event, Clone, Debug)]
pub struct InventoryEffectCommitted {
    pub player: Option<Entity>,
    pub uuid: Uuid,
    pub id: String,
    pub record: InventoryEffectRecord,
    pub already_committed: bool,
    pub live_applied: bool,
}

/// A rejection distinguishes proven non-application from ambiguous persistent state.
#[derive(Event, Clone, Debug)]
pub struct InventoryEffectFailed {
    pub player: Option<Entity>,
    pub uuid: Uuid,
    pub id: String,
    pub kind: WriteFailureKind,
    pub message: String,
}

/// Loaded inventory epoch and receipts, retained across ordinary saves.
#[derive(Component, Clone, Debug, Default)]
pub struct InventoryPersistence {
    revision: u64,
    receipts: BTreeMap<String, InventoryEffectRecord>,
}

impl InventoryPersistence {
    /// Copy persistence metadata from the same record that supplied the player's slots.
    pub fn from_record(data: Option<&PlayerData>) -> Self {
        data.map_or_else(Self::default, |data| Self {
            revision: data.inventory_revision,
            receipts: data.inventory_effects.clone(),
        })
    }
    /// Read the epoch retained by ordinary autosave/disconnect snapshots.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Read durable receipts retained by ordinary autosave/disconnect snapshots.
    pub fn receipts(&self) -> &BTreeMap<String, InventoryEffectRecord> {
        &self.receipts
    }
}

/// Provider workers and native completion delivery share no mutable ECS references.
#[derive(Resource)]
struct Coordinator {
    writer: Arc<SerializedPlayerProvider>,
    tx: mpsc::Sender<Completion>,
    rx: Mutex<mpsc::Receiver<Completion>>,
    pending: BTreeMap<Uuid, String>,
}

/// Prepared inventory components are not visible before provider acknowledgement.
struct Slots {
    main: MainInventory,
    armour: ArmourInventory,
    offhand: OffhandSlot,
    ui: PlayerUi,
    ender: EnderChestInventory,
    held: HeldSlot,
}

/// A frozen actor owns its old and proposed inventories until persistence resolves.
#[derive(Component)]
struct Quarantined {
    uuid: Uuid,
    runtime: i64,
    session: u64,
    xuid: Option<String>,
    protocol_uuid: Option<String>,
    outbound: tokio::sync::mpsc::Sender<jolyne::valentine::McpePacket>,
    id: String,
    expected: InventoryState,
    original: Slots,
    replacement: Slots,
    health: Health,
    input_tick: i64,
}

struct Completion {
    player: Entity,
    uuid: Uuid,
    id: String,
    result: Result<EffectWrite, EffectWriteFailure>,
}

/// Deliver a conservative result even when a provider future unwinds or is cancelled.
struct ReplyGuard {
    tx: mpsc::Sender<Completion>,
    writer: Arc<SerializedPlayerProvider>,
    player: Entity,
    uuid: Uuid,
    id: String,
    finished: bool,
}

impl ReplyGuard {
    /// Publish one worker result and prevent a second cancellation notification.
    fn finish(mut self, result: Result<EffectWrite, EffectWriteFailure>) {
        self.finished = true;
        let _ = self.tx.send(Completion {
            player: self.player,
            uuid: self.uuid,
            id: self.id.clone(),
            result,
        });
    }
}
impl Drop for ReplyGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.writer.require_recovery(self.uuid);
            let _ = self.tx.send(Completion {
                player: self.player,
                uuid: self.uuid,
                id: self.id.clone(),
                result: Err(EffectWriteFailure {
                    kind: WriteFailureKind::RecoveryRequired,
                    resume_source: false,
                    message: "inventory persistence worker interrupted".into(),
                }),
            });
        }
    }
}

/// Install one shared serialized writer and a non-blocking acknowledgement system.
/// Call this before the network loop clones its player provider for login loading.
pub fn install(server: &mut GameServer) -> Result<(), EffectError> {
    let world = server.ecs.world_mut();
    if world.contains_resource::<Coordinator>() {
        return Ok(());
    }
    let provider = world
        .get_resource::<PlayerProviderResource>()
        .ok_or(EffectError::MissingProvider)?
        .0
        .clone();
    if !provider.atomic_durable_records() {
        return Err(EffectError::UnsupportedProvider);
    }
    let writer = Arc::new(SerializedPlayerProvider::new(provider));
    world.insert_resource(PlayerProviderResource(writer.clone()));
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
    Ok(())
}

/// Whether packet-driven gameplay and ordinary saves must stop for this actor.
pub fn is_quarantined(world: &World, player: Entity) -> bool {
    world.get::<Quarantined>(player).is_some() || is_recovery_fenced(world, player)
}

/// Query the durable receipt loaded or acknowledged with the actor's current inventory.
pub fn receipt<'a>(
    world: &'a World,
    player: Entity,
    id: &str,
) -> Option<&'a InventoryEffectRecord> {
    world.get::<InventoryPersistence>(player)?.receipts.get(id)
}

/// Reject admission while an account has an outstanding or ambiguous durable write.
pub(super) fn allow_spawn(world: &World, uuid: Uuid) -> Result<(), StorageError> {
    if recovery::account_fenced(world, uuid) {
        return Err(StorageError::PlayerRecoveryRequired);
    }
    if let Some(coordinator) = world.get_resource::<Coordinator>() {
        if coordinator.writer.recovery_required(uuid) {
            return Err(StorageError::PlayerRecoveryRequired);
        }
        if coordinator.pending.contains_key(&uuid) {
            return Err(StorageError::Database(
                "player inventory write is pending".into(),
            ));
        }
    }
    Ok(())
}

/// Validate, quarantine and schedule one durable effect without polling a future on ECS.
pub fn submit(
    world: &mut World,
    player: Entity,
    effect: InventoryEffect,
) -> Result<Submission, EffectError> {
    if !valid_inventory_effect_id(&effect.id) {
        return Err(EffectError::InvalidIdentifier);
    }
    if is_quarantined(world, player) {
        return Err(EffectError::Busy);
    }
    let handle = tokio::runtime::Handle::try_current().map_err(|_| EffectError::MissingRuntime)?;
    let hash = request_hash(&effect.replacement)?;
    if let Some(record) = receipt(world, player, &effect.id) {
        return if record.request_hash == hash {
            Ok(Submission::AlreadyCommitted {
                record: record.clone(),
            })
        } else {
            Err(EffectError::Conflict)
        };
    }
    let coordinator = world
        .get_resource::<Coordinator>()
        .ok_or(EffectError::NotInstalled)?;
    let uuid = world
        .get::<PlayerUuid>(player)
        .ok_or(EffectError::MissingPlayer)?
        .0;
    if coordinator.writer.recovery_required(uuid) {
        return Err(EffectError::RecoveryRequired);
    }
    if coordinator.pending.contains_key(&uuid) {
        return Err(EffectError::Busy);
    }
    let writer = coordinator.writer.clone();
    let tx = coordinator.tx.clone();
    if world.get::<Player>(player).is_none()
        || world
            .get::<crate::entity::player_life::Dead>(player)
            .is_some()
    {
        return Err(EffectError::MissingPlayer);
    }
    let health = world
        .get::<Health>(player)
        .filter(|health| !health.is_dead())
        .cloned()
        .ok_or(EffectError::MissingPlayer)?;
    let source = InventoryState::capture(world, player)?;
    if source != effect.expected || effect.replacement.revision != source.revision {
        return Err(EffectError::Conflict);
    }
    effect.replacement.validate()?;
    let (_, mut candidate) =
        super::player_record(world, player).ok_or(EffectError::MissingPlayer)?;
    effect.replacement.put_into(&mut candidate);
    candidate
        .validate(uuid)
        .map_err(|error| EffectError::InvalidInventory(error.to_string()))?;
    let replacement = resolve(world, &candidate)?;
    let original = clone_slots(world, player)?;
    let session = world
        .get::<PlayerSession>(player)
        .ok_or(EffectError::MissingPlayer)?;
    let frozen = Quarantined {
        uuid,
        runtime: world
            .get::<RuntimeEntityId>(player)
            .ok_or(EffectError::MissingPlayer)?
            .0,
        session: session.session_id,
        xuid: session.xuid.clone(),
        protocol_uuid: session.uuid.clone(),
        outbound: session.outbound_tx.clone(),
        id: effect.id.clone(),
        expected: source,
        original,
        replacement,
        health,
        input_tick: world
            .get::<PlayerInput>(player)
            .map_or(0, |input| input.tick),
    };
    close_window(world, player);
    world
        .entity_mut(player)
        .remove::<(
            MainInventory,
            ArmourInventory,
            OffhandSlot,
            PlayerUi,
            EnderChestInventory,
            Health,
            PlayerInput,
            BreakingState,
            super::item_use::ItemUsing,
        )>()
        .insert(frozen);
    clear_inputs(world, player);
    world
        .resource_mut::<Coordinator>()
        .pending
        .insert(uuid, effect.id.clone());
    let guard = ReplyGuard {
        tx,
        writer: writer.clone(),
        player,
        uuid,
        id: effect.id.clone(),
        finished: false,
    };
    handle.spawn(async move {
        let result = writer
            .commit_effect(uuid, &effect.id, hash, candidate)
            .await;
        guard.finish(result);
    });
    Ok(Submission::Queued)
}

/// Poll completed writes before packet input can observe newly published identities.
pub fn poll(world: &mut World) {
    let completions: Vec<_> = world
        .get_resource::<Coordinator>()
        .map(|coordinator| {
            coordinator
                .rx
                .lock()
                .map(|rx| rx.try_iter().collect())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    for completion in completions {
        world
            .resource_mut::<Coordinator>()
            .pending
            .remove(&completion.uuid);
        let valid = world
            .get::<Quarantined>(completion.player)
            .is_some_and(|frozen| same_source(world, completion.player, frozen, &completion.id));
        match completion.result {
            Ok(write) => {
                let mut live_applied = false;
                if valid && !write.already_committed {
                    let frozen = world
                        .entity_mut(completion.player)
                        .take::<Quarantined>()
                        .expect("source checked above");
                    let connected = world.get::<PlayerSession>(completion.player).is_some();
                    put_slots(
                        world,
                        completion.player,
                        frozen.replacement,
                        frozen.health,
                        connected,
                        frozen.input_tick,
                    );
                    let mut state = world
                        .get::<InventoryPersistence>(completion.player)
                        .cloned()
                        .unwrap_or_default();
                    state.revision = write.record.revision;
                    state
                        .receipts
                        .insert(completion.id.clone(), write.record.clone());
                    world.entity_mut(completion.player).insert(state);
                    synchronize(world, completion.player);
                    live_applied = true;
                } else if world.get_entity(completion.player).is_ok() {
                    kick(
                        world,
                        completion.player,
                        "Inventory state changed; reconnect to restore its durable result.",
                    );
                }
                world.trigger(InventoryEffectCommitted {
                    player: world
                        .get_entity(completion.player)
                        .is_ok()
                        .then_some(completion.player),
                    uuid: completion.uuid,
                    id: completion.id,
                    record: write.record,
                    already_committed: write.already_committed,
                    live_applied,
                });
            }
            Err(error) => {
                if error.kind == WriteFailureKind::NotApplied && error.resume_source && valid {
                    let frozen = world
                        .entity_mut(completion.player)
                        .take::<Quarantined>()
                        .expect("source checked above");
                    let connected = world.get::<PlayerSession>(completion.player).is_some();
                    put_slots(
                        world,
                        completion.player,
                        frozen.original,
                        frozen.health,
                        connected,
                        frozen.input_tick,
                    );
                    synchronize(world, completion.player);
                } else {
                    if error.kind == WriteFailureKind::RecoveryRequired {
                        world
                            .resource::<Coordinator>()
                            .writer
                            .require_recovery(completion.uuid);
                    }
                    kick(
                        world,
                        completion.player,
                        "Inventory persistence requires recovery before continuing.",
                    );
                }
                world.trigger(InventoryEffectFailed {
                    player: world
                        .get_entity(completion.player)
                        .is_ok()
                        .then_some(completion.player),
                    uuid: completion.uuid,
                    id: completion.id,
                    kind: error.kind,
                    message: error.message,
                });
            }
        }
    }
}

/// A same-generation actor may intentionally lose its session while a write completes.
fn same_source(world: &World, player: Entity, frozen: &Quarantined, id: &str) -> bool {
    frozen.id == id
        && world.get::<Player>(player).is_some()
        && world.get::<Position>(player).is_some()
        && world.get::<Rotation>(player).is_some()
        && world.get::<HeldSlot>(player).map(|slot| slot.0) == Some(frozen.expected.held_slot)
        && world.get::<PlayerUuid>(player).map(|id| id.0) == Some(frozen.uuid)
        && world.get::<RuntimeEntityId>(player).map(|id| id.0) == Some(frozen.runtime)
        && world
            .get::<InventoryPersistence>(player)
            .map_or(0, |state| state.revision)
            == frozen.expected.revision
        && world.get::<MainInventory>(player).is_none()
        && world.get::<ArmourInventory>(player).is_none()
        && world.get::<OffhandSlot>(player).is_none()
        && world.get::<PlayerUi>(player).is_none()
        && world.get::<EnderChestInventory>(player).is_none()
        && world.get::<Health>(player).is_none()
        && world.get::<PlayerSession>(player).is_none_or(|session| {
            session.session_id == frozen.session
                && session.xuid == frozen.xuid
                && session.uuid == frozen.protocol_uuid
                && session.outbound_tx.same_channel(&frozen.outbound)
        })
}

/// Bind idempotency to the complete candidate inventory rather than a mutable display name.
fn request_hash(state: &InventoryState) -> Result<[u8; 32], EffectError> {
    let mut state = state.clone();
    state.revision = 0;
    let bytes = serde_json::to_vec(&state)
        .map_err(|error| EffectError::InvalidInventory(error.to_string()))?;
    Ok(Sha256::digest(bytes).into())
}

/// Resolve every slot through the ordinary persisted-inventory validation path.
fn resolve(world: &World, data: &PlayerData) -> Result<Slots, EffectError> {
    let registry = &world
        .get_resource::<super::types::ItemRegistryResource>()
        .ok_or(EffectError::MissingProvider)?
        .0;
    let mut candidate = data.clone();
    let (main, armour, offhand, ui, ender) = super::restore_items(&mut candidate, registry)
        .map_err(|error| EffectError::InvalidInventory(error.to_string()))?;
    let ids = super::types::block_network_ids(world);
    if main
        .0
        .slots()
        .iter()
        .chain(armour.slots())
        .chain(std::iter::once(&offhand.0))
        .chain(ui.0.slots())
        .chain(ender.0.slots())
        .any(|stack| super::inventory::wire_item(stack, ids).is_none())
    {
        return Err(EffectError::InvalidInventory(
            "unrepresentable item data".into(),
        ));
    }
    Ok(Slots {
        main,
        armour,
        offhand,
        ui,
        ender,
        held: HeldSlot(data.held_slot),
    })
}

/// Keep the original slot identities for a proven pre-write rejection.
fn clone_slots(world: &World, player: Entity) -> Result<Slots, EffectError> {
    Ok(Slots {
        main: world
            .get::<MainInventory>(player)
            .cloned()
            .ok_or(EffectError::MissingPlayer)?,
        armour: world
            .get::<ArmourInventory>(player)
            .cloned()
            .ok_or(EffectError::MissingPlayer)?,
        offhand: world
            .get::<OffhandSlot>(player)
            .cloned()
            .ok_or(EffectError::MissingPlayer)?,
        ui: world
            .get::<PlayerUi>(player)
            .cloned()
            .ok_or(EffectError::MissingPlayer)?,
        ender: world
            .get::<EnderChestInventory>(player)
            .cloned()
            .ok_or(EffectError::MissingPlayer)?,
        held: *world
            .get::<HeldSlot>(player)
            .ok_or(EffectError::MissingPlayer)?,
    })
}

/// Publish slots only into the actor that retained the quarantined source state.
fn put_slots(
    world: &mut World,
    player: Entity,
    slots: Slots,
    health: Health,
    connected: bool,
    input_tick: i64,
) {
    world.entity_mut(player).insert((
        slots.main,
        slots.armour,
        slots.offhand,
        slots.ui,
        slots.ender,
        slots.held,
        health,
        super::RequestIdAssignments::default(),
    ));
    if connected {
        world.entity_mut(player).insert((
            PlayerInput {
                tick: input_tick,
                ..PlayerInput::default()
            },
            BreakingState::default(),
        ));
    }
}

/// Send authoritative slot corrections and reconcile locally predicted movement after quarantine.
fn synchronize(world: &mut World, player: Entity) {
    super::send_inventory_contents(world, player);
    if let (Some(position), Some(rotation)) = (
        world.get::<Position>(player).copied(),
        world.get::<Rotation>(player).copied(),
    ) {
        crate::command::exec::move_entity(
            world,
            player,
            position.0 - glam::DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT,
            rotation.yaw,
            rotation.pitch,
        );
        world.entity_mut(player).insert((position, rotation));
    }
}

/// Close a source window without returning or changing its captured cursor/grid items.
fn close_window(world: &mut World, player: Entity) {
    let state = world
        .get::<crate::entity::components::ContainerWindows>(player)
        .map(|windows| (windows.open_id(), windows.screen()));
    if let Some((Some(id), screen)) = state {
        if let crate::entity::components::ScreenKind::Block(screen) = screen {
            super::containers::stop_viewing(world, player, screen);
        }
        world
            .get_mut::<crate::entity::components::ContainerWindows>(player)
            .expect("window checked")
            .close();
        if let Some(session) = world.get::<PlayerSession>(player) {
            session.send(
                jolyne::valentine::ContainerClosePacket {
                    container_id: id,
                    container_type: 0xff,
                    server_initiated_close: true,
                }
                .into(),
            );
        }
    }
}

/// Discard pre-quarantine inputs which have not yet entered their exclusive systems.
fn clear_inputs(world: &mut World, player: Entity) {
    if let Some(mut queue) = world.get_resource_mut::<super::packet_queues::ChatPacketQueue>() {
        queue.0.retain(|(entity, _, _)| *entity != player);
    }
    if let Some(mut queue) = world.get_resource_mut::<super::packet_queues::InventoryPacketQueue>()
    {
        queue.0.retain(|(entity, _)| *entity != player);
    }
    if let Some(mut queue) = world.get_resource_mut::<super::packet_queues::MovementPacketQueue>() {
        queue.0.retain(|(entity, _)| *entity != player);
    }
    if let Some(mut queue) = world.get_resource_mut::<super::packet_queues::BlockPacketQueue>() {
        queue.0.retain(|(entity, _)| *entity != player);
    }
    if let Some(mut queue) = world.get_resource_mut::<super::commands::PendingCommands>() {
        queue.0.retain(|(entity, _)| *entity != player);
    }
    if let Some(mut queue) = world.get_resource_mut::<super::item_use::ItemUseQueue>() {
        queue.0.retain(|(entity, _)| *entity != player);
    }
    if let Some(mut queue) =
        world.get_resource_mut::<crate::entity::mob::interaction::actions::MobActionQueue>()
    {
        queue.0.retain(|(entity, _)| *entity != player);
    }
}

/// Ask the normal transport to close while the actor remains protected from mutations.
fn kick(world: &World, player: Entity, message: &str) {
    if let Some(session) = world.get::<PlayerSession>(player) {
        session.send(
            DisconnectPacket {
                reason: EnumsConnectionDisconnectFailReason::Disconnected,
                messages: DisconnectPacketMessagesUnion::DisconnectPacketMessages(
                    DisconnectPacketMessages {
                        message: message.into(),
                        filtered_message: message.into(),
                    },
                ),
            }
            .into(),
        );
    }
}

#[cfg(test)]
mod tests {
    mod recovery;
    use super::*;
    use crate::item::test_support::stack;
    use crate::server::game::{PlayerSpawnData, ServerConfig};
    use crate::storage::{PlayerProvider, StorageResult};
    use crate::world::WorldConfig;
    use async_trait::async_trait;
    use jolyne::valentine::{McpePacket, McpePacketData};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tokio::sync::Semaphore;

    /// Stop exactly at a provider's write boundary and optionally fail after commit.
    struct TestProvider {
        durable: bool,
        records: Mutex<BTreeMap<Uuid, PlayerData>>,
        started: Semaphore,
        write_gate: Semaphore,
        fail_after_write: AtomicBool,
        writes: AtomicUsize,
    }
    impl TestProvider {
        /// Begin with an unwritten account and a closed deterministic write gate.
        fn new() -> Self {
            Self {
                durable: true,
                records: Mutex::new(BTreeMap::new()),
                started: Semaphore::new(0),
                write_gate: Semaphore::new(0),
                fail_after_write: AtomicBool::new(false),
                writes: AtomicUsize::new(0),
            }
        }
    }
    #[async_trait]
    impl PlayerProvider for TestProvider {
        fn atomic_durable_records(&self) -> bool {
            self.durable
        }
        async fn load(&self, uuid: Uuid) -> StorageResult<Option<PlayerData>> {
            Ok(self.records.lock().unwrap().get(&uuid).cloned())
        }
        async fn save(&self, uuid: Uuid, data: &PlayerData) -> StorageResult<()> {
            self.started.add_permits(1);
            self.write_gate.acquire().await.unwrap().forget();
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.records.lock().unwrap().insert(uuid, data.clone());
            if self.fail_after_write.load(Ordering::SeqCst) {
                Err(StorageError::Database(
                    "injected ambiguous write acknowledgement".into(),
                ))
            } else {
                Ok(())
            }
        }
        async fn delete(&self, uuid: Uuid) -> StorageResult<()> {
            self.records.lock().unwrap().remove(&uuid);
            Ok(())
        }
        async fn close(&self) -> StorageResult<()> {
            Ok(())
        }
    }

    /// Capture native completion events through the immediate observer boundary.
    #[derive(Resource, Default)]
    struct Observed {
        committed: Vec<InventoryEffectCommitted>,
        failed: Vec<InventoryEffectFailed>,
    }

    /// A connected authoritative player and a controlled persistence provider.
    fn harness() -> (
        GameServer,
        Entity,
        Arc<TestProvider>,
        tokio::sync::mpsc::Receiver<McpePacket>,
    ) {
        let mut server = GameServer::with_config(ServerConfig {
            world: WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        });
        let provider = Arc::new(TestProvider::new());
        server
            .ecs
            .world_mut()
            .insert_resource(PlayerProviderResource(provider.clone()));
        install(&mut server).unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(1 << 16);
        let player = server
            .spawn_player(PlayerSpawnData {
                session_id: 7,
                display_name: "Alice".into(),
                xuid: Some("123".into()),
                uuid: Some("00000000-0000-0000-0000-000000000007".into()),
                runtime_id: 7,
                position: glam::DVec3::new(1.5, 17.62, 1.5),
                outbound_tx: tx,
                saved_data: None,
                chunk_radius: 2,
            })
            .unwrap();
        server.ecs.world_mut().insert_resource(Observed::default());
        server.ecs.world_mut().add_observer(
            |event: On<InventoryEffectCommitted>, mut observed: ResMut<Observed>| {
                observed.committed.push(event.event().clone())
            },
        );
        server.ecs.world_mut().add_observer(
            |event: On<InventoryEffectFailed>, mut observed: ResMut<Observed>| {
                observed.failed.push(event.event().clone())
            },
        );
        while rx.try_recv().is_ok() {}
        (server, player, provider, rx)
    }

    /// Replace a selected stack and a crafting input as one complete operation.
    fn effect(world: &World, player: Entity, id: &str) -> InventoryEffect {
        let expected = InventoryState::capture(world, player).unwrap();
        let mut replacement = expected.clone();
        replacement.main[0] = (&stack("minecraft:stone", 17)).into();
        replacement.ui[28] = (&stack("minecraft:oak_log", 3)).into();
        InventoryEffect {
            id: id.into(),
            expected,
            replacement,
        }
    }

    /// Poll only this operation with a bounded asynchronous deadline and no sleeps.
    async fn finish(server: &mut GameServer) {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                poll(server.ecs.world_mut());
                let observed = server.ecs.world().resource::<Observed>();
                if !observed.committed.is_empty() || !observed.failed.is_empty() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("controlled persistence operation completes");
    }

    #[tokio::test]
    async fn slots_publish_only_after_durable_ack_and_queued_inputs_cannot_change_the_source() {
        let (mut server, player, provider, mut rx) = harness();
        let request = effect(server.ecs.world(), player, "one");
        assert_eq!(
            submit(server.ecs.world_mut(), player, request.clone()),
            Ok(Submission::Queued)
        );
        provider.started.acquire().await.unwrap().forget();
        assert!(is_quarantined(server.ecs.world(), player));
        assert!(server.ecs.world().get::<MainInventory>(player).is_none());
        assert!(!server.save_player(7).await);
        server.tick();
        server
            .ecs
            .world_mut()
            .resource_mut::<super::super::commands::PendingCommands>()
            .0
            .push((
                player,
                jolyne::valentine::CommandRequestPacket {
                    command: "/give @s diamond 64".into(),
                    ..Default::default()
                },
            ));
        super::super::commands::run_commands(server.ecs.world_mut());
        assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
        assert!(
            server
                .ecs
                .world()
                .resource::<Observed>()
                .committed
                .is_empty()
        );
        assert!(
            !std::iter::from_fn(|| rx.try_recv().ok())
                .any(|packet| matches!(packet.data, McpePacketData::InventoryContentPacket(_)))
        );
        provider.write_gate.add_permits(1);
        finish(&mut server).await;
        assert!(!is_quarantined(server.ecs.world(), player));
        assert_eq!(
            server
                .ecs
                .world()
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(0)
                .unwrap()
                .count,
            17
        );
        assert_eq!(
            server
                .ecs
                .world()
                .get::<PlayerUi>(player)
                .unwrap()
                .0
                .item(28)
                .unwrap()
                .count,
            3
        );
        let record = receipt(server.ecs.world(), player, "one").unwrap().clone();
        assert_eq!(record.revision, 1);
        assert!(server.ecs.world().resource::<Observed>().committed[0].live_applied);
        assert_eq!(
            submit(server.ecs.world_mut(), player, request),
            Ok(Submission::AlreadyCommitted { record })
        );
        assert_eq!(provider.writes.load(Ordering::SeqCst), 1);
        assert!(
            std::iter::from_fn(|| rx.try_recv().ok())
                .any(|packet| matches!(packet.data, McpePacketData::InventoryContentPacket(_)))
        );
    }

    #[tokio::test]
    async fn concurrent_stale_autosave_waits_for_effect_then_cannot_overwrite_it() {
        let (mut server, player, provider, _) = harness();
        let (uuid, before) = super::super::player_record(server.ecs.world(), player).unwrap();
        let request = effect(server.ecs.world(), player, "one");
        submit(server.ecs.world_mut(), player, request).unwrap();
        provider.started.acquire().await.unwrap().forget();
        let writer = server
            .ecs
            .world()
            .resource::<PlayerProviderResource>()
            .0
            .clone();
        let save = tokio::spawn(async move { writer.save(uuid, &before).await });
        provider.write_gate.add_permits(1);
        finish(&mut server).await;
        assert!(matches!(
            save.await.unwrap(),
            Err(StorageError::PlayerRevisionConflict)
        ));
        let record = provider.records.lock().unwrap()[&uuid].clone();
        assert_eq!(record.inventory[0].count, 17);
        assert_eq!(record.inventory_revision, 1);
        provider.write_gate.add_permits(1);
        assert!(server.save_player(7).await);
        assert_eq!(
            provider.records.lock().unwrap()[&uuid]
                .inventory_effects
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn retained_sessionless_actor_receives_committed_slots_without_old_input_ownership() {
        let (mut server, player, provider, _) = harness();
        let request = effect(server.ecs.world(), player, "one");
        submit(server.ecs.world_mut(), player, request).unwrap();
        provider.started.acquire().await.unwrap().forget();
        server
            .ecs
            .world_mut()
            .entity_mut(player)
            .remove::<PlayerSession>();
        provider.write_gate.add_permits(1);
        finish(&mut server).await;
        assert!(!is_quarantined(server.ecs.world(), player));
        assert!(server.ecs.world().get::<PlayerSession>(player).is_none());
        assert!(server.ecs.world().get::<PlayerInput>(player).is_none());
        assert!(server.ecs.world().get::<Health>(player).is_some());
        assert_eq!(
            server
                .ecs
                .world()
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(0)
                .unwrap()
                .count,
            17
        );
        assert!(server.ecs.world().resource::<Observed>().committed[0].live_applied);
    }

    #[tokio::test]
    async fn replaced_connections_cannot_receive_a_stale_durable_effect() {
        let (mut server, player, provider, mut rx) = harness();
        let request = effect(server.ecs.world(), player, "one");
        submit(server.ecs.world_mut(), player, request).unwrap();
        provider.started.acquire().await.unwrap().forget();
        server
            .ecs
            .world_mut()
            .get_mut::<PlayerSession>(player)
            .unwrap()
            .session_id = 99;
        provider.write_gate.add_permits(1);
        finish(&mut server).await;
        assert!(is_quarantined(server.ecs.world(), player));
        assert!(!server.ecs.world().resource::<Observed>().committed[0].live_applied);
        assert!(
            std::iter::from_fn(|| rx.try_recv().ok())
                .any(|packet| matches!(packet.data, McpePacketData::DisconnectPacket(_)))
        );
    }

    #[tokio::test]
    async fn ambiguous_acknowledgement_keeps_quarantine_and_receipt_is_recoverable_after_restart() {
        let (mut server, player, provider, mut rx) = harness();
        provider.fail_after_write.store(true, Ordering::SeqCst);
        let request = effect(server.ecs.world(), player, "one");
        submit(server.ecs.world_mut(), player, request).unwrap();
        provider.started.acquire().await.unwrap().forget();
        provider.write_gate.add_permits(1);
        finish(&mut server).await;
        assert!(is_quarantined(server.ecs.world(), player));
        assert_eq!(
            server.ecs.world().resource::<Observed>().failed[0].kind,
            WriteFailureKind::RecoveryRequired
        );
        assert!(!server.save_player(7).await);
        let uuid = server.ecs.world().get::<PlayerUuid>(player).unwrap().0;
        assert!(matches!(
            server
                .ecs
                .world()
                .resource::<PlayerProviderResource>()
                .0
                .load(uuid)
                .await,
            Err(StorageError::PlayerRecoveryRequired)
        ));
        assert!(
            std::iter::from_fn(|| rx.try_recv().ok())
                .any(|packet| matches!(packet.data, McpePacketData::DisconnectPacket(_)))
        );
        let recovered = SerializedPlayerProvider::new(provider.clone());
        let saved = recovered.load(uuid).await.unwrap().unwrap();
        assert_eq!(saved.inventory[0].count, 17);
        assert_eq!(saved.inventory_effects["one"].revision, 1);
    }

    #[tokio::test]
    async fn malformed_or_wrong_expected_inventory_never_quarantines_or_writes() {
        let (mut server, player, provider, mut rx) = harness();
        let before = InventoryState::capture(server.ecs.world(), player).unwrap();
        let mut request = effect(server.ecs.world(), player, "one");
        request.replacement.main[35] = StoredItemStack {
            item_id: "minecraft:unknown_item".into(),
            count: 1,
            ..Default::default()
        };
        assert!(matches!(
            submit(server.ecs.world_mut(), player, request),
            Err(EffectError::InvalidInventory(_))
        ));
        let mut request = effect(server.ecs.world(), player, "two");
        request.expected.held_slot = 1;
        assert_eq!(
            submit(server.ecs.world_mut(), player, request),
            Err(EffectError::Conflict)
        );
        assert!(!is_quarantined(server.ecs.world(), player));
        assert_eq!(
            InventoryState::capture(server.ecs.world(), player).unwrap(),
            before
        );
        assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn unsupported_providers_are_rejected_before_installation_or_io() {
        let mut server = GameServer::with_config(ServerConfig {
            world: WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        });
        let mut provider = TestProvider::new();
        provider.durable = false;
        let provider = Arc::new(provider);
        server
            .ecs
            .world_mut()
            .insert_resource(PlayerProviderResource(provider.clone()));
        assert_eq!(install(&mut server), Err(EffectError::UnsupportedProvider));
        assert!(!server.ecs.world().contains_resource::<Coordinator>());
        assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
    }
}
