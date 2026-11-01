//! Actual item component decisions precede encoded-input consumption and projectile effects.

use super::*;
use crate::ecs::events::EventKind;
use crate::entity::components::{HeldSlot, MainInventory};
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
use crate::server::worlds::DimensionRouter;
use bevy_ecs::prelude::*;
use jolyne::valentine::*;
use std::sync::{Arc, Mutex};

struct Harness {
    router: DimensionRouter,
    manager: Arc<Mutex<PluginManager>>,
    player: Entity,
    _packets: tokio::sync::mpsc::Receiver<McpePacket>,
}
impl Harness {
    /// Initialize real canonical exports and the ordinary native bridge before player admission.
    async fn new(proposal: String, committed: String) -> Self {
        Self::required(proposal, committed, true).await
    }
    /// Optional component traps also veto their operation, while required traps fence admission.
    async fn required(proposal: String, committed: String, required: bool) -> Self {
        Self::with_capabilities(proposal, committed, required, &[]).await
    }
    /// Explicit mechanics are independent of a valid item-default decision.
    async fn with_capabilities(
        proposal: String,
        committed: String,
        required: bool,
        extra: &[crate::plugin::manifest::PluginCapability],
    ) -> Self {
        let mut router = DimensionRouter::new(GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        }));
        let engine = engine();
        let manifest = PluginManifest {
            id: PluginId("example.items".into()),
            name: "Items".into(),
            version: "1".into(),
            api_version: PLUGIN_API_VERSION.into(),
            required,
            config: None,
            capabilities: [
                crate::plugin::manifest::PluginCapability::ReadPlayers,
                crate::plugin::manifest::PluginCapability::ItemValues,
                crate::plugin::manifest::PluginCapability::SendMessages,
            ]
            .into_iter()
            .chain(extra.iter().cloned())
            .collect(),
            subscriptions: [
                EventKind::ItemAction,
                EventKind::ItemActionCommitted,
                EventKind::PlayerItemUse,
            ]
            .into(),
            limits: PluginLimits::default(),
        };
        let pending = PendingPlugin {
            component: fixture::component(
                &engine,
                &[
                    ("on-item-action", proposal),
                    ("on-item-action-committed", committed),
                    ("on-player-item-use", "unreachable".into()),
                ],
            ),
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
        PluginManager::attach_to_world(
            manager.clone(),
            router.world_mut(0).unwrap().ecs.world_mut(),
        );
        router.set_plugin_manager(manager.clone());
        router.initialize_plugins().unwrap();
        let (outbound_tx, packets) = tokio::sync::mpsc::channel(65536);
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
        Self {
            router,
            manager,
            player,
            _packets: packets,
        }
    }
    /// Inspect the actual current native source and entities.
    fn world(&self) -> &World {
        self.router.world(0).unwrap().ecs.world()
    }
    /// Trusted test setup is distinct from guest mutation imports.
    fn world_mut(&mut self) -> &mut World {
        self.router.world_mut(0).unwrap().ecs.world_mut()
    }
    /// Count real projectile actors rather than attempted launches.
    fn projectiles(&mut self) -> usize {
        let world = self.world_mut();
        world
            .query_filtered::<Entity, With<crate::entity::projectile::ProjectileConfig>>()
            .iter(world)
            .count()
    }
    /// Install a server-resolved source stack rather than trusting a client item declaration.
    fn held(&mut self, identifier: &str, count: u8) {
        let world = self.router.world_mut(0).unwrap().ecs.world_mut();
        let stack = crate::server::game::world_stack(world, identifier, count).unwrap();
        world
            .get_mut::<MainInventory>(self.player)
            .unwrap()
            .0
            .set_item(0, stack)
            .unwrap();
        world.get_mut::<HeldSlot>(self.player).unwrap().0 = 0;
    }
    /// Observe the authoritative selected slot, never an echoed client descriptor.
    fn count(&self) -> u8 {
        self.router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get::<MainInventory>(self.player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .count
    }
    /// Route a real encoded click with the exact current source item and input face.
    async fn click(
        &mut self,
        position: glam::IVec3,
        face: u8,
    ) -> Result<(), crate::server::worlds::WorldTickError> {
        let world = self.router.world(0).unwrap().ecs.world();
        let stack = world
            .get::<MainInventory>(self.player)
            .unwrap()
            .0
            .item(0)
            .unwrap();
        let items = &world
            .resource::<crate::server::game::types::ItemRegistryResource>()
            .0;
        let item = crate::server::game::item_descriptor(
            stack,
            items,
            crate::server::game::types::block_network_ids(world),
        )
        .unwrap();
        let packet = McpePacket::from(InventoryTransactionPacket {
            transaction: InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(
                Box::new(ItemUseInventoryTransaction {
                    action_type: EnumsItemUseInventoryTransactionActionType::Place,
                    position: BlockPos {
                        x: position.x,
                        y: position.y,
                        z: position.z,
                    },
                    face,
                    slot: 0,
                    hand: EnumsHandSlot::Mainhand,
                    item,
                    click_position: Vec3 {
                        x: 0.5,
                        y: 1.0,
                        z: 0.5,
                    },
                    ..Default::default()
                }),
            ),
            ..Default::default()
        });
        let mut bytes = bytes::BytesMut::new();
        packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
        self.router.route_packet(
            42,
            McpePacketArgs,
            BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap(),
        );
        self.router.tick().await
    }
    /// Deliver an ordinary encoded air/release input through the real packet pipeline.
    async fn air(
        &mut self,
        request: crate::server::game::item_use::UseRequest,
    ) -> Result<(), crate::server::worlds::WorldTickError> {
        use crate::server::game::item_use::UseRequest;
        let transaction = match request {
            UseRequest::UseAir => {
                InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(Box::new(
                    ItemUseInventoryTransaction {
                        action_type: EnumsItemUseInventoryTransactionActionType::Use,
                        slot: 0,
                        ..Default::default()
                    },
                ))
            }
            UseRequest::Release | UseRequest::Consume => {
                InventoryTransactionPacketTransaction::ItemReleaseInventoryTransaction(
                    ItemReleaseInventoryTransaction {
                        action_type: if request == UseRequest::Consume {
                            EnumsItemReleaseInventoryTransactionActionType::Use
                        } else {
                            EnumsItemReleaseInventoryTransactionActionType::Release
                        },
                        slot: 0,
                        ..Default::default()
                    },
                )
            }
        };
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
        self.router.tick().await
    }
    /// Guest logs witness actual canonical invocation without renewing fuel.
    fn logs(&self) -> Vec<String> {
        self.manager.lock().unwrap().plugins[&PluginId("example.items".into())]
            .store
            .data()
            .test_logs
            .clone()
    }
}

