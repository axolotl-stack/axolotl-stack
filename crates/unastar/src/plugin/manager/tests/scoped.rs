//! Real guests exercise raw canonical resources, not only SDK lifetime wrappers.

use super::*;
use crate::ecs::events::{EventBuffer, EventKind};
use crate::plugin::manifest::PluginCapability;

/// Attach one real component and discard unrelated fixture joins before the controlled callback.
fn attach_scoped(
    router: &mut DimensionRouter,
    bodies: &[(&str, String)],
) -> Arc<Mutex<PluginManager>> {
    for (_, server) in router.iter_mut() {
        server.ecs.world_mut().resource_mut::<EventBuffer>().drain();
    }
    let engine = engine();
    let manifest = PluginManifest {
        id: PluginId("example.global".into()),
        name: "Scoped".into(),
        version: "1".into(),
        api_version: PLUGIN_API_VERSION.into(),
        required: true,
        config: None,
        capabilities: [
            PluginCapability::ReadPlayers,
            PluginCapability::SendMessages,
            PluginCapability::ScheduleTasks,
        ]
        .into(),
        subscriptions: [
            EventKind::PlayerJump,
            EventKind::PlayerChat,
            EventKind::Tick,
        ]
        .into(),
        limits: PluginLimits::default(),
    };
    let pending = PendingPlugin {
        component: super::super::fixture::component(&engine, bodies),
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
    for (_, server) in router.iter_mut() {
        PluginManager::attach_to_world(manager.clone(), server.ecs.world_mut());
    }
    router.set_plugin_manager(manager.clone());
    manager
}
/// Log one byte from actual canonical guest execution.
fn witness() -> &'static str {
    "i32.const 1024 i32.const 120 i32.store8 i32.const 0 i32.const 1024 i32.const 1 call @log@"
}
/// Queue a controlled input, consumed through the router's ordinary shared manager.
fn scope_callback(router: &mut DimensionRouter, player: Entity) {
    router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .resource_mut::<EventBuffer>()
        .push(ServerEvent::PlayerJump { entity: player });
}
/// Inspect guest witnesses without causing a second callback or renewing fuel.
fn logs(manager: &Arc<Mutex<PluginManager>>) -> Vec<String> {
    std::mem::take(
        &mut manager
            .lock()
            .unwrap()
            .plugins
            .get_mut(&PluginId("example.global".into()))
            .unwrap()
            .store
            .data_mut()
            .test_logs,
    )
}

