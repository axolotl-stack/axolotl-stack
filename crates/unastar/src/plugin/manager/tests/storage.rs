use super::*;
use crate::plugin::storage::{Blob, Limits, Registry};
use std::sync::{Arc, Mutex};

/// Exercise real canonical string/list/result imports without requiring a Wasm toolchain.
fn storage_component(engine: &Engine, load: &str, tick: &str) -> Component {
    /// Keep fixture strings outside the canonical allocator's bookkeeping.
    fn literal(address: usize, value: &str) -> String {
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
    let load = format!(
        "{} {} {} {}",
        literal(1024, "state"),
        literal(1040, "first"),
        literal(1056, "next"),
        load
    )
    .replace("call $cas", "call @compare-exchange@")
    .replace("call $read", "call @read@")
    .replace("call $configuration", "call @configuration@");
    fixture::component(engine, &[("on-load", load), ("on-tick", tick.into())])
}

/// Canonical ABI result acknowledgement must be checked by the guest itself.
const CREATE: &str = r#"
    i32.const 1024 i32.const 5 i32.const 0 i64.const 0 i32.const 1040 i32.const 5 i32.const 128 call $cas
    i32.const 128 i32.load if unreachable end
    i32.const 136 i64.load i64.const 1 i64.ne if unreachable end
"#;

/// Read the actual returned blob, including its owned bytes and revision.
const READ: &str = r#"
    i32.const 1024 i32.const 5 i32.const 160 call $read
    i32.const 160 i32.load if unreachable end
    i32.const 168 i32.load i32.const 1 i32.ne if unreachable end
    i32.const 176 i64.load i64.const 1 i64.ne if unreachable end
    i32.const 188 i32.load i32.const 5 i32.ne if unreachable end
    i32.const 184 i32.load i32.load8_u i32.const 102 i32.ne if unreachable end
"#;

/// A minimal manager owns the same namespace pool as its pending component.
fn manager(
    engine: Engine,
    component: Component,
    registry: Arc<Mutex<Registry>>,
    configuration: Vec<u8>,
) -> PluginManager {
    let limits = PluginLimits::default();
    let mut store = create_store(
        &engine,
        WasiCtx::builder().build(),
        PluginId("example.store".into()),
        &limits,
    )
    .unwrap();
    store.data_mut().storage_registry = registry.clone();
    store.data_mut().configuration = configuration;
    let manifest = PluginManifest {
        id: PluginId("example.store".into()),
        name: "Example".into(),
        version: "1".into(),
        api_version: PLUGIN_API_VERSION.into(),
        required: true,
        config: None,
        capabilities: Default::default(),
        subscriptions: [crate::ecs::events::EventKind::Tick].into(),
        limits,
    };
    let mut manager = PluginManager {
        engine,
        plugins: HashMap::new(),
        pending: vec![PendingPlugin {
            manifest,
            component,
            store,
        }],
        tick_prepared: false,
        tick_budget_valid: true,
        readiness: Default::default(),
        command_roots: Default::default(),
        ui_registry: Default::default(),
        view_registry: Default::default(),
        storage_registry: registry,
    };
    manager.require_plugins(&["example.store".into()]);
    manager
}

/// No player, filesystem grant or live foreign-world access is needed for storage.
fn world() -> bevy_ecs::world::World {
    let mut world = bevy_ecs::world::World::new();
    world.insert_resource(crate::ecs::resources::TickCounter { current: 42 });
    world.insert_resource(crate::ecs::events::ActionQueue::default());
    world.insert_resource(crate::ecs::events::EventBuffer::default());
    world
}

#[test]
fn actual_component_stores_and_restores_state_without_filesystem_permission() {
    let root = tempfile::tempdir().unwrap();
    let registry = Arc::new(Mutex::new(Registry::new(
        root.path().to_owned(),
        Limits::default(),
    )));
    let engine = engine();
    let cfg = "i32.const 208 call $configuration i32.const 212 i32.load i32.const 3 i32.ne if unreachable end";
    let component = storage_component(&engine, &format!("{CREATE}{READ}{cfg}"), "");
    let mut active = manager(engine.clone(), component, registry.clone(), b"cfg".to_vec());
    let mut world = world();
    assert!(active.begin_tick(&mut world));
    assert!(active.check_required().is_ok());
    assert_eq!(active.plugins.len(), 1);
    assert!(
        active
            .plugins
            .values()
            .all(|plugin| plugin.manifest.capabilities.is_empty())
    );
    drop(active);
    let component = storage_component(&engine, READ, "");
    let mut restarted = manager(engine, component, registry.clone(), Vec::new());
    assert!(restarted.begin_tick(&mut world));
    assert_eq!(restarted.plugins.len(), 1);
    let namespace = registry.lock().unwrap().namespace("example.store").unwrap();
    assert_eq!(
        namespace.lock().unwrap().read("state"),
        Ok(Some(Blob {
            revision: 1,
            bytes: b"first".to_vec()
        }))
    );
}

#[test]
fn committed_storage_is_recoverable_after_a_component_trap() {
    let root = tempfile::tempdir().unwrap();
    let registry = Arc::new(Mutex::new(Registry::new(
        root.path().to_owned(),
        Limits::default(),
    )));
    let engine = engine();
    let component = storage_component(&engine, &format!("{CREATE} unreachable"), "");
    let mut active = manager(engine, component, registry.clone(), Vec::new());
    let mut world = world();
    assert!(!active.begin_tick(&mut world));
    assert!(active.check_required().is_err());
    assert!(active.plugins.is_empty());
    assert!(
        world
            .resource_mut::<crate::ecs::events::ActionQueue>()
            .drain()
            .is_empty()
    );
    let namespace = registry.lock().unwrap().namespace("example.store").unwrap();
    assert_eq!(
        namespace.lock().unwrap().read("state"),
        Ok(Some(Blob {
            revision: 1,
            bytes: b"first".to_vec()
        }))
    );
}

#[test]
fn storage_acknowledgement_does_not_replenish_guest_fuel() {
    let root = tempfile::tempdir().unwrap();
    let registry = Arc::new(Mutex::new(Registry::new(
        root.path().to_owned(),
        Limits::default(),
    )));
    let engine = engine();
    let component = storage_component(&engine, CREATE, "(loop $spin br $spin)");
    let mut active = manager(engine, component, registry, Vec::new());
    let mut world = world();
    assert!(active.begin_tick(&mut world));
    let plugin = active.plugins.values_mut().next().unwrap();
    plugin.store.set_fuel(1000).unwrap();
    active.tick(&mut world);
    assert!(active.check_required().is_err());
}
