use bevy_ecs::prelude::*;
use tracing::{info, warn};
use unastar_api::PluginAction;
use crate::ecs::events::ActionQueue;
use crate::entity::components::{PlayerSession, PlayerUuid};
use crate::server::game::{GameServer, SessionEntityMap};
use jolyne::valentine::{McpePacket, TextPacket, TextPacketType};

impl GameServer {
    /// Dispatches actions from the ActionQueue (Post-Simulation).
    pub fn dispatch_actions(&mut self) {
        let mut actions = {
            let mut queue = self.ecs.world_mut().get_resource_mut::<ActionQueue>()
                .expect("ActionQueue resource must exist");
            queue.drain()
        };

        for action in actions {
            match action {
                PluginAction::Kick { player_id, reason } => {
                    self.handle_kick_action(&player_id, &reason);
                }
                PluginAction::SendMessage { player_id, message } => {
                    self.handle_send_message_action(&player_id, &message);
                }
                PluginAction::SetBlock { x, y, z, block_name } => {
                    self.handle_set_block_action(x, y, z, &block_name);
                }
                PluginAction::Log { .. } => {
                    // Already handled in PluginManager for convenience, 
                    // but could be handled here if native logic pushes to queue.
                }
            }
        }
    }

    fn handle_set_block_action(&mut self, x: i32, y: i32, z: i32, block_name: &str) {
        // Resolve block name to runtime ID
        // Note: Assuming "minecraft:stone" format or just "stone"
        let name = if block_name.contains(':') {
            block_name.to_string()
        } else {
            format!("minecraft:{}", block_name)
        };

        if let Some(block) = self.blocks.get_by_name(&name) {
            let runtime_id = block.default_state_id;
            info!(pos = ?(x, y, z), block = %name, "Setting block from plugin");
            self.place_block(x, y, z, runtime_id);
        } else {
            warn!(block = %name, "Plugin tried to set unknown block");
        }
    }

    fn handle_kick_action(&mut self, player_uuid_str: &str, reason: &str) {
        let session_id = self.find_session_by_uuid(player_uuid_str);
        
        if let Some(session_id) = session_id {
            info!(uuid = %player_uuid_str, reason = %reason, "Kicking player due to plugin action");
            // In a real implementation, we'd send a DisconnectPacket or close the stream.
            // For now, we just despawn.
            self.despawn_player(session_id);
        }
    }

    fn handle_send_message_action(&self, player_uuid_str: &str, message: &str) {
        let session_entity = self.find_entity_by_uuid(player_uuid_str);

        if let Some(entity) = session_entity {
            let world = self.ecs.world();
            if let Some(session) = world.get::<PlayerSession>(entity) {
                let _ = session.send(McpePacket::from(TextPacket {
                    packet_type: TextPacketType::Raw,
                    needs_translation: false,
                    source_name: "".to_string(),
                    message: message.to_string(),
                    xuid: "".to_string(),
                    platform_chat_id: "".to_string(),
                }));
            }
        }
    }

    fn find_session_by_uuid(&self, uuid_str: &str) -> Option<crate::network::SessionId> {
        let world = self.ecs.world();
        let session_map = world.get_resource::<SessionEntityMap>()?;
        let target_uuid = uuid::Uuid::parse_str(uuid_str).ok()?;

        for (session_id, entity) in session_map.iter() {
            if let Some(uuid_comp) = world.get::<PlayerUuid>(entity) {
                if uuid_comp.0 == target_uuid {
                    return Some(*session_id);
                }
            }
        }
        None
    }

    fn find_entity_by_uuid(&self, uuid_str: &str) -> Option<Entity> {
        let world = self.ecs.world();
        let session_map = world.get_resource::<SessionEntityMap>()?;
        let target_uuid = uuid::Uuid::parse_str(uuid_str).ok()?;

        for (_, entity) in session_map.iter() {
            if let Some(uuid_comp) = world.get::<PlayerUuid>(entity) {
                if uuid_comp.0 == target_uuid {
                    return Some(*entity);
                }
            }
        }
        None
    }
}
