//! Typed UI is exercised by real canonical components and actual encoded client packets.

use super::*;
use crate::ecs::events::{EventBuffer, EventKind};
use crate::plugin::manifest::PluginCapability;
use crate::server::game::{GameServer, PlayerSpawnData};
use jolyne::valentine::*;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

struct Harness {
    server: GameServer,
    manager: Arc<Mutex<PluginManager>>,
    player: Entity,
    rx: mpsc::Receiver<McpePacket>,
}
impl Harness {
    /// A complete source actor and shared runtime with an owner-scoped presentation capability.
    fn new(open: String, response: String, closed: String) -> Self {
        let mut server = GameServer::with_config(crate::ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        });
        let engine = engine();
        let manifest = PluginManifest {
            id: PluginId("example.ui".into()),
            name: "UI".into(),
            version: "1".into(),
            api_version: PLUGIN_API_VERSION.into(),
            required: false,
            config: None,
            capabilities: [
                PluginCapability::ReadPlayers,
                PluginCapability::Presentation,
                PluginCapability::SendMessages,
            ]
            .into(),
            subscriptions: [EventKind::PlayerJump].into(),
            limits: PluginLimits::default(),
        };
        let pending = PendingPlugin {
            component: fixture::component(
                &engine,
                &[
                    ("on-player-jump", open),
                    ("on-player-form-response", response),
                    ("on-form-closed", closed),
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
        }));
        PluginManager::attach_to_world(manager.clone(), server.ecs.world_mut());
        manager
            .lock()
            .unwrap()
            .initialize_startup(server.ecs.world_mut())
            .unwrap();
        let (player, rx) = spawn(&mut server, 7);
        let mut h = Self {
            server,
            manager,
            player,
            rx,
        };
        h.packets();
        h
    }
    /// Feed a committed native notification; the handler receives real scoped resources.
    fn open(&mut self) {
        self.server
            .ecs
            .world_mut()
            .resource_mut::<EventBuffer>()
            .push(ServerEvent::PlayerJump {
                entity: self.player,
            });
        self.tick();
    }
    /// Dispatch a complete standalone component cycle without replacing its fuel budget per callback.
    fn tick(&mut self) {
        self.manager
            .lock()
            .unwrap()
            .tick(self.server.ecs.world_mut());
    }
    /// Every inspected UI packet must also survive the actual current wire codec.
    fn packets(&mut self) -> Vec<McpePacket> {
        drain(&mut self.rx)
    }
    /// Genuine form response ingress, without exposing protocol JSON to the guest callback.
    fn respond(&mut self, wire: u32, json: &str, session: u64) {
        let packet = McpePacket::from(ModalFormResponsePacket {
            form_id: wire,
            json_response: Some(json.into()),
            form_cancel_reason: None,
        });
        let mut bytes = bytes::BytesMut::new();
        packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
        self.server.route_packet(
            session,
            McpePacketArgs,
            BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap(),
        );
        self.tick();
    }
}
/// Spawn through native identity initialization and keep the real outbound channel alive.
fn spawn(server: &mut GameServer, session: u64) -> (Entity, mpsc::Receiver<McpePacket>) {
    let (tx, rx) = mpsc::channel(65536);
    let actor = server
        .spawn_player(PlayerSpawnData {
            session_id: session,
            runtime_id: session as i64,
            display_name: format!("Viewer{session}"),
            xuid: Some(session.to_string()),
            uuid: Some(uuid::Uuid::from_u128(session as u128).to_string()),
            position: glam::DVec3::new(0.5, 65.62, 0.5),
            outbound_tx: tx,
            saved_data: None,
            chunk_radius: 0,
        })
        .unwrap();
    (actor, rx)
}
/// Decode the packets that came from the authoritative publisher, including all conditional fields.
fn drain(rx: &mut mpsc::Receiver<McpePacket>) -> Vec<McpePacket> {
    let mut out = Vec::new();
    while let Ok(packet) = rx.try_recv() {
        if matches!(
            packet.data,
            McpePacketData::ModalFormRequestPacket(_)
                | McpePacketData::ClientboundCloseFormPacket(_)
                | McpePacketData::SetTitlePacket(_)
                | McpePacketData::SetDisplayObjectivePacket(_)
                | McpePacketData::SetScorePacket(_)
                | McpePacketData::RemoveObjectivePacket(_)
                | McpePacketData::TextPacket(_)
        ) {
            let mut bytes = bytes::BytesMut::new();
            packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
            out.push(
                BorrowedMcpePacket::decode_inner(&mut bytes.freeze())
                    .unwrap()
                    .into_owned(McpePacketArgs)
                    .unwrap(),
            );
        }
    }
    out
}
/// Initialize owned UTF-8 values independently of the canonical realloc bookkeeping.
fn literal(address: usize, text: &str) -> String {
    text.bytes()
        .enumerate()
        .map(|(offset, byte)| {
            format!(
                "i32.const {} i32.const {byte} i32.store8 ",
                address + offset
            )
        })
        .collect()
}
/// A two-button typed form; caller verifies the actual result rather than guessing success.
fn form() -> String {
    format!(
        r#"
    {} {} {} {}
    i32.const 1500 i32.const 1200 i32.store i32.const 1504 i32.const 3 i32.store
    i32.const 1508 i32.const 1210 i32.store i32.const 1512 i32.const 3 i32.store
    local.get 0 local.get 1 i32.const 1000 i32.const 4 i32.const 1100 i32.const 6 i32.const 1500 i32.const 2 i64.const 77 i32.const 128 call @show-form@
    i32.const 128 i32.load8_u if unreachable end
"#,
        literal(1000, "Menu"),
        literal(1100, "Choose"),
        literal(1200, "One"),
        literal(1210, "Two")
    )
}
/// One stable sidebar key and an owned row value.
fn sidebar(title: &str, text: &str) -> String {
    format!(
        r#"
    {} {} {}
    i32.const 1600 i32.const 1300 i32.store i32.const 1604 i32.const 3 i32.store
    i32.const 1608 i32.const 1400 i32.store i32.const 1612 i32.const {} i32.store
    local.get 0 local.get 1 i32.const 1 i32.const 1000 i32.const {} i32.const 1600 i32.const 1 i32.const 128 call @set-sidebar@
    i32.const 128 i32.load8_u if unreachable end
"#,
        literal(1000, title),
        literal(1300, "row"),
        literal(1400, text),
        text.len(),
        title.len()
    )
}
/// A maintained action bar with an actual typed success result.
fn bar(text: &str) -> String {
    format!(
        "{} local.get 0 local.get 1 i32.const 1 i32.const 1100 i32.const {} i32.const 128 call @set-action-bar@ i32.const 128 i32.load8_u if unreachable end",
        literal(1100, text),
        text.len()
    )
}
/// A scoped response witness that uses the bound player's actual message capability.
fn message(text: &str) -> String {
    format!(
        "{} local.get 1 i32.const 1200 i32.const {} i32.const 192 call @[method]player.message@ i32.const 192 i32.load8_u if unreachable end",
        literal(1200, text),
        text.len()
    )
}
/// Obtain the actual client form identifier only from a successfully emitted request.
fn wire(packets: &[McpePacket]) -> u32 {
    packets
        .iter()
        .find_map(|packet| {
            if let McpePacketData::ModalFormRequestPacket(form) = &packet.data {
                Some(form.form_id)
            } else {
                None
            }
        })
        .expect("form packet")
}

