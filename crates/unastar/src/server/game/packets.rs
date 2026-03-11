//! Packet routing.
//!
//! Routes incoming packets into per-domain queues for ECS processing.
//! Each packet is pushed into a typed queue Resource, which the corresponding
//! domain system drains during `PacketApplySet`.

use tracing::{debug, info, warn};

use super::GameServer;
use super::packet_queues::*;
use super::types::SessionEntityMap;
use crate::network::SessionId;
use jolyne::valentine::{
    BorrowedMcpePacket, BorrowedMcpePacketData, McpePacket, McpePacketArgs, McpePacketData,
    McpePacketName,
};

impl GameServer {
    /// Route an inbound packet into the appropriate domain queue.
    ///
    /// Called from the main tick loop before ECS tick runs. Pushes the
    /// packet data into a typed queue Resource so the domain system can
    /// process it during `PacketApplySet`.
    pub fn route_packet(
        &mut self,
        session_id: SessionId,
        packet_args: McpePacketArgs,
        packet: BorrowedMcpePacket,
    ) {
        // Resolve entity
        let entity = {
            let session_map = match self.ecs.world().get_resource::<SessionEntityMap>() {
                Some(map) => map,
                None => return,
            };
            match session_map.get(session_id) {
                Some(e) => e,
                None => return,
            }
        };

        // Disconnect is immediate (not queued) — despawn must happen now
        if packet.packet_id() == McpePacketName::PacketDisconnect {
            info!(session_id, "Client sent disconnect packet");
            if let Some(mut event_buffer) = self
                .ecs
                .world_mut()
                .get_resource_mut::<crate::ecs::events::EventBuffer>()
            {
                event_buffer.push(crate::ecs::events::ServerEvent::PlayerQuit { entity });
            }
            self.despawn_player(session_id);
            return;
        }

        // Route by packet type into domain queues
        let world = self.ecs.world_mut();

        match packet {
            // ── Movement ──────────────────────────────────────────────────
            BorrowedMcpePacket {
                data: BorrowedMcpePacketData::PacketPlayerAction(pk),
                ..
            } => {
                if let Some(mut q) = world.get_resource_mut::<MovementPacketQueue>() {
                    q.0.push((entity, MovementInput::PlayerAction(pk.into())));
                }
            }

            // ── Chat & Commands ───────────────────────────────────────────
            BorrowedMcpePacket {
                data: BorrowedMcpePacketData::PacketCommandRequest(pk),
                ..
            } => {
                if let Some(mut q) = world.get_resource_mut::<ChatPacketQueue>() {
                    q.0.push((entity, session_id, ChatAction::Command(pk.into())));
                }
            }

            // ── Chunks ────────────────────────────────────────────────────
            BorrowedMcpePacket {
                data: BorrowedMcpePacketData::PacketRequestChunkRadius(req),
                ..
            } => {
                if let Some(mut q) = world.get_resource_mut::<ChunkPacketQueue>() {
                    q.0.push((entity, ChunkAction::RadiusRequest(req.into())));
                }
            }
            BorrowedMcpePacket {
                data: BorrowedMcpePacketData::PacketSubchunkRequest(req),
                ..
            } => {
                if let Some(mut q) = world.get_resource_mut::<ChunkPacketQueue>() {
                    q.0.push((entity, ChunkAction::SubchunkRequest(req.into())));
                }
            }

            // ── Inventory ─────────────────────────────────────────────────
            BorrowedMcpePacket {
                data: BorrowedMcpePacketData::PacketContainerClose(pk),
                ..
            } => {
                if let Some(mut q) = world.get_resource_mut::<InventoryPacketQueue>() {
                    q.0.push((entity, InventoryAction::ContainerClose(pk.into())));
                }
            }
            other => match other.into_owned(packet_args) {
                Ok(packet) => Self::route_owned_packet(world, entity, session_id, packet),
                Err(error) => {
                    warn!(session_id, ?error, "Failed to materialize borrowed packet");
                }
            },
        }
    }

    fn route_owned_packet(
        world: &mut bevy_ecs::world::World,
        entity: bevy_ecs::prelude::Entity,
        session_id: SessionId,
        packet: McpePacket,
    ) {
        match packet.data {
            McpePacketData::PacketPlayerAuthInput(pk) => {
                if let Some(mut q) = world.get_resource_mut::<MovementPacketQueue>() {
                    q.0.push((entity, MovementInput::AuthInput(*pk)));
                }
            }
            McpePacketData::PacketText(pk) => {
                if let Some(mut q) = world.get_resource_mut::<ChatPacketQueue>() {
                    q.0.push((entity, session_id, ChatAction::Text(*pk)));
                }
            }
            McpePacketData::PacketMobEquipment(pk) => {
                if let Some(mut q) = world.get_resource_mut::<InventoryPacketQueue>() {
                    q.0.push((entity, InventoryAction::MobEquipment(*pk)));
                }
            }
            McpePacketData::PacketItemStackRequest(pk) => {
                if let Some(mut q) = world.get_resource_mut::<InventoryPacketQueue>() {
                    q.0.push((entity, InventoryAction::ItemStackRequest(pk)));
                }
            }
            McpePacketData::PacketInteract(pk) => {
                if let Some(mut q) = world.get_resource_mut::<InventoryPacketQueue>() {
                    q.0.push((entity, InventoryAction::Interact(*pk)));
                }
            }
            McpePacketData::PacketInventoryTransaction(mut pk) => {
                use jolyne::valentine::types::{
                    TransactionTransactionData, TransactionTransactionType,
                    TransactionUseItemActionType,
                };

                let is_click_block = pk.transaction.transaction_type
                    == TransactionTransactionType::ItemUse
                    && matches!(
                        &pk.transaction.transaction_data,
                        Some(TransactionTransactionData::ItemUse(use_item))
                            if use_item.action_type == TransactionUseItemActionType::ClickBlock
                    );

                if is_click_block {
                    if let Some(TransactionTransactionData::ItemUse(use_item)) =
                        pk.transaction.transaction_data.take()
                        && let Some(mut q) = world.get_resource_mut::<BlockPacketQueue>()
                    {
                        q.0.push((entity, BlockAction::BlockClick(*use_item)));
                    }
                } else if let Some(mut q) = world.get_resource_mut::<InventoryPacketQueue>() {
                    q.0.push((entity, InventoryAction::Transaction(*pk)));
                }
            }
            McpePacketData::PacketMovePlayer(_) => {
                // Intentionally ignored — use PlayerAuthInput instead
            }
            _other => {
                debug!(session_id, "Unhandled packet");
            }
        }
    }
}
