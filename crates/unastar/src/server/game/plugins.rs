use crate::ecs::events::ActionQueue;
use crate::entity::components::{PlayerSession, PlayerUuid, Position, Rotation, RuntimeEntityId};
use crate::server::game::types::system_text;
use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::{
    LegacyEntityType, McpePacket, MovePlayerPacket, MovePlayerPacketMode, MovePlayerPacketTeleport,
    MovePlayerPacketTeleportCause, Vec3F,
};
use tracing::{info, warn};
use unastar_api::PluginAction;

/// System to handle actions requested by plugins via API.
pub fn process_plugin_actions(
    mut action_queue: ResMut<ActionQueue>,
    mut players: Query<(
        &mut Position,
        &mut Rotation,
        &RuntimeEntityId,
        &PlayerUuid,
        &PlayerSession,
    )>,
) {
    for action in action_queue.drain() {
        match action {
            PluginAction::SendMessage { player_id, message } => {
                for (_, _, _, uuid, session) in players.iter() {
                    if uuid.0.to_string() == player_id {
                        let packet = system_text(&message);
                        let _ = session.send(McpePacket::from(packet));
                        break;
                    }
                }
            }
            PluginAction::Teleport {
                player_id,
                position: pos,
            } => {
                for (mut player_pos, mut rot, rid, uuid, session) in players.iter_mut() {
                    if uuid.0.to_string() == player_id {
                        let new_pos = DVec3::new(pos.x, pos.y, pos.z);
                        player_pos.0 = new_pos;

                        // Send Teleport Packet
                        let packet = MovePlayerPacket {
                            runtime_id: rid.0 as i32,
                            position: Vec3F {
                                x: pos.x as f32,
                                y: pos.y as f32,
                                z: pos.z as f32,
                            },
                            pitch: rot.pitch,
                            yaw: rot.yaw,
                            head_yaw: rot.yaw, // Use same yaw
                            mode: MovePlayerPacketMode::Teleport,
                            on_ground: false,
                            ridden_runtime_id: 0,
                            teleport: Some(MovePlayerPacketTeleport {
                                cause: MovePlayerPacketTeleportCause::Command,
                                source_entity_type: LegacyEntityType::Player,
                            }),
                            tick: 0,
                        };
                        let _ = session.send(McpePacket::from(packet));

                        info!(player=%player_id, to=?new_pos, "Plugin teleported player");
                        break;
                    }
                }
            }
            PluginAction::Log { .. } => {
                // Handled in PluginManager immediately, shouldn't be here
            }
            PluginAction::Kick { player_id, reason } => {
                for (_, _, _, uuid, _session) in players.iter() {
                    if uuid.0.to_string() == player_id {
                        // TODO: Send DisconnectPacket
                        warn!(player=%player_id, reason=%reason, "Plugin kick requested (not impl)");
                        break;
                    }
                }
            }
        }
    }
}
