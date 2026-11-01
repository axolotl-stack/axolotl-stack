//! Actual components control real native joins, initial publication and actor teardown.

use super::*;
use crate::ecs::events::EventKind;
use crate::entity::components::{Health, PlayerSession};
use crate::plugin::manifest::PluginCapability;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
use crate::server::worlds::DimensionRouter;
use std::sync::{Arc, Mutex};

struct Harness {
    router: DimensionRouter,
    manager: Arc<Mutex<PluginManager>>,
}
impl Harness {
    /// Install a real canonical component before the first complete native join.
    fn new(bodies: &[(&str, String)], required: bool) -> Self {
        let mut router = DimensionRouter::new(GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        }));
        let engine = engine();
        let manifest = PluginManifest {
            id: PluginId("example.session".into()),
            name: "Sessions".into(),
            version: "1".into(),
            api_version: PLUGIN_API_VERSION.into(),
            required,
            config: None,
            capabilities: [
                PluginCapability::ReadPlayers,
                PluginCapability::SendMessages,
                PluginCapability::ScheduleTasks,
            ]
            .into(),
            subscriptions: [
                EventKind::PlayerJoin,
                EventKind::PlayerQuit,
                EventKind::PlayerAdmitted,
            ]
            .into(),
            limits: PluginLimits::default(),
        };
        let pending = PendingPlugin {
            component: fixture::component(&engine, bodies),
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
        }));
        PluginManager::attach_to_world(
            manager.clone(),
            router.world_mut(0).unwrap().ecs.world_mut(),
        );
        router.set_plugin_manager(manager.clone());
        router.initialize_plugins().unwrap();
        Self { router, manager }
    }
    /// The caller owns actual packet delivery; no fake player or queued join is constructed.
    fn join(
        &mut self,
    ) -> (
        Entity,
        tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>,
    ) {
        let (outbound_tx, packets) = tokio::sync::mpsc::channel(1024);
        let player = self
            .router
            .spawn_player(PlayerSpawnData {
                session_id: 42,
                runtime_id: 42,
                display_name: "Player".into(),
                xuid: Some("42".into()),
                uuid: Some(uuid::Uuid::from_u128(42).to_string()),
                position: glam::DVec3::new(0.5, 65.62, 0.5),
                outbound_tx,
                saved_data: None,
                chunk_radius: 0,
            })
            .unwrap();
        (player, packets)
    }
    /// Read actual guest log imports without invoking an extra callback or renewing fuel.
    fn logs(&self) -> Vec<String> {
        std::mem::take(
            &mut self
                .manager
                .lock()
                .unwrap()
                .plugins
                .values_mut()
                .next()
                .unwrap()
                .store
                .data_mut()
                .test_logs,
        )
    }
}
/// Write immutable literal data through the real guest linear memory.
fn text(at: u32, text: &str) -> String {
    text.bytes()
        .enumerate()
        .map(|(index, byte)| {
            format!(
                "i32.const {} i32.const {byte} i32.store8 ",
                at + index as u32
            )
        })
        .collect()
}
/// Observe a callback using the canonical host log import.
fn witness(byte: u8) -> String {
    format!(
        "i32.const 2048 i32.const {byte} i32.store8 i32.const 0 i32.const 2048 i32.const 1 call @log@"
    )
}
/// Return a typed canonical Allow proposal.
fn allow() -> &'static str {
    "i32.const 32 i32.const 0 i32.store i32.const 32"
}
/// Create and retain an actual imported hold; its representation stays in guest memory.
fn hold() -> &'static str {
    "local.get 1 i32.const 64 call @[method]player.defer-admission@ i32.const 64 i32.load8_u if unreachable end i32.const 512 i32.const 68 i32.load i32.store"
}
/// Return typed Deferred after retaining the owned token.
fn deferred() -> String {
    format!("{} i32.const 32 i32.const 2 i32.store i32.const 32", hold())
}

