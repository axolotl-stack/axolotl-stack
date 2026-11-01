//! Actual component block decisions are checked against encoded inputs and native world/item effects.

use super::*;
use crate::ecs::events::EventKind;
use crate::entity::components::{HeldSlot, MainInventory};
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
use crate::server::worlds::DimensionRouter;
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
    /// Compose independent capability families in one real component.
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
            id: PluginId("example.blocks".into()),
            name: "Blocks".into(),
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
                EventKind::BlockTransaction,
                EventKind::BlockCommitted,
                EventKind::BlockBreak,
                EventKind::BlockPlace,
                EventKind::PlayerInteractBlock,
            ]
            .into(),
            limits: PluginLimits::default(),
        };
        let pending = PendingPlugin {
            component: fixture::component(
                &engine,
                &[
                    ("on-block-transaction", proposal),
                    ("on-block-committed", committed),
                    ("on-block-break", "unreachable".into()),
                    ("on-block-place", "unreachable".into()),
                    ("on-player-interact-block", "unreachable".into()),
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
    /// Read actual native block state after every accepted or rejected operation.
    fn block(&self, position: glam::IVec3) -> u32 {
        crate::server::game::blocks::block_at(
            self.router.world(0).unwrap().ecs.world(),
            position.into(),
        )
        .unwrap()
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
    /// Guest logs witness actual canonical invocation without renewing fuel.
    fn logs(&self) -> Vec<String> {
        self.manager.lock().unwrap().plugins[&PluginId("example.blocks".into())]
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
    /// Obtain a cell stride for list traversal without assuming a host struct layout.
    fn size(&self, record: &str) -> usize {
        self.sizes
            .size(&wit_parser::Type::Id(self.record(record)))
            .size_wasm32()
    }
    /// Drop the owned source value exactly as the Rust guest bridge does after its handler returns.
    fn drop_proposal(&self) -> String {
        let held =
            self.param("on-block-transaction", "proposal") + self.field("block-proposal", "held");
        format!(
            "local.get 0 i32.load8_u offset={held} if local.get 0 i32.load offset={} call @stack_drop@ end",
            held + 4
        )
    }
    /// Drop both optional owned held values when committed batch capture succeeded.
    fn drop_committed(&self) -> String {
        let items =
            self.param("on-block-committed", "event") + self.field("block-committed", "held-items");
        let mut code = format!("local.get 0 i32.load8_u offset={items} i32.eqz if ");
        for name in ["before", "after"] {
            let at = items + 4 + self.field("block-held-items", name);
            code.push_str(&format!("local.get 0 i32.load8_u offset={at} if local.get 0 i32.load offset={} call @stack_drop@ end ",at+4));
        }
        code.push_str("end");
        code
    }
}
/// Emit a compact real-guest witness from reserved scratch memory.
fn witness(value: u8) -> String {
    format!(
        "i32.const 3000 i32.const {value} i32.store8 i32.const 0 i32.const 3000 i32.const 1 call @log@"
    )
}
/// Inspect all door cells through the actual indirect proposal ABI, then accept or cancel once.
fn door_proposal(allow: bool) -> String {
    let layout = Layout::new();
    let proposal = layout.param("on-block-transaction", "proposal");
    let cells = proposal + layout.field("block-proposal", "cells");
    let upper_y = layout.size("planned-block-cell")
        + layout.field("planned-block-cell", "position")
        + layout.field("block-pos", "y");
    let id = proposal + layout.field("block-proposal", "operation-id");
    format!(
        "i32.const 3104 local.get 0 i64.load offset={id} i64.store local.get 0 i32.load offset={} i32.const 2 i32.ne if unreachable end local.get 0 i32.load offset={cells} i32.load offset={upper_y} i32.const 17 i32.ne if unreachable end {} {} i32.const {}",
        cells + 4,
        witness(b'P'),
        layout.drop_proposal(),
        u8::from(allow)
    )
}
/// A real committed callback validates two actual changes and releases its supplied values.
fn door_committed() -> String {
    let layout = Layout::new();
    let event = layout.param("on-block-committed", "event");
    let cells = event + layout.field("block-committed", "cells");
    let id = event + layout.field("block-committed", "operation-id");
    format!(
        "local.get 0 i64.load offset={id} i32.const 3104 i64.load i64.ne if unreachable end local.get 0 i32.load8_u offset={cells} if unreachable end local.get 0 i32.load offset={} i32.const 2 i32.ne if unreachable end {} {}",
        cells + 8,
        witness(b'C'),
        layout.drop_committed()
    )
}

/// Inspect a complete multi-cell use batch through its real canonical record.
fn batch_proposal(allow: bool, minimum: usize) -> String {
    let layout = Layout::new();
    let proposal = layout.param("on-block-transaction", "proposal");
    let cells = proposal + layout.field("block-proposal", "cells");
    format!(
        "local.get 0 i32.load offset={} i32.const {minimum} i32.lt_u if unreachable end {} {} i32.const {}",
        cells + 4,
        witness(b'P'),
        layout.drop_proposal(),
        u8::from(allow)
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_veto_cancels_both_door_cells_and_the_item_cost() {
    let mut h = Harness::new(door_proposal(false), "unreachable".into()).await;
    h.held("minecraft:wooden_door", 2);
    h.click(glam::IVec3::new(1, 15, 1), 1).await.unwrap();
    assert_eq!(h.logs(), ["P"]);
    assert_eq!(
        h.block(glam::IVec3::new(1, 16, 1)),
        *crate::world::chunk::blocks::AIR
    );
    assert_eq!(
        h.block(glam::IVec3::new(1, 17, 1)),
        *crate::world::chunk::blocks::AIR
    );
    assert_eq!(h.count(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepted_plan_commits_both_cells_once_and_skips_scalar_compatibility_callbacks() {
    let mut h = Harness::new(door_proposal(true), door_committed()).await;
    h.held("minecraft:wooden_door", 2);
    h.click(glam::IVec3::new(1, 15, 1), 1).await.unwrap();
    assert_eq!(h.logs(), ["P", "C"]);
    assert_ne!(
        h.block(glam::IVec3::new(1, 16, 1)),
        *crate::world::chunk::blocks::AIR
    );
    assert_ne!(
        h.block(glam::IVec3::new(1, 17, 1)),
        *crate::world::chunk::blocks::AIR
    );
    assert_eq!(h.count(), 1);
}

/// A native authorization handler remains able to reject before managed component dispatch.
struct NativeVeto;
impl crate::server::game::block_interaction::BlockInteractionHandler for NativeVeto {
    fn on_interaction(
        &self,
        _: &mut bevy_ecs::world::World,
        event: &mut crate::server::game::block_interaction::BlockInteraction,
    ) {
        event.cancel();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn earlier_native_veto_emits_no_component_completion_or_item_cost() {
    let mut h = Harness::new(door_proposal(true), "unreachable".into()).await;
    h.held("minecraft:wooden_door", 2);
    h.router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .resource_mut::<crate::server::game::block_interaction::BlockInteractionHooks>()
        .register(NativeVeto);
    h.click(glam::IVec3::new(1, 15, 1), 1).await.unwrap();
    assert!(h.logs().is_empty());
    assert_eq!(h.count(), 2);
    assert_eq!(
        h.block(glam::IVec3::new(1, 16, 1)),
        *crate::world::chunk::blocks::AIR
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn late_native_observer_veto_does_not_turn_an_accepted_proposal_into_a_completion() {
    let mut h = Harness::new(door_proposal(true), "unreachable".into()).await;
    h.held("minecraft:wooden_door", 2);
    h.router.world_mut(0).unwrap().ecs.world_mut().add_observer(
        |mut event: bevy_ecs::observer::On<
            crate::server::game::block_interaction::BlockInteraction,
        >| event.cancel(),
    );
    h.click(glam::IVec3::new(1, 15, 1), 1).await.unwrap();
    assert_eq!(h.logs(), ["P"]);
    assert_eq!(h.count(), 2);
    assert_eq!(
        h.block(glam::IVec3::new(1, 16, 1)),
        *crate::world::chunk::blocks::AIR
    );
    assert_eq!(
        h.block(glam::IVec3::new(1, 17, 1)),
        *crate::world::chunk::blocks::AIR
    );
}

/// Mutate one captured cell during authorization without granting the ordinary operation permission.
struct ChangeUpper;
impl crate::server::game::block_interaction::BlockInteractionHandler for ChangeUpper {
    fn on_interaction(
        &self,
        world: &mut bevy_ecs::world::World,
        event: &mut crate::server::game::block_interaction::BlockInteraction,
    ) {
        if event.kind() == crate::server::game::block_interaction::BlockInteractionKind::Place {
            crate::server::game::blocks::set_block(
                world,
                glam::IVec3::new(1, 17, 1),
                *crate::world::chunk::blocks::STONE,
                false,
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_before_state_rejects_the_whole_stale_plan_after_component_approval() {
    let mut h = Harness::new(door_proposal(true), "unreachable".into()).await;
    h.held("minecraft:wooden_door", 2);
    h.router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .resource_mut::<crate::server::game::block_interaction::BlockInteractionHooks>()
        .register(ChangeUpper);
    h.click(glam::IVec3::new(1, 15, 1), 1).await.unwrap();
    assert_eq!(h.logs(), ["P"]);
    assert_eq!(h.count(), 2);
    assert_eq!(
        h.block(glam::IVec3::new(1, 16, 1)),
        *crate::world::chunk::blocks::AIR
    );
    assert_eq!(
        h.block(glam::IVec3::new(1, 17, 1)),
        *crate::world::chunk::blocks::STONE
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multi_cell_growth_is_planned_before_a_single_veto_and_consumes_no_item() {
    let mut h = Harness::new(batch_proposal(false, 2), "unreachable".into()).await;
    h.held("minecraft:bone_meal", 2);
    let world = h.router.world_mut(0).unwrap().ecs.world_mut();
    let grass = world
        .resource::<crate::server::game::types::BlockRegistryResource>()
        .0
        .default_state_id_by_name("minecraft:grass_block")
        .unwrap();
    crate::server::game::blocks::set_block(world, glam::IVec3::new(1, 15, 1), grass, false);
    h.click(glam::IVec3::new(1, 15, 1), 1).await.unwrap();
    assert_eq!(h.logs(), ["P"]);
    assert_eq!(h.count(), 2);
    assert_eq!(
        h.block(glam::IVec3::new(1, 16, 1)),
        *crate::world::chunk::blocks::AIR
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adjacent_bucket_plan_is_vetoed_before_block_or_source_replacement() {
    let mut h = Harness::new(batch_proposal(false, 2), "unreachable".into()).await;
    h.held("minecraft:water_bucket", 1);
    h.click(glam::IVec3::new(1, 15, 1), 1).await.unwrap();
    assert_eq!(h.logs(), ["P"]);
    assert_eq!(h.count(), 1);
    assert_eq!(
        h.block(glam::IVec3::new(1, 16, 1)),
        *crate::world::chunk::blocks::AIR
    );
    assert_eq!(
        h.router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get::<MainInventory>(h.player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .item_id(),
        "minecraft:water_bucket"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn affected_cell_quota_rejects_the_whole_plan_without_item_cost() {
    let mut h = Harness::new("unreachable".into(), "unreachable".into()).await;
    h.held("minecraft:wooden_door", 2);
    h.manager
        .lock()
        .unwrap()
        .plugins
        .get_mut(&PluginId("example.blocks".into()))
        .unwrap()
        .manifest
        .limits
        .max_block_cells = 1;
    h.click(glam::IVec3::new(1, 15, 1), 1).await.unwrap();
    assert!(h.logs().is_empty());
    assert_eq!(h.count(), 2);
    assert_eq!(
        h.block(glam::IVec3::new(1, 16, 1)),
        *crate::world::chunk::blocks::AIR
    );
    assert_eq!(
        h.block(glam::IVec3::new(1, 17, 1)),
        *crate::world::chunk::blocks::AIR
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn optional_trap_discards_the_plan_and_preserves_world_and_selected_stack() {
    let mut h = Harness::required(
        format!("{} unreachable", door_proposal(true)),
        "unreachable".into(),
        false,
    )
    .await;
    h.held("minecraft:wooden_door", 2);
    h.click(glam::IVec3::new(1, 15, 1), 1).await.unwrap();
    assert_eq!(h.count(), 2);
    assert_eq!(
        h.block(glam::IVec3::new(1, 16, 1)),
        *crate::world::chunk::blocks::AIR
    );
    assert_eq!(
        h.block(glam::IVec3::new(1, 17, 1)),
        *crate::world::chunk::blocks::AIR
    );
    assert!(h.manager.lock().unwrap().check_required().is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn required_trap_fences_the_original_actor_before_any_default_block_effect() {
    let mut h = Harness::new(
        format!("{} unreachable", door_proposal(true)),
        "unreachable".into(),
    )
    .await;
    h.held("minecraft:wooden_door", 2);
    assert!(h.click(glam::IVec3::new(1, 15, 1), 1).await.is_err());
    assert_eq!(
        h.block(glam::IVec3::new(1, 16, 1)),
        *crate::world::chunk::blocks::AIR
    );
    assert_eq!(
        h.block(glam::IVec3::new(1, 17, 1)),
        *crate::world::chunk::blocks::AIR
    );
    assert!(crate::server::game::inventory_effects::is_recovery_fenced(
        h.router.world(0).unwrap().ecs.world(),
        h.player
    ));
    assert!(h.manager.lock().unwrap().check_required().is_err());
}

/// A synchronous native completion listener may remove the source before the owned WASM observer.
struct RemoveAfterCommit;
impl crate::server::game::block_interaction::BlockInteractionHandler for RemoveAfterCommit {
    fn on_committed(
        &self,
        world: &mut bevy_ecs::world::World,
        event: &crate::server::game::block_interaction::PlayerBlockCommitted,
    ) {
        world.despawn(event.player());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn committed_identity_and_item_values_remain_owned_after_the_source_actor_is_removed() {
    let layout = Layout::new();
    let event = layout.param("on-block-committed", "event");
    let name =
        event + layout.field("block-committed", "player") + layout.field("player-identity", "name");
    let items = event + layout.field("block-committed", "held-items") + 4;
    let mut body = format!(
        "i32.const 0 local.get 0 i32.load offset={name} local.get 0 i32.load offset={} call @log@ ",
        name + 4
    );
    for (field, count) in [("before", 2), ("after", 1)] {
        let item = items + layout.field("block-held-items", field);
        body.push_str(&format!("local.get 0 i32.load8_u offset={item} i32.eqz if unreachable end local.get 0 i32.load offset={} i32.const 128 call @[method]stack.count@ i32.const 128 i32.load8_u if unreachable end i32.const 129 i32.load8_u i32.const {count} i32.ne if unreachable end ",item+4));
    }
    body.push_str(&door_committed());
    let mut h = Harness::new(door_proposal(true), body).await;
    h.held("minecraft:wooden_door", 2);
    h.router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .resource_mut::<crate::server::game::block_interaction::BlockInteractionHooks>()
        .register(RemoveAfterCommit);
    h.click(glam::IVec3::new(1, 15, 1), 1).await.unwrap();
    assert_eq!(h.logs(), ["P", "Player", "C"]);
    assert!(
        h.router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get_entity(h.player)
            .is_err()
    );
    assert_ne!(
        h.block(glam::IVec3::new(1, 16, 1)),
        *crate::world::chunk::blocks::AIR
    );
}

/// Change only the actor of an unchanged reciprocal partner during proposal callbacks.
struct ChangePartnerActor(glam::IVec3);
impl crate::server::game::block_interaction::BlockInteractionHandler for ChangePartnerActor {
    fn on_interaction(
        &self,
        world: &mut bevy_ecs::world::World,
        event: &mut crate::server::game::block_interaction::BlockInteraction,
    ) {
        assert!(event.positions().contains(&self.0));
        let (cx, cz) = crate::world::ecs::world_to_chunk_coords(self.0.x, self.0.z);
        let chunk = world
            .resource::<crate::world::ecs::ChunkManager>()
            .get_by_coords(cx, cz)
            .unwrap();
        let actor = world
            .get::<crate::world::ecs::ChunkData>(chunk)
            .unwrap()
            .inner
            .block_entities()
            .iter()
            .find(|actor| actor.position() == self.0.to_array())
            .unwrap()
            .clone();
        let mut compound = actor.bedrock_compound().unwrap().unwrap();
        compound
            .0
            .insert("example".into(), zuri_nbt::NBTTag::String("changed".into()));
        let actor =
            crate::world::block_entity::BlockEntity::from_bedrock_compound(compound).unwrap();
        world
            .get_mut::<crate::world::ecs::ChunkData>(chunk)
            .unwrap()
            .inner
            .set_block_entity(actor);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unchanged_partner_actor_change_invalidates_the_whole_captured_plan() {
    let layout = Layout::new();
    let proposal = layout.param("on-block-transaction", "proposal");
    let cells = proposal + layout.field("block-proposal", "cells");
    let body = format!(
        "i32.const 3204 i32.load if local.get 0 i32.load offset={} i32.const 2 i32.ne if unreachable end end i32.const 3204 i32.const 1 i32.store {}",
        cells + 4,
        batch_proposal(true, 1)
    );
    let committed = format!("{} {}", witness(b'C'), layout.drop_committed());
    let mut h = Harness::new(body, committed).await;
    h.held("minecraft:chest", 2);
    let first = glam::IVec3::new(2, 16, 2);
    h.click(first - glam::IVec3::Y, 1).await.unwrap();
    assert_eq!(h.count(), 1);
    let state = h.block(first);
    let world = h.router.world(0).unwrap().ecs.world();
    let registry = &world
        .resource::<crate::server::game::types::BlockRegistryResource>()
        .0;
    let block = registry.get_by_runtime_id(state).unwrap();
    let next = match block
        .state_value(state, "minecraft:cardinal_direction")
        .unwrap()
    {
        crate::registry::block::StateValue::Str("north" | "south") => first - glam::IVec3::X,
        _ => first - glam::IVec3::Z,
    };
    h.router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .resource_mut::<crate::server::game::block_interaction::BlockInteractionHooks>()
        .register(ChangePartnerActor(first));
    h.click(next - glam::IVec3::Y, 1).await.unwrap();
    assert_eq!(h.logs(), ["P", "C", "P"]);
    assert_eq!(h.count(), 1);
    assert_eq!(h.block(first), state);
    assert_eq!(h.block(next), *crate::world::chunk::blocks::AIR);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn raw_live_player_retained_from_an_earlier_block_scope_is_rejected() {
    let layout = Layout::new();
    let tx = layout.param("on-block-transaction", "tx");
    let player = layout.param("on-block-transaction", "player");
    let body = format!(
        r#"
        i32.const 3204 i32.load i32.eqz if
            local.get 0 i32.load offset={player} i32.const 128 call @[method]player.to-handle@
            i32.const 128 i32.load8_u if unreachable end
            local.get 0 i32.load offset={tx} i32.const 132 i32.load i32.const 144 call @[method]world-tx.player@
            i32.const 144 i32.load8_u if unreachable end
            i32.const 3200 i32.const 148 i32.load i32.store
            i32.const 132 i32.load call @player-handle_drop@
        else
            i32.const 3200 i32.load i32.const 160 call @[method]player.position@
            i32.const 160 i32.load8_u i32.const 1 i32.ne if unreachable end
            i32.const 3200 i32.load call @player_drop@
            {}
        end
        i32.const 3204 i32.const 1 i32.store
        {} i32.const 0
    "#,
        witness(b'S'),
        layout.drop_proposal()
    );
    let mut h = Harness::new(body, "unreachable".into()).await;
    h.held("minecraft:wooden_door", 2);
    h.click(glam::IVec3::new(1, 15, 1), 1).await.unwrap();
    h.click(glam::IVec3::new(1, 15, 1), 1).await.unwrap();
    assert_eq!(h.logs(), ["S"]);
    assert_eq!(h.count(), 2);
    assert_eq!(
        h.block(glam::IVec3::new(1, 16, 1)),
        *crate::world::chunk::blocks::AIR
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typed_before_proposed_properties_and_actual_after_values_cross_the_real_component_abi() {
    let layout = Layout::new();
    let proposal = layout.param("on-block-transaction", "proposal");
    let cells = proposal + layout.field("block-proposal", "cells");
    let before = layout.field("planned-block-cell", "before");
    let proposed =
        layout.size("planned-block-cell") + layout.field("planned-block-cell", "proposed");
    let properties = proposed + layout.field("block-value", "properties");
    let stride = layout.size("block-property");
    let value = layout.field("block-property", "value");
    let log_identifier = |offset: usize| {
        format!(
            "i32.const 0 local.get 0 i32.load offset={cells} i32.load offset={offset} local.get 0 i32.load offset={cells} i32.load offset={} call @log@ ",
            offset + 4
        )
    };
    let mut body = format!(
        "{} {} i32.const 3300 local.get 0 i32.load offset={cells} i32.load offset={properties} i32.store i32.const 3304 local.get 0 i32.load offset={cells} i32.load offset={} i32.store i32.const 3308 i32.const 0 i32.store ",
        log_identifier(before),
        log_identifier(proposed),
        properties + 4
    );
    body.push_str(&format!(r#"
        block $done loop $next
            i32.const 3308 i32.load i32.const 3304 i32.load i32.ge_u br_if $done
            i32.const 3312 i32.const 3300 i32.load i32.const 3308 i32.load i32.const {stride} i32.mul i32.add i32.store
            i32.const 0 i32.const 3312 i32.load i32.load i32.const 3312 i32.load i32.load offset=4 call @log@
            i32.const 3312 i32.load i32.load8_u offset={value} i32.eqz if
                i32.const 3000 i32.const 3312 i32.load i32.load8_u offset={} i32.const 48 i32.add i32.store8
                i32.const 0 i32.const 3000 i32.const 1 call @log@
            end
            i32.const 3308 i32.const 3308 i32.load i32.const 1 i32.add i32.store br $next
        end end
    "#,value+4));
    body.push_str(&door_proposal(true));
    let event = layout.param("on-block-committed", "event");
    let changes = event + layout.field("block-committed", "cells");
    let after = layout.size("committed-block-cell") + layout.field("committed-block-cell", "after");
    let committed = format!(
        "local.get 0 i32.load offset={} i32.load8_u offset={after} i32.eqz if unreachable end i32.const 0 local.get 0 i32.load offset={} i32.load offset={} local.get 0 i32.load offset={} i32.load offset={} call @log@ {}",
        changes + 4,
        changes + 4,
        after + 4,
        changes + 4,
        after + 8,
        door_committed()
    );
    let mut h = Harness::new(body, committed).await;
    h.held("minecraft:wooden_door", 2);
    h.click(glam::IVec3::new(1, 15, 1), 1).await.unwrap();
    let logs = h.logs();
    assert_eq!(logs[0], "minecraft:air");
    assert_eq!(logs[1], "minecraft:wooden_door");
    assert!(logs.windows(2).any(|pair| pair == ["upper_block_bit", "1"]));
    assert_eq!(&logs[logs.len() - 2..], ["minecraft:wooden_door", "C"]);
    assert_eq!(h.count(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn composed_block_veto_preserves_explicit_ui_reply_and_image_accounting_but_discards_mechanics()
 {
    use crate::plugin::manifest::PluginCapability;
    let layout = Layout::new();
    let tx = layout.param("on-block-transaction", "tx");
    let player = layout.param("on-block-transaction", "player");
    let proposal = format!(
        r#"
        local.get 0 i32.load offset={player} i32.const 64 call @[method]player.inventory@
        i32.const 64 i32.load8_u if unreachable end
        i32.const 68 i32.load call @inventory-image_drop@
        local.get 0 i32.load offset={player} f64.const 0.5 f64.const 0 f64.const 0 i32.const 80 call @[method]player.set-velocity@
        i32.const 80 i32.load8_u if unreachable end
        i32.const 1100 i32.const 120 i32.store8
        local.get 0 i32.load offset={tx} local.get 0 i32.load offset={player} i32.const 1 i32.const 1100 i32.const 1 i32.const 96 call @set-action-bar@
        i32.const 96 i32.load8_u if unreachable end
        local.get 0 i32.load offset={player} i32.const 1100 i32.const 1 i32.const 112 call @[method]player.message@
        i32.const 112 i32.load8_u if unreachable end
        {} {} i32.const 0
    "#,
        layout.drop_proposal(),
        witness(b'R')
    );
    let mut h = Harness::with_capabilities(
        proposal,
        "unreachable".into(),
        true,
        &[
            PluginCapability::ReadInventory,
            PluginCapability::Presentation,
            PluginCapability::PlayerMechanics,
        ],
    )
    .await;
    h.held("minecraft:wooden_door", 2);
    while h._packets.try_recv().is_ok() {}
    h.click(glam::IVec3::new(1, 15, 1), 1).await.unwrap();
    assert_eq!(h.count(), 2);
    assert_eq!(
        h.block(glam::IVec3::new(1, 16, 1)),
        *crate::world::chunk::blocks::AIR
    );
    assert!(
        h.router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get::<crate::entity::components::Velocity>(h.player)
            .is_none()
    );
    let packets = std::iter::from_fn(|| h._packets.try_recv().ok()).collect::<Vec<_>>();
    assert!(
        packets
            .iter()
            .any(|packet| matches!(&packet.data, McpePacketData::TextPacket(_)))
    );
    assert!(packets.iter().any(|packet|matches!(&packet.data,McpePacketData::SetTitlePacket(value) if &*value.title_text=="x")));
    assert!(
        !packets
            .iter()
            .any(|packet| matches!(&packet.data, McpePacketData::SetActorMotionPacket(_)))
    );
    assert_eq!(
        h.manager
            .lock()
            .unwrap()
            .plugins
            .values()
            .next()
            .unwrap()
            .store
            .data()
            .test_item_bytes(),
        0
    );
}
