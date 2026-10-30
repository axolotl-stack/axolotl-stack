//! Headless tests of player damage, death and respawn: a real server on the
//! void spawn platform, players joined through `spawn_player`, client
//! packets routed through `route_packet` and the server's packets captured
//! from each player's session channel.

mod death;
mod environment;
mod hurt;
mod input_tick;
mod respawn;

use bevy_ecs::prelude::*;
use bytes::BytesMut;
use glam::{DVec3, IVec3};
use jolyne::valentine::types::{
    ActorRuntimeId, EnumsActorEvent, EnumsPlayerAuthInputPacketPayloadInputData,
    EnumsPlayerRespawnState, TextPacketBody, Vec3,
};
use jolyne::valentine::{
    BorrowedMcpePacket, McpePacket, McpePacketArgs, McpePacketData, PlayerAuthInputPacket,
};
use tokio::sync::mpsc::Receiver;
use unastar_data::actor_components::body::ActorDamageCause;

use crate::entity::components::{DroppedItem, GameMode, Health, ItemEntityStack, Position};
use crate::entity::mob::combat::Hit;
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::entity::systems::BlockSource;
use crate::entity::xp_orb::XpOrb;
use crate::entity::{EntityRandom, PLAYER_EYE_HEIGHT};
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
use crate::storage::PlayerData;
use crate::world::ChunkManager;
use crate::world::chunk::blocks;
use crate::world::ecs::ChunkData;
use crate::world::rules::{Difficulty, GameRules};

/// Session and runtime ID of the player under test.
const SESSION: u64 = 7;
/// Session and runtime ID of a second player standing next to it.
const VIEWER: u64 = 8;
/// The platform's top face (stone fills y 0..=15).
const GROUND: f64 = 16.0;
/// Where the player under test stands.
const HOME: DVec3 = DVec3::new(4.5, GROUND, 4.5);

struct Joined {
    entity: Entity,
    rx: Receiver<McpePacket>,
}

struct Harness {
    server: GameServer,
    player: Entity,
    rx: Receiver<McpePacket>,
}

fn eyes(feet: DVec3) -> DVec3 {
    feet + DVec3::Y * PLAYER_EYE_HEIGHT
}

fn uuid(session: u64) -> String {
    format!("00000000-0000-0000-0000-{session:012}")
}

fn join(
    server: &mut GameServer,
    session: u64,
    name: &str,
    feet: DVec3,
    saved_data: Option<PlayerData>,
) -> Joined {
    let (outbound_tx, rx) = tokio::sync::mpsc::channel(1 << 16);
    let entity = server
        .spawn_player(PlayerSpawnData {
            session_id: session,
            display_name: name.into(),
            xuid: None,
            uuid: Some(uuid(session)),
            runtime_id: session as i64,
            position: eyes(feet),
            outbound_tx,
            saved_data,
            chunk_radius: 2,
        })
        .expect("valid player spawn");
    Joined { entity, rx }
}

/// A survival player named Steve standing on the platform at [`HOME`], the
/// platform loaded and the join packets drained.
fn harness(difficulty: Difficulty) -> Harness {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        difficulty,
        ..ServerConfig::default()
    });
    server
        .ecs
        .world_mut()
        .insert_resource(EntityRandom::seeded(11));
    let Joined { entity, rx } = join(&mut server, SESSION, "Steve", HOME, None);
    let mut h = Harness {
        server,
        player: entity,
        rx,
    };
    h.load_platform();
    h.drain();
    h
}

impl Harness {
    fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    /// Tick until the platform under [`HOME`] is loaded.
    fn load_platform(&mut self) {
        let under = HOME.floor().as_ivec3() - IVec3::Y;
        for _ in 0..20 {
            if self.world().block_at(under) == Some(*blocks::STONE) {
                return;
            }
            self.server.tick();
        }
        panic!("the platform under the player never loaded");
    }

    fn tick(&mut self) {
        self.server.tick();
    }

    fn ticks(&mut self, count: usize) {
        for _ in 0..count {
            self.server.tick();
        }
    }

