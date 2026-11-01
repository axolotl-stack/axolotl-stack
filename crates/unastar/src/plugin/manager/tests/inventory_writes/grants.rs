//! Real SDK grants use the canonical provider and genuine native item actors.

use super::*;
use crate::entity::components::{
    ArmourInventory, DroppedItem, Health, ItemEntityStack, PickupDelay, PlayerSession,
};
use crate::storage::LevelDBPlayerProvider;

#[path = "grants/nonfreezing.rs"]
mod nonfreezing;
use crate::storage::item_grants::{ItemMutation, PersistentItem};

fn limits() -> PluginLimits {
    PluginLimits {
        fuel_per_tick: 0,
        callback_timeout_ms: 0,
        ..Default::default()
    }
}

struct Gate {
    inner: Arc<LevelDBPlayerProvider>,
    started: Semaphore,
    release: Semaphore,
    lost_ack: AtomicBool,
    hold_terminal: AtomicBool,
    terminal_started: Semaphore,
    terminal_release: Semaphore,
}
#[async_trait]
impl PlayerProvider for Gate {
    fn atomic_durable_records(&self) -> bool {
        true
    }
    fn atomic_durable_items(&self) -> bool {
        true
    }
    async fn load(&self, uuid: uuid::Uuid) -> StorageResult<Option<PlayerData>> {
        self.inner.load(uuid).await
    }
    async fn save(&self, uuid: uuid::Uuid, data: &PlayerData) -> StorageResult<()> {
        if data.native_terminal.is_some() && self.hold_terminal.load(Ordering::Acquire) {
            self.terminal_started.add_permits(1);
            self.terminal_release.acquire().await.unwrap().forget();
        }
        self.inner.save(uuid, data).await
    }
    async fn delete(&self, uuid: uuid::Uuid) -> StorageResult<()> {
        self.inner.delete(uuid).await
    }
    async fn close(&self) -> StorageResult<()> {
        self.inner.close().await
    }
    async fn world_items(
        &self,
        world: uuid::Uuid,
        dimension: i32,
    ) -> StorageResult<Vec<PersistentItem>> {
        self.inner.world_items(world, dimension).await
    }
    async fn retire_world_item(&self, item: &PersistentItem) -> StorageResult<()> {
        self.inner.retire_world_item(item).await
    }
    async fn save_with_items(
        &self,
        uuid: uuid::Uuid,
        data: &PlayerData,
        items: &ItemMutation,
    ) -> StorageResult<()> {
        self.started.add_permits(1);
        self.release.acquire().await.unwrap().forget();
        self.inner.save_with_items(uuid, data, items).await?;
        if self.lost_ack.load(Ordering::Acquire) {
            Err(StorageError::Database(
                "controlled lost item acknowledgement".into(),
            ))
        } else {
            Ok(())
        }
    }
}

