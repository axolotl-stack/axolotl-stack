//! Command handling.
//!
//! Contains command request processing and output sending.

use glam::DVec3;
use tracing::trace;

use super::GameServer;
use super::types::{SessionEntityMap, system_text};
use crate::command::{CommandOutput, CommandParseError, parse_command_line};
use crate::entity::components::transform::Position;
use crate::entity::components::{PlayerSession, RuntimeEntityId};
use crate::network::SessionId;
use crate::world::ecs::ChunkLoader;
use jolyne::valentine::types::{LegacyEntityType, Vec3F};
use jolyne::valentine::{
    CommandRequestPacket, McpePacket, MovePlayerPacket, MovePlayerPacketMode,
    MovePlayerPacketTeleport, MovePlayerPacketTeleportCause,
};

impl GameServer {
    /// Handle a command request from a player.
    pub(super) fn handle_command_request(
        &mut self,
        session_id: SessionId,
        req: &CommandRequestPacket,
    ) {
        if req.internal {
            trace!(session_id, "Ignoring internal CommandRequest");
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
                self.send_command_output(
                    session_id,
                    CommandOutput {
                        messages: vec![],
                        errors: vec![e.to_string()],
                    },
                );
                return;
            }
        };

        // Handle teleport command directly (needs server access)
        let name_lower = invocation.name.to_ascii_lowercase();
        if name_lower == "tp" || name_lower == "teleport" {
            self.handle_teleport_command(session_id, invocation.args.rest());
            return;
        }

        let Some(_command) = self.commands.find(&invocation.name) else {
            self.send_command_output(
                session_id,
                CommandOutput {
                    messages: vec![],
                    errors: vec![format!("Unknown command: {}", invocation.name)],
                },
            );
            return;
        };

        // Execute command - need to pass self, which is complex
        // For now, just acknowledge
        let output = CommandOutput {
            messages: vec![format!("Command '{}' executed", invocation.name)],
            errors: vec![],
        };

        if !output.is_empty() {
            self.send_command_output(session_id, output);
        }
    }

    /// Handle teleport command: /tp <x> <y> <z>
    fn handle_teleport_command(&mut self, session_id: SessionId, args: &[String]) {
        // Parse coordinates
        if args.len() < 3 {
            self.send_command_output(
                session_id,
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
                self.send_command_output(
                    session_id,
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
                self.send_command_output(
                    session_id,
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
                self.send_command_output(
                    session_id,
                    CommandOutput {
                        messages: vec![],
                        errors: vec![format!("Invalid z coordinate: {}", args[2])],
                    },
                );
                return;
            }
        };

        // Get player entity and runtime ID
        let (entity, runtime_id) = {
            let session_map = match self.ecs.world().get_resource::<SessionEntityMap>() {
                Some(map) => map,
                None => return,
            };
            let entity = match session_map.get(session_id) {
                Some(e) => e,
                None => return,
            };
            let runtime_id = match self.ecs.world().get::<RuntimeEntityId>(entity) {
                Some(id) => id.0,
                None => return,
            };
            (entity, runtime_id)
        };

        // Update position in ECS
        {
            let world = self.ecs.world_mut();
            if let Some(mut position) = world.get_mut::<Position>(entity) {
                position.0 = DVec3::new(x, y, z);
            }

            // Update chunk loader to trigger chunk loading at new position
            if let Some(mut chunk_loader) = world.get_mut::<ChunkLoader>(entity) {
                let new_chunk_x = (x / 16.0).floor() as i32;
                let new_chunk_z = (z / 16.0).floor() as i32;
                chunk_loader.move_to(new_chunk_x, new_chunk_z);
            }
        }

        // Build teleport packet
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
            tick: self.current_tick as i64,
        };

        // Send teleport packet to client (get session after mutable borrow is done)
        if let Some(session) = self.ecs.world().get::<PlayerSession>(entity) {
            let _ = session.send(McpePacket::from(packet));
        }

        self.send_command_output(
            session_id,
            CommandOutput {
                messages: vec![format!("Teleported to {:.1}, {:.1}, {:.1}", x, y, z)],
                errors: vec![],
            },
        );
    }

    /// Send command output messages to a player.
    pub(super) fn send_command_output(&self, session_id: SessionId, output: CommandOutput) {
        let session_map = match self.ecs.world().get_resource::<SessionEntityMap>() {
            Some(map) => map,
            None => return,
        };
        let entity = match session_map.get(session_id) {
            Some(e) => e,
            None => return,
        };
        let session = match self.ecs.world().get::<PlayerSession>(entity) {
            Some(s) => s,
            None => return,
        };

        for message in output.messages {
            let _ = session.send(McpePacket::from(system_text(&message)));
        }
        for error in output.errors {
            let _ = session.send(McpePacket::from(system_text(&format!("Error: {error}"))));
        }
    }
}