    fn drain(&mut self) -> Vec<McpePacketData> {
        std::iter::from_fn(|| self.rx.try_recv().ok().map(|packet| packet.data)).collect()
    }

    fn health(&mut self) -> f32 {
        let player = self.player;
        self.world().get::<Health>(player).unwrap().current
    }

    fn set_health(&mut self, health: f32) {
        let player = self.player;
        self.world().get_mut::<Health>(player).unwrap().current = health;
        self.tick();
        self.drain();
    }

    fn set_mode(&mut self, mode: GameMode) {
        let player = self.player;
        *self.world().get_mut::<GameMode>(player).unwrap() = mode;
    }

    fn rules(&mut self) -> Mut<'_, GameRules> {
        self.world().resource_mut::<GameRules>()
    }

    fn dead(&mut self) -> bool {
        let player = self.player;
        self.world().get::<super::Dead>(player).is_some()
    }

    fn feet(&mut self) -> DVec3 {
        let player = self.player;
        self.world().get::<Position>(player).unwrap().0 - DVec3::Y * PLAYER_EYE_HEIGHT
    }

    fn set_block(&mut self, pos: IVec3, block: u32) {
        let world = self.world();
        let chunk = world
            .resource::<ChunkManager>()
            .get_by_coords(pos.x >> 4, pos.z >> 4)
            .expect("chunk loaded");
        world.get_mut::<ChunkData>(chunk).unwrap().inner.set_block(
            (pos.x & 15) as u8,
            pos.y as i16,
            (pos.z & 15) as u8,
            block,
        );
    }

    /// Route `packet` from the player's client as the network task does:
    /// encoded, then decoded as a borrowed packet.
    fn route(&mut self, packet: impl Into<McpePacket>) {
        let packet: McpePacket = packet.into();
        let mut buf = BytesMut::new();
        packet.data.encode_inner(&mut buf, 0, 0).unwrap();
        let borrowed = BorrowedMcpePacket::decode_inner(&mut buf.freeze()).unwrap();
        self.server.route_packet(SESSION, McpePacketArgs, borrowed);
    }

    /// One `PlayerAuthInput` moving the player's feet to `feet`, then a
    /// tick; `landed` reports the vertical collision of a landing.
    fn step(&mut self, feet: DVec3, landed: bool) {
        let position = eyes(feet);
        self.route(PlayerAuthInputPacket {
            position: Vec3 {
                x: position.x as f32,
                y: position.y as f32,
                z: position.z as f32,
            },
            input_data: if landed {
                vec![EnumsPlayerAuthInputPacketPayloadInputData::Verticalcollision]
            } else {
                Vec::new()
            },
            ..Default::default()
        });
        self.tick();
    }

    /// Jump the player's feet up to `height` blocks above the ground at
    /// `at`, fall one block per input to the ground and land on it.
    fn fall(&mut self, at: DVec3, height: u32) {
        for y in (0..=height).rev() {
            self.step(DVec3::new(at.x, GROUND + f64::from(y), at.z), false);
        }
        self.step(DVec3::new(at.x, GROUND, at.z), true);
    }

    /// An adult cow with its feet two blocks west of the player.
    fn cow(&mut self) -> Entity {
        let feet = self.feet() - DVec3::X * 2.0;
        let cow = spawn_mob(
            self.world(),
            "minecraft:cow",
            feet,
            0.0,
            SpawnCause::Spawned,
        )
        .unwrap();
        self.tick();
        self.drain();
        cow
    }

    /// A melee hit of `damage` from `mob`, as a mob's attack deals it.
    fn hit_from(&mut self, mob: Entity, damage: f32) -> f32 {
        let from = self.world().get::<Position>(mob).unwrap().0;
        let player = self.player;
        super::hurt_player(
            self.world(),
            player,
            &Hit {
                damage,
                cause: ActorDamageCause::EntityAttack,
                attacker: Some(mob),
                by_player: false,
                knockback_from: Some(from),
                knockback_power: KnockbackPower::DEFAULT,
            },
        )
    }

    fn drops(&mut self) -> Vec<(String, u8)> {
        let world = self.world();
        let mut drops: Vec<(String, u8)> = world
            .query_filtered::<&ItemEntityStack, With<DroppedItem>>()
            .iter(world)
            .map(|stack| (stack.0.item_id().to_owned(), stack.0.count))
            .collect();
        drops.sort();
        drops
    }

    fn experience_dropped(&mut self) -> u32 {
        let world = self.world();
        world
            .query::<&XpOrb>()
            .iter(world)
            .map(|orb| orb.value)
            .sum()
    }

    /// A second player standing next to the player under test.
    fn viewer(&mut self) -> Joined {
        let mut viewer = join(&mut self.server, VIEWER, "Alex", HOME + DVec3::X, None);
        self.tick();
        self.drain();
        std::iter::from_fn(|| viewer.rx.try_recv().ok()).for_each(drop);
        viewer
    }
}