fn gated(dir: &std::path::Path, lost_ack: bool) -> Arc<Gate> {
    Arc::new(Gate {
        inner: Arc::new(LevelDBPlayerProvider::open(dir).unwrap()),
        started: Semaphore::new(0),
        release: Semaphore::new(0),
        lost_ack: AtomicBool::new(lost_ack),
        hold_terminal: AtomicBool::new(false),
        terminal_started: Semaphore::new(0),
        terminal_release: Semaphore::new(0),
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn held_original_quit_body_and_world_actors_wait_for_the_same_batch_ack() {
    let dir = tempfile::tempdir().unwrap();
    let provider = gated(dir.path(), false);
    let mut harness = Harness::new(50, provider.clone(), limits(), true);
    ready(&mut harness).await;
    fill(&mut harness);
    harness.action().await.unwrap();
    provider.started.acquire().await.unwrap().forget();
    assert!(drops(&mut harness).is_empty());
    assert!(
        provider
            .world_items(uuid::Uuid::from_u128(9), 0)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(native::inventory_busy(
        harness.router.world(0).unwrap().ecs.world(),
        harness.player
    ));
    assert!(!native::is_quarantined(
        harness.router.world(0).unwrap().ecs.world(),
        harness.player
    ));
    harness.router.despawn_player(1);
    let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
    world.flush();
    assert!(world.get::<PlayerSession>(harness.player).is_none());
    assert!(world.get::<Health>(harness.player).is_some());
    provider.release.add_permits(1);
    harness.acknowledge(1).await;
    assert_eq!(
        drops(&mut harness)
            .iter()
            .map(|(_, _, count)| u32::from(*count))
            .sum::<u32>(),
        130
    );
    let world = harness.router.world(0).unwrap().ecs.world();
    assert!(world.get::<Health>(harness.player).is_some());
    assert!(world.get::<PlayerSession>(harness.player).is_none());
    assert!(!native::is_quarantined(world, harness.player));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lost_ack_cold_reopen_uses_exact_receipt_without_duplicate_items() {
    let dir = tempfile::tempdir().unwrap();
    {
        let provider = gated(dir.path(), true);
        let mut harness = Harness::new(50, provider.clone(), limits(), true);
        ready(&mut harness).await;
        fill(&mut harness);
        harness.action().await.unwrap();
        provider.release.add_permits(1);
        harness.acknowledge(1).await;
        assert!(drops(&mut harness).is_empty());
        assert!(native::inventory_busy(
            harness.router.world(0).unwrap().ecs.world(),
            harness.player
        ));
        assert_eq!(
            provider
                .world_items(uuid::Uuid::from_u128(9), 0)
                .await
                .unwrap()
                .len(),
            3
        );
    }
    let provider = Arc::new(LevelDBPlayerProvider::open(dir.path()).unwrap());
    let saved = provider
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    let mut harness = Harness::with_saved(57, provider.clone(), limits(), true, Some(saved));
    ready(&mut harness).await;
    assert_eq!(drops(&mut harness).len(), 3);
    harness.action().await.unwrap();
    assert!(harness.logs().iter().any(|line| line == "grant-replayed"));
    assert_eq!(drops(&mut harness).len(), 3);
}

async fn ready(harness: &mut Harness) {
    for _ in 0..1000 {
        crate::server::game::item_grants::poll(
            harness.router.world_mut(0).unwrap().ecs.world_mut(),
        );
        tokio::task::yield_now().await;
    }
}

fn fill(harness: &mut Harness) {
    let mut inventory = harness
        .router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .get_mut::<MainInventory>(harness.player)
        .unwrap();
    for slot in 0..36 {
        inventory
            .0
            .set_item(
                slot,
                crate::item::test_support::stack("minecraft:stone", 64),
            )
            .unwrap();
    }
}

fn drops(harness: &mut Harness) -> Vec<(Entity, PersistentItem, u8)> {
    let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
    world
        .query::<(
            Entity,
            &crate::server::game::item_grants::DurableItem,
            &ItemEntityStack,
        )>()
        .iter(world)
        .map(|(entity, row, stack)| (entity, row.0.clone(), stack.0.count))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exact_preferred_placement_and_full_overflow_publish_after_real_ack() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(LevelDBPlayerProvider::open(dir.path()).unwrap());
    let mut harness = Harness::new(54, provider.clone(), limits(), true);
    ready(&mut harness).await;
    fill(&mut harness);
    let before = harness
        .router
        .world(0)
        .unwrap()
        .ecs
        .world()
        .get::<MainInventory>(harness.player)
        .unwrap()
        .clone();
    harness.action().await.unwrap();
    harness.acknowledge(1).await;
    // Drive the owned SDK observation after ACK so its exact Plan hash assertion runs.
    harness.router.tick().await.unwrap();
    let world = harness.router.world(0).unwrap().ecs.world();
    assert_eq!(
        world
            .get::<MainInventory>(harness.player)
            .unwrap()
            .0
            .slots(),
        before.0.slots(),
        "occupied preferred and main slots are preserved"
    );
    let actual = drops(&mut harness);
    assert_eq!(
        actual
            .iter()
            .map(|(_, _, count)| u32::from(*count))
            .sum::<u32>(),
        130
    );
    assert_eq!(actual.len(), 3);
    assert!(actual.iter().all(|(_, row, _)| row.stack.nbt.is_some()));
    let stored = provider
        .world_items(uuid::Uuid::from_u128(9), 0)
        .await
        .unwrap();
    assert_eq!(stored.len(), 3);
    assert_eq!(
        stored
            .iter()
            .map(|row| u32::from(row.stack.count))
            .sum::<u32>(),
        130
    );
    assert!(harness.logs().iter().any(|line| line == "grant-staged"));
    let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
    let ordinary = crate::entity::spawn_item_drop(
        world,
        glam::DVec3::new(0.5, 16.0, 0.5),
        crate::item::test_support::stack("minecraft:stone", 1),
        None,
    )
    .unwrap();
    let output = crate::command::execute(
        world,
        crate::command::CommandSource::server(),
        "kill @e[type=item]",
    );
    assert_eq!(
        output.success_count, 1,
        "only the ordinary transient item is removable synchronously"
    );
    assert!(world.get_entity(ordinary).is_err());
    assert_eq!(
        drops(&mut harness).len(),
        3,
        "synchronous removal cannot bypass durable retirement"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exact_nonstackable_quantity_and_armour_preference_are_not_clamped() {
    for mode in [53, 55] {
        let dir = tempfile::tempdir().unwrap();
        let provider = Arc::new(LevelDBPlayerProvider::open(dir.path()).unwrap());
        let mut harness = Harness::new(mode, provider, limits(), true);
        ready(&mut harness).await;
        if mode == 53 {
            fill(&mut harness);
        }
        harness.action().await.unwrap();
        harness.acknowledge(1).await;
        if mode == 53 {
            let actual = drops(&mut harness);
            assert_eq!(actual.len(), 3);
            assert!(actual.iter().all(|(_, _, count)| *count == 1));
        } else {
            let world = harness.router.world(0).unwrap().ecs.world();
            assert_eq!(
                world
                    .get::<ArmourInventory>(harness.player)
                    .unwrap()
                    .slots()[0]
                    .count,
                1
            );
            assert_eq!(
                world
                    .get::<MainInventory>(harness.player)
                    .unwrap()
                    .0
                    .slots()[0]
                    .count,
                0
            );
            assert_eq!(drops(&mut harness).len(), 1);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_quantities_and_trapped_callback_never_write_or_spawn() {
    for mode in [51, 52] {
        let dir = tempfile::tempdir().unwrap();
        let provider = Arc::new(LevelDBPlayerProvider::open(dir.path()).unwrap());
        let mut harness = Harness::new(mode, provider.clone(), limits(), true);
        ready(&mut harness).await;
        let result = harness.action().await;
        if mode == 51 {
            assert!(result.is_err());
        } else {
            result.unwrap();
        }
        assert!(
            provider
                .world_items(uuid::Uuid::from_u128(9), 0)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            provider
                .load(uuid::Uuid::from_u128(1))
                .await
                .unwrap()
                .is_none()
        );
        assert!(drops(&mut harness).is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cold_reopen_replay_and_picked_tombstones_do_not_repeat_grants() {
    let dir = tempfile::tempdir().unwrap();
    let ids;
    {
        let provider = Arc::new(LevelDBPlayerProvider::open(dir.path()).unwrap());
        let mut harness = Harness::new(50, provider.clone(), limits(), true);
        ready(&mut harness).await;
        fill(&mut harness);
        harness.action().await.unwrap();
        harness.acknowledge(1).await;
        ids = drops(&mut harness)
            .into_iter()
            .map(|(_, row, _)| row.id)
            .collect::<HashSet<_>>();
        assert_eq!(ids.len(), 3);
        // Make room and touch the genuine native item actors. Their retirement and
        // the collector's new inventory are one conditional canonical batch.
        let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
        world
            .get_mut::<MainInventory>(harness.player)
            .unwrap()
            .0
            .set_item(0, crate::item::ItemStack::empty())
            .unwrap();
        for (_, mut delay) in world
            .query_filtered::<(Entity, &mut PickupDelay), With<DroppedItem>>()
            .iter_mut(world)
        {
            delay.0 = 0;
        }
        for _ in 0..5000 {
            let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
            crate::server::game::item_grants::poll(world);
            native::poll(world);
            if provider
                .world_items(uuid::Uuid::from_u128(9), 0)
                .await
                .unwrap()
                .iter()
                .map(|row| row.stack.count as u32)
                .sum::<u32>()
                < 130
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        // Wait for publication too, retaining the original body while IO completes.
        harness.acknowledge(2).await;
        assert_eq!(harness.count(), 64);
    }
    {
        let provider = Arc::new(LevelDBPlayerProvider::open(dir.path()).unwrap());
        let saved = provider
            .load(uuid::Uuid::from_u128(1))
            .await
            .unwrap()
            .unwrap();
        let before = provider
            .world_items(uuid::Uuid::from_u128(9), 0)
            .await
            .unwrap();
        assert_eq!(
            before.iter().map(|row| row.stack.count as u32).sum::<u32>(),
            66
        );
        let mut harness = Harness::with_saved(57, provider.clone(), limits(), true, Some(saved));
        ready(&mut harness).await;
        harness.action().await.unwrap();
        assert!(harness.logs().iter().any(|line| line == "grant-replayed"));
        let after = provider
            .world_items(uuid::Uuid::from_u128(9), 0)
            .await
            .unwrap();
        assert_eq!(before, after);
        assert!(after.iter().all(|row| ids.contains(&row.id)));
        assert_eq!(drops(&mut harness).len(), 2);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn canonical_drop_compare_and_retirement_survive_true_provider_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let uuid = uuid::Uuid::from_u128(111);
    let world = uuid::Uuid::from_u128(222);
    let row = PersistentItem {
        id: uuid::Uuid::from_u128(333),
        world,
        dimension: 1,
        feet: [2.5, 80.0, 3.5],
        stack: crate::storage::StoredItemStack::from(&crate::item::test_support::stack(
            "minecraft:diamond",
            4,
        )),
    };
    {
        let provider = LevelDBPlayerProvider::open(dir.path()).unwrap();
        let data = PlayerData {
            uuid: uuid.to_string(),
            ..Default::default()
        };
        provider
            .save_with_items(uuid, &data, &ItemMutation::Create(vec![row.clone()]))
            .await
            .unwrap();
        let mut changed = row.clone();
        changed.stack.count = 3;
        assert!(
            provider
                .save_with_items(
                    uuid,
                    &data,
                    &ItemMutation::Take {
                        expected: changed,
                        remaining: 0
                    }
                )
                .await
                .is_err()
        );
        provider.retire_world_item(&row).await.unwrap();
    }
    let provider = LevelDBPlayerProvider::open(dir.path()).unwrap();
    assert!(provider.world_items(world, 1).await.unwrap().is_empty());
    let data = provider.load(uuid).await.unwrap().unwrap();
    assert!(
        provider
            .save_with_items(uuid, &data, &ItemMutation::Create(vec![row]))
            .await
            .is_err(),
        "retired stable IDs cannot be resurrected"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_accounts_can_use_the_same_module_key_without_sharing_drop_ids() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(LevelDBPlayerProvider::open(dir.path()).unwrap());
    let mut harness = Harness::new(50, provider.clone(), limits(), true);
    ready(&mut harness).await;
    fill(&mut harness);
    harness.action().await.unwrap();
    harness.acknowledge(1).await;
    let (outbound_tx, _packets) = tokio::sync::mpsc::channel(65536);
    harness.player = harness
        .router
        .world_mut(0)
        .unwrap()
        .spawn_player(crate::server::game::PlayerSpawnData {
            session_id: 2,
            runtime_id: 2,
            display_name: "Player2".into(),
            xuid: Some("2".into()),
            uuid: Some(uuid::Uuid::from_u128(2).to_string()),
            position: glam::DVec3::new(0.5, 17.62, 0.5),
            outbound_tx,
            saved_data: None,
            chunk_radius: 0,
        })
        .unwrap();
    fill(&mut harness);
    harness.router.refresh_directory();
    harness.action().await.unwrap();
    harness.acknowledge(2).await;
    let actual = drops(&mut harness);
    assert_eq!(actual.len(), 6);
    assert_eq!(
        actual
            .iter()
            .map(|(_, row, _)| row.id)
            .collect::<HashSet<_>>()
            .len(),
        6
    );
    assert_eq!(
        actual
            .iter()
            .map(|(_, _, count)| u32::from(*count))
            .sum::<u32>(),
        260
    );
    assert_eq!(
        provider
            .world_items(uuid::Uuid::from_u128(9), 0)
            .await
            .unwrap()
            .len(),
        6
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn empty_inventory_uses_legal_stacks_and_reused_changed_intent_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(LevelDBPlayerProvider::open(dir.path()).unwrap());
    {
        let mut harness = Harness::new(50, provider.clone(), limits(), true);
        ready(&mut harness).await;
        harness.action().await.unwrap();
        harness.acknowledge(1).await;
        let world = harness.router.world(0).unwrap().ecs.world();
        let slots = world
            .get::<MainInventory>(harness.player)
            .unwrap()
            .0
            .slots();
        assert_eq!(
            slots[..3].iter().map(|item| item.count).collect::<Vec<_>>(),
            [64, 64, 2]
        );
        assert!(drops(&mut harness).is_empty());
    }
    let saved = provider
        .load(uuid::Uuid::from_u128(1))
        .await
        .unwrap()
        .unwrap();
    let mut harness =
        Harness::with_saved(56, provider.clone(), limits(), true, Some(saved.clone()));
    ready(&mut harness).await;
    harness.action().await.unwrap();
    assert!(harness.logs().iter().any(|line| line == "grant-conflict"));
    assert_eq!(
        provider
            .load(uuid::Uuid::from_u128(1))
            .await
            .unwrap()
            .unwrap(),
        saved
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unsupported_provider_and_resource_quota_reject_before_writes() {
    let provider = Arc::new(Provider::new());
    let mut harness = Harness::new(50, provider.clone(), limits(), true);
    assert!(harness.action().await.is_err());
    assert_eq!(provider.writes.load(Ordering::Acquire), 0);
    assert!(drops(&mut harness).is_empty());

    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(LevelDBPlayerProvider::open(dir.path()).unwrap());
    let mut quota = limits();
    quota.max_pending_inventory_operations = 0;
    let mut harness = Harness::new(50, provider.clone(), quota, true);
    ready(&mut harness).await;
    assert!(harness.action().await.is_err());
    assert!(
        provider
            .load(uuid::Uuid::from_u128(1))
            .await
            .unwrap()
            .is_none()
    );
    assert!(drops(&mut harness).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_component_cannot_observe_a_modules_grant_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(LevelDBPlayerProvider::open(dir.path()).unwrap());
    let mut harness = Harness::new(50, provider.clone(), limits(), true);
    ready(&mut harness).await;
    harness.action().await.unwrap();
    harness.acknowledge(1).await;
    {
        let mut manager = harness.manager.lock().unwrap();
        for plugin in manager.plugins.values_mut() {
            plugin.manifest.subscriptions.remove(&EventKind::PlayerJump);
        }
        let limits = limits();
        let mut context = store(&manager.engine, &limits);
        context.data_mut().configuration = vec![58];
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
                capabilities: [PluginCapability::ReadPlayers, PluginCapability::GrantItems].into(),
                subscriptions: [EventKind::PlayerJump].into(),
                limits,
            },
        });
    }
    harness.action().await.unwrap();
    assert!(
        harness
            .logs()
            .iter()
            .any(|line| line == "grant-namespace-isolated")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn occupied_compatible_positional_slot_drops_every_item_without_using_empty_main_slots() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(LevelDBPlayerProvider::open(dir.path()).unwrap());
    let mut harness = Harness::new(59, provider.clone(), limits(), true);
    ready(&mut harness).await;
    // The SDK fixture first persists a genuinely identical named prototype in slot zero.
    harness.action().await.unwrap();
    harness.acknowledge(1).await;
    harness.router.tick().await.unwrap();
    let original = harness
        .router
        .world(0)
        .unwrap()
        .ecs
        .world()
        .get::<MainInventory>(harness.player)
        .unwrap()
        .clone();
    assert_eq!(original.0.slots()[0].count, 32);
    assert!(original.0.slots()[1..].iter().all(|item| item.is_empty()));
    harness.action().await.unwrap();
    harness.acknowledge(2).await;
    assert_eq!(
        harness
            .router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get::<MainInventory>(harness.player)
            .unwrap()
            .0
            .slots(),
        original.0.slots()
    );
    assert_eq!(
        drops(&mut harness)
            .iter()
            .map(|(_, _, count)| u32::from(*count))
            .sum::<u32>(),
        130
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_original_transport_during_io_never_receives_the_inventory_publication() {
    let dir = tempfile::tempdir().unwrap();
    let provider = gated(dir.path(), false);
    let mut harness = Harness::new(50, provider.clone(), limits(), true);
    ready(&mut harness).await;
    fill(&mut harness);
    harness.action().await.unwrap();
    provider.started.acquire().await.unwrap().forget();
    let (replacement, mut packets) = tokio::sync::mpsc::channel(128);
    harness
        .router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .get_mut::<PlayerSession>(harness.player)
        .unwrap()
        .outbound_tx = replacement;
    provider.release.add_permits(1);
    harness.acknowledge(1).await;
    let world = harness.router.world(0).unwrap().ecs.world();
    assert!(native::inventory_busy(world, harness.player));
    assert!(!native::is_quarantined(world, harness.player));
    assert!(world.get::<MainInventory>(harness.player).is_some());
    assert_eq!(
        drops(&mut harness)
            .iter()
            .map(|(_, _, count)| u32::from(*count))
            .sum::<u32>(),
        130
    );
    assert!(
        !std::iter::from_fn(|| packets.try_recv().ok()).any(|packet| matches!(
            packet.data,
            jolyne::valentine::McpePacketData::InventoryContentPacket(_)
        ))
    );
}