#[tokio::test]
async fn component_admission_deny_sends_exact_rejection_without_any_initial_state() {
    let reason = "Please retry later.\nInitialization is unavailable.";
    let mut harness = Harness::new(
        &[(
            "on-player-admission",
            format!(
                "{} i32.const 32 i32.const 1 i32.store i32.const 36 i32.const 1024 i32.store i32.const 40 i32.const {} i32.store i32.const 32",
                text(1024, reason),
                reason.len()
            ),
        )],
        true,
    );
    let (player, mut packets) = harness.join();
    assert!(
        harness
            .router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get_entity(player)
            .is_err()
    );
    let packet = packets.try_recv().unwrap();
    let jolyne::valentine::McpePacketData::DisconnectPacket(packet) = packet.data else {
        panic!("only rejection is delivered");
    };
    let jolyne::valentine::DisconnectPacketMessagesUnion::DisconnectPacketMessages(messages) =
        packet.messages
    else {
        panic!("rejection text");
    };
    assert_eq!(messages.message.to_string(), reason);
    assert!(packets.try_recv().is_err());
    assert!(harness.manager.lock().unwrap().check_required().is_ok());
}

#[tokio::test]
async fn component_admitted_notification_follows_the_actual_initial_batch_once() {
    let mut harness = Harness::new(
        &[
            (
                "on-player-admission",
                format!("{} {}", witness(b'P'), allow()),
            ),
            ("on-player-admitted", witness(b'A')),
        ],
        true,
    );
    let (player, mut packets) = harness.join();
    assert_eq!(harness.logs(), ["P"]);
    assert!(packets.try_recv().is_err());
    harness.router.tick().await.unwrap();
    assert_eq!(harness.logs(), ["A"]);
    assert!(!crate::server::game::player_events::initial_state_pending(
        harness.router.world(0).unwrap().ecs.world(),
        player
    ));
    assert!(
        std::iter::from_fn(|| packets.try_recv().ok()).any(|packet| matches!(
            packet.data,
            jolyne::valentine::McpePacketData::InventoryContentPacket(_)
        ))
    );
    harness.router.tick().await.unwrap();
    assert!(harness.logs().is_empty());
}

#[tokio::test]
async fn component_deferred_hold_blocks_legacy_early_release_until_explicit_acknowledgement() {
    let mut harness = Harness::new(
        &[
            ("on-player-admission", deferred()),
            ("on-player-admitted", witness(b'A')),
        ],
        true,
    );
    let (player, mut packets) = harness.join();
    assert!(crate::server::game::player_events::release_initial_state(
        harness.router.world_mut(0).unwrap().ecs.world_mut(),
        player
    ));
    harness.router.tick().await.unwrap();
    assert!(packets.try_recv().is_err());
    assert!(harness.logs().is_empty());
    assert!(crate::server::game::player_events::initial_state_pending(
        harness.router.world(0).unwrap().ecs.world(),
        player
    ));
}

#[tokio::test]
async fn component_quit_retains_the_original_actor_without_a_network_session() {
    let mut harness = Harness::new(
        &[("on-player-quit", format!("{} i32.const 1", witness(b'Q')))],
        true,
    );
    let (player, _packets) = harness.join();
    harness.router.tick().await.unwrap();
    harness.router.despawn_player(42);
    let world = harness.router.world(0).unwrap().ecs.world();
    assert!(world.get::<PlayerSession>(player).is_none());
    assert!(
        world
            .get::<crate::server::game::player_events::DetachedPlayer>(player)
            .is_some()
    );
    assert!(world.get::<Health>(player).is_some());
    assert_eq!(harness.logs(), ["Q"]);
    harness.router.tick().await.unwrap();
    assert!(harness.logs().is_empty());
}

