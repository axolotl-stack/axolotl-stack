use super::*;
use crate::{
    server::game::{GameServer, ServerConfig},
    storage::{LevelDBWorldProvider, WorldProvider},
    world::{
        WorldConfig,
        chunk::blocks,
        ecs::{ChunkManagerWorldExt, ChunkPendingUnload},
    },
};
use std::path::PathBuf;

struct Temp(PathBuf);
impl Temp {
    /// Unique real provider directory.
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("unastar-native-columns-{}", uuid::Uuid::new_v4())))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
/// The ordinary native host, with its real configured LevelDB writer.
fn server(dir: &Temp) -> (GameServer, Arc<ColumnWriter>) {
    let mut server = GameServer::with_config(ServerConfig {
        world: WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let provider = Arc::new(LevelDBWorldProvider::open(&dir.0, 0).unwrap());
    let writer = provider.column_writer().unwrap();
    server
        .ecs
        .world_mut()
        .resource_mut::<ChunkManager>()
        .set_provider(provider);
    (server, writer)
}
/// Poll real asynchronous IO through the same nonblocking native completion phase.
async fn finish(world: &mut World, ticket: &ColumnTicket) -> ColumnOperationStatus {
    for _ in 0..1000 {
        poll(world);
        match ticket.status() {
            ColumnOperationStatus::Pending => {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await
            }
            status => return status,
        }
    }
    panic!("column operation did not complete");
}
/// Wait until the owned worker, not a mocked provider, reaches its write boundary.
async fn entered(pause: &super::super::column_writer::leveldb::TestPause) {
    for _ in 0..1000 {
        if pause.entered.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    panic!("writer did not enter pause");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn newer_dirty_generation_is_not_cleared_by_old_save_ack() {
    let dir = Temp::new();
    let (mut server, writer) = server(&dir);
    let world = server.ecs.world_mut();
    let chunk = world.get_or_create_chunk(0, 0);
    let captured = capture(world, &[chunk]).unwrap();
    let pause = writer.pause_next();
    let ticket = save(world, captured, false).unwrap();
    entered(&pause).await;
    let before = world
        .get::<ChunkStateFlags>(chunk)
        .unwrap()
        .dirty_generation();
    world
        .get_mut::<ChunkData>(chunk)
        .unwrap()
        .inner
        .set_block(1, 20, 1, *blocks::STONE);
    world
        .get_mut::<ChunkStateFlags>(chunk)
        .unwrap()
        .mark_dirty();
    pause.release();
    assert!(matches!(
        finish(world, &ticket).await,
        ColumnOperationStatus::Applied(_)
    ));
    let flags = world.get::<ChunkStateFlags>(chunk).unwrap();
    assert!(flags.is_dirty());
    assert!(flags.dirty_generation() > before);
    assert_eq!(
        world
            .get::<ChunkData>(chunk)
            .unwrap()
            .inner
            .get_block(1, 20, 1),
        *blocks::STONE
    );
    assert!(!pinned(world, chunk));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_disk_ack_precedes_physical_publication_and_holds_real_pins() {
    let dir = Temp::new();
    let (mut server, writer) = server(&dir);
    let world = server.ecs.world_mut();
    let chunk = world.get_or_create_chunk(0, 0);
    let mut candidate = capture(world, &[chunk]).unwrap();
    candidate.columns[0]
        .column
        .chunk
        .set_block(1, 20, 1, *blocks::STONE);
    let pause = writer.pause_next();
    let ticket = commit(world, candidate, "native/first").unwrap();
    entered(&pause).await;
    assert_ne!(
        world
            .get::<ChunkData>(chunk)
            .unwrap()
            .inner
            .get_block(1, 20, 1),
        *blocks::STONE
    );
    assert!(pinned(world, chunk));
    assert!(cell_reserved(world, IVec3::new(1, 20, 1)));
    assert!(matches!(
        capture(world, &[chunk]),
        Err(ColumnWriteError::Busy)
    ));
    world.entity_mut(chunk).insert(ChunkPendingUnload::new(0));
    crate::world::ecs::systems::process_chunk_unloads(world);
    assert!(world.get::<ChunkData>(chunk).is_some());
    pause.release();
    assert!(matches!(
        finish(world, &ticket).await,
        ColumnOperationStatus::Applied(Some(_))
    ));
    assert_eq!(
        world
            .get::<ChunkData>(chunk)
            .unwrap()
            .inner
            .get_block(1, 20, 1),
        *blocks::STONE
    );
    assert!(!writer.reserved());
    assert!(!pinned(world, chunk));
    world
        .get_mut::<ChunkData>(chunk)
        .unwrap()
        .inner
        .set_block(2, 20, 1, *blocks::STONE);
    world
        .get_mut::<ChunkStateFlags>(chunk)
        .unwrap()
        .mark_dirty();
    assert_eq!(server.save_all_chunks().await, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ambiguous_write_keeps_columns_and_actor_ownership_loaded() {
    let dir = Temp::new();
    let (mut server, writer) = server(&dir);
    let world = server.ecs.world_mut();
    let chunk = world.get_or_create_chunk(0, 0);
    let cow = crate::entity::mob::spawn_mob(
        world,
        "minecraft:cow",
        glam::DVec3::new(8.0, 16.0, 8.0),
        0.0,
        crate::entity::mob::SpawnCause::Spawned,
    )
    .unwrap();
    writer.set_fault(1);
    let candidate = capture(world, &[chunk]).unwrap();
    let ticket = commit(world, candidate, "native/uncertain").unwrap();
    assert!(matches!(
        finish(world, &ticket).await,
        ColumnOperationStatus::Failed(ColumnWriteError::RecoveryRequired)
    ));
    assert!(pinned(world, chunk));
    assert!(writer.reserved());
    assert!(world.get::<ChunkData>(chunk).is_some());
    world
        .get_mut::<crate::entity::components::Health>(cow)
        .unwrap()
        .current = 3.0;
    assert!(world.entities().contains(cow));
    world.entity_mut(chunk).insert(ChunkPendingUnload::new(0));
    crate::world::ecs::systems::process_chunk_unloads(world);
    assert!(world.get::<ChunkData>(chunk).is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replacement_chunk_lifetime_cannot_receive_a_delayed_ack() {
    let dir = Temp::new();
    let (mut server, writer) = server(&dir);
    let world = server.ecs.world_mut();
    let chunk = world.get_or_create_chunk(0, 0);
    let mut candidate = capture(world, &[chunk]).unwrap();
    candidate.columns[0]
        .column
        .chunk
        .set_block(1, 20, 1, *blocks::STONE);
    let pause = writer.pause_next();
    let ticket = commit(world, candidate, "native/replacement").unwrap();
    entered(&pause).await;
    world.resource_mut::<ChunkManager>().remove_by_coords(0, 0);
    world.despawn(chunk);
    // No storage read while IO is paused: install a genuine new loaded lifetime.
    let replacement = world
        .spawn((
            ChunkPosition::new(0, 0),
            ChunkData::new(crate::world::Chunk::new(0, 0)),
            ChunkStateFlags::new_generated(),
        ))
        .id();
    world
        .resource_mut::<ChunkManager>()
        .insert(ChunkPosition::new(0, 0), replacement);
    pause.release();
    assert!(matches!(
        finish(world, &ticket).await,
        ColumnOperationStatus::Failed(ColumnWriteError::RecoveryRequired)
    ));
    assert_ne!(
        world
            .get::<ChunkData>(replacement)
            .unwrap()
            .inner
            .get_block(1, 20, 1),
        *blocks::STONE
    );
    assert!(writer.reserved());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn world_closure_leaves_disk_work_owned_and_reports_recovery() {
    let dir = Temp::new();
    let (mut server, writer) = server(&dir);
    let world = server.ecs.world_mut();
    let chunk = world.get_or_create_chunk(0, 0);
    let candidate = capture(world, &[chunk]).unwrap();
    let pause = writer.pause_next();
    let ticket = commit(world, candidate, "native/closed-world").unwrap();
    entered(&pause).await;
    drop(server);
    assert!(matches!(
        ticket.status(),
        ColumnOperationStatus::Failed(ColumnWriteError::RecoveryRequired)
    ));
    pause.release();
    writer.flush().await.unwrap();
    assert_eq!(
        writer
            .receipt("native/closed-world")
            .await
            .unwrap()
            .unwrap()
            .outcome,
        super::super::column_writer::ReceiptOutcome::Applied
    );
    assert!(writer.reserved());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn late_actor_changes_prevent_unload_after_successful_save() {
    let dir = Temp::new();
    let (mut server, writer) = server(&dir);
    let world = server.ecs.world_mut();
    let chunk = world.get_or_create_chunk(0, 0);
    let cow = crate::entity::mob::spawn_mob(
        world,
        "minecraft:cow",
        glam::DVec3::new(8.0, 16.0, 8.0),
        0.0,
        crate::entity::mob::SpawnCause::Spawned,
    )
    .unwrap();
    let candidate = capture(world, &[chunk]).unwrap();
    let pause = writer.pause_next();
    let ticket = save(world, candidate, true).unwrap();
    entered(&pause).await;
    world
        .get_mut::<crate::entity::components::Health>(cow)
        .unwrap()
        .current = 3.0;
    pause.release();
    assert!(matches!(
        finish(world, &ticket).await,
        ColumnOperationStatus::Applied(_)
    ));
    assert!(world.get::<ChunkData>(chunk).is_some());
    assert!(world.entities().contains(cow));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejected_stale_unload_keeps_the_real_dirty_generation_and_terrain() {
    let dir = Temp::new();
    let (mut server, writer) = server(&dir);
    let world = server.ecs.world_mut();
    let chunk = world.get_or_create_chunk(0, 0);
    let captured = capture(world, &[chunk]).unwrap();
    let generation = world
        .get::<ChunkStateFlags>(chunk)
        .unwrap()
        .dirty_generation();
    let newer = writer.capture().unwrap();
    writer
        .ordinary(newer, &captured.columns)
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    let ticket = save(world, captured, true).unwrap();
    assert!(matches!(
        finish(world, &ticket).await,
        ColumnOperationStatus::Failed(ColumnWriteError::Stale)
    ));
    let flags = world.get::<ChunkStateFlags>(chunk).unwrap();
    assert!(flags.is_dirty());
    assert_eq!(flags.dirty_generation(), generation);
    assert!(world.get::<ChunkData>(chunk).is_some());
    assert!(!pinned(world, chunk));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_capture_preserves_actual_actor_and_block_actor_payloads_on_restart() {
    use crate::server::game::types::BlockRegistryResource;
    use crate::world::block_entity::BlockEntity;
    use zuri_nbt::tag;
    let dir = Temp::new();
    let expected = {
        let (mut host, writer) = server(&dir);
        let world = host.ecs.world_mut();
        let chunk = world.get_or_create_chunk(0, 0);
        let cow = crate::entity::mob::spawn_mob(
            world,
            "minecraft:cow",
            glam::DVec3::new(8.0, 16.0, 8.0),
            0.0,
            crate::entity::mob::SpawnCause::Spawned,
        )
        .unwrap();
        world
            .get_mut::<crate::entity::components::Health>(cow)
            .unwrap()
            .current = 7.0;
        let chest = world
            .resource::<BlockRegistryResource>()
            .0
            .default_state_id_by_name("minecraft:chest")
            .unwrap();
        let actor = BlockEntity::from_bedrock_compound(
            tag::Compound::builder()
                .with_string("id", "Chest")
                .with_int("x", 2)
                .with_int("y", 20)
                .with_int("z", 2)
                .with_int("Marker", 419)
                .build(),
        )
        .unwrap();
        {
            let mut data = world.get_mut::<ChunkData>(chunk).unwrap();
            data.inner.set_block(2, 20, 2, chest);
            assert!(data.inner.set_block_entity(actor));
        }
        world
            .get_mut::<ChunkStateFlags>(chunk)
            .unwrap()
            .mark_dirty();
        let mut candidate = capture(world, &[chunk]).unwrap();
        let actors = candidate.columns[0].column.actors.clone();
        candidate.columns[0]
            .column
            .chunk
            .set_block(1, 20, 1, *blocks::STONE);
        let ticket = commit(world, candidate, "native/full-column").unwrap();
        assert!(matches!(
            finish(world, &ticket).await,
            ColumnOperationStatus::Applied(Some(_))
        ));
        writer.flush().await.unwrap();
        actors
    };
    let provider = {
        let mut opened = None;
        for _ in 0..1000 {
            if let Ok(provider) = LevelDBWorldProvider::open(&dir.0, 0) {
                opened = Some(provider);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        opened.expect("owned provider closed")
    };
    let loaded = provider
        .load_column(crate::world::ChunkPos::new(0, 0), 0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.actors, expected);
    assert_eq!(loaded.chunk.get_block(1, 20, 1), *blocks::STONE);
    assert_eq!(loaded.chunk.block_entities().len(), 1);
    let actor = loaded.chunk.block_entities()[0]
        .bedrock_compound()
        .unwrap()
        .unwrap();
    assert!(matches!(actor.0.get("Marker"), Some(zuri_nbt::NBTTag::Int(value)) if value.0 == 419));
    assert_eq!(
        provider
            .column_writer()
            .unwrap()
            .receipt("native/full-column")
            .await
            .unwrap()
            .unwrap()
            .epoch,
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ordinary_save_cannot_acknowledge_a_detached_mutated_candidate() {
    let dir = Temp::new();
    let (mut server, _) = server(&dir);
    let world = server.ecs.world_mut();
    let chunk = world.get_or_create_chunk(0, 0);
    let mut candidate = capture(world, &[chunk]).unwrap();
    candidate.columns[0]
        .column
        .chunk
        .set_block(1, 20, 1, *blocks::STONE);
    assert!(matches!(
        save(world, candidate, false),
        Err(ColumnWriteError::Invalid)
    ));
    assert!(world.get::<ChunkStateFlags>(chunk).unwrap().is_dirty());
    assert_ne!(
        world
            .get::<ChunkData>(chunk)
            .unwrap()
            .inner
            .get_block(1, 20, 1),
        *blocks::STONE
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn whole_column_reservation_also_blocks_a_container_partner_in_another_column() {
    use crate::server::game::{ContainerMutationLocks, types::BlockRegistryResource};
    use crate::world::block_entity::BlockEntity;
    use zuri_nbt::tag;
    let dir = Temp::new();
    let (mut server, writer) = server(&dir);
    let world = server.ecs.world_mut();
    let first = world.get_or_create_chunk(0, 0);
    let second = world.get_or_create_chunk(1, 0);
    let chest = world
        .resource::<BlockRegistryResource>()
        .0
        .default_state_id_by_name("minecraft:chest")
        .unwrap();
    for (chunk, x, other) in [(first, 15, 16), (second, 16, 15)] {
        let actor = BlockEntity::from_bedrock_compound(
            tag::Compound::builder()
                .with_string("id", "Chest")
                .with_int("x", x)
                .with_int("y", 20)
                .with_int("z", 2)
                .with_int("pairx", other)
                .with_int("pairz", 2)
                .build(),
        )
        .unwrap();
        let mut data = world.get_mut::<ChunkData>(chunk).unwrap();
        data.inner.set_block((x & 15) as u8, 20, 2, chest);
        assert!(data.inner.set_block_entity(actor));
    }
    world.init_resource::<ContainerMutationLocks>();
    assert!(world.resource::<ContainerMutationLocks>().is_empty());
    let candidate = capture(world, &[second]).unwrap();
    let pause = writer.pause_next();
    let ticket = commit(world, candidate, "native/paired-owner").unwrap();
    entered(&pause).await;
    assert!(crate::server::game::container_cell_locked(
        world,
        IVec3::new(15, 20, 2)
    ));
    assert!(
        crate::server::game::blocks::try_set_block(
            world,
            IVec3::new(15, 20, 2),
            *blocks::STONE,
            false
        )
        .is_err()
    );
    assert_eq!(
        world
            .get::<ChunkData>(first)
            .unwrap()
            .inner
            .get_block(15, 20, 2),
        chest
    );
    pause.release();
    assert!(matches!(
        finish(world, &ticket).await,
        ColumnOperationStatus::Applied(_)
    ));
    assert!(!crate::server::game::container_cell_locked(
        world,
        IVec3::new(15, 20, 2)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mandatory_save_reports_unaccepted_terrain_without_clearing_its_buffer() {
    let dir = Temp::new();
    let (mut server, writer) = server(&dir);
    let chunk = server.ecs.world_mut().get_or_create_chunk(0, 0);
    writer.close().await.unwrap();
    assert!(matches!(
        server.try_save_all_chunks().await,
        Err(crate::storage::StorageError::ColumnWrite(
            ColumnWriteError::Closed
        ))
    ));
    assert!(server.ecs.world().get::<ChunkData>(chunk).is_some());
    assert!(
        server
            .ecs
            .world()
            .get::<ChunkStateFlags>(chunk)
            .unwrap()
            .is_dirty()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owner_lost_after_io_acceptance_requires_the_real_receipt_and_original_payload() {
    let dir = Temp::new();
    let (mut server, writer) = server(&dir);
    let owner = Arc::new(());
    let world = server.ecs.world_mut();
    let chunk = world.get_or_create_chunk(0, 0);
    let mut candidate = capture(world, &[chunk]).unwrap();
    let before = candidate.columns[0].column.chunk.get_block(1, 20, 1);
    candidate.columns[0]
        .column
        .chunk
        .set_block(1, 20, 1, *blocks::STONE);
    let pause = writer.pause_next();
    let ticket = commit_scoped(
        world,
        candidate,
        "scoped/one",
        [9; 32],
        Arc::downgrade(&owner),
        vec![ColumnBlockChange {
            position: IVec3::new(1, 20, 1),
            before,
            after: *blocks::STONE,
        }],
    )
    .unwrap();
    entered(&pause).await;
    drop(owner);
    pause.release();
    assert!(matches!(
        finish(world, &ticket).await,
        ColumnOperationStatus::Failed(ColumnWriteError::RecoveryRequired)
    ));
    assert_eq!(
        world
            .get::<ChunkData>(chunk)
            .unwrap()
            .inner
            .get_block(1, 20, 1),
        before
    );
    assert!(pinned(world, chunk));
    let resumed = Arc::new(());
    assert!(matches!(
        reconcile_scoped(world, "scoped/missing", [9; 32], Arc::downgrade(&resumed)),
        Err(ColumnWriteError::RecoveryRequired)
    ));
    let recovered =
        reconcile_scoped(world, "scoped/one", [9; 32], Arc::downgrade(&resumed)).unwrap();
    assert!(matches!(
        recovered.status(),
        ColumnOperationStatus::Applied(Some(_))
    ));
    assert_eq!(
        world
            .get::<ChunkData>(chunk)
            .unwrap()
            .inner
            .get_block(1, 20, 1),
        *blocks::STONE
    );
    assert!(!pinned(world, chunk));
}
