use std::collections::HashMap;
use std::sync::Arc;

use jolyne::valentine::types::{
    BlockPos, EnumsPlayerActionType, EnumsPlayerPositionModeComponentPositionMode,
};
use jolyne::valentine::{PlayerActionPacket, RespawnPacket};
use parking_lot::Mutex;

use super::*;
use crate::entity::components::Hunger;
use crate::server::game::types::PlayerProviderResource;
use crate::storage::{PlayerProvider, StorageResult};

/// Kill the player with a mob hit, after leaving it hungry, and drain the
/// death packets.
fn die(h: &mut Harness) {
    let player = h.player;
    h.world().get_mut::<Hunger>(player).unwrap().food_level = 5;
    let cow = h.cow();
    h.hit_from(cow, 100.0);
    assert!(h.dead());
    h.drain();
}

fn teleports(
    packets: &[McpePacketData],
) -> Vec<(DVec3, EnumsPlayerPositionModeComponentPositionMode)> {
    packets
        .iter()
        .filter_map(|packet| match packet {
            McpePacketData::MovePlayerPacket(movement)
                if movement.player_runtime_id.actor_runtime_id == SESSION =>
            {
                Some((
                    DVec3::new(
                        f64::from(movement.position.x),
                        f64::from(movement.position.y),
                        f64::from(movement.position.z),
                    ),
                    movement.position_mode,
                ))
            }
            _ => None,
        })
        .collect()
}

fn assert_fresh_hunger(hunger: &Hunger) {
    let fresh = Hunger::default();
    assert_eq!(
        (hunger.food_level, hunger.saturation, hunger.exhaustion),
        (fresh.food_level, fresh.saturation, fresh.exhaustion)
    );
}

/// The respawned player: alive with full health and fresh hunger at the
/// world spawn, its client told so.
fn assert_respawned(h: &mut Harness, packets: &[McpePacketData]) {
    let spawn = world_spawn(h);
    let player = h.player;
    assert!(!h.dead());
    assert_eq!(h.health(), 20.0);
    assert_fresh_hunger(h.world().get::<Hunger>(player).unwrap());
    assert_eq!(h.world().get::<Position>(player).unwrap().0, spawn);
    assert_eq!(
        teleports(packets),
        [(spawn, EnumsPlayerPositionModeComponentPositionMode::Respawn)]
    );
    assert!(actor_events(packets, SESSION).contains(&EnumsActorEvent::SpawnAlive));
    assert_eq!(health_updates(packets), [20.0]);
    assert_eq!(
        attribute_updates(packets, "minecraft:player.hunger"),
        [20.0]
    );
}

#[test]
fn the_client_ready_respawn_restores_the_player_at_the_world_spawn() {
    let mut h = harness(Difficulty::Normal);
    die(&mut h);
    // A dead player stays dead until its client asks to respawn.
    h.ticks(5);
    assert!(h.dead());
    assert_eq!(
        health_updates(&h.drain()),
        [0.0],
        "the client sees zero health"
    );

    h.route(RespawnPacket {
        position: Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        state: EnumsPlayerRespawnState::Clientreadytospawn,
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: SESSION,
        },
    });
    h.tick();
    let packets = h.drain();
    assert_eq!(
        respawns(&packets),
        [(EnumsPlayerRespawnState::Readytospawn, world_spawn(&mut h))]
    );
    assert_respawned(&mut h, &packets);

    // A second request from a living player changes nothing.
    h.route(RespawnPacket {
        position: Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        state: EnumsPlayerRespawnState::Clientreadytospawn,
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: SESSION,
        },
    });
    h.tick();
    assert!(teleports(&h.drain()).is_empty());
}

#[test]
fn the_respawn_action_restores_the_player_at_the_world_spawn() {
    let mut h = harness(Difficulty::Normal);
    die(&mut h);
    h.route(PlayerActionPacket {
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: SESSION,
        },
        action: EnumsPlayerActionType::Respawn,
        block_position: BlockPos { x: 0, y: 0, z: 0 },
        result_pos: BlockPos { x: 0, y: 0, z: 0 },
        face: -1,
    });
    h.tick();
    let packets = h.drain();
    assert_respawned(&mut h, &packets);
}

#[test]
fn immediate_respawn_skips_the_death_screen() {
    let mut h = harness(Difficulty::Normal);
    h.rules().do_immediate_respawn = true;
    let player = h.player;
    h.world().get_mut::<Hunger>(player).unwrap().food_level = 5;
    let cow = h.cow();
    h.hit_from(cow, 100.0);
    h.tick();
    let packets = h.drain();
    assert!(
        death_infos(&packets).len() == 1,
        "the death is still reported"
    );
    assert_respawned(&mut h, &packets);
}

/// Player records kept in memory.
#[derive(Default)]
struct MemoryPlayers(Mutex<HashMap<uuid::Uuid, PlayerData>>);

#[async_trait::async_trait]
impl PlayerProvider for MemoryPlayers {
    async fn load(&self, uuid: uuid::Uuid) -> StorageResult<Option<PlayerData>> {
        Ok(self.0.lock().get(&uuid).cloned())
    }

    async fn save(&self, uuid: uuid::Uuid, data: &PlayerData) -> StorageResult<()> {
        self.0.lock().insert(uuid, data.clone());
        Ok(())
    }

    async fn delete(&self, uuid: uuid::Uuid) -> StorageResult<()> {
        self.0.lock().remove(&uuid);
        Ok(())
    }

    async fn close(&self) -> StorageResult<()> {
        Ok(())
    }
}

#[tokio::test]
async fn a_dead_player_saved_and_reloaded_comes_back_alive_at_spawn() {
    let mut h = harness(Difficulty::Normal);
    let players = Arc::new(MemoryPlayers::default());
    h.world()
        .insert_resource(PlayerProviderResource(players.clone()));
    die(&mut h);
    assert!(h.server.save_player(SESSION).await);

    let id = uuid::Uuid::parse_str(&uuid(SESSION)).unwrap();
    let saved =
        crate::server::connect::load_player_data(Some(players.as_ref()), Some(&uuid(SESSION)))
            .await
            .expect("the saved record is valid");
    assert_eq!(
        players.0.lock().get(&id).map(|data| data.health),
        Some(20.0)
    );
    let spawn = world_spawn(&mut h);
    assert_eq!(DVec3::from_array(saved.position), spawn);

    h.server.despawn_player(SESSION);
    // The previous-position spawn rule puts the player where it was saved.
    let feet = DVec3::from_array(saved.position) - DVec3::Y * PLAYER_EYE_HEIGHT;
    let rejoined = join(&mut h.server, SESSION, "Steve", feet, Some(saved));
    h.player = rejoined.entity;
    h.rx = rejoined.rx;
    h.tick();
    assert!(!h.dead());
    assert_eq!(h.health(), 20.0);
    let player = h.player;
    assert_fresh_hunger(h.world().get::<Hunger>(player).unwrap());
    let position = h.world().get::<Position>(player).unwrap().0;
    assert!(position.distance(spawn) < 1.0e-9, "{position} vs {spawn}");
}