/// Initialization work schedules a fresh same-session transaction rather than reentering.
fn schedule_initialization() -> &'static str {
    r#"
    local.get 1 i32.const 160 call @[method]player.to-handle@
    i32.const 160 i32.load8_u if unreachable end
    i32.const 164 i32.load i64.const 9 i32.const 176 call @[method]player-handle.schedule@
    i32.const 176 i32.load8_u if unreachable end
    i32.const 180 i32.load call @task_drop@
    i32.const 164 i32.load call @player-handle_drop@
    "#
}

#[tokio::test]
async fn component_initialization_task_can_ack_while_initializing_and_drop_the_staged_hold() {
    let admission = format!(
        "{} {} i32.const 32 i32.const 2 i32.store i32.const 32",
        hold(),
        schedule_initialization()
    );
    let task = format!(
        r#"
        local.get 1 i32.const 96 call @[method]player.availability@
        i32.const 96 i32.load8_u if unreachable end
        i32.const 97 i32.load8_u i32.const 1 i32.ne if unreachable end
        local.get 1 i32.const 112 call @[method]player.initial-state-ready@
        i32.const 112 i32.load8_u if unreachable end
        i32.const 113 i32.load8_u i32.eqz if unreachable end
        i32.const 512 i32.load local.get 0 i32.const 0 i32.const 0 i32.const 0 i32.const 128 call @[method]admission-hold.resolve@
        i32.const 128 i32.load8_u if unreachable end
        i32.const 129 i32.load8_u if unreachable end
        i32.const 512 i32.load call @admission-hold_drop@
        {}
    "#,
        witness(b'I')
    );
    let mut harness = Harness::new(
        &[
            ("on-player-admission", admission),
            ("on-player-task", task),
            ("on-player-admitted", witness(b'A')),
        ],
        true,
    );
    let (_player, mut packets) = harness.join();
    assert!(packets.try_recv().is_err());
    harness.router.tick().await.unwrap();
    assert_eq!(harness.logs(), ["I"]);
    assert!(
        packets.try_recv().is_err(),
        "staged acknowledgement is not a publication notification"
    );
    harness.router.tick().await.unwrap();
    assert_eq!(harness.logs(), ["A"]);
    assert!(packets.try_recv().is_ok());
}

#[tokio::test]
async fn component_dropping_a_deferred_hold_rejects_without_claiming_admission() {
    let admission = format!(
        "{} {} i32.const 32 i32.const 2 i32.store i32.const 32",
        hold(),
        schedule_initialization()
    );
    let mut harness = Harness::new(
        &[
            ("on-player-admission", admission),
            (
                "on-player-task",
                "i32.const 512 i32.load call @admission-hold_drop@".into(),
            ),
            ("on-player-admitted", witness(b'A')),
        ],
        true,
    );
    let (player, mut packets) = harness.join();
    harness.router.tick().await.unwrap();
    assert!(packets.try_recv().is_err());
    harness.router.tick().await.unwrap();
    assert!(
        harness
            .router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get_entity(player)
            .is_err()
    );
    let packets: Vec<_> = std::iter::from_fn(|| packets.try_recv().ok()).collect();
    assert_eq!(packets.len(), 1);
    assert!(matches!(
        packets[0].data,
        jolyne::valentine::McpePacketData::DisconnectPacket(_)
    ));
    assert!(harness.logs().is_empty());
    assert!(harness.manager.lock().unwrap().check_required().is_ok());
}

#[tokio::test]
async fn component_default_quit_removes_actor_and_legacy_queued_quit_is_suppressed() {
    let mut harness = Harness::new(
        &[("on-player-quit", format!("{} i32.const 0", witness(b'Q')))],
        true,
    );
    let (player, _packets) = harness.join();
    harness.router.tick().await.unwrap();
    harness.router.despawn_player(42);
    assert!(
        harness
            .router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get_entity(player)
            .is_err()
    );
    harness
        .router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .resource_mut::<crate::ecs::events::EventBuffer>()
        .push(ServerEvent::PlayerQuit { entity: player });
    harness.router.tick().await.unwrap();
    assert_eq!(harness.logs(), ["Q"]);
    assert!(harness.manager.lock().unwrap().check_required().is_ok());
}

