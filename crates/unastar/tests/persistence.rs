use std::sync::Arc;

use bevy_ecs::prelude::*;
use unastar::server::{GameServer, ServerConfig};
use unastar::storage::{BlazeDBProvider, ChunkColumn, LevelDBWorldProvider, WorldProvider};
use unastar::world::ecs::{
    ChunkData, ChunkLoadConfig, ChunkManager, ChunkStateFlags, PendingChunkGenerations,
    systems::{process_completed_generations, world_to_local_coords},
};
use unastar::world::{Chunk, ChunkPos, WorldConfig, WorldGenerator};

#[tokio::test(flavor = "multi_thread")]
async fn async_stored_chunks_survive_save_cycle_for_both_providers() {
    for blaze in [false, true] {
        let directory =
            std::env::temp_dir().join(format!("unastar-persistence-{}", uuid::Uuid::new_v4()));
        let provider: Arc<dyn WorldProvider> = if blaze {
            BlazeDBProvider::open(&directory, None).unwrap()
        } else {
            Arc::new(LevelDBWorldProvider::open(&directory, 0).unwrap())
        };
        let mut chunk = Chunk::new(2, -3);
        chunk.set_block(4, 75, 6, *unastar::world::chunk::blocks::DIAMOND_ORE);
        provider
            .save_column(ChunkPos::new(2, -3), 0, &ChunkColumn::new(chunk))
            .await
            .unwrap();
        let mut server = GameServer::with_config(ServerConfig {
            world: WorldConfig {
                generator: WorldGenerator::Vanilla { seed: 12345 },
                ..WorldConfig::default()
            },
            ..ServerConfig::default()
        });
        let viewer = server.ecs.world_mut().spawn_empty().id();
        let receiver = {
            let mut manager = server.ecs.world_mut().resource_mut::<ChunkManager>();
            manager.set_provider(provider.clone());
            manager.request_generation(2, -3, viewer).unwrap()
        };
        let result = receiver.await.unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        tx.send(result).unwrap();
        server
            .ecs
            .world_mut()
            .resource_mut::<PendingChunkGenerations>()
            .add(2, -3, rx);
        server
            .ecs
            .world_mut()
            .insert_resource(ChunkLoadConfig::default());
        let mut schedule = Schedule::default();
        schedule.add_systems(process_completed_generations);
        schedule.run(server.ecs.world_mut());
        let entity = server
            .ecs
            .world()
            .resource::<ChunkManager>()
            .get_by_coords(2, -3)
            .unwrap();
        assert_eq!(
            server
                .ecs
                .world()
                .get::<ChunkData>(entity)
                .unwrap()
                .inner
                .get_block(4, 75, 6),
            *unastar::world::chunk::blocks::DIAMOND_ORE
        );
        assert!(
            !server
                .ecs
                .world()
                .get::<ChunkStateFlags>(entity)
                .unwrap()
                .is_dirty()
        );
        assert_eq!(server.save_all_chunks().await, 0);
        assert_eq!(
            provider
                .load_column(ChunkPos::new(2, -3), 0)
                .await
                .unwrap()
                .unwrap()
                .chunk
                .get_block(4, 75, 6),
            *unastar::world::chunk::blocks::DIAMOND_ORE
        );
        drop(server);
        provider.close().await.unwrap();
        drop(provider);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn out_of_range_world_y_cannot_alias_valid_blocks() {
    let mut chunk = Chunk::new(0, 0);
    for y in [65_536, -65_536, i32::MAX, i32::MIN] {
        let (x, local_y, z) = world_to_local_coords(0, y, 0);
        assert_eq!(
            chunk.set_block(x, local_y, z, 42),
            None,
            "Y={y} must be rejected"
        );
    }
    assert_eq!(
        chunk.get_block(0, 0, 0),
        *unastar::world::chunk::blocks::AIR
    );
}

#[tokio::test]
async fn disconnect_reconnect_restores_inventory_and_player_state() {
    use glam::DVec3;
    use unastar::entity::components::{
        ArmourInventory, Experience, GameMode, Health, HeldSlot, Hunger, MainInventory,
        OffhandSlot, Position, Rotation,
    };
    use unastar::item::ItemStack;
    use unastar::server::PlayerSpawnData;
    use unastar::server::game::types::{PlayerDataStoreResource, PlayerProviderResource};
    use unastar::storage::{LevelDBPlayerProvider, PlayerProvider};

    let directory = std::env::temp_dir().join(format!("unastar-player-{}", uuid::Uuid::new_v4()));
    let provider = Arc::new(LevelDBPlayerProvider::open(&directory).unwrap());
    let mut server = GameServer::new();
    server
        .ecs
        .world_mut()
        .insert_resource(PlayerProviderResource(provider.clone()));
    let positions = Arc::new(unastar::config::PlayerDataStore::new(
        directory.join("positions"),
    ));
    server
        .ecs
        .world_mut()
        .insert_resource(PlayerDataStoreResource(positions.clone()));
    let uuid = uuid::Uuid::new_v4();
    assert!(
        unastar::server::connect::load_player_data(
            Some(provider.as_ref()),
            Some(&uuid.to_string())
        )
        .await
        .is_none()
    );
    let (outbound_tx, _outbound_rx) = tokio::sync::mpsc::channel(32);
    let entity = server.spawn_player(PlayerSpawnData {
        session_id: 1,
        display_name: "persistence".into(),
        xuid: None,
        uuid: Some(uuid.to_string()),
        runtime_id: 1,
        position: DVec3::new(33.25, 75.0, -8.5),
        saved_data: None,
        outbound_tx,
        chunk_radius: 1,
    });
    server.ecs.world_mut().entity_mut(entity).insert((
        GameMode::Adventure,
        Rotation {
            yaw: 123.0,
            pitch: -12.0,
            ..Rotation::default()
        },
    ));
    server
        .ecs
        .world_mut()
        .get_mut::<MainInventory>(entity)
        .unwrap()
        .0
        .set_item(8, ItemStack::new("minecraft:diamond", 17))
        .unwrap();
    server.ecs.world_mut().entity_mut(entity).insert((
        Health {
            current: 13.0,
            max: 24.0,
        },
        Hunger {
            food_level: 9,
            saturation: 2.5,
            exhaustion: 1.25,
        },
        Experience {
            level: 7,
            progress: 0.5,
        },
        HeldSlot(8),
        OffhandSlot(ItemStack::new("minecraft:torch", 12)),
    ));
    server
        .ecs
        .world_mut()
        .get_mut::<ArmourInventory>(entity)
        .unwrap()
        .set_helmet(ItemStack::new("minecraft:iron_helmet", 1).with_damage(7))
        .unwrap();
    assert!(server.save_player(1).await);
    server.despawn_player(1);
    let data = provider.load(uuid).await.unwrap().unwrap();
    assert_eq!(data.position, [33.25, 75.0, -8.5]);
    assert_eq!(data.rotation, [123.0, -12.0]);
    assert_eq!(data.game_mode, 2);
    let last = positions
        .load_last_position(&uuid.to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        [last.location.x, last.location.y, last.location.z],
        [33.25, 75.0, -8.5]
    );
    assert_eq!([last.location.yaw, last.location.pitch], [123.0, -12.0]);
    drop(server);
    drop(provider);
    let provider = Arc::new(LevelDBPlayerProvider::open(&directory).unwrap());
    let saved_data = unastar::server::connect::load_player_data(
        Some(provider.as_ref()),
        Some(&uuid.to_string()),
    )
    .await
    .unwrap();
    let config = unastar::config::UnastarConfig::default();
    let spawn = unastar::server::connect::resolve_spawn_location(
        &config,
        Some(&uuid.to_string()),
        &jolyne::WorldTemplate::default(),
        &positions,
        Some(&saved_data),
    )
    .await;
    let mut server = GameServer::new();
    server
        .ecs
        .world_mut()
        .insert_resource(PlayerProviderResource(provider.clone()));
    let (outbound_tx, mut outbound_rx) = tokio::sync::mpsc::channel(32);
    let entity = server.spawn_player(PlayerSpawnData {
        session_id: 2,
        display_name: "reconnect".into(),
        xuid: None,
        uuid: Some(uuid.to_string()),
        runtime_id: 2,
        position: DVec3::new(spawn.x as f64, spawn.y as f64, spawn.z as f64),
        saved_data: Some(saved_data),
        outbound_tx,
        chunk_radius: 1,
    });
    let world = server.ecs.world();
    assert_eq!(
        world.get::<Position>(entity).unwrap().0,
        DVec3::new(33.25, 75.0, -8.5)
    );
    assert_eq!(world.get::<Rotation>(entity).unwrap().yaw, 123.0);
    assert_eq!(world.get::<Rotation>(entity).unwrap().pitch, -12.0);
    assert_eq!(*world.get::<GameMode>(entity).unwrap(), GameMode::Adventure);
    assert_eq!(
        world
            .get::<MainInventory>(entity)
            .unwrap()
            .0
            .item(8)
            .unwrap(),
        &ItemStack::new("minecraft:diamond", 17)
    );
    assert_eq!(
        world
            .get::<ArmourInventory>(entity)
            .unwrap()
            .helmet()
            .damage,
        7
    );
    assert_eq!(world.get::<OffhandSlot>(entity).unwrap().0.count, 12);
    assert_eq!(world.get::<HeldSlot>(entity).unwrap().0, 8);
    assert_eq!(world.get::<Health>(entity).unwrap().current, 13.0);
    assert_eq!(world.get::<Health>(entity).unwrap().max, 24.0);
    assert_eq!(world.get::<Hunger>(entity).unwrap().food_level, 9);
    assert_eq!(world.get::<Hunger>(entity).unwrap().saturation, 2.5);
    assert_eq!(world.get::<Experience>(entity).unwrap().level, 7);
    assert_eq!(world.get::<Experience>(entity).unwrap().progress, 0.5);
    let main_contents = std::iter::from_fn(|| outbound_rx.try_recv().ok())
        .find_map(|packet| match packet.data {
            jolyne::valentine::McpePacketData::InventoryContentPacket(packet)
                if packet.container_id == 0 =>
            {
                Some(packet)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(main_contents.slots[8].stacksize, 17);
    assert_ne!(main_contents.slots[8].id, 0);
    drop(server);
    drop(provider);
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn synchronous_world_extension_does_not_replace_stored_column() {
    use unastar::world::ecs::ChunkManagerWorldExt;
    let directory = std::env::temp_dir().join(format!("unastar-sync-{}", uuid::Uuid::new_v4()));
    let provider = Arc::new(LevelDBWorldProvider::open(&directory, 0).unwrap());
    let mut chunk = Chunk::new(0, 0);
    chunk.set_block(1, 75, 1, *unastar::world::chunk::blocks::DIAMOND_ORE);
    provider
        .save_column(ChunkPos::new(0, 0), 0, &ChunkColumn::new(chunk))
        .await
        .unwrap();
    let mut world = World::new();
    let mut manager = ChunkManager::default();
    manager.set_provider(provider.clone());
    world.insert_resource(manager);
    let entity = world.get_or_create_chunk(0, 0);
    assert_eq!(
        world
            .get::<ChunkData>(entity)
            .unwrap()
            .inner
            .get_block(1, 75, 1),
        *unastar::world::chunk::blocks::DIAMOND_ORE
    );
    assert!(!world.get::<ChunkStateFlags>(entity).unwrap().is_dirty());
    drop(world);
    drop(provider);
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn corrupt_player_record_falls_back_without_deleting_data() {
    use bleveldb::{DB, Options, ReadOptions, WriteOptions};
    use unastar::storage::LevelDBPlayerProvider;
    let directory = std::env::temp_dir().join(format!("unastar-corrupt-{}", uuid::Uuid::new_v4()));
    let uuid = uuid::Uuid::new_v4();
    let corrupt = b"{not valid json";
    {
        let options = Options::new();
        options.create_if_missing(true);
        let db = DB::open(&directory, &options).unwrap();
        db.put(uuid.as_bytes(), corrupt, &WriteOptions::new())
            .unwrap();
    }
    let provider = LevelDBPlayerProvider::open(&directory).unwrap();
    assert!(
        unastar::server::connect::load_player_data(Some(&provider), Some(&uuid.to_string()))
            .await
            .is_none()
    );
    drop(provider);
    {
        let db = DB::open(&directory, &Options::new()).unwrap();
        assert_eq!(
            db.get(uuid.as_bytes(), &ReadOptions::new())
                .unwrap()
                .unwrap(),
            corrupt
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn storage_read_errors_never_generate_replacement_terrain() {
    use async_trait::async_trait;
    use unastar::storage::{StorageError, StorageResult};
    struct UnreadableWorld;
    #[async_trait]
    impl WorldProvider for UnreadableWorld {
        async fn load_column(&self, _: ChunkPos, _: i32) -> StorageResult<Option<ChunkColumn>> {
            Err(StorageError::Database("unreadable column".into()))
        }
        async fn save_column(&self, _: ChunkPos, _: i32, _: &ChunkColumn) -> StorageResult<()> {
            panic!("failed reads must never produce terrain to save")
        }
        async fn flush(&self) -> StorageResult<()> {
            Ok(())
        }
        async fn close(&self) -> StorageResult<()> {
            Ok(())
        }
    }
    let mut manager = ChunkManager::new(WorldConfig {
        generator: WorldGenerator::Vanilla { seed: 12345 },
        ..WorldConfig::default()
    });
    manager.set_provider(Arc::new(UnreadableWorld));
    let mut world = World::new();
    let viewer = world.spawn_empty().id();
    let receiver = manager.request_generation(0, 0, viewer).unwrap();
    assert!(matches!(
        receiver.await.unwrap(),
        Err(StorageError::Database(_))
    ));
}
