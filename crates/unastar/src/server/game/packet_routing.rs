//! Packet routing infrastructure.
//!
//! This module provides the `PacketQueues` resource that holds domain-specific packet queues.
//! After packets are routed to their appropriate domains, handler systems drain these queues
//! and process the packets in parallel.

use super::packet_domains::*;
use crate::network::SessionId;
use bevy_ecs::prelude::*;

/// Queues of packets routed to specific domains.
///
/// This resource is inserted into the Bevy ECS world and accessed by both the packet router
/// (to enqueue packets) and handler systems (to dequeue and process packets).
///
/// Each domain has its own vector of `(SessionId, Entity, PacketType)` tuples, where:
/// - `SessionId`: The network session that sent the packet
/// - `Entity`: The player entity associated with the session
/// - `PacketType`: The domain-specific packet variant
///
/// # Parallelism
///
/// Handler systems for different domains can run in parallel when they access different
/// ECS components. Bevy's scheduler automatically detects data dependencies and maximizes
/// parallel execution.
#[derive(Resource, Default)]
pub struct PacketQueues {
    /// Movement packet queue (Position, Rotation, PlayerInput, PlayerState)
    pub movement: Vec<(SessionId, Entity, MovementPacket)>,

    /// Block interaction queue (ChunkManager, ChunkData)
    pub blocks: Vec<(SessionId, Entity, BlockPacket)>,

    /// Item use queue (item use transactions and actions)
    pub item_use: Vec<(SessionId, Entity, ItemUsePacket)>,

    /// Inventory management queue (MainInventory, HeldSlot)
    pub inventory: Vec<(SessionId, Entity, InventoryPacket)>,

    /// Chunk loading queue (ChunkManager)
    pub chunks: Vec<(SessionId, Entity, ChunkPacket)>,

    /// Chat and commands queue (PlayerName, permissions, etc.)
    pub chat: Vec<(SessionId, Entity, ChatPacket)>,

    /// Spawn-related queue (respawn, dimension changes)
    pub spawn: Vec<(SessionId, Entity, SpawnPacket)>,
}

impl PacketQueues {
    /// Clear all queues.
    ///
    /// This should be called at the start of each tick after handler systems have processed
    /// all packets from the previous tick.
    pub fn clear_all(&mut self) {
        self.movement.clear();
        self.blocks.clear();
        self.item_use.clear();
        self.inventory.clear();
        self.chunks.clear();
        self.chat.clear();
        self.spawn.clear();
    }
}