#[tokio::test]
async fn component_required_admission_trap_is_fatal_without_initial_publication() {
    let mut harness = Harness::new(
        &[("on-player-admission", format!("{} unreachable", hold()))],
        true,
    );
    let (_player, mut packets) = harness.join();
    let packets: Vec<_> = std::iter::from_fn(|| packets.try_recv().ok()).collect();
    assert!(!packets.is_empty());
    assert!(packets.iter().all(|packet| matches!(
        packet.data,
        jolyne::valentine::McpePacketData::DisconnectPacket(_)
    )));
    assert!(harness.router.tick().await.is_err());
    assert!(harness.manager.lock().unwrap().check_required().is_err());
}

#[tokio::test]
async fn component_required_quit_trap_fences_before_native_save_or_teardown() {
    use crate::storage::PlayerProvider;
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(crate::storage::LevelDBPlayerProvider::open(directory.path()).unwrap());
    let mut harness = Harness::new(&[("on-player-quit", "unreachable".into())], true);
    harness
        .router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .insert_resource(crate::server::game::types::PlayerProviderResource(
            provider.clone(),
        ));
    let (player, _packets) = harness.join();
    harness.router.tick().await.unwrap();
    assert!(harness.router.save_player(42).await);
    harness
        .router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .get_mut::<Health>(player)
        .unwrap()
        .current = 11.0;
    assert!(harness.router.prepare_disconnect(42));
    assert!(harness.router.check_plugins().is_err());
    assert!(!harness.router.save_player(42).await);
    let world = harness.router.world(0).unwrap().ecs.world();
    assert!(crate::server::game::inventory_effects::is_recovery_fenced(
        world, player
    ));
    assert!(crate::server::game::player_state::capture(world, player).is_err());
    assert!(world.get::<PlayerSession>(player).is_some());
    assert_eq!(
        provider
            .load(uuid::Uuid::from_u128(42))
            .await
            .unwrap()
            .unwrap()
            .health,
        20.0
    );
}

#[tokio::test]
async fn component_unresolved_admission_cannot_overwrite_the_native_account_record() {
    use crate::storage::PlayerProvider;
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(crate::storage::LevelDBPlayerProvider::open(directory.path()).unwrap());
    let mut harness = Harness::new(&[("on-player-admission", deferred())], true);
    harness
        .router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .insert_resource(crate::server::game::types::PlayerProviderResource(
            provider.clone(),
        ));
    let (player, _packets) = harness.join();
    assert!(!harness.router.save_player(42).await);
    assert!(harness.router.prepare_disconnect(42));
    assert!(harness.router.check_plugins().is_ok());
    assert!(
        !harness.router.save_player(42).await,
        "departure cannot turn unresolved initialization into an ordinary save"
    );
    harness.router.despawn_player(42);
    assert!(
        harness
            .router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get_entity(player)
            .is_err()
    );
    assert!(
        provider
            .load(uuid::Uuid::from_u128(42))
            .await
            .unwrap()
            .is_none()
    );
}

/// Native observer simulates a replacement channel after component admission has returned.
struct ReplaceChannel(Arc<Mutex<Option<tokio::sync::mpsc::Sender<jolyne::valentine::McpePacket>>>>);
impl crate::server::game::player_events::PlayerEventHandler for ReplaceChannel {
    fn on_joined(
        &self,
        world: &mut bevy_ecs::world::World,
        event: &crate::server::game::player_events::PlayerJoined,
    ) {
        world
            .get_mut::<PlayerSession>(event.player())
            .unwrap()
            .outbound_tx = self.0.lock().unwrap().take().unwrap();
    }
}

