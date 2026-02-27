//! Chat and command domain system.
//!
//! Processes Text (chat broadcast) and CommandRequest packets, including
//! the /tp command which writes Position.

use bevy_ecs::prelude::*;
use glam::DVec3;
use tracing::{debug, info, trace};

use super::packet_queues::{ChatAction, ChatEvents, ChatPacketQueue};
use super::types::{CommandRegistryResource, SessionEntityMap, system_text};
use crate::command::{CommandOutput, CommandParseError, parse_command_line};
use crate::ecs::events::ServerEvent;
use crate::entity::components::transform::Position;
use crate::entity::components::{PlayerName, PlayerSession, PlayerUuid, RuntimeEntityId};
use crate::world::ecs::ChunkLoader;
use jolyne::valentine::types::{LegacyEntityType, Vec3F};
use jolyne::valentine::{
    McpePacket, MovePlayerPacket, MovePlayerPacketMode, MovePlayerPacketTeleport,
    MovePlayerPacketTeleportCause, TextPacket, TextPacketCategory, TextPacketContent,
    TextPacketContentAnnouncement, TextPacketType,
};

/// ECS system: drain chat/command packet queue and process.
///
/// Runs in `PacketApplySet`. Serializes with movement (both write Position
/// due to /tp), but parallel with inventory and chunks.
#[allow(clippy::too_many_arguments)]
pub fn apply_chat_and_commands(
    mut queue: ResMut<ChatPacketQueue>,
    mut events: ResMut<ChatEvents>,
    session_map: Res<SessionEntityMap>,
    commands: Res<CommandRegistryResource>,
    tick_counter: Res<crate::ecs::resources::TickCounter>,
    sessions: Query<&PlayerSession>,
    names: Query<(&PlayerName, &PlayerUuid)>,
    runtime_ids: Query<&RuntimeEntityId>,
    mut tp_targets: Query<(&mut Position, Option<&mut ChunkLoader>)>,
) {
    let _span = tracing::info_span!("apply_chat_and_commands", count = queue.0.len()).entered();
    for (entity, _session_id, action) in queue.0.drain(..) {
        match action {
            ChatAction::Text(pk) => {
                handle_text(entity, &pk, &session_map, &sessions, &names, &mut events);
            }
            ChatAction::Command(req) => {
                handle_command_request(
                    entity,
                    &req,
                    &commands,
                    &tick_counter,
                    &sessions,
                    &runtime_ids,
                    &mut tp_targets,
                );
            }
        }
    }
}

fn handle_text(
    entity: Entity,
    pk: &TextPacket,
    session_map: &Res<SessionEntityMap>,
    sessions: &Query<&PlayerSession>,
    names: &Query<(&PlayerName, &PlayerUuid)>,
    events: &mut ResMut<ChatEvents>,
) {
    let (_source_name, message) = match &pk.content {
        Some(TextPacketContent::Chat(ann))
        | Some(TextPacketContent::Announcement(ann))
        | Some(TextPacketContent::Whisper(ann)) => (ann.source_name.clone(), ann.message.clone()),
        Some(TextPacketContent::Raw(json)) | Some(TextPacketContent::System(json)) => {
            (String::new(), json.message.clone())
        }
        _ => (String::new(), String::new()),
    };

    if message.is_empty() {
        trace!("Text packet with empty message");
        return;
    }

    let (sender_name, sender_uuid) = names
        .get(entity)
        .map(|(n, u)| (n.0.clone(), u.0.to_string()))
        .unwrap_or_else(|_| ("Unknown".to_string(), String::new()));

    match pk.type_ {
        TextPacketType::Chat => {
            info!(sender = %sender_name, message = %message, "Chat message");

            events.0.push(ServerEvent::PlayerChat {
                entity,
                player_id: sender_uuid,
                message: message.clone(),
            });

            let broadcast_packet = TextPacket {
                type_: TextPacketType::Chat,
                needs_translation: false,
                category: TextPacketCategory::Authored,
                content: Some(TextPacketContent::Chat(TextPacketContentAnnouncement {
                    source_name: sender_name,
                    message,
                })),
                xuid: String::new(),
                platform_chat_id: String::new(),
                filtered_message: None,
            };

            for (_sid, other_entity) in session_map.iter() {
                if let Ok(session) = sessions.get(other_entity) {
                    let _ = session.send(McpePacket::from(broadcast_packet.clone()));
                }
            }
        }
        TextPacketType::Whisper => {
            debug!(sender = %sender_name, message = %message, "Whisper (not fully implemented)");
        }
        _ => {
            trace!(text_type = ?pk.type_, "Unhandled text type");
        }
    }
}