#[tokio::test]
async fn form_selection_is_typed_bound_to_its_viewer_and_consumed_once() {
    let response = format!(
        "local.get 2 i64.const 1 i64.ne if unreachable end local.get 3 i64.const 77 i64.ne if unreachable end local.get 4 if unreachable end local.get 5 i32.const 1 i32.ne if unreachable end {}",
        message("selected")
    );
    let mut h = Harness::new(form(), response, String::new());
    h.open();
    let sent = h.packets();
    let id = wire(&sent);
    let McpePacketData::ModalFormRequestPacket(request) = &sent
        .iter()
        .find(|packet| matches!(packet.data, McpePacketData::ModalFormRequestPacket(_)))
        .unwrap()
        .data
    else {
        unreachable!()
    };
    let json: serde_json::Value = serde_json::from_str(&request.form_uijson).unwrap();
    assert_eq!(json["buttons"].as_array().unwrap().len(), 2);
    let (_other, mut other_rx) = spawn(&mut h.server, 8);
    drain(&mut other_rx);
    h.respond(id, "1", 8);
    assert!(
        h.packets()
            .iter()
            .all(|packet| !matches!(packet.data, McpePacketData::TextPacket(_)))
    );
    h.respond(id, "1", 7);
    assert_eq!(
        h.packets()
            .iter()
            .filter(|packet| matches!(packet.data, McpePacketData::TextPacket(_)))
            .count(),
        1
    );
    h.respond(id, "1", 7);
    assert!(
        h.packets()
            .iter()
            .all(|packet| !matches!(packet.data, McpePacketData::TextPacket(_)))
    );
}

