//! Native player event regressions through actual authoritative boundaries.

use super::*;
use crate::entity::components::{
    GameMode, Health, Hunger, MainInventory, Player, PlayerInput, PlayerName, PlayerSession,
    PlayerState, PlayerUuid, Position, Rotation, RuntimeEntityId,
};
use crate::server::game::packet_queues::{
    BlockPacketQueue, MovementEvents, MovementInput, MovementPacketQueue,
};
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
use bevy_ecs::system::RunSystemOnce;
use jolyne::valentine::{McpePacket, McpePacketData, PlayerAuthInputPacket, Vec3};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc;

/// A minimal connected mover using the real exclusive movement system.
fn mover() -> (World, Entity, mpsc::Receiver<McpePacket>) {
    let mut world = World::new();
    world.init_resource::<PlayerEventHooks>();
    world.init_resource::<MovementPacketQueue>();
    world.init_resource::<BlockPacketQueue>();
    world.init_resource::<MovementEvents>();
    world.insert_resource(crate::server::game::hunger::ExhaustionValues::vanilla());
    let (tx, rx) = mpsc::channel(128);
    let player = world
        .spawn((
            Player,
            PlayerName("Alice".into()),
            PlayerUuid(uuid::Uuid::from_u128(1)),
            PlayerSession::new(
                1,
                "Alice".into(),
                Some("123".into()),
                Some(uuid::Uuid::from_u128(1).to_string()),
                tx,
            ),
            Position(DVec3::new(1.0, 65.62, 1.0)),
            Rotation::default(),
            PlayerState::default(),
            PlayerInput::default(),
            Health::default(),
            Hunger::default(),
            GameMode::Survival,
            RuntimeEntityId(1),
        ))
        .id();
    (world, player, rx)
}

/// Queue one real authoritative input and run only its processing boundary.
fn move_to(world: &mut World, player: Entity, to: DVec3) {
    let mut packet = PlayerAuthInputPacket {
        position: Vec3 {
            x: to.x as f32,
            y: to.y as f32,
            z: to.z as f32,
        },
        ..Default::default()
    };
    packet.player_rotation.x = 12.0;
    packet.player_rotation.y = 34.0;
    world
        .resource_mut::<MovementPacketQueue>()
        .0
        .push((player, MovementInput::AuthInput(Box::new(packet))));
    world
        .run_system_once(crate::server::game::movement::apply_movement)
        .unwrap();
}

/// Count committed movement, allowing proposal-time comparison with observations.
#[derive(Resource, Default)]
struct Moves(Vec<(DVec3, DVec3)>);

/// Reject every movement proposal while leaving committed observers installed.
struct RejectMove;
impl PlayerEventHandler for RejectMove {
    fn on_move(&self, _world: &mut World, event: &mut PlayerMove) {
        event.cancel();
    }
}

#[test]
fn vetoed_movement_changes_no_state_and_emits_only_authoritative_correction() {
    let (mut world, player, mut rx) = mover();
    world.init_resource::<Moves>();
    world
        .resource_mut::<PlayerEventHooks>()
        .register(RejectMove);
    world.add_observer(|event: On<PlayerMoved>, mut moves: ResMut<Moves>| {
        moves.0.push((event.from, event.to))
    });
    let from = world.get::<Position>(player).unwrap().0;
    let hunger = world.get::<Hunger>(player).unwrap().exhaustion;
    move_to(&mut world, player, from + DVec3::X);
    assert_eq!(world.get::<Position>(player).unwrap().0, from);
    assert_eq!(world.get::<Rotation>(player).unwrap().yaw, 0.0);
    assert_eq!(world.get::<Hunger>(player).unwrap().exhaustion, hunger);
    assert!(world.resource::<Moves>().0.is_empty());
    assert!(world.resource::<MovementEvents>().0.is_empty());
    assert!(matches!(
        rx.try_recv().unwrap().data,
        McpePacketData::MovePlayerPacket(_)
    ));
}

/// Redirect one axis while preserving the other input facts.
struct RedirectMove;
impl PlayerEventHandler for RedirectMove {
    fn on_move(&self, _world: &mut World, event: &mut PlayerMove) {
        event.destination = event.from() + DVec3::Z;
    }
}