#[tokio::test]
async fn component_late_replacement_never_receives_stale_initial_state_or_player_task() {
    let admission = format!(
        "{} {} i32.const 32 i32.const 2 i32.store i32.const 32",
        hold(),
        schedule_initialization()
    );
    let mut harness = Harness::new(
        &[
            ("on-player-admission", admission),
            ("on-player-task", "unreachable".into()),
            ("on-task-failed", witness(b'F')),
            ("on-player-admitted", witness(b'A')),
        ],
        true,
    );
    let (replacement, mut replacement_packets) = tokio::sync::mpsc::channel(1024);
    harness
        .router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .resource_mut::<crate::server::game::player_events::PlayerEventHooks>()
        .register(ReplaceChannel(Arc::new(Mutex::new(Some(replacement)))));
    let (_player, mut original_packets) = harness.join();
    harness.router.tick().await.unwrap();
    assert!(replacement_packets.try_recv().is_err());
    assert!(original_packets.try_recv().is_err());
    assert_eq!(harness.logs(), ["F"]);
    assert!(harness.manager.lock().unwrap().check_required().is_ok());
}

#[tokio::test]
async fn component_admission_waits_for_whole_initial_batch_capacity() {
    let mut harness = Harness::new(&[("on-player-admitted", witness(b'A'))], true);
    let (player, mut packets) = harness.join();
    let sender = harness
        .router
        .world(0)
        .unwrap()
        .ecs
        .world()
        .get::<PlayerSession>(player)
        .unwrap()
        .outbound_tx
        .clone();
    for _ in 0..sender.capacity() {
        sender
            .try_send(jolyne::valentine::McpePacket::from(
                jolyne::valentine::SetCommandsEnabledPacket {
                    commands_enabled: false,
                },
            ))
            .unwrap();
    }
    harness.router.tick().await.unwrap();
    assert!(harness.logs().is_empty());
    let queued: Vec<_> = std::iter::from_fn(|| packets.try_recv().ok()).collect();
    assert!(queued.iter().all(|packet| matches!(
        packet.data,
        jolyne::valentine::McpePacketData::SetCommandsEnabledPacket(_)
    )));
    assert!(crate::server::game::player_events::initial_state_pending(
        harness.router.world(0).unwrap().ecs.world(),
        player
    ));
    harness.router.tick().await.unwrap();
    assert_eq!(harness.logs(), ["A"]);
    assert!(
        std::iter::from_fn(|| packets.try_recv().ok()).any(|packet| matches!(
            packet.data,
            jolyne::valentine::McpePacketData::InventoryContentPacket(_)
        ))
    );
}

