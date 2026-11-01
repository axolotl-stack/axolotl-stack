//! Real Rust components exercise storage acknowledgements and native ownership lifetimes.

use super::*;
use crate::entity::components::{MainInventory, PlayerUuid};
use crate::server::game::inventory_effects as native;
use crate::server::game::types::PlayerProviderResource;
use crate::storage::{PlayerData, PlayerProvider, StorageError, StorageResult};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Semaphore;
use wasmtime::component::Component;

/// Compile one genuine SDK component in a separate target directory, without the outer Cargo lock.
fn guest() -> &'static PathBuf {
    static GUEST: OnceLock<PathBuf> = OnceLock::new();
    GUEST.get_or_init(|| {
        let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| package.join("../../target"))
            .join("guest-inventory-fixture");
        let output =
            std::process::Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
                .args([
                    "build",
                    "--locked",
                    "--release",
                    "--target",
                    "wasm32-wasip2",
                    "--manifest-path",
                ])
                .arg(package.join("tests/fixtures/inventory-component/Cargo.toml"))
                .env("CARGO_TARGET_DIR", &target)
                .output()
                .expect("build inventory component fixture");
        assert!(
            output.status.success(),
            "inventory fixture build failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        target.join("wasm32-wasip2/release/inventory_component_fixture.wasm")
    })
}

/// Stop a provider exactly at its atomic write and optionally lose its acknowledgement.
struct Provider {
    records: Mutex<BTreeMap<uuid::Uuid, PlayerData>>,
    started: Semaphore,
    gate: Semaphore,
    writes: AtomicUsize,
    fail_after: AtomicBool,
}
impl Provider {
    /// Create a closed write gate so queued work can be inspected deterministically.
    fn new() -> Self {
        Self {
            records: Default::default(),
            started: Semaphore::new(0),
            gate: Semaphore::new(0),
            writes: AtomicUsize::new(0),
            fail_after: AtomicBool::new(false),
        }
    }
}
#[async_trait]
impl PlayerProvider for Provider {
    fn atomic_durable_records(&self) -> bool {
        true
    }
    async fn load(&self, uuid: uuid::Uuid) -> StorageResult<Option<PlayerData>> {
        Ok(self.records.lock().unwrap().get(&uuid).cloned())
    }
    async fn save(&self, uuid: uuid::Uuid, data: &PlayerData) -> StorageResult<()> {
        self.started.add_permits(1);
        self.gate.acquire().await.unwrap().forget();
        self.records.lock().unwrap().insert(uuid, data.clone());
        self.writes.fetch_add(1, Ordering::SeqCst);
        if self.fail_after.load(Ordering::SeqCst) {
            Err(StorageError::Database(
                "controlled lost acknowledgement".into(),
            ))
        } else {
            Ok(())
        }
    }
    async fn delete(&self, uuid: uuid::Uuid) -> StorageResult<()> {
        self.records.lock().unwrap().remove(&uuid);
        Ok(())
    }
    async fn close(&self) -> StorageResult<()> {
        Ok(())
    }
}

#[derive(Resource, Default)]
struct Completions(usize);

struct Harness {
    router: DimensionRouter,
    manager: Arc<Mutex<PluginManager>>,
    player: Entity,
    _packets: tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>,
    _archives: Option<tempfile::TempDir>,
}
impl Harness {
    /// Ordinary router startup installs transaction support from the manifest before a real join.
    fn new(mode: u8, provider: Arc<dyn PlayerProvider>, limits: PluginLimits, write: bool) -> Self {
        Self::with_saved(mode, provider, limits, write, None)
    }

