//! Real canonical components authorize actual encoded player movement inputs.

use super::*;
use crate::ecs::events::EventKind;
use crate::entity::components::{PlayerSession, Position};
use crate::server::game::player_events::PlayerMove;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
use crate::server::worlds::DimensionRouter;
use std::sync::{Arc, Mutex};

struct Harness {
    router: DimensionRouter,
    manager: Arc<Mutex<PluginManager>>,
    player: Entity,
    packets: tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>,
}
impl Harness {
    /// Initialize the current canonical ABI before creating a real native player.
    fn new(bodies: &[(&str, String)]) -> Self {
        let mut router = DimensionRouter::new(GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        }));
        let engine = engine();
        let manifest = PluginManifest {
            id: PluginId("example.movement".into()),
            name: "Movement".into(),
            version: "1".into(),
            api_version: PLUGIN_API_VERSION.into(),
            required: true,
            config: None,
            capabilities: [crate::plugin::manifest::PluginCapability::ReadPlayers].into(),
            subscriptions: [EventKind::PlayerMove, EventKind::PlayerMoved].into(),
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
            command_roots: Default::default(),
        }));
        PluginManager::attach_to_world(
            manager.clone(),
            router.world_mut(0).unwrap().ecs.world_mut(),
        );
        router.set_plugin_manager(manager.clone());
        router.initialize_plugins().unwrap();
        let (outbound_tx, mut packets) = tokio::sync::mpsc::channel(1024);
        let player = router
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
        while packets.try_recv().is_ok() {}
        Self {
            router,
            manager,
            player,
            packets,
        }
    }
    /// Route an encoded authenticated input through the real packet queue and movement system.
    async fn move_to(&mut self, x: f32) -> Result<(), crate::server::worlds::WorldTickError> {
        use jolyne::valentine::{BorrowedMcpePacket, McpePacket, McpePacketArgs};
        let packet = McpePacket::from(jolyne::valentine::PlayerAuthInputPacket {
            position: jolyne::valentine::Vec3 {
                x,
                y: 65.62,
                z: 0.5,
            },
            ..Default::default()
        });
        let mut bytes = bytes::BytesMut::new();
        packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
        let borrowed = BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap();
        self.router
            .world_mut(0)
            .unwrap()
            .route_packet(42, McpePacketArgs, borrowed);
        self.router.tick().await
    }
    /// Read component witnesses without executing a callback or replenishing its budget.
    fn logs(&self) -> Vec<String> {
        self.manager
            .lock()
            .unwrap()
            .plugins
            .values()
            .next()
            .unwrap()
            .store
            .data()
            .test_logs
            .clone()
    }
    /// Read the actual native position after the transaction.
    fn position(&self) -> glam::DVec3 {
        self.router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get::<Position>(self.player)
            .unwrap()
            .0
    }
}

/// A host log is evidence that the real guest callback executed.
fn witness(value: u8) -> String {
    format!(
        "i32.const 1024 i32.const {value} i32.store8 i32.const 0 i32.const 1024 i32.const 1 call @log@"
    )
}
/// Return an accepted destination using this export's actual canonical scalar arguments.
fn accept(x: &str) -> String {
    format!(
        "i32.const 32 i32.const 1 i32.store i32.const 40 {x} f64.store i32.const 48 local.get 6 f64.store i32.const 56 local.get 7 f64.store i32.const 32"
    )
}

