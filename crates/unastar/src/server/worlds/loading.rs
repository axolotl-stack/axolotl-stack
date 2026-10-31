//! Identity-bound client loading barriers after a real server world migration.

use bevy_ecs::prelude::*;
use jolyne::valentine::{BorrowedMcpePacket, BorrowedMcpePacketData, McpePacketName};

use crate::ecs::resources::TickCounter;
use crate::entity::components::PlayerSession;
use crate::server::game::player_events::ConnectionIdentity;

#[derive(Component, Clone)]
pub(super) struct DimensionLoading {
    pub identity: ConnectionIdentity,
    pub id: u32,
    pub expires: u64,
}

/// Whether ordinary gameplay input still waits for this transport's loading ACK.
pub fn is_loading(world: &World, player: Entity) -> bool {
    world.get::<DimensionLoading>(player).is_some()
}

/// Process the bounded native loading handshake before ordinary packet guards.
/// Radius/subchunk requests remain available while client terrain is loading.
pub(crate) fn intercept(world: &mut World, player: Entity, packet: &BorrowedMcpePacket) -> bool {
    let Some(loading) = world.get::<DimensionLoading>(player).cloned() else {
        return false;
    };
    if !loading.identity.matches(world, player) {
        return true;
    }
    if let BorrowedMcpePacketData::ServerboundLoadingScreenPacket(response) = &packet.data {
        if response.loading_screen_id == Some(loading.id)
            && response.loading_screen_packet_type
                == jolyne::valentine::EnumsServerboundLoadingScreenPacketType::Endloadingscreen
        {
            world.entity_mut(player).remove::<DimensionLoading>();
        }
        return true;
    }
    !matches!(
        packet.packet_id(),
        McpePacketName::RequestChunkRadiusPacket | McpePacketName::SubChunkRequestPacket
    )
}

/// Close stalled clients without ever accepting old-dimension gameplay input.
/// Native disconnect still observes/saves the player's actual destination world.
pub(super) fn expire(server: &mut crate::GameServer) {
    let now = server.ecs.world().resource::<TickCounter>().current;
    let expired = {
        let world = server.ecs.world_mut();
        let mut query = world.query::<(Entity, &DimensionLoading)>();
        query
            .iter(world)
            .filter(|(_, loading)| now >= loading.expires)
            .map(|(player, loading)| (player, loading.clone()))
            .collect::<Vec<_>>()
    };
    for (player, loading) in expired {
        let world = server.ecs.world_mut();
        if !loading.identity.matches(world, player) {
            continue;
        }
        if let Some(session) = world.get::<PlayerSession>(player) {
            use jolyne::valentine::*;
            let _ = session.send(McpePacket::from(DisconnectPacket {
                reason: EnumsConnectionDisconnectFailReason::Disconnected,
                messages: DisconnectPacketMessagesUnion::DisconnectPacketMessages(
                    DisconnectPacketMessages {
                        message: "Dimension loading timed out.".into(),
                        filtered_message: "Dimension loading timed out.".into(),
                    },
                ),
            }));
        }
        world.entity_mut(player).remove::<DimensionLoading>();
        server.despawn_player(loading.identity.session_id());
    }
}

/// Remove world-local chunk subscriptions when an actor leaves this ECS world.
pub(crate) fn detach_chunk_viewer(world: &mut World, player: Entity) {
    let mut query = world.query::<&mut crate::world::ecs::ChunkViewers>();
    for mut viewers in query.iter_mut(world) {
        viewers.remove(player);
    }
    let mut manager = world.resource_mut::<crate::world::ecs::ChunkManager>();
    for viewers in manager.pending_viewers.values_mut() {
        viewers.retain(|viewer| *viewer != player);
    }
    for viewers in manager.pending_generation.values_mut() {
        viewers.retain(|viewer| *viewer != player);
    }
}
