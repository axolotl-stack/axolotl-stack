//! Native migrations exercise real world ownership and transport boundaries.

use super::*;
use crate::entity::components::*;
use crate::server::game::{self, player_events, player_state, types};
use crate::world::ecs::ChunkManagerWorldExt;
use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::mpsc;

fn server(dimension: i32) -> crate::GameServer {
    crate::GameServer::with_config(crate::ServerConfig {
        world: crate::world::WorldConfig {
            dimension,
            ..crate::world::WorldConfig::void_spawn_platform(1)
        },
        ..Default::default()
    })
}
fn router() -> DimensionRouter {
    let mut router = DimensionRouter::new(server(0));
    router.add(server(1)).unwrap();
    router.add(server(2)).unwrap();
    router
}
fn spawn(
    router: &mut DimensionRouter,
    dimension: i32,
    session: u64,
) -> (Entity, mpsc::Receiver<McpePacket>) {
    let (tx, mut rx) = mpsc::channel(8192);
    let player = router
        .spawn_player_in(
            dimension,
            crate::PlayerSpawnData {
                session_id: session,
                display_name: format!("actor{session}"),
                xuid: Some(format!("account{session}")),
                uuid: Some(uuid::Uuid::from_u128(session as u128).to_string()),
                runtime_id: session as i64 + 200,
                position: DVec3::new(4.5, 17.62, 4.5),
                saved_data: None,
                outbound_tx: tx,
                chunk_radius: 0,
            },
        )
        .unwrap();
    while rx.try_recv().is_ok() {}
    (player, rx)
}
fn request(router: &mut DimensionRouter, player: Entity, dimension: i32) {
    request_transfer_checked(
        router.world_mut(0).unwrap().ecs.world_mut(),
        player,
        dimension,
        DVec3::new(4.5, 16.0, 4.5),
        0,
        "request".into(),
        |_, _, _| true,
    )
    .unwrap();
}
fn route(router: &mut DimensionRouter, session: u64, packet: McpePacket) {
    let mut bytes = bytes::BytesMut::new();
    packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
    router.route_packet(
        session,
        McpePacketArgs,
        BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap(),
    );
}
fn packets(rx: &mut mpsc::Receiver<McpePacket>) -> Vec<McpePacketData> {
    let mut packets = Vec::new();
    while let Ok(packet) = rx.try_recv() {
        packets.push(packet.data);
    }
    packets
}
#[derive(Component)]
struct Owned(String);
#[derive(Default)]
struct Observations {
    commits: Vec<PlayerWorldTransferred>,
    failures: Vec<TransferError>,
}
struct Observe(Arc<Mutex<Observations>>);
impl WorldTransferHandler for Observe {
    fn on_committed(&self, source: &mut World, target: &mut World, event: &PlayerWorldTransferred) {
        assert!(source.get_entity(event.previous_player).is_err());
        assert!(event.identity().matches(target, event.player));
        assert!(player_events::initial_state_pending(target, event.player));
        self.0.lock().unwrap().commits.push(event.clone());
    }
    fn on_failed(&self, _: &mut World, event: &WorldTransferFailed) {
        self.0.lock().unwrap().failures.push(event.error.clone());
    }
}
fn observe(router: &mut DimensionRouter) -> Arc<Mutex<Observations>> {
    let observed = Arc::new(Mutex::new(Observations::default()));
    router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .get_resource_or_init::<WorldTransferHooks>()
        .register(Observe(observed.clone()));
    observed
}

