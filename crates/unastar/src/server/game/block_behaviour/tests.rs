//! Block behaviour tests through a real `World`: the change path, neighbour
//! updates, structures and interaction. [`Harness`] is shared with the
//! family modules' tests.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::{McpePacket, McpePacketData};
use tokio::sync::mpsc;

use super::*;
use crate::ecs::events::EventBuffer;
use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{
    DroppedItem, GameMode, HeldSlot, ItemEntityStack, MainInventory, PlayerSession, PlayerState,
    Position, Rotation,
};
use crate::registry::{BlockRegistry, ItemRegistry};
use crate::server::game::types::{
    BlockRegistryResource, ItemRegistryResource, ServerConfigResource,
};
use crate::world::chunk::Chunk;
use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkStateFlags};

/// The floor height: blocks at y = [`FLOOR`] are stone, everything above is
/// air.
pub const FLOOR: i32 = 63;

/// A 3×3-chunk world around the origin with a stone floor at y = 63, the
/// vanilla registries and one survival player at (0.5, 64, 0.5) (eyes at
/// 65.62) viewing every chunk, whose packets land in `rx`.
pub struct Harness {
    pub world: World,
    pub player: Entity,
    pub rx: mpsc::Receiver<McpePacket>,
}

/// The state of `name` with `props` set.
pub fn state(name: &str, props: &[(&str, StateValue)]) -> u32 {
    let entry = BlockRegistry::vanilla()
        .get_by_name(name)
        .unwrap_or_else(|| panic!("{name} is a block"));
    props
        .iter()
        .fold(entry.default_state_id, |state, (prop, value)| {
            entry
                .with_state_value(state, prop, *value)
                .unwrap_or_else(|| panic!("{name} has {prop}={value:?}"))
        })
}

impl Harness {
    pub fn new() -> Self {
        let mut world = World::new();
        world.insert_resource(ChunkManager::new(
            crate::world::WorldConfig::void_spawn_platform(1),
        ));
        world.insert_resource(EventBuffer::default());
        world.insert_resource(ServerConfigResource(Default::default()));
        world.insert_resource(BlockRegistryResource(Arc::new(
            BlockRegistry::vanilla().clone(),
        )));
        let mut items = ItemRegistry::new();
        items.load_vanilla();
        world.insert_resource(ItemRegistryResource(Arc::new(items)));
        world.insert_resource(TickCounter::default());
        world.insert_resource(EntityRandom::seeded(7));
        world.insert_resource(BlockRandom::seeded(7));
        // Noon under the default game rules (crops read the sky light).
        world.insert_resource(crate::world::rules::WorldTime { ticks: 6000 });
        world.insert_resource(crate::world::rules::GameRules::default());
        let (tx, rx) = mpsc::channel(1 << 16);
        let player = world
            .spawn((
                GameMode::Survival,
                Position(DVec3::new(
                    0.5,
                    64.0 + crate::entity::PLAYER_EYE_HEIGHT,
                    0.5,
                )),
                Rotation::default(),
                MainInventory::default(),
                HeldSlot::default(),
                PlayerState::default(),
                PlayerSession::new(1, "blocks".into(), None, None, tx),
            ))
            .id();
        let stone = *crate::world::chunk::blocks::STONE;
        for cx in -1..=1 {
            for cz in -1..=1 {
                let mut chunk = Chunk::new(cx, cz);
                for x in 0..16 {
                    for z in 0..16 {
                        chunk.set_block(x, FLOOR as i16, z, stone);
                    }
                }
                let mut viewers = crate::world::ecs::ChunkViewers::default();
                viewers.insert(player);
                let entity = world
                    .spawn((
                        ChunkData::new(chunk),
                        ChunkStateFlags::default(),
                        ChunkPosition::new(cx, cz),
                        viewers,
                    ))
                    .id();
                world
                    .resource_mut::<ChunkManager>()
                    .insert(ChunkPosition::new(cx, cz), entity);
            }
        }
        Self { world, player, rx }
    }

    /// Change the block at `pos` through the change path (neighbour updates
    /// run).
    pub fn set(&mut self, pos: IVec3, state: u32) {
        crate::server::game::blocks::set_block(&mut self.world, pos, state, false);
    }

    /// Write the block at `pos` without any update (world setup).
    pub fn put(&mut self, pos: IVec3, state: u32) {
        let (cx, cz) = crate::world::ecs::world_to_chunk_coords(pos.x, pos.z);
        let chunk = self
            .world
            .resource::<ChunkManager>()
            .get_by_coords(cx, cz)
            .expect("chunk loaded");
        let (x, y, z) = crate::world::ecs::world_to_local_coords(pos.x, pos.y, pos.z);
        self.world
            .get_mut::<ChunkData>(chunk)
            .unwrap()
            .inner
            .set_block(x, y, z, state);
    }

    pub fn state(&self, pos: IVec3) -> u32 {
        self.world.block_at(pos).expect("loaded")
    }

    pub fn name(&self, pos: IVec3) -> &str {
        name_at(&self.world, pos).expect("loaded")
    }

    pub fn value(&self, pos: IVec3, property: &str) -> Option<StateValue> {
        let state = self.state(pos);
        self.world
            .registry()
            .get_by_runtime_id(state)?
            .state_value(state, property)
    }

    /// Advance `ticks` game ticks of block ticks (scheduled and random);
    /// each starts by sending the previous tick's block updates, as the
    /// server tick does.
    pub fn tick(&mut self, ticks: u64) {
        for _ in 0..ticks {
            crate::server::game::blocks::send_block_updates(&mut self.world);
            self.world.resource_mut::<TickCounter>().current += 1;
            tick_blocks(&mut self.world);
        }
    }

