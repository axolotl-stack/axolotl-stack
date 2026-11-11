//! Chat and command domain systems.
//!
//! `apply_chat_and_commands` turns Text packets into chat events and queues
//! CommandRequest packets; `run_commands` (exclusive, executors need the whole
//! world) runs queued player requests and server-console lines through
//! [`crate::command::execute`] and answers each player request with a
//! `CommandOutputPacket` whose origin echoes the request's. Requests that
//! arrived ahead of their player's inputs run earlier, in
//! `run_early_commands` ([`CommandOrder`]).

use bevy_ecs::prelude::*;
use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender};
use tracing::{debug, error, info, trace, warn};

use super::packet_queues::{ChatAction, ChatEvents, ChatPacketQueue};
use crate::command::{CommandSource, execute};
use crate::ecs::events::ServerEvent;
use crate::entity::components::{PlayerName, PlayerSession, PlayerUuid};
use jolyne::valentine::types::{
    EnumsTextPacketType, TextPacketBody, TextPacketPayloadAuthorAndMessageMessageType,
};
use jolyne::valentine::{CommandRequestPacket, McpePacket, TextPacket};

/// Player command requests waiting for `run_commands`.
#[derive(Resource, Default)]
pub struct PendingCommands(pub Vec<(Entity, CommandRequestPacket)>);

/// Keeps a player's command requests in place among that player's inputs.
///
/// BDS handles a client's packets in the order they arrive: a `/setblock`
/// sent just before an input that starts breaking the block is in place for
/// that input. Here inputs go to per-domain queues, and `run_commands` runs
/// after the systems that apply them. So routing records which players sent
/// an input this tick, and a request from a player with none yet runs in
/// `run_early_commands`, before every input system. A request after an input
/// still runs after all of the tick's inputs.
#[derive(Resource, Default)]
pub struct CommandOrder {
    early: Vec<(Entity, CommandRequestPacket)>,
    sent_input: HashSet<Entity>,
}

impl CommandOrder {
    /// `entity` sent a packet that a system before `run_commands` applies.
    pub(super) fn input(&mut self, entity: Entity) {
        self.sent_input.insert(entity);
    }

    /// Keeps `request` for `run_early_commands` when `entity` sent no input
    /// yet this tick, else hands it back for `run_commands`.
    pub(super) fn request(
        &mut self,
        entity: Entity,
        request: CommandRequestPacket,
    ) -> Option<CommandRequestPacket> {
        if self.sent_input.contains(&entity) {
            return Some(request);
        }
        self.early.push((entity, request));
        None
    }
}

/// Lines typed on the server console (stdin), run with server permission.
#[derive(Resource)]
pub struct ConsoleCommands {
    receiver: Mutex<Receiver<String>>,
}

impl ConsoleCommands {
    /// A console queue fed by `sender`.
    pub fn channel() -> (Sender<String>, Self) {
        let (sender, receiver) = std::sync::mpsc::channel();
        (
            sender,
            Self {
                receiver: Mutex::new(receiver),
            },
        )
    }