struct Layout {
    resolve: wit_parser::Resolve,
    sizes: wit_parser::SizeAlign,
    world: wit_parser::WorldId,
}
impl Layout {
    /// Load the owning current WIT, deriving all indirect record locations from its layout.
    fn new() -> Self {
        let mut resolve = wit_parser::Resolve::default();
        let (package, _) = resolve
            .push_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../unastar-api/wit"))
            .unwrap();
        let world = resolve
            .select_world(&[package], Some("unastar-plugin"))
            .unwrap();
        let mut sizes = wit_parser::SizeAlign::default();
        sizes.fill(&resolve);
        Self {
            resolve,
            sizes,
            world,
        }
    }
    /// Locate an actual named record rather than its exported type alias.
    fn record(&self, name: &str) -> wit_parser::TypeId {
        self.resolve
            .types
            .iter()
            .find_map(|(id, ty)| {
                (ty.name.as_deref() == Some(name)
                    && matches!(ty.kind, wit_parser::TypeDefKind::Record(_)))
                .then_some(id)
            })
            .unwrap()
    }
    /// Derive a member offset within the record's canonical wasm32 representation.
    fn field(&self, record: &str, name: &str) -> usize {
        let wit_parser::TypeDefKind::Record(record) = &self.resolve.types[self.record(record)].kind
        else {
            unreachable!()
        };
        let mut offset = 0usize;
        for field in &record.fields {
            offset = offset.next_multiple_of(self.sizes.align(&field.ty).align_wasm32());
            if field.name == name {
                return offset;
            }
            offset += self.sizes.size(&field.ty).size_wasm32();
        }
        panic!("missing field")
    }
    /// Prove that this export indirects its parameters before emitting memory-based test code.
    fn param(&self, export: &str, name: &str) -> usize {
        let function = self.resolve.worlds[self.world]
            .exports
            .values()
            .find_map(|item| match item {
                wit_parser::WorldItem::Function(function) if function.name == export => {
                    Some(function)
                }
                _ => None,
            })
            .unwrap();
        assert!(
            self.resolve
                .wasm_signature(wit_parser::abi::AbiVariant::GuestExport, function)
                .indirect_params
        );
        let mut offset = 0usize;
        for parameter in &function.params {
            offset = offset.next_multiple_of(self.sizes.align(&parameter.ty).align_wasm32());
            if parameter.name == name {
                return offset;
            }
            offset += self.sizes.size(&parameter.ty).size_wasm32();
        }
        panic!("missing parameter")
    }
    /// Release the detached selected value after the actual guest proposal callback.
    fn drop_proposal(&self) -> String {
        let at = self.param("on-item-action", "proposal")
            + self.field("item-action-proposal", "selected");
        format!("local.get 0 i32.load offset={at} call @stack_drop@")
    }
    /// Release the entire successful known-cost batch, as the Rust guest Drop implementation does.
    fn drop_committed(&self) -> String {
        let at = self.param("on-item-action-committed", "event")
            + self.field("item-action-committed", "items");
        let before = at + 4 + self.field("item-action-items", "selected-before");
        let after = at + 4 + self.field("item-action-items", "selected-after");
        let ammo = at + 4 + self.field("item-action-items", "ammunition");
        let ammo_before = ammo + 4 + self.field("item-ammunition-change", "before");
        let ammo_after = ammo + 4 + self.field("item-ammunition-change", "after");
        format!(
            "local.get 0 i32.load8_u offset={at} i32.eqz if local.get 0 i32.load offset={before} call @stack_drop@ local.get 0 i32.load8_u offset={after} if local.get 0 i32.load offset={} call @stack_drop@ end local.get 0 i32.load8_u offset={ammo} if local.get 0 i32.load offset={ammo_before} call @stack_drop@ local.get 0 i32.load8_u offset={ammo_after} if local.get 0 i32.load offset={} call @stack_drop@ end end end",
            after + 4,
            ammo_after + 4
        )
    }
}
/// Emit a compact real-guest witness from reserved scratch memory.
fn witness(value: u8) -> String {
    format!(
        "i32.const 3000 i32.const {value} i32.store8 i32.const 0 i32.const 3000 i32.const 1 call @log@"
    )
}