    /// Put `count` of `item` in the player's selected hotbar slot.
    pub fn hold(&mut self, item: &str, count: u8) {
        let stack = crate::item::test_support::stack(item, count);
        let block = {
            let items = &self.world.resource::<ItemRegistryResource>().0;
            let registry = &self.world.resource::<BlockRegistryResource>().0;
            items
                .get_by_name(item)
                .and_then(|entry| item_block(registry, entry))
                .filter(|block| block.string_id == item)
                .map(|block| block.default_state_id)
        };
        let stack = match block {
            Some(runtime_id) => stack.with_runtime_id(runtime_id as i32),
            None => stack,
        };
        self.world
            .get_mut::<MainInventory>(self.player)
            .unwrap()
            .0
            .set_item(0, stack)
            .unwrap();
    }

    /// Move the player's eyes to `eye`, looking with `yaw`/`pitch`.
    pub fn stand(&mut self, eye: DVec3, yaw: f32, pitch: f32) {
        self.world.get_mut::<Position>(self.player).unwrap().0 = eye;
        *self.world.get_mut::<Rotation>(self.player).unwrap() = Rotation::new(yaw, pitch);
    }

    /// The player clicks face `face` of the block at `clicked` at `click`
    /// (relative to the block) with the held stack, as an `ItemUse`
    /// ClickBlock transaction; placements plugins would approve commit at
    /// once. Returns the committed placement events' blocks.
    pub fn click(&mut self, clicked: IVec3, face: u8, click: DVec3) -> Vec<(IVec3, u32)> {
        use jolyne::valentine::types::{BlockPos, Vec3};
        let stack = self
            .world
            .get::<MainInventory>(self.player)
            .unwrap()
            .0
            .item(0)
            .cloned()
            .unwrap_or_else(crate::item::ItemStack::empty);
        let ids = crate::server::game::types::block_network_ids(&self.world);
        let mut item =
            jolyne::valentine::CerealizerNetworkItemStackDescriptorSerializedData::default();
        if let Some(entry) = stack.item() {
            item.id = i16::try_from(entry.network_id()).unwrap();
            item.stacksize = u16::from(stack.count);
            item.auxvalue = u32::try_from(stack.damage).unwrap();
            item.block_runtime_id = ids
                .item_block_to_wire(entry.network_id(), u32::try_from(stack.runtime_id).unwrap());
        }
        let data = jolyne::valentine::ItemUseInventoryTransaction {
            position: BlockPos {
                x: clicked.x,
                y: clicked.y,
                z: clicked.z,
            },
            face,
            slot: 0,
            item,
            click_position: Vec3 {
                x: click.x as f32,
                y: click.y as f32,
                z: click.z as f32,
            },
            ..Default::default()
        };
        crate::server::game::blocks::handle_block_click(&mut self.world, self.player, &data);
        self.commit_events()
    }

    /// The player breaks the block at `pos` (an approved break intent).
    pub fn break_at(&mut self, pos: IVec3) {
        let block_id = self.state(pos);
        crate::server::game::commit_block_event(
            &mut self.world,
            &crate::ecs::events::ServerEvent::BlockBreak {
                entity: self.player,
                player_id: String::new(),
                position: (pos.x, pos.y, pos.z),
                block_id,
            },
        );
    }

    /// Commit every queued placement as an approving plugin manager would.
    fn commit_events(&mut self) -> Vec<(IVec3, u32)> {
        let events = self.world.resource_mut::<EventBuffer>().drain();
        let mut placed = Vec::new();
        for event in events {
            if let crate::ecs::events::ServerEvent::BlockPlace {
                position, block_id, ..
            } = &event
            {
                placed.push((IVec3::new(position.0, position.1, position.2), *block_id));
                crate::server::game::commit_block_event(&mut self.world, &event);
            }
        }
        placed
    }

    /// Item entities on the ground: `(item id, count)`.
    pub fn drops(&mut self) -> Vec<(String, u8)> {
        let mut query = self
            .world
            .query_filtered::<&ItemEntityStack, With<DroppedItem>>();
        let mut drops: Vec<_> = query
            .iter(&self.world)
            .map(|stack| (stack.0.item_id().to_string(), stack.0.count))
            .collect();
        drops.sort();
        drops
    }

    /// Every packet sent to the player since the last call, including the
    /// block updates and block sounds still queued for the next tick.
    pub fn packets(&mut self) -> Vec<McpePacketData> {
        crate::server::game::blocks::send_block_updates(&mut self.world);
        let mut out = Vec::new();
        while let Ok(packet) = self.rx.try_recv() {
            out.push(packet.data);
        }
        out
    }

    /// The block positions and wire states of every block change sent to
    /// the player since the last call (UpdateBlock and the entries of
    /// UpdateSubChunkBlocks, in order), decoded back to runtime ids.
    pub fn updates(&mut self) -> Vec<(IVec3, u32)> {
        let ids = crate::server::game::types::block_network_ids(&self.world);
        let decode = |x: i32, y: i32, z: i32, wire: u32| {
            (
                IVec3::new(x, y, z),
                ids.from_wire(wire)
                    .expect("block updates carry known wire ids"),
            )
        };
        self.packets()
            .into_iter()
            .flat_map(|packet| match packet {
                McpePacketData::UpdateBlockPacket(update) => vec![decode(
                    update.block_position.x,
                    update.block_position.y,
                    update.block_position.z,
                    update.block_runtime_id,
                )],
                McpePacketData::UpdateSubChunkBlocksPacket(batch) => batch
                    .blocks_changed
                    .blocks_changed_standards
                    .iter()
                    .map(|entry| decode(entry.pos.x, entry.pos.y, entry.pos.z, entry.runtime_id))
                    .collect(),
                _ => Vec::new(),
            })
            .collect()
    }
}