    /// Reopen uses the exact durable record returned by storage rather than a fabricated actor image.
    fn with_saved(
        mode: u8,
        provider: Arc<dyn PlayerProvider>,
        limits: PluginLimits,
        write: bool,
        saved_data: Option<PlayerData>,
    ) -> Self {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        });
        server
            .ecs
            .world_mut()
            .insert_resource(PlayerProviderResource(provider));
        let mut router = DimensionRouter::new(server);
        let engine = engine();
        let mut context = store(&engine, &limits);
        context.data_mut().configuration = vec![mode];
        let mut capabilities = [
            PluginCapability::ReadPlayers,
            PluginCapability::ReadInventory,
            PluginCapability::ItemValues,
            PluginCapability::ScheduleTasks,
        ]
        .into_iter()
        .collect::<HashSet<_>>();
        if write {
            capabilities.insert(PluginCapability::WriteInventory);
        }
        let archives = (mode == 26).then(|| tempfile::tempdir().unwrap());
        if archives.is_some() {
            capabilities.insert(PluginCapability::PlayerStateArchives);
        }
        let pending = PendingPlugin {
            component: Component::from_file(&engine, guest()).unwrap(),
            store: context,
            manifest: PluginManifest {
                id: PluginId("example.inventory".into()),
                name: "Inventory".into(),
                version: "1".into(),
                api_version: PLUGIN_API_VERSION.into(),
                required: true,
                config: None,
                capabilities,
                subscriptions: [
                    EventKind::PlayerJump,
                    EventKind::PlayerJoin,
                    EventKind::PlayerQuit,
                    EventKind::PlayerAdmitted,
                    EventKind::PlayerMove,
                    EventKind::Tick,
                ]
                .into(),
                limits,
            },
        };
        let manager = Arc::new(Mutex::new(PluginManager {
            pending: Vec::new(),
            engine,
            plugins: Default::default(),
            tick_prepared: false,
            tick_budget_valid: true,
            readiness: Default::default(),
            storage_registry: Default::default(),
            archive_registry: Default::default(),
            command_roots: Default::default(),
            ui_registry: Default::default(),
            view_registry: Default::default(),
        }));
        if let Some(directory) = &archives {
            manager
                .lock()
                .unwrap()
                .configure_archives(&crate::config::PluginArchiveConfig {
                    directory: directory.path().join("archives"),
                    ..Default::default()
                })
                .unwrap();
        }
        manager.lock().unwrap().pending.push(pending);
        let world = router.world_mut(0).unwrap().ecs.world_mut();
        world.insert_resource(crate::world::WorldIdentity {
            id: uuid::Uuid::from_u128(9),
            name: "example-overworld".into(),
            dimension: 0,
        });
        world.init_resource::<Completions>();
        world.add_observer(
            |_: On<native::InventoryEffectCommitted>, mut done: ResMut<Completions>| done.0 += 1,
        );
        world.add_observer(
            |_: On<native::InventoryEffectFailed>, mut done: ResMut<Completions>| done.0 += 1,
        );
        PluginManager::attach_to_world(manager.clone(), world);
        router.set_plugin_manager(manager.clone());
        router.initialize_plugins().unwrap();
        let (outbound_tx, packets) = tokio::sync::mpsc::channel(65536);
        let player = router
            .world_mut(0)
            .unwrap()
            .spawn_player(crate::server::game::PlayerSpawnData {
                session_id: 1,
                runtime_id: 1,
                display_name: "Player0".into(),
                xuid: Some("1".into()),
                uuid: Some(uuid::Uuid::from_u128(1).to_string()),
                position: glam::DVec3::new(0.5, 17.62, 0.5),
                outbound_tx,
                saved_data,
                chunk_radius: 0,
            })
            .unwrap();
        router.refresh_directory();
        Self {
            router,
            manager,
            player,
            _packets: packets,
            _archives: archives,
        }
    }

    /// Use the ordinary decoded input queue to reach the cancellable native movement boundary.
    async fn move_to(&mut self, x: f32) -> Result<(), crate::server::worlds::WorldTickError> {
        use jolyne::valentine::{BorrowedMcpePacket, McpePacket, McpePacketArgs};
        let packet = McpePacket::from(jolyne::valentine::PlayerAuthInputPacket {
            position: jolyne::valentine::Vec3 {
                x,
                y: 17.62,
                z: 0.5,
            },
            ..Default::default()
        });
        let mut bytes = bytes::BytesMut::new();
        packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
        let borrowed = BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap();
        self.router
            .world_mut(0)
            .unwrap()
            .route_packet(1, McpePacketArgs, borrowed);
        self.router.tick().await
    }

    /// Read counters from the real context, never from mirrored test bookkeeping.
    fn retained(&self) -> (usize, usize) {
        retained(&self.manager)
    }

    /// Drive the actual native input-to-component dispatch for a fully identified actor.
    async fn action(&mut self) -> Result<(), crate::server::worlds::WorldTickError> {
        scope_callback(&mut self.router, self.player);
        self.router.tick().await
    }

    /// Poll only the installed native coordinator until an actual provider completion is observed.
    async fn acknowledge(&mut self, expected: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let world = self.router.world_mut(0).unwrap().ecs.world_mut();
                native::poll(world);
                world.flush();
                if world.resource::<Completions>().0 >= expected {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("native inventory acknowledgement");
    }

    /// Inspect the accepted live stack only after the coordinator has published it.
    fn count(&self) -> u8 {
        self.router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get::<MainInventory>(self.player)
            .unwrap()
            .0
            .slots()[0]
            .count
    }

    /// Consume observations from the real guest without invoking it again.
    fn logs(&self) -> Vec<String> {
        let mut manager = self.manager.lock().unwrap();
        manager
            .plugins
            .values_mut()
            .flat_map(|plugin| std::mem::take(&mut plugin.store.data_mut().test_logs))
            .collect()
    }

    /// Drain actual wire output, counting only inventory publication packets.
    fn inventory_packets(&mut self) -> usize {
        std::iter::from_fn(|| self._packets.try_recv().ok())
            .filter(|packet| {
                matches!(
                    packet.data,
                    jolyne::valentine::McpePacketData::InventoryContentPacket(_)
                )
            })
            .count()
    }
}

