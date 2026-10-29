//! Chat and command domain system.
//!
//! Processes Text (chat broadcast) and CommandRequest packets, including
//! the /tp command which writes Position and /summon, which spawns its mob
//! from the command buffer (spawning needs exclusive world access), as do
//! the world and player commands (`world_commands`).

use bevy_ecs::prelude::*;
use glam::DVec3;
use tracing::{debug, info, trace};

use super::packet_queues::{ChatAction, ChatEvents, ChatPacketQueue};
use super::types::{CommandRegistryResource, system_text};
use crate::command::{
    CommandContext, CommandOutput, CommandParseError, Operator, SummonRequest, parse_command_line,
};
use crate::ecs::events::ServerEvent;
use crate::entity::components::transform::Position;
use crate::entity::components::{PlayerName, PlayerSession, PlayerUuid, RuntimeEntityId};
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::world::ecs::ChunkLoader;
use jolyne::valentine::types::{
    ActorRuntimeId, EnumsPlayerPositionModeComponentPositionMode, EnumsTextPacketType,
    PlayerInputTick, TextPacketBody, TextPacketPayloadAuthorAndMessageMessageType, Vec2, Vec3,
};
use jolyne::valentine::{McpePacket, MovePlayerPacket, MovePlayerTeleportData, TextPacket};

