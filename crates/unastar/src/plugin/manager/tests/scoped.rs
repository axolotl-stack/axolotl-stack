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
            EventKind::PlayerJoin,
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
        command_roots: Default::default(),
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
fn join(router: &mut DimensionRouter, player: Entity) {
    router
        .world_mut(0)
        .unwrap()
        .ecs
        .world_mut()
        .resource_mut::<EventBuffer>()
        .push(ServerEvent::PlayerJoin {
            entity: player,
            player_id: "1".into(),
            username: "Player0".into(),
        });
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

#[tokio::test]
async fn raw_guest_cannot_reuse_owned_live_player_after_the_callback() {
    let (mut router, clients) = router();
    let manager = attach_scoped(
        &mut router,
        &[
            (
                "on-player-join",
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
    join(&mut router, clients[0].0);
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

#[tokio::test]
async fn scheduled_player_callback_receives_a_fresh_scope_once() {
    let (mut router, clients) = router();
    let manager = attach_scoped(
        &mut router,
        &[
            ("on-player-join", schedule().into()),
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
    join(&mut router, clients[0].0);
    router.tick().await.unwrap();
    assert_eq!(logs(&manager), ["x"]);
    router.tick().await.unwrap();
    assert!(logs(&manager).is_empty());
}

#[tokio::test]
async fn trapping_scheduled_callback_latches_required_failure_before_further_work() {
    let (mut router, clients) = router();
    let manager = attach_scoped(
        &mut router,
        &[
            ("on-player-join", schedule().into()),
            ("on-player-task", "unreachable".into()),
        ],
    );
    join(&mut router, clients[0].0);
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

#[tokio::test]
async fn departed_player_reports_typed_failure_without_forging_a_live_callback() {
    let (mut router, clients) = router();
    let manager = attach_scoped(
        &mut router,
        &[
            ("on-player-join", schedule().into()),
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
    join(&mut router, clients[0].0);
    router.register_tick_handler(Depart);
    router.tick().await.unwrap();
    assert_eq!(logs(&manager), ["x"]);
    assert!(manager.lock().unwrap().check_required().is_ok());
}

#[tokio::test]
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
            ("on-player-join", body),
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
    join(&mut router, clients[0].0);
    router.tick().await.unwrap();
    assert_eq!(logs(&manager), ["x"]);
}

#[tokio::test]
async fn exact_connection_handle_follows_actual_world_migration_before_scheduled_execution() {
    let (mut router, clients) = router();
    let manager = attach_scoped(
        &mut router,
        &[
            ("on-player-join", schedule().into()),
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
    join(&mut router, clients[0].0);
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

#[tokio::test]
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
