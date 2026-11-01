//! Genuine scoped component calls exercise staged exact-connection termination.

use super::*;
use crate::entity::components::{Health, PlayerSession};
use crate::plugin::manifest::PluginCapability;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
use crate::server::worlds::DimensionRouter;
use bevy_ecs::prelude::*;
use jolyne::valentine::*;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Harness {
    router: DimensionRouter,
    manager: Arc<Mutex<PluginManager>>,
    player: Entity,
    packets: tokio::sync::mpsc::Receiver<McpePacket>,
}
impl Harness {
    /// Attach real component exports before ordinary native player admission.
    async fn new(command: String, tick: String, capability: bool) -> Self {
        Self::with_runtime(command, tick, capability, 42).await
    }
    /// A real transport test supplies the runtime identity negotiated in its actual StartGame.
    async fn with_runtime(command: String, tick: String, capability: bool, runtime: i64) -> Self {
        Self::with_exports(command, tick, capability, runtime, &[]).await
    }
    /// Additional real lifecycle exports exercise initialization without bypassing admission.
    async fn with_exports(
        command: String,
        tick: String,
        capability: bool,
        runtime: i64,
        extra: &[(&str, String)],
    ) -> Self {
        let mut router = DimensionRouter::new(GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        }));
        let engine = engine();
        let manifest = PluginManifest {
            id: PluginId("example.connection".into()),
            name: "Connections".into(),
            version: "1".into(),
            api_version: PLUGIN_API_VERSION.into(),
            required: false,
            config: None,
            capabilities: if capability {
                [PluginCapability::ConnectionControl].into()
            } else {
                Default::default()
            },
            subscriptions: [
                crate::ecs::events::EventKind::PlayerCommand,
                crate::ecs::events::EventKind::Tick,
            ]
            .into(),
            limits: PluginLimits::default(),
        };
        let mut manifest = manifest;
        if extra.iter().any(|(name, _)| *name == "on-player-admission") {
            manifest
                .subscriptions
                .insert(crate::ecs::events::EventKind::PlayerJoin);
        }
        let mut bodies = vec![("on-player-command", command), ("on-tick", tick)];
        bodies.extend_from_slice(extra);
        let component = fixture::component(&engine, &bodies);
        let pending = PendingPlugin {
            store: store(&engine, &manifest.limits),
            component,
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
        let (outbound_tx, packets) = tokio::sync::mpsc::channel(512);
        let player = router
            .spawn_player(PlayerSpawnData {
                session_id: 42,
                runtime_id: runtime,
                display_name: "Player".into(),
                xuid: None,
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
        harness.drain();
        harness
    }
    /// Read the actual authoritative world rather than a component snapshot substitute.
    fn world(&self) -> &World {
        self.router.world(0).unwrap().ecs.world()
    }
    /// Trusted fixture arrangement may change native state before a fresh scoped callback.
    fn world_mut(&mut self) -> &mut World {
        self.router.world_mut(0).unwrap().ecs.world_mut()
    }
    /// Remove initial traffic before inspecting terminal packets or reserving real capacity.
    fn drain(&mut self) -> Vec<McpePacket> {
        let mut packets = Vec::new();
        while let Ok(packet) = self.packets.try_recv() {
            packets.push(packet);
        }
        packets
    }
    /// Dispatch an actual encoded command request through the ordinary synchronous boundary.
    async fn command(&mut self) {
        self.command_from(42).await;
    }
    /// Source routing is authoritative even when multiple accounts share the same component.
    async fn command_from(&mut self, session: u64) {
        let packet = McpePacket::from(CommandRequestPacket {
            command: "/notice".into(),
            origin: types::CommandOriginDatajson {
                type_: "player".into(),
                request_id: "opaque-42".into(),
                ..Default::default()
            },
            is_internal: false,
            version: "latest".into(),
        });
        let mut bytes = bytes::BytesMut::new();
        packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
        self.router.route_packet(
            session,
            McpePacketArgs,
            BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap(),
        );
        self.router.tick().await.unwrap();
    }
    /// Logs come from actual canonical imports, not a simulated guest dispatcher.
    fn logs(&self) -> Vec<String> {
        self.manager.lock().unwrap().plugins[&PluginId("example.connection".into())]
            .store
            .data()
            .test_logs
            .clone()
    }
}
/// Materialize bounded guest display text through canonical memory, never host commands.
fn string(address: usize, value: &str) -> String {
    value
        .bytes()
        .enumerate()
        .map(|(i, b)| format!("i32.const {} i32.const {b} i32.store8 ", address + i))
        .collect()
}
/// A successful import returns an owned ticket in Staged state, which can be retained or dropped.
fn close_command(drop_ticket: bool, trap: bool) -> String {
    format!(
        "{} local.get 1 i32.const 1 i32.const 3100 i32.const 7 i32.const 128 call @[method]player.disconnect@ i32.const 128 i32.load8_u if unreachable end i32.const 512 i32.const 132 i32.load i32.store i32.const 516 i32.const 1 i32.store i32.const 132 i32.load i32.const 160 call @[method]connection-close-request.status@ i32.const 160 i32.load8_u if unreachable end i32.const 161 i32.load8_u if unreachable end {} {} i32.const 0",
        string(3100, "Example"),
        if drop_ticket {
            "i32.const 512 i32.load call @connection-close-request_drop@ i32.const 516 i32.const 0 i32.store"
        } else {
            ""
        },
        if trap { "unreachable" } else { "" }
    )
}
/// Owned ticket status remains queryable from global callbacks without a live player.
fn status_tick() -> String {
    "i32.const 516 i32.load if i32.const 512 i32.load i32.const 160 call @[method]connection-close-request.status@ i32.const 160 i32.load8_u if unreachable end i32.const 3000 i32.const 161 i32.load8_u i32.const 48 i32.add i32.store8 i32.const 0 i32.const 3000 i32.const 1 call @log@ end".into()
}
/// Assert the actual terminal packet rather than treating ticket staging as client acknowledgement.
fn terminal(packets: Vec<McpePacket>) -> DisconnectPacket {
    let mut packets = packets.into_iter().filter_map(|packet| match packet.data {
        McpePacketData::DisconnectPacket(packet) => Some(packet),
        _ => None,
    });
    let packet = packets.next().expect("terminal packet queued");
    assert!(packets.next().is_none());
    packet
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_stages_then_queues_and_retains_honest_delivery_status() {
    let mut h = Harness::new(close_command(false, false), status_tick(), true).await;
    h.command().await;
    assert_eq!(h.logs(), vec!["1"]);
    let packet = terminal(h.drain());
    assert_eq!(packet.reason, EnumsConnectionDisconnectFailReason::Kicked);
    let DisconnectPacketMessagesUnion::DisconnectPacketMessages(messages) = packet.messages else {
        panic!("missing display reason")
    };
    assert_eq!(&*messages.message, "Example");
    h.router.tick().await.unwrap();
    assert_eq!(h.logs(), vec!["1", "2"]);
    assert!(h.world().get::<PlayerSession>(h.player).is_some());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ticket_drop_cannot_cancel_a_successfully_staged_termination() {
    let mut h = Harness::new(close_command(true, false), String::new(), true).await;
    h.command().await;
    terminal(h.drain());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn callback_trap_discards_termination_and_releases_its_quota() {
    let mut h = Harness::new(close_command(false, true), String::new(), true).await;
    h.command().await;
    assert!(
        h.drain()
            .into_iter()
            .all(|packet| !matches!(packet.data, McpePacketData::DisconnectPacket(_)))
    );
    assert_eq!(
        h.manager.lock().unwrap().plugins[&PluginId("example.connection".into())]
            .store
            .data()
            .connection_close_count
            .load(std::sync::atomic::Ordering::Relaxed),
        0
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn termination_requires_its_separate_capability() {
    let body = format!(
        "{} local.get 1 i32.const 1 i32.const 3100 i32.const 7 i32.const 128 call @[method]player.disconnect@ i32.const 128 i32.load8_u i32.const 1 i32.ne if unreachable end i32.const 132 i32.load8_u if unreachable end i32.const 133 i32.load8_u i32.const 6 i32.ne if unreachable end i32.const 0",
        string(3100, "Example")
    );
    let mut h = Harness::new(body, String::new(), false).await;
    h.command().await;
    assert!(
        h.drain()
            .into_iter()
            .all(|packet| !matches!(packet.data, McpePacketData::DisconnectPacket(_)))
    );
    assert!(h.world().get_entity(h.player).is_ok());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backpressure_retries_on_the_same_authenticated_channel_after_ticket_drop() {
    let mut h = Harness::new(close_command(true, false), String::new(), true).await;
    let tx = h
        .world()
        .get::<PlayerSession>(h.player)
        .unwrap()
        .outbound_tx
        .clone();
    let permits = tx
        .try_reserve_many(tx.max_capacity())
        .unwrap()
        .collect::<Vec<_>>();
    h.command().await;
    assert_eq!(
        h.world()
            .resource::<super::super::connection_control::Queue>()
            .0
            .len(),
        1
    );
    assert_eq!(
        h.manager.lock().unwrap().plugins[&PluginId("example.connection".into())]
            .store
            .data()
            .connection_close_count
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    drop(permits);
    h.router.tick().await.unwrap();
    terminal(h.drain());
    assert_eq!(
        h.manager.lock().unwrap().plugins[&PluginId("example.connection".into())]
            .store
            .data()
            .connection_close_count
            .load(std::sync::atomic::Ordering::Relaxed),
        0
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_channel_and_component_unload_cannot_target_a_replacement() {
    for unload in [false, true] {
        let mut h = Harness::new(close_command(false, false), String::new(), true).await;
        let tx = h
            .world()
            .get::<PlayerSession>(h.player)
            .unwrap()
            .outbound_tx
            .clone();
        let permits = tx
            .try_reserve_many(tx.max_capacity())
            .unwrap()
            .collect::<Vec<_>>();
        h.command().await;
        let state = h
            .world()
            .resource::<super::super::connection_control::Queue>()
            .0[0]
            .state
            .clone();
        let (new_tx, mut new_rx) = tokio::sync::mpsc::channel(64);
        if unload {
            h.manager.lock().unwrap().plugins.clear();
        } else {
            let player = h.player;
            h.world_mut()
                .get_mut::<PlayerSession>(player)
                .unwrap()
                .outbound_tx = new_tx;
        }
        drop(permits);
        super::super::connection_control::publish(h.world_mut());
        assert!(matches!(
            state.status(),
            wit_types::ConnectionCloseStatus::Failed(_)
        ));
        assert!(new_rx.try_recv().is_err());
        assert!(
            h.drain()
                .into_iter()
                .all(|packet| !matches!(packet.data, McpePacketData::DisconnectPacket(_)))
        );
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deadline_is_bounded_and_never_fabricates_queued_delivery() {
    let mut h = Harness::new(close_command(false, false), String::new(), true).await;
    let tx = h
        .world()
        .get::<PlayerSession>(h.player)
        .unwrap()
        .outbound_tx
        .clone();
    let permits = tx
        .try_reserve_many(tx.max_capacity())
        .unwrap()
        .collect::<Vec<_>>();
    h.command().await;
    let state = h
        .world()
        .resource::<super::super::connection_control::Queue>()
        .0[0]
        .state
        .clone();
    super::super::connection_control::publish_at(
        h.world_mut(),
        Instant::now() + Duration::from_secs(11),
    );
    assert!(matches!(
        state.status(),
        wit_types::ConnectionCloseStatus::Failed(wit_types::ConnectionControlError::TimedOut)
    ));
    assert!(
        h.world()
            .resource::<super::super::connection_control::Queue>()
            .0
            .is_empty()
    );
    drop(permits);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_quota_survives_ticket_drop_and_repeated_callbacks() {
    let mut h = Harness::new(close_command(true, false), String::new(), true).await;
    h.manager
        .lock()
        .unwrap()
        .plugins
        .get_mut(&PluginId("example.connection".into()))
        .unwrap()
        .store
        .data_mut()
        .api_limits
        .max_pending_connection_requests = 1;
    let tx = h
        .world()
        .get::<PlayerSession>(h.player)
        .unwrap()
        .outbound_tx
        .clone();
    let permits = tx
        .try_reserve_many(tx.max_capacity())
        .unwrap()
        .collect::<Vec<_>>();
    h.command().await;
    let (other_tx, mut other_rx) = tokio::sync::mpsc::channel(512);
    h.router
        .spawn_player(PlayerSpawnData {
            session_id: 43,
            runtime_id: 43,
            display_name: "Other".into(),
            xuid: None,
            uuid: Some(uuid::Uuid::from_u128(43).to_string()),
            position: glam::DVec3::new(2.5, 17.62, 2.5),
            outbound_tx: other_tx,
            saved_data: None,
            chunk_radius: 0,
        })
        .unwrap();
    h.command_from(43).await;
    while let Ok(packet) = other_rx.try_recv() {
        assert!(!matches!(packet.data, McpePacketData::DisconnectPacket(_)));
    }
    assert_eq!(
        h.world()
            .resource::<super::super::connection_control::Queue>()
            .0
            .len(),
        1
    );
    assert_eq!(
        h.manager.lock().unwrap().plugins[&PluginId("example.connection".into())]
            .store
            .data()
            .connection_close_count
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    drop(permits);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connected_dead_and_recovery_fenced_actors_need_no_position_or_health_snapshot() {
    for recovering in [false, true] {
        let mut h = Harness::new(close_command(false, false), String::new(), true).await;
        let player = h.player;
        if recovering {
            let source =
                crate::server::game::inventory_effects::RecoverySource::capture(h.world(), player)
                    .unwrap();
            crate::server::game::inventory_effects::fence_for_recovery(h.world_mut(), &source)
                .unwrap();
        } else {
            h.world_mut().get_mut::<Health>(player).unwrap().current = 0.0;
        }
        // Invoke the actual canonical export in a fresh trusted callback: ordinary input
        // remains blocked while dead or fenced, but connection termination stays available.
        let manager = h.manager.clone();
        let mut manager = manager.lock().unwrap();
        manager.begin_tick(h.world_mut());
        let plugin = manager
            .plugins
            .get_mut(&PluginId("example.connection".into()))
            .unwrap();
        super::super::lifecycle::invoke(plugin, h.world_mut(), |store, bindings| {
            let tx = make_world_tx(store)?;
            let source = make_player_resource(store, player)?;
            bindings.call_on_player_command(store, tx, source, "/notice", None)
        })
        .unwrap();
        drop(manager);
        super::super::connection_control::publish(h.world_mut());
        terminal(h.drain());
        if recovering {
            assert!(crate::server::game::inventory_effects::is_recovery_fenced(
                h.world(),
                player
            ));
        }
    }
}

/// Native quit policy remains authoritative after a component requests transport termination.
struct Retain;
impl crate::server::game::player_events::PlayerEventHandler for Retain {
    fn on_quit(
        &self,
        _world: &mut World,
        event: &mut crate::server::game::player_events::PlayerQuit,
    ) {
        event.retain_entity();
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retained_actor_has_an_explicit_not_connected_error_in_a_fresh_scope() {
    use wit_types::HostPlayer;
    let mut h = Harness::new(String::from("i32.const 0"), String::new(), true).await;
    h.world_mut()
        .resource_mut::<crate::server::game::player_events::PlayerEventHooks>()
        .register(Retain);
    h.router.despawn_player(42);
    assert!(
        h.world()
            .get::<crate::server::game::player_events::DetachedPlayer>(h.player)
            .is_some()
    );
    let mut context = store(&engine(), &PluginLimits::default());
    context.data_mut().capabilities = [PluginCapability::ConnectionControl].into();
    let player = h.player;
    with_world(&mut context, h.world_mut(), |store| {
        let source = make_player_resource(store, player).unwrap();
        assert!(matches!(
            HostPlayer::disconnect(
                store.data_mut(),
                source,
                wit_types::ConnectionCloseReason {
                    kind: wit_types::ConnectionCloseKind::Kicked,
                    message: "Example".into()
                }
            ),
            Err(wit_types::ConnectionControlError::NotConnected)
        ));
    });
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn termination_reuses_expired_scope_guards_and_bounded_reason_validation() {
    use wit_types::{HostPlayer, HostWorldTx};
    let mut h = Harness::new(String::from("i32.const 0"), String::new(), true).await;
    let mut context = store(&engine(), &PluginLimits::default());
    context.data_mut().capabilities = [
        PluginCapability::ConnectionControl,
        PluginCapability::ReadPlayers,
    ]
    .into();
    let player = h.player;
    let live = with_world(&mut context, h.world_mut(), |store| {
        let ctx = store.data_mut();
        let source = ctx.borrow_player(player).unwrap();
        let handle = HostPlayer::to_handle(ctx, source).unwrap();
        let tx = ctx.borrow_transaction().unwrap();
        HostWorldTx::player(ctx, tx, handle).unwrap().rep()
    });
    with_world(&mut context, h.world_mut(), |store| {
        assert!(matches!(
            HostPlayer::disconnect(
                store.data_mut(),
                wasmtime::component::Resource::new_borrow(live),
                wit_types::ConnectionCloseReason {
                    kind: wit_types::ConnectionCloseKind::Kicked,
                    message: "Example".into()
                }
            ),
            Err(wit_types::ConnectionControlError::Access(
                wit_types::AccessError::ExpiredScope
            ))
        ));
    });
    for message in ["x".repeat(4097), String::from("bad\0reason")] {
        with_world(&mut context, h.world_mut(), |store| {
            let source = make_player_resource(store, player).unwrap();
            assert!(matches!(
                HostPlayer::disconnect(
                    store.data_mut(),
                    source,
                    wit_types::ConnectionCloseReason {
                        kind: wit_types::ConnectionCloseKind::Kicked,
                        message
                    }
                ),
                Err(wit_types::ConnectionControlError::Access(
                    wit_types::AccessError::InvalidArgument
                ))
            ));
        });
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_removal_or_world_closure_never_queues_a_replacement_termination() {
    for close_world in [false, true] {
        let mut h = Harness::new(close_command(false, false), String::new(), true).await;
        let tx = h
            .world()
            .get::<PlayerSession>(h.player)
            .unwrap()
            .outbound_tx
            .clone();
        let permits = tx
            .try_reserve_many(tx.max_capacity())
            .unwrap()
            .collect::<Vec<_>>();
        h.command().await;
        let state = h
            .world()
            .resource::<super::super::connection_control::Queue>()
            .0[0]
            .state
            .clone();
        drop(permits);
        if close_world {
            drop(h.router);
            assert!(matches!(
                state.status(),
                wit_types::ConnectionCloseStatus::Failed(
                    wit_types::ConnectionControlError::Access(wit_types::AccessError::WorldClosed)
                )
            ));
        } else {
            let player = h.player;
            h.world_mut().despawn(player);
            super::super::connection_control::publish(h.world_mut());
            assert!(matches!(
                state.status(),
                wit_types::ConnectionCloseStatus::Failed(
                    wit_types::ConnectionControlError::Access(wit_types::AccessError::StalePlayer)
                )
            ));
        }
    }
}

#[path = "connection_control/network.rs"]
mod network;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initial_admission_explicitly_uses_its_existing_reject_or_hold_path() {
    let body = format!(
        "{} local.get 1 i32.const 1 i32.const 3100 i32.const 7 i32.const 128 call @[method]player.disconnect@ i32.const 128 i32.load8_u i32.const 1 i32.ne if unreachable end i32.const 132 i32.load8_u i32.const 2 i32.ne if unreachable end i32.const 32 i32.const 0 i32.store i32.const 32",
        string(3100, "Example")
    );
    let h = Harness::with_exports(
        "i32.const 0".into(),
        String::new(),
        true,
        42,
        &[("on-player-admission", body)],
    )
    .await;
    assert!(h.world().get::<PlayerSession>(h.player).is_some());
    assert!(
        h.world()
            .get_resource::<super::super::connection_control::Queue>()
            .is_none()
    );
}
/// Controlled writes demonstrate that termination does not bypass inventory save gates.
struct PendingProvider {
    gate: tokio::sync::Semaphore,
    writes: std::sync::atomic::AtomicUsize,
}
#[async_trait::async_trait]
impl crate::storage::PlayerProvider for PendingProvider {
    fn atomic_durable_records(&self) -> bool {
        true
    }
    async fn load(
        &self,
        _uuid: uuid::Uuid,
    ) -> crate::storage::StorageResult<Option<crate::storage::PlayerData>> {
        Ok(None)
    }
    async fn save(
        &self,
        _uuid: uuid::Uuid,
        _data: &crate::storage::PlayerData,
    ) -> crate::storage::StorageResult<()> {
        self.gate.acquire().await.unwrap().forget();
        self.writes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }
    async fn delete(&self, _uuid: uuid::Uuid) -> crate::storage::StorageResult<()> {
        Ok(())
    }
    async fn close(&self) -> crate::storage::StorageResult<()> {
        Ok(())
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pending_durable_inventory_source_can_close_without_publishing_or_ordinary_saving_slots()
{
    use crate::server::game::inventory_effects as inventory;
    let mut h = Harness::new(close_command(false, false), String::new(), true).await;
    let player = h.player;
    let provider = Arc::new(PendingProvider {
        gate: tokio::sync::Semaphore::new(0),
        writes: Default::default(),
    });
    h.world_mut()
        .insert_resource(crate::server::game::types::PlayerProviderResource(
            provider.clone(),
        ));
    inventory::install(h.router.world_mut(0).unwrap()).unwrap();
    let expected = inventory::InventoryState::capture(h.world(), player).unwrap();
    inventory::submit(
        h.world_mut(),
        player,
        inventory::InventoryEffect {
            id: "example-close-pending".into(),
            replacement: expected.clone(),
            expected,
        },
    )
    .unwrap();
    assert!(inventory::is_quarantined(h.world(), player));
    {
        let manager = h.manager.clone();
        let mut manager = manager.lock().unwrap();
        manager.begin_tick(h.world_mut());
        let plugin = manager
            .plugins
            .get_mut(&PluginId("example.connection".into()))
            .unwrap();
        super::super::lifecycle::invoke(plugin, h.world_mut(), |store, bindings| {
            let tx = make_world_tx(store)?;
            let source = make_player_resource(store, player)?;
            bindings.call_on_player_command(store, tx, source, "/notice", None)
        })
        .unwrap();
    }
    super::super::connection_control::publish(h.world_mut());
    terminal(h.drain());
    assert!(inventory::is_quarantined(h.world(), player));
    assert!(
        !tokio::time::timeout(Duration::from_millis(100), h.router.save_player(42))
            .await
            .unwrap()
    );
    assert_eq!(
        provider.writes.load(std::sync::atomic::Ordering::Relaxed),
        0
    );
    h.world_mut()
        .resource_mut::<crate::server::game::player_events::PlayerEventHooks>()
        .register(Retain);
    assert!(h.router.prepare_disconnect(42));
    h.router.despawn_player(42);
    assert!(inventory::is_quarantined(h.world(), player));
    assert!(h.world().get::<PlayerSession>(player).is_none());
    provider.gate.add_permits(1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reconnected_same_account_and_session_number_never_inherits_the_old_request() {
    let mut h = Harness::new(close_command(false, false), String::new(), true).await;
    let tx = h
        .world()
        .get::<PlayerSession>(h.player)
        .unwrap()
        .outbound_tx
        .clone();
    let permits = tx
        .try_reserve_many(tx.max_capacity())
        .unwrap()
        .collect::<Vec<_>>();
    h.command().await;
    let state = h
        .world()
        .resource::<super::super::connection_control::Queue>()
        .0[0]
        .state
        .clone();
    h.router.despawn_player(42);
    let (new_tx, mut new_rx) = tokio::sync::mpsc::channel(512);
    let replacement = h
        .router
        .spawn_player(PlayerSpawnData {
            session_id: 42,
            runtime_id: 42,
            display_name: "Player".into(),
            xuid: None,
            uuid: Some(uuid::Uuid::from_u128(42).to_string()),
            position: glam::DVec3::new(0.5, 17.62, 0.5),
            outbound_tx: new_tx,
            saved_data: None,
            chunk_radius: 0,
        })
        .unwrap();
    assert_ne!(replacement, h.player);
    drop(permits);
    super::super::connection_control::publish(h.world_mut());
    assert!(matches!(
        state.status(),
        wit_types::ConnectionCloseStatus::Failed(wit_types::ConnectionControlError::Access(
            wit_types::AccessError::StalePlayer
        ))
    ));
    while let Ok(packet) = new_rx.try_recv() {
        assert!(!matches!(packet.data, McpePacketData::DisconnectPacket(_)));
    }
    assert!(h.world().get::<PlayerSession>(replacement).is_some());
}