#[test]
fn redirects_commit_the_corrected_position_then_notify_once() {
    let (mut world, player, mut rx) = mover();
    world.init_resource::<Moves>();
    world
        .resource_mut::<PlayerEventHooks>()
        .register(RedirectMove);
    world.add_observer(|event: On<PlayerMoved>, mut moves: ResMut<Moves>| {
        moves.0.push((event.from, event.to))
    });
    let from = world.get::<Position>(player).unwrap().0;
    move_to(&mut world, player, from + DVec3::X);
    let to = world.get::<Position>(player).unwrap().0;
    assert!((to - (from + DVec3::Z)).length() < 0.00001);
    assert_eq!(world.resource::<Moves>().0, vec![(from, to)]);
    assert_eq!(world.get::<Rotation>(player).unwrap().yaw, 34.0);
    assert!(matches!(
        rx.try_recv().unwrap().data,
        McpePacketData::MovePlayerPacket(_)
    ));
}

#[test]
fn late_observer_veto_suppresses_committed_movement_and_invalid_redirects_fail_closed() {
    for invalid in [false, true] {
        let (mut world, player, mut rx) = mover();
        world.init_resource::<Moves>();
        world.add_observer(move |mut event: On<PlayerMove>| {
            if invalid {
                event.destination.x = f64::NAN;
            } else {
                event.cancel();
            }
        });
        world.add_observer(|event: On<PlayerMoved>, mut moves: ResMut<Moves>| {
            moves.0.push((event.from, event.to))
        });
        let from = world.get::<Position>(player).unwrap().0;
        move_to(&mut world, player, from + DVec3::X);
        assert_eq!(world.get::<Position>(player).unwrap().0, from);
        assert!(world.resource::<Moves>().0.is_empty());
        assert!(matches!(
            rx.try_recv().unwrap().data,
            McpePacketData::MovePlayerPacket(_)
        ));
    }
}

#[test]
fn callbacks_may_remove_or_replace_the_connection_without_committing_stale_movement() {
    for despawn in [false, true] {
        let (mut world, player, mut rx) = mover();
        world.add_observer(
            move |event: On<PlayerMove>,
                  mut commands: Commands,
                  mut sessions: Query<&mut PlayerSession>| {
                if despawn {
                    commands.entity(event.player()).despawn();
                } else {
                    sessions.get_mut(event.player()).unwrap().session_id = 99;
                }
            },
        );
        let from = world.get::<Position>(player).unwrap().0;
        move_to(&mut world, player, from + DVec3::X);
        assert!(
            world
                .get::<Position>(player)
                .is_none_or(|position| position.0 == from)
        );
        assert!(rx.try_recv().is_err());
    }
}

/// Rewrite presentation text while retaining the actual sending account.
struct RewriteChat;
impl PlayerEventHandler for RewriteChat {
    fn on_chat(&self, _world: &mut World, event: &mut PlayerChat) {
        assert_eq!(event.identity().xuid(), Some("123"));
        event.display_name = "Display".into();
        event.message = format!("Prefix: {}", event.message);
    }
}

#[test]
fn chat_rewrite_cancel_and_connection_revalidation_share_one_broadcast_boundary() {
    let (mut world, player, _rx) = mover();
    world
        .resource_mut::<PlayerEventHooks>()
        .register(RewriteChat);
    let allowed = chat(&mut world, player, "Hello".into()).unwrap();
    assert_eq!(
        (allowed.display_name.as_str(), allowed.message.as_str()),
        ("Display", "Prefix: Hello")
    );
    world.add_observer(|mut event: On<PlayerChat>| event.cancel());
    assert!(chat(&mut world, player, "Hello".into()).is_none());

    let (mut world, player, _) = mover();
    world.add_observer(
        |event: On<PlayerChat>, mut sessions: Query<&mut PlayerSession>| {
            sessions.get_mut(event.player()).unwrap().xuid = Some("456".into())
        },
    );
    assert!(chat(&mut world, player, "Hello".into()).is_none());
}

