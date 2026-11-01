//! Filled containers travel through the actual scoped component, writer and native windows.
use super::*;
use crate::{
    block_entity::{BlockContainer, ContainerKind},
    entity::components::{ContainerWindows, MainInventory, Position},
    server::game::types::{BlockRegistryResource, ItemRegistryResource},
    storage::{LevelDBWorldProvider, WorldProvider, column_writer::ColumnWriter},
};

/// Build the production Rust SDK against this exact source's component interface.
fn bytes() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let target = tempfile::tempdir().unwrap();
        let output = Command::new(env!("CARGO"))
            .args([
                "build",
                "--locked",
                "--target",
                "wasm32-wasip2",
                "--manifest-path",
            ])
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/durable-container/Cargo.toml"),
            )
            .env("CARGO_TARGET_DIR", target.path())
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("RUSTFLAGS")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::read(
            target
                .path()
                .join("wasm32-wasip2/debug/durable_container_fixture.wasm"),
        )
        .unwrap()
    })
}
/// Read typed stored display metadata independently of the guest implementation.
fn name(stack: &crate::item::ItemStack) -> Option<String> {
    let tag = crate::item::enchant::item_tag(stack)?;
    let zuri_nbt::NBTTag::Compound(display) = tag.0.get("display")? else {
        return None;
    };
    let zuri_nbt::NBTTag::String(name) = display.0.get("Name")? else {
        return None;
    };
    Some(name.0.clone())
}

