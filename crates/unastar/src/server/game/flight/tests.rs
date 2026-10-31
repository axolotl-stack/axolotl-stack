//! Real native flight permission, durability, snapshot and input regressions.

use super::*;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig, player_events, player_state};
use crate::world::WorldConfig;
use jolyne::valentine::{McpePacket, McpePacketData};
use tokio::sync::mpsc;

/// Use ordinary player creation and clear its initial packets.
fn fixture() -> (GameServer, Entity, mpsc::Receiver<McpePacket>) {
    fixture_with(false)
}

/// Defer through the real join callback, before the host publishes initial state.
fn fixture_with(defer: bool) -> (GameServer, Entity, mpsc::Receiver<McpePacket>) {
    let mut server = GameServer::with_config(ServerConfig {
        world: WorldConfig::void_spawn_platform(1),
        ..Default::default()
    });
    if defer {
        struct Defer;
        impl super::super::player_events::PlayerEventHandler for Defer {
            fn on_joined(
                &self,
                world: &mut World,
                event: &super::super::player_events::PlayerJoined,
            ) {
                assert!(player_events::defer_initial_state(world, event.player()));
            }
        }
        server
            .ecs
            .world_mut()
            .resource_mut::<super::super::player_events::PlayerEventHooks>()
            .register(Defer);
    }
    let (tx, mut rx) = mpsc::channel(128);
    let player = server
        .spawn_player(PlayerSpawnData {
            session_id: 7,
            display_name: "actor".into(),
            xuid: Some("123".into()),
            uuid: Some(uuid::Uuid::from_u128(7).to_string()),
            runtime_id: 77,
            position: glam::DVec3::new(4.5, 17.62, 4.5),
            saved_data: None,
            outbound_tx: tx,
            chunk_radius: 0,
        })
        .unwrap();
    while rx.try_recv().is_ok() {}
    (server, player, rx)
}

