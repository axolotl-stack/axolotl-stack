use super::*;

fn settings(root: &Path) -> (WorldStorageConfig, WorldConfig) {
    (
        WorldStorageConfig {
            data_dir: root.to_path_buf(),
            ..Default::default()
        },
        WorldConfig::void_spawn_platform(1),
    )
}

#[test]
fn world_manifest_preserves_identity_and_rejects_changed_terrain_or_live_provider_alias() {
    let directory = tempfile::tempdir().unwrap();
    let (storage, world) = settings(directory.path());
    let (identity, provider) = open(&storage, world).unwrap();
    assert!(open(&storage, world).is_err());
    let retained = provider.clone();
    drop(provider);
    assert!(open(&storage, world).is_err());
    drop(retained);
    let (restored, provider) = open(&storage, world).unwrap();
    assert_eq!(restored, identity);
    drop(provider);
    let mut changed = world;
    changed.generator = WorldGenerator::SuperFlat;
    assert!(open(&storage, changed).is_err());
    let mut renamed = storage.clone();
    renamed.name = "renamed".into();
    assert!(open(&renamed, world).is_err());
    let (restored, _) = open(&storage, world).unwrap();
    assert_eq!(restored, identity);
}

#[test]
fn legacy_data_requires_explicit_adoption_and_malformed_manifest_never_resets_it() {
    let directory = tempfile::tempdir().unwrap();
    let (mut storage, world) = settings(directory.path());
    let database = directory.path().join("db");
    fs::create_dir(&database).unwrap();
    fs::write(database.join("existing-data"), b"preserve").unwrap();
    assert!(open(&storage, world).is_err());
    assert!(!directory.path().join(MANIFEST).exists());
    assert_eq!(
        fs::read(database.join("existing-data")).unwrap(),
        b"preserve"
    );
    fs::remove_file(database.join("existing-data")).unwrap();
    let provider = crate::storage::LevelDBWorldProvider::open(&database, 0).unwrap();
    drop(provider);
    assert!(open(&storage, world).is_err());
    storage.adopt_existing = true;
    let (_, provider) = open(&storage, world).unwrap();
    drop(provider);
    fs::write(directory.path().join(MANIFEST), "broken").unwrap();
    assert!(open(&storage, world).is_err());
    assert_eq!(
        fs::read_to_string(directory.path().join(MANIFEST)).unwrap(),
        "broken"
    );
}

#[test]
fn seed_dimension_bounds_and_provider_changes_are_never_adopted_over_an_existing_manifest() {
    let directory = tempfile::tempdir().unwrap();
    let (mut storage, mut world) = settings(directory.path());
    world.generator = WorldGenerator::JavaTerrain {
        seed: 7,
        preset: crate::world::generator::JavaDimension::Overworld,
    };
    let (identity, provider) = open(&storage, world).unwrap();
    drop(provider);
    storage.adopt_existing = true;
    let original = fs::read(directory.path().join(MANIFEST)).unwrap();
    for changed in [
        WorldConfig {
            generator: WorldGenerator::JavaTerrain {
                seed: 8,
                preset: crate::world::generator::JavaDimension::Overworld,
            },
            ..world
        },
        WorldConfig {
            dimension: 1,
            ..world
        },
        WorldConfig {
            bounds: WorldBounds::Radius { radius_chunks: 2 },
            ..world
        },
        WorldConfig {
            storage_provider: StorageProvider::BlazeDb,
            ..world
        },
    ] {
        assert!(open(&storage, changed).is_err());
    }
    assert_eq!(fs::read(directory.path().join(MANIFEST)).unwrap(), original);
    let (restored, _) = open(&storage, world).unwrap();
    assert_eq!(restored, identity);
}

#[tokio::test]
async fn both_world_providers_preserve_actual_saved_blocks_and_identity_across_restart() {
    for kind in [StorageProvider::LevelDb, StorageProvider::BlazeDb] {
        let directory = tempfile::tempdir().unwrap();
        let (storage, mut world) = settings(directory.path());
        world.storage_provider = kind;
        let (identity, provider) = open(&storage, world).unwrap();
        let position = ChunkPos::new(2, -3);
        let mut chunk = crate::world::Chunk::new(position.x, position.z);
        chunk.set_block(1, 64, 2, *crate::world::chunk::blocks::DIRT);
        provider
            .save_column(position, 0, &ChunkColumn::new(chunk))
            .await
            .unwrap();
        provider.flush().await.unwrap();
        provider.close().await.unwrap();
        drop(provider);
        let (restored, provider) = open(&storage, world).unwrap();
        assert_eq!(restored, identity);
        let loaded = provider.load_column(position, 0).await.unwrap().unwrap();
        assert_eq!(
            loaded.chunk.get_block(1, 64, 2),
            *crate::world::chunk::blocks::DIRT
        );
        provider.close().await.unwrap();
    }
}

fn runtime_config(root: &Path) -> UnastarConfig {
    let mut config = UnastarConfig {
        world: WorldConfig::void_spawn_platform(1),
        ..Default::default()
    };
    config.world_storage.data_dir = root.join("primary");
    config.players.data_dir = root.join("players");
    for (dimension, name) in [(1, "lower"), (2, "upper")] {
        config
            .secondary_worlds
            .push(crate::config::SecondaryWorldConfig {
                name: name.into(),
                data_dir: root.join(name),
                adopt_existing: false,
                world: WorldConfig {
                    dimension,
                    ..WorldConfig::void_spawn_platform(1)
                },
                spawn_rules: vec![crate::config::SpawnRule {
                    always_at_location: true,
                    location: Some(crate::config::SpawnLocation::default()),
                    ..Default::default()
                }],
            });
    }
    config
}