#[tokio::test]
async fn invalid_form_payload_never_becomes_a_selection_or_a_command() {
    for json in ["9", "-1", "\"/op somebody\"", "[0]", "1.0"] {
        let response = format!(
            "local.get 4 i32.const 1 i32.ne if unreachable end local.get 5 i32.const 5 i32.ne if unreachable end {}",
            message("invalid")
        );
        let mut h = Harness::new(form(), response, String::new());
        h.open();
        let id = wire(&h.packets());
        h.respond(id, json, 7);
        assert_eq!(
            h.packets()
                .iter()
                .filter(|packet| matches!(packet.data, McpePacketData::TextPacket(_)))
                .count(),
            1,
            "{json}"
        );
        assert!(
            h.server
                .ecs
                .world()
                .get::<crate::command::Operator>(h.player)
                .is_none()
        );
    }
}

#[tokio::test]
async fn sidebar_and_actionbar_are_real_packets_without_identity_or_life_mutation() {
    let mut h = Harness::new(
        format!("{} {}", sidebar("Board", "Value"), bar("Ready")),
        String::new(),
        String::new(),
    );
    h.open();
    let packets = h.packets();
    assert!(packets.iter().any(|packet|matches!(&packet.data,McpePacketData::SetDisplayObjectivePacket(value)if &*value.objective_display_name=="Board")));
    assert!(packets.iter().any(|packet|matches!(&packet.data,McpePacketData::SetScorePacket(value)if value.score_info.len()==1)));
    assert!(packets.iter().any(|packet|matches!(&packet.data,McpePacketData::SetTitlePacket(value)if &*value.title_text=="Ready")));
    assert_eq!(
        h.server
            .ecs
            .world()
            .get::<crate::entity::components::PlayerName>(h.player)
            .unwrap()
            .0,
        "Viewer7"
    );
    assert_eq!(
        h.server
            .ecs
            .world()
            .get::<crate::entity::components::Health>(h.player)
            .unwrap()
            .current,
        20.0
    );
    h.tick();
    assert!(h.packets().is_empty());
    h.server
        .ecs
        .world_mut()
        .resource_mut::<crate::ecs::TickCounter>()
        .current += 20;
    h.tick();
    assert_eq!(
        h.packets()
            .iter()
            .filter(|packet| matches!(packet.data, McpePacketData::SetTitlePacket(_)))
            .count(),
        1
    );
}

#[tokio::test]
async fn callback_trap_rolls_back_every_new_widget_and_form() {
    let mut h = Harness::new(
        format!(
            "{} {} {} unreachable",
            sidebar("Board", "Value"),
            bar("Ready"),
            form()
        ),
        String::new(),
        String::new(),
    );
    h.open();
    assert!(h.packets().is_empty());
    h.manager
        .lock()
        .unwrap()
        .publish_ui(h.server.ecs.world_mut());
    assert!(h.packets().is_empty());
}

#[tokio::test]
async fn dropping_owner_restores_sidebar_actionbar_and_open_form() {
    let mut h = Harness::new(
        format!("{} {} {}", sidebar("Board", "Value"), bar("Ready"), form()),
        String::new(),
        String::new(),
    );
    h.open();
    h.packets();
    h.manager
        .lock()
        .unwrap()
        .plugins
        .remove(&PluginId("example.ui".into()));
    h.tick();
    let packets = h.packets();
    assert!(
        packets
            .iter()
            .any(|packet| matches!(packet.data, McpePacketData::RemoveObjectivePacket(_)))
    );
    assert!(packets.iter().any(|packet|matches!(&packet.data,McpePacketData::SetTitlePacket(value)if value.title_text.is_empty())));
    assert!(
        packets
            .iter()
            .any(|packet| matches!(packet.data, McpePacketData::ClientboundCloseFormPacket(_)))
    );
}

#[tokio::test]
async fn one_slot_transport_makes_sidebar_progress_without_losing_rows() {
    let mut h = Harness::new(sidebar("Board", "Value"), String::new(), String::new());
    let (tx, mut rx) = mpsc::channel(1);
    h.server
        .ecs
        .world_mut()
        .get_mut::<crate::entity::components::PlayerSession>(h.player)
        .unwrap()
        .outbound_tx = tx;
    h.open();
    drain(&mut rx);
    let mut packets = Vec::new();
    for _ in 0..6 {
        h.tick();
        packets.extend(drain(&mut rx));
    }
    assert_eq!(
        packets
            .iter()
            .filter(|packet| matches!(packet.data, McpePacketData::SetDisplayObjectivePacket(_)))
            .count(),
        1
    );
    assert_eq!(
        packets
            .iter()
            .filter(|packet| matches!(packet.data, McpePacketData::SetScorePacket(_)))
            .count(),
        1
    );
}

