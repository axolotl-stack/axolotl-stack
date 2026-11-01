use super::*;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig, player_events};
use crate::server::worlds::DimensionRouter;
use crate::storage::{LevelDBPlayerProvider, PlayerProvider};
use std::path::Path;
use std::sync::Arc;

struct Retain;
impl player_events::PlayerEventHandler for Retain {
    fn on_quit(&self, _: &mut World, event: &mut player_events::PlayerQuit) {
        event.retain_entity();
    }
}

/// Real dimension worlds share one durable account provider and loaded native terrain.
fn router(provider: Arc<dyn PlayerProvider>) -> DimensionRouter {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..Default::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(crate::world::WorldIdentity {
        id: uuid::Uuid::from_u128(3),
        name: "example-source-world".into(),
        dimension: 0,
    });
    world.insert_resource(super::super::super::types::PlayerProviderResource(provider));
    world
        .resource_mut::<crate::world::rules::GameRules>()
        .do_mob_spawning = false;
    world
        .resource_mut::<crate::world::rules::GameRules>()
        .show_death_messages = false;
    world
        .resource_mut::<player_events::PlayerEventHooks>()
        .register(Retain);
    let mut column = crate::world::chunk::Chunk::new(0, 0);
    for x in 0..16 {
        for z in 0..16 {
            column.set_block(x, 15, z, *crate::world::chunk::blocks::STONE);
        }
    }
    let position = crate::world::ecs::ChunkPosition::new(0, 0);
    let chunk = world
        .spawn((crate::world::ecs::ChunkData::new(column), position))
        .id();
    world
        .resource_mut::<crate::world::ChunkManager>()
        .insert(position, chunk);
    super::super::restoration::install(&mut server).unwrap();
    DimensionRouter::new(server)
}

/// Reopen a true LevelDB instance, not an in-memory mirror of a previous ECS world.
fn open(path: &Path) -> DimensionRouter {
    router(Arc::new(LevelDBPlayerProvider::open(path).unwrap()))
}
fn world(router: &DimensionRouter) -> &World {
    router.world(0).unwrap().ecs.world()
}
fn world_mut(router: &mut DimensionRouter) -> &mut World {
    router.world_mut(0).unwrap().ecs.world_mut()
}

/// The source begins as a real native connection and is detached by the actual quit path.
fn retained(router: &mut DimensionRouter) -> Entity {
    let (tx, _rx) = tokio::sync::mpsc::channel(8192);
    let player = router
        .world_mut(0)
        .unwrap()
        .spawn_player(PlayerSpawnData {
            session_id: 1,
            runtime_id: 101,
            display_name: "ExampleSource".into(),
            xuid: Some("73".into()),
            uuid: Some(uuid::Uuid::from_u128(73).to_string()),
            position: DVec3::new(4.5, 17.62, 4.5),
            saved_data: None,
            outbound_tx: tx,
            chunk_radius: 0,
        })
        .unwrap();
    let world = world_mut(router);
    crate::command::exec::set_game_mode(world, player, GameMode::Survival);
    world.entity_mut(player).insert((
        Health::with_current(9.0, 20.0),
        Hunger {
            food_level: 7,
            saturation: 2.0,
            exhaustion: 1.0,
        },
        Age(701),
        FreezingState::from_strength(0.3).unwrap(),
    ));
    let registry = world
        .resource::<super::super::super::types::ItemRegistryResource>()
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
        .get_mut::<EnderChestInventory>(player)
        .unwrap()
        .0
        .set_item(2, item)
        .unwrap();
    player_events::disconnect(world, 1);
    assert!(world.get::<PlayerSession>(player).is_none());
    assert!(world.get::<player_events::DetachedPlayer>(player).is_some());
    player
}

/// Native tests retain explicit byte leases exactly as the component bridge does.
fn reserve() -> super::coordinator::Reserve {
    Arc::new(|bytes| {
        if bytes <= 16 * 1024 * 1024 {
            Ok(Arc::new(bytes))
        } else {
            Err(Error::LimitExceeded)
        }
    })
}
fn reference() -> Reference {
    Reference {
        module: "example.sources".into(),
        uuid: uuid::Uuid::from_u128(73),
        xuid: Some("73".into()),
        name: "ExampleSource".into(),
        nonce: uuid::Uuid::from_u128(19).to_string(),
        world: uuid::Uuid::from_u128(3),
        dimension: 0,
    }
}

