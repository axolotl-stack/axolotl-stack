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
        if packet.packet_id() == McpePacketName::DisconnectPacket {
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

        if super::player_events::initial_state_pending(self.ecs.world(), entity) {
            return;
        }

        // Route by packet type into domain queues
        let world = self.ecs.world_mut();

        match packet {
            BorrowedMcpePacket {
                data: BorrowedMcpePacketData::ModalFormResponsePacket(response),
                ..
            } => {
                // Bound borrowed bytes before allocating application-owned text.
                if response
                    .json_response
                    .as_ref()
                    .is_some_and(|data| data.as_bytes().len() > 65536)
                {
                    return;
                }
                let cancelled =
                    response.form_cancel_reason.is_some() || response.json_response.is_none();
                let data = response
                    .json_response
                    .map(|data| data.to_string_lossy().into_owned());
                super::player_events::form_response(
                    world,
                    entity,
                    session_id,
                    response.form_id,
                    data,
                    cancelled,
                );
            }
            // ── Movement ──────────────────────────────────────────────────
            BorrowedMcpePacket {
                data: BorrowedMcpePacketData::PlayerActionPacket(pk),
                ..
            } => {
                if pk.action == jolyne::valentine::types::EnumsPlayerActionType::Respawn {
                    if let Some(mut q) = world
                        .get_resource_mut::<crate::entity::player_life::respawn::RespawnQueue>()
                    {
                        q.0.push((
                            entity,
                            crate::entity::player_life::respawn::RespawnRequest::Action,
                        ));
                    }
                } else if let Some(mut q) = world.get_resource_mut::<MovementPacketQueue>() {
                    q.0.push((entity, MovementInput::PlayerAction(pk.into())));
                }
            }
            // ── Chat & Commands ───────────────────────────────────────────
            BorrowedMcpePacket {
                data: BorrowedMcpePacketData::CommandRequestPacket(pk),
                ..
            } => {
                if let Some(mut q) = world.get_resource_mut::<ChatPacketQueue>() {
                    q.0.push((entity, session_id, ChatAction::Command(pk.into())));
                }
            }

            // ── Chunks ────────────────────────────────────────────────────
            BorrowedMcpePacket {
                data: BorrowedMcpePacketData::RequestChunkRadiusPacket(req),
                ..
            } => {
                if let Some(mut q) = world.get_resource_mut::<ChunkPacketQueue>() {
                    q.0.push((entity, ChunkAction::RadiusRequest(req.into())));
                }
            }
            BorrowedMcpePacket {
                data: BorrowedMcpePacketData::SubChunkRequestPacket(req),
                ..
            } => {
                if let Some(mut q) = world.get_resource_mut::<ChunkPacketQueue>() {
                    q.0.push((entity, ChunkAction::SubchunkRequest(req.into())));
                }
            }

            // ── Inventory ─────────────────────────────────────────────────
            BorrowedMcpePacket {
                data: BorrowedMcpePacketData::ContainerClosePacket(pk),
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
            McpePacketData::PlayerAuthInputPacket(pk) => {
                if let Some(mut q) = world.get_resource_mut::<MovementPacketQueue>() {
                    q.0.push((entity, MovementInput::AuthInput(pk)));
                }
            }
            McpePacketData::TextPacket(pk) => {
                if let Some(mut q) = world.get_resource_mut::<ChatPacketQueue>() {
                    q.0.push((entity, session_id, ChatAction::Text(pk)));
                }
            }
            McpePacketData::MobEquipmentPacket(pk) => {
                if let Some(mut q) = world.get_resource_mut::<InventoryPacketQueue>() {
                    q.0.push((entity, InventoryAction::MobEquipment(pk)));
                }
            }
            McpePacketData::ItemStackRequestPacket(pk) => {
                if let Some(mut q) = world.get_resource_mut::<InventoryPacketQueue>() {
                    q.0.push((entity, InventoryAction::ItemStackRequest(pk)));
                }
            }
            McpePacketData::InteractPacket(pk) => {
                if let Some(mut q) = world.get_resource_mut::<InventoryPacketQueue>() {
                    q.0.push((entity, InventoryAction::Interact(pk)));
                }
            }
            McpePacketData::BlockPickRequestPacket(pk) => {
                if let Some(mut q) = world.get_resource_mut::<InventoryPacketQueue>() {
                    q.0.push((entity, InventoryAction::BlockPick(pk)));
                }
            }
            McpePacketData::InventoryTransactionPacket(pk) => {
                use jolyne::valentine::types::{
                    EnumsItemUseInventoryTransactionActionType,
                    InventoryTransactionPacketTransaction,
                };

                let is_click_block = matches!(
                    &pk.transaction,
                    InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(
                        use_item,
                    ) if use_item.action_type == EnumsItemUseInventoryTransactionActionType::Place
                );

                if is_click_block {
                    if let InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(
                        use_item,
                    ) = pk.transaction
                        && let Some(mut q) = world.get_resource_mut::<BlockPacketQueue>()
                    {
                        q.0.push((entity, BlockAction::BlockClick(*use_item)));
                    }
                } else if let Some(request) = air_use(&pk.transaction) {
                    if let Some(mut q) =
                        world.get_resource_mut::<super::item_use::ItemUseQueue>()
                    {
                        q.0.push((entity, request));
                    }
                } else if let InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(
                    on_actor,
                ) = pk.transaction
                {
                    if let Some(mut q) =
                        world.get_resource_mut::<crate::entity::mob::interaction::actions::MobActionQueue>()
                    {
                        q.0.push((entity, on_actor));
                    }
                } else if let Some(mut q) = world.get_resource_mut::<InventoryPacketQueue>() {
                    q.0.push((entity, InventoryAction::Transaction(pk)));
                }
            }
            McpePacketData::RespawnPacket(pk) => {
                // Only the client-ready state is the client's to send.
                if pk.state == jolyne::valentine::types::EnumsPlayerRespawnState::Clientreadytospawn
                    && let Some(mut q) = world
                        .get_resource_mut::<crate::entity::player_life::respawn::RespawnQueue>()
                {
                    q.0.push((
                        entity,
                        crate::entity::player_life::respawn::RespawnRequest::ClientReady,
                    ));
                }
            }
            McpePacketData::MovePlayerPacket(_) => {
                // Intentionally ignored — use PlayerAuthInput instead
            }
            McpePacketData::AnimatePacket(pk) => {
                crate::server::broadcast::relay_player_swing(world, entity, &pk);
            }
            McpePacketData::SetLocalPlayerAsInitializedPacket(_) => {
                // Ends the streamed spawn; jolyne's receive path checked its runtime ID.
            }
            _other => {
                debug!(session_id, "Unhandled packet");
            }
        }
    }
}

/// The air-use request an inventory transaction makes, if any: `ItemUse`
/// with action `Use` (click air) or any `ItemRelease`.
fn air_use(
    transaction: &jolyne::valentine::types::InventoryTransactionPacketTransaction,
) -> Option<super::item_use::UseRequest> {
    use super::item_use::UseRequest;
    use jolyne::valentine::types::{
        EnumsItemReleaseInventoryTransactionActionType as Release,
        EnumsItemUseInventoryTransactionActionType as ItemUse,
        InventoryTransactionPacketTransaction as T,
    };
    match transaction {
        T::ItemUseInventoryTransaction(use_item) if use_item.action_type == ItemUse::Use => {
            Some(UseRequest::UseAir)
        }
        T::ItemReleaseInventoryTransaction(release) => match release.action_type {
            Release::Release => Some(UseRequest::Release),
            Release::Use => Some(UseRequest::Consume),
            Release::Unknown(_) => None,
        },
        _ => None,
    }
}