/// Compute owned callback layouts from the same WIT used by the actual canonical encoder.
fn quit_offsets() -> (usize, usize, usize, usize, usize, usize) {
    use wit_parser::{Int, Resolve, SizeAlign, Type, TypeDefKind, WorldItem};
    let mut resolve = Resolve::default();
    let (package, _) = resolve
        .push_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../unastar-api/wit"))
        .unwrap();
    let world = resolve
        .select_world(&[package], Some("unastar-plugin"))
        .unwrap();
    let function = resolve.worlds[world]
        .exports
        .iter()
        .find_map(|(_, item)| match item {
            WorldItem::Function(function) if function.name == "on-player-quit" => Some(function),
            _ => None,
        })
        .unwrap();
    let mut sizes = SizeAlign::default();
    sizes.fill(&resolve);
    let event_at = sizes.field_offsets(function.params.iter().map(|param| &param.ty))[1]
        .0
        .size_wasm32();
    let record = |name: &str| {
        resolve
            .types
            .iter()
            .find_map(|(_, ty)| match &ty.kind {
                TypeDefKind::Record(record) if ty.name.as_deref() == Some(name) => Some(record),
                _ => None,
            })
            .unwrap()
    };
    let field = |name: &str, wanted: &str| {
        let record = record(name);
        let offsets = sizes.field_offsets(record.fields.iter().map(|field| &field.ty));
        let index = record
            .fields
            .iter()
            .position(|field| field.name == wanted)
            .unwrap();
        (offsets[index].0.size_wasm32(), record.fields[index].ty)
    };
    let (observation, Type::Id(result)) = field("quit-event", "observation") else {
        panic!("typed observation result")
    };
    let TypeDefKind::Result(result) = &resolve.types[result].kind else {
        panic!("observation result")
    };
    let health = event_at
        + observation
        + sizes
            .payload_offset(Int::U8, [result.ok.as_ref(), result.err.as_ref()])
            .size_wasm32()
        + field("player-snapshot", "health").0;
    let reason_at = event_at + field("quit-event", "reason").0;
    let Type::Id(reason_type) = field("quit-event", "reason").1 else {
        panic!("typed reason")
    };
    let TypeDefKind::Variant(reason) = &resolve.types[reason_type].kind else {
        panic!("reason variant")
    };
    let detail_at = reason_at
        + sizes
            .payload_offset(
                reason.tag(),
                reason.cases.iter().map(|case| case.ty.as_ref()),
            )
            .size_wasm32();
    let code_at = detail_at + field("disconnect-detail", "protocol-code").0;
    (
        event_at + observation,
        health,
        event_at + field("quit-event", "previously-admitted").0,
        reason_at,
        code_at,
        detail_at + field("disconnect-detail", "message").0,
    )
}

#[tokio::test]
async fn component_quit_owns_exact_cause_and_health_after_original_actor_is_removed() {
    let (observation, health, admitted, reason, code, message) = quit_offsets();
    let quit = format!(
        r#"
        local.get 0 i32.load8_u offset={observation} if unreachable end
        local.get 0 f32.load offset={health} f32.const 11 f32.ne if unreachable end
        local.get 0 i32.load8_u offset={admitted} i32.eqz if unreachable end
        local.get 0 i32.load8_u offset={reason} i32.const 2 i32.ne if unreachable end
        local.get 0 i32.load8_u offset={code} i32.const 1 i32.ne if unreachable end
        local.get 0 i32.load offset={} i32.const -7 i32.ne if unreachable end
        local.get 0 i32.load8_u offset={message} i32.const 1 i32.ne if unreachable end
        local.get 0 i32.load offset={} i32.load8_u i32.const 108 i32.ne if unreachable end
        i32.const 3000 local.get 0 f32.load offset={health} f32.store
        {} i32.const 0
    "#,
        code + 4,
        message + 4,
        witness(b'Q')
    );
    let tick = format!(
        "i32.const 3000 f32.load f32.const 11 f32.ne if unreachable end {}",
        witness(b'O')
    );
    let mut harness = Harness::new(&[("on-player-quit", quit), ("on-tick", tick)], true);
    let (player, _packets) = harness.join();
    harness.router.tick().await.unwrap();
    harness
        .manager
        .lock()
        .unwrap()
        .plugins
        .values_mut()
        .next()
        .unwrap()
        .manifest
        .subscriptions
        .insert(EventKind::Tick);
    let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
    world.get_mut::<Health>(player).unwrap().current = 11.0;
    world
        .entity_mut(player)
        .insert(crate::network::departure::QuitCause::PeerDisconnect {
            reason: jolyne::valentine::EnumsConnectionDisconnectFailReason::UnknownValue(-7),
            message: Some("left".into()),
        });
    harness.router.despawn_player(42);
    assert!(
        harness
            .router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get_entity(player)
            .is_err()
    );
    assert_eq!(harness.logs(), ["Q"]);
    harness.router.tick().await.unwrap();
    assert_eq!(harness.logs(), ["O"]);
}

