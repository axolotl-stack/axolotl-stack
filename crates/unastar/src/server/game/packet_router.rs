//! Packet routing logic.
//!
//! This module implements the `PacketRouter` which routes incoming packets to their appropriate
//! domain queues based on packet type and, for certain packets, internal discriminant fields.

use bevy_ecs::prelude::*;
use jolyne::valentine::types::{Action, TransactionTransactionType};
use jolyne::valentine::*;
use tracing::{debug, warn};

use super::packet_domains::*;
use super::packet_routing::PacketQueues;
use crate::network::SessionId;

/// Packet routing engine.
///
/// Routes incoming packets to domain-specific queues based on:
/// 1. Packet type (simple 1:1 mappings)
/// 2. Discriminant fields (e.g., `PlayerAction.action`, `InventoryTransaction.transaction_type`)
pub struct PacketRouter;

impl PacketRouter {
    /// Route a packet to the appropriate domain queue.
    ///
    /// # Arguments
    /// * `session_id` - Network session that sent the packet
    /// * `entity` - Player entity associated with the session
    /// * `packet` - The raw Minecraft packet to route
    /// * `queues` - Mutable reference to the packet queues resource
    ///
    /// # Routing Strategy
    ///
    /// Most packets have a simple 1:1 mapping to a domain. For example:
    /// - `MovePlayerPacket` → `MovementPacket::MovePlayer`
    /// - `TextPacket` → `ChatPacket::Text`
    ///
    /// Some packets are routed based on internal discriminant fields:
    /// - `PlayerActionPacket` is routed by the `action` field to different domains
    /// - `InventoryTransactionPacket` is routed by the `transaction_type` field
    pub fn route_packet(
        session_id: SessionId,
        entity: Entity,
        packet: McpePacket,
        queues: &mut PacketQueues,
    ) {
        match &packet.data {
            // ===== Movement Domain =====
            // Box<MovePlayerPacket>
            McpePacketData::PacketMovePlayer(pk) => {
                queues.movement.push((
                    session_id,
                    entity,
                    MovementPacket::MovePlayer((**pk).clone()),
                ));
            }
            // Box<PlayerAuthInputPacket>
            McpePacketData::PacketPlayerAuthInput(pk) => {
                queues.movement.push((
                    session_id,
                    entity,
                    MovementPacket::AuthInput((**pk).clone()),
                ));
            }

            // ===== Block Domain =====
            // Box<BlockPickRequestPacket>
            McpePacketData::PacketBlockPickRequest(pk) => {
                queues
                    .blocks
                    .push((session_id, entity, BlockPacket::PickRequest((**pk).clone())));
            }

            // ===== Inventory Domain =====
            // ItemStackRequestPacket (NOT boxed)
            McpePacketData::PacketItemStackRequest(pk) => {
                queues.inventory.push((
                    session_id,
                    entity,
                    InventoryPacket::ItemStackRequest(pk.clone()),
                ));
            }
            // ContainerClosePacket (NOT boxed)
            McpePacketData::PacketContainerClose(pk) => {
                queues.inventory.push((
                    session_id,
                    entity,
                    InventoryPacket::ContainerClose(pk.clone()),
                ));
            }
            // Box<MobEquipmentPacket>
            McpePacketData::PacketMobEquipment(pk) => {
                queues.inventory.push((
                    session_id,
                    entity,
                    InventoryPacket::MobEquipment((**pk).clone()),
                ));
            }

            // ===== Chunk Domain =====
            // SubchunkRequestPacket (NOT boxed)
            McpePacketData::PacketSubchunkRequest(pk) => {
                queues
                    .chunks
                    .push((session_id, entity, ChunkPacket::SubchunkRequest(pk.clone())));
            }
            // RequestChunkRadiusPacket (NOT boxed)
            McpePacketData::PacketRequestChunkRadius(pk) => {
                queues
                    .chunks
                    .push((session_id, entity, ChunkPacket::RadiusRequest(pk.clone())));
            }

            // ===== Chat Domain =====
            // Box<TextPacket>
            McpePacketData::PacketText(pk) => {
                queues
                    .chat
                    .push((session_id, entity, ChatPacket::Text((**pk).clone())));
            }
            // Box<CommandRequestPacket>
            McpePacketData::PacketCommandRequest(pk) => {
                queues
                    .chat
                    .push((session_id, entity, ChatPacket::Command((**pk).clone())));
            }

            // ===== Discriminant-Based Routing =====

            // Box<PlayerActionPacket> - route by action field
            McpePacketData::PacketPlayerAction(pk) => {
                Self::route_player_action(session_id, entity, (**pk).clone(), queues);
            }

            // Box<InventoryTransactionPacket> - route by transaction_type field
            McpePacketData::PacketInventoryTransaction(pk) => {
                Self::route_inventory_transaction(session_id, entity, (**pk).clone(), queues);
            }

            // ===== Unhandled Packets =====
            _ => {
                debug!(
                    session_id,
                    packet_id = ?packet.data.packet_id(),
                    "Unhandled packet in router"
                );
            }
        }
    }

