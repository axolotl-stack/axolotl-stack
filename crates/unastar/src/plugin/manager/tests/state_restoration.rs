//! Actual components exercise full-state acknowledgement and native source ownership.

use super::*;
use crate::entity::components::{EnderChestInventory, Health, MainInventory};
use crate::server::game::types::PlayerProviderResource;
use crate::server::game::{player_state, player_state::restoration as native};
use crate::storage::{PlayerData, PlayerProvider, StorageError, StorageResult};
use async_trait::async_trait;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio::sync::Semaphore;
use wasmtime::component::Component;

/// Build the exact current SDK contract independently of the outer native Cargo lock.
fn guest() -> &'static PathBuf {
    static GUEST: OnceLock<PathBuf> = OnceLock::new();
    GUEST.get_or_init(|| {
        let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| package.join("../../target"))
            .join("guest-state-fixture");
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
                .arg(package.join("tests/fixtures/state-component/Cargo.toml"))
                .env("CARGO_TARGET_DIR", &target)
                .output()
                .unwrap();
        assert!(
            output.status.success(),
            "state fixture build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        target.join("wasm32-wasip2/release/state_component_fixture.wasm")
    })
}

/// A controllable atomic provider distinguishes pending, durable and ambiguous writes.
struct Provider {
    records: Mutex<BTreeMap<uuid::Uuid, PlayerData>>,
    gate: Semaphore,
    writes: AtomicUsize,
    fail_after: AtomicBool,
}
impl Provider {
    /// Storage starts closed so tests can inspect actual native ownership before ACK.
    fn new() -> Self {
        Self {
            records: Default::default(),
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
    async fn load(&self, id: uuid::Uuid) -> StorageResult<Option<PlayerData>> {
        Ok(self.records.lock().unwrap().get(&id).cloned())
    }
    async fn save(&self, id: uuid::Uuid, data: &PlayerData) -> StorageResult<()> {
        self.gate.acquire().await.unwrap().forget();
        self.records.lock().unwrap().insert(id, data.clone());
        self.writes.fetch_add(1, Ordering::SeqCst);
        if self.fail_after.load(Ordering::SeqCst) {
            Err(StorageError::Database(
                "controlled lost acknowledgement".into(),
            ))
        } else {
            Ok(())
        }
    }
    async fn delete(&self, id: uuid::Uuid) -> StorageResult<()> {
        self.records.lock().unwrap().remove(&id);
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
    packets: tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>,
    _archives: tempfile::TempDir,
}
impl Harness {
    /// Reload the same module namespace while native accepted holds outlive the old instance.
    fn reload(&mut self, mode: u8) {
        let mut manager = self.manager.lock().unwrap();
        let previous = manager
            .plugins
            .remove(&PluginId("example.state".into()))
            .unwrap();
        let mut context = store(&manager.engine, &previous.manifest.limits);
        context.data_mut().configuration = vec![mode];
        context.data_mut().archive_registry = manager.archive_registry.clone();
        let component = Component::from_file(&manager.engine, guest()).unwrap();
        manager.pending.push(PendingPlugin {
            component,
            store: context,
            manifest: previous.manifest,
        });
        drop(manager);
        self.router.initialize_plugins().unwrap();
    }
    /// Load a required real component before constructing the actual authenticated actor.
    fn new(mode: u8, provider: Arc<dyn PlayerProvider>, saved: Option<PlayerData>) -> Self {
        Self::with_archives(mode, provider, saved, tempfile::tempdir().unwrap())
    }
    /// Restart retains only durable archives, never an old live world or player capability.
    fn with_archives(
        mode: u8,
        provider: Arc<dyn PlayerProvider>,
        saved: Option<PlayerData>,
        archives: tempfile::TempDir,
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
        let limits = PluginLimits {
            max_pending_inventory_operations: if mode == 17 { 0 } else { 64 },
            ..Default::default()
        };
        let mut context = store(&engine, &limits);
        context.data_mut().configuration = vec![mode];
        let mut pending = PendingPlugin {
            component: Component::from_file(&engine, guest()).unwrap(),
            store: context,
            manifest: PluginManifest {
                id: PluginId("example.state".into()),
                name: "State".into(),
                version: "1".into(),
                api_version: PLUGIN_API_VERSION.into(),
                required: true,
                config: None,
                capabilities: [
                    PluginCapability::ReadPlayers,
                    PluginCapability::ReadInventory,
                    PluginCapability::ItemValues,
                    PluginCapability::RestorePlayerState,
                    PluginCapability::PlayerStateArchives,
                    PluginCapability::ScheduleTasks,
                ]
                .into(),
                subscriptions: [
                    EventKind::PlayerJump,
                    EventKind::PlayerJoin,
                    EventKind::PlayerQuit,
                    EventKind::PlayerAdmitted,
                    EventKind::Tick,
                ]
                .into(),
                limits,
            },
        };
        if mode == 18 {
            pending
                .manifest
                .capabilities
                .remove(&PluginCapability::RestorePlayerState);
        }
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
        {
            let mut manager = manager.lock().unwrap();
            manager
                .configure_archives(&crate::config::PluginArchiveConfig {
                    directory: archives.path().join("archives"),
                    ..Default::default()
                })
                .unwrap();
            let mut pending = pending;
            pending.store.data_mut().archive_registry = manager.archive_registry.clone();
            manager.pending.push(pending);
        }
        let world = router.world_mut(0).unwrap().ecs.world_mut();
        world.insert_resource(crate::world::WorldIdentity {
            id: uuid::Uuid::from_u128(9),
            name: "example-overworld".into(),
            dimension: 0,
        });
        world.init_resource::<Completions>();
        world.add_observer(|_: On<native::Committed>, mut done: ResMut<Completions>| done.0 += 1);
        world.add_observer(|_: On<native::Failed>, mut done: ResMut<Completions>| done.0 += 1);
        PluginManager::attach_to_world(manager.clone(), world);
        router.set_plugin_manager(manager.clone());
        router.initialize_plugins().unwrap();
        let (player, packets) = Self::spawn(&mut router, 1, saved);
        router.refresh_directory();
        Self {
            router,
            manager,
            player,
            packets,
            _archives: archives,
        }
    }
    /// A fresh connection owns fresh runtime and outbound identities even for the same account.
    fn spawn(
        router: &mut DimensionRouter,
        session: u64,
        saved: Option<PlayerData>,
    ) -> (
        Entity,
        tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>,
    ) {
        Self::spawn_account(router, session, saved, 1)
    }
    /// Distinct authenticated accounts are actual spawn inputs, never edited archive identities.
    fn spawn_account(
        router: &mut DimensionRouter,
        session: u64,
        saved: Option<PlayerData>,
        account: u128,
    ) -> (
        Entity,
        tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>,
    ) {
        let (outbound_tx, packets) = tokio::sync::mpsc::channel(65536);
        let player = router
            .world_mut(0)
            .unwrap()
            .spawn_player(crate::server::game::PlayerSpawnData {
                session_id: session,
                runtime_id: session as i64,
                display_name: format!("Player{session}"),
                xuid: Some(account.to_string()),
                uuid: Some(uuid::Uuid::from_u128(account).to_string()),
                position: glam::DVec3::new(0.5, 17.62, 0.5),
                outbound_tx,
                saved_data: saved,
                chunk_radius: 0,
            })
            .unwrap();
        (player, packets)
    }
    /// Run the ordinary native buffered callback and aggregate router work boundary.
    async fn action(&mut self) -> Result<(), crate::server::worlds::WorldTickError> {
        scope_callback(&mut self.router, self.player);
        self.router.tick().await
    }
    /// Poll only the installed coordinator, avoiding unrelated simulation of expected raw values.
    async fn acknowledge(&mut self, count: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let world = self.router.world_mut(0).unwrap().ecs.world_mut();
                native::poll(world);
                world.flush();
                if world.resource::<Completions>().0 >= count {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("state acknowledgement");
    }
    /// Inspect actual guest observations without dispatching another callback.
    fn logs(&self) -> Vec<String> {
        let mut manager = self.manager.lock().unwrap();
        std::mem::take(
            &mut manager
                .plugins
                .values_mut()
                .next()
                .unwrap()
                .store
                .data_mut()
                .test_logs,
        )
    }
    /// The complete raw source is unavailable while native ownership is held.
    fn held(&self) -> bool {
        player_state::quarantine::is_held(self.router.world(0).unwrap().ecs.world(), self.player)
    }
    /// Count only gameplay publication; disconnect/diagnostic output is allowed on rejection.
    fn gameplay_packets(&mut self) -> usize {
        use jolyne::valentine::McpePacketData::*;
        std::iter::from_fn(|| self.packets.try_recv().ok())
            .filter(|packet| {
                let gameplay = matches!(
                    packet.data,
                    InventoryContentPacket(_)
                        | MobEffectPacket(_)
                        | SetActorMotionPacket(_)
                        | UpdateAttributesPacket(_)
                        | SetPlayerGameTypePacket(_)
                        | UpdateAbilitiesPacket(_)
                );
                if gameplay {
                    let mut encoded = bytes::BytesMut::new();
                    packet.data.encode_inner(&mut encoded, 0, 0).unwrap();
                    jolyne::valentine::BorrowedMcpePacket::decode_inner(&mut encoded.freeze())
                        .unwrap();
                }
                gameplay
            })
            .count()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_state_component_waits_for_full_durable_ack_then_installs_exact_native_values() {
    let provider = Arc::new(Provider::new());
    let mut h = Harness::new(0, provider.clone(), None);
    h.action().await.unwrap();
    assert!(h.held());
    assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
    assert!(h.logs().iter().any(|s| s == "queued"));
    provider.gate.add_permits(1);
    h.acknowledge(1).await;
    assert!(!h.held());
    let world = h.router.world(0).unwrap().ecs.world();
    let raw = player_state::capture(world, h.player).unwrap();
    assert_eq!(raw.data.health, 7.0);
    assert_eq!(raw.velocity, [0.25, 0.5, -0.25]);
    assert_eq!(raw.air_current, 17);
    assert_eq!(raw.data.age_ticks, 77);
    assert_eq!(raw.effects[0].duration_ticks, 93);
    assert_eq!(raw.base_game_mode, Some(0));
    assert_eq!(raw.flight_override, Some(true));
    assert_eq!(raw.data.inventory_revision, 1);
    assert_eq!(
        world.get::<MainInventory>(h.player).unwrap().0.slots()[0].count,
        3
    );
    assert_eq!(
        world
            .get::<EnderChestInventory>(h.player)
            .unwrap()
            .0
            .slots()[2]
            .count,
        3
    );
    assert!(
        provider
            .records
            .lock()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .restoration_checkpoint
            .is_some()
    );
    h.router.tick().await.unwrap();
    assert!(h.logs().iter().any(|s| s == "committed"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_state_callback_trap_writes_nothing_and_invalid_values_never_hold_native_state() {
    for mode in [1, 3] {
        let provider = Arc::new(Provider::new());
        let mut h = Harness::new(mode, provider.clone(), None);
        let result = h.action().await;
        assert_eq!(result.is_err(), mode == 1);
        assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
        assert!(!h.held());
        assert!(provider.records.lock().unwrap().is_empty());
        if mode == 3 {
            assert!(h.logs().iter().any(|s| s == "invalid-values"));
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepted_full_state_write_survives_dropped_guest_operation_and_lost_ack_is_uncertain() {
    for mode in [2, 10] {
        let provider = Arc::new(Provider::new());
        let mut h = Harness::new(mode, provider.clone(), None);
        h.action().await.unwrap();
        assert!(h.held());
        provider.fail_after.store(mode == 10, Ordering::SeqCst);
        provider.gate.add_permits(1);
        h.acknowledge(1).await;
        assert_eq!(provider.writes.load(Ordering::SeqCst), 1);
        if mode == 10 {
            assert!(h.held());
            h.router.tick().await.unwrap();
            assert!(h.logs().iter().any(|s| s == "uncertain"));
        } else {
            assert!(!h.held());
            assert_eq!(
                h.router
                    .world(0)
                    .unwrap()
                    .ecs
                    .world()
                    .get::<Health>(h.player)
                    .unwrap()
                    .current,
                7.0
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deferred_owner_can_restore_without_initial_publication_and_release_only_after_ack() {
    let provider = Arc::new(Provider::new());
    let mut h = Harness::new(4, provider.clone(), None);
    assert_eq!(h.gameplay_packets(), 0);
    h.router.tick().await.unwrap();
    assert!(h.held());
    assert_eq!(h.gameplay_packets(), 0);
    provider.gate.add_permits(1);
    h.acknowledge(1).await;
    assert_eq!(
        h.gameplay_packets(),
        0,
        "ACK alone does not resolve admission"
    );
    h.router.tick().await.unwrap();
    h.router.tick().await.unwrap();
    assert!(h.logs().iter().any(|s| s == "resolved"));
    assert!(h.gameplay_packets() > 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expected_state_changed_before_submission_is_rejected_without_storage_or_partial_mutation()
{
    let provider = Arc::new(Provider::new());
    let mut h = Harness::new(8, provider.clone(), None);
    h.action().await.unwrap();
    h.logs();
    h.router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .get_mut::<Health>(h.player)
        .unwrap()
        .current = 13.0;
    h.action().await.unwrap();
    assert!(!h.held());
    assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
    assert!(h.logs().iter().any(|s| s.starts_with("rejected:")));
    assert_eq!(
        h.router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get::<Health>(h.player)
            .unwrap()
            .current,
        13.0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unheld_duplicate_account_blocks_full_state_install_before_any_native_write() {
    let provider = Arc::new(Provider::new());
    let mut h = Harness::new(0, provider.clone(), None);
    let (_other, _packets) = Harness::spawn(&mut h.router, 2, None);
    h.action().await.unwrap();
    assert!(!h.held());
    assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
    h.router.tick().await.unwrap();
    let logs = h.logs();
    assert!(
        logs.iter().any(|s| s == "rejected:Conflict"),
        "guest observations: {logs:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stored_complete_checkpoint_hydrates_silently_before_rejected_or_deferred_admission() {
    let provider = Arc::new(Provider::new());
    let mut source = Harness::new(0, provider.clone(), None);
    source.action().await.unwrap();
    provider.gate.add_permits(1);
    source.acknowledge(1).await;
    let saved = provider
        .records
        .lock()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    drop(source);
    for mode in [5, 6] {
        let mut h = Harness::new(mode, provider.clone(), Some(saved.clone()));
        assert_eq!(
            h.gameplay_packets(),
            0,
            "stock hydration must not publish before admission"
        );
        if mode == 6 {
            let raw =
                player_state::capture(h.router.world(0).unwrap().ecs.world(), h.player).unwrap();
            assert_eq!(raw.effects[0].duration_ticks, 93);
        }
        h.router.tick().await.unwrap();
        assert_eq!(
            h.gameplay_packets(),
            0,
            "rejected/held admission remains unpublished"
        );
        if mode == 6 {
            let raw =
                player_state::capture(h.router.world(0).unwrap().ecs.world(), h.player).unwrap();
            assert_eq!(raw.data.health, 7.0);
            assert_eq!(raw.velocity, [0.25, 0.5, -0.25]);
            assert_eq!(raw.effects[0].duration_ticks, 92);
            assert!(h.logs().iter().all(|s| s != "admitted"));
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exact_fenced_source_remains_withheld_after_ack_and_guest_resource_drop() {
    for retain in [false, true] {
        let provider = Arc::new(Provider::new());
        let mut h = Harness::new(7, provider.clone(), None);
        let original =
            player_state::capture(h.router.world(0).unwrap().ecs.world(), h.player).unwrap();
        h.action().await.unwrap();
        assert!(h.held());
        let source = h.player;
        assert!(
            player_state::restore(
                h.router.world_mut(0).unwrap().ecs.world_mut(),
                source,
                &original
            )
            .is_err()
        );
        assert!(!crate::command::exec::set_mode_override(
            h.router.world_mut(0).unwrap().ecs.world_mut(),
            source,
            Some(crate::entity::components::GameMode::Creative)
        ));
        let old_budget = h
            .manager
            .lock()
            .unwrap()
            .plugins
            .values()
            .next()
            .unwrap()
            .store
            .data()
            .resources
            .value_budget
            .clone();
        if retain {
            h.router.world_mut(0).unwrap().despawn_player(1);
            let world = h.router.world(0).unwrap().ecs.world();
            assert!(
                world
                    .get::<crate::server::game::player_events::DetachedPlayer>(source)
                    .is_some()
            );
            assert!(
                world
                    .get::<crate::entity::components::PlayerSession>(source)
                    .is_none()
            );
            assert!(
                world
                    .get::<crate::entity::components::PlayerState>(source)
                    .is_none()
            );
        }
        let (target, packets) = Harness::spawn(&mut h.router, 2, None);
        h.player = target;
        h.packets = packets;
        h.router.refresh_directory();
        h.router.tick().await.unwrap();
        assert!(h.held());
        assert_eq!(h.gameplay_packets(), 0);
        provider.gate.add_permits(1);
        h.acknowledge(1).await;
        h.router.tick().await.unwrap();
        h.router.tick().await.unwrap();
        let world = h.router.world(0).unwrap().ecs.world();
        assert!(player_state::quarantine::is_held(world, source));
        assert!(world.get::<MainInventory>(source).is_none());
        assert_eq!(
            world.get::<MainInventory>(target).unwrap().0.slots()[0].count,
            3
        );
        assert!(h.logs().iter().any(|s| s == "resolved"));
        h.reload(12);
        assert!(
            old_budget.used() > 0,
            "native source buffers stay charged after unload"
        );
        h.action().await.unwrap();
        assert!(h.logs().iter().any(|s| s == "source-reacquired"));
        h.reload(14);
        h.router
            .world_mut(0)
            .unwrap()
            .ecs
            .world_mut()
            .get_mut::<Health>(target)
            .unwrap()
            .current = 3.0;
        h.router
            .world_mut(0)
            .unwrap()
            .ecs
            .world_mut()
            .get_mut::<MainInventory>(target)
            .unwrap()
            .0
            .set_item(0, Default::default())
            .unwrap();
        h.action().await.unwrap();
        assert!(h.logs().iter().any(|s| s == "replayed"));
        assert_eq!(provider.writes.load(Ordering::SeqCst), 1);
        assert_eq!(
            h.router
                .world(0)
                .unwrap()
                .ecs
                .world()
                .get::<Health>(target)
                .unwrap()
                .current,
            3.0
        );
        assert_eq!(
            h.router
                .world(0)
                .unwrap()
                .ecs
                .world()
                .get::<MainInventory>(target)
                .unwrap()
                .0
                .slots()[0]
                .count,
            0
        );
        assert!(player_state::quarantine::is_held(
            h.router.world(0).unwrap().ecs.world(),
            source
        ));
        h.router
            .world_mut(0)
            .unwrap()
            .ecs
            .world_mut()
            .despawn(source);
        h.reload(13);
        h.action().await.unwrap();
        assert!(h.logs().iter().any(|s| s == "source-absent"));
        assert_eq!(
            old_budget.used(),
            0,
            "actual actor removal releases its last native lease"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepted_write_after_original_departure_reports_real_absence_and_reopens_checkpoint() {
    let provider = Arc::new(Provider::new());
    let mut h = Harness::new(11, provider.clone(), None);
    h.action().await.unwrap();
    assert!(h.held());
    let old = h.player;
    h.router.world_mut(0).unwrap().despawn_player(1);
    assert!(
        h.router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get_entity(old)
            .is_err()
    );
    let (outbound_tx, _replacement) = tokio::sync::mpsc::channel(8);
    assert!(
        matches!(
            h.router
                .world_mut(0)
                .unwrap()
                .spawn_player(crate::server::game::PlayerSpawnData {
                    session_id: 3,
                    runtime_id: 3,
                    display_name: "Replacement".into(),
                    xuid: Some("1".into()),
                    uuid: Some(uuid::Uuid::from_u128(2).to_string()),
                    position: glam::DVec3::new(0.5, 17.62, 0.5),
                    outbound_tx,
                    saved_data: None,
                    chunk_radius: 0,
                }),
            Err(StorageError::PlayerRecoveryRequired)
        ),
        "pending authenticated account remains fenced after actor removal"
    );
    provider.gate.add_permits(1);
    h.acknowledge(1).await;
    h.router.tick().await.unwrap();
    assert!(h.logs().iter().any(|s| s == "committed-unapplied"));
    let saved = provider
        .records
        .lock()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    let (new, _packets) = Harness::spawn(&mut h.router, 2, Some(saved));
    assert_ne!(new, old);
    let raw = player_state::capture(h.router.world(0).unwrap().ecs.world(), new).unwrap();
    assert_eq!(raw.data.health, 7.0);
    assert_eq!(raw.data.inventory_revision, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_outbound_channel_cannot_receive_or_apply_an_old_full_state_completion() {
    let provider = Arc::new(Provider::new());
    let mut h = Harness::new(11, provider.clone(), None);
    h.action().await.unwrap();
    let (replacement, mut packets) = tokio::sync::mpsc::channel(128);
    h.router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .get_mut::<crate::entity::components::PlayerSession>(h.player)
        .unwrap()
        .outbound_tx = replacement;
    provider.gate.add_permits(1);
    h.acknowledge(1).await;
    assert!(h.held());
    assert!(packets.try_recv().is_err());
    assert!(
        h.router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get::<MainInventory>(h.player)
            .is_none()
    );
    h.router.tick().await.unwrap();
    assert!(h.logs().iter().any(|s| s == "committed-unapplied"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn state_image_account_scope_and_pending_capacity_are_checked_before_native_ownership() {
    for mode in [9, 17, 18, 19] {
        let provider = Arc::new(Provider::new());
        let mut h = Harness::new(mode, provider.clone(), None);
        h.action().await.unwrap();
        if mode == 9 {
            let (other, packets) = Harness::spawn_account(&mut h.router, 2, None, 2);
            h.player = other;
            h.packets = packets;
            h.router.refresh_directory();
            h.action().await.unwrap();
        }
        assert!(!h.held());
        assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
        let logs = h.logs();
        assert!(
            logs.iter().any(|s| s
                == match mode {
                    9 => "wrong-account",
                    17 => "limited",
                    18 => "denied",
                    _ => "invalid-request",
                }),
            "mode{mode}: {logs:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acknowledged_state_can_apply_to_the_original_retained_actor_without_network_input() {
    let provider = Arc::new(Provider::new());
    let mut h = Harness::new(21, provider.clone(), None);
    h.action().await.unwrap();
    h.router.world_mut(0).unwrap().despawn_player(1);
    let world = h.router.world(0).unwrap().ecs.world();
    assert!(
        world
            .get::<crate::server::game::player_events::DetachedPlayer>(h.player)
            .is_some()
    );
    assert!(h.held());
    provider.gate.add_permits(1);
    h.acknowledge(1).await;
    let world = h.router.world(0).unwrap().ecs.world();
    assert!(!h.held());
    assert!(
        world
            .get::<crate::entity::components::PlayerSession>(h.player)
            .is_none()
    );
    assert!(
        world
            .get::<crate::entity::components::PlayerInput>(h.player)
            .is_none()
    );
    assert_eq!(
        world.get::<MainInventory>(h.player).unwrap().0.slots()[0].count,
        3
    );
    h.router.tick().await.unwrap();
    assert!(h.logs().iter().any(|s| s == "committed"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn staged_source_archive_survives_trap_without_claiming_an_accepted_source_hold() {
    let provider = Arc::new(Provider::new());
    let mut h = Harness::new(22, provider.clone(), None);
    assert!(h.action().await.is_err());
    assert!(!h.held());
    assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
    {
        let mut manager = h.manager.lock().unwrap();
        let context = manager
            .plugins
            .values_mut()
            .next()
            .unwrap()
            .store
            .data_mut();
        let namespace = context.archive_namespace.as_ref().unwrap().clone();
        assert!(
            namespace
                .lock()
                .unwrap()
                .lookup("held-source")
                .unwrap()
                .is_some()
        );
    }
    let archives = std::mem::replace(&mut h._archives, tempfile::tempdir().unwrap());
    drop(h);
    let mut reopened = Harness::with_archives(13, provider, None, archives);
    reopened.action().await.unwrap();
    assert!(reopened.logs().iter().any(|s| s == "source-absent"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn historical_archive_without_a_hold_is_blocked_until_the_actual_source_departs() {
    let provider = Arc::new(Provider::new());
    let mut h = Harness::new(23, provider.clone(), None);
    h.action().await.unwrap();
    assert!(!h.held());
    let source = h.player;
    let (target, packets) = Harness::spawn(&mut h.router, 2, None);
    h.player = target;
    h.packets = packets;
    h.router.refresh_directory();
    h.router.tick().await.unwrap();
    h.router.tick().await.unwrap();
    assert!(h.logs().iter().any(|s| s == "rejected:Conflict"));
    assert_eq!(provider.writes.load(Ordering::SeqCst), 0);
    h.router.world_mut(0).unwrap().despawn_player(1);
    assert!(
        h.router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get_entity(source)
            .is_err()
    );
    h.router.tick().await.unwrap();
    assert!(h.held());
    provider.gate.add_permits(1);
    h.acknowledge(1).await;
    h.router.tick().await.unwrap();
    h.router.tick().await.unwrap();
    assert!(h.logs().iter().any(|s| s == "resolved"));
    assert_eq!(provider.writes.load(Ordering::SeqCst), 1);
}
