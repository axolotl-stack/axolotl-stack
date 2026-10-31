//! Raw-state, deferred-publication and restart regressions for cold conditions.

use super::*;
use crate::command::exec::{base_game_mode, mode_override, set_mode_override};
use crate::server::game::player_events::{
    self, PlayerEventHandler, PlayerEventHooks, PlayerJoined,
};
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
use jolyne::valentine::types::{DataItemEntry, DataItemEntryPayload};
use jolyne::valentine::{McpePacket, McpePacketData, actor_data_i_ds};
use tokio::sync::mpsc;

struct Defer;
impl PlayerEventHandler for Defer {
    fn on_joined(&self, world: &mut World, event: &PlayerJoined) {
        assert!(player_events::defer_initial_state(world, event.player()));
    }
}

/// Join a complete native player, optionally holding its initial state before
/// the ordinary join packet batch is allowed to publish.
fn player(deferred: bool) -> (GameServer, Entity, mpsc::Receiver<McpePacket>) {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..Default::default()
    });
    if deferred {
        server
            .ecs
            .world_mut()
            .resource_mut::<PlayerEventHooks>()
            .register(Defer);
    }
    let (tx, mut rx) = mpsc::channel(1 << 16);
    let player = server
        .spawn_player(PlayerSpawnData {
            session_id: 7,
            display_name: "Snapshot".into(),
            xuid: Some("123".into()),
            uuid: Some(uuid::Uuid::from_u128(7).to_string()),
            runtime_id: 7,
            position: DVec3::new(1.5, 17.62, 1.5),
            outbound_tx: tx,
            saved_data: None,
            chunk_radius: 0,
        })
        .unwrap();
    while rx.try_recv().is_ok() {}
    (server, player, rx)
}

/// Isolate the packets emitted by the operation being asserted.
fn drain(rx: &mut mpsc::Receiver<McpePacket>) -> Vec<McpePacketData> {
    std::iter::from_fn(|| rx.try_recv().ok().map(|packet| packet.data)).collect()
}

/// Read explicit cold renderer data rather than inferring it from a display.
fn strength(data: &[DataItemEntry]) -> Option<f32> {
    data.iter().find_map(|entry| {
        if entry.id != actor_data_i_ds::FREEZING_EFFECT_STRENGTH {
            return None;
        }
        match &entry.payload {
            DataItemEntryPayload::DataItemFloatPayload(value) => Some(value.value),
            _ => None,
        }
    })
}

#[test]
fn raw_cold_and_age_round_trip_with_the_effective_mode_and_durable_base() {
    let (mut server, player, mut rx) = player(false);
    let base = *server.ecs.world().get::<GameMode>(player).unwrap();
    server
        .ecs
        .world_mut()
        .entity_mut(player)
        .insert((FreezingState::from_strength(1.0).unwrap(), Age(12345)));
    assert!(set_mode_override(
        server.ecs.world_mut(),
        player,
        Some(GameMode::Survival)
    ));
    let before = capture(server.ecs.world(), player).unwrap();
    assert_eq!(
        (before.data.cold_exposure, before.data.age_ticks),
        (1.0, 12345)
    );
    let snapshot: PlayerSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&before).unwrap()).unwrap();
    server
        .ecs
        .world_mut()
        .entity_mut(player)
        .insert((FreezingState::default(), Age(0)));
    drain(&mut rx);
    restore(server.ecs.world_mut(), player, &snapshot).unwrap();
    assert_eq!(capture(server.ecs.world(), player).unwrap(), snapshot);
    assert_eq!(
        mode_override(server.ecs.world(), player),
        Some(GameMode::Survival)
    );
    assert_eq!(base_game_mode(server.ecs.world(), player), Some(base));
    let packets = drain(&mut rx);
    assert!(packets.iter().any(
        |packet| matches!(packet,McpePacketData::SetActorDataPacket(data)
        if strength(&data.actor_data.data)==Some(1.0))
    ));
    assert!(packets.iter().any(|packet| matches!(packet,McpePacketData::UpdateAttributesPacket(data)
        if data.attribute_list.iter().any(|attribute|&*attribute.name=="minecraft:movement"&&attribute.current_value==0.05))));
    assert_eq!(
        server.ecs.world().get::<RuntimeEntityId>(player).unwrap().0,
        7
    );
    assert_eq!(
        server
            .ecs
            .world()
            .get::<PlayerSession>(player)
            .unwrap()
            .session_id,
        7
    );
}