/// Queue an owned native request through the real aggregate router boundary.
fn request(router: &mut DimensionRouter, intent: Intent) -> Arc<Operation> {
    let prepared = prepare(world(router), intent, Arc::new(()), reserve()).unwrap();
    let operation = prepared.operation.clone();
    submit(world_mut(router), prepared);
    operation
}
fn seal(router: &mut DimensionRouter, player: Entity) -> Arc<Operation> {
    let snapshot = Arc::new(capture(world(router), player).unwrap());
    let hold = quarantine::hold(
        world_mut(router),
        player,
        snapshot.clone(),
        quarantine::Role::Source,
        Arc::new(()),
    )
    .unwrap();
    request(
        router,
        Intent::Seal {
            player,
            hold,
            snapshot,
            reference: reference(),
        },
    )
}
async fn step(router: &mut DimensionRouter) {
    router.tick().await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
}
async fn finished(router: &mut DimensionRouter, operation: &Arc<Operation>) {
    for _ in 0..200 {
        match operation.status() {
            Status::Staged | Status::Queued => step(router).await,
            Status::Committed { .. } => return,
            Status::Rejected(error) => panic!("source request rejected: {error:?}"),
            Status::Uncertain => panic!("source request requires recovery"),
            Status::Cancelled => panic!("source request cancelled"),
        }
    }
    panic!("source request did not finish");
}
async fn canonical(router: &DimensionRouter) -> PlayerData {
    let provider = world(router)
        .resource::<super::super::super::types::PlayerProviderResource>()
        .0
        .clone();
    provider
        .load(uuid::Uuid::from_u128(73))
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn canonical_source_cold_restart_creates_a_fresh_frozen_native_actor_without_a_session() {
    let directory = tempfile::tempdir().unwrap();
    let old_runtime;
    {
        let mut router = open(directory.path());
        let player = retained(&mut router);
        old_runtime = world(&router).get::<RuntimeEntityId>(player).unwrap().0;
        let sealed = seal(&mut router, player);
        finished(&mut router, &sealed).await;
        assert_eq!(
            canonical(&router).await.source_actor.unwrap().phase,
            SourcePhase::Frozen
        );
        assert!(quarantine::is_held(world(&router), player));
    }
    let mut router = open(directory.path());
    let observed = request(&mut router, Intent::Inspect(reference()));
    finished(&mut router, &observed).await;
    assert!(observed.player(world(&router)).is_none());
    let created = request(
        &mut router,
        Intent::Continue {
            observed: observed.clone(),
            action: Action::Recreate,
        },
    );
    finished(&mut router, &created).await;
    let player = created
        .player(world(&router))
        .expect("actual recreated source");
    assert!(world(&router).get::<PlayerSession>(player).is_none());
    assert!(
        world(&router)
            .get::<player_events::DetachedPlayer>(player)
            .is_some()
    );
    assert_ne!(
        world(&router).get::<RuntimeEntityId>(player).unwrap().0,
        old_runtime
    );
    assert!(
        world(&router)
            .get::<crate::entity::components::PlayerInput>(player)
            .is_none()
    );
    assert!(
        world(&router)
            .get::<crate::world::ecs::ChunkLoader>(player)
            .is_none()
    );
    assert!(quarantine::is_held(world(&router), player));
    assert!(world(&router).get::<Health>(player).is_none());
    world_mut(&mut router).resource_mut::<TickCounter>().current = 7_000;
    let active = request(
        &mut router,
        Intent::Continue {
            observed: created,
            action: Action::Activate,
        },
    );
    finished(&mut router, &active).await;
    assert_eq!(world(&router).get::<Health>(player).unwrap().current, 9.0);
    assert!(!quarantine::is_held(world(&router), player));
    assert_eq!(
        world(&router)
            .get::<EnderChestInventory>(player)
            .unwrap()
            .0
            .item(2)
            .unwrap()
            .count,
        7
    );
    assert!(world(&router).get::<Age>(player).unwrap().0 >= 701);
    assert!(world(&router).get::<PlayerSession>(player).is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_source_cold_restart_uses_latest_acknowledged_physical_values() {
    let directory = tempfile::tempdir().unwrap();
    {
        let mut router = open(directory.path());
        let actor = retained(&mut router);
        let sealed = seal(&mut router, actor);
        finished(&mut router, &sealed).await;
        let active = request(
            &mut router,
            Intent::Continue {
                observed: sealed,
                action: Action::Activate,
            },
        );
        finished(&mut router, &active).await;
        let world = world_mut(&mut router);
        world.get_mut::<Health>(actor).unwrap().current = 3.0;
        world.get_mut::<Hunger>(actor).unwrap().food_level = 5;
        world.get_mut::<MainInventory>(actor).unwrap().0.slots_mut()[0].count = 2;
        assert_eq!(router.world_mut(0).unwrap().save_all_players().await, 1);
        let acknowledged = canonical(&router).await;
        assert_eq!(acknowledged.health, 3.0);
        assert_eq!(acknowledged.inventory[0].count, 2);
        assert!(acknowledged.source_actor.unwrap().sample > 0);
    }
    let mut router = open(directory.path());
    let observed = request(&mut router, Intent::Inspect(reference()));
    finished(&mut router, &observed).await;
    assert_eq!(
        observed
            .observation()
            .unwrap()
            .snapshot()
            .unwrap()
            .data
            .health,
        3.0
    );
    let created = request(
        &mut router,
        Intent::Continue {
            observed,
            action: Action::Recreate,
        },
    );
    finished(&mut router, &created).await;
    let active = request(
        &mut router,
        Intent::Continue {
            observed: created,
            action: Action::Activate,
        },
    );
    finished(&mut router, &active).await;
    let actor = active.player(world(&router)).unwrap();
    assert_eq!(world(&router).get::<Health>(actor).unwrap().current, 3.0);
    assert_eq!(
        world(&router)
            .get::<MainInventory>(actor)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .count,
        2
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stale_generation_and_unowned_nonce_cannot_replace_or_retire_a_native_body() {
    let directory = tempfile::tempdir().unwrap();
    let mut router = open(directory.path());
    let actor = retained(&mut router);
    let sealed = seal(&mut router, actor);
    finished(&mut router, &sealed).await;
    let active = request(
        &mut router,
        Intent::Continue {
            observed: sealed.clone(),
            action: Action::Activate,
        },
    );
    finished(&mut router, &active).await;
    let stale = request(
        &mut router,
        Intent::Continue {
            observed: sealed,
            action: Action::Retire,
        },
    );
    for _ in 0..4 {
        step(&mut router).await;
    }
    assert!(matches!(stale.status(), Status::Rejected(_)));
    assert!(world(&router).get::<Health>(actor).is_some());
    let retired = request(
        &mut router,
        Intent::Continue {
            observed: active,
            action: Action::Retire,
        },
    );
    finished(&mut router, &retired).await;
    assert!(world(&router).get_entity(actor).is_err());
    let closed = canonical(&router).await;
    assert_eq!(
        closed.source_actor.as_ref().unwrap().phase,
        SourcePhase::Retired
    );
    let attempt = request(
        &mut router,
        Intent::Continue {
            observed: retired,
            action: Action::Recreate,
        },
    );
    for _ in 0..4 {
        step(&mut router).await;
    }
    assert!(matches!(
        attempt.status(),
        Status::Rejected(Error::NotOwned)
    ));
    let mut wrong = reference();
    wrong.nonce = uuid::Uuid::from_u128(20).to_string();
    let mismatched = request(&mut router, Intent::Inspect(wrong));
    for _ in 0..100 {
        if !matches!(mismatched.status(), Status::Staged | Status::Queued) {
            break;
        }
        step(&mut router).await;
    }
    assert!(matches!(
        mismatched.status(),
        Status::Rejected(Error::Conflict)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_historical_source_without_canonical_ownership_is_unowned_not_consumed() {
    let directory = tempfile::tempdir().unwrap();
    let mut router = open(directory.path());
    let observed = request(&mut router, Intent::Inspect(reference()));
    for _ in 0..100 {
        if !matches!(observed.status(), Status::Staged | Status::Queued) {
            break;
        }
        step(&mut router).await;
    }
    assert!(matches!(
        observed.status(),
        Status::Rejected(Error::NotOwned)
    ));
    assert!(observed.player(world(&router)).is_none());
    let world = world_mut(&mut router);
    assert!(
        world
            .query_filtered::<Entity, With<Player>>()
            .iter(world)
            .next()
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cold_activation_restores_actual_food_policy_and_phase_before_the_first_food_tick() {
    let directory = tempfile::tempdir().unwrap();
    {
        let mut router = open(directory.path());
        let player = retained(&mut router);
        let world = world_mut(&mut router);
        world.get_mut::<Hunger>(player).unwrap().food_level = 20;
        world.entity_mut(player).insert((
            super::super::super::hunger::PlayerFoodPolicy {
                exhaustion: false,
                food_drain: false,
                natural_regeneration: false,
                starvation: false,
            },
            super::super::super::hunger::FoodTimer {
                ticks: 19,
                regeneration: 79,
            },
        ));
        let sealed = seal(&mut router, player);
        finished(&mut router, &sealed).await;
    }
    let mut router = open(directory.path());
    let inspected = request(&mut router, Intent::Inspect(reference()));
    finished(&mut router, &inspected).await;
    let raw = inspected.observation().unwrap().snapshot().unwrap();
    assert_eq!(raw.food_regeneration, 79);
    assert!(!raw.food_policy.unwrap().natural_regeneration);
    let created = request(
        &mut router,
        Intent::Continue {
            observed: inspected,
            action: Action::Recreate,
        },
    );
    finished(&mut router, &created).await;
    let active = request(
        &mut router,
        Intent::Continue {
            observed: created,
            action: Action::Activate,
        },
    );
    finished(&mut router, &active).await;
    let actor = active.player(world(&router)).unwrap();
    assert_eq!(
        world(&router)
            .get::<super::super::super::hunger::FoodTimer>(actor)
            .unwrap()
            .regeneration,
        79
    );
    step(&mut router).await;
    assert_eq!(world(&router).get::<Health>(actor).unwrap().current, 9.0);
    assert!(
        !world(&router)
            .get::<super::super::super::hunger::PlayerFoodPolicy>(actor)
            .unwrap()
            .natural_regeneration
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unresolved_source_join_hydrates_silently_and_cannot_damage_heal_save_or_release() {
    let directory = tempfile::tempdir().unwrap();
    let mut router = open(directory.path());
    let actor = retained(&mut router);
    let sealed = seal(&mut router, actor);
    finished(&mut router, &sealed).await;
    let saved = canonical(&router).await;
    let (tx, mut rx) = tokio::sync::mpsc::channel(8192);
    let fresh = router
        .world_mut(0)
        .unwrap()
        .spawn_player(PlayerSpawnData {
            session_id: 2,
            runtime_id: 102,
            display_name: "ExampleSource".into(),
            xuid: Some("73".into()),
            uuid: Some(uuid::Uuid::from_u128(73).to_string()),
            position: DVec3::from_array(saved.position),
            saved_data: Some(saved),
            outbound_tx: tx,
            chunk_radius: 0,
        })
        .unwrap();
    assert!(admission_blocked(world(&router), fresh));
    assert!(player_events::initial_state_pending(world(&router), fresh));
    assert!(!player_events::release_initial_state(
        world_mut(&mut router),
        fresh
    ));
    assert!(!crate::entity::player_life::kill_player(
        world_mut(&mut router),
        fresh
    ));
    assert!(!super::super::super::effects::add_effect_from(
        world_mut(&mut router),
        fresh,
        None,
        EffectType::INSTANT_HEALTH,
        0,
        1
    ));
    assert_eq!(router.world_mut(0).unwrap().save_all_players().await, 0);
    for _ in 0..4 {
        step(&mut router).await;
    }
    assert_eq!(world(&router).get::<Health>(fresh).unwrap().current, 9.0);
    assert!(player_events::initial_state_pending(world(&router), fresh));
    while let Ok(packet) = rx.try_recv() {
        assert!(
            !matches!(
                packet.data,
                jolyne::valentine::McpePacketData::InventoryContentPacket(_)
                    | jolyne::valentine::McpePacketData::MobEffectPacket(_)
                    | jolyne::valentine::McpePacketData::SetActorMotionPacket(_)
                    | jolyne::valentine::McpePacketData::SetPlayerGameTypePacket(_)
                    | jolyne::valentine::McpePacketData::UpdateAttributesPacket(_)
            ),
            "unresolved source values cannot publish"
        );
    }
}

/// A real durable provider can stop precisely before a terminal save or lose its acknowledgement.
struct TerminalGate {
    inner: Arc<LevelDBPlayerProvider>,
    started: tokio::sync::Semaphore,
    release: tokio::sync::Semaphore,
    mode: std::sync::atomic::AtomicU8,
    acknowledged: std::sync::atomic::AtomicBool,
}
impl TerminalGate {
    fn open(path: &Path) -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(LevelDBPlayerProvider::open(path).unwrap()),
            started: tokio::sync::Semaphore::new(0),
            release: tokio::sync::Semaphore::new(0),
            mode: std::sync::atomic::AtomicU8::new(0),
            acknowledged: std::sync::atomic::AtomicBool::new(false),
        })
    }
}
#[async_trait::async_trait]
impl PlayerProvider for TerminalGate {
    fn atomic_durable_records(&self) -> bool {
        true
    }
    async fn load(&self, uuid: uuid::Uuid) -> crate::storage::StorageResult<Option<PlayerData>> {
        self.inner.load(uuid).await
    }
    async fn save(&self, uuid: uuid::Uuid, data: &PlayerData) -> crate::storage::StorageResult<()> {
        use std::sync::atomic::Ordering;
        let terminal = data
            .source_actor
            .as_ref()
            .is_some_and(|source| source.phase == SourcePhase::Terminal);
        let mode = self.mode.load(Ordering::Acquire);
        if terminal && mode != 0 {
            self.started.add_permits(1);
            if mode == 1 {
                self.release.acquire().await.unwrap().forget();
            }
            if mode == 2 {
                return Err(std::io::Error::other("terminal save rejected").into());
            }
        }
        self.inner.save(uuid, data).await?;
        if terminal {
            self.acknowledged.store(true, Ordering::Release);
            if mode == 3 {
                return Err(std::io::Error::other("terminal acknowledgement unavailable").into());
            }
        }
        Ok(())
    }
    async fn delete(&self, uuid: uuid::Uuid) -> crate::storage::StorageResult<()> {
        self.inner.delete(uuid).await
    }
    async fn close(&self) -> crate::storage::StorageResult<()> {
        self.inner.close().await
    }
}

/// Only actual spawned native item entities count as publication.
fn drops(world: &mut World) -> usize {
    world
        .query_filtered::<Entity, With<crate::entity::components::DroppedItem>>()
        .iter(world)
        .count()
}

/// Native death remains unobserved until both terminal data and the actual drop disposition commit.
#[derive(Resource, Default)]
struct Deaths(usize);
struct ObserveTerminal(Arc<TerminalGate>);
impl crate::entity::player_life::hooks::PlayerLifecycleHandler for ObserveTerminal {
    fn on_death(&self, world: &mut World, event: &crate::entity::player_life::hooks::PlayerDeath) {
        assert!(
            self.0
                .acknowledged
                .load(std::sync::atomic::Ordering::Acquire)
        );
        assert!(!event.keep_inventory);
        assert!(drops(world) > 0);
        world.resource_mut::<Deaths>().0 += 1;
    }
}

/// Activate a genuine existing native source through its acknowledged canonical generation.
async fn active(router: &mut DimensionRouter) -> (Entity, Arc<Operation>) {
    let player = retained(router);
    let sealed = seal(router, player);
    finished(router, &sealed).await;
    let active = request(
        router,
        Intent::Continue {
            observed: sealed,
            action: Action::Activate,
        },
    );
    finished(router, &active).await;
    (player, active)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminal_tombstone_and_true_post_drop_slots_ack_before_any_native_drop() {
    let directory = tempfile::tempdir().unwrap();
    let gate = TerminalGate::open(directory.path());
    let mut router = router(gate.clone());
    let (player, _) = active(&mut router).await;
    world_mut(&mut router).init_resource::<Deaths>();
    world_mut(&mut router)
        .resource_mut::<crate::entity::player_life::hooks::PlayerLifecycleHooks>()
        .register(ObserveTerminal(gate.clone()));
    gate.mode.store(1, std::sync::atomic::Ordering::Release);
    assert!(crate::entity::player_life::kill_player(
        world_mut(&mut router),
        player
    ));
    for _ in 0..100 {
        step(&mut router).await;
        if gate.started.try_acquire().is_ok() {
            break;
        }
    }
    assert!(quarantine::is_held(world(&router), player));
    assert_eq!(drops(world_mut(&mut router)), 0);
    assert_eq!(world(&router).resource::<Deaths>().0, 0);
    let before = gate
        .inner
        .load(uuid::Uuid::from_u128(73))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        before.source_actor.as_ref().unwrap().phase,
        SourcePhase::Active
    );
    assert_eq!(before.inventory[0].count, 7);
    // A global rule change cannot rewrite an already staged terminal disposition.
    world_mut(&mut router)
        .resource_mut::<crate::world::rules::GameRules>()
        .keep_inventory = true;
    gate.release.add_permits(1);
    for _ in 0..200 {
        step(&mut router).await;
        if world(&router).resource::<Deaths>().0 == 1 {
            break;
        }
    }
    assert_eq!(world(&router).resource::<Deaths>().0, 1);
    let after = canonical(&router).await;
    assert_eq!(
        after.source_actor.as_ref().unwrap().phase,
        SourcePhase::Terminal
    );
    assert_eq!(after.health, 0.0);
    assert_eq!(after.inventory[0].count, 0);
    assert_eq!(after.ender_chest[2].count, 7);
    assert!(world(&router).get::<Dead>(player).is_some());
    let inspected = request(&mut router, Intent::Inspect(reference()));
    finished(&mut router, &inspected).await;
    let retired = request(
        &mut router,
        Intent::Continue {
            observed: inspected,
            action: Action::Retire,
        },
    );
    finished(&mut router, &retired).await;
    assert!(world(&router).get_entity(player).is_err());
    let prevented = request(
        &mut router,
        Intent::Continue {
            observed: retired,
            action: Action::Recreate,
        },
    );
    for _ in 0..4 {
        step(&mut router).await;
    }
    assert!(matches!(
        prevented.status(),
        Status::Rejected(Error::NotOwned)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminal_ack_uncertainty_never_publishes_drops_and_cold_reopen_cannot_recreate_loot() {
    let directory = tempfile::tempdir().unwrap();
    let weak;
    {
        let gate = TerminalGate::open(directory.path());
        weak = Arc::downgrade(&gate);
        let mut router = router(gate.clone());
        let (player, _) = active(&mut router).await;
        gate.mode.store(3, std::sync::atomic::Ordering::Release);
        crate::entity::player_life::kill_player(world_mut(&mut router), player);
        for _ in 0..100 {
            step(&mut router).await;
            if gate.acknowledged.load(std::sync::atomic::Ordering::Acquire) {
                break;
            }
        }
        for _ in 0..3 {
            step(&mut router).await;
        }
        assert_eq!(drops(world_mut(&mut router)), 0);
        assert!(quarantine::is_held(world(&router), player));
        assert!(
            world(&router)
                .resource::<super::super::super::types::PlayerProviderResource>()
                .0
                .load(uuid::Uuid::from_u128(73))
                .await
                .is_err()
        );
    }
    for _ in 0..100 {
        if weak.upgrade().is_none() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(weak.upgrade().is_none());
    let mut reopened = open(directory.path());
    let inspected = request(&mut reopened, Intent::Inspect(reference()));
    finished(&mut reopened, &inspected).await;
    assert_eq!(
        inspected.observation().unwrap().source.phase,
        SourcePhase::Terminal
    );
    assert_eq!(inspected.observation().unwrap().data.inventory[0].count, 0);
    let denied = request(
        &mut reopened,
        Intent::Continue {
            observed: inspected,
            action: Action::Recreate,
        },
    );
    for _ in 0..4 {
        step(&mut reopened).await;
    }
    assert!(matches!(denied.status(), Status::Rejected(Error::NotOwned)));
}