/// Retained values and accepted-work slots survive outside any live world borrow.
fn retained(manager: &Arc<Mutex<PluginManager>>) -> (usize, usize) {
    let manager = manager.lock().unwrap();
    let context = manager.plugins.values().next().unwrap().store.data();
    (
        context.resources.value_budget.used(),
        context.inventory_pending_count(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_stages_then_awaits_native_acknowledgement_and_reads_its_receipt() {
    let provider = Arc::new(Provider::new());
    let mut harness = Harness::new(0, provider.clone(), PluginLimits::default(), true);
    harness.action().await.unwrap();
    assert!(harness.logs().iter().any(|log| log == "queued"));
    assert!(native::is_quarantined(
        harness.router.world(0).unwrap().ecs.world(),
        harness.player
    ));
    assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
    let (bytes, pending) = harness.retained();
    assert!(bytes > 0);
    assert_eq!(pending, 1);
    provider.gate.add_permits(1);
    harness.acknowledge(1).await;
    assert_eq!(harness.count(), 3);
    assert_eq!(harness.retained(), (0, 0));
    harness.router.tick().await.unwrap();
    assert!(harness.logs().iter().any(|log| log == "committed"));
    harness.action().await.unwrap();
    assert!(harness.logs().iter().any(|log| log == "receipt"));
    assert_eq!(provider.writes.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_all_guest_resources_cannot_cancel_or_uncharge_an_accepted_native_write() {
    let provider = Arc::new(Provider::new());
    let mut harness = Harness::new(2, provider.clone(), PluginLimits::default(), true);
    harness.action().await.unwrap();
    assert!(harness.logs().iter().any(|log| log == "dropped"));
    assert!(harness.retained().0 > 0);
    assert_eq!(harness.retained().1, 1);
    provider.gate.add_permits(1);
    harness.acknowledge(1).await;
    assert_eq!(harness.count(), 3);
    assert_eq!(harness.retained(), (0, 0));
    assert_eq!(provider.writes.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn callback_trap_before_submission_writes_nothing_and_releases_staged_reservations() {
    let provider = Arc::new(Provider::new());
    let mut harness = Harness::new(1, provider.clone(), PluginLimits::default(), true);
    assert!(harness.action().await.is_err());
    assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
    assert!(provider.records.lock().unwrap().is_empty());
    assert_eq!(harness.retained(), (0, 0));
    assert!(native::is_recovery_fenced(
        harness.router.world(0).unwrap().ecs.world(),
        harness.player
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capability_capacity_identifier_and_expired_scope_denials_perform_no_writes() {
    for mode in [8, 9, 10, 11, 12] {
        let provider = Arc::new(Provider::new());
        let limits = PluginLimits {
            max_pending_inventory_operations: if mode == 9 { 0 } else { 64 },
            max_owned_resources: if mode == 10 { 5 } else { 4096 },
            ..Default::default()
        };
        let mut harness = Harness::new(mode, provider.clone(), limits, mode != 11);
        harness.action().await.unwrap();
        let expected = match mode {
            8 => "expired",
            9 | 10 => "limited",
            11 => "denied",
            _ => "invalid",
        };
        assert!(
            harness.logs().iter().any(|log| log == expected),
            "case {mode}"
        );
        assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
        assert_eq!(harness.retained(), (0, 0));
        assert_eq!(harness.count(), 0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_receipt_reopens_and_replays_without_a_second_physical_grant() {
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(crate::storage::LevelDBPlayerProvider::open(directory.path()).unwrap());
    let mut harness = Harness::new(0, provider.clone(), PluginLimits::default(), true);
    harness.action().await.unwrap();
    harness.acknowledge(1).await;
    let uuid = harness
        .router
        .world(0)
        .unwrap()
        .ecs
        .world()
        .get::<PlayerUuid>(harness.player)
        .unwrap()
        .0;
    harness.router.tick().await.unwrap();
    assert!(harness.logs().iter().any(|log| log == "committed"));
    drop(harness);
    provider.close().await.unwrap();
    drop(provider);

    let reopened = Arc::new(crate::storage::LevelDBPlayerProvider::open(directory.path()).unwrap());
    let data = reopened.load(uuid).await.unwrap().unwrap();
    assert_eq!(data.inventory_revision, 1);
    assert_eq!(data.inventory[0].count, 3);
    assert_eq!(data.inventory[0].item_id, "minecraft:diamond");
    let mut harness = Harness::with_saved(
        18,
        reopened.clone(),
        PluginLimits::default(),
        true,
        Some(data),
    );
    harness.action().await.unwrap();
    assert!(harness.logs().iter().any(|log| log == "replayed"));
    assert_eq!(harness.count(), 3);
    assert_eq!(harness.retained(), (0, 0));
    assert_eq!(
        reopened
            .load(uuid)
            .await
            .unwrap()
            .unwrap()
            .inventory_revision,
        1
    );
    drop(harness);
    reopened.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_pending_requests_share_one_native_write_and_one_pending_reservation() {
    let provider = Arc::new(Provider::new());
    let mut harness = Harness::new(3, provider.clone(), PluginLimits::default(), true);
    harness.action().await.unwrap();
    assert_eq!(harness.retained().1, 1);
    provider.gate.add_permits(1);
    harness.acknowledge(1).await;
    harness.router.tick().await.unwrap();
    assert_eq!(harness.count(), 3);
    assert_eq!(harness.retained(), (0, 0));
    assert_eq!(provider.writes.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uncertain_acknowledgements_retain_frozen_source_bytes_until_actual_source_removal() {
    let provider = Arc::new(Provider::new());
    provider.fail_after.store(true, Ordering::SeqCst);
    let mut harness = Harness::new(0, provider.clone(), PluginLimits::default(), true);
    harness.action().await.unwrap();
    provider.gate.add_permits(1);
    harness.acknowledge(1).await;
    harness.router.tick().await.unwrap();
    assert!(harness.logs().iter().any(|log| log == "uncertain"));
    assert!(harness.retained().0 > 0);
    assert_eq!(harness.retained().1, 1);
    assert!(
        harness
            .router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get::<MainInventory>(harness.player)
            .is_none()
    );
    harness.router.despawn_player(1);
    harness.router.world_mut(0).unwrap().ecs.world_mut().flush();
    assert_eq!(harness.retained(), (0, 0));
    assert_eq!(provider.writes.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn world_closure_cannot_release_worker_owned_values_before_the_write_finishes() {
    let provider = Arc::new(Provider::new());
    let mut harness = Harness::new(2, provider.clone(), PluginLimits::default(), true);
    harness.action().await.unwrap();
    provider.started.acquire().await.unwrap().forget();
    let manager = harness.manager.clone();
    drop(harness);
    assert!(retained(&manager).0 > 0);
    assert_eq!(retained(&manager).1, 1);
    provider.gate.add_permits(1);
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while retained(&manager) != (0, 0) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("worker reservation after closed world");
    assert_eq!(provider.writes.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn held_initialization_submits_in_a_fresh_scope_and_publishes_only_after_ack_and_allow() {
    let provider = Arc::new(Provider::new());
    let mut harness = Harness::new(4, provider.clone(), PluginLimits::default(), true);
    assert_eq!(harness.inventory_packets(), 0);
    harness.router.tick().await.unwrap();
    assert!(harness.logs().iter().any(|log| log == "initial-staged"));
    assert_eq!(harness.inventory_packets(), 0);
    assert_eq!(harness.retained().1, 1);
    provider.gate.add_permits(1);
    harness.acknowledge(1).await;
    assert_eq!(harness.count(), 3);
    assert_eq!(harness.inventory_packets(), 0);
    for _ in 0..3 {
        harness.router.tick().await.unwrap();
    }
    let observed = harness.logs();
    assert!(observed.iter().any(|log| log == "committed"));
    assert!(observed.iter().any(|log| log == "resolved"));
    assert!(observed.iter().any(|log| log == "admitted"));
    assert!(harness.inventory_packets() > 0);
    assert!(!crate::server::game::player_events::initial_state_pending(
        harness.router.world(0).unwrap().ecs.world(),
        harness.player
    ));
    assert_eq!(harness.retained(), (0, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_durable_inventory_ack_does_not_implicitly_resolve_an_owned_admission_hold() {
    let provider = Arc::new(Provider::new());
    let mut harness = Harness::new(5, provider.clone(), PluginLimits::default(), true);
    harness.router.tick().await.unwrap();
    provider.gate.add_permits(1);
    harness.acknowledge(1).await;
    for _ in 0..2 {
        harness.router.tick().await.unwrap();
    }
    assert_eq!(harness.count(), 3);
    assert_eq!(harness.inventory_packets(), 0);
    assert!(crate::server::game::player_events::initial_state_pending(
        harness.router.world(0).unwrap().ecs.world(),
        harness.player
    ));
    assert!(!harness.logs().iter().any(|log| log == "admitted"));
    assert_eq!(harness.retained(), (0, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejecting_initialization_in_the_same_callback_discards_its_staged_inventory_write() {
    let provider = Arc::new(Provider::new());
    let mut harness = Harness::new(6, provider.clone(), PluginLimits::default(), true);
    harness.router.tick().await.unwrap();
    harness.router.tick().await.unwrap();
    assert!(harness.logs().iter().any(|log| log == "rejected"));
    assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
    assert!(provider.records.lock().unwrap().is_empty());
    assert_eq!(harness.inventory_packets(), 0);
    assert_eq!(harness.retained(), (0, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepted_write_acknowledges_only_its_original_retained_or_departed_actor() {
    for mode in [13, 14] {
        let provider = Arc::new(Provider::new());
        let mut harness = Harness::new(mode, provider.clone(), PluginLimits::default(), true);
        harness.action().await.unwrap();
        harness.router.despawn_player(1);
        harness.router.world_mut(0).unwrap().ecs.world_mut().flush();
        assert!(harness.retained().0 > 0);
        assert_eq!(harness.retained().1, 1);
        provider.gate.add_permits(1);
        harness.acknowledge(1).await;
        harness.router.tick().await.unwrap();
        assert!(
            harness.logs().iter().any(|log| log == "committed"),
            "case {mode}"
        );
        if mode == 13 {
            assert_eq!(harness.count(), 3);
        } else {
            assert!(
                harness
                    .router
                    .world(0)
                    .unwrap()
                    .ecs
                    .world()
                    .get_entity(harness.player)
                    .is_err()
            );
        }
        assert_eq!(harness.retained(), (0, 0));
        assert_eq!(provider.writes.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expected_image_conflicts_preserve_intervening_inventory_changes_without_any_write() {
    let provider = Arc::new(Provider::new());
    let mut harness = Harness::new(19, provider.clone(), PluginLimits::default(), true);
    harness.action().await.unwrap();
    let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
    let item = world
        .resource::<crate::server::game::types::ItemRegistryResource>()
        .0
        .stack("minecraft:diamond", 1)
        .unwrap()
        .into_slot();
    world
        .get_mut::<MainInventory>(harness.player)
        .unwrap()
        .0
        .set_item(0, item)
        .unwrap();
    harness.action().await.unwrap();
    assert!(harness.logs().iter().any(|log| log == "rejected"));
    assert_eq!(harness.count(), 1);
    assert_eq!(harness.retained(), (0, 0));
    assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_scoped_player_cannot_submit_the_original_actors_owned_image() {
    let provider = Arc::new(Provider::new());
    let mut harness = Harness::new(21, provider.clone(), PluginLimits::default(), true);
    harness.action().await.unwrap();
    let (outbound_tx, _packets) = tokio::sync::mpsc::channel(65536);
    let other = harness
        .router
        .world_mut(0)
        .unwrap()
        .spawn_player(crate::server::game::PlayerSpawnData {
            session_id: 2,
            runtime_id: 2,
            display_name: "Other".into(),
            xuid: Some("2".into()),
            uuid: Some(uuid::Uuid::from_u128(2).to_string()),
            position: glam::DVec3::new(2.5, 17.62, 0.5),
            outbound_tx,
            saved_data: None,
            chunk_radius: 0,
        })
        .unwrap();
    scope_callback(&mut harness.router, other);
    harness.router.tick().await.unwrap();
    assert!(harness.logs().iter().any(|log| log == "wrong-player"));
    assert_eq!(harness.count(), 0);
    assert_eq!(harness.retained(), (0, 0));
    assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_second_component_cannot_read_another_modules_durable_receipt() {
    let provider = Arc::new(Provider::new());
    let mut harness = Harness::new(0, provider.clone(), PluginLimits::default(), true);
    harness.action().await.unwrap();
    provider.gate.add_permits(1);
    harness.acknowledge(1).await;
    harness.router.tick().await.unwrap();
    {
        let mut manager = harness.manager.lock().unwrap();
        let limits = PluginLimits::default();
        let mut context = store(&manager.engine, &limits);
        context.data_mut().configuration = vec![22];
        let component = Component::from_file(&manager.engine, guest()).unwrap();
        manager.pending.push(PendingPlugin {
            component,
            store: context,
            manifest: PluginManifest {
                id: PluginId("example.other".into()),
                name: "Other".into(),
                version: "1".into(),
                api_version: PLUGIN_API_VERSION.into(),
                required: true,
                config: None,
                capabilities: [
                    PluginCapability::ReadPlayers,
                    PluginCapability::ReadInventory,
                ]
                .into(),
                subscriptions: [EventKind::PlayerJump].into(),
                limits,
            },
        });
    }
    harness.action().await.unwrap();
    assert!(harness.logs().iter().any(|log| log == "isolated"));
    assert_eq!(provider.writes.load(Ordering::SeqCst), 1);
    assert_eq!(harness.count(), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_owned_image_cannot_authorize_a_write_after_actual_world_migration() {
    let provider = Arc::new(Provider::new());
    let mut harness = Harness::new(23, provider.clone(), PluginLimits::default(), true);
    harness.action().await.unwrap();
    let world = harness
        .router
        .create_world(ServerConfig {
            world: crate::world::WorldConfig {
                dimension: 1,
                ..crate::world::WorldConfig::void_spawn_platform(1)
            },
            ..Default::default()
        })
        .unwrap();
    harness.router.add(world).unwrap();
    crate::server::worlds::request_transfer_checked(
        harness.router.world_mut(0).unwrap().ecs.world_mut(),
        harness.player,
        1,
        glam::DVec3::new(4.5, 16.0, 4.5),
        0,
        "inventory-origin".into(),
        |_, _, _| true,
    )
    .unwrap();
    provider.gate.add_permits(1);
    harness.router.tick().await.unwrap();
    let (dimension, player) = harness.router.locate_session(1).unwrap();
    assert_eq!(dimension, 1);
    let writes = provider.writes.load(Ordering::SeqCst);
    harness
        .router
        .world_mut(1)
        .unwrap()
        .ecs
        .world_mut()
        .resource_mut::<EventBuffer>()
        .push(ServerEvent::PlayerJump { entity: player });
    harness.router.tick().await.unwrap();
    assert!(harness.logs().iter().any(|log| log == "wrong-world"));
    assert_eq!(harness.retained(), (0, 0));
    assert_eq!(provider.writes.load(Ordering::SeqCst), writes);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retained_old_operation_does_not_block_canonical_receipt_replay_after_reconnect() {
    let provider = Arc::new(Provider::new());
    let mut harness = Harness::new(24, provider.clone(), PluginLimits::default(), true);
    harness.action().await.unwrap();
    provider.gate.add_permits(1);
    harness.acknowledge(1).await;
    harness.router.tick().await.unwrap();
    let data = provider
        .records
        .lock()
        .unwrap()
        .get(&uuid::Uuid::from_u128(1))
        .unwrap()
        .clone();
    harness.router.despawn_player(1);
    provider.gate.add_permits(8);
    let (outbound_tx, packets) = tokio::sync::mpsc::channel(65536);
    harness.player = harness
        .router
        .world_mut(0)
        .unwrap()
        .spawn_player(crate::server::game::PlayerSpawnData {
            session_id: 2,
            runtime_id: 2,
            display_name: "Player0".into(),
            xuid: Some("1".into()),
            uuid: Some(uuid::Uuid::from_u128(1).to_string()),
            position: glam::DVec3::new(0.5, 17.62, 0.5),
            outbound_tx,
            saved_data: Some(data),
            chunk_radius: 0,
        })
        .unwrap();
    harness._packets = packets;
    harness.action().await.unwrap();
    assert!(harness.logs().iter().any(|log| log == "reconnect-receipt"));
    assert_eq!(harness.count(), 3);
    assert_eq!(harness.retained(), (0, 0));
    let record = provider
        .records
        .lock()
        .unwrap()
        .get(&uuid::Uuid::from_u128(1))
        .unwrap()
        .clone();
    assert_eq!(record.inventory_revision, 1);
    assert_eq!(record.inventory_effects.len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_proven_rejected_request_can_retry_while_its_original_ticket_is_retained() {
    let provider = Arc::new(Provider::new());
    let mut harness = Harness::new(25, provider.clone(), PluginLimits::default(), true);
    harness.action().await.unwrap();
    let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
    let item = world
        .resource::<crate::server::game::types::ItemRegistryResource>()
        .0
        .stack("minecraft:diamond", 1)
        .unwrap()
        .into_slot();
    world
        .get_mut::<MainInventory>(harness.player)
        .unwrap()
        .0
        .set_item(0, item)
        .unwrap();
    harness.action().await.unwrap();
    assert!(harness.logs().iter().any(|log| log == "rejected"));
    assert_eq!(harness.retained(), (0, 0));
    harness.action().await.unwrap();
    assert!(harness.logs().iter().any(|log| log == "retry-staged"));
    assert_eq!(harness.retained().1, 1);
    provider.gate.add_permits(1);
    harness.acknowledge(1).await;
    harness.router.tick().await.unwrap();
    assert_eq!(harness.count(), 3);
    assert_eq!(harness.retained(), (0, 0));
    assert_eq!(provider.writes.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn historical_archive_inventory_cannot_authorize_a_live_inventory_replacement() {
    let provider = Arc::new(Provider::new());
    let mut harness = Harness::new(26, provider.clone(), PluginLimits::default(), true);
    harness.action().await.unwrap();
    assert!(
        harness
            .logs()
            .iter()
            .any(|log| log == "archive-is-observation")
    );
    assert_eq!(harness.count(), 0);
    assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
    assert_eq!(harness.retained(), (0, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn valid_default_cancellation_preserves_an_independent_durable_inventory_intent() {
    let provider = Arc::new(Provider::new());
    let mut harness = Harness::new(27, provider.clone(), PluginLimits::default(), true);
    let position = harness
        .router
        .world(0)
        .unwrap()
        .ecs
        .world()
        .get::<crate::entity::components::Position>(harness.player)
        .unwrap()
        .0;
    harness.router.tick().await.unwrap();
    assert!(!crate::server::game::player_events::initial_state_pending(
        harness.router.world(0).unwrap().ecs.world(),
        harness.player,
    ));
    harness.move_to(3.5).await.unwrap();
    assert_eq!(
        harness
            .router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get::<crate::entity::components::Position>(harness.player)
            .unwrap()
            .0,
        position
    );
    assert_eq!(harness.retained().1, 1);
    provider.gate.add_permits(1);
    harness.acknowledge(1).await;
    harness.router.tick().await.unwrap();
    assert_eq!(harness.count(), 3);
    assert_eq!(provider.writes.load(Ordering::SeqCst), 1);
    assert_eq!(harness.retained(), (0, 0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_malformed_callback_result_discards_its_staged_durable_inventory_intent() {
    let provider = Arc::new(Provider::new());
    let mut harness = Harness::new(28, provider.clone(), PluginLimits::default(), true);
    harness.router.tick().await.unwrap();
    assert!(!crate::server::game::player_events::initial_state_pending(
        harness.router.world(0).unwrap().ecs.world(),
        harness.player,
    ));
    assert!(harness.move_to(3.5).await.is_err());
    assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
    assert_eq!(harness.retained(), (0, 0));
    assert_eq!(
        harness
            .router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .resource::<Completions>()
            .0,
        0
    );
}