fn drain(joined: &mut Joined) -> Vec<McpePacketData> {
    std::iter::from_fn(|| joined.rx.try_recv().ok().map(|packet| packet.data)).collect()
}

fn actor_events(packets: &[McpePacketData], runtime_id: u64) -> Vec<EnumsActorEvent> {
    packets
        .iter()
        .filter_map(|packet| match packet {
            McpePacketData::ActorEventPacket(event)
                if event.target_runtime_id.actor_runtime_id == runtime_id =>
            {
                Some(event.event_id)
            }
            _ => None,
        })
        .collect()
}

/// The `minecraft:health` values of the `UpdateAttributes` sent.
fn health_updates(packets: &[McpePacketData]) -> Vec<f32> {
    attribute_updates(packets, "minecraft:health")
}

fn attribute_updates(packets: &[McpePacketData], name: &str) -> Vec<f32> {
    packets
        .iter()
        .filter_map(|packet| match packet {
            McpePacketData::UpdateAttributesPacket(update)
                if update.target_runtime_id.actor_runtime_id == SESSION =>
            {
                Some(update)
            }
            _ => None,
        })
        .flat_map(|update| update.attribute_list.iter())
        .filter(|attribute| &*attribute.name == name)
        .map(|attribute| attribute.current_value)
        .collect()
}

fn motions(packets: &[McpePacketData]) -> Vec<Vec3> {
    packets
        .iter()
        .filter_map(|packet| match packet {
            McpePacketData::SetActorMotionPacket(motion)
                if motion.target_runtime_id.actor_runtime_id == SESSION =>
            {
                Some(motion.motion.clone())
            }
            _ => None,
        })
        .collect()
}

fn respawns(packets: &[McpePacketData]) -> Vec<(EnumsPlayerRespawnState, DVec3)> {
    packets
        .iter()
        .filter_map(|packet| match packet {
            McpePacketData::RespawnPacket(respawn) => Some((
                respawn.state,
                DVec3::new(
                    f64::from(respawn.position.x),
                    f64::from(respawn.position.y),
                    f64::from(respawn.position.z),
                ),
            )),
            _ => None,
        })
        .collect()
}

/// The `DeathInfo` key and parameters sent.
fn death_infos(packets: &[McpePacketData]) -> Vec<(String, Vec<String>)> {
    packets
        .iter()
        .filter_map(|packet| match packet {
            McpePacketData::DeathInfoPacket(info) => Some((
                info.death_cause_attack_name.to_string(),
                info.death_cause_message_list
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )),
            _ => None,
        })
        .collect()
}

/// The translated chat messages sent: key and parameters.
fn translations(packets: &[McpePacketData]) -> Vec<(String, Vec<String>)> {
    packets
        .iter()
        .filter_map(|packet| match packet {
            McpePacketData::TextPacket(text) => match &text.body {
                TextPacketBody::MessageAndParams(body) => Some((
                    body.message.to_string(),
                    body.parameter_list
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                )),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// The world spawn players respawn at (the void platform's spawn, in the
/// players' eye-height position convention).
fn world_spawn(h: &mut Harness) -> DVec3 {
    h.world().resource::<super::respawn::WorldSpawn>().0
}
