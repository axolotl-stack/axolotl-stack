//! Canonical component requests exercise actual native saves, migration and result delivery.

use super::*;
use crate::ecs::events::{EventBuffer, EventKind};
use crate::entity::components::{MainInventory, PlayerSession, Position};
use crate::plugin::manifest::PluginCapability;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig, types};
use crate::server::worlds::DimensionRouter;
use crate::storage::{LevelDBPlayerProvider, PlayerData, PlayerProvider};
use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::{McpePacket, McpePacketData};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

struct Provider {
    inner: LevelDBPlayerProvider,
    writes: AtomicUsize,
    fail: AtomicBool,
}
#[async_trait::async_trait]
impl PlayerProvider for Provider {
    fn atomic_durable_records(&self) -> bool {
        true
    }
    async fn load(&self, uuid: uuid::Uuid) -> crate::storage::StorageResult<Option<PlayerData>> {
        self.inner.load(uuid).await
    }
    async fn save(
        &self,
        uuid: uuid::Uuid,
        record: &PlayerData,
    ) -> crate::storage::StorageResult<()> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.inner.save(uuid, record).await?;
        if self.fail.load(Ordering::SeqCst) {
            Err(crate::storage::StorageError::Database(
                "acknowledgement unavailable".into(),
            ))
        } else {
            Ok(())
        }
    }
    async fn delete(&self, uuid: uuid::Uuid) -> crate::storage::StorageResult<()> {
        self.inner.delete(uuid).await
    }
    async fn close(&self) -> crate::storage::StorageResult<()> {
        self.inner.close().await
    }
}

struct Harness {
    router: DimensionRouter,
    manager: Arc<Mutex<PluginManager>>,
    player: Entity,
    packets: tokio::sync::mpsc::Receiver<McpePacket>,
    provider: Arc<Provider>,
    _directory: tempfile::TempDir,
}

/// One complete loaded floor, with explicit clearance and support in both real worlds.
fn server(dimension: i32) -> GameServer {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig {
            dimension,
            ..crate::world::WorldConfig::void_spawn_platform(1)
        },
        ..Default::default()
    });
    let world = server.ecs.world_mut();
    world
        .resource_mut::<crate::world::rules::GameRules>()
        .do_mob_spawning = false;
    for x in -1..=1 {
        for z in -1..=1 {
            let mut column = crate::world::chunk::Chunk::new(x, z);
            for local_x in 0..16 {
                for local_z in 0..16 {
                    column.set_block(local_x, 63, local_z, *crate::world::chunk::blocks::STONE);
                }
            }
            let position = crate::world::ecs::ChunkPosition::new(x, z);
            let entity = world
                .spawn((crate::world::ecs::ChunkData::new(column), position))
                .id();
            world
                .resource_mut::<crate::world::ChunkManager>()
                .insert(position, entity);
        }
    }
    server
}

