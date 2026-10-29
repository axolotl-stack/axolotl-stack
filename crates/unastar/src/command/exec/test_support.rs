//! A headless game server with players for executor tests.

use crate::command::{CommandOutput, CommandSource, Operator, execute};
use crate::entity::PLAYER_EYE_HEIGHT;
use crate::entity::components::transform::{Position, Rotation};
use crate::entity::components::{
    ArmourInventory, Experience, GameMode, HeldSlot, MainInventory, OffhandSlot, Player,
    PlayerName, PlayerSession, PlayerUuid, RuntimeEntityId,
};
use crate::server::game::{GameServer, ServerConfig};
use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::{McpePacket, McpePacketData};
use std::collections::HashMap;
use tokio::sync::mpsc;

/// A void-platform server (chunk 0,0 loaded) and the players a test adds.
pub struct Harness {
    pub server: GameServer,
    receivers: HashMap<Entity, mpsc::Receiver<McpePacket>>,
    next_id: i64,
}

impl Harness {
    pub fn new() -> Self {
        let mut server = GameServer::with_config(ServerConfig {
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
        Self {
            server,
            receivers: HashMap::new(),
            next_id: 1,
        }
    }

    pub fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    /// A survival player with empty inventories, feet at (3.5, 64, 3.5).
    pub fn player(&mut self, name: &str, operator: bool) -> Entity {
        self.player_at(name, operator, DVec3::new(3.5, 64.0, 3.5))
    }

    pub fn player_at(&mut self, name: &str, operator: bool, feet: DVec3) -> Entity {
        let id = self.next_id;
        self.next_id += 1;
        let (tx, rx) = mpsc::channel(4096);
        let world = self.world();
        let entity = world
            .spawn((
                Player,
                PlayerName(name.into()),
                PlayerUuid(uuid::Uuid::nil()),
                PlayerSession::new(id as u64, name.into(), None, None, tx),
                Position(feet + DVec3::Y * PLAYER_EYE_HEIGHT),
                Rotation::default(),
                GameMode::Survival,
                RuntimeEntityId(id),
                Experience::default(),
            ))
            .insert((
                MainInventory::default(),
                ArmourInventory::default(),
                OffhandSlot::default(),
                HeldSlot(0),
            ))
            .id();
        if operator {
            world.entity_mut(entity).insert(Operator);
        }
        self.receivers.insert(entity, rx);
        entity
    }

    /// Runs a command as `player`.
    pub fn run(&mut self, player: Entity, line: &str) -> CommandOutput {
        let world = self.world();
        let source = CommandSource::player(world, player).expect("a player");
        execute(world, source, line)
    }

    /// Everything sent to `player` so far.
    pub fn sent(&mut self, player: Entity) -> Vec<McpePacketData> {
        let rx = self.receivers.get_mut(&player).expect("a harness player");
        let mut out = Vec::new();
        while let Ok(packet) = rx.try_recv() {
            out.push(packet.data);
        }
        out
    }
}