#[tokio::test]
async fn component_acknowledgement_cannot_release_another_components_hold() {
    let first = format!(
        "{} {} i32.const 32 i32.const 2 i32.store i32.const 32",
        hold(),
        schedule_initialization()
    );
    let resolve = "i32.const 512 i32.load local.get 0 i32.const 0 i32.const 0 i32.const 0 i32.const 128 call @[method]admission-hold.resolve@ i32.const 128 i32.load8_u if unreachable end i32.const 512 i32.load call @admission-hold_drop@";
    let mut harness = Harness::new(
        &[
            ("on-player-admission", first),
            ("on-player-task", resolve.into()),
        ],
        true,
    );
    {
        let mut manager = harness.manager.lock().unwrap();
        let manifest = PluginManifest {
            id: PluginId("example.other".into()),
            name: "Other".into(),
            version: "1".into(),
            api_version: PLUGIN_API_VERSION.into(),
            required: true,
            config: None,
            capabilities: [PluginCapability::ReadPlayers].into(),
            subscriptions: [EventKind::PlayerJoin].into(),
            limits: PluginLimits::default(),
        };
        let pending = PendingPlugin {
            component: fixture::component(&manager.engine, &[("on-player-admission", deferred())]),
            store: store(&manager.engine, &manifest.limits),
            manifest,
        };
        manager.pending.push(pending);
    }
    harness.router.initialize_plugins().unwrap();
    let (player, mut packets) = harness.join();
    crate::server::game::player_events::release_initial_state(
        harness.router.world_mut(0).unwrap().ecs.world_mut(),
        player,
    );
    harness.router.tick().await.unwrap();
    harness.router.tick().await.unwrap();
    assert!(packets.try_recv().is_err());
    assert!(crate::server::game::player_events::initial_state_pending(
        harness.router.world(0).unwrap().ecs.world(),
        player
    ));
    assert!(harness.manager.lock().unwrap().check_required().is_ok());
}