#[tokio::test]
async fn migration_moves_owned_state_routes_real_world_input_and_waits_for_matching_loading_ack() {
    let mut router = router();
    let (player, mut rx) = spawn(&mut router, 0, 1);
    let observed = observe(&mut router);
    let world = router.world_mut(0).unwrap().ecs.world_mut();
    world
        .resource_mut::<TransferComponents>()
        .preserve::<Owned>();
    world
        .entity_mut(player)
        .insert(Owned("owned context".into()));
    let mut stack = crate::item::test_support::stack("minecraft:diamond", 7);
    stack.net_id = 1234;
    world
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(3, stack)
        .unwrap();
    world.get_mut::<PlayerInput>(player).unwrap().tick = 123;
    crate::command::exec::set_mode_override(
        world,
        player,
        Some(crate::entity::components::GameMode::Creative),
    );
    packets(&mut rx);
    let old = player_state::capture(world, player).unwrap();
    request(&mut router, player, 1);
    router.tick().await.unwrap();
    let (dimension, target) = router.locate_session(1).expect("target owns transport");
    assert_eq!(dimension, 1);
    assert!(
        router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get_entity(player)
            .is_err()
    );
    let target_world = router.world(1).unwrap().ecs.world();
    assert_eq!(
        target_world.get::<Owned>(target).unwrap().0,
        "owned context"
    );
    assert_eq!(
        target_world
            .get::<MainInventory>(target)
            .unwrap()
            .0
            .item(3)
            .unwrap()
            .net_id,
        1234
    );
    let current = player_state::capture(target_world, target).unwrap();
    assert_eq!(current.data.inventory, old.data.inventory);
    assert_eq!(current.base_game_mode, Some(0));
    assert_eq!(current.data.game_mode, 1);
    assert_eq!(current.data.dimension, 1);
    assert_eq!(target_world.get::<PlayerInput>(target).unwrap().tick, 123);
    assert!(!player_events::initial_state_pending(target_world, target));
    assert!(is_loading(target_world, target));
    assert_eq!(observed.lock().unwrap().commits.len(), 1);
    let outgoing = packets(&mut rx);
    let loading_id = outgoing
        .iter()
        .find_map(|packet| {
            if let McpePacketData::ChangeDimensionPacket(change) = packet {
                assert_eq!(change.dimension_id.value, 1);
                change.loading_screen_id
            } else {
                None
            }
        })
        .unwrap();
    assert!(outgoing.iter().any(|packet|matches!(packet,McpePacketData::PlayerActionPacket(action) if action.action==EnumsPlayerActionType::Changedimensionack)));
    route(
        &mut router,
        1,
        McpePacket::from(ServerboundLoadingScreenPacket {
            loading_screen_packet_type: EnumsServerboundLoadingScreenPacketType::Endloadingscreen,
            loading_screen_id: Some(loading_id + 1),
        }),
    );
    assert!(is_loading(router.world(1).unwrap().ecs.world(), target));
    route(
        &mut router,
        1,
        McpePacket::from(ServerboundLoadingScreenPacket {
            loading_screen_packet_type: EnumsServerboundLoadingScreenPacketType::Endloadingscreen,
            loading_screen_id: Some(loading_id),
        }),
    );
    assert!(!is_loading(router.world(1).unwrap().ecs.world(), target));
    router.despawn_player(1);
    assert!(router.locate_session(1).is_none());
}

