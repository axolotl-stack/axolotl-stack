//! Genuine components declare command roots and handle real player command packets.

use super::*;
use crate::ecs::events::EventKind;
use crate::plugin::manifest::PluginCapability;
use crate::server::game::{GameServer, PlayerSpawnData};
use jolyne::valentine::*;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Construct a complete native world and attach one shared component manager.
fn harness() -> (GameServer, Arc<Mutex<PluginManager>>) {
    let mut server = GameServer::with_config(crate::ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..Default::default()
    });
    let manager = Arc::new(Mutex::new(PluginManager {
        engine: engine(),
        plugins: Default::default(),
        pending: Vec::new(),
        tick_prepared: false,
        tick_budget_valid: true,
        readiness: Default::default(),
        storage_registry: Default::default(),
        command_roots: Default::default(),
    }));
    PluginManager::attach_to_world(manager.clone(), server.ecs.world_mut());
    (server, manager)
}
/// Write bounded public test strings into canonical guest linear memory.
fn string(address: usize, value: &str) -> String {
    value
        .bytes()
        .enumerate()
        .map(|(offset, byte)| {
            format!(
                "i32.const {} i32.const {byte} i32.store8 ",
                address + offset
            )
        })
        .collect()
}
/// Reserve a root and assert its actual typed canonical result.
fn register(name: &str, expected_error: bool, expected: u8) -> String {
    format!(
        "{} {} i32.const 1024 i32.const {} i32.const 1100 i32.const 7 i32.const 128 call @register-root@ i32.const 128 i32.load8_u i32.const {} i32.ne if unreachable end i32.const 129 i32.load8_u i32.const {expected} i32.ne if unreachable end",
        string(1024, name),
        string(1100, "Example"),
        name.len(),
        u8::from(expected_error)
    )
}
/// Load a real component with controlled initialization and scoped command execution.
fn add(manager: &Arc<Mutex<PluginManager>>, id: &str, load: String, command: String) {
    let mut manager = manager.lock().unwrap();
    let manifest = PluginManifest {
        id: PluginId(id.into()),
        name: "Commands".into(),
        version: "1".into(),
        api_version: PLUGIN_API_VERSION.into(),
        required: false,
        config: None,
        capabilities: [
            PluginCapability::ReadPlayers,
            PluginCapability::SendMessages,
            PluginCapability::Commands,
        ]
        .into(),
        subscriptions: [EventKind::PlayerCommand].into(),
        limits: PluginLimits::default(),
    };
    let component = fixture::component(
        &manager.engine,
        &[("on-load", load), ("on-player-command", command)],
    );
    let store = store(&manager.engine, &manifest.limits);
    manager.pending.push(PendingPlugin {
        manifest,
        component,
        store,
    });
}
/// Spawn an authenticated actor through the native initialization and advertisement path.
fn spawn(server: &mut GameServer) -> (Entity, mpsc::Receiver<McpePacket>) {
    let (tx, rx) = mpsc::channel(65536);
    let player = server
        .spawn_player(PlayerSpawnData {
            session_id: 7,
            runtime_id: 7,
            display_name: "Reader".into(),
            xuid: Some("7".into()),
            uuid: Some(uuid::Uuid::from_u128(7).to_string()),
            position: glam::DVec3::new(0.5, 65.62, 0.5),
            outbound_tx: tx,
            saved_data: None,
            chunk_radius: 0,
        })
        .unwrap();
    (player, rx)
}
/// Decode actual outgoing AvailableCommands packets without observing unrelated initial traffic.
fn advertisements(rx: &mut mpsc::Receiver<McpePacket>) -> Vec<AvailableCommandsPacket> {
    let mut packets = Vec::new();
    while let Ok(packet) = rx.try_recv() {
        if let McpePacketData::AvailableCommandsPacket(packet) = packet.data {
            let mut bytes = bytes::BytesMut::new();
            McpePacket::from(*packet)
                .data
                .encode_inner(&mut bytes, 0, 0)
                .unwrap();
            let decoded = BorrowedMcpePacket::decode_inner(&mut bytes.freeze())
                .unwrap()
                .into_owned(McpePacketArgs)
                .unwrap();
            let McpePacketData::AvailableCommandsPacket(packet) = decoded.data else {
                panic!("wrong advertisement packet");
            };
            packets.push(*packet);
        }
    }
    packets
}
/// Deliver an encoded CommandRequest through native packet ingress, with real origin metadata.
fn request(server: &mut GameServer, line: &str) {
    let packet = McpePacket::from(CommandRequestPacket {
        command: line.into(),
        origin: types::CommandOriginDatajson {
            type_: "player".into(),
            request_id: "req-7".into(),
            ..Default::default()
        },
        is_internal: false,
        version: "latest".into(),
    });
    let mut bytes = bytes::BytesMut::new();
    packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
    server.route_packet(
        7,
        McpePacketArgs,
        BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap(),
    );
    server.tick();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_request_reaches_scoped_handler_and_preserves_opaque_origin_id() {
    let (mut server, manager) = harness();
    add(
        &manager,
        "example.commands",
        register("sketch", false, 0),
        format!(
            r#"
        local.get 4 i32.const 1 i32.ne if unreachable end
        local.get 6 i32.const 5 i32.ne if unreachable end
        local.get 5 i32.load8_u i32.const 114 i32.ne if unreachable end
        {} local.get 1 i32.const 1200 i32.const 7 i32.const 160 call @[method]player.message@
        i32.const 160 i32.load8_u if unreachable end
        i32.const 0
    "#,
            string(1200, "handled")
        ),
    );
    manager
        .lock()
        .unwrap()
        .initialize_startup(server.ecs.world_mut())
        .unwrap();
    let (_player, mut rx) = spawn(&mut server);
    let declared = advertisements(&mut rx);
    assert_eq!(declared.len(), 1);
    assert!(
        declared[0]
            .commands
            .iter()
            .any(|root| &*root.name == "sketch")
    );
    request(&mut server, "/sketch words after the root");
    let mut messages = 0;
    while let Ok(packet) = rx.try_recv() {
        match packet.data {
            McpePacketData::TextPacket(_) => messages += 1,
            McpePacketData::CommandOutputPacket(_) => {
                panic!("handled command reached native default")
            }
            _ => {}
        }
    }
    assert_eq!(messages, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn builtin_registration_keeps_metadata_and_pass_keeps_native_permissions() {
    let (mut server, manager) = harness();
    let baseline = server
        .ecs
        .world()
        .resource::<crate::command::CommandRegistry>()
        .schema()
        .packet()
        .clone();
    add(
        &manager,
        "example.commands",
        register("difficulty", false, 1),
        "i32.const 1".into(),
    );
    manager
        .lock()
        .unwrap()
        .initialize_startup(server.ecs.world_mut())
        .unwrap();
    let (_player, mut rx) = spawn(&mut server);
    let declared = advertisements(&mut rx);
    assert_eq!(declared[0], baseline);
    request(&mut server, "/difficulty hard");
    let mut outputs = Vec::new();
    while let Ok(packet) = rx.try_recv() {
        if let McpePacketData::CommandOutputPacket(packet) = packet.data {
            outputs.push(packet);
        }
    }
    assert_eq!(outputs.len(), 1);
    assert_eq!(&*outputs[0].origin_data.request_id, "req-7");
    assert!(
        outputs[0]
            .output
            .output_messages
            .iter()
            .any(|message| &*message.message_id == "commands.generic.error.permissions")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_owner_conflicts_and_trapped_registration_is_rolled_back_before_next_init() {
    for trap in [false, true] {
        let (mut server, manager) = harness();
        add(
            &manager,
            "example.first",
            format!(
                "{} {}",
                register("sketch", false, 0),
                if trap { "unreachable" } else { "" }
            ),
            "i32.const 1".into(),
        );
        add(
            &manager,
            "example.second",
            register("sketch", !trap, if trap { 0 } else { 1 }),
            "i32.const 1".into(),
        );
        manager
            .lock()
            .unwrap()
            .initialize_startup(server.ecs.world_mut())
            .unwrap();
        assert_eq!(
            manager.lock().unwrap().plugins.len(),
            if trap { 1 } else { 2 }
        );
        let (_player, mut rx) = spawn(&mut server);
        let declared = advertisements(&mut rx);
        assert_eq!(
            declared[0]
                .commands
                .iter()
                .filter(|root| &*root.name == "sketch")
                .count(),
            1
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owner_unload_removes_only_its_roots_and_exact_channel_replacement_gets_a_fresh_catalogue()
{
    let (mut server, manager) = harness();
    add(
        &manager,
        "example.first",
        register("sketch", false, 0),
        "i32.const 1".into(),
    );
    add(
        &manager,
        "example.second",
        register("notice", false, 0),
        "i32.const 1".into(),
    );
    manager
        .lock()
        .unwrap()
        .initialize_startup(server.ecs.world_mut())
        .unwrap();
    let (player, mut rx) = spawn(&mut server);
    advertisements(&mut rx);
    manager
        .lock()
        .unwrap()
        .publish_command_roots(server.ecs.world_mut());
    assert!(advertisements(&mut rx).is_empty());
    manager
        .lock()
        .unwrap()
        .plugins
        .remove(&PluginId("example.first".into()));
    manager
        .lock()
        .unwrap()
        .publish_command_roots(server.ecs.world_mut());
    let declared = advertisements(&mut rx);
    assert_eq!(declared.len(), 1);
    assert!(
        !declared[0]
            .commands
            .iter()
            .any(|root| &*root.name == "sketch")
    );
    assert!(
        declared[0]
            .commands
            .iter()
            .any(|root| &*root.name == "notice")
    );
    let (tx, mut replacement) = mpsc::channel(16);
    server
        .ecs
        .world_mut()
        .get_mut::<crate::entity::components::PlayerSession>(player)
        .unwrap()
        .outbound_tx = tx;
    manager
        .lock()
        .unwrap()
        .publish_command_roots(server.ecs.world_mut());
    assert_eq!(advertisements(&mut replacement).len(), 1);
    assert!(advertisements(&mut rx).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registration_validates_capability_quota_identifiers_and_builtin_aliases() {
    for case in ["capability", "quota", "identifier", "builtin-alias"] {
        let (mut server, manager) = harness();
        let body = match case {
            "capability" => register("sketch", true, 2),
            "quota" => format!(
                "{} {}",
                register("sketch", false, 0),
                register("notice", true, 3)
            ),
            "identifier" => format!(
                "{} {}",
                register("/invalid", true, 0),
                register("sketch", false, 0)
            ),
            _ => register("?", false, 1),
        };
        add(&manager, "example.commands", body, "i32.const 1".into());
        {
            let mut manager = manager.lock().unwrap();
            let pending = manager.pending.last_mut().unwrap();
            if case == "capability" {
                pending
                    .manifest
                    .capabilities
                    .remove(&PluginCapability::Commands);
            }
            if case == "quota" {
                pending.manifest.limits.max_command_roots = 1;
                pending.store.data_mut().api_limits.max_command_roots = 1;
            }
        }
        manager
            .lock()
            .unwrap()
            .initialize_startup(server.ecs.world_mut())
            .unwrap();
        assert_eq!(manager.lock().unwrap().plugins.len(), 1, "{case}");
        let (_player, mut rx) = spawn(&mut server);
        let declared = advertisements(&mut rx);
        assert_eq!(
            declared[0]
                .commands
                .iter()
                .any(|root| &*root.name == "sketch"),
            matches!(case, "quota" | "identifier")
        );
        assert!(
            !declared[0]
                .commands
                .iter()
                .any(|root| &*root.name == "notice")
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn catalogue_backpressure_retries_without_advancing_the_delivery_cache() {
    let (mut server, manager) = harness();
    add(
        &manager,
        "example.commands",
        register("sketch", false, 0),
        "i32.const 1".into(),
    );
    manager
        .lock()
        .unwrap()
        .initialize_startup(server.ecs.world_mut())
        .unwrap();
    let (player, mut rx) = spawn(&mut server);
    advertisements(&mut rx);
    let (tx, mut replacement) = mpsc::channel(1);
    server
        .ecs
        .world_mut()
        .get_mut::<crate::entity::components::PlayerSession>(player)
        .unwrap()
        .outbound_tx = tx;
    manager
        .lock()
        .unwrap()
        .publish_command_roots(server.ecs.world_mut());
    manager
        .lock()
        .unwrap()
        .plugins
        .remove(&PluginId("example.commands".into()));
    manager
        .lock()
        .unwrap()
        .publish_command_roots(server.ecs.world_mut());
    let old = advertisements(&mut replacement);
    assert_eq!(old.len(), 1);
    assert!(old[0].commands.iter().any(|root| &*root.name == "sketch"));
    manager
        .lock()
        .unwrap()
        .publish_command_roots(server.ecs.world_mut());
    let next = advertisements(&mut replacement);
    assert_eq!(next.len(), 1);
    assert!(!next[0].commands.iter().any(|root| &*root.name == "sketch"));
    manager
        .lock()
        .unwrap()
        .publish_command_roots(server.ecs.world_mut());
    assert!(advertisements(&mut replacement).is_empty());
}

/// A native publication barrier stands in for any component's asynchronous admission plan.
struct HoldInitial;
impl crate::server::game::player_events::PlayerEventHandler for HoldInitial {
    fn on_joined(
        &self,
        world: &mut bevy_ecs::world::World,
        event: &crate::server::game::player_events::PlayerJoined,
    ) {
        assert!(crate::server::game::player_events::defer_initial_state(
            world,
            event.player()
        ));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initializing_connection_receives_no_advertisement_until_actual_initial_publication() {
    let (mut server, manager) = harness();
    add(
        &manager,
        "example.commands",
        register("sketch", false, 0),
        "i32.const 1".into(),
    );
    manager
        .lock()
        .unwrap()
        .initialize_startup(server.ecs.world_mut())
        .unwrap();
    server
        .ecs
        .world_mut()
        .resource_mut::<crate::server::game::player_events::PlayerEventHooks>()
        .register(HoldInitial);
    let (player, mut rx) = spawn(&mut server);
    assert!(advertisements(&mut rx).is_empty());
    manager
        .lock()
        .unwrap()
        .publish_command_roots(server.ecs.world_mut());
    assert!(advertisements(&mut rx).is_empty());
    assert!(crate::server::game::player_events::release_initial_state(
        server.ecs.world_mut(),
        player
    ));
    server.tick();
    let initial = advertisements(&mut rx);
    assert_eq!(initial.len(), 1);
    assert!(
        initial[0]
            .commands
            .iter()
            .any(|root| &*root.name == "sketch")
    );
    manager
        .lock()
        .unwrap()
        .publish_command_roots(server.ecs.world_mut());
    assert!(advertisements(&mut rx).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn handled_builtin_and_trapping_required_callback_never_execute_native_default() {
    for trap in [false, true] {
        let (mut server, manager) = harness();
        add(
            &manager,
            "example.commands",
            String::new(),
            if trap { "unreachable" } else { "i32.const 0" }.into(),
        );
        if trap {
            manager.lock().unwrap().pending[0].manifest.required = true;
        }
        manager
            .lock()
            .unwrap()
            .initialize_startup(server.ecs.world_mut())
            .unwrap();
        let (player, mut rx) = spawn(&mut server);
        advertisements(&mut rx);
        server
            .ecs
            .world_mut()
            .entity_mut(player)
            .insert(crate::command::Operator);
        let before = *server
            .ecs
            .world()
            .resource::<crate::world::rules::Difficulty>();
        request(&mut server, "/difficulty hard");
        assert_eq!(
            *server
                .ecs
                .world()
                .resource::<crate::world::rules::Difficulty>(),
            before
        );
        while let Ok(packet) = rx.try_recv() {
            assert!(!matches!(
                packet.data,
                McpePacketData::CommandOutputPacket(_)
            ));
        }
        assert_eq!(manager.lock().unwrap().check_required().is_err(), trap);
    }
}
