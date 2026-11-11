//! The actor send range against BDS 1.26.51
//! (`testdata/bds-1.26.51-send-range.json`: `tools/bds-sweep/sweep.py range
//! --observer` at view / simulation distance 10 / 10, 6 / 4 and 16 / 12,
//! distilled by `range_fixture.py`).
//!
//! Each walk is replayed with the markers this server can spawn (the sweep
//! player, a cow, an item, an arrow, an experience orb) where BDS had them,
//! and an observer moved along the same steps. Every add and remove the
//! observer gets for them must come at the same step as on BDS, an add in
//! the tick of the step and a remove one tick later.

use std::sync::OnceLock;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::{McpePacket, McpePacketData};
use serde_json::Value;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::entity::components::{
    ActorViewers, GameMode, Player, PlayerName, PlayerSession, PlayerUuid, Position, Rotation,
    RuntimeEntityId, RuntimeId,
};
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::entity::{EntityRandom, PLAYER_EYE_HEIGHT};
use crate::server::game::{GameServer, ServerConfig};

fn fixture() -> &'static Value {
    static FIXTURE: OnceLock<Value> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        serde_json::from_str(include_str!("testdata/bds-1.26.51-send-range.json")).unwrap()
    })
}

fn vec3(value: &Value) -> DVec3 {
    DVec3::new(
        value[0].as_f64().unwrap(),
        value[1].as_f64().unwrap(),
        value[2].as_f64().unwrap(),
    )
}

fn player(world: &mut World, id: i64, feet: DVec3) -> (Entity, mpsc::Receiver<McpePacket>) {
    let (tx, rx) = mpsc::channel::<McpePacket>(1 << 16);
    let entity = world
        .spawn((
            Player,
            Position(feet + DVec3::Y * PLAYER_EYE_HEIGHT),
            Rotation::default(),
            GameMode::Creative,
            RuntimeEntityId(id),
            PlayerUuid(Uuid::from_u128(id as u128)),
            PlayerName(format!("player{id}")),
            PlayerSession::new(id as u64, format!("player{id}"), None, None, tx),
            ActorViewers::default(),
        ))
        .id();
    (entity, rx)
}

/// The runtime id a replayed marker is sent with, if this server spawns
/// its kind.
fn spawn_marker(world: &mut World, kind: &str, at: DVec3, sweep_player: i64) -> Option<i64> {
    let entity = match kind {
        "minecraft:player" => return Some(sweep_player),
        "minecraft:cow" | "minecraft:arrow" => {
            spawn_mob(world, kind, at, 0.0, SpawnCause::Spawned).unwrap()
        }
        "minecraft:item" => crate::entity::spawn_item_drop(
            world,
            at,
            crate::item::test_support::stack("minecraft:stick", 1),
            None,
        )
        .unwrap(),
        "minecraft:xp_orb" => crate::entity::xp_orb::spawn_orb(world, at, 1),
        _ => return None,
    };
    Some(world.get::<RuntimeId>(entity).unwrap().0)
}

/// The add (`true`) or remove packets an observer got, by runtime id.
fn adds_and_removes(rx: &mut mpsc::Receiver<McpePacket>) -> Vec<(i64, bool)> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|packet| match packet.data {
            McpePacketData::AddActorPacket(add) => {
                Some((add.target_runtime_id.actor_runtime_id as i64, true))
            }
            McpePacketData::AddItemActorPacket(add) => {
                Some((add.target_runtime_id.actor_runtime_id as i64, true))
            }
            McpePacketData::AddPlayerPacket(add) => {
                Some((add.target_runtime_id.actor_runtime_id as i64, true))
            }
            McpePacketData::RemoveActorPacket(remove) => {
                Some((remove.target_actor_id.actor_unique_id, false))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn actors_are_added_and_removed_where_bds_adds_and_removes_them() {
    let mut replayed = 0;
    for run in fixture()["runs"].as_array().unwrap() {
        for item in run["items"].as_array().unwrap() {
            let mut server = GameServer::with_config(ServerConfig {
                world: crate::world::WorldConfig::void_spawn_platform(1),
                ..ServerConfig::default()
            });
            let world = server.ecs.world_mut();
            world.insert_resource(EntityRandom::seeded(3));
            let markers = item["markers"].as_array().unwrap();
            let center = vec3(&markers.last().unwrap()["pos"]);
            let path = item["path"].as_array().unwrap();
            let step_feet = |step: usize| {
                DVec3::new(
                    path[step][0].as_f64().unwrap(),
                    center.y,
                    path[step][1].as_f64().unwrap(),
                )
            };
            let (_, _sweep_rx) = player(world, 1, center);
            let (observer, mut rx) = player(world, 2, step_feet(0));
            let ids: Vec<Option<i64>> = markers
                .iter()
                .map(|marker| {
                    spawn_marker(
                        world,
                        marker["type"].as_str().unwrap(),
                        vec3(&marker["pos"]),
                        1,
                    )
                })
                .collect();
            // The markers settle, and are added where the walk starts.
            for _ in 0..2 {
                server.tick();
            }
            adds_and_removes(&mut rx);
            let mut ours: Vec<(usize, usize, usize, bool)> = Vec::new();
            for step in 0..path.len() {
                let eye = step_feet(step) + DVec3::Y * PLAYER_EYE_HEIGHT;
                server
                    .ecs
                    .world_mut()
                    .get_mut::<Position>(observer)
                    .unwrap()
                    .0 = eye;
                for tick in 0..2 {
                    server.tick();
                    for (id, added) in adds_and_removes(&mut rx) {
                        if let Some(marker) = ids.iter().position(|marker| *marker == Some(id)) {
                            ours.push((step, tick, marker, added));
                        }
                    }
                }
            }
            let bds: Vec<(usize, usize, usize, bool)> = item["events"]
                .as_array()
                .unwrap()
                .iter()
                .map(|event| {
                    (
                        event["step"].as_u64().unwrap() as usize,
                        event["tick"].as_u64().unwrap() as usize,
                        event["marker"].as_u64().unwrap() as usize,
                        event["kind"] == "add",
                    )
                })
                .filter(|event| ids[event.2].is_some())
                .collect();
            let mut sorted_ours = ours.clone();
            sorted_ours.sort();
            let mut sorted_bds = bds.clone();
            sorted_bds.sort();
            assert_eq!(
                sorted_ours, sorted_bds,
                "view {} / simulation {} {}",
                run["view_distance"], run["simulation_distance"], item["key"]
            );
            replayed += bds.len();
        }
    }
    assert!(replayed > 150, "{replayed} events replayed");
}
