//! Real Rust components coordinate exact canonical source bodies across native restarts.

use super::*;
use crate::entity::components::{Health, MainInventory, Player, PlayerSession, PlayerUuid};
use crate::server::game::{player_events, player_state};
use crate::storage::{LevelDBPlayerProvider, PlayerData, PlayerProvider};
use std::{path::PathBuf, sync::OnceLock};
use wasmtime::component::Component;

/// Use an externally built exact fixture when supplied, otherwise build this owning SDK.
fn guest() -> &'static PathBuf {
    static GUEST: OnceLock<PathBuf> = OnceLock::new();
    GUEST.get_or_init(|| {
        if let Some(path) = std::env::var_os("UNASTAR_SOURCE_ACTOR_FIXTURE") {
            let path = PathBuf::from(path);
            assert!(path.is_file());
            return path;
        }
        let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| package.join("../../target"))
            .join("guest-source-actor-fixture");
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
                .arg(package.join("tests/fixtures/source-actor-component/Cargo.toml"))
                .env("CARGO_TARGET_DIR", &target)
                .output()
                .unwrap();
        assert!(
            output.status.success(),
            "source fixture build: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        target.join("wasm32-wasip2/release/source_actor_component_fixture.wasm")
    })
}

struct Harness {
    router: DimensionRouter,
    manager: Arc<Mutex<PluginManager>>,
    subject: Option<Entity>,
    subject_outbound: Option<tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>>,
    archives: tempfile::TempDir,
}
impl Harness {
    /// A real world has durable account storage and a loaded solid landing region.
    fn new(
        mode: u8,
        provider: Arc<dyn PlayerProvider>,
        archives: tempfile::TempDir,
        connect: bool,
    ) -> Self {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        });
        let world = server.ecs.world_mut();
        world.insert_resource(crate::world::WorldIdentity {
            id: uuid::Uuid::from_u128(9),
            name: "example-source-world".into(),
            dimension: 0,
        });
        world.insert_resource(crate::server::game::types::PlayerProviderResource(provider));
        world
            .resource_mut::<crate::world::rules::GameRules>()
            .do_mob_spawning = false;
        world
            .resource_mut::<crate::world::rules::GameRules>()
            .show_death_messages = false;
        let mut data = crate::world::chunk::Chunk::new(0, 0);
        for x in 0..16 {
            for z in 0..16 {
                data.set_block(x, 15, z, *crate::world::chunk::blocks::STONE);
            }
        }
        let pos = crate::world::ecs::ChunkPosition::new(0, 0);
        let chunk = world
            .spawn((crate::world::ecs::ChunkData::new(data), pos))
            .id();
        world
            .resource_mut::<crate::world::ChunkManager>()
            .insert(pos, chunk);
        player_state::restoration::install(&mut server).unwrap();
        let mut router = DimensionRouter::new(server);
        let mut manager = PluginManager::new().unwrap();
        manager
            .configure_archives(&crate::config::PluginArchiveConfig {
                directory: archives.path().join("archives"),
                ..Default::default()
            })
            .unwrap();
        let manager = Arc::new(Mutex::new(manager));
        PluginManager::attach_to_world(
            manager.clone(),
            router.world_mut(0).unwrap().ecs.world_mut(),
        );
        router.set_plugin_manager(manager.clone());
        let mut h = Self {
            router,
            manager,
            subject: None,
            subject_outbound: None,
            archives,
        };
        h.load(mode);
        if connect {
            let (player, outbound) = h.spawn(1, None);
            h.subject = Some(player);
            h.subject_outbound = Some(outbound);
        }
        h
    }
    /// Reload only the component; native accepted authority is deliberately retained.
    fn load(&mut self, mode: u8) {
        let mut manager = self.manager.lock().unwrap();
        manager.plugins.remove(&PluginId("example.sources".into()));
        let limits = PluginLimits::default();
        let mut context = store(&manager.engine, &limits);
        context.data_mut().configuration = vec![mode];
        context.data_mut().archive_registry = manager.archive_registry.clone();
        let component = Component::from_file(&manager.engine, guest()).unwrap();
        manager.pending.push(PendingPlugin {
            component,
            store: context,
            manifest: PluginManifest {
                id: PluginId("example.sources".into()),
                name: "Sources".into(),
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
                    EventKind::PlayerQuit,
                    EventKind::PlayerJoin,
                    EventKind::PlayerAdmitted,
                    EventKind::PlayerDamaged,
                    EventKind::PlayerDeath,
                    EventKind::Tick,
                ]
                .into(),
                limits,
            },
        });
        drop(manager);
        self.router.initialize_plugins().unwrap();
    }
    fn world(&self) -> &World {
        self.router.world(0).unwrap().ecs.world()
    }
    fn world_mut(&mut self) -> &mut World {
        self.router.world_mut(0).unwrap().ecs.world_mut()
    }
    /// Fresh sessions are genuine native spawn inputs, not reconstructed historical connections.
    fn spawn(
        &mut self,
        session: u64,
        saved: Option<PlayerData>,
    ) -> (
        Entity,
        tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>,
    ) {
        let (outbound_tx, rx) = tokio::sync::mpsc::channel(65536);
        let player = self
            .router
            .world_mut(0)
            .unwrap()
            .spawn_player(crate::server::game::PlayerSpawnData {
                session_id: session,
                runtime_id: session as i64,
                display_name: format!("Player{session}"),
                xuid: Some("1".into()),
                uuid: Some(uuid::Uuid::from_u128(1).to_string()),
                position: glam::DVec3::new(0.5, 17.62, 0.5),
                outbound_tx,
                saved_data: saved,
                chunk_radius: 0,
            })
            .unwrap();
        self.router.refresh_directory();
        (player, rx)
    }
    /// Only native test setup writes initial values, before any component captures them.
    fn prepare_subject(&mut self) {
        let player = self.subject.unwrap();
        let world = self.world_mut();
        crate::command::exec::set_game_mode(
            world,
            player,
            crate::entity::components::GameMode::Survival,
        );
        world.get_mut::<Health>(player).unwrap().current = 9.0;
        world
            .entity_mut(player)
            .insert(crate::server::game::hunger::PlayerFoodPolicy {
                exhaustion: false,
                food_drain: false,
                natural_regeneration: false,
                starvation: false,
            });
        let registry = world
            .resource::<crate::server::game::types::ItemRegistryResource>()
            .0
            .clone();
        let item = crate::item::ItemStack::new(registry.item_type("minecraft:stone").unwrap(), 7);
        world
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, item.clone())
            .unwrap();
        world
            .get_mut::<crate::entity::components::EnderChestInventory>(player)
            .unwrap()
            .0
            .set_item(2, item)
            .unwrap();
    }
    /// The guest sees an actual scoped input before actual native quit.
    async fn retain(&mut self) {
        let player = self.subject.unwrap();
        scope_callback(&mut self.router, player);
        self.router.tick().await.unwrap();
        assert!(self.router.prepare_disconnect(1));
        self.router.despawn_player(1);
        let logs = self.logs();
        assert!(self.world().get::<PlayerSession>(player).is_none());
        assert!(
            self.world()
                .get::<player_events::DetachedPlayer>(player)
                .is_some(),
            "retention: {logs:?}"
        );
    }
    fn logs(&self) -> Vec<String> {
        let mut manager = self.manager.lock().unwrap();
        std::mem::take(
            &mut manager
                .plugins
                .get_mut(&PluginId("example.sources".into()))
                .unwrap()
                .store
                .data_mut()
                .test_logs,
        )
    }
    /// Poll the real router/worker boundary; guest logs are results of executed imports.
    async fn until(&mut self, wanted: &str) {
        let mut logs = Vec::new();
        for _ in 0..500 {
            let result = self.router.tick().await;
            logs.extend(self.logs());
            assert!(result.is_ok(), "router: {result:?}; guest: {logs:?}");
            if logs.iter().any(|line| line == wanted) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        panic!("missing {wanted}; {logs:?}");
    }
    fn body(&mut self) -> Entity {
        let mut query = self
            .world_mut()
            .query_filtered::<Entity, (With<Player>, With<player_events::DetachedPlayer>)>();
        let actors: Vec<_> = query.iter(self.world()).collect();
        assert_eq!(actors.len(), 1);
        actors[0]
    }
    async fn stored(&self) -> PlayerData {
        self.world()
            .resource::<crate::server::game::types::PlayerProviderResource>()
            .0
            .load(uuid::Uuid::from_u128(1))
            .await
            .unwrap()
            .unwrap()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_quit_checkpoint_cold_body_activation_refence_and_retirement() {
    let directory = tempfile::tempdir().unwrap();
    let archives = tempfile::tempdir().unwrap();
    let mut h = Harness::new(
        0,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        true,
    );
    h.prepare_subject();
    let original = h.subject.unwrap();
    h.retain().await;
    assert!(player_state::quarantine::is_held(h.world(), original));
    h.until("source:sealed").await;
    let sealed = h.stored().await;
    assert_eq!(
        sealed.source_actor.as_ref().unwrap().phase,
        crate::storage::source::SourcePhase::Frozen
    );
    assert_eq!(sealed.inventory[0].count, 7);
    let Harness {
        router,
        manager,
        archives,
        ..
    } = h;
    drop(router);
    drop(manager);
    let mut h = Harness::new(
        3,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        false,
    );
    assert_eq!(h.world_mut().query::<&Player>().iter(h.world()).count(), 0);
    h.until("source:recreated").await;
    let body = h.body();
    assert!(h.world().get::<PlayerSession>(body).is_none());
    assert!(
        h.world()
            .get::<crate::world::ecs::ChunkLoader>(body)
            .is_none()
    );
    assert!(player_state::quarantine::is_held(h.world(), body));
    assert_eq!(
        h.world().get::<PlayerUuid>(body).unwrap().0,
        uuid::Uuid::from_u128(1)
    );
    h.load(4);
    h.until("source:refenced").await;
    assert_eq!(h.body(), body);
    let record = h.stored().await;
    let supplement = record
        .source_actor
        .as_ref()
        .unwrap()
        .supplement
        .as_ref()
        .unwrap();
    assert_eq!(record.health, 9.0);
    assert_eq!(record.inventory[0].count, 7);
    assert_eq!(record.ender_chest[2].count, 7);
    assert!(
        supplement
            .food_policy
            .is_some_and(|policy| !policy.natural_regeneration)
    );
    h.load(5);
    h.until("source:retired").await;
    assert!(h.world().get_entity(body).is_err());
    let retired = h.stored().await;
    assert_eq!(
        retired.source_actor.as_ref().unwrap().phase,
        crate::storage::source::SourcePhase::Retired
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_owning_admission_handoff_uses_current_canonical_stamp_and_receipts() {
    let directory = tempfile::tempdir().unwrap();
    let mut h = Harness::new(
        0,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        tempfile::tempdir().unwrap(),
        true,
    );
    h.prepare_subject();
    let stale = player_state::capture(h.world(), h.subject.unwrap())
        .unwrap()
        .data;
    h.retain().await;
    h.until("source:sealed").await;
    let source = h.body();
    let before = h.stored().await;
    assert!(before.inventory_revision > stale.inventory_revision);
    h.load(6);
    let (target, mut packets) = h.spawn(2, Some(stale));
    assert!(player_events::initial_state_pending(h.world(), target));
    h.until("source:handed-off").await;
    assert!(h.world().get::<MainInventory>(target).is_some());
    assert!(!player_state::source_actors::admission_blocked(
        h.world(),
        target
    ));
    assert!(player_state::quarantine::is_held(h.world(), source));
    let stored = h.stored().await;
    assert_eq!(
        stored.source_actor.as_ref().unwrap().phase,
        crate::storage::source::SourcePhase::Transferred
    );
    assert_eq!(stored.inventory_revision, before.inventory_revision + 1);
    assert_eq!(
        stored.inventory_effects.len(),
        before.inventory_effects.len() + 1
    );
    h.router.tick().await.unwrap();
    assert!(!player_events::initial_state_pending(h.world(), target));
    assert!(packets.try_recv().is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_pending_admission_cannot_publish_a_stale_unfenced_live_source() {
    let directory = tempfile::tempdir().unwrap();
    let mut h = Harness::new(
        0,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        tempfile::tempdir().unwrap(),
        true,
    );
    h.prepare_subject();
    let stale = player_state::capture(h.world(), h.subject.unwrap())
        .unwrap()
        .data;
    h.retain().await;
    h.until("source:sealed").await;
    let source = h.body();
    h.load(10);
    h.until("source:active-live").await;
    let (target, mut packets) = h.spawn(2, Some(stale));
    assert!(player_events::initial_state_pending(h.world(), target));
    assert!(!player_state::quarantine::is_held(h.world(), source));
    let lost = crate::entity::player_life::hurt_player(
        h.world_mut(),
        source,
        &Hit {
            damage: 3.0,
            cause: ActorDamageCause::Override,
            attacker: None,
            by_player: false,
            knockback_from: None,
            knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
        },
    );
    assert_eq!(lost, 3.0);
    assert_eq!(h.world().get::<Health>(source).unwrap().current, 6.0);
    assert!(h.logs().iter().any(|line| line == "source:damaged"));
    h.until("source:unfenced-rejected").await;
    assert!(player_events::initial_state_pending(h.world(), target));
    assert!(!player_state::quarantine::is_held(h.world(), source));
    assert_eq!(h.world().get::<Health>(source).unwrap().current, 6.0);
    assert!(packets.try_recv().is_err());
    let lost = crate::entity::player_life::hurt_player(
        h.world_mut(),
        source,
        &Hit {
            damage: 100.0,
            cause: ActorDamageCause::Override,
            attacker: None,
            by_player: false,
            knockback_from: None,
            knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
        },
    );
    assert_eq!(lost, 6.0);
    assert!(
        h.world()
            .get::<crate::entity::player_life::Dead>(source)
            .is_some()
    );
    let mut logs = Vec::new();
    for _ in 0..500 {
        h.router.tick().await.unwrap();
        logs.extend(h.logs());
        if logs.iter().any(|line| line == "source:death") {
            let stored = h.stored().await;
            assert_eq!(
                stored.source_actor.as_ref().unwrap().phase,
                crate::storage::source::SourcePhase::Terminal
            );
            assert!(stored.inventory.iter().all(|slot| slot.count == 0));
            assert!(player_events::initial_state_pending(h.world(), target));
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    panic!(
        "actual committed source death never observed: {logs:?}; canonical: {:?}",
        h.stored().await.source_actor
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_trapped_provisional_archive_is_unowned_after_cold_restart() {
    let directory = tempfile::tempdir().unwrap();
    let mut h = Harness::new(
        1,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        tempfile::tempdir().unwrap(),
        true,
    );
    h.prepare_subject();
    let player = h.subject.unwrap();
    scope_callback(&mut h.router, player);
    h.router.tick().await.unwrap();
    assert!(h.router.prepare_disconnect(1));
    h.router.despawn_player(1);
    assert!(h.manager.lock().unwrap().check_required().is_err());
    assert!(
        h.logs()
            .iter()
            .any(|line| line.contains("controlled provisional source failure"))
    );
    let Harness {
        router,
        manager,
        archives,
        ..
    } = h;
    drop(router);
    drop(manager);
    let mut h = Harness::new(
        8,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        false,
    );
    h.until("source:unowned").await;
    assert_eq!(h.world_mut().query::<&Player>().iter(h.world()).count(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_ordinary_departure_archive_cannot_authorize_source_recreation() {
    let directory = tempfile::tempdir().unwrap();
    let mut h = Harness::new(
        11,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        tempfile::tempdir().unwrap(),
        true,
    );
    h.prepare_subject();
    let player = h.subject.unwrap();
    scope_callback(&mut h.router, player);
    h.router.tick().await.unwrap();
    assert!(h.router.prepare_disconnect(1));
    h.router.despawn_player(1);
    assert!(h.world().get_entity(player).is_err());
    assert!(
        h.logs()
            .iter()
            .any(|line| line == "source:ordinary-departure")
    );
    let Harness {
        router,
        manager,
        archives,
        ..
    } = h;
    drop(router);
    drop(manager);
    let mut h = Harness::new(
        12,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        false,
    );
    h.until("source:unheld-rejected").await;
    assert_eq!(h.world_mut().query::<&Player>().iter(h.world()).count(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_dropped_checkpoint_tickets_remain_held_and_adoptable_on_reload() {
    let directory = tempfile::tempdir().unwrap();
    let mut h = Harness::new(
        13,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        tempfile::tempdir().unwrap(),
        true,
    );
    h.prepare_subject();
    h.retain().await;
    h.until("source:checkpoint-dropped").await;
    let player = h.body();
    let provider = h
        .world()
        .resource::<crate::server::game::types::PlayerProviderResource>()
        .0
        .clone();
    let mut accepted = None;
    for _ in 0..500 {
        h.router.tick().await.unwrap();
        let data = provider.load(uuid::Uuid::from_u128(1)).await.unwrap();
        if data
            .as_ref()
            .is_some_and(|data| data.source_actor.is_some())
        {
            accepted = data;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    let accepted = accepted.expect("actual canonical source acknowledgement");
    assert!(player_state::quarantine::is_held(h.world(), player));
    h.load(14);
    h.until("source:adopted-replay").await;
    assert_eq!(h.body(), player);
    assert!(player_state::quarantine::is_held(h.world(), player));
    let replay = h.stored().await;
    assert_eq!(replay.inventory_revision, accepted.inventory_revision);
    assert_eq!(replay.source_actor, accepted.source_actor);
}

/// Model a definitive atomic row write whose acknowledgement outlives the originating world.
struct SourceAckGate {
    inner: Arc<LevelDBPlayerProvider>,
    written: std::sync::atomic::AtomicBool,
    release: tokio::sync::Semaphore,
}

#[async_trait::async_trait]
impl PlayerProvider for SourceAckGate {
    fn atomic_durable_records(&self) -> bool {
        true
    }
    async fn load(&self, uuid: uuid::Uuid) -> crate::storage::StorageResult<Option<PlayerData>> {
        self.inner.load(uuid).await
    }
    async fn save(&self, uuid: uuid::Uuid, data: &PlayerData) -> crate::storage::StorageResult<()> {
        self.inner.save(uuid, data).await?;
        self.written
            .store(true, std::sync::atomic::Ordering::Release);
        self.release.acquire().await.unwrap().forget();
        Ok(())
    }
    async fn delete(&self, uuid: uuid::Uuid) -> crate::storage::StorageResult<()> {
        self.inner.delete(uuid).await
    }
    async fn close(&self) -> crate::storage::StorageResult<()> {
        self.inner.close().await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_closed_source_world_reconciles_only_the_durable_canonical_checkpoint() {
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(SourceAckGate {
        inner: Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        written: std::sync::atomic::AtomicBool::new(false),
        release: tokio::sync::Semaphore::new(0),
    });
    let mut h = Harness::new(13, provider.clone(), tempfile::tempdir().unwrap(), true);
    h.prepare_subject();
    h.retain().await;
    h.until("source:checkpoint-dropped").await;
    for _ in 0..500 {
        h.router.tick().await.unwrap();
        if provider.written.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(provider.written.load(std::sync::atomic::Ordering::Acquire));
    let player = h.body();
    assert!(player_state::quarantine::is_held(h.world(), player));
    let written = provider
        .inner
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        written.source_actor.as_ref().unwrap().phase,
        crate::storage::source::SourcePhase::Frozen
    );
    let Harness {
        router,
        manager,
        archives,
        ..
    } = h;
    drop(router);
    drop(manager);
    provider.release.add_permits(1);
    for _ in 0..500 {
        if Arc::strong_count(&provider) == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert_eq!(
        Arc::strong_count(&provider),
        1,
        "source worker released its closed-world provider"
    );
    drop(provider);
    let mut h = Harness::new(
        17,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        false,
    );
    h.until("source:recreated").await;
    let player = h.body();
    assert!(player_state::quarantine::is_held(h.world(), player));
    assert!(h.world().get::<PlayerSession>(player).is_none());
    assert_eq!(h.stored().await.inventory[0].count, 7);
}