impl Harness {
    /// Load a genuine current-WIT component and a durable provider before trusted native join.
    fn new(bodies: &[(&str, String)], permitted: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let provider = Arc::new(Provider {
            inner: LevelDBPlayerProvider::open(directory.path()).unwrap(),
            writes: AtomicUsize::new(0),
            fail: AtomicBool::new(false),
        });
        let mut primary = server(0);
        primary
            .ecs
            .world_mut()
            .insert_resource(types::PlayerProviderResource(provider.clone()));
        let mut router = DimensionRouter::new(primary);
        router.add(server(1)).unwrap();
        let engine = engine();
        let mut capabilities = HashSet::from([PluginCapability::ReadPlayers]);
        if permitted {
            capabilities.insert(PluginCapability::PlayerTravel);
        }
        let manifest = PluginManifest {
            id: PluginId("example.travel".into()),
            name: "Travel".into(),
            version: "1".into(),
            api_version: PLUGIN_API_VERSION.into(),
            required: false,
            config: None,
            capabilities,
            subscriptions: [EventKind::PlayerJump, EventKind::PlayerChat].into(),
            limits: PluginLimits::default(),
        };
        let pending = PendingPlugin {
            component: fixture::component(&engine, bodies),
            store: store(&engine, &manifest.limits),
            manifest,
        };
        let manager = Arc::new(Mutex::new(PluginManager {
            engine,
            plugins: Default::default(),
            pending: vec![pending],
            tick_prepared: false,
            tick_budget_valid: true,
            readiness: Default::default(),
            storage_registry: Default::default(),
            archive_registry: Default::default(),
            command_roots: Default::default(),
            ui_registry: Default::default(),
            view_registry: Default::default(),
        }));
        for (_, server) in router.iter_mut() {
            PluginManager::attach_to_world(manager.clone(), server.ecs.world_mut());
        }
        router.set_plugin_manager(manager.clone());
        router.initialize_plugins().unwrap();
        let (outbound_tx, mut packets) = tokio::sync::mpsc::channel(8192);
        let player = router
            .spawn_player_in(
                0,
                PlayerSpawnData {
                    session_id: 73,
                    runtime_id: 173,
                    display_name: "Traveller".into(),
                    xuid: Some("73".into()),
                    uuid: Some(uuid::Uuid::from_u128(73).to_string()),
                    position: DVec3::new(4.5, 65.62, 4.5),
                    outbound_tx,
                    saved_data: None,
                    chunk_radius: 0,
                },
            )
            .unwrap();
        while packets.try_recv().is_ok() {}
        Self {
            router,
            manager,
            player,
            packets,
            provider,
            _directory: directory,
        }
    }
    /// Deliver a real buffered callback through the router's normal component cycle.
    fn queue(&mut self) {
        let player = self.player;
        self.world_mut()
            .resource_mut::<EventBuffer>()
            .push(ServerEvent::PlayerJump { entity: player });
    }
    /// Stage before the router boundary for controlled stale/closure cases.
    fn stage(&mut self) {
        self.queue();
        self.manager
            .lock()
            .unwrap()
            .drain_world_events(self.router.world_mut(0).unwrap().ecs.world_mut());
    }
    /// Read one source world without retaining a mutable borrow across an operation.
    fn world(&self) -> &World {
        self.router.world(0).unwrap().ecs.world()
    }
    /// Mutate one actual source world for controlled native boundary cases.
    fn world_mut(&mut self) -> &mut World {
        self.router.world_mut(0).unwrap().ecs.world_mut()
    }
    /// Read guest witnesses without invoking callbacks or replenishing fuel.
    fn logs(&self) -> Vec<String> {
        self.manager
            .lock()
            .unwrap()
            .plugins
            .values()
            .next()
            .unwrap()
            .store
            .data()
            .test_logs
            .clone()
    }
    /// Advance a bounded number of real router cycles until native completion is observed.
    async fn finish(&mut self) {
        for _ in 0..8 {
            self.router.tick().await.unwrap();
            if !self.logs().is_empty() {
                return;
            }
        }
        panic!("native operation did not produce its bounded result");
    }
}