#[tokio::test]
async fn ordinary_runtime_loads_three_isolated_worlds_with_one_serialized_account_provider() {
    use crate::server::game::inventory_effects::SharedPlayerWriter;
    use crate::server::game::types::PlayerProviderResource;
    use crate::world::ecs::ChunkManager;
    let directory = tempfile::tempdir().unwrap();
    let config = runtime_config(directory.path());
    let mut runtime = super::super::UnastarServer::new(Arc::new(config.clone()))
        .await
        .unwrap();
    assert_eq!(
        runtime
            .worlds()
            .iter()
            .map(|(dimension, _)| dimension)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    let primary = runtime.game_server().ecs.world();
    let account = primary.resource::<PlayerProviderResource>().0.clone();
    let terrain = primary.resource::<ChunkManager>().provider().unwrap();
    let ids: Vec<_> = runtime
        .worlds()
        .iter()
        .map(|(_, server)| server.ecs.world().resource::<WorldIdentity>().clone())
        .collect();
    let writer = crate::server::game::inventory_effects::prepare_shared_writer(
        runtime.game_server_mut().ecs.world_mut(),
    )
    .unwrap();
    for (dimension, server) in runtime.worlds_mut().iter_mut() {
        let shared =
            crate::server::game::inventory_effects::prepare_shared_writer(server.ecs.world_mut())
                .unwrap();
        let world = server.ecs.world();
        assert!(Arc::ptr_eq(
            &account,
            &world.resource::<PlayerProviderResource>().0
        ));
        assert!(Arc::ptr_eq(&writer, &shared));
        assert!(world.contains_resource::<SharedPlayerWriter>());
        if dimension != 0 {
            assert!(!Arc::ptr_eq(
                &terrain,
                &world.resource::<ChunkManager>().provider().unwrap()
            ));
        }
        assert_eq!(world.resource::<WorldIdentity>().dimension, dimension);
        let provider = world.resource::<ChunkManager>().provider().unwrap();
        let mut chunk = crate::world::Chunk::new(0, 0);
        let block = [
            *crate::world::chunk::blocks::STONE,
            *crate::world::chunk::blocks::DIRT,
            *crate::world::chunk::blocks::WATER,
        ][dimension as usize];
        chunk.set_block(1, 64, 2, block);
        provider
            .save_column(ChunkPos::new(0, 0), dimension, &ChunkColumn::new(chunk))
            .await
            .unwrap();
        provider.flush().await.unwrap();
    }
    drop((account, writer, terrain));
    drop(runtime);
    let restored = super::super::UnastarServer::new(Arc::new(config.clone()))
        .await
        .unwrap();
    assert_eq!(
        restored
            .worlds()
            .iter()
            .map(|(_, server)| server.ecs.world().resource::<WorldIdentity>().clone())
            .collect::<Vec<_>>(),
        ids
    );
    for (dimension, server) in restored.worlds().iter() {
        let (column, loaded) = server
            .ecs
            .world()
            .resource::<ChunkManager>()
            .try_load_or_generate_chunk(0, 0)
            .unwrap();
        assert!(
            loaded,
            "existing configured terrain must load before generation"
        );
        let expected = [
            *crate::world::chunk::blocks::STONE,
            *crate::world::chunk::blocks::DIRT,
            *crate::world::chunk::blocks::WATER,
        ][dimension as usize];
        assert_eq!(column.chunk.get_block(1, 64, 2), expected);
    }
    drop(restored);
    fs::write(config.world_storage.data_dir.join(MANIFEST), "broken").unwrap();
    assert!(
        super::super::UnastarServer::new(Arc::new(config.clone()))
            .await
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(config.world_storage.data_dir.join(MANIFEST)).unwrap(),
        "broken"
    );
}

#[tokio::test]
async fn duplicate_persisted_world_ids_stop_the_ordinary_runtime_before_startup() {
    let directory = tempfile::tempdir().unwrap();
    let config = runtime_config(directory.path());
    let runtime = super::super::UnastarServer::new(Arc::new(config.clone()))
        .await
        .unwrap();
    drop(runtime);
    let primary: Manifest = toml_edit::de::from_str(
        &fs::read_to_string(config.world_storage.data_dir.join(MANIFEST)).unwrap(),
    )
    .unwrap();
    let secondary_path = config.secondary_worlds[0].data_dir.join(MANIFEST);
    let mut secondary: Manifest =
        toml_edit::de::from_str(&fs::read_to_string(&secondary_path).unwrap()).unwrap();
    secondary.id = primary.id;
    fs::write(
        &secondary_path,
        toml_edit::ser::to_string(&secondary).unwrap(),
    )
    .unwrap();
    assert!(
        super::super::UnastarServer::new(Arc::new(config))
            .await
            .is_err()
    );
}

#[test]
fn aliased_nested_and_player_storage_directories_are_rejected_before_provider_open() {
    let directory = tempfile::tempdir().unwrap();
    let mut config = runtime_config(directory.path());
    config.secondary_worlds[0].data_dir = config.world_storage.data_dir.join("nested");
    assert!(validate_directories(&config).is_err());
    config.secondary_worlds[0].data_dir = config.players.data_dir.clone();
    assert!(validate_directories(&config).is_err());
    #[cfg(unix)]
    {
        config.secondary_worlds[0].data_dir = directory.path().join("alias");
        std::os::unix::fs::symlink(
            &config.world_storage.data_dir,
            &config.secondary_worlds[0].data_dir,
        )
        .unwrap();
        assert!(validate_directories(&config).is_err());
    }
}