    /// Route PlayerAction packets based on the `action` discriminant field.
    ///
    /// PlayerAction packets are used for many different purposes, so we route them
    /// to different domains based on what the action is doing:
    /// - Block breaking → Block domain
    /// - Movement state → Movement domain
    /// - Item use → ItemUse domain
    /// - Spawn/respawn → Spawn domain
    fn route_player_action(
        session_id: SessionId,
        entity: Entity,
        pk: PlayerActionPacket,
        queues: &mut PacketQueues,
    ) {
        use Action::*;

        match pk.action {
            // Block breaking actions → Block domain
            StartBreak
            | AbortBreak
            | StopBreak
            | ContinueBreak
            | CrackBreak
            | PredictBreak
            | CreativePlayerDestroyBlock => {
                queues
                    .blocks
                    .push((session_id, entity, BlockPacket::Action(pk)));
            }

            // Movement state actions → Movement domain
            StartSprint | StopSprint | StartSneak | StopSneak | Jump | Swimming | StopSwimming
            | StartGlide | StopGlide | StartCrawling | StopCrawling | StartFlying | StopFlying => {
                queues
                    .movement
                    .push((session_id, entity, MovementPacket::Action(pk)));
            }

            // Item use actions → ItemUse domain
            StartItemUseOn | StopItemUseOn | InteractBlock | StartUsingItem => {
                queues
                    .item_use
                    .push((session_id, entity, ItemUsePacket::Action(pk)));
            }

            // Spawn/respawn actions → Spawn domain
            Respawn | DimensionChangeAck => {
                queues
                    .spawn
                    .push((session_id, entity, SpawnPacket::Action(pk)));
            }

            // Other actions - log and ignore
            _ => {
                warn!(
                    session_id,
                    action = ?pk.action,
                    "Unhandled PlayerAction variant in router"
                );
            }
        }
    }

    /// Route InventoryTransaction packets based on the `transaction_type` discriminant field.
    ///
    /// InventoryTransaction packets can represent:
    /// - Item use (clicking blocks, using items) → ItemUse domain
    /// - Normal inventory operations (moving items) → Inventory domain
    fn route_inventory_transaction(
        session_id: SessionId,
        entity: Entity,
        pk: InventoryTransactionPacket,
        queues: &mut PacketQueues,
    ) {
        use TransactionTransactionType::*;

        match pk.transaction.transaction_type {
            // Item use transactions → ItemUse domain
            ItemUse | ItemUseOnEntity | ItemRelease => {
                queues
                    .item_use
                    .push((session_id, entity, ItemUsePacket::Transaction(pk)));
            }

            // Inventory transactions → Inventory domain
            Normal | InventoryMismatch => {
                queues
                    .inventory
                    .push((session_id, entity, InventoryPacket::Transaction(pk)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_player_action_routing() {
        // TODO: Add unit tests for routing logic
        // - Test each PlayerAction variant routes to correct domain
        // - Test each InventoryTransaction type routes correctly
        // - Test simple 1:1 packet routing
    }
}
