//! A real Rust SDK component edits authoritative loaded cells through encoded command input.

use super::*;
use crate::ecs::events::EventKind;
use crate::plugin::manifest::PluginCapability;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
use crate::server::worlds::DimensionRouter;
use crate::world::ecs::{ChunkData, ChunkManager};
use bevy_ecs::prelude::{Entity, On, Query, Res, World};
use glam::IVec3;
use jolyne::valentine::*;
use std::{
    path::Path,
    process::Command,
    sync::{Arc, Mutex, OnceLock},
};

/// Compile the actual SDK in a separate target so native Cargo's artifact lock cannot deadlock it.
fn component_bytes() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let manifest =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/world-blocks/Cargo.toml");
        let target = tempfile::tempdir().unwrap();
        let output = Command::new(env!("CARGO"))
            .args([
                "build",
                "--locked",
                "--target",
                "wasm32-wasip2",
                "--manifest-path",
            ])
            .arg(manifest)
            .env("CARGO_TARGET_DIR", target.path())
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("RUSTFLAGS")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "component fixture build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::read(
            target
                .path()
                .join("wasm32-wasip2/debug/world_blocks_fixture.wasm"),
        )
        .unwrap()
    })
}

struct Harness {
    router: DimensionRouter,
    manager: Arc<Mutex<PluginManager>>,
    player: Entity,
    packets: tokio::sync::mpsc::Receiver<McpePacket>,
}
impl Harness {
    /// Ordinary component initialization and native admission precede every encoded input.
    async fn new(capabilities: bool, pending: u32, mode: &[u8]) -> Self {
        Self::with_fixture(capabilities, pending, mode, component_bytes(), None, &[]).await
    }
    /// Exercise the same ordinary host pipeline with a real durable provider
    /// and its configured persistent identity before component initialization.
    async fn with_fixture(
        capabilities: bool,
        pending: u32,
        mode: &[u8],
        bytes: &[u8],
        setup: Option<(
            Arc<dyn crate::storage::WorldProvider>,
            crate::world::WorldIdentity,
        )>,
        extra: &[PluginCapability],
    ) -> Self {
        let mut router = DimensionRouter::new(GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        }));
        let durable = setup.is_some();
        if let Some((provider, identity)) = setup {
            let world = router.world_mut(0).unwrap().ecs.world_mut();
            world.resource_mut::<ChunkManager>().set_provider(provider);
            world.insert_resource(identity);
        }
        let engine = engine();
        let manifest = PluginManifest {
            id: PluginId("example.terrain".into()),
            name: "Terrain".into(),
            version: "1".into(),
            api_version: PLUGIN_API_VERSION.into(),
            required: false,
            config: None,
            capabilities: if capabilities {
                let mut capabilities: std::collections::HashSet<_> = [
                    PluginCapability::ReadBlocks,
                    PluginCapability::WriteBlocks,
                    PluginCapability::ItemValues,
                ]
                .into();
                if durable {
                    capabilities.insert(PluginCapability::WriteDurableBlocks);
                }
                capabilities.extend(extra.iter().cloned());
                capabilities
            } else {
                Default::default()
            },
            subscriptions: [EventKind::PlayerCommand, EventKind::ItemAction].into(),
            limits: PluginLimits {
                max_pending_block_edits: pending,
                max_pending_durable_block_writes: pending,
                ..Default::default()
            },
        };
        let mut store = store(&engine, &manifest.limits);
        store.data_mut().configuration = mode.to_vec();
        let pending = PendingPlugin {
            component: Component::new(&engine, bytes).unwrap(),
            store,
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
        PluginManager::attach_to_world(
            manager.clone(),
            router.world_mut(0).unwrap().ecs.world_mut(),
        );
        router.set_plugin_manager(manager.clone());
        router.initialize_plugins().unwrap();
        let (outbound_tx, packets) = tokio::sync::mpsc::channel(65_536);
        let player = router
            .spawn_player(PlayerSpawnData {
                session_id: 42,
                runtime_id: 42,
                display_name: "Player".into(),
                xuid: Some("42".into()),
                uuid: Some(uuid::Uuid::from_u128(42).to_string()),
                position: glam::DVec3::new(0.5, 17.62, 0.5),
                outbound_tx,
                saved_data: None,
                chunk_radius: 0,
            })
            .unwrap();
        router.tick().await.unwrap();
        let mut harness = Self {
            router,
            manager,
            player,
            packets,
        };
        harness.put(IVec3::new(4, 21, 1), "minecraft:obsidian");
        harness.put(IVec3::new(5, 21, 1), "minecraft:chest");
        harness.drain();
        harness
    }
    /// The fixture never replaces or weakens the authenticated session sender.
    fn world(&self) -> &World {
        self.router.world(0).unwrap().ecs.world()
    }
    /// Trusted native arrangements represent changes outside the component's transaction.
    fn world_mut(&mut self) -> &mut World {
        self.router.world_mut(0).unwrap().ecs.world_mut()
    }
    /// Read the real canonical identity rather than a guest status substitute.
    fn block(&self, x: i32) -> String {
        let state = crate::server::game::blocks::block_at(self.world(), (x, 21, 1)).unwrap();
        self.world()
            .resource::<crate::server::game::types::BlockRegistryResource>()
            .0
            .get_by_runtime_id(state)
            .unwrap()
            .string_id
            .clone()
    }
    /// Prepare actual native states using the existing checked write boundary.
    fn put(&mut self, at: IVec3, identifier: &str) {
        let state = self
            .world()
            .resource::<crate::server::game::types::BlockRegistryResource>()
            .0
            .default_state_id_by_name(identifier)
            .unwrap();
        crate::server::game::blocks::try_set_block(self.world_mut(), at, state, false).unwrap();
    }
    /// Route a real encoded request, leaving physical publication for the ordinary world phase.
    fn command(&mut self, line: &str) {
        let packet = McpePacket::from(CommandRequestPacket {
            command: line.into(),
            origin: types::CommandOriginDatajson {
                type_: "player".into(),
                request_id: "example-42".into(),
                ..Default::default()
            },
            is_internal: false,
            version: "latest".into(),
        });
        let mut bytes = bytes::BytesMut::new();
        packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
        self.router.route_packet(
            42,
            McpePacketArgs,
            BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap(),
        );
        // Packet ingress queues commands; run the normal local ECS phase while
        // leaving the router's later block-publication phase untouched.
        self.router.world_mut(0).unwrap().tick();
    }
    /// An actual native use input exercises cross-family reply validation before publication.
    fn air(&mut self) {
        let transaction = InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(
            Box::new(ItemUseInventoryTransaction {
                action_type: EnumsItemUseInventoryTransactionActionType::Use,
                slot: 0,
                ..Default::default()
            }),
        );
        let packet = McpePacket::from(InventoryTransactionPacket {
            transaction,
            ..Default::default()
        });
        let mut bytes = bytes::BytesMut::new();
        packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
        self.router.route_packet(
            42,
            McpePacketArgs,
            BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap(),
        );
    }
    /// Native publication consumes accepted requests outside the component manager lock.
    async fn tick(&mut self) {
        self.router.tick().await.unwrap();
    }
    /// Normalize SDK enum qualification in real imported diagnostics without changing values.
    fn logs(&self) -> Vec<String> {
        self.manager.lock().unwrap().plugins[&PluginId("example.terrain".into())]
            .store
            .data()
            .test_logs
            .iter()
            .map(|message| {
                message
                    .replace("BlockEditStatus::", "")
                    .replace("DurableBlockStatus::", "")
                    .replace("DurableBlockError::", "")
                    .replace("WorldBlockError::", "")
                    .replace("AccessError::", "")
                    .replace("BlockEditSupport::", "")
            })
            .collect()
    }
    /// Inspect actual outbound packets while preserving the original transport identity.
    fn drain(&mut self) -> Vec<McpePacket> {
        let mut result = Vec::new();
        while let Ok(packet) = self.packets.try_recv() {
            result.push(packet);
        }
        result
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_sdk_stages_then_publishes_the_complete_batch_with_the_original_budget() {
    let mut h = Harness::new(true, 128, b"").await;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let observed = seen.clone();
    let manager = h.manager.clone();
    h.world_mut().add_observer(
        move |event: On<crate::world::ecs::BlockChanged>,
              chunks: Query<&ChunkData>,
              manager_map: Res<ChunkManager>| {
            if event.block_pos.y == 21 && [1, 2].contains(&event.block_pos.x) {
                let chunk = manager_map.get_by_coords(0, 0).unwrap();
                let second = chunks.get(chunk).unwrap().inner.get_block(2, 21, 1);
                let active = manager.try_lock().unwrap().tick_prepared;
                observed
                    .lock()
                    .unwrap()
                    .push((event.block_pos, second, active));
            }
        },
    );
    h.command("/build");
    assert_eq!(h.block(1), "minecraft:air");
    assert_eq!(h.block(2), "minecraft:air");
    assert!(
        h.logs()
            .iter()
            .any(|line| line.contains("stage:Ok(Staged)"))
    );
    h.tick().await;
    assert_eq!(h.block(1), "minecraft:obsidian");
    assert_eq!(h.block(2), "minecraft:obsidian");
    let obsidian = h
        .world()
        .resource::<crate::server::game::types::BlockRegistryResource>()
        .0
        .default_state_id_by_name("minecraft:obsidian")
        .unwrap();
    assert_eq!(seen.lock().unwrap().len(), 2);
    assert!(
        seen.lock()
            .unwrap()
            .iter()
            .all(|(_, second, active)| *second == obsidian && *active)
    );
    let packets = h.drain();
    for x in [1, 2] {
        assert!(packets.iter().any(|packet| matches!(&packet.data, McpePacketData::UpdateBlockPacket(update) if update.block_position.x == x && update.block_position.y == 21)));
    }
    h.command("/status");
    assert!(
        h.logs()
            .iter()
            .any(|line| line.contains("checked: 2, changed: 2"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_last_cell_rejects_the_whole_batch_without_partial_changes() {
    let mut h = Harness::new(true, 128, b"").await;
    h.command("/build");
    h.put(IVec3::new(2, 21, 1), "minecraft:stone");
    h.tick().await;
    assert_eq!(h.block(1), "minecraft:air");
    assert_eq!(h.block(2), "minecraft:stone");
    h.command("/status");
    assert!(
        h.logs()
            .iter()
            .any(|line| line.contains("Failed(Conflict)"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reservation_acquired_after_staging_rejects_the_complete_native_write() {
    let mut h = Harness::new(true, 128, b"").await;
    h.command("/build");
    h.world_mut()
        .init_resource::<crate::server::game::ContainerMutationLocks>();
    assert!(
        h.world_mut()
            .resource_mut::<crate::server::game::ContainerMutationLocks>()
            .acquire("example:pending", [IVec3::new(2, 21, 1)])
    );
    h.tick().await;
    assert_eq!(h.block(1), "minecraft:air");
    assert_eq!(h.block(2), "minecraft:air");
    h.command("/status");
    assert!(
        h.logs()
            .iter()
            .any(|line| line.contains("Failed(Reserved)"))
    );
    h.command("/read 2 21 1");
    assert!(
        h.logs()
            .iter()
            .any(|line| line.contains("read-error:Reserved"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owned_builder_rechecks_its_original_chunk_lifetime_in_a_fresh_scope() {
    let mut h = Harness::new(true, 128, b"").await;
    h.command("/prepare");
    let old = h
        .world()
        .resource::<ChunkManager>()
        .get_by_coords(0, 0)
        .unwrap();
    let data = h.world().get::<ChunkData>(old).unwrap().inner.clone();
    let chunk = h
        .world_mut()
        .spawn((
            ChunkData::new(data),
            crate::world::ecs::ChunkState::Loaded,
            crate::world::ecs::ChunkStateFlags::default(),
        ))
        .id();
    h.world_mut()
        .resource_mut::<ChunkManager>()
        .insert(crate::world::ecs::ChunkPosition::new(0, 0), chunk);
    h.command("/apply");
    assert!(
        h.logs()
            .iter()
            .any(|line| line.contains("stage-error:Conflict"))
    );
    assert_eq!(h.block(1), "minecraft:air");
    assert_eq!(h.block(2), "minecraft:air");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ticket_drop_retains_pending_quota_and_does_not_cancel_physical_work() {
    let mut h = Harness::new(true, 1, b"").await;
    h.command("/quota");
    assert!(
        h.logs()
            .iter()
            .any(|line| line.contains("quota:Err(Access(")
                && line.contains("name: \"limit-exceeded\"")),
        "{:?}",
        h.logs()
    );
    h.tick().await;
    assert_eq!(h.block(1), "minecraft:obsidian");
    assert_eq!(h.block(2), "minecraft:obsidian");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_cancellation_ends_a_pending_request_without_world_or_item_costs() {
    let mut h = Harness::new(true, 128, b"").await;
    h.command("/build");
    h.command("/cancel");
    h.tick().await;
    h.command("/status");
    assert_eq!(h.block(1), "minecraft:air");
    assert_eq!(h.block(2), "minecraft:air");
    assert!(
        h.logs()
            .iter()
            .any(|line| line.contains("status:Ok(Cancelled)"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn traps_and_owner_unload_never_publish_their_staged_requests() {
    let mut trap = Harness::new(true, 128, b"").await;
    trap.command("/trap");
    trap.tick().await;
    assert_eq!(trap.block(1), "minecraft:air");
    assert_eq!(trap.block(2), "minecraft:air");
    let mut unload = Harness::new(true, 128, b"").await;
    unload.command("/build");
    unload
        .manager
        .lock()
        .unwrap()
        .plugins
        .remove(&PluginId("example.terrain".into()));
    unload.tick().await;
    assert_eq!(unload.block(1), "minecraft:air");
    assert_eq!(unload.block(2), "minecraft:air");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unsupported_proposals_and_invalid_properties_do_not_change_native_cells() {
    let mut h = Harness::new(true, 128, b"").await;
    h.command("/unsupported");
    h.command("/invalid");
    h.tick().await;
    assert_eq!(h.block(1), "minecraft:air");
    assert_eq!(h.block(2), "minecraft:air");
    assert!(
        h.logs()
            .iter()
            .any(|line| line.contains("plan-error:Unsupported"))
    );
    assert!(
        h.logs()
            .iter()
            .any(|line| line.contains("plan-error:Access(")
                && line.contains("name: \"invalid-argument\"")),
        "{:?}",
        h.logs()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ungranted_access_and_unloaded_or_invalid_height_never_fabricate_air() {
    let mut denied = Harness::new(false, 128, b"").await;
    denied.command("/read 1 21 1");
    assert!(
        denied
            .logs()
            .iter()
            .any(|line| line.contains("read-error:Access(")
                && line.contains("name: \"permission-denied\"")),
        "{:?}",
        denied.logs()
    );
    let mut h = Harness::new(true, 128, b"").await;
    h.command("/read 1 65557 1");
    h.command("/read 100000 21 100000");
    assert!(
        h.logs()
            .iter()
            .any(|line| line.contains("read-error:OutOfBounds"))
    );
    assert!(
        h.logs()
            .iter()
            .any(|line| line.contains("read-error:Unloaded"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_block_actor_contents_are_preserved_when_a_literal_edit_is_unsupported() {
    use crate::world::block_entity::BlockEntity;
    let mut h = Harness::new(true, 128, b"").await;
    h.put(IVec3::new(1, 21, 1), "minecraft:chest");
    let mut container = crate::block_entity::BlockContainer::new(
        crate::block_entity::ContainerKind::Chest,
        IVec3::new(1, 21, 1),
    );
    container
        .items
        .set_item(
            0,
            crate::server::game::world_stack(h.world(), "minecraft:diamond", 3).unwrap(),
        )
        .unwrap();
    let actor = BlockEntity::from_bedrock_compound(container.encode()).unwrap();
    let chunk = h
        .world()
        .resource::<ChunkManager>()
        .get_by_coords(0, 0)
        .unwrap();
    h.world_mut()
        .get_mut::<ChunkData>(chunk)
        .unwrap()
        .inner
        .set_block_entity(actor.clone());
    h.command("/build");
    h.tick().await;
    assert_eq!(h.block(1), "minecraft:chest");
    assert_eq!(h.block(2), "minecraft:air");
    assert_eq!(
        h.world()
            .get::<ChunkData>(chunk)
            .unwrap()
            .inner
            .block_entities()
            .iter()
            .find(|stored| stored.position() == [1, 21, 1]),
        Some(&actor)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owned_observations_remain_historical_after_the_native_column_is_removed() {
    let mut h = Harness::new(true, 128, b"").await;
    h.command("/read 1 21 1");
    let chunk = h
        .world()
        .resource::<ChunkManager>()
        .get_by_coords(0, 0)
        .unwrap();
    h.world_mut().entity_mut(chunk).remove::<ChunkData>();
    h.command("/image");
    h.command("/read 1 21 1");
    assert!(
        h.logs()
            .iter()
            .any(|line| line.starts_with("image:") && line.contains("minecraft:air"))
    );
    assert!(
        h.logs()
            .iter()
            .any(|line| line.contains("read-error:Unloaded"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn secondary_storage_is_observed_and_unsupported_edits_preserve_both_layers() {
    let mut h = Harness::new(true, 128, b"").await;
    let chunk = h
        .world()
        .resource::<ChunkManager>()
        .get_by_coords(0, 0)
        .unwrap();
    let registry = h
        .world()
        .resource::<crate::server::game::types::BlockRegistryResource>()
        .0
        .clone();
    let stone = registry
        .default_state_id_by_name("minecraft:stone")
        .unwrap();
    let water = registry
        .default_state_id_by_name("minecraft:water")
        .unwrap();
    h.world_mut()
        .get_mut::<ChunkData>(chunk)
        .unwrap()
        .inner
        .set_block_layers(1, 21, 1, stone, Some(water))
        .unwrap();
    h.command("/read 1 21 1");
    assert!(
        h.logs()
            .iter()
            .any(|line| line.contains("read:minecraft:stone:minecraft:water:UnsupportedFamily"))
    );
    h.command("/build");
    h.tick().await;
    let data = &h.world().get::<ChunkData>(chunk).unwrap().inner;
    assert_eq!(data.get_block_layer(1, 21, 1, 0), stone);
    assert_eq!(data.get_block_layer(1, 21, 1, 1), water);
    assert_eq!(h.block(2), "minecraft:air");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unconverted_state_spaces_fail_explicitly_instead_of_interpreting_foreign_ids() {
    let mut h = Harness::new(true, 128, b"").await;
    let chunk = h
        .world()
        .resource::<ChunkManager>()
        .get_by_coords(0, 0)
        .unwrap();
    h.world_mut().get_mut::<ChunkData>(chunk).unwrap().inner =
        crate::world::Chunk::new_in_space(0, 0, crate::world::chunk::BlockStateSpace::Java);
    h.command("/read 1 21 1");
    assert!(
        h.logs()
            .iter()
            .any(|line| line.contains("read-error:Unsupported"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_item_reply_discards_world_work_before_any_inventory_or_projectile_effect() {
    use crate::entity::components::{HeldSlot, MainInventory, Projectile};
    let mut h = Harness::new(true, 128, b"invalid-item").await;
    let stack = crate::server::game::world_stack(h.world(), "minecraft:snowball", 2).unwrap();
    let player = h.player;
    h.world_mut()
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(0, stack)
        .unwrap();
    h.world_mut().get_mut::<HeldSlot>(player).unwrap().0 = 0;
    h.air();
    h.tick().await;
    assert_eq!(h.block(1), "minecraft:air");
    assert_eq!(h.block(2), "minecraft:air");
    assert_eq!(
        h.world()
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .count,
        2
    );
    assert_eq!(
        h.world_mut()
            .query_filtered::<Entity, bevy_ecs::prelude::With<Projectile>>()
            .iter(h.world())
            .count(),
        0
    );
    h.command("/status");
    assert!(
        h.logs()
            .iter()
            .any(|line| line.contains("Failed(Access(")
                && line.contains("name: \"callback-failed\"")),
        "{:?}",
        h.logs()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_native_registry_replacement_invalidates_its_older_owned_witnesses() {
    let mut h = Harness::new(true, 128, b"").await;
    h.command("/prepare");
    let replacement = h
        .world()
        .resource::<crate::server::game::types::BlockRegistryResource>()
        .0
        .as_ref()
        .clone();
    h.world_mut()
        .insert_resource(crate::server::game::types::BlockRegistryResource(Arc::new(
            replacement,
        )));
    h.command("/apply");
    assert!(
        h.logs()
            .iter()
            .any(|line| line.contains("stage-error:Conflict"))
    );
    assert_eq!(h.block(1), "minecraft:air");
    assert_eq!(h.block(2), "minecraft:air");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn negative_coordinates_use_the_loaded_columns_actual_logical_height() {
    let mut h = Harness::new(true, 128, b"").await;
    let mut data = crate::world::Chunk::new(-1, 0);
    data.set_block_bounds(0..128);
    let chunk = h
        .world_mut()
        .spawn((
            ChunkData::new(data),
            crate::world::ecs::ChunkState::Loaded,
            crate::world::ecs::ChunkStateFlags::default(),
            crate::world::ecs::ChunkPosition::new(-1, 0),
            crate::world::ecs::ChunkViewers::default(),
        ))
        .id();
    h.world_mut()
        .resource_mut::<ChunkManager>()
        .insert(crate::world::ecs::ChunkPosition::new(-1, 0), chunk);
    h.command("/read -1 0 1");
    h.command("/read -1 -1 1");
    h.command("/read -1 128 1");
    assert!(
        h.logs()
            .iter()
            .any(|line| line.contains("read:minecraft:air:minecraft:air:Supported")),
        "{:?}",
        h.logs()
    );
    assert_eq!(
        h.logs()
            .iter()
            .filter(|line| line.contains("read-error:OutOfBounds"))
            .count(),
        2
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_bounded_builder_never_exposes_its_partial_detached_plan() {
    let mut h = Harness::new(true, 128, b"").await;
    h.manager
        .lock()
        .unwrap()
        .plugins
        .get_mut(&PluginId("example.terrain".into()))
        .unwrap()
        .store
        .data_mut()
        .api_limits
        .max_block_cells = 1;
    h.command("/build");
    h.tick().await;
    assert_eq!(h.block(1), "minecraft:air");
    assert_eq!(h.block(2), "minecraft:air");
    assert!(
        h.logs()
            .iter()
            .any(|line| line.contains("plan-error:Access(")
                && line.contains("name: \"limit-exceeded\"")),
        "{:?}",
        h.logs()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn raw_retained_transaction_cannot_read_after_its_callback_scope_ends() {
    let mut h = Harness::new(true, 128, b"").await;
    let manager = h.manager.clone();
    let mut manager = manager.lock().unwrap();
    let store = &mut manager
        .plugins
        .get_mut(&PluginId("example.terrain".into()))
        .unwrap()
        .store;
    let expired = with_world(store, h.world_mut(), |store| {
        make_world_tx(store).unwrap().rep()
    });
    let result = with_world(store, h.world_mut(), |store| {
        wit_types::HostWorldTx::block(
            store.data_mut(),
            wasmtime::component::Resource::new_borrow(expired),
            wit_types::BlockPos { x: 1, y: 21, z: 1 },
        )
    });
    assert!(matches!(
        result,
        Err(wit_types::WorldBlockError::Access(
            wit_types::AccessError::ExpiredScope
        ))
    ));
    assert!(store.data().world_ptr.is_null());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn raw_builder_cannot_apply_its_expected_cells_in_a_foreign_world_scope() {
    let mut h = Harness::new(true, 128, b"").await;
    let manager = h.manager.clone();
    let mut manager = manager.lock().unwrap();
    let store = &mut manager
        .plugins
        .get_mut(&PluginId("example.terrain".into()))
        .unwrap()
        .store;
    let edit = with_world(store, h.world_mut(), |store| {
        let tx = make_world_tx(store).unwrap();
        let cell = wit_types::HostWorldTx::block(
            store.data_mut(),
            wasmtime::component::Resource::new_borrow(tx.rep()),
            wit_types::BlockPos { x: 1, y: 21, z: 1 },
        )
        .unwrap();
        let layers = wit_types::HostBlockCell::layers(
            store.data_mut(),
            wasmtime::component::Resource::new_borrow(cell.rep()),
        )
        .unwrap();
        let edit = wit_types::HostWorldTx::edit_blocks(
            store.data_mut(),
            wasmtime::component::Resource::new_borrow(tx.rep()),
        )
        .unwrap();
        wit_types::HostBlockEdit::replace(
            store.data_mut(),
            wasmtime::component::Resource::new_borrow(edit.rep()),
            wasmtime::component::Resource::new_borrow(cell.rep()),
            layers,
        )
        .unwrap();
        edit.rep()
    });
    let mut foreign = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig {
            dimension: 1,
            ..crate::world::WorldConfig::void_spawn_platform(1)
        },
        ..Default::default()
    });
    let result = with_world(store, foreign.ecs.world_mut(), |store| {
        let tx = make_world_tx(store).unwrap();
        wit_types::HostWorldTx::apply_blocks(
            store.data_mut(),
            tx,
            wasmtime::component::Resource::new_borrow(edit),
        )
    });
    assert!(matches!(
        result,
        Err(wit_types::WorldBlockError::Access(
            wit_types::AccessError::WrongWorld
        ))
    ));
    assert!(store.data().pending_block_edits.is_empty());
}

#[path = "world_durable.rs"]
mod durable;

#[path = "world_containers.rs"]
mod container_creation;