/// Record registration order and retain departing actors through the public API.
struct Retain(Arc<AtomicUsize>);
impl PlayerEventHandler for Retain {
    fn on_joined(&self, world: &mut World, event: &PlayerJoined) {
        assert!(world.get::<MainInventory>(event.player()).is_some());
        assert_eq!(event.identity().xuid(), Some("123"));
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn on_quit(&self, world: &mut World, event: &mut PlayerQuit) {
        assert!(world.get::<PlayerSession>(event.player()).is_some());
        event.retain_entity();
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// Spawn a connected player using the host's actual initialization path.
fn join(server: &mut GameServer, id: u64, name: &str) -> (Entity, mpsc::Receiver<McpePacket>) {
    let (tx, rx) = mpsc::channel(1 << 16);
    let player = server
        .spawn_player(PlayerSpawnData {
            session_id: id,
            display_name: name.into(),
            xuid: Some("123".into()),
            uuid: Some(uuid::Uuid::from_u128(u128::from(id)).to_string()),
            runtime_id: id as i64,
            position: DVec3::new(1.0, 17.62, 1.0),
            outbound_tx: tx,
            saved_data: None,
            chunk_radius: 2,
        })
        .unwrap();
    (player, rx)
}

#[test]
fn retained_disconnect_detaches_transport_and_inputs_but_keeps_attackable_state() {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let count = Arc::new(AtomicUsize::new(0));
    server
        .ecs
        .world_mut()
        .resource_mut::<PlayerEventHooks>()
        .register(Retain(count.clone()));
    let (player, _) = join(&mut server, 1, "Alice");
    server
        .ecs
        .world_mut()
        .entity_mut(player)
        .insert(crate::command::Operator);
    server
        .ecs
        .world_mut()
        .resource_mut::<crate::entity::projectile::ThrowQueue>()
        .0
        .push(player);
    server.despawn_player(1);
    let world = server.ecs.world();
    assert!(world.get::<DetachedPlayer>(player).is_some());
    assert!(world.get::<Player>(player).is_some());
    assert!(world.get::<Health>(player).is_some());
    assert!(world.get::<MainInventory>(player).is_some());
    assert!(world.get::<PlayerSession>(player).is_none());
    assert!(world.get::<PlayerInput>(player).is_none());
    assert!(world.get::<crate::command::Operator>(player).is_none());
    assert!(
        world
            .get::<crate::world::ecs::ChunkLoader>(player)
            .is_none()
    );
    assert!(
        world
            .resource::<crate::entity::projectile::ThrowQueue>()
            .0
            .is_empty()
    );
    assert!(
        !world
            .resource::<crate::server::game::SessionEntityMap>()
            .contains(1)
    );
    assert_eq!(count.load(Ordering::SeqCst), 2);
    server.despawn_player(1);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn default_disconnect_removes_actor_and_notifies_existing_viewers() {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let (first, _) = join(&mut server, 1, "Alice");
    let (_, mut viewer) = join(&mut server, 2, "Bob");
    while viewer.try_recv().is_ok() {}
    server.despawn_player(1);
    assert!(server.ecs.world().get_entity(first).is_err());
    assert!(
        std::iter::from_fn(|| viewer.try_recv().ok())
            .any(|packet| matches!(packet.data, McpePacketData::RemoveActorPacket(_)))
    );
}

/// Register another handler while an event is being dispatched.
struct RegisterDuringMove(Arc<AtomicUsize>);
impl PlayerEventHandler for RegisterDuringMove {
    fn on_move(&self, world: &mut World, event: &mut PlayerMove) {
        world
            .resource_mut::<PlayerEventHooks>()
            .register(CountMoves(self.0.clone()));
        event.cancel();
    }
}
/// Count later snapshots and inspect cancellation from the first handler.
struct CountMoves(Arc<AtomicUsize>);
impl PlayerEventHandler for CountMoves {
    fn on_move(&self, _world: &mut World, event: &mut PlayerMove) {
        assert!(event.is_cancelled());
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn handler_registry_changes_apply_to_the_next_snapshot_and_keep_vetoes_visible() {
    let (mut world, player, _) = mover();
    let count = Arc::new(AtomicUsize::new(0));
    world
        .resource_mut::<PlayerEventHooks>()
        .register(RegisterDuringMove(count.clone()));
    let from = world.get::<Position>(player).unwrap().0;
    move_to(&mut world, player, from + DVec3::X);
    assert_eq!(count.load(Ordering::SeqCst), 0);
    move_to(&mut world, player, from + DVec3::X);
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn newly_connected_viewers_receive_existing_retained_player_actors() {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    server
        .ecs
        .world_mut()
        .resource_mut::<PlayerEventHooks>()
        .register(Retain(Arc::new(AtomicUsize::new(0))));
    let (_first, _) = join(&mut server, 1, "Alice");
    server.despawn_player(1);
    let (_, mut viewer) = join(&mut server, 2, "Bob");
    while viewer.try_recv().is_ok() {}
    server.tick();
    assert!(std::iter::from_fn(||viewer.try_recv().ok()).any(|packet|matches!(packet.data,McpePacketData::AddPlayerPacket(ref add) if add.target_runtime_id.actor_runtime_id==1)));
}

/// Replace or remove the departing subject during its own callback.
struct ChangeDepartingActor(bool);
impl PlayerEventHandler for ChangeDepartingActor {
    fn on_quit(&self, world: &mut World, event: &mut PlayerQuit) {
        if self.0 {
            world.despawn(event.player());
        } else {
            world
                .get_mut::<PlayerSession>(event.player())
                .unwrap()
                .session_id = 99;
        }
    }
}

#[test]
fn departing_callbacks_can_remove_or_replace_subject_without_stale_cleanup() {
    for remove in [false, true] {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        });
        server
            .ecs
            .world_mut()
            .resource_mut::<PlayerEventHooks>()
            .register(ChangeDepartingActor(remove));
        let (player, _) = join(&mut server, 1, "Alice");
        server.despawn_player(1);
        if remove {
            assert!(server.ecs.world().get_entity(player).is_err());
        } else {
            assert_eq!(
                server
                    .ecs
                    .world()
                    .get::<PlayerSession>(player)
                    .unwrap()
                    .session_id,
                99
            );
        }
    }
}

/// Received form facts, recorded independently of request/menu policy.
#[derive(Resource, Default)]
struct Forms(Vec<(u32, Option<String>, bool, String)>);

/// Record only authenticated bounded form notifications.
struct RecordForms;
impl PlayerEventHandler for RecordForms {
    fn on_form_response(&self, world: &mut World, event: &PlayerFormResponse) {
        world.resource_mut::<Forms>().0.push((
            event.form_id,
            event.data.clone(),
            event.cancelled,
            event.identity().xuid().unwrap_or_default().to_owned(),
        ));
    }
}

/// Encode and route a real form response through the public borrowed packet API.
fn form_packet(
    server: &mut GameServer,
    session: u64,
    id: u32,
    data: Option<&str>,
    cancelled: bool,
) {
    use bytes::BytesMut;
    use jolyne::valentine::{
        BorrowedMcpePacket, EnumsModalFormCancelReason, McpePacketArgs, ModalFormResponsePacket,
    };
    let packet = McpePacket::from(ModalFormResponsePacket {
        form_id: id,
        json_response: data.map(Into::into),
        form_cancel_reason: cancelled.then_some(EnumsModalFormCancelReason::Userclosed),
    });
    let mut bytes = BytesMut::new();
    packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
    let borrowed = BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap();
    server.route_packet(session, McpePacketArgs, borrowed);
}

#[test]
fn real_form_packets_report_response_cancellation_and_raw_bounded_data() {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    server.ecs.world_mut().init_resource::<Forms>();
    server
        .ecs
        .world_mut()
        .resource_mut::<PlayerEventHooks>()
        .register(RecordForms);
    let (_player, mut rx) = join(&mut server, 1, "Alice");
    while rx.try_recv().is_ok() {}
    form_packet(&mut server, 1, 7, Some("17"), false);
    form_packet(&mut server, 1, 8, None, true);
    // The host transports raw optional text; JSON interpretation is the consumer's.
    form_packet(&mut server, 1, 9, Some("not-json"), false);
    let boundary = "x".repeat(65536);
    form_packet(&mut server, 1, 10, Some(&boundary), false);
    let oversized = "x".repeat(65537);
    form_packet(&mut server, 1, 11, Some(&oversized), false);
    let forms = &server.ecs.world().resource::<Forms>().0;
    assert_eq!(
        &forms[..3],
        &[
            (7, Some("17".into()), false, "123".into()),
            (8, None, true, "123".into()),
            (9, Some("not-json".into()), false, "123".into())
        ]
    );
    assert_eq!(forms.len(), 4);
    assert_eq!(forms[3].1.as_ref().unwrap().len(), 65536);
    assert!(rx.try_recv().is_err());
}

#[test]
fn form_packets_with_no_handler_have_no_default_gameplay_or_network_effects() {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let (player, mut rx) = join(&mut server, 1, "Alice");
    while rx.try_recv().is_ok() {}
    let position = server.ecs.world().get::<Position>(player).unwrap().0;
    let health = server.ecs.world().get::<Health>(player).unwrap().current;
    let items = server
        .ecs
        .world()
        .get::<MainInventory>(player)
        .unwrap()
        .0
        .slots()
        .to_vec();
    form_packet(&mut server, 1, 17, Some("[true,3]"), false);
    form_packet(&mut server, 1, 18, None, true);
    assert_eq!(
        server.ecs.world().get::<Position>(player).unwrap().0,
        position
    );
    assert_eq!(
        server.ecs.world().get::<Health>(player).unwrap().current,
        health
    );
    assert_eq!(
        server
            .ecs
            .world()
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .slots(),
        items
    );
    assert!(rx.try_recv().is_err());
}

/// Invalidate the connection before later form consumers receive the response.
struct ReplaceFormConnection(bool);
impl PlayerEventHandler for ReplaceFormConnection {
    fn on_form_response(&self, world: &mut World, event: &PlayerFormResponse) {
        if self.0 {
            world.despawn(event.player());
        } else {
            world
                .get_mut::<PlayerSession>(event.player())
                .unwrap()
                .session_id = 99;
        }
    }
}

#[test]
fn form_routing_rejects_stale_sessions_and_stops_after_callback_identity_change() {
    for callback in [None, Some(false), Some(true)] {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        });
        server.ecs.world_mut().init_resource::<Forms>();
        if let Some(remove) = callback {
            server
                .ecs
                .world_mut()
                .resource_mut::<PlayerEventHooks>()
                .register(ReplaceFormConnection(remove));
        }
        server
            .ecs
            .world_mut()
            .resource_mut::<PlayerEventHooks>()
            .register(RecordForms);
        let (player, _rx) = join(&mut server, 1, "Alice");
        if callback.is_none() {
            server
                .ecs
                .world_mut()
                .get_mut::<PlayerSession>(player)
                .unwrap()
                .session_id = 99;
        }
        form_packet(&mut server, 1, 7, Some("0"), false);
        assert!(server.ecs.world().resource::<Forms>().0.is_empty());
    }
}

#[test]
fn deferred_observer_removal_prevents_movement_commit_and_notification() {
    let (mut world, player, mut rx) = mover();
    world.init_resource::<Moves>();
    world.add_observer(|event: On<PlayerMove>, mut commands: Commands| {
        commands.entity(event.player()).despawn();
    });
    world.add_observer(|event: On<PlayerMoved>, mut moves: ResMut<Moves>| {
        moves.0.push((event.from, event.to));
    });
    let from = world.get::<Position>(player).unwrap().0;
    move_to(&mut world, player, from + DVec3::X);
    assert!(world.get_entity(player).is_err());
    assert!(world.resource::<Moves>().0.is_empty());
    assert!(world.resource::<MovementEvents>().0.is_empty());
    assert!(rx.try_recv().is_err());
}

/// Retain one inventory group while asynchronous initialization is outstanding.
#[derive(Resource)]
struct HeldInitialInventory(MainInventory);
struct DeferAlice;
impl PlayerEventHandler for DeferAlice {
    fn on_joined(&self, world: &mut World, event: &PlayerJoined) {
        if event.identity().name() == "Alice" {
            assert!(defer_initial_state(world, event.player()));
            let inventory = world
                .entity_mut(event.player())
                .take::<MainInventory>()
                .unwrap();
            world.insert_resource(HeldInitialInventory(inventory));
        }
    }
    fn on_quit(&self, _world: &mut World, event: &mut PlayerQuit) {
        event.retain_entity();
    }
}

/// Build a real host that delays one initialized actor's first publication.
fn deferred_server() -> GameServer {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..Default::default()
    });
    server
        .ecs
        .world_mut()
        .resource_mut::<PlayerEventHooks>()
        .register(DeferAlice);
    server
}

#[test]
fn deferred_snapshot_restoration_publishes_final_state_only_after_release() {
    use crate::server::game::player_state::{self, SnapshotEffect};
    let mut server = deferred_server();
    let (alice, mut own) = join(&mut server, 1, "Alice");
    let inventory = server
        .ecs
        .world_mut()
        .remove_resource::<HeldInitialInventory>()
        .unwrap()
        .0;
    server.ecs.world_mut().entity_mut(alice).insert(inventory);
    let mut snapshot = player_state::capture(server.ecs.world(), alice).unwrap();
    snapshot.data.position[0] += 3.0;
    snapshot.velocity = [0.1, 0.2, 0.3];
    snapshot.data.health = 7.0;
    snapshot.data.food = 14;
    snapshot.effects = vec![SnapshotEffect {
        id: 1,
        level: 1,
        duration_ticks: 200,
        ambient: true,
        show_particles: false,
        infinite: false,
    }];
    player_state::restore(server.ecs.world_mut(), alice, &snapshot).unwrap();
    assert!(
        own.try_recv().is_err(),
        "restoration must withhold its direct state packets"
    );
    server.tick();
    let early = std::iter::from_fn(|| own.try_recv().ok()).collect::<Vec<_>>();
    assert!(
        !early.iter().any(|packet| matches!(
            packet.data,
            McpePacketData::InventoryContentPacket(_)
                | McpePacketData::UpdateAttributesPacket(_)
                | McpePacketData::MobEffectPacket(_)
                | McpePacketData::MovePlayerPacket(_)
                | McpePacketData::SetActorMotionPacket(_)
                | McpePacketData::ActorEventPacket(_)
                | McpePacketData::UpdateAbilitiesPacket(_)
                | McpePacketData::UpdatePlayerGameTypePacket(_)
        )),
        "tick systems must not publish pending state"
    );
    assert!(release_initial_state(server.ecs.world_mut(), alice));
    server.tick();
    let published = std::iter::from_fn(|| own.try_recv().ok()).collect::<Vec<_>>();
    assert!(published.iter().any(|packet| matches!(&packet.data, McpePacketData::MobEffectPacket(effect) if effect.effect_id==1 && effect.effect_amplifier==1 && !effect.show_particles && effect.ambient)));
    assert!(published.iter().any(|packet| matches!(&packet.data, McpePacketData::MovePlayerPacket(movement) if movement.position.x==snapshot.data.position[0] as f32)));
    assert!(published.iter().any(|packet| matches!(&packet.data, McpePacketData::SetActorMotionPacket(motion) if motion.motion.x==0.1)));
    server.tick();
    assert!(
        !std::iter::from_fn(|| own.try_recv().ok())
            .any(|packet| matches!(packet.data, McpePacketData::MobEffectPacket(_))),
        "released effects must not be sent twice"
    );
}

#[test]
fn deferred_initial_state_waits_for_complete_inventory_then_publishes_once() {
    let mut server = deferred_server();
    let (_, mut viewer) = join(&mut server, 2, "Bob");
    server.tick();
    while viewer.try_recv().is_ok() {}
    let (alice, mut own) = join(&mut server, 1, "Alice");
    assert!(initial_state_pending(server.ecs.world(), alice));
    assert!(own.try_recv().is_err());
    crate::server::game::send_inventory_contents(server.ecs.world(), alice);
    assert!(own.try_recv().is_err());
    server.tick();
    assert!(!std::iter::from_fn(||viewer.try_recv().ok()).any(|packet|matches!(packet.data,McpePacketData::AddPlayerPacket(ref add) if add.target_runtime_id.actor_runtime_id==1)));
    assert!(!release_initial_state(server.ecs.world_mut(), alice));
    let inventory = server
        .ecs
        .world_mut()
        .remove_resource::<HeldInitialInventory>()
        .unwrap()
        .0;
    server.ecs.world_mut().entity_mut(alice).insert(inventory);
    assert!(release_initial_state(server.ecs.world_mut(), alice));
    assert!(release_initial_state(server.ecs.world_mut(), alice));
    server.tick();
    assert!(!initial_state_pending(server.ecs.world(), alice));
    let initial: Vec<_> = std::iter::from_fn(|| own.try_recv().ok()).collect();
    assert!(
        initial
            .iter()
            .any(|packet| matches!(packet.data, McpePacketData::InventoryContentPacket(_)))
    );
    server.tick();
    assert_eq!(std::iter::from_fn(||viewer.try_recv().ok()).filter(|packet|matches!(packet.data,McpePacketData::AddPlayerPacket(ref add) if add.target_runtime_id.actor_runtime_id==1)).count(),1);
    assert!(
        !std::iter::from_fn(|| own.try_recv().ok())
            .any(|packet| matches!(packet.data, McpePacketData::InventoryContentPacket(_)))
    );
    assert!(!defer_initial_state(server.ecs.world_mut(), alice));
    assert!(!release_initial_state(server.ecs.world_mut(), alice));
}

#[test]
fn deferred_initial_state_cannot_release_to_a_replaced_connection() {
    let mut server = deferred_server();
    let (alice, mut own) = join(&mut server, 1, "Alice");
    let inventory = server
        .ecs
        .world_mut()
        .remove_resource::<HeldInitialInventory>()
        .unwrap()
        .0;
    server.ecs.world_mut().entity_mut(alice).insert(inventory);
    server
        .ecs
        .world_mut()
        .get_mut::<PlayerSession>(alice)
        .unwrap()
        .session_id = 9;
    assert!(!release_initial_state(server.ecs.world_mut(), alice));
    server.tick();
    let residual: Vec<_> = std::iter::from_fn(|| own.try_recv().ok()).collect();
    assert!(
        !residual.iter().any(|packet| matches!(
            packet.data,
            McpePacketData::SetCommandsEnabledPacket(_) | McpePacketData::InventoryContentPacket(_)
        )),
        "unexpected initial state: {:?}",
        residual
            .iter()
            .map(|packet| packet.data.packet_id())
            .collect::<Vec<_>>()
    );
    assert!(initial_state_pending(server.ecs.world(), alice));
}

#[test]
fn retained_deferred_actor_can_publish_to_viewers_without_a_departed_transport() {
    let mut server = deferred_server();
    let (_, mut viewer) = join(&mut server, 2, "Bob");
    server.tick();
    while viewer.try_recv().is_ok() {}
    let (alice, mut own) = join(&mut server, 1, "Alice");
    server.despawn_player(1);
    let inventory = server
        .ecs
        .world_mut()
        .remove_resource::<HeldInitialInventory>()
        .unwrap()
        .0;
    server.ecs.world_mut().entity_mut(alice).insert(inventory);
    assert!(release_initial_state(server.ecs.world_mut(), alice));
    server.tick();
    server.tick();
    assert!(!initial_state_pending(server.ecs.world(), alice));
    assert!(own.try_recv().is_err());
    assert_eq!(std::iter::from_fn(||viewer.try_recv().ok()).filter(|packet|matches!(packet.data,McpePacketData::AddPlayerPacket(ref add) if add.target_runtime_id.actor_runtime_id==1)).count(),1);
}

#[test]
fn deferred_initial_state_discards_real_form_packets_until_publication() {
    let mut server = deferred_server();
    server.ecs.world_mut().init_resource::<Forms>();
    server
        .ecs
        .world_mut()
        .resource_mut::<PlayerEventHooks>()
        .register(RecordForms);
    let (alice, _) = join(&mut server, 1, "Alice");
    form_packet(&mut server, 1, 7, Some("0"), false);
    assert!(server.ecs.world().resource::<Forms>().0.is_empty());
    let inventory = server
        .ecs
        .world_mut()
        .remove_resource::<HeldInitialInventory>()
        .unwrap()
        .0;
    server.ecs.world_mut().entity_mut(alice).insert(inventory);
    assert!(release_initial_state(server.ecs.world_mut(), alice));
    server.tick();
    form_packet(&mut server, 1, 7, Some("0"), false);
    assert_eq!(server.ecs.world().resource::<Forms>().0.len(), 1);
}