/// Read the actual queued wire layer rather than a mode approximation.
fn abilities(rx: &mut mpsc::Receiver<McpePacket>) -> Vec<u32> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|packet| match packet.data {
            McpePacketData::UpdateAbilitiesPacket(packet) => {
                Some(packet.data.layers[0].ability_values)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn flight_grant_changes_only_permission_and_revoke_stops_active_flight() {
    let (mut server, player, mut rx) = fixture();
    let world = server.ecs.world_mut();
    assert!(set_override(world, player, Some(true)));
    assert!(allowed(world, player));
    assert_eq!(world.get::<GameMode>(player), Some(&GameMode::Survival));
    let packet = abilities(&mut rx).pop().unwrap();
    assert_ne!(packet & MAY_FLY, 0);
    assert_eq!(
        packet & ((1 << 8) | (1 << 11)),
        0,
        "permission grants neither invulnerability nor instant build"
    );
    world.get_mut::<PlayerState>(player).unwrap().flying = true;
    assert!(set_override(world, player, None));
    assert!(!allowed(world, player));
    assert!(!world.get::<PlayerState>(player).unwrap().flying);
    assert_eq!(abilities(&mut rx).pop().unwrap() & MAY_FLY, 0);
    assert_eq!(
        crate::command::exec::base_game_mode(world, player),
        Some(GameMode::Survival)
    );
}

#[test]
fn temporary_modes_and_raw_snapshots_preserve_the_independent_permission() {
    let (mut server, player, mut rx) = fixture();
    let world = server.ecs.world_mut();
    set_override(world, player, Some(true));
    crate::command::exec::set_mode_override(world, player, Some(GameMode::Adventure));
    assert_ne!(abilities(&mut rx).last().unwrap() & MAY_FLY, 0);
    let snapshot = player_state::capture(world, player).unwrap();
    assert_eq!(snapshot.flight_override, Some(true));
    set_override(world, player, None);
    player_state::restore(world, player, &snapshot).unwrap();
    assert!(allowed(world, player));
    assert_eq!(
        crate::command::exec::mode_override(world, player),
        Some(GameMode::Adventure)
    );
    assert_eq!(
        crate::command::exec::base_game_mode(world, player),
        Some(GameMode::Survival)
    );
    let mut legacy = snapshot;
    legacy.flight_override = None;
    player_state::restore(world, player, &legacy).unwrap();
    assert!(!allowed(world, player));
}

#[test]
fn deferred_initial_state_withholds_flight_packets_until_the_native_release() {
    let (mut server, player, mut rx) = fixture_with(true);
    let world = server.ecs.world_mut();
    assert!(player_events::initial_state_pending(world, player));
    set_override(world, player, Some(true));
    assert!(abilities(&mut rx).is_empty());
    assert!(player_events::release_initial_state(world, player));
    server.tick();
    assert!(abilities(&mut rx).iter().any(|bits| bits & MAY_FLY != 0));
}

#[test]
fn a_full_channel_retries_the_permission_on_the_same_original_connection() {
    let (mut server, player, mut rx) = fixture();
    let world = server.ecs.world_mut();
    let sender = world
        .get::<PlayerSession>(player)
        .unwrap()
        .outbound_tx
        .clone();
    for _ in 0..128 {
        sender
            .try_send(crate::server::game::types::system_text("queued").into())
            .unwrap();
    }
    assert!(set_override(world, player, Some(true)));
    assert_eq!(world.resource::<Pending>().0.len(), 1);
    while rx.try_recv().is_ok() {}
    sync(world);
    assert_eq!(abilities(&mut rx).len(), 1);
    assert!(world.resource::<Pending>().0.is_empty());
}

#[test]
fn authoritative_input_requires_real_permission_before_clearing_fall_state() {
    use crate::entity::components::Position;
    use crate::server::game::packet_queues::{MovementInput, MovementPacketQueue};
    use bevy_ecs::system::RunSystemOnce;
    use jolyne::valentine::{
        EnumsPlayerAuthInputPacketPayloadInputData as Flag, PlayerAuthInputPacket, Vec3,
    };
    let (mut server, player, _rx) = fixture();
    let world = server.ecs.world_mut();
    for allowed in [false, true, false] {
        assert!(set_override(world, player, allowed.then_some(true)));
        world
            .entity_mut(player)
            .insert(crate::entity::player_life::environment::PlayerFall::default());
        let position = world.get::<Position>(player).unwrap().0;
        let mut input = PlayerAuthInputPacket {
            position: Vec3 {
                x: position.x as f32,
                y: position.y as f32 - 1.0,
                z: position.z as f32,
            },
            ..Default::default()
        };
        input.input_data.push(Flag::Startflying);
        world
            .resource_mut::<MovementPacketQueue>()
            .0
            .push((player, MovementInput::AuthInput(Box::new(input))));
        world
            .run_system_once(super::super::movement::apply_movement)
            .unwrap();
        assert_eq!(world.get::<PlayerState>(player).unwrap().flying, allowed);
        assert_eq!(
            world
                .get::<crate::entity::player_life::environment::PlayerFall>(player)
                .unwrap()
                .fall
                == Default::default(),
            allowed
        );
    }
}

/// Join another ordinary connection at the same visible position.
fn viewer(server: &mut GameServer, session: u64) -> (Entity, mpsc::Receiver<McpePacket>) {
    let (tx, rx) = mpsc::channel(128);
    let player = server
        .spawn_player(PlayerSpawnData {
            session_id: session,
            display_name: format!("viewer{session}"),
            xuid: Some(session.to_string()),
            uuid: Some(uuid::Uuid::from_u128(session as u128).to_string()),
            runtime_id: session as i64 + 100,
            position: glam::DVec3::new(4.5, 17.62, 4.5),
            saved_data: None,
            outbound_tx: tx,
            chunk_radius: 0,
        })
        .unwrap();
    (player, rx)
}

#[test]
fn existing_and_late_viewers_receive_actual_permission_and_hidden_viewers_do_not() {
    use crate::entity::components::ActorAudience;
    let (mut server, player, _own) = fixture();
    let (peer, mut rx) = viewer(&mut server, 8);
    server.tick();
    while rx.try_recv().is_ok() {}
    assert!(set_override(server.ecs.world_mut(), player, Some(true)));
    assert!(abilities(&mut rx).iter().any(|bits| bits & MAY_FLY != 0));
    let (_, mut late) = viewer(&mut server, 9);
    server.tick();
    let actual = std::iter::from_fn(|| late.try_recv().ok())
        .find_map(|packet| match packet.data {
            McpePacketData::AddPlayerPacket(packet)
                if packet.target_runtime_id.actor_runtime_id == 77 =>
            {
                Some(packet.abilities_data)
            }
            _ => None,
        })
        .expect("late viewer receives the actor snapshot");
    assert_ne!(actual.layers[0].ability_values & MAY_FLY, 0);
    server
        .ecs
        .world_mut()
        .entity_mut(player)
        .insert(ActorAudience::only([player]));
    server.tick();
    while rx.try_recv().is_ok() {}
    assert!(set_override(server.ecs.world_mut(), player, None));
    assert!(
        abilities(&mut rx).is_empty(),
        "hidden peer must not receive the actor's permission changes"
    );
    assert!(server.ecs.world().get::<PlayerSession>(peer).is_some());
}

#[tokio::test]
async fn a_real_world_migration_preserves_flight_permission_without_changing_durable_mode() {
    use crate::server::worlds::{DimensionRouter, request_transfer_checked};
    let (server, player, mut rx) = fixture();
    let mut router = DimensionRouter::new(server);
    router
        .add(GameServer::with_config(ServerConfig {
            world: WorldConfig {
                dimension: 1,
                ..WorldConfig::void_spawn_platform(1)
            },
            ..Default::default()
        }))
        .unwrap();
    assert!(set_override(
        router.world_mut(0).unwrap().ecs.world_mut(),
        player,
        Some(true)
    ));
    while rx.try_recv().is_ok() {}
    request_transfer_checked(
        router.world_mut(0).unwrap().ecs.world_mut(),
        player,
        1,
        glam::DVec3::new(4.5, 16.0, 4.5),
        0,
        "flight migration".into(),
        |_, _, _| true,
    )
    .unwrap();
    router.tick().await.unwrap();
    let (dimension, target) = router.locate_session(7).unwrap();
    assert_eq!(dimension, 1);
    let world = router.world(1).unwrap().ecs.world();
    assert!(allowed(world, target));
    assert_eq!(override_value(world, target), Some(true));
    assert_eq!(world.get::<GameMode>(target), Some(&GameMode::Survival));
    let snapshot = player_state::capture(world, target).unwrap();
    assert_eq!(snapshot.data.game_mode, 0);
    assert_eq!(snapshot.flight_override, Some(true));
    assert_eq!(snapshot.data.dimension, 1);
    assert!(abilities(&mut rx).iter().any(|bits| bits & MAY_FLY != 0));
    assert!(
        router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get_entity(player)
            .is_err()
    );
}