#[test]
fn malformed_cold_is_rejected_before_any_life_inventory_mode_age_or_packet_change() {
    let (mut server, player, mut rx) = player(false);
    server
        .ecs
        .world_mut()
        .entity_mut(player)
        .insert((FreezingState::from_strength(0.75).unwrap(), Age(987)));
    assert!(set_mode_override(
        server.ecs.world_mut(),
        player,
        Some(GameMode::Adventure)
    ));
    let initial = capture(server.ecs.world(), player).unwrap();
    drain(&mut rx);
    for exposure in [f32::NAN, f32::INFINITY, -0.01, 1.01] {
        let mut bad = initial.clone();
        bad.data.cold_exposure = exposure;
        assert!(restore(server.ecs.world_mut(), player, &bad).is_err());
        assert_eq!(capture(server.ecs.world(), player).unwrap(), initial);
        assert!(drain(&mut rx).is_empty());
    }
}

#[test]
fn pending_restoration_withholds_cold_until_the_actual_initial_batch() {
    let (mut source, actor, _) = player(false);
    source
        .ecs
        .world_mut()
        .entity_mut(actor)
        .insert((FreezingState::from_strength(1.0).unwrap(), Age(77)));
    assert!(set_mode_override(
        source.ecs.world_mut(),
        actor,
        Some(GameMode::Survival)
    ));
    let snapshot = capture(source.ecs.world(), actor).unwrap();
    let (mut server, player, mut rx) = player(true);
    assert!(player_events::initial_state_pending(
        server.ecs.world(),
        player
    ));
    restore(server.ecs.world_mut(), player, &snapshot).unwrap();
    crate::entity::freezing::refresh(server.ecs.world_mut(), player);
    crate::server::broadcast::player_views::sync_player_views(server.ecs.world_mut());
    assert!(
        drain(&mut rx).is_empty(),
        "restoring and rendering pending state must publish nothing"
    );
    assert_eq!(capture(server.ecs.world(), player).unwrap(), snapshot);
    assert!(player_events::release_initial_state(
        server.ecs.world_mut(),
        player
    ));
    server.tick();
    assert!(!player_events::initial_state_pending(
        server.ecs.world(),
        player
    ));
    let cold = *server.ecs.world().get::<FreezingState>(player).unwrap();
    assert!(cold.strength() > 0.9);
    assert_eq!(server.ecs.world().get::<Age>(player).unwrap().0, 78);
    let packets = drain(&mut rx);
    assert!(packets.iter().any(
        |packet| matches!(packet,McpePacketData::SetActorDataPacket(data)
        if strength(&data.actor_data.data)==Some(cold.strength()))
    ));
    assert!(packets.iter().any(|packet| matches!(packet,McpePacketData::UpdateAttributesPacket(data)
        if data.attribute_list.iter().any(|attribute|&*attribute.name=="minecraft:movement"&&attribute.current_value==cold.movement(0.1)))));
    assert_eq!(
        packets
            .iter()
            .filter(|packet| matches!(packet, McpePacketData::GameRulesChangedPacket(_)))
            .count(),
        1
    );
    server.tick();
    assert!(
        !drain(&mut rx)
            .iter()
            .any(|packet| matches!(packet, McpePacketData::GameRulesChangedPacket(_)))
    );
}

#[test]
fn disk_round_trip_recreates_a_raw_dead_actor_without_warming_or_resetting_age() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("snapshot.json");
    {
        let (mut server, player, _) = player(false);
        server.ecs.world_mut().entity_mut(player).insert((
            FreezingState::from_strength(0.875).unwrap(),
            Age(4567),
            Dead,
        ));
        server
            .ecs
            .world_mut()
            .get_mut::<Health>(player)
            .unwrap()
            .current = 0.0;
        let raw = capture(server.ecs.world(), player).unwrap();
        assert_eq!(
            raw.data.cold_exposure, 0.875,
            "raw capture does not normalize death"
        );
        std::fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
    }
    let raw: PlayerSnapshot = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..Default::default()
    });
    let actor = spawn_detached(server.ecs.world_mut(), "Snapshot", &raw).unwrap();
    assert_eq!(capture(server.ecs.world(), actor).unwrap(), raw);
    assert!(server.ecs.world().get::<Dead>(actor).is_some());
    assert!(server.ecs.world().get::<PlayerSession>(actor).is_none());
    assert_eq!(
        server
            .ecs
            .world()
            .get::<FreezingState>(actor)
            .unwrap()
            .strength(),
        0.875
    );
    assert_eq!(server.ecs.world().get::<Age>(actor).unwrap().0, 4567);
}