    /// Reads stdin lines on a background thread for the life of the process.
    pub fn stdin() -> Self {
        let (sender, console) = Self::channel();
        let spawned = std::thread::Builder::new()
            .name("console".into())
            .spawn(move || {
                let stdin = std::io::stdin();
                let mut line = String::new();
                loop {
                    line.clear();
                    match stdin.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let command = line.trim();
                            if !command.is_empty() && sender.send(command.to_owned()).is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        if let Err(error) = spawned {
            warn!(%error, "Server console unavailable");
        }
        console
    }

    fn drain(&self) -> Vec<String> {
        self.receiver
            .lock()
            .map(|receiver| receiver.try_iter().collect())
            .unwrap_or_default()
    }
}

/// ECS system: drain the chat/command packet queue.
///
/// Runs in `PacketApplySet`; chat becomes `PlayerChat` events, command
/// requests wait in [`PendingCommands`] for the exclusive `run_commands`.
pub fn apply_chat_and_commands(
    mut queue: ResMut<ChatPacketQueue>,
    mut events: ResMut<ChatEvents>,
    mut pending: ResMut<PendingCommands>,
    names: Query<(&PlayerName, &PlayerUuid)>,
) {
    let _span = tracing::info_span!("apply_chat_and_commands", count = queue.0.len()).entered();
    for (entity, _session_id, action) in queue.0.drain(..) {
        match action {
            ChatAction::Text(pk) => handle_text(entity, &pk, &names, &mut events),
            ChatAction::Command(request) => pending.0.push((entity, request)),
        }
    }
}

/// Exclusive ECS system, before every input system of `PacketApplySet`:
/// runs the requests that arrived ahead of their player's inputs
/// ([`CommandOrder`]). The routing of this tick is over, so it also starts
/// the next tick's record.
pub fn run_early_commands(world: &mut World) {
    let Some(mut order) = world.get_resource_mut::<CommandOrder>() else {
        return;
    };
    order.sent_input.clear();
    let requests = std::mem::take(&mut order.early);
    for (entity, request) in requests {
        run_player_request(world, entity, request);
    }
}

/// Exclusive ECS system: runs queued player commands, then console lines.
pub fn run_commands(world: &mut World) {
    let requests = std::mem::take(&mut world.resource_mut::<PendingCommands>().0);
    for (entity, request) in requests {
        run_player_request(world, entity, request);
    }
    let lines = world
        .get_resource::<ConsoleCommands>()
        .map(ConsoleCommands::drain)
        .unwrap_or_default();
    for line in lines {
        let output = execute(world, CommandSource::server(), &line);
        print_console_notes(world);
        // Like BDS: one log record per answer, its messages one per line,
        // logged as an error when the command failed.
        if output.messages.is_empty() {
            continue;
        }
        let text = output.to_text().join("\n");
        if output.is_success() {
            info!(target: "console", "{text}");
        } else {
            error!(target: "console", "{text}");
        }
    }
    // `/schedule`d functions whose time came or whose area loaded.
    crate::command::exec::run_scheduled_functions(world);
}

/// Runs one player request and answers it with a `CommandOutputPacket`.
fn run_player_request(world: &mut World, entity: Entity, request: CommandRequestPacket) {
    if request.is_internal {
        trace!("Ignoring internal CommandRequest");
        return;
    }
    debug!(command = %request.command, "Command");
    let Some(output) = crate::command::run_as_player(world, entity, &request.command) else {
        return;
    };
    print_console_notes(world);
    let Some(packet) = player_answer(world, &output, request.origin) else {
        return;
    };
    if let Some(session) = world.get::<PlayerSession>(entity) {
        let _ = session.send(McpePacket::from(packet));
    }
}

/// The `CommandOutput` a player gets for its command, if any. BDS answers
/// `/say` and `/me` with their chat line only. While `sendCommandFeedback`
/// is off it sends nothing for a success and a failure with output type
/// `silent` (measured: `tools/bds-sweep/level_data.py`'s player).
pub(crate) fn player_answer(
    world: &World,
    output: &crate::command::CommandOutput,
    origin: jolyne::valentine::types::CommandOriginDatajson,
) -> Option<jolyne::valentine::CommandOutputPacket> {
    if output.messages.is_empty() {
        return None;
    }
    let feedback = world
        .get_resource::<crate::world::rules::GameRules>()
        .is_none_or(crate::world::rules::GameRules::sends_command_feedback);
    let mut packet = output.to_packet(origin);
    if !feedback {
        if output.is_success() {
            return None;
        }
        packet.output.output_type = "silent".into();
    }
    Some(packet)
}

/// Prints the extra console lines the last command left
/// ([`crate::command::ConsoleNote`]).
fn print_console_notes(world: &mut World) {
    let Some(mut notes) = world.get_resource_mut::<crate::command::ConsoleNotes>() else {
        return;
    };
    for note in std::mem::take(&mut notes.0) {
        if note.error {
            error!(target: "console", "{}", note.text);
        } else {
            info!(target: "console", "{}", note.text);
        }
    }
}

fn handle_text(
    entity: Entity,
    pk: &TextPacket,
    names: &Query<(&PlayerName, &PlayerUuid)>,
    events: &mut ResMut<ChatEvents>,
) {
    let message = match &pk.body {
        TextPacketBody::AuthorAndMessage(author) => &*author.message,
        TextPacketBody::MessageOnly(message)
            if matches!(
                message.message_type,
                EnumsTextPacketType::Raw | EnumsTextPacketType::Systemmessage
            ) =>
        {
            &*message.message
        }
        _ => "",
    };

    if message.is_empty() {
        trace!("Text packet with empty message");
        return;
    }

    let (sender_name, sender_uuid) = names
        .get(entity)
        .map(|(n, u)| (n.0.clone(), u.0.to_string()))
        .unwrap_or_else(|_| ("Unknown".to_string(), String::new()));

    match &pk.body {
        TextPacketBody::AuthorAndMessage(author)
            if author.message_type == TextPacketPayloadAuthorAndMessageMessageType::Chat =>
        {
            info!(sender = %sender_name, message = %message, "Chat message");

            events.0.push(ServerEvent::PlayerChat {
                entity,
                player_id: sender_uuid,
                message: message.to_string(),
            });
        }
        TextPacketBody::AuthorAndMessage(author)
            if author.message_type == TextPacketPayloadAuthorAndMessageMessageType::Whisper =>
        {
            debug!(sender = %sender_name, message = %message, "Whisper (not fully implemented)");
        }
        _ => {
            trace!(text_body = ?pk.body, "Unhandled text type");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Operator;
    use crate::entity::components::RuntimeEntityId;
    use crate::entity::components::transform::Position;
    use bevy_ecs::system::RunSystemOnce;
    use glam::DVec3;
    use jolyne::valentine::McpePacketData;
    use jolyne::valentine::types::{CommandOriginDatajson, TextPacketPayloadAuthorAndMessage};
    use tokio::sync::mpsc;

    /// A game server with one player standing (eye height) at x 3.5,
    /// feet y 64, z 3.5 over a loaded empty chunk.
    fn test_server(
        operator: bool,
    ) -> (super::super::GameServer, Entity, mpsc::Receiver<McpePacket>) {
        let mut server = super::super::GameServer::with_config(super::super::ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        });
        let world = server.ecs.world_mut();
        world.insert_resource(crate::entity::EntityRandom::seeded(5));
        let chunk = world
            .spawn(crate::world::ecs::ChunkData::new(
                crate::world::chunk::Chunk::new(0, 0),
            ))
            .id();
        world
            .resource_mut::<crate::world::ecs::ChunkManager>()
            .insert(crate::world::ecs::ChunkPosition::new(0, 0), chunk);
        let (tx, rx) = mpsc::channel(256);
        let player = world
            .spawn((
                crate::entity::components::Player,
                PlayerSession::new(1, "Alice".into(), None, None, tx),
                PlayerName("Alice".into()),
                Position(DVec3::new(
                    3.5,
                    64.0 + crate::entity::PLAYER_EYE_HEIGHT,
                    3.5,
                )),
                RuntimeEntityId(1),
            ))
            .id();
        if operator {
            world.entity_mut(player).insert(Operator);
        }
        (server, player, rx)
    }

    fn command(server: &mut super::super::GameServer, player: Entity, line: &str) {
        server
            .ecs
            .world_mut()
            .resource_mut::<ChatPacketQueue>()
            .0
            .push((
                player,
                1,
                ChatAction::Command(CommandRequestPacket {
                    command: line.into(),
                    origin: CommandOriginDatajson {
                        type_: "player".into(),
                        request_id: "req-7".into(),
                        ..Default::default()
                    },
                    is_internal: false,
                    version: "latest".into(),
                }),
            ));
        server.tick();
    }

    /// (success count, [(key, params)]) of one CommandOutput.
    type Received = (u32, Vec<(String, Vec<String>)>);

    /// Every CommandOutput the player received.
    fn outputs(rx: &mut mpsc::Receiver<McpePacket>) -> Vec<Received> {
        let mut out = Vec::new();
        while let Ok(packet) = rx.try_recv() {
            if let McpePacketData::CommandOutputPacket(packet) = packet.data {
                assert_eq!(&*packet.origin_data.request_id, "req-7");
                assert_eq!(&*packet.output.output_type, "alloutput");
                out.push((
                    packet.output.success_count,
                    packet
                        .output
                        .output_messages
                        .iter()
                        .map(|m| {
                            (
                                m.message_id.to_string(),
                                m.parameters.iter().map(|p| p.to_string()).collect(),
                            )
                        })
                        .collect(),
                ));
            }
        }
        out
    }

    fn mob_positions(server: &mut super::super::GameServer) -> Vec<DVec3> {
        let world = server.ecs.world_mut();
        world
            .query_filtered::<&Position, With<crate::entity::mob::MobKind>>()
            .iter(world)
            .map(|position| position.0)
            .collect()
    }

    #[test]
    fn members_get_the_bds_permission_error_for_operator_commands() {
        let (mut server, player, mut rx) = test_server(false);
        command(&mut server, player, "/summon minecraft:cow 6 70 9");
        assert!(mob_positions(&mut server).is_empty());
        assert_eq!(
            outputs(&mut rx),
            [(
                0,
                vec![(
                    "commands.generic.error.permissions".to_owned(),
                    vec!["summon".to_owned()]
                )]
            )]
        );
    }

    #[test]
    fn operators_summon_at_absolute_relative_and_own_positions() {
        let (mut server, player, mut rx) = test_server(true);
        command(&mut server, player, "/summon minecraft:cow 6 70 9");
        assert_eq!(mob_positions(&mut server), [DVec3::new(6.5, 70.0, 9.5)]);
        assert_eq!(
            outputs(&mut rx),
            [(1, vec![("commands.summon.success".to_owned(), vec![])])]
        );

        let (mut server, player, _rx) = test_server(true);
        command(&mut server, player, "/summon cow ~2 ~ ~-1.5");
        assert_eq!(mob_positions(&mut server), [DVec3::new(5.5, 64.0, 2.0)]);

        let (mut server, player, _rx) = test_server(true);
        command(&mut server, player, "/summon cow");
        assert_eq!(mob_positions(&mut server), [DVec3::new(3.5, 64.0, 3.5)]);
    }

    #[test]
    fn unknown_actor_types_are_syntax_errors_and_unsupported_ones_say_why() {
        let (mut server, player, mut rx) = test_server(true);
        command(&mut server, player, "/summon minecraft:not_a_mob");
        assert!(mob_positions(&mut server).is_empty());
        assert_eq!(
            outputs(&mut rx),
            [(
                0,
                vec![(
                    "commands.generic.syntax".to_owned(),
                    vec![
                        "/summon ".to_owned(),
                        "minecraft:not_a_mob".to_owned(),
                        String::new()
                    ]
                )]
            )]
        );

        // A known actor the mob runtime cannot run yet reports why.
        let (mut server, player, mut rx) = test_server(true);
        let reason = server
            .ecs
            .world()
            .resource::<crate::entity::mob::MobRegistry>()
            .unsupported_reason("minecraft:ender_dragon")
            .expect("the ender dragon is not spawnable yet")
            .to_owned();
        command(&mut server, player, "/summon ender_dragon");
        assert!(mob_positions(&mut server).is_empty());
        let outputs = outputs(&mut rx);
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].0, 0);
        assert!(outputs[0].1[0].0.contains(&reason), "{outputs:?}");
    }

    #[test]
    fn console_lines_run_with_server_permission() {
        let (mut server, _player, _rx) = test_server(false);
        let (sender, console) = ConsoleCommands::channel();
        server.ecs.world_mut().insert_resource(console);
        sender.send("summon cow 1 70 1".into()).unwrap();
        server.tick();
        assert_eq!(mob_positions(&mut server), [DVec3::new(1.5, 70.0, 1.5)]);
    }

    #[test]
    fn chat_only_queues_cancellable_event_without_delivery() {
        let mut world = World::new();
        world.insert_resource(ChatPacketQueue::default());
        world.insert_resource(ChatEvents::default());
        world.insert_resource(PendingCommands::default());
        let (tx, mut rx) = mpsc::channel(16);
        let entity = world
            .spawn((
                PlayerSession::new(1, "Alice".into(), None, None, tx),
                PlayerName("Alice".into()),
                PlayerUuid(uuid::Uuid::nil()),
            ))
            .id();
        let packet = TextPacket {
            localize: false,
            body: TextPacketBody::AuthorAndMessage(TextPacketPayloadAuthorAndMessage {
                message_type: TextPacketPayloadAuthorAndMessageMessageType::Chat,
                player_name: "spoofed".into(),
                message: "hello".into(),
            }),
            senders_xuid: "".into(),
            platform_id: "".into(),
            filtered_message: None,
        };
        world
            .resource_mut::<ChatPacketQueue>()
            .0
            .push((entity, 1, ChatAction::Text(packet)));
        world.run_system_once(apply_chat_and_commands).unwrap();
        assert!(rx.try_recv().is_err());
        assert!(matches!(&world.resource::<ChatEvents>().0[..],
            [ServerEvent::PlayerChat { entity: sender, message, .. }]
                if *sender == entity && message == "hello"));
    }
}