struct Containers {
    h: Harness,
    writer: Arc<ColumnWriter>,
    directory: tempfile::TempDir,
}
impl Containers {
    /// Install an actual provider and stable world identity before initialization.
    async fn new(pending: u32) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let provider =
            Arc::new(LevelDBWorldProvider::open(directory.path().join("db"), 0).unwrap());
        Self::open(directory, provider, pending).await
    }
    /// The same configured module/world namespace survives a real provider reopen.
    async fn open(
        directory: tempfile::TempDir,
        provider: Arc<LevelDBWorldProvider>,
        pending: u32,
    ) -> Self {
        let writer = provider.column_writer().unwrap();
        let identity = crate::world::WorldIdentity {
            id: uuid::Uuid::from_u128(814),
            name: "primary".into(),
            dimension: 0,
        };
        let h = Harness::with_fixture(
            true,
            pending,
            b"",
            bytes(),
            Some((provider, identity)),
            &[PluginCapability::WriteDurableContainers],
        )
        .await;
        Self {
            h,
            writer,
            directory,
        }
    }
    /// Preserve known uncertainty while shutting down, then validate persisted metadata.
    async fn reopen(self, uncertain: bool) -> Self {
        let Self {
            h,
            writer,
            directory,
        } = self;
        drop(h);
        let closed = writer.close().await;
        assert_eq!(closed.is_err(), uncertain);
        drop(writer);
        let provider =
            Arc::new(LevelDBWorldProvider::open(directory.path().join("db"), 0).unwrap());
        Self::open(directory, provider, 16).await
    }
    /// Wait for actual host completion through normal router phases and receipt queries.
    async fn finish(&mut self) {
        for _ in 0..1000 {
            self.h.tick().await;
            self.h.command("/receipt container-1");
            if self
                .h
                .logs()
                .iter()
                .any(|log| log.contains("receipt:Ok(Some(Committed"))
            {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        panic!("{:?}", self.h.logs());
    }
    /// Read the actual saved actor, independently of the runtime cache.
    fn saved(&self) -> BlockContainer {
        let world = self.h.world();
        let chunk = world
            .resource::<ChunkManager>()
            .get_by_coords(0, 0)
            .unwrap();
        let actor = world
            .get::<ChunkData>(chunk)
            .unwrap()
            .inner
            .block_entities()
            .iter()
            .find(|actor| actor.position() == [1, 21, 1])
            .unwrap();
        let kind = if self.h.block(1) == "minecraft:barrel" {
            ContainerKind::Barrel
        } else {
            ContainerKind::Chest
        };
        BlockContainer::decode_checked(
            kind,
            IVec3::new(1, 21, 1),
            actor.bedrock_compound().unwrap().unwrap(),
            &world.resource::<ItemRegistryResource>().0,
            &world.resource::<BlockRegistryResource>().0,
        )
        .unwrap()
    }
    /// Drive a normal encoded input rather than calling an internal open helper.
    fn packet(&mut self, packet: McpePacket) {
        let mut output = bytes::BytesMut::new();
        packet.data.encode_inner(&mut output, 0, 0).unwrap();
        self.h.router.route_packet(
            42,
            McpePacketArgs,
            BorrowedMcpePacket::decode_inner(&mut output.freeze()).unwrap(),
        );
        self.h.router.world_mut(0).unwrap().tick();
    }
    /// Open the real container screen with the current empty held stack descriptor.
    fn open_window(&mut self) {
        self.open_window_at(IVec3::new(1, 21, 1));
    }
    /// Reach is arranged natively; the actual request and window are still encoded.
    fn open_window_at(&mut self, position: IVec3) {
        let player = self.h.player;
        self.h.world_mut().get_mut::<Position>(player).unwrap().0 = glam::DVec3::new(
            f64::from(position.x) + 0.5,
            f64::from(position.y),
            f64::from(position.z) + 2.5,
        );
        let world = self.h.world();
        let stack = world
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .unwrap();
        let item = crate::server::game::item_descriptor(
            stack,
            &world.resource::<ItemRegistryResource>().0,
            crate::server::game::types::block_network_ids(world),
        )
        .unwrap();
        self.packet(McpePacket::from(InventoryTransactionPacket {
            transaction: InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(
                Box::new(ItemUseInventoryTransaction {
                    action_type: EnumsItemUseInventoryTransactionActionType::Place,
                    position: BlockPos {
                        x: position.x,
                        y: position.y,
                        z: position.z,
                    },
                    face: 3,
                    slot: 0,
                    hand: EnumsHandSlot::Mainhand,
                    item,
                    click_position: Vec3 {
                        x: 0.5,
                        y: 0.5,
                        z: 1.0,
                    },
                    ..Default::default()
                }),
            ),
            ..Default::default()
        }));
    }
    /// Take through an encoded authoritative request with the actual container network identity.
    fn take_three(&mut self) {
        use jolyne::valentine::EnumsContainerEnumName as Name;
        let window = self
            .h
            .world()
            .get::<ContainerWindows>(self.h.player)
            .unwrap()
            .open_id()
            .unwrap();
        let mut source = None;
        while let Ok(packet) = self.h.packets.try_recv() {
            if let McpePacketData::InventoryContentPacket(content) = packet.data
                && content.container_id == u32::from(window)
            {
                source = content.slots.first().and_then(|item| item.net_id_variant);
            }
        }
        let slot = |(name, slot, net)| ItemStackRequestCerealSlotInfoData {
            fullcontainername: FullContainerName {
                container_name: name,
                dynamic_id: None,
            },
            slot,
            net_id_variant: net,
        };
        self.packet(McpePacket::from(ItemStackRequestPacket {
            requests: vec![ItemStackRequestPacketDataRequestData {
                client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 { id: -1 },
                actions: vec![
                    ItemStackRequestPacketDataRequestDataActionsItem::TakeActionData(
                        ItemStackRequestCerealTakeActionData {
                            amount: 3,
                            source: slot((
                                if self.saved().kind == ContainerKind::Barrel {
                                    Name::BarrelContainer
                                } else {
                                    Name::LevelEntityContainer
                                },
                                0,
                                source.expect("actual window contents"),
                            )),
                            destination: slot((Name::HotbarContainer, 0, 0)),
                            ..Default::default()
                        },
                    ),
                ],
                ..Default::default()
            }],
        }));
        let mut result = None;
        while let Ok(packet) = self.h.packets.try_recv() {
            if let McpePacketData::ItemStackResponsePacket(response) = packet.data {
                result = Some(response.responses[0].result);
            }
        }
        assert_eq!(result, Some(EnumsItemStackNetResult::Success));
    }
    /// Reload a new genuine instance after the old module owner is removed.
    fn reload(&mut self, manifest: PluginManifest) {
        let engine = self.h.manager.lock().unwrap().engine.clone();
        self.h.manager.lock().unwrap().pending.push(PendingPlugin {
            component: Component::new(&engine, bytes()).unwrap(),
            store: store(&engine, &manifest.limits),
            manifest,
        });
        self.h.router.initialize_plugins().unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn barrel_is_invisible_until_disk_ack_then_opens_a_real_filled_window() {
    let mut h = Containers::new(16).await;
    let pause = h.writer.pause_next();
    h.h.command("/barrel container-1");
    h.h.tick().await;
    for _ in 0..1000 {
        if pause.entered.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert!(pause.entered.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(h.h.block(1), "minecraft:air");
    assert!(!crate::server::game::container_has_decoded_state(
        h.h.world(),
        IVec3::new(1, 21, 1)
    ));
    pause.release();
    h.finish().await;
    assert_eq!(h.h.block(1), "minecraft:barrel");
    assert!(crate::server::game::container_has_decoded_state(
        h.h.world(),
        IVec3::new(1, 21, 1)
    ));
    let saved = h.saved();
    assert_eq!(saved.items.item(0).unwrap().count, 63);
    assert_eq!(saved.items.item(26).unwrap().count, 7);
    assert_eq!(
        name(saved.items.item(0).unwrap()),
        Some("Workshop record".into())
    );
    let mut actor_projection = false;
    while let Ok(packet) = h.h.packets.try_recv() {
        if let McpePacketData::BlockActorDataPacket(packet) = packet.data {
            actor_projection |= packet.block_position.x == 1
                && packet.block_position.y == 21
                && packet.block_position.z == 1;
        }
    }
    assert!(actor_projection, "acknowledged native actor projection");
    h.open_window();
    assert!(
        h.h.world()
            .get::<ContainerWindows>(h.h.player)
            .unwrap()
            .open_id()
            .is_some()
    );
    h.take_three();
    assert_eq!(h.saved().items.item(0).unwrap().count, 60);
    assert_eq!(
        h.h.world()
            .get::<MainInventory>(h.h.player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .count,
        3
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn single_chest_guards_the_complete_neighbor_footprint_and_never_pairs() {
    let mut h = Containers::new(16).await;
    h.h.command("/chest container-1");
    h.finish().await;
    assert_eq!(h.h.block(1), "minecraft:chest");
    assert!(h.saved().pair.is_none());
    h.open_window();
    assert!(
        h.h.world()
            .get::<ContainerWindows>(h.h.player)
            .unwrap()
            .open_id()
            .is_some()
    );
    assert_eq!(h.saved().items.size(), 27);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adjacent_chest_and_a_late_changed_neighbor_reject_before_any_write() {
    let mut h = Containers::new(16).await;
    h.h.put(IVec3::new(2, 21, 1), "minecraft:chest");
    h.h.command("/chest container-1");
    h.h.tick().await;
    assert_eq!(h.h.block(1), "minecraft:air");
    assert!(!h.writer.reserved());
    assert!(h.h.logs().iter().any(|log| log.contains("Unsupported")));
    h.h.put(IVec3::new(2, 21, 1), "minecraft:air");
    h.h.command("/chest container-1");
    h.h.put(IVec3::new(2, 21, 1), "minecraft:stone");
    h.h.tick().await;
    h.h.command("/status");
    assert_eq!(h.h.block(1), "minecraft:air");
    assert!(!h.writer.reserved());
    assert!(h.h.logs().iter().any(|log| log.contains("NotApplied")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trap_cancel_and_invalid_slot_create_neither_actor_nor_receipt() {
    for command in [
        "/cancel-container container-1",
        "/bad-slot container-1",
        "/trap-container container-1",
    ] {
        let mut h = Containers::new(16).await;
        h.h.command(command);
        h.h.tick().await;
        assert_eq!(h.h.block(1), "minecraft:air");
        assert!(!h.writer.reserved());
        assert!(!crate::server::game::container_has_decoded_state(
            h.h.world(),
            IVec3::new(1, 21, 1)
        ));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn container_permission_is_separate_and_receipts_do_not_grant_literal_writes() {
    let mut h = Containers::new(16).await;
    let mut manifest =
        h.h.manager
            .lock()
            .unwrap()
            .plugins
            .remove(&PluginId("example.terrain".into()))
            .unwrap()
            .manifest;
    manifest
        .capabilities
        .remove(&PluginCapability::WriteDurableBlocks);
    h.reload(manifest.clone());
    h.h.command("/barrel container-1");
    h.finish().await;
    assert_eq!(h.h.block(1), "minecraft:barrel");
    h.h.manager.lock().unwrap().plugins.remove(&manifest.id);
    manifest
        .capabilities
        .remove(&PluginCapability::WriteDurableContainers);
    manifest
        .capabilities
        .insert(PluginCapability::WriteDurableBlocks);
    h.reload(manifest);
    h.h.put(IVec3::new(1, 21, 1), "minecraft:air");
    h.h.command("/barrel container-2");
    assert!(
        h.h.logs()
            .iter()
            .any(|log| log.contains("permission-denied"))
    );
    assert_eq!(h.h.block(1), "minecraft:air");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropped_ticket_keeps_native_capacity_and_still_commits_its_contents() {
    let mut h = Containers::new(1).await;
    h.h.command("/drop-container container-1");
    h.h.command("/barrel container-2");
    assert!(h.h.logs().iter().any(|log| log.contains("limit-exceeded")));
    h.finish().await;
    assert_eq!(h.saved().items.item(0).unwrap().count, 63);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owner_lost_after_acceptance_resumes_only_its_original_durable_container() {
    let mut h = Containers::new(16).await;
    let pause = h.writer.pause_next();
    h.h.command("/barrel container-1");
    h.h.tick().await;
    for _ in 0..1000 {
        if pause.entered.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    h.h.world_mut()
        .init_resource::<crate::server::game::ContainerMutationLocks>();
    assert!(
        h.h.world_mut()
            .resource_mut::<crate::server::game::ContainerMutationLocks>()
            .acquire("example.pending", [IVec3::new(1, 21, 1)])
    );
    let manifest =
        h.h.manager
            .lock()
            .unwrap()
            .plugins
            .remove(&PluginId("example.terrain".into()))
            .unwrap()
            .manifest;
    pause.release();
    for _ in 0..100 {
        h.h.tick().await;
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert_eq!(h.h.block(1), "minecraft:air");
    assert!(h.writer.reserved());
    let mut revoked = manifest.clone();
    revoked
        .capabilities
        .remove(&PluginCapability::WriteDurableContainers);
    h.reload(revoked);
    h.h.command("/resume container-1");
    assert!(
        h.h.logs()
            .iter()
            .any(|log| log.contains("permission-denied"))
    );
    assert!(h.writer.reserved());
    h.h.manager.lock().unwrap().plugins.remove(&manifest.id);
    h.reload(manifest);
    h.h.command("/resume container-1");
    h.h.tick().await;
    assert_eq!(h.h.block(1), "minecraft:air");
    assert!(h.writer.reserved());
    assert!(
        h.h.world_mut()
            .resource_mut::<crate::server::game::ContainerMutationLocks>()
            .release("example.pending", [IVec3::new(1, 21, 1)])
    );
    h.h.command("/resume container-1");
    h.finish().await;
    assert_eq!(h.saved().items.item(26).unwrap().count, 7);
    assert!(!h.writer.reserved());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uncertain_ack_cold_reopen_recovers_actor_and_all_slots_without_recreation() {
    let mut h = Containers::new(16).await;
    h.writer.set_fault(2);
    h.h.command("/barrel container-1");
    for _ in 0..100 {
        h.h.tick().await;
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert_eq!(h.h.block(1), "minecraft:air");
    assert!(h.writer.reserved());
    let mut h = h.reopen(true).await;
    assert_eq!(h.h.block(1), "minecraft:barrel");
    h.h.command("/receipt container-1");
    assert!(h.h.logs().iter().any(|log| log.contains("Committed")));
    h.open_window();
    assert!(
        h.h.world()
            .get::<ContainerWindows>(h.h.player)
            .unwrap()
            .open_id()
            .is_some()
    );
    assert_eq!(h.saved().items.item(0).unwrap().count, 63);
    assert_eq!(h.saved().items.item(26).unwrap().count, 7);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ordinary_inventory_changes_persist_and_receipt_query_never_refills_slots() {
    let mut h = Containers::new(16).await;
    h.h.command("/barrel container-1");
    h.finish().await;
    h.open_window();
    h.take_three();
    h.h.router
        .world_mut(0)
        .unwrap()
        .try_save_all_chunks()
        .await
        .unwrap();
    let mut h = h.reopen(false).await;
    h.h.command("/receipt container-1");
    assert_eq!(h.saved().items.item(0).unwrap().count, 60);
    assert_eq!(h.saved().items.item(26).unwrap().count, 7);
    assert_eq!(
        name(h.saved().items.item(0).unwrap()),
        Some("Workshop record".into())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn single_chest_cross_column_guards_pin_and_reopen_the_whole_real_footprint() {
    let mut h = Containers::new(16).await;
    let pause = h.writer.pause_next();
    h.h.command("/edge-chest container-1");
    h.h.tick().await;
    for _ in 0..1000 {
        if pause.entered.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert!(pause.entered.load(std::sync::atomic::Ordering::Acquire));
    for position in [
        IVec3::new(15, 21, 15),
        IVec3::new(16, 21, 15),
        IVec3::new(15, 21, 16),
    ] {
        assert!(
            crate::server::game::blocks::try_set_block(
                h.h.world_mut(),
                position,
                *crate::world::chunk::blocks::STONE,
                false
            )
            .is_err()
        );
    }
    pause.release();
    h.finish().await;
    let mut h = h.reopen(false).await;
    let world = h.h.world();
    let chunk = world
        .resource::<ChunkManager>()
        .get_by_coords(0, 0)
        .unwrap();
    let actor = world
        .get::<ChunkData>(chunk)
        .unwrap()
        .inner
        .block_entities()
        .iter()
        .find(|actor| actor.position() == [15, 21, 15])
        .unwrap();
    let decoded = BlockContainer::decode_checked(
        ContainerKind::Chest,
        IVec3::new(15, 21, 15),
        actor.bedrock_compound().unwrap().unwrap(),
        &world.resource::<ItemRegistryResource>().0,
        &world.resource::<BlockRegistryResource>().0,
    )
    .unwrap();
    assert_eq!(decoded.items.item(0).unwrap().count, 63);
    assert!(decoded.pair.is_none());
    h.open_window_at(IVec3::new(15, 21, 15));
    assert!(
        h.h.world()
            .get::<ContainerWindows>(h.h.player)
            .unwrap()
            .open_id()
            .is_some()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn matching_receipt_after_external_removal_never_recreates_or_refills_a_container() {
    let mut h = Containers::new(16).await;
    h.h.command("/barrel container-1");
    h.finish().await;
    h.h.put(IVec3::new(1, 21, 1), "minecraft:air");
    h.h.router
        .world_mut(0)
        .unwrap()
        .try_save_all_chunks()
        .await
        .unwrap();
    let mut h = h.reopen(false).await;
    assert_eq!(h.h.block(1), "minecraft:air");
    h.h.command("/barrel container-1");
    h.h.tick().await;
    assert!(
        h.h.logs()
            .iter()
            .any(|log| log.contains("status:Ok(Committed")),
        "{:?}",
        h.h.logs()
    );
    assert_eq!(h.h.block(1), "minecraft:air");
    assert!(!h.writer.reserved());
    assert!(!crate::server::game::container_has_decoded_state(
        h.h.world(),
        IVec3::new(1, 21, 1)
    ));
}