/// Write a fixture string through ordinary guest memory, with no extra data segments.
fn item_identifier() -> String {
    b"minecraft:diamond"
        .iter()
        .enumerate()
        .map(|(index, byte)| format!("i32.const {} i32.const {byte} i32.store8 ", 1024 + index))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_item_values_survive_callbacks_without_changing_inventory() {
    let (mut router, clients) = router();
    let before = router
        .world(0)
        .unwrap()
        .ecs
        .world()
        .get::<crate::entity::components::MainInventory>(clients[0].0)
        .unwrap()
        .0
        .slots()
        .to_vec();
    let manager = attach_scoped(
        &mut router,
        &[
            (
                "on-player-jump",
                format!(
                    r#"
            {}
            local.get 0 i32.const 1024 i32.const 17 i32.const 3 i32.const 64 call @[method]world-tx.item@
            i32.const 64 i32.load8_u if unreachable end
            i32.const 68 i32.load i32.const 4 i32.const 80 call @[method]stack.with-count@
            i32.const 80 i32.load8_u if unreachable end
            i32.const 512 i32.const 84 i32.load i32.store
            i32.const 68 i32.load call @stack_drop@
        "#,
                    item_identifier()
                ),
            ),
            (
                "on-player-chat",
                format!(
                    r#"
            i32.const 512 i32.load i32.const 96 call @[method]stack.count@
            i32.const 96 i32.load8_u if unreachable end
            i32.const 97 i32.load8_u i32.const 4 i32.ne if unreachable end
            i32.const 512 i32.load call @stack_drop@
            {} i32.const 1
        "#,
                    witness()
                ),
            ),
        ],
    );
    manager.lock().unwrap().pending[0]
        .manifest
        .capabilities
        .insert(PluginCapability::ItemValues);
    scope_callback(&mut router, clients[0].0);
    router.tick().await.unwrap();
    router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .resource_mut::<EventBuffer>()
        .push(ServerEvent::PlayerChat {
            entity: clients[0].0,
            player_id: "1".into(),
            message: "inspect".into(),
        });
    router.tick().await.unwrap();
    assert_eq!(logs(&manager), ["x"]);
    assert_eq!(
        router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get::<crate::entity::components::MainInventory>(clients[0].0)
            .unwrap()
            .0
            .slots(),
        before
    );
    assert_eq!(
        manager
            .lock()
            .unwrap()
            .plugins
            .values()
            .next()
            .unwrap()
            .store
            .data()
            .resources
            .value_budget
            .used(),
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_item_constructor_requires_its_capability() {
    let (mut router, clients) = router();
    let manager = attach_scoped(
        &mut router,
        &[(
            "on-player-jump",
            format!(
                r#"
        {}
        local.get 0 i32.const 1024 i32.const 17 i32.const 3 i32.const 64 call @[method]world-tx.item@
        i32.const 64 i32.load8_u i32.const 1 i32.ne if unreachable end
        i32.const 68 i32.load8_u i32.const 6 i32.ne if unreachable end
        {}
    "#,
                item_identifier(),
                witness()
            ),
        )],
    );
    scope_callback(&mut router, clients[0].0);
    router.tick().await.unwrap();
    assert_eq!(logs(&manager), ["x"]);
    assert_eq!(
        manager
            .lock()
            .unwrap()
            .plugins
            .values()
            .next()
            .unwrap()
            .store
            .data()
            .resources
            .value_budget
            .used(),
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn raw_guest_cannot_reuse_owned_live_player_after_the_callback() {
    let (mut router, clients) = router();
    let manager = attach_scoped(
        &mut router,
        &[
            (
                "on-player-jump",
                r#"
            local.get 1 i32.const 64 call @[method]player.to-handle@
            i32.const 64 i32.load8_u if unreachable end
            i32.const 516 i32.const 68 i32.load i32.store
            local.get 0 i32.const 68 i32.load i32.const 80 call @[method]world-tx.player@
            i32.const 80 i32.load8_u if unreachable end
            i32.const 512 i32.const 84 i32.load i32.store
        "#
                .into(),
            ),
            (
                "on-player-chat",
                format!(
                    r#"
            i32.const 512 i32.load i32.const 96 call @[method]player.health@
            i32.const 96 i32.load8_u i32.const 1 i32.ne if unreachable end
            i32.const 100 i32.load8_u if unreachable end
            local.get 0 i32.const 516 i32.load i32.const 112 call @[method]world-tx.player@
            i32.const 112 i32.load8_u if unreachable end
            i32.const 116 i32.load call @player_drop@
            i32.const 512 i32.load call @player_drop@
            i32.const 516 i32.load call @player-handle_drop@
            {} i32.const 1
        "#,
                    witness()
                ),
            ),
        ],
    );
    scope_callback(&mut router, clients[0].0);
    router.tick().await.unwrap();
    router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .resource_mut::<EventBuffer>()
        .push(ServerEvent::PlayerChat {
            entity: clients[0].0,
            player_id: "1".into(),
            message: "check".into(),
        });
    router.tick().await.unwrap();
    assert_eq!(logs(&manager), ["x"]);
}

/// Schedule exact-player work, releasing temporary resources as a real guest does.
fn schedule() -> &'static str {
    r#"
    local.get 1 i32.const 64 call @[method]player.to-handle@
    i32.const 64 i32.load8_u if unreachable end
    i32.const 68 i32.load i64.const 99 i32.const 80 call @[method]player-handle.schedule@
    i32.const 80 i32.load8_u if unreachable end
    i32.const 84 i32.load call @task_drop@
    i32.const 68 i32.load call @player-handle_drop@
"#
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scheduled_player_callback_receives_a_fresh_scope_once() {
    let (mut router, clients) = router();
    let manager = attach_scoped(
        &mut router,
        &[
            ("on-player-jump", schedule().into()),
            (
                "on-player-task",
                format!(
                    r#"
        local.get 3 i64.const 99 i64.ne if unreachable end
        local.get 1 i32.const 96 call @[method]player.health@
        i32.const 96 i32.load8_u if unreachable end
        {}
    "#,
                    witness()
                ),
            ),
        ],
    );
    scope_callback(&mut router, clients[0].0);
    router.tick().await.unwrap();
    assert_eq!(logs(&manager), ["x"]);
    router.tick().await.unwrap();
    assert!(logs(&manager).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trapping_scheduled_callback_latches_required_failure_before_further_work() {
    let (mut router, clients) = router();
    let manager = attach_scoped(
        &mut router,
        &[
            ("on-player-jump", schedule().into()),
            ("on-player-task", "unreachable".into()),
        ],
    );
    scope_callback(&mut router, clients[0].0);
    assert!(matches!(
        router.tick().await,
        Err(crate::server::worlds::WorldTickError::RequiredComponentUnavailable(_))
    ));
    assert!(manager.lock().unwrap().check_required().is_err());
    assert!(router.tick().await.is_err());
}

/// Disconnect after the local callback but before the task dispatcher.
struct Depart;
impl RouterTickHandler for Depart {
    fn on_tick(&self, router: &mut DimensionRouter, _: &WorldDirectory) {
        router.despawn_player(1);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn departed_player_reports_typed_failure_without_forging_a_live_callback() {
    let (mut router, clients) = router();
    let manager = attach_scoped(
        &mut router,
        &[
            ("on-player-jump", schedule().into()),
            ("on-player-task", "unreachable".into()),
            (
                "on-task-failed",
                format!(
                    "local.get 2 i32.const 1 i32.ne if unreachable end {}",
                    witness()
                ),
            ),
        ],
    );
    scope_callback(&mut router, clients[0].0);
    router.register_tick_handler(Depart);
    router.tick().await.unwrap();
    assert_eq!(logs(&manager), ["x"]);
    assert!(manager.lock().unwrap().check_required().is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_task_cancellation_reports_cancelled_and_never_runs_player_code() {
    let (mut router, clients) = router();
    let body = schedule().replace(
        "i32.const 84 i32.load call @task_drop@",
        r#"
        i32.const 84 i32.load i32.const 96 call @[method]task.cancel@
        i32.const 96 i32.load8_u if unreachable end
        i32.const 97 i32.load8_u i32.eqz if unreachable end
        i32.const 84 i32.load call @task_drop@
    "#,
    );
    let manager = attach_scoped(
        &mut router,
        &[
            ("on-player-jump", body),
            ("on-player-task", "unreachable".into()),
            (
                "on-task-failed",
                format!(
                    "local.get 2 i32.const 10 i32.ne if unreachable end {}",
                    witness()
                ),
            ),
        ],
    );
    scope_callback(&mut router, clients[0].0);
    router.tick().await.unwrap();
    assert_eq!(logs(&manager), ["x"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exact_connection_handle_follows_actual_world_migration_before_scheduled_execution() {
    let (mut router, clients) = router();
    let manager = attach_scoped(
        &mut router,
        &[
            ("on-player-jump", schedule().into()),
            (
                "on-player-task",
                format!(
                    r#"
            local.get 0 i32.const 64 call @[method]world-tx.world@
            i32.const 64 i32.load8_u if unreachable end
            i32.const 68 i32.load i32.const 80 call @[method]world-handle.dimension@
            i32.const 80 i32.load8_u if unreachable end
            i32.const 1024 i32.const 84 i32.load i32.const 48 i32.add i32.store8
            i32.const 0 i32.const 1024 i32.const 1 call @log@
            i32.const 68 i32.load call @world-handle_drop@
            local.get 1 i32.const 96 call @[method]player.availability@
            i32.const 96 i32.load8_u if unreachable end
            {}
        "#,
                    witness()
                ),
            ),
        ],
    );
    scope_callback(&mut router, clients[0].0);
    crate::server::worlds::request_transfer_checked(
        router.world_mut(0).unwrap().ecs.world_mut(),
        clients[0].0,
        1,
        glam::DVec3::new(4.5, 16.0, 4.5),
        0,
        "scope-migration".into(),
        |_, _, _| true,
    )
    .unwrap();
    router.tick().await.unwrap();
    let observed = logs(&manager);
    assert_eq!(
        router.locate_session(1).unwrap().0,
        1,
        "guest witnesses: {observed:?}"
    );
    assert_eq!(observed, ["1", "x"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replaced_world_with_same_dimension_reports_typed_world_closed_failure() {
    let (mut router, _clients) = router();
    let manager = attach_scoped(&mut router, &[
        ("on-load", r#"
            i32.const 64 call @worlds@
            i32.const 64 i32.load8_u if unreachable end
            i32.const 68 i32.load i32.load i64.const 9 i32.const 96 call @[method]world-handle.schedule@
            i32.const 96 i32.load8_u if unreachable end
            i32.const 100 i32.load call @task_drop@
        "#.into()),
        ("on-world-task", "unreachable".into()),
        ("on-task-failed", format!("local.get 2 i32.const 3 i32.ne if unreachable end {}", witness())),
    ]);
    // Initialization belongs to a real world lifetime absent from this router.
    let mut previous = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..Default::default()
    });
    manager
        .lock()
        .unwrap()
        .initialize_startup(previous.ecs.world_mut())
        .unwrap();
    router.tick().await.unwrap();
    assert_eq!(logs(&manager), ["x"]);
}

#[path = "inventory.rs"]
mod inventory_tests;

#[path = "inventory_writes.rs"]
mod inventory_write_tests;

#[path = "item_comparisons.rs"]
mod item_comparison_tests;
#[path = "state_restoration.rs"]
mod state_restoration_tests;

/// The actual component receives an error until a runtime has installed a real offer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resource_pack_offer_is_unavailable_before_runtime_installation() {
    let (mut router, _clients) = router();
    let body = format!(
        "i32.const 512 call @resource-packs@ i32.const 512 i32.load8_u i32.const 1 i32.ne if unreachable end {}",
        witness()
    );
    let manager = attach_scoped(&mut router, &[("on-load", body)]);
    router.initialize_plugins().unwrap();
    assert_eq!(logs(&manager), ["x"]);
}

/// An installed empty offer is a successful fact, not the absence of runtime configuration.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resource_pack_offer_distinguishes_an_actual_empty_stack_from_unavailability() {
    let (mut router, _clients) = router();
    let offer =
        crate::server::resource_packs::load(&crate::config::ResourcePackConfig::default(), false)
            .unwrap();
    for (_, world) in router.iter_mut() {
        world
            .ecs
            .world_mut()
            .insert_resource(crate::server::resource_packs::ConfiguredPacks(
                offer.clone(),
            ));
    }
    let body = format!(
        "i32.const 512 call @resource-packs@ i32.const 512 i32.load8_u if unreachable end i32.const 516 i32.load8_u if unreachable end i32.const 524 i32.load if unreachable end {}",
        witness()
    );
    let manager = attach_scoped(&mut router, &[("on-load", body)]);
    router.initialize_plugins().unwrap();
    assert_eq!(logs(&manager), ["x"]);
}

/// Check canonical record/list/string lowering against actual original loaded archives.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resource_pack_offer_reports_the_immutable_required_stack_in_actual_component_callbacks() {
    use crate::server::resource_packs::{
        self, ConfiguredPacks,
        tests::{archive, configured, document},
    };
    use serde_json::json;
    let first = archive(
        document(10, json!([1, 2, 3])).to_string().as_bytes(),
        "manifest.json",
    );
    let second = archive(
        document(20, json!([4, 5, 6])).to_string().as_bytes(),
        "manifest.json",
    );
    let (directory, mut config) = configured(&first);
    let path = directory.path().join("second.mcpack");
    std::fs::write(&path, second).unwrap();
    config.paths.push(path);
    let offer = resource_packs::load(&config, true).unwrap();
    let mut body = String::from(
        "i32.const 512 call @resource-packs@ i32.const 512 i32.load8_u if unreachable end i32.const 516 i32.load8_u i32.const 1 i32.ne if unreachable end i32.const 524 i32.load i32.const 2 i32.ne if unreachable end ",
    );
    for (index, pack) in offer.packs.iter().enumerate() {
        let fields = [
            (0, pack.uuid.to_string().into_bytes()),
            (8, pack.version.as_bytes().to_vec()),
            (16, pack.sha256().to_vec()),
        ];
        for (offset, bytes) in fields {
            let field = index * 24 + offset;
            body.push_str(&format!("i32.const 520 i32.load i32.const {} i32.add i32.load i32.const {} i32.ne if unreachable end ", field+4, bytes.len()));
            for (byte_index, byte) in bytes.iter().enumerate() {
                body.push_str(&format!("i32.const 520 i32.load i32.const {field} i32.add i32.load i32.const {byte_index} i32.add i32.load8_u i32.const {byte} i32.ne if unreachable end "));
            }
        }
    }
    body.push_str(witness());
    let (mut router, _clients) = router();
    for (_, world) in router.iter_mut() {
        world
            .ecs
            .world_mut()
            .insert_resource(ConfiguredPacks(offer.clone()));
    }
    // Mutating paths cannot turn the loaded metadata into a fresh or partial offer.
    for path in &config.paths {
        std::fs::write(path, b"changed after load").unwrap();
    }
    let manager = attach_scoped(&mut router, &[("on-load", body.clone()), ("on-tick", body)]);
    router.initialize_plugins().unwrap();
    assert_eq!(logs(&manager), ["x"]);
    router.tick().await.unwrap();
    assert_eq!(logs(&manager), ["x"]);
}

/// A malformed physical player cannot disappear from an apparently complete fresh enumeration.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scoped_player_enumeration_rejects_malformed_actors_instead_of_returning_a_partial_list() {
    let (mut router, clients) = router();
    let malformed = router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .spawn(crate::entity::components::Player)
        .id();
    let body = format!(
        "local.get 0 i32.const 512 call @[method]world-tx.players@ i32.const 512 i32.load8_u i32.const 768 i32.load8_u i32.const 1 i32.xor i32.ne if unreachable end i32.const 768 i32.const 1 i32.store8 {}",
        witness()
    );
    let manager = attach_scoped(&mut router, &[("on-player-jump", body)]);
    scope_callback(&mut router, clients[0].0);
    router.tick().await.unwrap();
    assert_eq!(logs(&manager), ["x"]);
    router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .despawn(malformed);
    scope_callback(&mut router, clients[0].0);
    router.tick().await.unwrap();
    assert_eq!(logs(&manager), ["x"]);
}