/// Ordinary native initial publication remains the authority for all guest UI.
struct Hold;
impl crate::server::game::player_events::PlayerEventHandler for Hold {
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
#[tokio::test]
async fn staged_ui_waits_for_actual_native_initial_publication() {
    let mut h = Harness::new(String::new(), String::new(), String::new());
    h.server
        .ecs
        .world_mut()
        .resource_mut::<crate::server::game::player_events::PlayerEventHooks>()
        .register(Hold);
    let (player, rx) = spawn(&mut h.server, 9);
    h.player = player;
    h.rx = rx;
    let mut manager = h.manager.lock().unwrap();
    let plugin = manager
        .plugins
        .get_mut(&PluginId("example.ui".into()))
        .unwrap();
    // Stage through the actual host context with the same scoped validation as canonical imports.
    super::super::lifecycle::invoke(plugin, h.server.ecs.world_mut(), |store, _| {
        use super::super::wit_bindings::unastar::plugin::presentation::Host;
        let tx = make_world_tx(store)?;
        let player = make_player_resource(store, player)?;
        store
            .data_mut()
            .set_action_bar(tx, player, Some("Waiting".into()))
            .map_err(|error| anyhow::anyhow!("{error:?}"))
    })
    .unwrap();
    drop(manager);
    h.tick();
    assert!(h.packets().is_empty());
    assert!(crate::server::game::player_events::release_initial_state(
        h.server.ecs.world_mut(),
        player
    ));
    h.server.tick();
    h.tick();
    assert!(h.packets().iter().any(|packet|matches!(&packet.data,McpePacketData::SetTitlePacket(value)if &*value.title_text=="Waiting")));
}

#[tokio::test]
async fn disconnected_form_closes_without_manufacturing_a_live_player_callback() {
    let closed = "local.get 2 i32.const 3 i32.ne if unreachable end i32.const 0 i32.const 0 i32.const 0 call @log@";
    let mut h = Harness::new(form(), "unreachable".into(), closed.into());
    h.open();
    h.packets();
    h.server.despawn_player(7);
    h.tick();
    let mut manager = h.manager.lock().unwrap();
    let logs = &mut manager
        .plugins
        .get_mut(&PluginId("example.ui".into()))
        .unwrap()
        .store
        .data_mut()
        .test_logs;
    assert_eq!(logs.len(), 1);
}

/// Log one bounded ownership outcome through the real guest import.
fn log(text: &str) -> String {
    format!(
        "{} i32.const 0 i32.const 1800 i32.const {} call @log@",
        literal(1800, text),
        text.len()
    )
}
#[tokio::test]
async fn another_component_cannot_replace_an_owned_sidebar() {
    let attempt = |title: &str| {
        sidebar(title,"Value").replace("i32.const 128 i32.load8_u if unreachable end",&format!("i32.const 128 i32.load8_u if i32.const 129 i32.load8_u i32.const 1 i32.ne if unreachable end {} else {} end",log("conflict"),log("owned")))
    };
    let mut h = Harness::new(attempt("First"), String::new(), String::new());
    {
        let mut manager = h.manager.lock().unwrap();
        let manifest = PluginManifest {
            id: PluginId("example.other".into()),
            name: "Other UI".into(),
            version: "1".into(),
            api_version: PLUGIN_API_VERSION.into(),
            required: false,
            config: None,
            capabilities: [
                PluginCapability::ReadPlayers,
                PluginCapability::Presentation,
            ]
            .into(),
            subscriptions: [EventKind::PlayerJump].into(),
            limits: PluginLimits::default(),
        };
        let pending = PendingPlugin {
            component: fixture::component(
                &manager.engine,
                &[("on-player-jump", attempt("Second"))],
            ),
            store: store(&manager.engine, &manifest.limits),
            manifest,
        };
        manager.pending.push(pending);
    }
    h.open();
    let packets = h.packets();
    assert_eq!(
        packets
            .iter()
            .filter(|packet| matches!(packet.data, McpePacketData::SetDisplayObjectivePacket(_)))
            .count(),
        1
    );
    let manager = h.manager.lock().unwrap();
    let mut outcomes: Vec<_> = manager
        .plugins
        .values()
        .flat_map(|plugin| plugin.store.data().test_logs.clone())
        .collect();
    outcomes.sort();
    assert_eq!(outcomes, ["conflict", "owned"]);
}

#[tokio::test]
async fn widget_quota_denial_preserves_an_already_staged_sidebar() {
    let denied=bar("Denied").replace("i32.const 128 i32.load8_u if unreachable end","i32.const 128 i32.load8_u i32.const 1 i32.ne if unreachable end i32.const 129 i32.load8_u i32.const 3 i32.ne if unreachable end");
    let mut h = Harness::new(
        format!("{} {denied}", sidebar("Board", "Value")),
        String::new(),
        String::new(),
    );
    {
        let mut manager = h.manager.lock().unwrap();
        let plugin = manager
            .plugins
            .get_mut(&PluginId("example.ui".into()))
            .unwrap();
        plugin.manifest.limits.max_ui_widgets = 1;
        plugin.store.data_mut().api_limits.max_ui_widgets = 1;
    }
    h.open();
    let packets = h.packets();
    assert!(
        packets
            .iter()
            .any(|packet| matches!(packet.data, McpePacketData::SetDisplayObjectivePacket(_)))
    );
    assert!(
        !packets
            .iter()
            .any(|packet| matches!(packet.data, McpePacketData::SetTitlePacket(_)))
    );
}

#[tokio::test]
async fn stale_form_identifier_cannot_close_or_answer_a_replacement() {
    let close = r#"local.get 0 local.get 1 i64.const 1 i32.const 192 call @close-form@
        i32.const 192 i32.load8_u if unreachable end i32.const 193 i32.load8_u if unreachable end"#;
    let body = format!(
        "i32.const 600 i32.load i32.eqz if {} else {} {close} end i32.const 600 i32.const 600 i32.load i32.const 1 i32.add i32.store",
        form(),
        form()
    );
    let mut h = Harness::new(body, message("new"), String::new());
    h.open();
    let first = wire(&h.packets());
    h.open();
    let second = wire(&h.packets());
    assert_ne!(first, second);
    h.respond(first, "0", 7);
    assert!(
        h.packets()
            .iter()
            .all(|packet| !matches!(packet.data, McpePacketData::TextPacket(_)))
    );
    h.respond(second, "0", 7);
    assert_eq!(
        h.packets()
            .iter()
            .filter(|packet| matches!(packet.data, McpePacketData::TextPacket(_)))
            .count(),
        1
    );
}

#[tokio::test]
async fn trapped_update_preserves_the_previous_committed_sidebar() {
    let body = format!(
        "i32.const 600 i32.load i32.eqz if {} else {} unreachable end i32.const 600 i32.const 1 i32.store",
        sidebar("Original", "Value"),
        sidebar("Uncommitted", "Replacement")
    );
    let mut h = Harness::new(body, String::new(), String::new());
    h.open();
    assert!(h.packets().iter().any(|packet| matches!(&packet.data,
        McpePacketData::SetDisplayObjectivePacket(value) if &*value.objective_display_name == "Original")));
    h.open();
    assert!(h.packets().is_empty());
    h.tick();
    assert!(h.packets().is_empty());
}

#[tokio::test]
async fn retained_raw_live_player_cannot_mutate_ui_in_a_later_transaction() {
    let retain = r#"
        local.get 1 i32.const 64 call @[method]player.to-handle@
        i32.const 64 i32.load8_u if unreachable end
        local.get 0 i32.const 68 i32.load i32.const 80 call @[method]world-tx.player@
        i32.const 80 i32.load8_u if unreachable end
        i32.const 512 i32.const 84 i32.load i32.store
        i32.const 68 i32.load call @player-handle_drop@
    "#;
    let stale = bar("Stale")
        .replace("local.get 0 local.get 1", "local.get 0 i32.const 512 i32.load")
        .replace(
            "i32.const 128 i32.load8_u if unreachable end",
            "i32.const 128 i32.load8_u i32.const 1 i32.ne if unreachable end i32.const 129 i32.load8_u if unreachable end i32.const 512 i32.load call @player_drop@",
        );
    let body = format!(
        "i32.const 600 i32.load i32.eqz if {retain} else {stale} {} end i32.const 600 i32.const 1 i32.store",
        log("rejected")
    );
    let mut h = Harness::new(body, String::new(), String::new());
    h.open();
    assert!(h.packets().is_empty());
    h.open();
    assert!(h.packets().is_empty());
    let manager = h.manager.lock().unwrap();
    assert_eq!(
        manager.plugins[&PluginId("example.ui".into())]
            .store
            .data()
            .test_logs,
        ["rejected"]
    );
}

#[tokio::test]
async fn form_response_follows_the_exact_connection_into_a_fresh_world_scope() {
    let response = format!(
        r#"
        local.get 0 i32.const 64 call @[method]world-tx.world@
        i32.const 64 i32.load8_u if unreachable end
        i32.const 68 i32.load i32.const 80 call @[method]world-handle.dimension@
        i32.const 80 i32.load8_u if unreachable end
        i32.const 84 i32.load i32.const 1 i32.ne if unreachable end
        i32.const 68 i32.load call @world-handle_drop@
        {}
    "#,
        message("migrated")
    );
    let mut h = Harness::new(form(), response, String::new());
    h.open();
    let id = wire(&h.packets());
    let mut router = crate::server::worlds::DimensionRouter::new(h.server);
    let destination = router
        .create_world(crate::ServerConfig {
            world: crate::world::WorldConfig {
                dimension: 1,
                ..crate::world::WorldConfig::void_spawn_platform(1)
            },
            ..Default::default()
        })
        .unwrap();
    router.add(destination).unwrap();
    PluginManager::attach_to_world(
        h.manager.clone(),
        router.world_mut(1).unwrap().ecs.world_mut(),
    );
    router.set_plugin_manager(h.manager.clone());
    router.refresh_directory();
    crate::server::worlds::request_transfer_checked(
        router.world_mut(0).unwrap().ecs.world_mut(),
        h.player,
        1,
        glam::DVec3::new(4.5, 16.0, 4.5),
        0,
        "menu-transfer".into(),
        |_, _, _| true,
    )
    .unwrap();
    router.tick().await.unwrap();
    assert_eq!(router.locate_session(7).unwrap().0, 1);
    let mut loading_id = None;
    while let Ok(packet) = h.rx.try_recv() {
        if let McpePacketData::ChangeDimensionPacket(change) = packet.data {
            loading_id = change.loading_screen_id;
        }
    }
    let ack = McpePacket::from(ServerboundLoadingScreenPacket {
        loading_screen_packet_type: EnumsServerboundLoadingScreenPacketType::EndLoadingScreen,
        loading_screen_id: Some(loading_id.expect("actual native loading barrier")),
    });
    let mut bytes = bytes::BytesMut::new();
    ack.data.encode_inner(&mut bytes, 0, 0).unwrap();
    router.route_packet(
        7,
        McpePacketArgs,
        BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap(),
    );
    router.tick().await.unwrap();
    assert_eq!(wire(&drain(&mut h.rx)), id);
    let packet = McpePacket::from(ModalFormResponsePacket {
        form_id: id,
        json_response: Some("0".into()),
        form_cancel_reason: None,
    });
    let mut bytes = bytes::BytesMut::new();
    packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
    router.route_packet(
        7,
        McpePacketArgs,
        BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap(),
    );
    router.tick().await.unwrap();
    assert_eq!(
        drain(&mut h.rx)
            .iter()
            .filter(|packet| matches!(packet.data, McpePacketData::TextPacket(_)))
            .count(),
        1
    );
}

#[tokio::test]
async fn invalid_widget_value_reserves_nothing_and_a_later_valid_write_can_commit() {
    let invalid = sidebar("Bad\nTitle", "Value").replace(
        "i32.const 128 i32.load8_u if unreachable end",
        "i32.const 128 i32.load8_u i32.const 1 i32.ne if unreachable end i32.const 129 i32.load8_u i32.const 2 i32.ne if unreachable end",
    );
    let mut h = Harness::new(
        format!("{invalid} {}", bar("Accepted")),
        String::new(),
        String::new(),
    );
    {
        let mut manager = h.manager.lock().unwrap();
        let plugin = manager
            .plugins
            .get_mut(&PluginId("example.ui".into()))
            .unwrap();
        plugin.store.data_mut().api_limits.max_ui_widgets = 1;
    }
    h.open();
    let packets = h.packets();
    assert!(
        !packets
            .iter()
            .any(|packet| matches!(packet.data, McpePacketData::SetDisplayObjectivePacket(_)))
    );
    assert!(packets.iter().any(|packet| matches!(&packet.data,
        McpePacketData::SetTitlePacket(value) if &*value.title_text == "Accepted")));
}
