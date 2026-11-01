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
        manager
            .configure_storage(&crate::config::PluginStorageConfig {
                directory: archives.path().join("storage"),
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
        context.data_mut().storage_registry = manager.storage_registry.clone();
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
                    PluginCapability::InventoryMechanics,
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
        let _ = self.until_logs(wanted).await;
    }
    /// Keep the exact owned callback evidence when a control needs to correlate native facts.
    async fn until_logs(&mut self, wanted: &str) -> Vec<String> {
        let mut logs = Vec::new();
        for _ in 0..500 {
            let result = self.router.tick().await;
            logs.extend(self.logs());
            assert!(result.is_ok(), "router: {result:?}; guest: {logs:?}");
            if logs.iter().any(|line| line == wanted) {
                return logs;
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
    assert!(retired.source_actor.as_ref().unwrap().supplement.is_some());
    let (joining, mut packets) = h.spawn(2, Some(retired));
    assert!(player_events::initial_state_pending(h.world(), joining));
    assert!(player_state::source_actors::admission_blocked(
        h.world(),
        joining
    ));
    assert!(!h.router.world_mut(0).unwrap().save_player(2).await);
    assert!(packets.try_recv().is_err());
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

/// Ownership acknowledgement never replays its older physical capture into a live body.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_live_checkpoint_keeps_damage_motion_food_cold_and_age_during_ack() {
    use crate::entity::components::{Age, Hunger, Position, Velocity};
    use crate::entity::freezing::FreezingState;
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(SourceAckGate {
        inner: Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        written: std::sync::atomic::AtomicBool::new(false),
        release: tokio::sync::Semaphore::new(0),
    });
    let mut h = Harness::new(32, provider.clone(), tempfile::tempdir().unwrap(), true);
    h.prepare_subject();
    let actor = h.subject.unwrap();
    h.retain().await;
    assert!(!player_state::quarantine::is_held(h.world(), actor));
    assert!(player_state::source_actors::pending(h.world(), actor));
    for _ in 0..300 {
        h.router.tick().await.unwrap();
        if provider.written.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(provider.written.load(std::sync::atomic::Ordering::Acquire));
    let before = player_state::capture(h.world(), actor).unwrap();
    let hit = crate::entity::player_life::hurt_player(
        h.world_mut(),
        actor,
        &Hit {
            damage: 2.0,
            cause: ActorDamageCause::Override,
            attacker: None,
            by_player: false,
            knockback_from: None,
            knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
        },
    );
    assert!(hit > 0.0);
    h.world_mut().entity_mut(actor).insert((
        Velocity(glam::DVec3::new(0.08, 0.0, 0.0)),
        FreezingState::from_strength(0.5).unwrap(),
        Hunger {
            food_level: 10,
            saturation: 0.0,
            exhaustion: 5.0,
        },
        crate::server::game::hunger::PlayerFoodPolicy::default(),
    ));
    let position = h.world().get::<Position>(actor).unwrap().0;
    for _ in 0..5 {
        h.router.tick().await.unwrap();
    }
    assert!(h.world().get::<Age>(actor).unwrap().0 > before.data.age_ticks);
    assert_ne!(h.world().get::<Position>(actor).unwrap().0, position);
    assert!(h.world().get::<Hunger>(actor).unwrap().food_level < 10);
    assert!(h.world().get::<FreezingState>(actor).unwrap().strength() < 0.5);
    let progressed = player_state::capture(h.world(), actor).unwrap();
    provider.release.add_permits(2);
    h.until("source:live-latest").await;
    assert!(h.world().get::<Health>(actor).unwrap().current <= before.data.health - 2.0);
    assert!(!player_state::quarantine::is_held(h.world(), actor));
    let data = provider
        .inner
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    assert!(data.health <= progressed.data.health);
    assert!(data.food < 10);
    assert!(data.age_ticks >= progressed.data.age_ticks);
    assert!(data.cold_exposure <= progressed.data.cold_exposure);
    assert_eq!(data.inventory[0].count, 7);
    let Harness {
        router,
        manager,
        archives,
        ..
    } = h;
    drop(router);
    drop(manager);
    drop(provider);
    let mut cold = Harness::new(
        3,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        false,
    );
    cold.until("source:recreated").await;
    let body = cold.body();
    assert!(cold.world().get::<PlayerSession>(body).is_none());
    assert!(player_state::quarantine::is_held(cold.world(), body));
    assert_eq!(cold.stored().await.age_ticks, data.age_ticks);
}

/// The real dead-only terminal gate suppresses drops and supersedes a pending live ACK.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_live_checkpoint_death_before_ack_never_reinstalls_active_values() {
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(SourceAckGate {
        inner: Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        written: std::sync::atomic::AtomicBool::new(false),
        release: tokio::sync::Semaphore::new(0),
    });
    let mut h = Harness::new(30, provider.clone(), tempfile::tempdir().unwrap(), true);
    h.prepare_subject();
    let actor = h.subject.unwrap();
    h.retain().await;
    for _ in 0..300 {
        h.router.tick().await.unwrap();
        if provider.written.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    let hit = crate::entity::player_life::hurt_player(
        h.world_mut(),
        actor,
        &Hit {
            damage: 100.0,
            cause: ActorDamageCause::Override,
            attacker: None,
            by_player: false,
            knockback_from: None,
            knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
        },
    );
    assert!(hit > 0.0);
    assert!(player_state::quarantine::is_held(h.world(), actor));
    assert_eq!(
        h.world_mut()
            .query::<&crate::entity::components::DroppedItem>()
            .iter(h.world())
            .count(),
        0
    );
    provider.release.add_permits(1);
    for _ in 0..300 {
        h.router.tick().await.unwrap();
        if provider
            .inner
            .load(uuid::Uuid::from_u128(1))
            .await
            .unwrap()
            .is_some_and(|data| {
                data.source_actor.is_some_and(|source| {
                    source.phase == crate::storage::source::SourcePhase::Terminal
                })
            })
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(player_state::quarantine::is_held(h.world(), actor));
    provider.release.add_permits(1);
    h.until("source:live-terminal").await;
    let terminal = provider
        .inner
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        terminal.source_actor.as_ref().unwrap().phase,
        crate::storage::source::SourcePhase::Terminal
    );
    assert_eq!(terminal.health, 0.0);
    assert!(terminal.inventory.iter().all(|item| item.count == 0));
    for _ in 0..3 {
        h.router.tick().await.unwrap();
    }
    assert!(
        h.world_mut()
            .query::<&crate::entity::components::DroppedItem>()
            .iter(h.world())
            .count()
            > 0
    );
}

/// A durable provisional reference survives a trap without manufacturing canonical authority.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_live_provisional_trap_is_unowned_after_reload() {
    let directory = tempfile::tempdir().unwrap();
    let mut h = Harness::new(
        31,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        tempfile::tempdir().unwrap(),
        true,
    );
    h.prepare_subject();
    let actor = h.subject.unwrap();
    scope_callback(&mut h.router, actor);
    h.router.tick().await.unwrap();
    assert!(h.router.prepare_disconnect(1));
    h.router.despawn_player(1);
    assert!(h.manager.lock().unwrap().check_required().is_err());
    assert!(!player_state::source_actors::pending(h.world(), actor));
    let Harness {
        router,
        manager,
        archives,
        ..
    } = h;
    drop(router);
    drop(manager);
    let mut cold = Harness::new(
        8,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        false,
    );
    cold.until("source:unowned").await;
    assert_eq!(
        cold.world_mut()
            .query::<&Player>()
            .iter(cold.world())
            .count(),
        0
    );
}

/// Closing the original world after durable installation does not turn absence into consumption.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_live_checkpoint_closed_world_reconciles_the_acknowledged_row() {
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(SourceAckGate {
        inner: Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        written: std::sync::atomic::AtomicBool::new(false),
        release: tokio::sync::Semaphore::new(0),
    });
    let mut h = Harness::new(30, provider.clone(), tempfile::tempdir().unwrap(), true);
    h.prepare_subject();
    h.retain().await;
    for _ in 0..300 {
        h.router.tick().await.unwrap();
        if provider.written.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(provider.written.load(std::sync::atomic::Ordering::Acquire));
    assert!(!player_state::quarantine::is_held(
        h.world(),
        h.subject.unwrap()
    ));
    let Harness {
        router,
        manager,
        archives,
        ..
    } = h;
    drop(router);
    drop(manager);
    provider.release.add_permits(1);
    for _ in 0..300 {
        if Arc::strong_count(&provider) == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert_eq!(Arc::strong_count(&provider), 1);
    drop(provider);
    let mut cold = Harness::new(
        17,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        false,
    );
    cold.until("source:recreated").await;
    let body = cold.body();
    assert!(cold.world().get::<PlayerSession>(body).is_none());
    assert_eq!(cold.stored().await.inventory[0].count, 7);
}

/// Exact native lineage and physical eligibility are revalidated after callback staging.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_live_source_rejects_foreign_quarantined_and_replaced_lineages() {
    use player_state::source_actors as native;
    let directory = tempfile::tempdir().unwrap();
    let mut h = Harness::new(
        30,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        tempfile::tempdir().unwrap(),
        true,
    );
    h.prepare_subject();
    h.retain().await;
    h.until("source:live-ack").await;
    let actor = h.subject.unwrap();
    let reference = h
        .world()
        .get::<native::LiveSource>(actor)
        .unwrap()
        .0
        .reference
        .clone();
    let lineage = h
        .world()
        .get::<players::PlayerLineage>(actor)
        .unwrap()
        .clone();
    let reserve =
        Arc::new(|_: usize| -> Result<Arc<dyn Send + Sync>, native::Error> { Ok(Arc::new(())) });
    let intent =
        |reference: native::Reference,
         lineage: players::PlayerLineage,
         snapshot: Arc<player_state::PlayerSnapshot>| native::Intent::Live {
            player: actor,
            lineage,
            snapshot,
            reference,
        };
    let snapshot = Arc::new(player_state::capture(h.world(), actor).unwrap());
    let mut foreign = reference.clone();
    foreign.module = "example.other".into();
    let prepared = native::prepare(
        h.world(),
        intent(foreign, lineage.clone(), snapshot.clone()),
        Arc::new(()),
        reserve.clone(),
    )
    .unwrap();
    let operation = prepared.operation.clone();
    native::submit(h.world_mut(), prepared);
    assert!(matches!(
        operation.status(),
        native::Status::Rejected(native::Error::NotOwned)
    ));
    let held = player_state::quarantine::hold(
        h.world_mut(),
        actor,
        snapshot.clone(),
        player_state::quarantine::Role::Source,
        Arc::new(()),
    )
    .unwrap();
    let prepared = native::prepare(
        h.world(),
        intent(reference.clone(), lineage.clone(), snapshot.clone()),
        Arc::new(()),
        reserve.clone(),
    )
    .unwrap();
    let operation = prepared.operation.clone();
    native::submit(h.world_mut(), prepared);
    assert!(matches!(
        operation.status(),
        native::Status::Rejected(native::Error::Busy)
    ));
    assert!(player_state::quarantine::release_source(
        h.world_mut(),
        &held
    ));
    // Retaining the old lineage value cannot authorize a replacement component lineage.
    h.world_mut()
        .entity_mut(actor)
        .remove::<players::PlayerLineage>();
    players::ensure_identity(h.world_mut(), actor).unwrap();
    let prepared = native::prepare(
        h.world(),
        intent(reference, lineage, snapshot),
        Arc::new(()),
        reserve,
    )
    .unwrap();
    let operation = prepared.operation.clone();
    native::submit(h.world_mut(), prepared);
    assert!(matches!(
        operation.status(),
        native::Status::Rejected(native::Error::SourceGone)
    ));
}

/// A remove decision cancels departure-only checkpoint staging before ordinary persistence.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_live_checkpoint_remove_keeps_normal_departure_save_and_no_authority() {
    let directory = tempfile::tempdir().unwrap();
    let mut h = Harness::new(
        34,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        tempfile::tempdir().unwrap(),
        true,
    );
    h.prepare_subject();
    let actor = h.subject.unwrap();
    scope_callback(&mut h.router, actor);
    h.router.tick().await.unwrap();
    assert!(h.router.prepare_disconnect(1));
    assert!(!player_state::source_actors::pending(h.world(), actor));
    assert!(h.router.world_mut(0).unwrap().save_player(1).await);
    h.router.despawn_player(1);
    assert!(h.world().get_entity(actor).is_err());
    assert!(h.stored().await.source_actor.is_none());
    let Harness {
        router,
        manager,
        archives,
        ..
    } = h;
    drop(router);
    drop(manager);
    let mut cold = Harness::new(
        8,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        false,
    );
    cold.until("source:unowned").await;
}

/// Persisting provisional history cannot authorize an initial full-state installation.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_unowned_provisional_archive_cannot_authorize_held_state_install() {
    let directory = tempfile::tempdir().unwrap();
    let mut h = Harness::new(
        31,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        tempfile::tempdir().unwrap(),
        true,
    );
    h.prepare_subject();
    let actor = h.subject.unwrap();
    scope_callback(&mut h.router, actor);
    h.router.tick().await.unwrap();
    assert!(h.router.prepare_disconnect(1));
    h.router.despawn_player(1);
    assert!(h.manager.lock().unwrap().check_required().is_err());
    let Harness {
        router,
        manager,
        archives,
        ..
    } = h;
    drop(router);
    drop(manager);
    let mut cold = Harness::new(
        38,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        true,
    );
    cold.until("source:provisional-install-rejected").await;
    let actor = cold.subject.unwrap();
    assert!(player_events::initial_state_pending(cold.world(), actor));
    assert!(!player_state::quarantine::is_held(cold.world(), actor));
    assert!(cold.world().get::<MainInventory>(actor).unwrap().0.slots()[0].is_empty());
    assert!(
        cold.world()
            .resource::<crate::server::game::types::PlayerProviderResource>()
            .0
            .load(uuid::Uuid::from_u128(1))
            .await
            .unwrap()
            .is_none()
    );
}

/// Real ownership IO keeps the source vulnerable; publication moves its current native buffers.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_live_rebind_moves_latest_damage_inventory_and_timers_into_reserved_initial_batch()
 {
    use crate::entity::components::{Age, Hunger, Position, Velocity};
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(SourceAckGate {
        inner: Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        written: std::sync::atomic::AtomicBool::new(false),
        release: tokio::sync::Semaphore::new(0),
    });
    provider.release.add_permits(1);
    let mut h = Harness::new(30, provider.clone(), tempfile::tempdir().unwrap(), true);
    h.prepare_subject();
    h.retain().await;
    h.until("source:live-ack").await;
    let source = h.subject.unwrap();
    let data = h.stored().await;
    provider
        .written
        .store(false, std::sync::atomic::Ordering::Release);
    h.load(40);
    let (target, mut packets) = h.spawn(2, Some(data));
    h.until("rebind:staged").await;
    for _ in 0..300 {
        h.router.tick().await.unwrap();
        if provider.written.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(provider.written.load(std::sync::atomic::Ordering::Acquire));
    assert!(!player_state::quarantine::is_held(h.world(), source));
    assert!(player_state::quarantine::is_held(h.world(), target));
    assert!(packets.try_recv().is_err());
    h.world_mut()
        .get_mut::<crate::entity::components::Effects>(source)
        .unwrap()
        .add(crate::entity::components::EffectType::SPEED, 1, 600);
    let age = h.world().get::<Age>(source).unwrap().0;
    h.world_mut().entity_mut(source).insert((
        Velocity(glam::DVec3::new(0.05, 0.0, 0.0)),
        Hunger {
            food_level: 10,
            saturation: 0.0,
            exhaustion: 5.0,
        },
        crate::server::game::hunger::PlayerFoodPolicy::default(),
    ));
    let damage = crate::entity::player_life::hurt_player(
        h.world_mut(),
        source,
        &Hit {
            damage: 2.0,
            cause: ActorDamageCause::Override,
            attacker: None,
            by_player: false,
            knockback_from: None,
            knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
        },
    );
    assert!(damage > 0.0);
    for _ in 0..5 {
        h.router.tick().await.unwrap();
    }
    let position = h.world().get::<Position>(source).unwrap().0;
    assert!(h.world().get::<Age>(source).unwrap().0 > age);
    provider.release.add_permits(1);
    h.until("rebind:published").await;
    assert!(h.world().get_entity(source).is_err());
    assert!(h.world().get::<Health>(target).unwrap().current <= 7.0);
    assert!(h.world().get::<Age>(target).unwrap().0 > age);
    assert!(h.world().get::<Hunger>(target).unwrap().food_level < 10);
    assert!(h.world().get::<Position>(target).unwrap().0.x >= position.x);
    assert_eq!(
        h.world().get::<MainInventory>(target).unwrap().0.slots()[0].count,
        6
    );
    assert!(!player_events::initial_state_pending(h.world(), target));
    let mut encoded = 0;
    let (mut effect, mut motion, mut position, mut life) = (false, false, false, false);
    while let Ok(packet) = packets.try_recv() {
        let mut bytes = bytes::BytesMut::new();
        packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
        jolyne::valentine::BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap();
        match &packet.data {
            jolyne::valentine::McpePacketData::MobEffectPacket(value) => {
                effect |= value.effect_id == 1 && value.effect_duration_ticks > 0
            }
            jolyne::valentine::McpePacketData::SetActorMotionPacket(_) => motion = true,
            jolyne::valentine::McpePacketData::MovePlayerPacket(_) => position = true,
            jolyne::valentine::McpePacketData::ActorEventPacket(value) => {
                life |= value.event_id == jolyne::valentine::EnumsActorEvent::SpawnAlive
            }
            _ => (),
        }
        encoded += 1;
    }
    assert!(encoded > 0);
    assert!(effect && motion && position && life);
    let row = provider
        .inner
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.source_actor.as_ref().unwrap().phase,
        crate::storage::source::SourcePhase::Transferred
    );
    assert!(row.restoration_checkpoint.is_some());
}

/// Genuine death during ownership IO supersedes its live receipt before any publication.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_live_rebind_pending_death_keeps_replacement_unpublished_until_terminal_ack()
 {
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(SourceAckGate {
        inner: Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        written: std::sync::atomic::AtomicBool::new(false),
        release: tokio::sync::Semaphore::new(1),
    });
    let mut h = Harness::new(30, provider.clone(), tempfile::tempdir().unwrap(), true);
    h.prepare_subject();
    h.retain().await;
    h.until("source:live-ack").await;
    let source = h.subject.unwrap();
    let data = h.stored().await;
    provider
        .written
        .store(false, std::sync::atomic::Ordering::Release);
    h.load(40);
    let (target, mut packets) = h.spawn(2, Some(data));
    h.until("rebind:staged").await;
    for _ in 0..300 {
        h.router.tick().await.unwrap();
        if provider.written.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(provider.written.load(std::sync::atomic::Ordering::Acquire));
    let damage = crate::entity::player_life::hurt_player(
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
    assert!(damage > 0.0);
    assert!(player_state::quarantine::is_held(h.world(), source));
    provider.release.add_permits(1);
    for _ in 0..300 {
        h.router.tick().await.unwrap();
        if provider
            .inner
            .load(uuid::Uuid::from_u128(1))
            .await
            .unwrap()
            .is_some_and(|row| {
                row.source_actor.is_some_and(|source| {
                    source.phase == crate::storage::source::SourcePhase::Terminal
                })
            })
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(packets.try_recv().is_err());
    assert!(player_events::initial_state_pending(h.world(), target));
    assert_eq!(
        h.world_mut()
            .query::<&crate::entity::components::DroppedItem>()
            .iter(h.world())
            .count(),
        0
    );
    provider.release.add_permits(1);
    h.until("rebind:terminal").await;
    let row = provider
        .inner
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.source_actor.as_ref().unwrap().phase,
        crate::storage::source::SourcePhase::Terminal
    );
    assert!(row.inventory.iter().all(|slot| slot.count == 0));
    assert!(packets.try_recv().is_err());
    assert!(player_events::initial_state_pending(h.world(), target));
    assert!(
        h.world_mut()
            .query::<&crate::entity::components::DroppedItem>()
            .iter(h.world())
            .count()
            > 0
    );
}

/// A persisted generated intent from a failed callback never becomes an ownership ACK.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_live_rebind_trap_intent_cold_readback_has_no_receipt_or_fake_player() {
    let directory = tempfile::tempdir().unwrap();
    let mut h = Harness::new(
        30,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        tempfile::tempdir().unwrap(),
        true,
    );
    h.prepare_subject();
    h.retain().await;
    h.until("source:live-ack").await;
    let source = h.subject.unwrap();
    let data = h.stored().await;
    h.load(41);
    let (target, mut packets) = h.spawn(2, Some(data));
    let mut failed = false;
    for _ in 0..300 {
        if h.router.tick().await.is_err() {
            failed = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(failed);
    assert!(h.world().get::<Health>(source).is_some());
    assert!(!player_state::quarantine::is_held(h.world(), source));
    assert!(!player_state::quarantine::is_held(h.world(), target));
    assert!(packets.try_recv().is_err());
    let Harness {
        router,
        manager,
        archives,
        ..
    } = h;
    drop(router);
    drop(manager);
    let mut cold = Harness::new(
        42,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        false,
    );
    cold.until("rebind:cold-unowned").await;
    assert_eq!(
        cold.world_mut()
            .query::<&Player>()
            .iter(cold.world())
            .count(),
        0
    );
}

/// The post-ACK publication wait is not a protected period for the original body.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_live_rebind_backpressure_rolls_buffers_back_and_keeps_source_attackable()
{
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(SourceAckGate {
        inner: Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        written: std::sync::atomic::AtomicBool::new(false),
        release: tokio::sync::Semaphore::new(1),
    });
    let mut h = Harness::new(30, provider.clone(), tempfile::tempdir().unwrap(), true);
    h.prepare_subject();
    let base_initial_count = h
        .router
        .world(0)
        .unwrap()
        .initial_packet_batch(h.subject.unwrap())
        .unwrap()
        .packets
        .len();
    h.retain().await;
    h.until("source:live-ack").await;
    let source = h.subject.unwrap();
    let row = h.stored().await;
    provider
        .written
        .store(false, std::sync::atomic::Ordering::Release);
    h.load(40);
    let (target, mut packets) = h.spawn(2, Some(row));
    h.until("rebind:staged").await;
    for _ in 0..300 {
        h.router.tick().await.unwrap();
        if provider.written.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    let sender = h
        .world()
        .get::<PlayerSession>(target)
        .unwrap()
        .outbound_tx
        .clone();
    let pressure = jolyne::valentine::McpePacket::from(crate::world::clock::clock_registry_packet(
        *h.world().resource::<crate::world::rules::WorldTime>(),
    ));
    while sender.try_send(pressure.clone()).is_ok() {}
    assert_eq!(sender.capacity(), 0);
    provider.release.add_permits(1);
    h.until("rebind:publish-staged").await;
    for _ in 0..3 {
        h.router.tick().await.unwrap();
    }
    assert!(h.world().get::<Health>(source).is_some());
    assert!(!player_state::quarantine::is_held(h.world(), source));
    assert!(player_state::quarantine::is_held(h.world(), target));
    // Capacity for the base batch alone cannot publish without final effects/position/motion.
    for _ in 0..base_initial_count {
        packets.try_recv().unwrap();
    }
    assert_eq!(sender.capacity(), base_initial_count);
    h.router.tick().await.unwrap();
    assert!(h.world().get::<Health>(source).is_some());
    assert!(!player_state::quarantine::is_held(h.world(), source));
    assert!(player_state::quarantine::is_held(h.world(), target));
    let damage = crate::entity::player_life::hurt_player(
        h.world_mut(),
        source,
        &Hit {
            damage: 2.0,
            cause: ActorDamageCause::Override,
            attacker: None,
            by_player: false,
            knockback_from: None,
            knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
        },
    );
    assert!(damage > 0.0);
    while packets.try_recv().is_ok() {}
    h.until("rebind:published").await;
    assert!(h.world().get_entity(source).is_err());
    assert!(h.world().get::<Health>(target).unwrap().current <= 7.0);
    assert!(!player_events::initial_state_pending(h.world(), target));
    let mut projected = false;
    while let Ok(packet) = packets.try_recv() {
        let mut bytes = bytes::BytesMut::new();
        packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
        let packet = jolyne::valentine::BorrowedMcpePacket::decode_inner(&mut bytes.freeze())
            .unwrap()
            .into_owned(jolyne::valentine::McpePacketArgs)
            .unwrap();
        if let jolyne::valentine::McpePacketData::UpdateAttributesPacket(attributes) = packet.data {
            projected |= attributes
                .attribute_list
                .iter()
                .any(|value| value.name == "minecraft:health" && value.current_value <= 7.0);
        }
    }
    assert!(projected);
}

/// Cold readback proves the ownership receipt but cannot revive its old source generation.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_live_rebind_cold_ack_replay_never_recreates_the_historical_source() {
    let directory = tempfile::tempdir().unwrap();
    let mut h = Harness::new(
        30,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        tempfile::tempdir().unwrap(),
        true,
    );
    h.prepare_subject();
    h.retain().await;
    h.until("source:live-ack").await;
    let row = h.stored().await;
    h.load(40);
    let (_target, _packets) = h.spawn(2, Some(row));
    h.until("rebind:published").await;
    let Harness {
        router,
        manager,
        archives,
        ..
    } = h;
    drop(router);
    drop(manager);
    let mut cold = Harness::new(
        42,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        false,
    );
    cold.until("rebind:cold-ack").await;
    assert_eq!(
        cold.world_mut()
            .query::<&Player>()
            .iter(cold.world())
            .count(),
        0
    );
}

/// Replacing a world before its ACK cannot bind a stale actor, despite equal durable dimension ID.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_live_rebind_world_closed_after_write_recovers_only_owned_receipt() {
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(SourceAckGate {
        inner: Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        written: std::sync::atomic::AtomicBool::new(false),
        release: tokio::sync::Semaphore::new(1),
    });
    let mut h = Harness::new(30, provider.clone(), tempfile::tempdir().unwrap(), true);
    h.prepare_subject();
    h.retain().await;
    h.until("source:live-ack").await;
    let row = h.stored().await;
    provider
        .written
        .store(false, std::sync::atomic::Ordering::Release);
    h.load(40);
    let (_target, mut packets) = h.spawn(2, Some(row));
    h.until("rebind:staged").await;
    for _ in 0..300 {
        h.router.tick().await.unwrap();
        if provider.written.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(provider.written.load(std::sync::atomic::Ordering::Acquire));
    assert!(packets.try_recv().is_err());
    let Harness {
        router,
        manager,
        archives,
        ..
    } = h;
    drop(router);
    drop(manager);
    provider.release.add_permits(1);
    for _ in 0..300 {
        if Arc::strong_count(&provider) == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert_eq!(Arc::strong_count(&provider), 1);
    drop(provider);
    let mut cold = Harness::new(
        42,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        false,
    );
    cold.until("rebind:cold-ack").await;
    assert_eq!(
        cold.world_mut()
            .query::<&Player>()
            .iter(cold.world())
            .count(),
        0
    );
}

/// An exact original target connection is required after the canonical result as well as before it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_live_rebind_late_replaced_target_never_receives_the_original_source() {
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(SourceAckGate {
        inner: Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        written: std::sync::atomic::AtomicBool::new(false),
        release: tokio::sync::Semaphore::new(1),
    });
    let mut h = Harness::new(30, provider.clone(), tempfile::tempdir().unwrap(), true);
    h.prepare_subject();
    h.retain().await;
    h.until("source:live-ack").await;
    let source = h.subject.unwrap();
    let row = h.stored().await;
    provider
        .written
        .store(false, std::sync::atomic::Ordering::Release);
    h.load(40);
    let (target, mut original_packets) = h.spawn(2, Some(row));
    h.until("rebind:staged").await;
    for _ in 0..300 {
        h.router.tick().await.unwrap();
        if provider.written.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    let (sender, mut replaced_packets) = tokio::sync::mpsc::channel(65536);
    h.world_mut().entity_mut(target).insert(PlayerSession::new(
        3,
        "Replacement".into(),
        Some("1".into()),
        Some(uuid::Uuid::from_u128(1).to_string()),
        sender,
    ));
    h.world_mut()
        .resource_mut::<crate::server::game::SessionEntityMap>()
        .remove(2);
    h.world_mut()
        .resource_mut::<crate::server::game::SessionEntityMap>()
        .insert(3, target);
    provider.release.add_permits(1);
    h.until("rebind:failed-publication").await;
    assert!(h.world().get::<Health>(source).is_some());
    assert!(!player_state::quarantine::is_held(h.world(), source));
    assert!(h.world().get::<Health>(target).is_none());
    assert!(original_packets.try_recv().is_err());
    assert!(replaced_packets.try_recv().is_err());
}

/// Explicit cleanup persists the latest real source before removal without publishing an old shell.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_live_rebind_explicit_retirement_preserves_latest_raw_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(SourceAckGate {
        inner: Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        written: std::sync::atomic::AtomicBool::new(false),
        release: tokio::sync::Semaphore::new(1),
    });
    let mut h = Harness::new(30, provider.clone(), tempfile::tempdir().unwrap(), true);
    h.prepare_subject();
    h.retain().await;
    h.until("source:live-ack").await;
    let source = h.subject.unwrap();
    let row = h.stored().await;
    provider
        .written
        .store(false, std::sync::atomic::Ordering::Release);
    h.load(45);
    let (_target, mut packets) = h.spawn(2, Some(row));
    h.until("rebind:staged").await;
    for _ in 0..300 {
        h.router.tick().await.unwrap();
        if provider.written.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(
        crate::entity::player_life::hurt_player(
            h.world_mut(),
            source,
            &Hit {
                damage: 2.0,
                cause: ActorDamageCause::Override,
                attacker: None,
                by_player: false,
                knockback_from: None,
                knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT
            }
        ) > 0.0
    );
    provider.release.add_permits(2);
    h.until("rebind:retired").await;
    assert!(h.world().get_entity(source).is_err());
    let current = provider
        .inner
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        current.source_actor.as_ref().unwrap().phase,
        crate::storage::source::SourcePhase::Retired
    );
    assert!(current.source_actor.as_ref().unwrap().supplement.is_some());
    assert_eq!(current.health, 7.0);
    while let Ok(packet) = packets.try_recv() {
        assert!(!matches!(
            packet.data,
            jolyne::valentine::McpePacketData::UpdateAttributesPacket(_)
                | jolyne::valentine::McpePacketData::InventoryContentPacket(_)
        ));
    }
    let Harness {
        router,
        manager,
        archives,
        ..
    } = h;
    drop(router);
    drop(manager);
    drop(provider);
    let mut cold = Harness::new(
        46,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        false,
    );
    cold.until("rebind:cold-retired").await;
    assert_eq!(
        cold.world_mut()
            .query::<&Player>()
            .iter(cold.world())
            .count(),
        0
    );
}

/// A callback failure after durable ownership never publishes or erases its actual receipt.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_live_rebind_trap_after_ack_keeps_target_unpublished_and_cold_receipt_readonly()
 {
    let directory = tempfile::tempdir().unwrap();
    let mut h = Harness::new(
        30,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        tempfile::tempdir().unwrap(),
        true,
    );
    h.prepare_subject();
    h.retain().await;
    h.until("source:live-ack").await;
    let source = h.subject.unwrap();
    let row = h.stored().await;
    h.load(44);
    let (target, mut packets) = h.spawn(2, Some(row));
    let mut failed = false;
    for _ in 0..300 {
        if h.router.tick().await.is_err() {
            failed = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(failed);
    assert!(h.world().get::<Health>(source).is_some());
    assert!(!player_state::quarantine::is_held(h.world(), source));
    assert!(player_state::quarantine::is_held(h.world(), target));
    assert!(packets.try_recv().is_err());
    assert_eq!(
        h.stored().await.source_actor.unwrap().phase,
        crate::storage::source::SourcePhase::Transferred
    );
    let Harness {
        router,
        manager,
        archives,
        ..
    } = h;
    drop(router);
    drop(manager);
    let mut cold = Harness::new(
        42,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        false,
    );
    cold.until("rebind:cold-ack").await;
    assert_eq!(
        cold.world_mut()
            .query::<&Player>()
            .iter(cold.world())
            .count(),
        0
    );
}

/// An actual source-free retained death waits for canonical post-drop ACK before values or module ownership.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_initial_terminal_waits_for_native_ack_then_adopts_without_repeating_drops()
 {
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(SourceAckGate {
        inner: Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        written: std::sync::atomic::AtomicBool::new(false),
        release: tokio::sync::Semaphore::new(0),
    });
    let mut h = Harness::new(50, provider.clone(), tempfile::tempdir().unwrap(), true);
    h.prepare_subject();
    h.retain().await;
    let source = h.subject.unwrap();
    assert!(
        provider
            .inner
            .load(uuid::Uuid::from_u128(1))
            .await
            .unwrap()
            .is_none_or(|data| data.source_actor.is_none())
    );
    assert!(
        crate::entity::player_life::hurt_player(
            h.world_mut(),
            source,
            &Hit {
                damage: 100.0,
                cause: ActorDamageCause::Override,
                attacker: None,
                by_player: false,
                knockback_from: None,
                knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT
            }
        ) > 0.0
    );
    for _ in 0..300 {
        h.router.tick().await.unwrap();
        if provider.written.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(provider.written.load(std::sync::atomic::Ordering::Acquire));
    assert!(player_state::quarantine::is_held(h.world(), source));
    assert_eq!(
        h.world_mut()
            .query::<&crate::entity::components::DroppedItem>()
            .iter(h.world())
            .count(),
        0
    );
    let native = provider
        .inner
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    assert!(native.source_actor.is_none());
    let native_death = native.native_terminal.as_ref().unwrap().death.clone();
    assert_eq!(
        native_death,
        h.world()
            .get::<crate::entity::player_life::death::NativeDeathId>(source)
            .unwrap()
            .0
            .to_string()
    );
    assert_eq!(native.health, 0.0);
    assert!(native.inventory.iter().all(|item| item.count == 0));
    // Rules changed after the genuine death do not change its captured native disposition.
    h.world_mut()
        .resource_mut::<crate::world::rules::GameRules>()
        .keep_inventory = true;
    provider.release.add_permits(1);
    let observed = h.until_logs("terminal:staged").await;
    assert!(
        observed
            .iter()
            .any(|line| line == &format!("terminal:death:{native_death}"))
    );
    let drops = h
        .world_mut()
        .query::<&crate::entity::components::DroppedItem>()
        .iter(h.world())
        .count();
    assert!(drops > 0);
    provider.release.add_permits(1);
    h.until("terminal:owned").await;
    assert_eq!(
        h.world_mut()
            .query::<&crate::entity::components::DroppedItem>()
            .iter(h.world())
            .count(),
        drops
    );
    let current = provider
        .inner
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    assert!(current.native_terminal.is_none());
    assert_eq!(
        current.source_actor.as_ref().unwrap().phase,
        crate::storage::source::SourcePhase::Terminal
    );
    assert_eq!(current.health, 0.0);
    assert!(current.inventory.iter().all(|item| item.count == 0));
    assert!(
        h.world()
            .get::<crate::entity::player_life::Dead>(source)
            .is_some()
    );
    assert!(!player_state::quarantine::is_held(h.world(), source));
    let Harness {
        router,
        manager,
        archives,
        ..
    } = h;
    drop(router);
    drop(manager);
    drop(provider);
    let mut cold = Harness::new(
        53,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        false,
    );
    cold.until("terminal:cold-owned").await;
    assert_eq!(
        cold.world_mut()
            .query::<&Player>()
            .iter(cold.world())
            .count(),
        0
    );
}

/// A durable producer archive from a trapped adoption is never source ownership.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_initial_terminal_trap_keeps_native_proof_and_archive_unowned_after_restart()
 {
    let directory = tempfile::tempdir().unwrap();
    let mut h = Harness::new(
        51,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        tempfile::tempdir().unwrap(),
        true,
    );
    h.prepare_subject();
    h.retain().await;
    let source = h.subject.unwrap();
    assert!(
        crate::entity::player_life::hurt_player(
            h.world_mut(),
            source,
            &Hit {
                damage: 100.0,
                cause: ActorDamageCause::Override,
                attacker: None,
                by_player: false,
                knockback_from: None,
                knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT
            }
        ) > 0.0
    );
    let mut failed = false;
    for _ in 0..300 {
        if h.router.tick().await.is_err() {
            failed = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(failed);
    let saved = h.stored().await;
    assert!(saved.source_actor.is_none());
    assert!(saved.native_terminal.is_some());
    assert!(saved.inventory.iter().all(|item| item.count == 0));
    let Harness {
        router,
        manager,
        archives,
        ..
    } = h;
    drop(router);
    drop(manager);
    let mut cold = Harness::new(
        54,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        archives,
        false,
    );
    cold.until("terminal:cold-unowned").await;
    assert_eq!(
        cold.world_mut()
            .query::<&Player>()
            .iter(cold.world())
            .count(),
        0
    );
}

/// Restoring raw dead values does not manufacture a committed native terminal witness.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_initial_terminal_rejects_dead_state_without_real_native_death() {
    let directory = tempfile::tempdir().unwrap();
    let mut h = Harness::new(
        52,
        Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        tempfile::tempdir().unwrap(),
        true,
    );
    h.prepare_subject();
    h.retain().await;
    let source = h.subject.unwrap();
    let mut raw = player_state::capture(h.world(), source).unwrap();
    raw.dead = true;
    raw.data.health = 0.0;
    player_state::restore(h.world_mut(), source, &raw).unwrap();
    h.until("terminal:no-witness").await;
    assert!(
        h.world_mut()
            .query::<&crate::entity::components::DroppedItem>()
            .iter(h.world())
            .next()
            .is_none()
    );
    let row = h
        .router
        .world(0)
        .unwrap()
        .ecs
        .world()
        .resource::<crate::server::game::types::PlayerProviderResource>()
        .0
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap();
    assert!(row.is_none_or(|data| data.native_terminal.is_none() && data.source_actor.is_none()));
}

/// Closing the originating world before ACK keeps only genuine canonical post-drop evidence.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_initial_terminal_world_close_preserves_native_proof_without_a_body_or_module()
 {
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(SourceAckGate {
        inner: Arc::new(LevelDBPlayerProvider::open(directory.path()).unwrap()),
        written: std::sync::atomic::AtomicBool::new(false),
        release: tokio::sync::Semaphore::new(0),
    });
    let mut h = Harness::new(50, provider.clone(), tempfile::tempdir().unwrap(), true);
    h.prepare_subject();
    h.retain().await;
    let source = h.subject.unwrap();
    assert!(
        crate::entity::player_life::hurt_player(
            h.world_mut(),
            source,
            &Hit {
                damage: 100.0,
                cause: ActorDamageCause::Override,
                attacker: None,
                by_player: false,
                knockback_from: None,
                knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT
            }
        ) > 0.0
    );
    for _ in 0..300 {
        h.router.tick().await.unwrap();
        if provider.written.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(provider.written.load(std::sync::atomic::Ordering::Acquire));
    assert!(
        h.world_mut()
            .query::<&crate::entity::components::DroppedItem>()
            .iter(h.world())
            .next()
            .is_none()
    );
    let before = provider
        .inner
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    let native_death = before.native_terminal.as_ref().unwrap().death.clone();
    let Harness {
        router, manager, ..
    } = h;
    drop(router);
    drop(manager);
    provider.release.add_permits(1);
    for _ in 0..300 {
        if Arc::strong_count(&provider) == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert_eq!(Arc::strong_count(&provider), 1);
    drop(provider);
    let reopened = LevelDBPlayerProvider::open(directory.path()).unwrap();
    let row = reopened
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    assert!(row.source_actor.is_none());
    assert_eq!(row.native_terminal.as_ref().unwrap().death, native_death);
    assert_eq!(row.health, 0.0);
    assert!(row.inventory.iter().all(|item| item.count == 0));
    row.validate(uuid::Uuid::from_u128(1)).unwrap();
}