struct Veto;
impl WorldTransferHandler for Veto {
    fn on_transfer(&self, _: &mut World, _: &mut World, event: &mut PlayerWorldTransfer) {
        event.cancel();
    }
}
#[tokio::test]
async fn late_veto_never_moves_entity_or_emits_dimension_packets_and_registry_mutation_is_safe() {
    let mut router = router();
    let (player, mut rx) = spawn(&mut router, 0, 1);
    let observed = observe(&mut router);
    router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .get_resource_or_init::<WorldTransferHooks>()
        .register(Veto);
    request(&mut router, player, 1);
    router.tick().await.unwrap();
    assert_eq!(router.locate_session(1), Some((0, player)));
    assert_eq!(
        observed.lock().unwrap().failures,
        vec![TransferError::Rejected]
    );
    assert!(observed.lock().unwrap().commits.is_empty());
    assert!(
        !packets(&mut rx)
            .iter()
            .any(|packet| matches!(packet, McpePacketData::ChangeDimensionPacket(_)))
    );
}
struct MutateSource;
impl WorldTransferHandler for MutateSource {
    fn on_transfer(&self, source: &mut World, _: &mut World, event: &mut PlayerWorldTransfer) {
        source
            .get_mut::<MainInventory>(event.player())
            .unwrap()
            .0
            .set_item(0, crate::item::test_support::stack("minecraft:stick", 1))
            .unwrap();
    }
}
#[tokio::test]
async fn nested_source_mutation_is_preserved_instead_of_overwritten_by_stale_snapshot() {
    let mut router = router();
    let (player, _rx) = spawn(&mut router, 0, 1);
    let observed = observe(&mut router);
    router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .get_resource_or_init::<WorldTransferHooks>()
        .register(MutateSource);
    request(&mut router, player, 1);
    router.tick().await.unwrap();
    assert_eq!(router.locate_session(1), Some((0, player)));
    assert_eq!(
        observed.lock().unwrap().failures,
        vec![TransferError::StaleSource]
    );
    assert_eq!(
        router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .count,
        1
    );
}
#[tokio::test]
async fn queued_request_cannot_follow_a_replaced_transport_even_when_numeric_session_is_reused() {
    let mut router = router();
    let (player, _rx) = spawn(&mut router, 0, 1);
    let observed = observe(&mut router);
    request(&mut router, player, 1);
    let world = router.world_mut(0).unwrap().ecs.world_mut();
    let old = world.get::<PlayerSession>(player).unwrap();
    let (tx, _rx2) = mpsc::channel(8192);
    let replacement = PlayerSession::new(
        old.session_id,
        old.display_name.clone(),
        old.xuid.clone(),
        old.uuid.clone(),
        tx,
    );
    world.entity_mut(player).insert(replacement);
    router.tick().await.unwrap();
    assert_eq!(router.locate_session(1), Some((0, player)));
    assert_eq!(
        observed.lock().unwrap().failures,
        vec![TransferError::StaleSource]
    );
}
#[tokio::test]
async fn unknown_components_reject_without_dropping_owned_data() {
    let mut router = router();
    let (player, _rx) = spawn(&mut router, 0, 1);
    let observed = observe(&mut router);
    router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .entity_mut(player)
        .insert(Owned("keep".into()));
    request(&mut router, player, 1);
    router.tick().await.unwrap();
    assert!(matches!(
        &observed.lock().unwrap().failures[..],
        [TransferError::UnsupportedComponents(_)]
    ));
    assert_eq!(
        router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get::<Owned>(player)
            .unwrap()
            .0,
        "keep"
    );
}
#[tokio::test]
async fn prepared_destination_uses_real_loaded_collision_surfaces_and_rejects_void_or_liquid() {
    let mut server = server(1);
    let world = server.ecs.world_mut();
    world.get_or_create_chunk(0, 0);
    assert!(is_safe_destination(world, DVec3::new(4.5, 16.0, 4.5)));
    assert_eq!(
        find_safe_destination(world, DVec3::new(4.5, 19.0, 4.5), 4),
        Some(DVec3::new(4.5, 16.0, 4.5))
    );
    assert!(!is_safe_destination(world, DVec3::new(4.5, 30.0, 4.5)));
    assert!(!is_safe_destination(world, DVec3::new(400.5, 16.0, 4.5)));
    let lava = world
        .resource::<types::BlockRegistryResource>()
        .0
        .get_by_name("minecraft:lava")
        .unwrap()
        .default_state_id;
    game::blocks::set_block(world, glam::IVec3::new(4, 16, 4), lava, false);
    assert!(!is_safe_destination(world, DVec3::new(4.5, 16.0, 4.5)));
}
struct CountTicks(Arc<AtomicUsize>);
impl RouterTickHandler for CountTicks {
    fn on_tick(&self, router: &mut DimensionRouter, directory: &WorldDirectory) {
        assert_eq!(directory.players().count(), 2);
        assert_eq!(router.iter().count(), 3);
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[tokio::test]
async fn aggregate_ticks_and_queued_crossworld_effects_use_fresh_transport_bound_directory() {
    let mut router = router();
    let (alice, _a) = spawn(&mut router, 0, 1);
    let (bob, _b) = spawn(&mut router, 1, 2);
    let ticks = Arc::new(AtomicUsize::new(0));
    router.register_tick_handler(CountTicks(ticks.clone()));
    assert!(queue_router_request(
        router.world_mut(0).unwrap().ecs.world_mut(),
        move |router, directory| {
            let target = directory.session(2).unwrap();
            let (dimension, entity) = router.resolve(target).unwrap();
            assert_eq!((dimension, entity), (1, bob));
            router
                .world_mut(dimension)
                .unwrap()
                .ecs
                .world_mut()
                .get_mut::<Health>(entity)
                .unwrap()
                .current = 13.0;
            let source = directory.actor(0, alice).unwrap().clone();
            assert!(router.resolve(&source).is_some());
        }
    ));
    router.tick().await.unwrap();
    assert_eq!(ticks.load(Ordering::SeqCst), 1);
    assert_eq!(
        router
            .world(1)
            .unwrap()
            .ecs
            .world()
            .get::<Health>(bob)
            .unwrap()
            .current,
        13.0
    );
    let directory = router.refresh_directory();
    let stale = directory.session(2).unwrap().clone();
    router.despawn_player(2);
    let (_replacement, _rx) = spawn(&mut router, 1, 2);
    assert!(router.resolve(&stale).is_none());
}
#[test]
fn duplicate_world_and_sessions_and_incompatible_registry_are_rejected() {
    let mut router = DimensionRouter::new(server(0));
    assert_eq!(router.add(server(0)), Err(TransferError::InvalidDimension));
    let mut target = server(1);
    Arc::make_mut(
        &mut target
            .ecs
            .world_mut()
            .resource_mut::<types::ServerWorldTemplate>()
            .0,
    )
    .start_game_template
    .block_properties
    .push(ServerBlockProperty::default());
    assert_eq!(
        router.add(target),
        Err(TransferError::IncompatibleRegistries)
    );
    let mut target = server(1);
    let mut template = target
        .ecs
        .world_mut()
        .resource_mut::<types::ServerWorldTemplate>();
    let template = Arc::make_mut(&mut template.0);
    template.start_game_template.block_network_ids_are_hashes =
        !template.start_game_template.block_network_ids_are_hashes;
    assert_eq!(
        router.add(target),
        Err(TransferError::IncompatibleRegistries)
    );
    let (_player, _rx) = spawn(&mut router, 0, 1);
    let (tx, _rx) = mpsc::channel(1);
    assert!(
        router
            .spawn_player_in(
                0,
                crate::PlayerSpawnData {
                    session_id: 1,
                    display_name: "duplicate".into(),
                    xuid: None,
                    uuid: None,
                    runtime_id: 1,
                    position: DVec3::ZERO,
                    saved_data: None,
                    outbound_tx: tx,
                    chunk_radius: 0
                }
            )
            .is_err()
    );
}
#[tokio::test]
async fn terminal_account_fence_cannot_be_bypassed_by_reconnecting_into_another_world() {
    let mut router = router();
    let (player, _rx) = spawn(&mut router, 0, 1);
    let source = game::inventory_effects::RecoverySource::capture(
        router.world(0).unwrap().ecs.world(),
        player,
    )
    .unwrap();
    game::inventory_effects::fence_for_recovery(
        router.world_mut(0).unwrap().ecs.world_mut(),
        &source,
    )
    .unwrap();
    router.despawn_player(1);
    let (tx, _rx) = mpsc::channel(128);
    let result = router.spawn_player_in(
        1,
        crate::PlayerSpawnData {
            session_id: 2,
            display_name: "actor".into(),
            xuid: Some("account1".into()),
            uuid: Some(uuid::Uuid::from_u128(1).to_string()),
            runtime_id: 2,
            position: DVec3::new(4.5, 17.62, 4.5),
            saved_data: None,
            outbound_tx: tx,
            chunk_radius: 0,
        },
    );
    assert!(matches!(
        result,
        Err(crate::storage::StorageError::PlayerRecoveryRequired)
    ));
}

#[derive(Default)]
struct RecordingProvider {
    record: Mutex<Option<crate::storage::PlayerData>>,
    ambiguous: bool,
}
#[async_trait::async_trait]
impl crate::storage::PlayerProvider for RecordingProvider {
    fn atomic_durable_records(&self) -> bool {
        true
    }
    async fn load(
        &self,
        _: uuid::Uuid,
    ) -> crate::storage::StorageResult<Option<crate::storage::PlayerData>> {
        Ok(self.record.lock().unwrap().clone())
    }
    async fn save(
        &self,
        _: uuid::Uuid,
        data: &crate::storage::PlayerData,
    ) -> crate::storage::StorageResult<()> {
        *self.record.lock().unwrap() = Some(data.clone());
        if self.ambiguous {
            Err(crate::storage::StorageError::Database(
                "write acknowledgement unavailable".into(),
            ))
        } else {
            Ok(())
        }
    }
    async fn delete(&self, _: uuid::Uuid) -> crate::storage::StorageResult<()> {
        *self.record.lock().unwrap() = None;
        Ok(())
    }
    async fn close(&self) -> crate::storage::StorageResult<()> {
        Ok(())
    }
}
#[tokio::test]
async fn ambiguous_destination_record_fences_source_and_does_not_publish_or_erase_saved_destination()
 {
    let provider = Arc::new(RecordingProvider {
        ambiguous: true,
        ..Default::default()
    });
    let mut primary = server(0);
    primary
        .ecs
        .world_mut()
        .insert_resource(types::PlayerProviderResource(provider.clone()));
    let mut router = DimensionRouter::new(primary);
    router.add(server(1)).unwrap();
    let (player, mut rx) = spawn(&mut router, 0, 1);
    let observed = observe(&mut router);
    request(&mut router, player, 1);
    router.tick().await.unwrap();
    assert_eq!(router.locate_session(1), Some((0, player)));
    assert!(game::inventory_effects::is_recovery_fenced(
        router.world(0).unwrap().ecs.world(),
        player
    ));
    assert_eq!(
        observed.lock().unwrap().failures,
        vec![TransferError::RecoveryRequired]
    );
    assert_eq!(
        provider.record.lock().unwrap().as_ref().unwrap().dimension,
        1
    );
    assert!(!router.save_player(1).await);
    assert!(
        !packets(&mut rx)
            .iter()
            .any(|packet| matches!(packet, McpePacketData::ChangeDimensionPacket(_)))
    );
    assert!(
        router
            .refresh_directory()
            .session(1)
            .is_some_and(|player| player.availability == PlayerAvailability::Recovering)
    );
}
#[tokio::test]
async fn durable_provider_reopen_and_reconnect_keep_actual_world_items_and_base_mode() {
    use crate::storage::PlayerProvider;
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(crate::storage::LevelDBPlayerProvider::open(directory.path()).unwrap());
    let mut primary = server(0);
    primary
        .ecs
        .world_mut()
        .insert_resource(types::PlayerProviderResource(provider.clone()));
    let mut router = DimensionRouter::new(primary);
    router.add(server(1)).unwrap();
    let (player, _rx) = spawn(&mut router, 0, 1);
    let world = router.world_mut(0).unwrap().ecs.world_mut();
    world
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(0, crate::item::test_support::stack("minecraft:emerald", 9))
        .unwrap();
    crate::command::exec::set_mode_override(
        world,
        player,
        Some(crate::entity::components::GameMode::Creative),
    );
    request(&mut router, player, 1);
    router.tick().await.unwrap();
    assert_eq!(router.locate_session(1).unwrap().0, 1);
    assert!(router.save_player(1).await);
    router.despawn_player(1);
    drop(router);
    drop(provider);
    let reopened = crate::storage::LevelDBPlayerProvider::open(directory.path()).unwrap();
    let saved = reopened
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.dimension, 1);
    assert_eq!(saved.game_mode, 0);
    assert_eq!(saved.inventory[0].count, 9);
    let config = crate::config::UnastarConfig::default();
    let dimension =
        crate::server::connect::select_join_dimension(&config, 0, Some(saved.dimension), None);
    assert_eq!(dimension, 1);
    let mut router = self::router();
    let (tx, _rx) = mpsc::channel(8192);
    let player = router
        .spawn_player_in(
            dimension,
            crate::PlayerSpawnData {
                session_id: 2,
                display_name: "reconnect".into(),
                xuid: Some("account1".into()),
                uuid: Some(saved.uuid.clone()),
                runtime_id: 201,
                position: DVec3::from_array(saved.position),
                saved_data: Some(saved),
                outbound_tx: tx,
                chunk_radius: 0,
            },
        )
        .unwrap();
    let facts = player_state::capture(router.world(1).unwrap().ecs.world(), player).unwrap();
    assert_eq!(facts.data.dimension, 1);
    assert_eq!(facts.data.inventory[0].count, 9);
    assert_eq!(facts.data.game_mode, 0);
    assert_eq!(facts.base_game_mode, None);
}
#[test]
fn fixed_spawn_rule_order_deliberately_overrides_saved_dimension() {
    let mut config = crate::config::UnastarConfig::default();
    assert_eq!(
        crate::server::connect::select_join_dimension(&config, 0, Some(2), Some(1)),
        2
    );
    config.spawn_rules.insert(
        0,
        crate::config::SpawnRule {
            always_at_location: true,
            previous_position: false,
            location: Some(Default::default()),
            ..Default::default()
        },
    );
    assert_eq!(
        crate::server::connect::select_join_dimension(&config, 0, Some(2), Some(1)),
        0
    );
}

#[test]
fn dimension_chunk_streams_and_data3d_use_client_vertical_ranges() {
    use crate::world::chunk::{Chunk, blocks};
    let mut chunk = Chunk::new(0, 0);
    chunk.set_biome(7);
    chunk.set_block(1, 0, 1, *blocks::STONE);
    chunk.set_block(1, 319, 1, *blocks::STONE);
    assert_eq!(chunk.highest_subchunk_for_dimension(1), 1);
    assert_eq!(chunk.highest_subchunk_for_dimension(2), 1);
    assert_eq!(chunk.highest_subchunk_for_dimension(0), 24);
    for (dimension, count) in [(0, 24), (1, 8), (2, 16)] {
        // Independent single-value network biome fixture: header, signed VarInt ID,
        // then a single zero border count after exactly this dimension's range.
        let expected: Vec<u8> = std::iter::repeat_n([1, 14], count)
            .flatten()
            .chain([0])
            .collect();
        assert_eq!(chunk.encode_biomes_for_dimension(dimension), expected);
        let disk = chunk.encode_biomes_disk_for_dimension(dimension);
        assert_eq!(&disk[512..517], &[1, 7, 0, 0, 0]);
        assert_eq!(&disk[517..], vec![255; count - 1]);
        let mut loaded = Chunk::new(0, 0);
        loaded
            .decode_biomes_disk_for_dimension(&disk, dimension)
            .unwrap();
        let bounds = crate::world::dimension_block_bounds(dimension);
        assert_eq!(loaded.biome_at(0, bounds.start as i16, 0), Some(7));
        assert_eq!(loaded.biome_at(0, (bounds.end - 1) as i16, 0), Some(7));
    }
    let mut target = server(1);
    let world = target.ecs.world_mut();
    let entity = world.get_or_create_chunk(0, 0);
    assert_eq!(
        world
            .get::<crate::world::ecs::ChunkData>(entity)
            .unwrap()
            .inner
            .block_bounds(),
        0..128
    );
    assert!(!is_safe_destination(world, DVec3::new(4.5, 129.0, 4.5)));
}
#[tokio::test]
async fn leveldb_dimension_biomes_reopen_at_the_correct_native_height() {
    use crate::storage::WorldProvider;
    let directory = tempfile::tempdir().unwrap();
    let provider = crate::storage::LevelDBWorldProvider::open(directory.path(), 1).unwrap();
    let mut chunk = crate::world::chunk::Chunk::new(0, 0);
    chunk.set_block_bounds(0..128);
    chunk.set_biome(8);
    chunk.set_block(1, 0, 1, *crate::world::chunk::blocks::STONE);
    provider
        .save_column(
            crate::world::ChunkPos::new(0, 0),
            1,
            &crate::storage::ChunkColumn::new(chunk),
        )
        .await
        .unwrap();
    provider.flush().await.unwrap();
    drop(provider);
    let reopened = crate::storage::LevelDBWorldProvider::open(directory.path(), 1).unwrap();
    let loaded = reopened
        .load_column(crate::world::ChunkPos::new(0, 0), 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.chunk.biome_at(0, 0, 0), Some(8));
    assert_eq!(loaded.chunk.biome_at(0, 127, 0), Some(8));
    assert!(
        reopened
            .load_column(crate::world::ChunkPos::new(0, 0), 0)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn differing_world_clocks_preserve_effect_lifetimes_and_rebase_absolute_hurt_and_fire() {
    use crate::ecs::resources::TickCounter;
    use crate::entity::mob::{combat::HurtTimer, state::LastHurt};
    use crate::entity::player_life::environment::Burning;
    let mut router = router();
    let (player, _rx) = spawn(&mut router, 0, 1);
    router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .resource_mut::<TickCounter>()
        .current = 10_000;
    router
        .world_mut(1)
        .unwrap()
        .ecs
        .world_mut()
        .resource_mut::<TickCounter>()
        .current = 300;
    let world = router.world_mut(0).unwrap().ecs.world_mut();
    world
        .get_mut::<Effects>(player)
        .unwrap()
        .active
        .insert(EffectType::SPEED, ActiveEffect::new(2, 400));
    world.entity_mut(player).insert((
        HurtTimer {
            until_tick: 10_050,
            last_damage: 3.0,
        },
        Burning {
            until_tick: 10_100,
            next_burn_tick: 10_030,
        },
        LastHurt {
            tick: 9_995,
            cause: unastar_data::actor_components::body::ActorDamageCause::EntityAttack,
            attacker: Some(player),
            by_player: true,
        },
        Age(77),
    ));
    request(&mut router, player, 1);
    router.tick().await.unwrap();
    let (_, target) = router.locate_session(1).unwrap();
    let world = router.world(1).unwrap().ecs.world();
    let now = world.resource::<TickCounter>().current;
    assert_eq!(now, 301);
    assert_eq!(
        world
            .get::<Effects>(target)
            .unwrap()
            .get(EffectType::SPEED)
            .unwrap()
            .duration_ticks,
        399
    );
    assert_eq!(world.get::<HurtTimer>(target).unwrap().until_tick - now, 49);
    assert_eq!(world.get::<Burning>(target).unwrap().until_tick - now, 99);
    assert_eq!(
        world.get::<Burning>(target).unwrap().next_burn_tick - now,
        29
    );
    assert_eq!(now - world.get::<LastHurt>(target).unwrap().tick, 6);
    assert!(world.get::<LastHurt>(target).unwrap().attacker.is_none());
    assert_eq!(world.get::<Age>(target).unwrap().0, 78);
}
struct UnsafeRewrite;
impl WorldTransferHandler for UnsafeRewrite {
    fn on_transfer(&self, _: &mut World, _: &mut World, event: &mut PlayerWorldTransfer) {
        event.feet.y = 0.0;
    }
}
#[tokio::test]
async fn late_unsafe_destination_rewrite_never_invokes_final_predicate_or_moves_player() {
    let mut router = router();
    let (player, mut rx) = spawn(&mut router, 0, 1);
    let observed = observe(&mut router);
    router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .get_resource_or_init::<WorldTransferHooks>()
        .register(UnsafeRewrite);
    let calls = Arc::new(AtomicUsize::new(0));
    let recorded = calls.clone();
    request_transfer_checked(
        router.world_mut(0).unwrap().ecs.world_mut(),
        player,
        1,
        DVec3::new(4.5, 16.0, 4.5),
        0,
        "rewrite".into(),
        move |_, _, _| {
            recorded.fetch_add(1, Ordering::SeqCst);
            true
        },
    )
    .unwrap();
    router.tick().await.unwrap();
    assert_eq!(router.locate_session(1), Some((0, player)));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        observed.lock().unwrap().failures,
        vec![TransferError::UnsafeDestination]
    );
    assert!(
        !packets(&mut rx)
            .iter()
            .any(|packet| matches!(packet, McpePacketData::ChangeDimensionPacket(_)))
    );
}
#[tokio::test]
async fn transport_backpressure_retries_without_preparing_or_partially_publishing_a_transfer() {
    let mut router = router();
    let (player, mut rx) = spawn(&mut router, 0, 1);
    let calls = Arc::new(AtomicUsize::new(0));
    let recorded = calls.clone();
    request_transfer_checked(
        router.world_mut(0).unwrap().ecs.world_mut(),
        player,
        1,
        DVec3::new(4.5, 16.0, 4.5),
        0,
        "backpressure".into(),
        move |_, _, _| {
            recorded.fetch_add(1, Ordering::SeqCst);
            true
        },
    )
    .unwrap();
    let tx = router
        .world(0)
        .unwrap()
        .ecs
        .world()
        .get::<PlayerSession>(player)
        .unwrap()
        .outbound_tx
        .clone();
    while tx
        .try_send(McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::Playerspawn,
        }))
        .is_ok()
    {}
    router.tick().await.unwrap();
    assert_eq!(router.locate_session(1), Some((0, player)));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(
        !packets(&mut rx)
            .iter()
            .any(|packet| matches!(packet, McpePacketData::ChangeDimensionPacket(_)))
    );
    router.tick().await.unwrap();
    assert_eq!(router.locate_session(1).unwrap().0, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn authoritative_join_load_rejects_malformed_records_instead_of_guessing_a_dimension() {
    let id = uuid::Uuid::from_u128(1);
    let provider = RecordingProvider {
        record: Mutex::new(Some(crate::storage::PlayerData {
            uuid: id.to_string(),
            dimension: 1,
            game_mode: 255,
            ..Default::default()
        })),
        ..Default::default()
    };
    assert!(
        crate::server::connect::try_load_player_data(Some(&provider), Some(&id.to_string()))
            .await
            .is_err()
    );
    *provider.record.lock().unwrap() = None;
    assert!(
        crate::server::connect::try_load_player_data(Some(&provider), Some(&id.to_string()))
            .await
            .unwrap()
            .is_none()
    );
}
#[tokio::test]
async fn last_position_acknowledgements_replace_complete_dimension_records_without_temporary_files()
{
    let directory = tempfile::tempdir().unwrap();
    let store = crate::config::PlayerDataStore::new(directory.path().into());
    let id = uuid::Uuid::from_u128(1).to_string();
    for dimension in [1, 2] {
        store
            .save_last_position(
                &id,
                &crate::config::PlayerLastPosition {
                    dimension,
                    location: crate::config::SpawnLocation {
                        x: 1.5,
                        y: 64.62,
                        z: 2.5,
                        yaw: 3.0,
                        pitch: 4.0,
                    },
                },
            )
            .await
            .unwrap();
        let read = store.load_last_position(&id).await.unwrap().unwrap();
        assert_eq!(read.dimension, dimension);
        assert_eq!(read.location.x, 1.5);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

#[tokio::test]
async fn restricted_actor_never_leaks_to_source_or_destination_viewers_before_projection_rebuild() {
    let mut router = router();
    let (player, mut own) = spawn(&mut router, 0, 1);
    let (_source_viewer, mut source_rx) = spawn(&mut router, 0, 2);
    let (_target_viewer, mut target_rx) = spawn(&mut router, 1, 3);
    router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .entity_mut(player)
        .insert(ActorAudience::only([]));
    router.tick().await.unwrap();
    packets(&mut own);
    packets(&mut source_rx);
    packets(&mut target_rx);
    request(&mut router, player, 1);
    router.tick().await.unwrap();
    let (_, target) = router.locate_session(1).unwrap();
    router.tick().await.unwrap();
    assert!(!packets(&mut source_rx).iter().any(|packet|matches!(packet,McpePacketData::RemoveActorPacket(remove) if remove.target_actor_id.actor_unique_id==201)));
    assert!(!packets(&mut target_rx).iter().any(|packet|matches!(packet,McpePacketData::AddPlayerPacket(add) if add.target_runtime_id.actor_runtime_id==201)));
    router
        .world_mut(1)
        .unwrap()
        .ecs
        .world_mut()
        .entity_mut(target)
        .insert(ActorAudience::all());
    router.tick().await.unwrap();
    assert!(packets(&mut target_rx).iter().any(|packet|matches!(packet,McpePacketData::AddPlayerPacket(add) if add.target_runtime_id.actor_runtime_id==201)));
}

#[tokio::test]
async fn dimension_transfer_preserves_cold_exposure_and_actor_age_after_source_tick() {
    use crate::entity::freezing::{FreezingImmune, FreezingState, FreezingVulnerable};
    let mut router = router();
    let (player, _rx) = spawn(&mut router, 0, 1);
    let world = router.world_mut(0).unwrap().ecs.world_mut();
    world
        .entity_mut(player)
        .insert((FreezingState::from_strength(0.7).unwrap(), Age(1_000)));
    let recorded = Arc::new(Mutex::new(None));
    let before = recorded.clone();
    request_transfer_checked(
        world,
        player,
        1,
        DVec3::new(4.5, 16.0, 4.5),
        0,
        "cold".into(),
        move |source, _, event| {
            *before.lock().unwrap() = Some((
                source
                    .get::<FreezingState>(event.player())
                    .unwrap()
                    .strength(),
                source.get::<Age>(event.player()).unwrap().0,
            ));
            true
        },
    )
    .unwrap();
    router.tick().await.unwrap();
    let (_, target) = router.locate_session(1).unwrap();
    let world = router.world(1).unwrap().ecs.world();
    let (strength, age) = recorded.lock().unwrap().unwrap();
    assert!(strength > 0.0 && strength < 0.7);
    assert_eq!(
        world.get::<FreezingState>(target).unwrap().strength(),
        strength
    );
    assert_eq!(world.get::<Age>(target).unwrap().0, age);
    let snapshot = player_state::capture(world, target).unwrap();
    assert_eq!(snapshot.data.cold_exposure, strength);
    assert_eq!(snapshot.data.age_ticks, age);
    let rules = TransferComponents::default();
    let mut fixture = World::new();
    let actor = fixture
        .spawn((FreezingImmune, FreezingVulnerable, Age(1)))
        .id();
    assert!(rules.unsupported(&fixture, actor).is_empty());
}

#[tokio::test]
async fn destination_preflight_loads_geometry_without_moving_any_session_and_releases_its_pin() {
    let mut router = router();
    let (player, _rx) = spawn(&mut router, 0, 1);
    assert_eq!(
        router
            .prepare_destination(1, DVec3::new(4.5, 18.0, 4.5), 4)
            .unwrap(),
        Some(DVec3::new(4.5, 16.0, 4.5))
    );
    assert_eq!(router.locate_session(1), Some((0, player)));
    let target = router.world(1).unwrap().ecs.world();
    let anchor = target.resource::<super::transfer::PreloadAnchor>().0;
    let chunk = target
        .resource::<crate::world::ecs::ChunkManager>()
        .get_by_coords(0, 0)
        .unwrap();
    assert!(
        target
            .get::<crate::world::ecs::ChunkViewers>(chunk)
            .unwrap()
            .contains(anchor)
    );
    assert!(
        router
            .world(1)
            .unwrap()
            .ecs
            .world()
            .resource::<game::SessionEntityMap>()
            .is_empty()
    );
    assert_eq!(
        router.prepare_destination(1, DVec3::new(4.5, 30.0, 4.5), 1),
        Err(TransferError::UnsafeDestination)
    );
    assert_eq!(
        router.prepare_destination(3, DVec3::ZERO, 1),
        Err(TransferError::UnknownDimension)
    );
    router
        .world_mut(1)
        .unwrap()
        .ecs
        .world_mut()
        .resource_mut::<crate::ecs::resources::TickCounter>()
        .current = 200;
    router.tick().await.unwrap();
    assert!(
        !router
            .world(1)
            .unwrap()
            .ecs
            .world()
            .contains_resource::<super::transfer::PreloadAnchor>()
    );
}

struct FlushProvider {
    calls: Arc<AtomicUsize>,
    fail: bool,
}
#[async_trait::async_trait]
impl crate::storage::WorldProvider for FlushProvider {
    async fn load_column(
        &self,
        _: crate::world::ChunkPos,
        _: i32,
    ) -> crate::storage::StorageResult<Option<crate::storage::ChunkColumn>> {
        Ok(None)
    }
    async fn save_column(
        &self,
        _: crate::world::ChunkPos,
        _: i32,
        _: &crate::storage::ChunkColumn,
    ) -> crate::storage::StorageResult<()> {
        Ok(())
    }
    async fn flush(&self) -> crate::storage::StorageResult<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            Err(crate::storage::StorageError::Database(
                "flush unavailable".into(),
            ))
        } else {
            Ok(())
        }
    }
    async fn close(&self) -> crate::storage::StorageResult<()> {
        self.flush().await
    }
}
#[tokio::test]
async fn shutdown_flushes_each_realm_and_nonprimary_stop_and_flush_errors_are_observed() {
    let mut router = router();
    let calls = Arc::new(AtomicUsize::new(0));
    for (_, server) in router.iter_mut() {
        server
            .ecs
            .world_mut()
            .resource_mut::<crate::world::ecs::ChunkManager>()
            .set_provider(Arc::new(FlushProvider {
                calls: calls.clone(),
                fail: false,
            }));
    }
    router.flush_worlds().await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert!(!router.stop_requested());
    router
        .world_mut(2)
        .unwrap()
        .ecs
        .world_mut()
        .insert_resource(crate::command::exec::StopRequested);
    assert!(router.stop_requested());
    router
        .world_mut(1)
        .unwrap()
        .ecs
        .world_mut()
        .resource_mut::<crate::world::ecs::ChunkManager>()
        .set_provider(Arc::new(FlushProvider { calls, fail: true }));
    assert!(router.flush_worlds().await.is_err());
}