/// Obtain the exact destination world lifetime during initialization, never a fabricated handle.
fn load_destination(dimension: i32) -> String {
    format!(
        r#"
        i32.const 64 call @worlds@
        i32.const 64 i32.load8_u if unreachable end
        i32.const 72 i32.load i32.const 2 i32.ne if unreachable end
        i32.const 512 i32.const 68 i32.load i32.load offset={} i32.store
        i32.const 68 i32.load i32.load offset={} call @world-handle_drop@
    "#,
        dimension * 4,
        (1 - dimension) * 4
    )
}
/// One actual import checks the canonical result rather than assuming a native effect.
fn request(feet: DVec3, radius: u16, expected: Option<u8>) -> String {
    let check = match expected {
        None => "i32.const 96 i32.load8_u if unreachable end".into(),
        Some(error) => format!(
            "i32.const 96 i32.load8_u i32.const 1 i32.ne if unreachable end i32.const 97 i32.load8_u i32.const {error} i32.ne if unreachable end"
        ),
    };
    format!(
        r#"
        local.get 1 i32.const 512 i32.load f64.const {} f64.const {} f64.const {} i32.const {radius} i64.const 91 i32.const 96 call @[method]player.relocate@
        {check}
    "#,
        feet.x, feet.y, feet.z
    )
}
/// Witness a successful canonical callback after checking an import's typed result.
fn witness() -> &'static str {
    "i32.const 1024 i32.const 120 i32.store8 i32.const 0 i32.const 1024 i32.const 1 call @log@"
}
/// Read owned outcomes through the generated current canonical record layout.
fn result_body() -> String {
    let (correlation, _) =
        fixture::field_layout("on-player-travel-result", "event", &["correlation"]);
    let (outcome, _) = fixture::field_layout("on-player-travel-result", "event", &["outcome"]);
    let (account, _) = fixture::field_layout(
        "on-player-travel-result",
        "event",
        &["identity", "authenticated-xuid", "some"],
    );
    let (connection, _) = fixture::field_layout(
        "on-player-travel-result",
        "event",
        &["identity", "connection-id", "some"],
    );
    let (source, _) =
        fixture::field_layout("on-player-travel-result", "event", &["origin", "dimension"]);
    format!(
        r#"
        local.get 0 i64.load offset={correlation} i64.const 91 i64.ne if unreachable end
        local.get 0 i64.load offset={account} i64.const 73 i64.ne if unreachable end
        local.get 0 i64.load offset={connection} i64.const 73 i64.ne if unreachable end
        local.get 0 i32.load offset={source} if unreachable end
        i32.const 1024 local.get 0 i32.load8_u offset={outcome} i32.const 48 i32.add i32.store8
        i32.const 0 i32.const 1024 i32.const 1 call @log@
    "#
    )
}
/// Compose independent request and owned-result callbacks into one real component.
fn bodies(dimension: i32, body: String) -> Vec<(&'static str, String)> {
    vec![
        ("on-load", load_destination(dimension)),
        ("on-player-jump", body),
        ("on-player-travel-result", result_body()),
    ]
}
/// Select loaded body clearance directly above the fixture's real solid floor.
fn destination() -> DVec3 {
    DVec3::new(8.5, 64.0, 8.5)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_travel_same_world_saves_before_move_and_reports_once() {
    let mut h = Harness::new(&bodies(0, request(destination(), 0, None)), true);
    let items = h
        .world()
        .get::<MainInventory>(h.player)
        .unwrap()
        .0
        .slots()
        .to_vec();
    h.queue();
    h.finish().await;
    assert_eq!(h.logs(), ["0"]);
    assert_eq!(h.provider.writes.load(Ordering::SeqCst), 1);
    let record = h
        .provider
        .load(uuid::Uuid::from_u128(73))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        record.position,
        (destination() + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT).to_array()
    );
    assert_eq!(
        h.world().get::<Position>(h.player).unwrap().0.to_array(),
        record.position
    );
    assert_eq!(
        h.world().get::<MainInventory>(h.player).unwrap().0.slots(),
        items
    );
    let mut teleports = 0;
    while let Ok(packet) = h.packets.try_recv() {
        if matches!(packet.data, McpePacketData::MovePlayerPacket(_)) {
            teleports += 1;
        }
    }
    assert_eq!(teleports, 1);
    h.router.tick().await.unwrap();
    assert_eq!(h.logs(), ["0"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_travel_cross_world_migrates_transport_inventory_and_saved_dimension() {
    let mut h = Harness::new(&bodies(1, request(destination(), 0, None)), true);
    let original = h
        .world()
        .get::<PlayerSession>(h.player)
        .unwrap()
        .outbound_tx
        .clone();
    h.queue();
    h.finish().await;
    assert_eq!(h.logs(), ["0"]);
    assert!(h.world().get_entity(h.player).is_err());
    let target = h.router.locate_session(73).unwrap();
    assert_eq!(target.0, 1);
    let world = h.router.world(1).unwrap().ecs.world();
    assert!(original.same_channel(&world.get::<PlayerSession>(target.1).unwrap().outbound_tx));
    assert!(world.get::<MainInventory>(target.1).is_some());
    let record = h
        .provider
        .load(uuid::Uuid::from_u128(73))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.dimension, 1);
    assert_eq!(
        record.position,
        (destination() + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT).to_array()
    );
    let mut dimensions = 0;
    while let Ok(packet) = h.packets.try_recv() {
        if matches!(packet.data, McpePacketData::ChangeDimensionPacket(_)) {
            dimensions += 1;
        }
    }
    assert_eq!(dimensions, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_travel_trap_discards_request_without_save_or_notification() {
    let mut h = Harness::new(
        &bodies(
            0,
            format!("{} unreachable", request(destination(), 0, None)),
        ),
        true,
    );
    let before = h.world().get::<Position>(h.player).unwrap().0;
    h.queue();
    h.router.tick().await.unwrap();
    assert_eq!(h.world().get::<Position>(h.player).unwrap().0, before);
    assert_eq!(h.provider.writes.load(Ordering::SeqCst), 0);
    assert!(h.logs().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_travel_permission_and_invalid_bounds_do_not_queue() {
    for (permitted, radius, error) in [(false, 0, 6), (true, 257, 5)] {
        let mut h = Harness::new(
            &bodies(
                0,
                format!(
                    "{} {}",
                    request(destination(), radius, Some(error)),
                    witness()
                ),
            ),
            permitted,
        );
        h.queue();
        h.router.tick().await.unwrap();
        assert_eq!(h.provider.writes.load(Ordering::SeqCst), 0);
        assert_eq!(h.logs(), ["x"]);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_travel_replaced_channel_cannot_move_or_save_original_request() {
    let mut h = Harness::new(&bodies(0, request(destination(), 0, None)), true);
    h.stage();
    let (sender, _receiver) = tokio::sync::mpsc::channel(8192);
    let player = h.player;
    h.world_mut()
        .get_mut::<PlayerSession>(player)
        .unwrap()
        .outbound_tx = sender;
    h.finish().await;
    assert_eq!(h.logs(), ["1"]);
    assert_eq!(h.provider.writes.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_travel_world_replacement_does_not_resolve_by_dimension() {
    let mut h = Harness::new(&bodies(1, request(destination(), 0, None)), true);
    h.stage();
    *h.router.world_mut(1).unwrap() = server(1);
    h.finish().await;
    assert_eq!(h.logs(), ["1"]);
    assert_eq!(h.provider.writes.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_travel_source_closure_still_delivers_owned_failure() {
    let mut h = Harness::new(&bodies(1, request(destination(), 0, None)), true);
    h.stage();
    *h.router.world_mut(0).unwrap() = server(0);
    h.finish().await;
    assert_eq!(h.logs(), ["1"]);
    assert_eq!(h.provider.writes.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_travel_unload_revokes_outstanding_native_authority() {
    let mut h = Harness::new(&bodies(1, request(destination(), 0, None)), true);
    h.stage();
    h.manager.lock().unwrap().plugins.clear();
    h.router.tick().await.unwrap();
    assert_eq!(h.provider.writes.load(Ordering::SeqCst), 0);
    assert_eq!(h.router.locate_session(73).unwrap().0, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_travel_unsafe_destination_reports_without_cost_or_save() {
    let mut h = Harness::new(
        &bodies(1, request(DVec3::new(8.5, 70.0, 8.5), 0, None)),
        true,
    );
    h.queue();
    h.finish().await;
    assert_eq!(h.logs(), ["1"]);
    assert_eq!(h.provider.writes.load(Ordering::SeqCst), 0);
    assert_eq!(h.router.locate_session(73).unwrap().0, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_travel_uncertain_acknowledgement_is_fenced_not_reported_unapplied() {
    let mut h = Harness::new(&bodies(0, request(destination(), 0, None)), true);
    h.provider.fail.store(true, Ordering::SeqCst);
    h.queue();
    h.finish().await;
    assert_eq!(h.logs(), ["2"]);
    assert_eq!(h.provider.writes.load(Ordering::SeqCst), 1);
    assert!(crate::server::game::inventory_effects::is_recovery_fenced(
        h.world(),
        h.player
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_travel_default_chat_cancellation_keeps_explicit_request() {
    let mut callbacks = bodies(0, String::new());
    callbacks.push((
        "on-player-chat",
        format!("{} i32.const 0", request(destination(), 0, None)),
    ));
    let mut h = Harness::new(&callbacks, true);
    let player = h.player;
    h.world_mut()
        .resource_mut::<EventBuffer>()
        .push(ServerEvent::PlayerChat {
            entity: player,
            player_id: "73".into(),
            message: "relocate".into(),
        });
    h.finish().await;
    assert_eq!(h.logs(), ["0"]);
    assert_eq!(h.provider.writes.load(Ordering::SeqCst), 1);
    while let Ok(packet) = h.packets.try_recv() {
        assert!(
            !matches!(packet.data, McpePacketData::TextPacket(_)),
            "cancelled default broadcast stays suppressed"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_travel_native_late_veto_does_not_write_or_move() {
    struct Veto;
    impl crate::entity::projectile::hooks::ProjectileHandler for Veto {
        fn on_teleport(
            &self,
            _world: &mut World,
            event: &mut crate::entity::projectile::hooks::PlayerTeleport,
        ) {
            event.cancel();
        }
    }
    let mut h = Harness::new(&bodies(0, request(destination(), 0, None)), true);
    h.world_mut()
        .get_resource_or_init::<crate::entity::projectile::hooks::ProjectileHooks>()
        .register(Veto);
    let before = h.world().get::<Position>(h.player).unwrap().0;
    h.queue();
    h.finish().await;
    assert_eq!(h.logs(), ["1"]);
    assert_eq!(h.provider.writes.load(Ordering::SeqCst), 0);
    assert_eq!(h.world().get::<Position>(h.player).unwrap().0, before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_travel_dead_or_quarantined_actor_cannot_stage() {
    for dead in [false, true] {
        let mut h = Harness::new(
            &bodies(
                0,
                format!("{} {}", request(destination(), 0, Some(8)), witness()),
            ),
            true,
        );
        let player = h.player;
        if dead {
            h.world_mut()
                .entity_mut(player)
                .insert(crate::entity::player_life::Dead);
        } else {
            let source =
                crate::server::game::inventory_effects::RecoverySource::capture(h.world(), player)
                    .unwrap();
            crate::server::game::inventory_effects::fence_for_recovery(h.world_mut(), &source)
                .unwrap();
        }
        h.queue();
        h.router.tick().await.unwrap();
        assert_eq!(h.logs(), ["x"]);
        assert_eq!(h.provider.writes.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_travel_safe_y_search_uses_actual_native_destination() {
    let mut h = Harness::new(
        &bodies(1, request(DVec3::new(8.5, 65.0, 8.5), 1, None)),
        true,
    );
    h.queue();
    h.finish().await;
    assert_eq!(h.logs(), ["0"]);
    let record = h
        .provider
        .load(uuid::Uuid::from_u128(73))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.dimension, 1);
    assert_eq!(
        record.position,
        (destination() + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT).to_array()
    );
}
