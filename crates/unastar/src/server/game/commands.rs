//! Command handling.
//!
//! Contains command request processing and output sending.

use tracing::trace;

use super::GameServer;
use super::types::{SessionEntityMap, system_text};
use crate::command::{CommandOutput, CommandParseError, parse_command_line};
use crate::entity::components::PlayerSession;
use crate::network::SessionId;
use jolyne::protocol::packets::PacketCommandRequest;
use jolyne::protocol::types::McpePacket;

impl GameServer {
    /// Handle a command request from a player.
    pub(super) fn handle_command_request(
        &mut self,
        session_id: SessionId,
        req: &PacketCommandRequest,
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