/// ECS system: drain chat/command packet queue and process.
///
/// Runs in `PacketApplySet`. Serializes with movement (both write Position
/// due to /tp), but parallel with inventory and chunks.
#[allow(clippy::too_many_arguments)]
pub fn apply_chat_and_commands(
    mut queue: ResMut<ChatPacketQueue>,
    mut events: ResMut<ChatEvents>,
    commands: Res<CommandRegistryResource>,
    tick_counter: Res<crate::ecs::resources::TickCounter>,
    sessions: Query<&PlayerSession>,
    names: Query<(&PlayerName, &PlayerUuid)>,
    runtime_ids: Query<&RuntimeEntityId>,
    operators: Query<(), With<Operator>>,
    mut tp_targets: Query<(&mut Position, Option<&mut ChunkLoader>)>,
    mut deferred: Commands,
) {
    let _span = tracing::info_span!("apply_chat_and_commands", count = queue.0.len()).entered();
    for (entity, _session_id, action) in queue.0.drain(..) {
        match action {
            ChatAction::Text(pk) => {
                handle_text(entity, &pk, &names, &mut events);
            }
            ChatAction::Command(req) => {
                handle_command_request(
                    entity,
                    &req,
                    &commands,
                    &tick_counter,
                    &sessions,
                    &runtime_ids,
                    &operators,
                    &mut tp_targets,
                    &mut deferred,
                );
            }
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

#[allow(clippy::too_many_arguments)]
fn handle_command_request(
    entity: Entity,
    req: &jolyne::valentine::CommandRequestPacket,
    commands: &Res<CommandRegistryResource>,
    tick_counter: &Res<crate::ecs::resources::TickCounter>,
    sessions: &Query<&PlayerSession>,
    runtime_ids: &Query<&RuntimeEntityId>,
    operators: &Query<(), With<Operator>>,
    tp_targets: &mut Query<(&mut Position, Option<&mut ChunkLoader>)>,
    deferred: &mut Commands,
) {
    if req.is_internal {
        trace!("Ignoring internal CommandRequest");
        return;
    }

    let command_line = req.command.trim();
    if !command_line.starts_with('/') {
        return;
    }

    let mut invocation = match parse_command_line(command_line) {
        Ok(inv) => inv,
        Err(CommandParseError::Empty) => return,
        Err(e) => {
            send_command_output(
                entity,
                sessions,
                CommandOutput {
                    messages: vec![],
                    errors: vec![e.to_string()],
                    ..Default::default()
                },
            );
            return;
        }
    };

    let Ok(session) = sessions.get(entity) else {
        return;
    };
    let context = CommandContext::new(session.session_id, operators.contains(entity));
    let mut output = commands.0.execute(&context, &mut invocation);
    if let Some(target) = output.teleport {
        handle_teleport_command(
            entity,
            target,
            tick_counter,
            sessions,
            runtime_ids,
            tp_targets,
        );
    }
    if let Some(summon) = output.summon.take() {
        deferred.queue(move |world: &mut World| apply_summon(world, entity, &summon));
    }
    if let Some(request) = output.world.take() {
        deferred.queue(move |world: &mut World| {
            super::world_commands::apply_world_request(world, entity, request);
        });
    }
    if !output.is_empty() {
        send_command_output(entity, sessions, output);
    }
}

fn handle_teleport_command(
    entity: Entity,
    target: DVec3,
    tick_counter: &Res<crate::ecs::resources::TickCounter>,
    sessions: &Query<&PlayerSession>,
    runtime_ids: &Query<&RuntimeEntityId>,
    tp_targets: &mut Query<(&mut Position, Option<&mut ChunkLoader>)>,
) {
    let DVec3 { x, y, z } = target;

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
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        position: Vec3 {
            x: x as f32,
            y: y as f32,
            z: z as f32,
        },
        rotation: Vec2 { x: 0.0, y: 0.0 },
        y_head_rotation: 0.0,
        position_mode: EnumsPlayerPositionModeComponentPositionMode::Teleport,
        on_ground: false,
        riding_runtime_id: ActorRuntimeId {
            actor_runtime_id: 0,
        },
        teleport_data: Some(MovePlayerTeleportData {
            teleportation_cause: 3,
            source_actor_type: 63,
        }),
        tick: PlayerInputTick {
            inputtick: tick_counter.current,
        },
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
            ..Default::default()
        },
    );
}

/// Spawn a `/summon`ed mob (relative coordinates resolve against the
/// executor's feet) and reply with the outcome.
fn apply_summon(world: &mut World, executor: Entity, request: &SummonRequest) {
    let Some(eye) = world.get::<Position>(executor).map(|p| p.0) else {
        return;
    };
    let feet = eye - DVec3::new(0.0, crate::entity::PLAYER_EYE_HEIGHT, 0.0);
    let position = request.resolve(feet);
    let mut output = CommandOutput::default();
    match spawn_mob(
        world,
        &request.identifier,
        position,
        0.0,
        SpawnCause::Spawned,
    ) {
        Ok(_) => output.message("Object successfully summoned"),
        Err(error) => output.error(format!("Unable to summon object: {error}")),
    }
    if let Some(session) = world.get::<PlayerSession>(executor) {
        send_output(session, output);
    }
}

fn send_command_output(entity: Entity, sessions: &Query<&PlayerSession>, output: CommandOutput) {
    if let Ok(session) = sessions.get(entity) {
        send_output(session, output);
    }
}

pub(super) fn send_output(session: &PlayerSession, output: CommandOutput) {
    for message in output.messages {
        let _ = session.send(McpePacket::from(system_text(&message)));
    }
    for error in output.errors {
        let _ = session.send(McpePacket::from(system_text(&format!("Error: {error}"))));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, CommandArgs, CommandContext, CommandRegistry, Operator};
    use bevy_ecs::system::RunSystemOnce;
    use jolyne::valentine::McpePacketData;
    use jolyne::valentine::types::TextPacketPayloadAuthorAndMessage;
    use tokio::sync::mpsc;

    fn fixture() -> (World, Entity, mpsc::Receiver<McpePacket>) {
        let mut world = World::new();
        world.insert_resource(ChatPacketQueue::default());
        world.insert_resource(ChatEvents::default());
        world.insert_resource(CommandRegistryResource(CommandRegistry::with_defaults()));
        world.insert_resource(crate::ecs::resources::TickCounter::default());
        let (tx, rx) = mpsc::channel(16);
        let entity = world
            .spawn((
                PlayerSession::new(1, "Alice".into(), None, None, tx),
                PlayerName("Alice".into()),
                PlayerUuid(uuid::Uuid::nil()),
                Position(DVec3::ZERO),
                RuntimeEntityId(1),
            ))
            .id();
        let mut sessions = super::super::types::SessionEntityMap::default();
        sessions.insert(1, entity);
        world.insert_resource(sessions);
        (world, entity, rx)
    }

    fn command(world: &mut World, entity: Entity, line: &str) {
        let mut request = jolyne::valentine::CommandRequestPacket {
            command: line.into(),
            ..Default::default()
        };
        // A client claiming console origin must not gain operator authority.
        request.origin.type_ = "console".into();
        world
            .resource_mut::<ChatPacketQueue>()
            .0
            .push((entity, 1, ChatAction::Command(request)));
        world.run_system_once(apply_chat_and_commands).unwrap();
    }

    #[test]
    fn teleport_rejects_nonoperators_without_moving() {
        for name in ["tp", "teleport", "TP"] {
            let (mut world, entity, mut rx) = fixture();
            command(&mut world, entity, &format!("/{name} 32 70 -16"));
            assert_eq!(world.get::<Position>(entity).unwrap().0, DVec3::ZERO);
            let McpePacketData::TextPacket(packet) = rx.try_recv().unwrap().data else {
                panic!("expected permission error, not teleport");
            };
            assert!(format!("{packet:?}").contains("permission"));
            assert!(rx.try_recv().is_err());
        }
    }

    #[test]
    fn teleport_allows_server_granted_operators() {
        for name in ["tp", "teleport"] {
            let (mut world, entity, mut rx) = fixture();
            world.entity_mut(entity).insert(Operator);
            command(&mut world, entity, &format!("/{name} 32 70 -16"));
            assert_eq!(
                world.get::<Position>(entity).unwrap().0,
                DVec3::new(32.0, 70.0, -16.0)
            );
            assert!(matches!(
                rx.try_recv().unwrap().data,
                McpePacketData::MovePlayerPacket(_)
            ));
            assert!(matches!(
                rx.try_recv().unwrap().data,
                McpePacketData::TextPacket(_)
            ));
            assert!(rx.try_recv().is_err());
        }
    }

    struct RestrictedCommand;

    impl Command for RestrictedCommand {
        fn name(&self) -> &'static str {
            "restricted"
        }
        fn requires_operator(&self) -> bool {
            true
        }
        fn execute(&self, _ctx: &CommandContext, _args: &mut CommandArgs) -> CommandOutput {
            let mut output = CommandOutput::default();
            output.message("actual execution");
            output
        }
    }

    #[test]
    fn registry_permissions_apply_to_custom_commands_and_execute_authorized_handlers() {
        let (mut world, entity, mut rx) = fixture();
        world
            .resource_mut::<CommandRegistryResource>()
            .0
            .register(RestrictedCommand);
        command(&mut world, entity, "/restricted");
        assert!(format!("{:?}", rx.try_recv().unwrap()).contains("permission"));
        world.entity_mut(entity).insert(Operator);
        command(&mut world, entity, "/restricted");
        assert!(format!("{:?}", rx.try_recv().unwrap()).contains("actual execution"));
    }

    /// A game server with one player standing (eye height) at x 3.5,
    /// feet y 64, z 3.5 over a loaded empty chunk.
    fn summon_server(
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
                PlayerSession::new(1, "Alice".into(), None, None, tx),
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

    fn summon(server: &mut super::super::GameServer, player: Entity, line: &str) {
        server
            .ecs
            .world_mut()
            .resource_mut::<ChatPacketQueue>()
            .0
            .push((
                player,
                1,
                ChatAction::Command(jolyne::valentine::CommandRequestPacket {
                    command: line.into(),
                    ..Default::default()
                }),
            ));
        server.tick();
    }

    fn mob_positions(server: &mut super::super::GameServer) -> Vec<DVec3> {
        let world = server.ecs.world_mut();
        world
            .query_filtered::<&Position, With<crate::entity::mob::MobKind>>()
            .iter(world)
            .map(|position| position.0)
            .collect()
    }

    fn texts(rx: &mut mpsc::Receiver<McpePacket>) -> Vec<String> {
        let mut texts = Vec::new();
        while let Ok(packet) = rx.try_recv() {
            if let McpePacketData::TextPacket(TextPacket {
                body: TextPacketBody::AuthorAndMessage(body),
                ..
            }) = packet.data
            {
                texts.push(body.message.to_string());
            }
        }
        texts
    }

    #[test]
    fn operators_summon_cows_at_absolute_relative_and_own_positions() {
        let (mut server, player, mut rx) = summon_server(true);
        summon(&mut server, player, "/summon minecraft:cow 6 70 9");
        assert_eq!(mob_positions(&mut server), [DVec3::new(6.5, 70.0, 9.5)]);
        assert!(
            texts(&mut rx)
                .iter()
                .any(|t| t.contains("Object successfully summoned"))
        );

        let (mut server, player, _rx) = summon_server(true);
        summon(&mut server, player, "/summon cow ~2 ~ ~-1.5");
        assert_eq!(mob_positions(&mut server), [DVec3::new(5.5, 64.0, 2.0)]);

        let (mut server, player, _rx) = summon_server(true);
        summon(&mut server, player, "/summon cow");
        assert_eq!(mob_positions(&mut server), [DVec3::new(3.5, 64.0, 3.5)]);
    }

    #[test]
    fn summon_refuses_nonoperators_and_reports_unknown_actors() {
        let (mut server, player, mut rx) = summon_server(false);
        summon(&mut server, player, "/summon minecraft:cow 6 70 9");
        assert!(mob_positions(&mut server).is_empty());
        assert!(texts(&mut rx).iter().any(|t| t.contains("permission")));

        let (mut server, player, mut rx) = summon_server(true);
        summon(&mut server, player, "/summon minecraft:not_a_mob");
        assert!(mob_positions(&mut server).is_empty());
        assert!(texts(&mut rx).iter().any(|t| t.contains("Unable to summon object")
            && t.contains("minecraft:not_a_mob")));

        // A known actor the mob runtime cannot run yet reports why.
        let (mut server, player, mut rx) = summon_server(true);
        let reason = server
            .ecs
            .world()
            .resource::<crate::entity::mob::MobRegistry>()
            .unsupported_reason("minecraft:ender_dragon")
            .expect("the ender dragon is not spawnable yet")
            .to_owned();
        summon(&mut server, player, "/summon ender_dragon");
        assert!(mob_positions(&mut server).is_empty());
        assert!(
            texts(&mut rx)
                .iter()
                .any(|t| t.contains("not supported yet") && t.contains(&reason))
        );
    }

    #[test]
    fn chat_only_queues_cancellable_event_without_delivery() {
        let (mut world, entity, mut rx) = fixture();
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