#[tokio::test]
async fn component_quit_storage_error_then_trap_fences_before_native_player_save() {
    use crate::plugin::storage::{Limits, Registry};
    use crate::storage::PlayerProvider;
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(
        crate::storage::LevelDBPlayerProvider::open(directory.path().join("players")).unwrap(),
    );
    let registry = Arc::new(Mutex::new(Registry::new(
        directory.path().join("components"),
        Limits {
            value_bytes: 1,
            ..Default::default()
        },
    )));
    let quit = format!(
        r#"
        {} {}
        i32.const 1024 i32.const 7 i32.const 0 i64.const 0 i32.const 1040 i32.const 2 i32.const 128 call @compare-exchange@
        i32.const 128 i32.load8_u i32.const 1 i32.ne if unreachable end
        i32.const 136 i32.load8_u i32.const 1 i32.ne if unreachable end
        {} unreachable
    "#,
        text(1024, "receipt"),
        text(1040, "ab"),
        witness(b'E')
    );
    let mut harness = Harness::new(&[("on-player-quit", quit)], true);
    {
        let mut manager = harness.manager.lock().unwrap();
        let context = manager
            .plugins
            .values_mut()
            .next()
            .unwrap()
            .store
            .data_mut();
        context.storage_registry = registry.clone();
        context.storage_namespace = None;
        assert!(!context.capabilities.contains(&PluginCapability::Filesystem));
    }
    harness
        .router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .insert_resource(crate::server::game::types::PlayerProviderResource(
            provider.clone(),
        ));
    let (player, _packets) = harness.join();
    harness.router.tick().await.unwrap();
    assert!(harness.router.save_player(42).await);
    harness
        .router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .get_mut::<Health>(player)
        .unwrap()
        .current = 11.0;
    assert!(harness.router.prepare_disconnect(42));
    assert_eq!(
        harness.logs(),
        ["E"],
        "the real imported CAS must report QuotaExceeded before the guest traps"
    );
    assert!(harness.router.check_plugins().is_err());
    assert!(!harness.router.save_player(42).await);
    assert!(crate::server::game::inventory_effects::is_recovery_fenced(
        harness.router.world(0).unwrap().ecs.world(),
        player
    ));
    assert_eq!(
        provider
            .load(uuid::Uuid::from_u128(42))
            .await
            .unwrap()
            .unwrap()
            .health,
        20.0
    );
    assert!(
        registry
            .lock()
            .unwrap()
            .namespace("example.session")
            .unwrap()
            .lock()
            .unwrap()
            .read("receipt")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn owned_initialization_plan_can_commit_while_ordinary_saves_and_publication_wait() {
    use crate::entity::components::{GameMode, MainInventory};
    use crate::server::game::inventory_effects::{self, InventoryEffect, InventoryState};
    use crate::storage::PlayerProvider;
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(crate::storage::LevelDBPlayerProvider::open(directory.path()).unwrap());
    let admission = format!(
        "{} {} i32.const 32 i32.const 2 i32.store i32.const 32",
        hold(),
        schedule_initialization()
    );
    let task = "i32.const 512 i32.load local.get 0 i32.const 0 i32.const 0 i32.const 0 i32.const 128 call @[method]admission-hold.resolve@ i32.const 128 i32.load8_u if unreachable end i32.const 512 i32.load call @admission-hold_drop@";
    let mut harness = Harness::new(
        &[
            ("on-player-admission", admission),
            ("on-player-task", task.into()),
            ("on-player-admitted", witness(b'A')),
        ],
        true,
    );
    let server = harness.router.world_mut(0).unwrap();
    server
        .ecs
        .world_mut()
        .insert_resource(crate::server::game::types::PlayerProviderResource(
            provider.clone(),
        ));
    inventory_effects::install(server).unwrap();
    let (player, mut packets) = harness.join();
    {
        let mut manager = harness.manager.lock().unwrap();
        let context = &mut manager.plugins.values_mut().next().unwrap().store;
        let hold = context
            .data()
            .test_admission_hold()
            .expect("actual component-created owned hold");
        let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
        with_world(context, world, |store| {
            let tx = store.data_mut().borrow_transaction().unwrap();
            let player = store.data_mut().borrow_player(player).unwrap();
            let entity = store.data_mut().live_player(player.rep()).unwrap().1;
            store
                .data_mut()
                .initialization_owner(tx.rep(), entity, hold)
                .unwrap();
        });
    }
    assert!(!harness.router.save_player(42).await);
    let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
    crate::command::exec::set_game_mode(world, player, GameMode::Survival);
    assert!(crate::command::exec::set_mode_override(
        world,
        player,
        Some(GameMode::Creative)
    ));
    let expected = InventoryState::capture(world, player).unwrap();
    let mut replacement = expected.clone();
    replacement.main[0] = (&crate::item::test_support::stack("minecraft:stone", 7)).into();
    inventory_effects::submit(
        world,
        player,
        InventoryEffect {
            id: "initialize".into(),
            expected,
            replacement,
        },
    )
    .unwrap();
    assert!(inventory_effects::is_quarantined(world, player));
    assert!(!harness.router.save_player(42).await);
    assert!(packets.try_recv().is_err());
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let world = harness.router.world_mut(0).unwrap().ecs.world_mut();
            inventory_effects::poll(world);
            if inventory_effects::receipt(world, player, "initialize").is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        packets.try_recv().is_err(),
        "durable initialization does not bypass admission holds"
    );
    assert!(!harness.router.save_player(42).await);
    let persisted = provider
        .load(uuid::Uuid::from_u128(42))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.inventory[0].count, 7);
    assert_eq!(
        persisted.game_mode, 0,
        "explicit initialization preserves the durable base mode"
    );
    harness.router.tick().await.unwrap();
    assert!(packets.try_recv().is_err());
    harness.router.tick().await.unwrap();
    assert_eq!(harness.logs(), ["A"]);
    assert_eq!(
        harness
            .router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .count,
        7
    );
}