/// Read stage through the owning WIT layout, not a guessed flattened signature.
fn decision(deny: Option<u8>) -> String {
    let layout = Layout::new();
    let stage =
        layout.param("on-item-action", "proposal") + layout.field("item-action-proposal", "stage");
    let check = deny.map_or_else(
        || "i32.const 0".into(),
        |deny| format!("local.get 0 i32.load8_u offset={stage} i32.const {deny} i32.eq"),
    );
    format!(
        "{} {} i32.const 32 {check} i32.store8 i32.const 32",
        witness(b'P'),
        layout.drop_proposal()
    )
}
/// Actual physical completions retain detached items and report the true operation stage.
fn completion() -> String {
    let layout = Layout::new();
    let stage = layout.param("on-item-action-committed", "event")
        + layout.field("item-action-committed", "stage");
    format!(
        "i32.const 3000 local.get 0 i32.load8_u offset={stage} i32.const 65 i32.add i32.store8 i32.const 0 i32.const 3000 i32.const 1 call @log@ {}",
        layout.drop_committed()
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn air_veto_prevents_using_consumption_and_projectile_default() {
    for item in ["minecraft:apple", "minecraft:snowball"] {
        let mut h = Harness::new(decision(Some(0)), completion()).await;
        h.held(item, 2);
        let player = h.player;
        h.world_mut()
            .get_mut::<crate::entity::components::Hunger>(player)
            .unwrap()
            .food_level = 10;
        h.air(crate::server::game::item_use::UseRequest::UseAir)
            .await
            .unwrap();
        assert_eq!(h.count(), 2);
        assert_eq!(h.projectiles(), 0);
        assert!(
            h.world()
                .get::<crate::server::game::item_use::ItemUsing>(player)
                .is_none()
        );
        assert_eq!(h.logs(), vec!["P"]);
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn launch_veto_has_zero_spawn_and_held_cost() {
    let mut h = Harness::new(decision(Some(4)), completion()).await;
    h.held("minecraft:snowball", 2);
    h.air(crate::server::game::item_use::UseRequest::UseAir)
        .await
        .unwrap();
    assert_eq!(h.count(), 2);
    assert_eq!(h.projectiles(), 0);
    assert_eq!(h.logs(), vec!["P", "P"]);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_launch_cost_commits_exactly_once_and_supersedes_scalar_use() {
    let mut h = Harness::new(decision(None), completion()).await;
    h.held("minecraft:snowball", 2);
    h.air(crate::server::game::item_use::UseRequest::UseAir)
        .await
        .unwrap();
    assert_eq!(h.count(), 1);
    assert_eq!(h.projectiles(), 1);
    assert_eq!(h.logs(), vec!["P", "P", "E"]);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn consume_veto_preserves_food_and_the_original_selected_count() {
    let mut h = Harness::new(decision(Some(2)), completion()).await;
    h.held("minecraft:apple", 2);
    let player = h.player;
    h.world_mut()
        .get_mut::<crate::entity::components::Hunger>(player)
        .unwrap()
        .food_level = 10;
    h.air(crate::server::game::item_use::UseRequest::UseAir)
        .await
        .unwrap();
    h.air(crate::server::game::item_use::UseRequest::Consume)
        .await
        .unwrap();
    assert_eq!(h.count(), 2);
    assert_eq!(
        h.world()
            .get::<crate::entity::components::Hunger>(player)
            .unwrap()
            .food_level,
        10
    );
    assert_eq!(h.logs(), vec!["P", "A", "P"]);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_food_consumption_is_owned_and_repeated_release_does_not_consume_twice() {
    let mut h = Harness::new(decision(None), completion()).await;
    h.held("minecraft:apple", 2);
    let player = h.player;
    h.world_mut()
        .get_mut::<crate::entity::components::Hunger>(player)
        .unwrap()
        .food_level = 10;
    h.air(crate::server::game::item_use::UseRequest::UseAir)
        .await
        .unwrap();
    h.air(crate::server::game::item_use::UseRequest::Consume)
        .await
        .unwrap();
    h.air(crate::server::game::item_use::UseRequest::Consume)
        .await
        .unwrap();
    assert_eq!(h.count(), 1);
    assert_eq!(
        h.world()
            .get::<crate::entity::components::Hunger>(player)
            .unwrap()
            .food_level,
        14
    );
    assert_eq!(h.logs().iter().filter(|s| s.as_str() == "C").count(), 1);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typed_custom_consumption_is_bounded_and_inert_pass_has_no_completion() {
    for consume in [0, 1, 9] {
        let layout = Layout::new();
        let body = format!(
            "{} i32.const 32 i32.const 2 i32.store8 i32.const 40 i64.const 87 i64.store i32.const 48 i32.const {consume} i32.store8 i32.const 32",
            layout.drop_proposal()
        );
        let mut h = Harness::required(body, completion(), consume == 1).await;
        h.held("minecraft:paper", 3);
        h.air(crate::server::game::item_use::UseRequest::UseAir)
            .await
            .unwrap();
        assert_eq!(h.count(), if consume == 1 { 2 } else { 3 });
        assert_eq!(h.logs().len(), usize::from(consume == 1));
    }
    let mut h = Harness::new(decision(None), completion()).await;
    h.held("minecraft:paper", 3);
    h.air(crate::server::game::item_use::UseRequest::UseAir)
        .await
        .unwrap();
    assert_eq!(h.logs(), vec!["P"]);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn late_native_veto_and_stale_selected_stack_prevent_cost_and_completion() {
    use crate::server::game::item_actions::{ItemActionKind, PlayerItemAction};
    for stale in [false, true] {
        let mut h = Harness::new(decision(None), completion()).await;
        h.held("minecraft:snowball", 2);
        h.world_mut().add_observer(
            move |mut event: On<PlayerItemAction>, mut slots: Query<&mut HeldSlot>| {
                if event.kind() == ItemActionKind::Launch {
                    if stale {
                        slots.get_mut(event.player()).unwrap().0 = 1;
                    } else {
                        event.cancel();
                    }
                }
            },
        );
        h.air(crate::server::game::item_use::UseRequest::UseAir)
            .await
            .unwrap();
        assert_eq!(h.count(), 2);
        assert_eq!(h.projectiles(), 0);
        assert_eq!(h.logs(), vec!["P", "P"]);
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn optional_and_required_traps_reject_without_native_costs() {
    for required in [false, true] {
        let mut h = Harness::required("unreachable".into(), completion(), required).await;
        h.held("minecraft:snowball", 2);
        assert_eq!(
            h.air(crate::server::game::item_use::UseRequest::UseAir)
                .await
                .is_err(),
            required
        );
        if required {
            assert!(crate::server::game::inventory_effects::is_recovery_fenced(
                h.world(),
                h.player
            ));
            assert!(h.world().get::<MainInventory>(h.player).is_none());
        } else {
            assert_eq!(h.count(), 2);
        }
        assert_eq!(h.projectiles(), 0);
        assert!(h.logs().is_empty());
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn encoded_block_click_emits_only_the_real_correlated_block_completion() {
    let mut h = Harness::new(decision(None), completion()).await;
    h.held("minecraft:stone", 2);
    h.click(glam::IVec3::new(1, 15, 1), 1).await.unwrap();
    assert_eq!(h.count(), 1);
    assert_eq!(h.logs(), vec!["P", "A"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn crossbow_charge_has_its_own_veto_and_known_ammunition_cost() {
    use crate::server::game::item_use::{ItemUsing, UseRequest};
    for deny in [true, false] {
        let mut h = Harness::new(decision(deny.then_some(3)), completion()).await;
        h.held("minecraft:crossbow", 1);
        let player = h.player;
        let arrow = crate::server::game::world_stack(h.world(), "minecraft:arrow", 2).unwrap();
        h.world_mut()
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(1, arrow)
            .unwrap();
        h.air(UseRequest::UseAir).await.unwrap();
        h.world_mut()
            .resource_mut::<crate::ecs::resources::TickCounter>()
            .current = 50;
        h.world_mut().get_mut::<ItemUsing>(player).unwrap().since = 0;
        h.router.tick().await.unwrap();
        assert_eq!(
            h.world()
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(1)
                .unwrap()
                .count,
            if deny { 2 } else { 1 }
        );
        let stack = h
            .world()
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .unwrap();
        let loaded = crate::item::enchant::item_tag(stack)
            .is_some_and(|tag| tag.0.contains_key("chargedItem"));
        assert_eq!(loaded, !deny);
        assert_eq!(
            h.logs().iter().filter(|s| s.as_str() == "D").count(),
            usize::from(!deny)
        );
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn late_changed_bow_ammunition_rejects_before_spawn_or_weapon_wear() {
    use crate::server::game::item_actions::{ItemActionKind, PlayerItemAction};
    use crate::server::game::item_use::{ItemUsing, UseRequest};
    let mut h = Harness::new(decision(None), completion()).await;
    h.held("minecraft:bow", 1);
    let player = h.player;
    let arrow = crate::server::game::world_stack(h.world(), "minecraft:arrow", 2).unwrap();
    h.world_mut()
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(1, arrow)
        .unwrap();
    h.air(UseRequest::UseAir).await.unwrap();
    h.world_mut()
        .resource_mut::<crate::ecs::resources::TickCounter>()
        .current = 50;
    h.world_mut().get_mut::<ItemUsing>(player).unwrap().since = 0;
    h.world_mut().add_observer(
        |event: On<PlayerItemAction>, mut inventories: Query<&mut MainInventory>| {
            if event.kind() == ItemActionKind::Launch {
                let mut inventory = inventories.get_mut(event.player()).unwrap();
                let changed = inventory.0.item(1).unwrap().grow(-1);
                inventory.0.set_item(1, changed).unwrap();
            }
        },
    );
    h.air(UseRequest::Release).await.unwrap();
    assert_eq!(h.projectiles(), 0);
    assert_eq!(
        h.world()
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .damage,
        0
    );
    assert!(!h.logs().iter().any(|s| s == "E"));
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_actor_target_player_is_borrowed_but_inert_pass_never_reports_a_commit() {
    let layout = Layout::new();
    let at = layout.param("on-item-action", "target");
    let proposal = format!(
        "local.get 0 i32.load8_u offset={at} i32.eqz if unreachable end {} {} i32.const 32 i32.const 0 i32.store8 i32.const 32",
        witness(b'T'),
        layout.drop_proposal()
    );
    let mut h = Harness::new(proposal, completion()).await;
    h.held("minecraft:paper", 2);
    let (outbound_tx, _packets) = tokio::sync::mpsc::channel(65536);
    let _target = h
        .router
        .spawn_player(PlayerSpawnData {
            session_id: 43,
            runtime_id: 43,
            display_name: "Target".into(),
            xuid: Some("43".into()),
            uuid: Some(uuid::Uuid::from_u128(43).to_string()),
            position: glam::DVec3::new(1.5, 17.62, 0.5),
            outbound_tx,
            saved_data: None,
            chunk_radius: 0,
        })
        .unwrap();
    let packet = McpePacket::from(InventoryTransactionPacket {
        transaction: InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(
            ItemUseOnActorInventoryTransaction {
                runtime_id: ActorRuntimeId {
                    actor_runtime_id: 43,
                },
                action_type: EnumsItemUseOnActorInventoryTransactionActionType::ItemInteract,
                slot: 0,
                ..Default::default()
            },
        ),
        ..Default::default()
    });
    let mut bytes = bytes::BytesMut::new();
    packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
    h.router.route_packet(
        42,
        McpePacketArgs,
        BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap(),
    );
    h.router.tick().await.unwrap();
    assert_eq!(h.logs(), vec!["T"]);
    assert_eq!(h.count(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn selected_value_quota_rejects_before_default_cost_or_callback_entry() {
    let mut h = Harness::new("unreachable".into(), "unreachable".into()).await;
    h.held("minecraft:snowball", 2);
    h.manager
        .lock()
        .unwrap()
        .plugins
        .get_mut(&PluginId("example.items".into()))
        .unwrap()
        .store
        .data_mut()
        .api_limits
        .max_owned_resources = 2;
    assert!(
        h.air(crate::server::game::item_use::UseRequest::UseAir)
            .await
            .is_err()
    );
    assert!(crate::server::game::inventory_effects::is_recovery_fenced(
        h.world(),
        h.player
    ));
    assert!(h.world().get::<MainInventory>(h.player).is_none());
    assert_eq!(h.projectiles(), 0);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn late_changed_source_channel_rejects_the_original_launch_without_cost() {
    use crate::server::game::item_actions::{ItemActionKind, PlayerItemAction};
    let mut h = Harness::new(decision(None), completion()).await;
    h.held("minecraft:snowball", 2);
    h.world_mut().add_observer(
        |event: On<PlayerItemAction>,
         mut sessions: Query<&mut crate::entity::components::PlayerSession>| {
            if event.kind() == ItemActionKind::Launch {
                let (sender, _receiver) = tokio::sync::mpsc::channel(32);
                sessions.get_mut(event.player()).unwrap().outbound_tx = sender;
            }
        },
    );
    h.air(crate::server::game::item_use::UseRequest::UseAir)
        .await
        .unwrap();
    assert_eq!(h.count(), 2);
    assert_eq!(h.projectiles(), 0);
    assert_eq!(h.logs(), vec!["P", "P"]);
}

/// Native listeners can remove an actor after a real block effect without destroying owned item facts.
struct RemoveBlockSource;
impl crate::server::game::block_interaction::BlockInteractionHandler for RemoveBlockSource {
    fn on_committed(
        &self,
        world: &mut World,
        event: &crate::server::game::block_interaction::PlayerBlockCommitted,
    ) {
        world.despawn(event.player());
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn correlated_completion_keeps_identity_and_cost_values_after_source_removal() {
    let layout = Layout::new();
    let event = layout.param("on-item-action-committed", "event");
    let name = event
        + layout.field("item-action-committed", "player")
        + layout.field("player-identity", "name");
    let after = event
        + layout.field("item-action-committed", "items")
        + 4
        + layout.field("item-action-items", "selected-after");
    let body = format!(
        "i32.const 0 local.get 0 i32.load offset={name} local.get 0 i32.load offset={} call @log@ local.get 0 i32.load8_u offset={after} i32.eqz if unreachable end local.get 0 i32.load offset={} i32.const 128 call @[method]stack.count@ i32.const 128 i32.load8_u if unreachable end i32.const 129 i32.load8_u i32.const 1 i32.ne if unreachable end {}",
        name + 4,
        after + 4,
        completion()
    );
    let mut h = Harness::new(decision(None), body).await;
    h.held("minecraft:stone", 2);
    h.world_mut()
        .resource_mut::<crate::server::game::block_interaction::BlockInteractionHooks>()
        .register(RemoveBlockSource);
    h.click(glam::IVec3::new(1, 15, 1), 1).await.unwrap();
    assert_eq!(h.logs(), vec!["P", "Player", "A"]);
    assert!(h.world().get_entity(h.player).is_err());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn later_native_block_veto_never_emits_an_item_completion() {
    let mut h = Harness::new(decision(None), completion()).await;
    h.held("minecraft:stone", 2);
    h.world_mut().add_observer(
        |mut event: On<crate::server::game::block_interaction::BlockInteraction>| event.cancel(),
    );
    h.click(glam::IVec3::new(1, 15, 1), 1).await.unwrap();
    assert_eq!(h.count(), 2);
    assert_eq!(h.logs(), vec!["P"]);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unsubscribed_component_preserves_normal_item_defaults() {
    let mut h = Harness::new("unreachable".into(), "unreachable".into()).await;
    h.held("minecraft:snowball", 2);
    h.manager
        .lock()
        .unwrap()
        .plugins
        .get_mut(&PluginId("example.items".into()))
        .unwrap()
        .manifest
        .subscriptions
        .clear();
    h.air(crate::server::game::item_use::UseRequest::UseAir)
        .await
        .unwrap();
    assert_eq!(h.count(), 1);
    assert_eq!(h.projectiles(), 1);
    assert!(h.logs().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_mob_target_never_manufactures_a_borrowed_player() {
    let layout = Layout::new();
    let at = layout.param("on-item-action", "target");
    let proposal = format!(
        "local.get 0 i32.load8_u offset={at} if unreachable end {} {} i32.const 32 i32.const 0 i32.store8 i32.const 32",
        witness(b'M'),
        layout.drop_proposal()
    );
    let mut h = Harness::new(proposal, completion()).await;
    h.held("minecraft:wheat", 2);
    let mob = crate::entity::mob::spawn_mob(
        h.world_mut(),
        "minecraft:cow",
        glam::DVec3::new(1.5, 16.0, 1.5),
        0.0,
        crate::entity::mob::SpawnCause::Spawned,
    )
    .unwrap();
    let runtime = h
        .world()
        .get::<crate::entity::components::RuntimeId>(mob)
        .unwrap()
        .0 as u64;
    assert_ne!(runtime, 42);
    let packet = McpePacket::from(InventoryTransactionPacket {
        transaction: InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(
            ItemUseOnActorInventoryTransaction {
                runtime_id: ActorRuntimeId {
                    actor_runtime_id: runtime,
                },
                action_type: EnumsItemUseOnActorInventoryTransactionActionType::ItemInteract,
                slot: 0,
                ..Default::default()
            },
        ),
        ..Default::default()
    });
    let mut bytes = bytes::BytesMut::new();
    packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
    h.router.route_packet(
        42,
        McpePacketArgs,
        BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap(),
    );
    h.router.tick().await.unwrap();
    assert_eq!(h.logs(), vec!["M"]);
    assert_eq!(h.count(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unreachable_actor_input_is_rejected_before_the_item_callback_or_any_cost() {
    let mut h = Harness::new("unreachable".into(), "unreachable".into()).await;
    h.held("minecraft:wheat", 2);
    let mob = crate::entity::mob::spawn_mob(
        h.world_mut(),
        "minecraft:cow",
        glam::DVec3::new(10.5, 16.0, 10.5),
        0.0,
        crate::entity::mob::SpawnCause::Spawned,
    )
    .unwrap();
    let runtime = h
        .world()
        .get::<crate::entity::components::RuntimeId>(mob)
        .unwrap()
        .0 as u64;
    let packet = McpePacket::from(InventoryTransactionPacket {
        transaction: InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(
            ItemUseOnActorInventoryTransaction {
                runtime_id: ActorRuntimeId {
                    actor_runtime_id: runtime,
                },
                action_type: EnumsItemUseOnActorInventoryTransactionActionType::Interact,
                slot: 0,
                ..Default::default()
            },
        ),
        ..Default::default()
    });
    let mut bytes = bytes::BytesMut::new();
    packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
    h.router.route_packet(
        42,
        McpePacketArgs,
        BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap(),
    );
    h.router.tick().await.unwrap();
    assert!(h.logs().is_empty());
    assert_eq!(h.count(), 2);
    assert!(
        h.world()
            .get::<crate::entity::mob::state::InLove>(mob)
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn denying_real_item_use_preserves_explicit_bounce_with_zero_default_cost_or_projectiles() {
    let layout = Layout::new();
    let player = layout.param("on-item-action", "player");
    let body = format!(
        "local.get 0 i32.load offset={player} f64.const 0.5 f64.const 0.25 f64.const 0 i32.const 64 call @[method]player.set-velocity@ i32.const 64 i32.load8_u if unreachable end {} i32.const 32 i32.const 1 i32.store8 i32.const 32",
        layout.drop_proposal()
    );
    let mut h = Harness::with_capabilities(
        body,
        "unreachable".into(),
        true,
        &[crate::plugin::manifest::PluginCapability::PlayerMechanics],
    )
    .await;
    h.held("minecraft:snowball", 2);
    let before = h
        .world()
        .get::<crate::entity::components::Position>(h.player)
        .unwrap()
        .0;
    while h._packets.try_recv().is_ok() {}
    h.air(crate::server::game::item_use::UseRequest::UseAir)
        .await
        .unwrap();
    assert_eq!(h.count(), 2);
    assert_eq!(h.projectiles(), 0);
    assert!(
        h.world()
            .get::<crate::server::game::item_use::ItemUsing>(h.player)
            .is_none()
    );
    assert_eq!(
        h.world()
            .get::<crate::entity::components::Position>(h.player)
            .unwrap()
            .0,
        before
    );
    assert_eq!(
        h.world()
            .get::<crate::entity::components::Velocity>(h.player)
            .unwrap()
            .0,
        glam::DVec3::new(0.5, 0.25, 0.0)
    );
    assert!(
        std::iter::from_fn(|| h._packets.try_recv().ok())
            .any(|packet| matches!(packet.data, McpePacketData::SetActorMotionPacket(_)))
    );
    assert!(h.manager.lock().unwrap().readiness.check().is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_handled_counts_discard_replacements_and_never_consume_or_launch() {
    for consume in [0, 9] {
        let layout = Layout::new();
        let player = layout.param("on-item-action", "player");
        let body = format!(
            "local.get 0 i32.load offset={player} f64.const 0.5 f64.const 0 f64.const 0 i32.const 64 call @[method]player.set-velocity@ i32.const 64 i32.load8_u if unreachable end i32.const 1100 i32.const 120 i32.store8 local.get 0 i32.load offset={player} i32.const 1100 i32.const 1 i32.const 80 call @[method]player.message@ i32.const 80 i32.load8_u if unreachable end {} i32.const 32 i32.const 2 i32.store8 i32.const 40 i64.const 87 i64.store i32.const 48 i32.const {consume} i32.store8 i32.const 32",
            layout.drop_proposal()
        );
        let mut h = Harness::with_capabilities(
            body,
            "unreachable".into(),
            false,
            &[crate::plugin::manifest::PluginCapability::PlayerMechanics],
        )
        .await;
        h.held("minecraft:paper", 3);
        while h._packets.try_recv().is_ok() {}
        h.air(crate::server::game::item_use::UseRequest::UseAir)
            .await
            .unwrap();
        assert_eq!(h.count(), 3);
        assert_eq!(h.projectiles(), 0);
        assert!(
            h.world()
                .get::<crate::entity::components::Velocity>(h.player)
                .is_none()
        );
        assert!(
            !std::iter::from_fn(|| h._packets.try_recv().ok()).any(|packet| matches!(
                packet.data,
                McpePacketData::SetActorMotionPacket(_) | McpePacketData::TextPacket(_)
            ))
        );
        assert!(h.manager.lock().unwrap().readiness.check().is_ok());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn required_malformed_item_reply_uses_the_existing_terminal_callback_failure_policy() {
    let layout = Layout::new();
    let body = format!(
        "{} i32.const 32 i32.const 2 i32.store8 i32.const 40 i64.const 87 i64.store i32.const 48 i32.const 0 i32.store8 i32.const 32",
        layout.drop_proposal()
    );
    let mut h = Harness::new(body, "unreachable".into()).await;
    h.held("minecraft:paper", 3);
    assert!(
        h.air(crate::server::game::item_use::UseRequest::UseAir)
            .await
            .is_err()
    );
    assert!(h.manager.lock().unwrap().readiness.check().is_err());
    assert!(crate::server::game::inventory_effects::is_recovery_fenced(
        h.world(),
        h.player
    ));
    assert_eq!(h.projectiles(), 0);
}
