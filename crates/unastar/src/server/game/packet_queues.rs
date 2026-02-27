//! Per-domain packet queue Resources for ECS-driven packet processing.
//!
//! Each domain has its own queue so bevy sees them as independent Resources,
//! enabling parallel scheduling of domain systems.

use bevy_ecs::prelude::*;

use crate::ecs::events::ServerEvent;
use crate::network::SessionId;

// ── Movement ────────────────────────────────────────────────────────────────

/// Movement packets waiting to be processed this tick.
#[derive(Resource, Default)]
pub struct MovementPacketQueue(pub Vec<(Entity, MovementInput)>);

/// Discriminated movement input.
pub enum MovementInput {
    /// Server-authoritative movement from PlayerAuthInput.
    AuthInput(jolyne::valentine::PlayerAuthInputPacket),
    /// Discrete player actions (sprint, sneak, jump, etc.).
    PlayerAction(jolyne::valentine::PlayerActionPacket),
}

/// Domain event buffer for the movement system.
#[derive(Resource, Default)]
pub struct MovementEvents(pub Vec<ServerEvent>);

// ── Blocks ──────────────────────────────────────────────────────────────────

/// Block action packets waiting to be processed.
#[derive(Resource, Default)]
pub struct BlockPacketQueue(pub Vec<(Entity, BlockAction)>);

/// Block action types.
pub enum BlockAction {
    /// Block actions extracted from PlayerAuthInput (break start/stop/crack/abort/predict).
    AuthInputActions(Vec<jolyne::valentine::PlayerAuthInputPacketBlockActionItem>),
    /// Block placement from InventoryTransaction ItemUse::ClickBlock.
    BlockClick(jolyne::valentine::types::TransactionUseItem),
}

// ── Inventory ───────────────────────────────────────────────────────────────

/// Inventory packets waiting to be processed.
#[derive(Resource, Default)]
pub struct InventoryPacketQueue(pub Vec<(Entity, InventoryAction)>);

/// Inventory action types.
pub enum InventoryAction {
    ItemStackRequest(jolyne::valentine::ItemStackRequestPacket),
    ContainerClose(jolyne::valentine::ContainerClosePacket),
    MobEquipment(jolyne::valentine::MobEquipmentPacket),
    Interact(jolyne::valentine::InteractPacket),
    Transaction(jolyne::valentine::InventoryTransactionPacket),
}

/// Domain event buffer for the inventory system.
#[derive(Resource, Default)]
pub struct InventoryEvents(pub Vec<ServerEvent>);

// ── Chat & Commands ─────────────────────────────────────────────────────────

/// Chat and command packets waiting to be processed.
#[derive(Resource, Default)]
pub struct ChatPacketQueue(pub Vec<(Entity, SessionId, ChatAction)>);

/// Chat action types.
pub enum ChatAction {
    Text(jolyne::valentine::TextPacket),
    Command(jolyne::valentine::CommandRequestPacket),
}

/// Domain event buffer for the chat system.
#[derive(Resource, Default)]
pub struct ChatEvents(pub Vec<ServerEvent>);

// ── Chunks ──────────────────────────────────────────────────────────────────

/// Chunk request packets waiting to be processed.
#[derive(Resource, Default)]
pub struct ChunkPacketQueue(pub Vec<(Entity, ChunkAction)>);

/// Chunk action types.
pub enum ChunkAction {
    SubchunkRequest(jolyne::valentine::SubchunkRequestPacket),
    RadiusRequest(jolyne::valentine::RequestChunkRadiusPacket),
}

// ── Event consolidation ─────────────────────────────────────────────────────

/// Consolidate per-domain event buffers into the main EventBuffer.
/// Runs after all domain systems so plugins see every event.
pub fn consolidate_domain_events(
    mut main_buffer: ResMut<crate::ecs::events::EventBuffer>,
    mut movement: ResMut<MovementEvents>,
    mut inventory: ResMut<InventoryEvents>,
    mut chat: ResMut<ChatEvents>,
) {
    for event in movement.0.drain(..) {
        main_buffer.push(event);
    }
    for event in inventory.0.drain(..) {
        main_buffer.push(event);
    }
    for event in chat.0.drain(..) {
        main_buffer.push(event);
    }
}
