//! Test support for replaying `bds-sweep-client` captures of scripts played
//! against BDS 1.26.51 (fixtures cut by `tools/bds-sweep/combat_fixture.py`).
//!
//! A fixture is one JSON record per line: `dir` (`c2s` or `s2c`), `input`
//! (the client tick of the latest input sent before the packet) and `hex`
//! (the packet without its batch length prefix). [`Replay`] is a server with
//! the scripted player (`BDSSweep`, an operator, feet at the sweep arena's
//! `(0.5, -60, 0.5)`, facing east) on a stone floor at y = -61.

use bevy_ecs::prelude::*;
use bytes::BytesMut;
use glam::{DVec3, IVec3};
use jolyne::valentine::{BorrowedMcpePacket, McpePacket, McpePacketArgs, McpePacketData};
use tokio::sync::mpsc::Receiver;

use crate::entity::components::{GameMode, HeldSlot, MainInventory, Rotation};
use crate::entity::systems::BlockSource;
use crate::item::ItemStack;
use crate::registry::BlockRegistry;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};

/// The runtime id BDS gave the scripted player.
pub const BDS_PLAYER: u64 = 1;
/// Our scripted player's session and runtime id.
pub const SESSION: u64 = 7;
/// The scripted player's feet.
pub const FEET: DVec3 = DVec3::new(0.5, -60.0, 0.5);

/// A capture record.
pub struct Record {
    pub from_client: bool,
    pub input: u64,
    pub packet: McpePacketData,
}

/// A recorded packet (hex, without its batch length prefix).
pub fn decode(hex: &str) -> McpePacketData {
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    let mut framed = Vec::with_capacity(bytes.len() + 5);
    let mut len = bytes.len() as u32;
    while len >= 0x80 {
        framed.push(len as u8 | 0x80);
        len >>= 7;
    }
    framed.push(len as u8);
    framed.extend_from_slice(&bytes);
    McpePacketData::decode_inner_bytes(&mut bytes::Bytes::from(framed), McpePacketArgs)
        .unwrap()
        .1
}

/// The records of a fixture.
pub fn records(fixture: &str) -> Vec<Record> {
    fixture
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            Record {
                from_client: row["dir"] == "c2s",
                input: row["input"].as_u64().unwrap_or(0),
                packet: decode(row["hex"].as_str().unwrap()),
            }
        })
        .collect()
}

/// A server with the scripted player.
pub struct Replay {
    pub server: GameServer,
    pub player: Entity,
    rx: Receiver<McpePacket>,
}

impl Replay {
    pub fn new() -> Self {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            operators: vec!["BDSSweep".into()],
            ..ServerConfig::default()
        });
        server
            .ecs
            .world_mut()
            .insert_resource(crate::entity::EntityRandom::seeded(13));
        let (outbound_tx, rx) = tokio::sync::mpsc::channel(1 << 16);
        let player = server
            .spawn_player(PlayerSpawnData {
                session_id: SESSION,
                display_name: "BDSSweep".into(),
                xuid: None,
                uuid: Some(format!("00000000-0000-0000-0000-{SESSION:012}")),
                runtime_id: SESSION as i64,
                position: FEET + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT,
                outbound_tx,
                saved_data: None,
                chunk_radius: 2,
            })
            .expect("valid player spawn");
        let mut replay = Self { server, player, rx };
        for _ in 0..40 {
            if [(-1, -1), (-1, 1), (1, -1), (1, 1)]
                .iter()
                .all(|&(x, z)| replay.world().block_at(IVec3::new(x, -61, z)).is_some())
            {
                break;
            }
            replay.server.tick();
        }
        *replay.world().get_mut::<GameMode>(player).unwrap() = GameMode::Survival;
        // Facing east, level, as the scripts teleport the player.
        *replay.world().get_mut::<Rotation>(player).unwrap() = Rotation {
            pitch: 0.0,
            yaw: -90.0,
            head_yaw: -90.0,
        };
        for x in -4..=6 {
            for z in -4..=4 {
                replay.set(IVec3::new(x, -61, z), "minecraft:stone");
                for y in -60..=-55 {
                    replay.set(IVec3::new(x, y, z), "minecraft:air");
                }
            }
        }
        replay.server.tick();
        replay.drain();
        replay
    }

    pub fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    pub fn set(&mut self, pos: IVec3, name: &str) {
        let state = BlockRegistry::vanilla()
            .default_state_id_by_name(name)
            .unwrap();
        crate::server::game::blocks::set_block(self.world(), pos, state, false);
    }

    /// Put `stack` in hotbar `slot` (the scripts' `/give` and
    /// `/replaceitem`).
    pub fn put(&mut self, slot: usize, stack: ItemStack) {
        let player = self.player;
        self.world()
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(slot, stack)
            .unwrap();
    }

    /// Empty the inventory (`/clear`).
    pub fn clear(&mut self) {
        let player = self.player;
        let mut inventory = self.world().get_mut::<MainInventory>(player).unwrap();
        for slot in 0..36 {
            let _ = inventory.0.set_item(slot, ItemStack::empty());
        }
    }

    pub fn select(&mut self, slot: u8) {
        let player = self.player;
        *self.world().get_mut::<HeldSlot>(player).unwrap() = HeldSlot(slot);
    }

    /// Route a client packet through the server's packet entry point.
    pub fn route(&mut self, packet: McpePacket) {
        let mut buf = BytesMut::new();
        packet.data.encode_inner(&mut buf, 0, 0).unwrap();
        let borrowed = BorrowedMcpePacket::decode_inner(&mut buf.freeze()).unwrap();
        self.server.route_packet(SESSION, McpePacketArgs, borrowed);
    }

    pub fn drain(&mut self) -> Vec<McpePacketData> {
        std::iter::from_fn(|| self.rx.try_recv().ok().map(|packet| packet.data)).collect()
    }

    /// One server tick, then the plugin manager's default for the tick's
    /// events when no plugin cancels them: block changes commit.
    pub fn tick(&mut self) {
        self.server.tick();
        let events = self
            .world()
            .resource_mut::<crate::ecs::events::EventBuffer>()
            .drain();
        for event in events {
            if matches!(
                event,
                crate::ecs::events::ServerEvent::BlockBreak { .. }
                    | crate::ecs::events::ServerEvent::BlockPlace { .. }
            ) {
                crate::server::game::blocks::commit_block_event(self.world(), &event);
            }
        }
    }
}