/// Derive an indirect committed record field offset from the selected WIT, not a fixed ABI guess.
fn movement_x(field: &str) -> usize {
    let mut resolve = wit_parser::Resolve::default();
    let (package, _) = resolve
        .push_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../unastar-api/wit"))
        .unwrap();
    let world = resolve
        .select_world(&[package], Some("unastar-plugin"))
        .unwrap();
    let function = resolve.worlds[world]
        .exports
        .values()
        .find_map(|item| match item {
            wit_parser::WorldItem::Function(function) if function.name == "on-player-moved" => {
                Some(function)
            }
            _ => None,
        })
        .unwrap();
    assert!(
        resolve
            .wasm_signature(wit_parser::abi::AbiVariant::GuestExport, function)
            .indirect_params
    );
    let mut sizes = wit_parser::SizeAlign::default();
    sizes.fill(&resolve);
    let mut offset = 0usize;
    for parameter in &function.params {
        offset = offset.next_multiple_of(sizes.align(&parameter.ty).align_wasm32());
        if parameter.name == "movement" {
            let wit_parser::Type::Id(mut record) = parameter.ty else {
                unreachable!()
            };
            while let wit_parser::TypeDefKind::Type(wit_parser::Type::Id(aliased)) =
                &resolve.types[record].kind
            {
                record = *aliased;
            }
            let wit_parser::TypeDefKind::Record(record) = &resolve.types[record].kind else {
                unreachable!()
            };
            for member in &record.fields {
                offset = offset.next_multiple_of(sizes.align(&member.ty).align_wasm32());
                if member.name == field {
                    return offset;
                }
                offset += sizes.size(&member.ty).size_wasm32();
            }
            unreachable!();
        }
        offset += sizes.size(&parameter.ty).size_wasm32();
    }
    unreachable!()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn component_veto_prevents_real_movement_and_never_emits_a_commit() {
    let mut harness = Harness::new(&[
        (
            "on-player-move",
            format!(
                "{} i32.const 32 i32.const 0 i32.store i32.const 32",
                witness(b'P')
            ),
        ),
        ("on-player-moved", witness(b'C')),
    ]);
    let from = harness.position();
    harness.move_to(1.5).await.unwrap();
    assert_eq!(harness.position(), from);
    assert_eq!(harness.logs(), ["P"]);
    assert!(
        std::iter::from_fn(|| harness.packets.try_recv().ok()).any(|packet| {
            matches!(
                packet.data,
                jolyne::valentine::McpePacketData::MovePlayerPacket(_)
            )
        })
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn component_redirect_reports_actual_final_feet_once_and_suppresses_buffered_duplicates() {
    let committed = format!(
        "local.get 0 f64.load offset={} f64.const 0.5 f64.ne if unreachable end local.get 0 f64.load offset={} f64.const 2.75 f64.ne if unreachable end {}",
        movement_x("from"),
        movement_x("to"),
        witness(b'C')
    );
    let mut harness = Harness::new(&[
        (
            "on-player-move",
            format!("{} {}", witness(b'P'), accept("f64.const 2.75")),
        ),
        ("on-player-moved", committed),
    ]);
    harness.move_to(1.5).await.unwrap();
    assert_eq!(harness.position().x, 2.75);
    assert_eq!(harness.logs(), ["P", "C"]);
    let player = harness.player;
    harness
        .router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .resource_mut::<crate::ecs::events::EventBuffer>()
        .push(ServerEvent::PlayerMove {
            entity: player,
            from: (0.5, 65.62, 0.5),
            to: (2.75, 65.62, 0.5),
        });
    harness.router.tick().await.unwrap();
    assert_eq!(harness.logs(), ["P", "C"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_later_native_veto_or_invalid_destination_never_emits_a_component_commit() {
    for invalid in [false, true] {
        let mut harness = Harness::new(&[
            (
                "on-player-move",
                format!("{} {}", witness(b'P'), accept("local.get 5")),
            ),
            ("on-player-moved", witness(b'C')),
        ]);
        harness
            .router
            .world_mut(0)
            .unwrap()
            .ecs
            .world_mut()
            .add_observer(move |mut event: bevy_ecs::prelude::On<PlayerMove>| {
                if invalid {
                    event.destination.x = f64::NAN;
                } else {
                    event.cancel();
                }
            });
        let from = harness.position();
        harness.move_to(1.5).await.unwrap();
        assert_eq!(harness.position(), from);
        assert_eq!(harness.logs(), ["P"]);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replaced_connection_never_receives_a_stale_component_movement_commit() {
    let mut harness = Harness::new(&[
        (
            "on-player-move",
            format!("{} {}", witness(b'P'), accept("local.get 5")),
        ),
        ("on-player-moved", witness(b'C')),
    ]);
    harness
        .router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .add_observer(
            |event: bevy_ecs::prelude::On<PlayerMove>,
             mut sessions: bevy_ecs::prelude::Query<&mut PlayerSession>| {
                sessions.get_mut(event.player()).unwrap().session_id = 99;
            },
        );
    let from = harness.position();
    harness.move_to(1.5).await.unwrap();
    assert_eq!(harness.position(), from);
    assert_eq!(harness.logs(), ["P"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn required_movement_trap_or_nonfinite_decision_fails_before_position_commit() {
    for body in ["unreachable".into(), accept("f64.const nan")] {
        let mut harness =
            Harness::new(&[("on-player-move", body), ("on-player-moved", witness(b'C'))]);
        assert!(harness.move_to(1.5).await.is_err());
        assert!(harness.logs().is_empty());
        let world = harness.router.world(0).unwrap().ecs.world();
        assert!(crate::server::game::inventory_effects::is_recovery_fenced(
            world,
            harness.player
        ));
        assert!(world.get::<Position>(harness.player).is_none());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn default_proposal_preserves_ordinary_movement_and_unchanged_input_has_no_commit() {
    let mut harness = Harness::new(&[("on-player-moved", witness(b'C'))]);
    harness.move_to(1.5).await.unwrap();
    assert_eq!(harness.position().x, 1.5);
    assert_eq!(harness.logs(), ["C"]);
    harness.move_to(1.5).await.unwrap();
    assert_eq!(harness.logs(), ["C"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_move_bound_still_applies_to_an_accepted_component_redirect() {
    let mut harness = Harness::new(&[
        (
            "on-player-move",
            format!("{} {}", witness(b'P'), accept("f64.const 1000")),
        ),
        ("on-player-moved", witness(b'C')),
    ]);
    let from = harness.position();
    harness.move_to(1.5).await.unwrap();
    assert_eq!(harness.position(), from);
    assert_eq!(harness.logs(), ["P"]);
}
