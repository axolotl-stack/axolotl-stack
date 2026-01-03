//! Domain-specific packet type definitions.
//!
//! This module defines packet enums grouped by logical domain (Movement, Blocks, Inventory, etc.).
//! Each domain represents a cohesive set of related packet types that will be processed together
//! by specialized handler systems, enabling parallel packet processing via Bevy ECS.

use jolyne::valentine::*;

/// Movement-related packets.
///
/// Handles player position, rotation, and movement state changes like sprinting,
/// sneaking, swimming, flying, gliding, and crawling.
#[derive(Debug, Clone)]
pub enum MovementPacket {
    /// Client-sent movement packet (usually ignored in server-authoritative mode)
    MovePlayer(MovePlayerPacket),

    /// Server-authoritative movement input from the client
    AuthInput(PlayerAuthInputPacket),

    /// Movement-related player actions (sprinting, sneaking, jumping, etc.)
    Action(PlayerActionPacket),
}

/// Block interaction packets.
///
/// Handles block breaking, placing, and picking in creative mode.
#[derive(Debug, Clone)]
pub enum BlockPacket {
    /// Creative mode block pick request
    PickRequest(BlockPickRequestPacket),

    /// Block-breaking player actions
    Action(PlayerActionPacket),
}

/// Item use packets.
///
/// Handles using items on blocks, entities, or in the air.
#[derive(Debug, Clone)]
pub enum ItemUsePacket {
    /// Item use transaction (click block, click air, etc.)
    Transaction(InventoryTransactionPacket),

    /// Item use player actions
    Action(PlayerActionPacket),
}

/// Inventory management packets.
///
/// Handles inventory transactions, slot changes, and container operations.
#[derive(Debug, Clone)]
pub enum InventoryPacket {
    /// Normal inventory transaction
    Transaction(InventoryTransactionPacket),

    /// Item stack request (creative picks, moves, etc.)
    ItemStackRequest(ItemStackRequestPacket),

    /// Container close notification
    ContainerClose(ContainerClosePacket),

    /// Hotbar slot change
    MobEquipment(MobEquipmentPacket),
}

/// Chunk loading packets.
///
/// Handles chunk and subchunk requests from the client.
#[derive(Debug, Clone)]
pub enum ChunkPacket {
    /// Subchunk request for loading world data
    SubchunkRequest(SubchunkRequestPacket),

    /// Chunk radius change request
    RadiusRequest(RequestChunkRadiusPacket),
}

/// Chat and command packets.
///
/// Handles text messages and command execution.
#[derive(Debug, Clone)]
pub enum ChatPacket {
    /// Text message from player
    Text(TextPacket),

    /// Command execution request
    Command(CommandRequestPacket),
}

/// Spawn-related packets.
///
/// Handles player respawning and dimension changes.
#[derive(Debug, Clone)]
pub enum SpawnPacket {
    /// Spawn-related player actions (respawn, dimension change acknowledgment)
    Action(PlayerActionPacket),
}
