//! Real components distinguish finite initialization capacity from callback budgets.

use super::*;

/// Minimal physical world required by the ordinary scoped invocation wrapper.
fn world() -> bevy_ecs::world::World {
    let mut world = bevy_ecs::world::World::new();
    world.insert_resource(crate::ecs::TickCounter { current: 42 });
    world.insert_resource(crate::world::ChunkManager::new(
        crate::world::WorldConfig::void_spawn_platform(1),
    ));
    world.insert_resource(crate::ecs::events::ActionQueue::default());
    world.insert_resource(crate::ecs::events::EventBuffer::default());
    world
}

/// Use the complete production canonical host imports.
fn linker(engine: &Engine) -> Linker<HostContext> {
    let mut linker = Linker::new(engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker).unwrap();
    UnastarPlugin::add_to_linker::<HostContext, HostContext>(&mut linker, |ctx| ctx).unwrap();
    linker
}

/// Finite guest work costs actual VM fuel without relying on optimizer instruction counts.
fn finite_loop(local: u32, iterations: u32) -> String {
    format!(
        "(local i32) i32.const {iterations} local.set {local} loop $again local.get {local} i32.const 1 i32.sub local.tee {local} br_if $again end"
    )
}

/// One ordinary manager starts a required fixture using the complete production path.
fn manager(engine: Engine, component: Component, limits: PluginLimits) -> PluginManager {
    let store = store(&engine, &limits);
    PluginManager {
        engine,
        plugins: Default::default(),
        pending: vec![PendingPlugin {
            component,
            store,
            manifest: PluginManifest {
                id: PluginId("example.budget".into()),
                name: "Budget".into(),
                version: "1".into(),
                api_version: PLUGIN_API_VERSION.into(),
                required: true,
                config: None,
                capabilities: Default::default(),
                subscriptions: [crate::ecs::events::EventKind::Tick].into(),
                limits,
            },
        }],
        tick_prepared: false,
        tick_budget_valid: true,
        readiness: Default::default(),
        storage_registry: Default::default(),
        archive_registry: Default::default(),
        command_roots: Default::default(),
        ui_registry: Default::default(),
        view_registry: Default::default(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_can_initialize_above_its_ordinary_cycle_fuel() {
    let engine = engine();
    let component = fixture::component(
        &engine,
        &[
            ("on-load", finite_loop(0, 1000)),
            ("on-tick", finite_loop(1, 1000)),
        ],
    );
    let limits = PluginLimits {
        fuel_per_tick: 100,
        fuel_per_initialization: 1_000_000,
        ..Default::default()
    };
    let mut manager = manager(engine, component, limits);
    let mut world = world();
    assert!(manager.initialize_startup(&mut world).is_ok());
    assert_eq!(manager.plugins.len(), 1);
    assert!(manager.begin_tick(&mut world));
    manager.tick(&mut world);
    let reason = manager.check_required().unwrap_err();
    assert!(reason.contains("fuel"), "{reason}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_unlimited_fuel_runs_real_initialization_and_repeated_cycles_above_the_default_cap()
 {
    let engine = engine();
    let component = fixture::component(
        &engine,
        &[
            ("on-load", finite_loop(0, 4_000_000)),
            ("on-tick", finite_loop(1, 4_000_000)),
        ],
    );
    let limits = PluginLimits {
        fuel_per_tick: 0,
        fuel_per_initialization: 0,
        callback_timeout_ms: 1000,
        ..Default::default()
    };
    let encoded = toml_edit::ser::to_string(&limits).unwrap();
    let restored: PluginLimits = toml_edit::de::from_str(&encoded).unwrap();
    assert_eq!(restored.fuel_per_tick, 0);
    assert_eq!(restored.fuel_per_initialization, 0);
    let mut manager = manager(engine, component, restored);
    let mut world = world();
    manager.initialize_startup(&mut world).unwrap();
    let spent = u64::MAX
        - manager
            .plugins
            .values_mut()
            .next()
            .unwrap()
            .store
            .get_fuel()
            .unwrap();
    assert!(spent > PluginLimits::default().fuel_per_tick);
    for _ in 0..2 {
        assert!(manager.begin_tick(&mut world));
        manager.tick(&mut world);
        manager.check_required().unwrap();
        let spent = u64::MAX
            - manager
                .plugins
                .values_mut()
                .next()
                .unwrap()
                .store
                .get_fuel()
                .unwrap();
        assert!(spent > PluginLimits::default().fuel_per_tick);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initialization_and_cycle_fuel_opt_out_independently() {
    for (initial, cycle) in [(0, 100), (100, 0)] {
        let engine = engine();
        let component = fixture::component(
            &engine,
            &[
                ("on-load", finite_loop(0, 1000)),
                ("on-tick", finite_loop(1, 1000)),
            ],
        );
        let limits = PluginLimits {
            fuel_per_initialization: initial,
            fuel_per_tick: cycle,
            ..Default::default()
        };
        let mut manager = manager(engine, component, limits);
        let mut world = world();
        if initial == 0 {
            manager.initialize_startup(&mut world).unwrap();
            assert!(manager.begin_tick(&mut world));
            manager.tick(&mut world);
            assert!(manager.check_required().unwrap_err().contains("fuel"));
        } else {
            assert!(
                manager
                    .initialize_startup(&mut world)
                    .unwrap_err()
                    .contains("fuel")
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_start_function_and_on_load_share_one_initialization_fuel_allowance() {
    let engine = engine();
    let bodies = [("on-load", finite_loop(0, 1000))];
    let component = fixture::component(&engine, &bodies);
    let limits = PluginLimits::default();
    let mut calibration = store(&engine, &limits);
    let mut world = world();
    let (instantiation, call) = with_world(&mut calibration, &mut world, |store| {
        begin_initialization_budget(store, &limits).unwrap();
        let before = store.get_fuel().unwrap();
        let bindings =
            UnastarPlugin::instantiate(&mut *store, &component, &linker(&engine)).unwrap();
        let after_instantiation = store.get_fuel().unwrap();
        bindings.call_on_load(&mut *store).unwrap();
        (
            before - after_instantiation,
            after_instantiation - store.get_fuel().unwrap(),
        )
    });
    assert!(call > 1);
    let limits = PluginLimits {
        fuel_per_initialization: instantiation + 2 * call - 1,
        ..Default::default()
    };
    let component = fixture::component_with_start(&engine, &bodies);
    let mut manager = manager(engine, component, limits);
    let reason = manager.initialize_startup(&mut world).unwrap_err();
    assert!(reason.contains("fuel"), "{reason}");
    assert!(manager.plugins.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_epoch_allowance_does_not_enlarge_the_next_callback_deadline() {
    let engine = engine();
    let body = |local| {
        format!(
            "(local i32) i32.const 0 i32.const 1024 i32.const 0 call @log@ {}",
            finite_loop(local, 2).replace("(local i32)", "")
        )
    };
    let component = fixture::component(&engine, &[("on-load", body(0)), ("on-tick", body(1))]);
    let limits = PluginLimits {
        initialization_timeout_ms: 1000,
        callback_timeout_ms: 10,
        ..Default::default()
    };
    let mut store = store(&engine, &limits);
    store.data_mut().test_log_epoch_engine = Some(engine.clone());
    let mut world = world();
    let bindings = with_world(&mut store, &mut world, |store| {
        begin_initialization_budget(store, &limits).unwrap();
        let bindings =
            UnastarPlugin::instantiate(&mut *store, &component, &linker(&engine)).unwrap();
        bindings.call_on_load(&mut *store).unwrap();
        bindings
    });
    let error = with_world(&mut store, &mut world, |store| {
        begin_fuel_budget(store, &limits).unwrap();
        prepare_invocation(store);
        bindings.call_on_tick(store, 42).unwrap_err()
    });
    assert!(wasm_failure_reason(&error).contains("deadline"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unlimited_instruction_fuel_does_not_disable_the_real_callback_epoch_deadline() {
    let engine = engine();
    let body = |local| {
        format!(
            "(local i32) i32.const 0 i32.const 1024 i32.const 0 call @log@ {}",
            finite_loop(local, 2).replace("(local i32)", "")
        )
    };
    let component = fixture::component(&engine, &[("on-load", body(0)), ("on-tick", body(1))]);
    let limits = PluginLimits {
        fuel_per_tick: 0,
        fuel_per_initialization: 0,
        initialization_timeout_ms: 1000,
        callback_timeout_ms: 10,
        ..Default::default()
    };
    let mut store = store(&engine, &limits);
    store.data_mut().test_log_epoch_engine = Some(engine.clone());
    let mut world = world();
    let bindings = with_world(&mut store, &mut world, |store| {
        begin_initialization_budget(store, &limits).unwrap();
        let bindings =
            UnastarPlugin::instantiate(&mut *store, &component, &linker(&engine)).unwrap();
        bindings.call_on_load(&mut *store).unwrap();
        bindings
    });
    let error = with_world(&mut store, &mut world, |store| {
        begin_fuel_budget(store, &limits).unwrap();
        prepare_invocation(store);
        bindings.call_on_tick(store, 42).unwrap_err()
    });
    assert!(wasm_failure_reason(&error).starts_with("guest execution deadline exhausted"));
}

#[test]
fn execution_limits_roundtrip_and_invalid_allowances_fail_before_guest_execution() {
    let limits = PluginLimits {
        fuel_per_initialization: 123456,
        initialization_timeout_ms: 750,
        callback_timeout_ms: 25,
        ..Default::default()
    };
    let encoded = toml_edit::ser::to_string(&limits).unwrap();
    let restored: PluginLimits = toml_edit::de::from_str(&encoded).unwrap();
    assert_eq!(restored.fuel_per_initialization, 123456);
    assert_eq!(restored.initialization_timeout_ms, 750);
    assert_eq!(restored.callback_timeout_ms, 25);
    for limits in [
        PluginLimits {
            initialization_timeout_ms: 0,
            ..Default::default()
        },
        PluginLimits {
            initialization_timeout_ms: 60001,
            ..Default::default()
        },
        PluginLimits {
            callback_timeout_ms: 0,
            ..Default::default()
        },
        PluginLimits {
            callback_timeout_ms: 1001,
            ..Default::default()
        },
    ] {
        assert!(
            create_store(
                &engine(),
                WasiCtx::builder().build(),
                PluginId("example.invalid".into()),
                &limits
            )
            .is_err()
        );
    }
}
