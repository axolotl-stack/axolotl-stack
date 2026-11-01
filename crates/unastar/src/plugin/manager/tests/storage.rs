use super::*;
use crate::plugin::storage::{Blob, Limits, Registry};
use std::sync::{Arc, Mutex};

/// Exercise real canonical string/list/result imports without requiring a Wasm toolchain.
fn storage_component(engine: &Engine, load: &str, tick: &str) -> Component {
    let mut source = component_source(
        true,
        "i32.const 32 i32.const 1 i32.store i32.const 36 local.get 4 f32.store i32.const 32",
        "i32.const 64 i32.const 0 i32.store i32.const 64",
        tick,
        load,
    );
    source = source.replace("(export \"get-tick-id\" (func (result u64)))", "(export \"configuration\" (func (result (list u8)))) (export \"get-tick-id\" (func (result u64)))");
    let imports = format!(
        r#"
        (import "unastar:plugin/storage@{PLUGIN_API_VERSION}" (instance $storage
            (type $blob-record (record (field "revision" u64) (field "bytes" (list u8))))
            (export "blob" (type $blob (eq $blob-record)))
            (type $error-enum (enum "invalid-key" "quota-exceeded" "conflict" "io-error" "uncertain"))
            (export "storage-error" (type $error (eq $error-enum)))
            (export "read" (func (param "key" string) (result (result (option $blob) (error $error)))))
            (export "compare-exchange" (func (param "key" string) (param "expected-revision" (option u64)) (param "bytes" (list u8)) (result (result u64 (error $error)))))))
        (alias export $storage "read" (func $read))
        (alias export $storage "compare-exchange" (func $cas))
        (alias export $host "configuration" (func $configuration))
        (core module $memory-module
            (memory (export "memory") 1)
            (data (i32.const 0) "state")
            (data (i32.const 16) "first")
            (data (i32.const 32) "next")
            (global $heap (mut i32) (i32.const 4096))
            (func (export "realloc") (param i32 i32 i32 i32) (result i32)
                (local $pointer i32)
                global.get $heap local.get 2 i32.const 1 i32.sub i32.add
                i32.const 0 local.get 2 i32.sub i32.and local.tee $pointer
                local.get 3 i32.add global.set $heap local.get $pointer))
        (core instance $memory (instantiate $memory-module))
        (core func $read-lowered (canon lower (func $read) (memory $memory "memory") (realloc (func $memory "realloc"))))
        (core func $cas-lowered (canon lower (func $cas) (memory $memory "memory") (realloc (func $memory "realloc"))))
        (core func $configuration-lowered (canon lower (func $configuration) (memory $memory "memory") (realloc (func $memory "realloc"))))
    "#
    );
    source = source.replacen(
        "(core module $module",
        &(imports + "(core module $module"),
        1,
    );
    source = source.replace(
        "(memory (export \"memory\") 1)\n                (global $damage-calls",
        "(import \"host\" \"read\" (func $read (param i32 i32 i32)))\n(import \"host\" \"cas\" (func $cas (param i32 i32 i32 i64 i32 i32 i32)))\n(import \"host\" \"configuration\" (func $configuration (param i32)))\n(import \"mem\" \"memory\" (memory 1))\n(import \"mem\" \"realloc\" (func $realloc (param i32 i32 i32 i32) (result i32)))\n(export \"memory\" (memory 0))\n(export \"realloc\" (func $realloc))\n(global $damage-calls",
    );
    source = source.replace(
        "(func (export \"realloc\") (param i32 i32 i32 i32) (result i32) i32.const 0)",
        "",
    );
    source = source.replace(
        "(export \"drop-player\" (func $player-drop)))",
        "(export \"read\" (func $read-lowered)) (export \"cas\" (func $cas-lowered)) (export \"configuration\" (func $configuration-lowered)) (export \"drop-player\" (func $player-drop)))",
    );
    source = source.replace(
        "(instantiate $module (with \"host\" (instance $host-core)))",
        "(instantiate $module (with \"host\" (instance $host-core)) (with \"mem\" (instance $memory)))",
    );
    Component::new(engine, source).unwrap()
}

/// Canonical ABI result acknowledgement must be checked by the guest itself.
const CREATE: &str = r#"
    i32.const 0 i32.const 5 i32.const 0 i64.const 0 i32.const 16 i32.const 5 i32.const 128 call $cas
    i32.const 128 i32.load if unreachable end
    i32.const 136 i64.load i64.const 1 i64.ne if unreachable end
"#;

/// Read the actual returned blob, including its owned bytes and revision.
const READ: &str = r#"
    i32.const 0 i32.const 5 i32.const 160 call $read
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