fn handle_command_request(
    entity: Entity,
    req: &jolyne::valentine::CommandRequestPacket,
    commands: &Res<CommandRegistryResource>,
    tick_counter: &Res<crate::ecs::resources::TickCounter>,
    sessions: &Query<&PlayerSession>,
    runtime_ids: &Query<&RuntimeEntityId>,
    tp_targets: &mut Query<(&mut Position, Option<&mut ChunkLoader>)>,
) {
    if req.internal {
        trace!("Ignoring internal CommandRequest");
        return;
    }

    let command_line = req.command.trim();
    if !command_line.starts_with('/') {
        return;
    }

    let invocation = match parse_command_line(command_line) {
        Ok(inv) => inv,
        Err(CommandParseError::Empty) => return,
        Err(e) => {
            send_command_output(
                entity,
                sessions,
                CommandOutput {
                    messages: vec![],
                    errors: vec![e.to_string()],
                },
            );
            return;
        }
    };

    let name_lower = invocation.name.to_ascii_lowercase();
    if name_lower == "tp" || name_lower == "teleport" {
        handle_teleport_command(
            entity,
            invocation.args.rest(),
            tick_counter,
            sessions,
            runtime_ids,
            tp_targets,
        );
        return;
    }

    let Some(_command) = commands.0.find(&invocation.name) else {
        send_command_output(
            entity,
            sessions,
            CommandOutput {
                messages: vec![],
                errors: vec![format!("Unknown command: {}", invocation.name)],
            },
        );
        return;
    };

    let output = CommandOutput {
        messages: vec![format!("Command '{}' executed", invocation.name)],
        errors: vec![],
    };

    if !output.is_empty() {
        send_command_output(entity, sessions, output);
    }
}

fn handle_teleport_command(
    entity: Entity,
    args: &[String],
    tick_counter: &Res<crate::ecs::resources::TickCounter>,
    sessions: &Query<&PlayerSession>,
    runtime_ids: &Query<&RuntimeEntityId>,
    tp_targets: &mut Query<(&mut Position, Option<&mut ChunkLoader>)>,
) {
    if args.len() < 3 {
        send_command_output(
            entity,
            sessions,
            CommandOutput {
                messages: vec![],
                errors: vec!["Usage: /tp <x> <y> <z>".to_string()],
            },
        );
        return;
    }

    let x: f64 = match args[0].parse() {
        Ok(v) => v,
        Err(_) => {
            send_command_output(
                entity,
                sessions,
                CommandOutput {
                    messages: vec![],
                    errors: vec![format!("Invalid x coordinate: {}", args[0])],
                },
            );
            return;
        }
    };
    let y: f64 = match args[1].parse() {
        Ok(v) => v,
        Err(_) => {
            send_command_output(
                entity,
                sessions,
                CommandOutput {
                    messages: vec![],
                    errors: vec![format!("Invalid y coordinate: {}", args[1])],
                },
            );
            return;
        }
    };
    let z: f64 = match args[2].parse() {
        Ok(v) => v,
        Err(_) => {
            send_command_output(
                entity,
                sessions,
                CommandOutput {
                    messages: vec![],
                    errors: vec![format!("Invalid z coordinate: {}", args[2])],
                },
            );
            return;
        }
    };

    let runtime_id = runtime_ids.get(entity).map(|r| r.0).unwrap_or(0);

    // Update position and chunk loader
    if let Ok((mut position, chunk_loader)) = tp_targets.get_mut(entity) {
        position.0 = DVec3::new(x, y, z);
        if let Some(mut loader) = chunk_loader {
            let new_chunk_x = (x / 16.0).floor() as i32;
            let new_chunk_z = (z / 16.0).floor() as i32;
            loader.move_to(new_chunk_x, new_chunk_z);
        }
    }

    // Send teleport packet
    let packet = MovePlayerPacket {
        runtime_id: runtime_id as i32,
        position: Vec3F {
            x: x as f32,
            y: y as f32,
            z: z as f32,
        },
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        mode: MovePlayerPacketMode::Teleport,
        on_ground: false,
        ridden_runtime_id: 0,
        teleport: Some(MovePlayerPacketTeleport {
            cause: MovePlayerPacketTeleportCause::Command,
            source_entity_type: LegacyEntityType::Player,
        }),
        tick: tick_counter.current as i64,
    };

    if let Ok(session) = sessions.get(entity) {
        let _ = session.send(McpePacket::from(packet));
    }

    send_command_output(
        entity,
        sessions,
        CommandOutput {
            messages: vec![format!("Teleported to {x:.1}, {y:.1}, {z:.1}")],
            errors: vec![],
        },
    );
}

fn send_command_output(entity: Entity, sessions: &Query<&PlayerSession>, output: CommandOutput) {
    let Ok(session) = sessions.get(entity) else {
        return;
    };

    for message in output.messages {
        let _ = session.send(McpePacket::from(system_text(&message)));
    }
    for error in output.errors {
        let _ = session.send(McpePacket::from(system_text(&format!("Error: {error}"))));
    }
}
