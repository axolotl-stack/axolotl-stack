//! Required readiness is exercised by loading and running real components.

use super::*;
use crate::ecs::TickCounter;
use crate::ecs::events::{ActionQueue, EventBuffer};
use std::path::{Path, PathBuf};

const KEEP_DAMAGE: &str =
    "i32.const 32 i32.const 1 i32.store i32.const 36 local.get 4 f32.store i32.const 32";
const KEEP_POSITION: &str = "i32.const 64 i32.const 1 i32.store i32.const 72 local.get 1 f64.store i32.const 80 local.get 2 f64.store i32.const 88 local.get 3 f64.store i32.const 64";

/// Build a manager without a background epoch task for deterministic tests.
fn manager() -> PluginManager {
    PluginManager {
        engine: engine(),
        plugins: Default::default(),
        pending: Vec::new(),
        tick_prepared: false,
        tick_budget_valid: true,
        readiness: Default::default(),
    }
}
/// A minimal world that supplies the host imports used during initialization.
fn world() -> bevy_ecs::world::World {
    let mut world = bevy_ecs::world::World::new();
    world.insert_resource(TickCounter { current: 42 });
    world.insert_resource(crate::world::ChunkManager::new(
        crate::world::WorldConfig::void_spawn_platform(1),
    ));
    world.insert_resource(ActionQueue::default());
    world.insert_resource(EventBuffer::default());
    world
}
/// Write a public component manifest and executable component text fixture.
fn plugin(root: &Path, directory: &str, id: &str, required: bool, load: &str) -> PathBuf {
    let directory = root.join(directory);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("plugin.toml"), format!("id = {id:?}\nname = \"Example\"\nversion = \"1\"\napi_version = {PLUGIN_API_VERSION:?}\nrequired = {required}\ncapabilities = [\"read-players\",\"schedule-tasks\"]\n")).unwrap();
    std::fs::write(
        directory.join("plugin.wasm"),
        component_source(true, KEEP_DAMAGE, KEEP_POSITION, "", load),
    )
    .unwrap();
    directory
}

#[test]
fn explicit_required_identity_cannot_disappear_or_load_a_corrupt_component() {
    for variant in ["missing", "manifest", "module", "version"] {
        let root = tempfile::tempdir().unwrap();
        if variant != "missing" {
            let dir = plugin(root.path(), "example", "example.required", false, "");
            match variant {
                "manifest" => std::fs::write(dir.join("plugin.toml"), "invalid = [").unwrap(),
                "module" => std::fs::write(dir.join("plugin.wasm"), b"not a component").unwrap(),
                "version" => {
                    let path = dir.join("plugin.toml");
                    let text = std::fs::read_to_string(&path)
                        .unwrap()
                        .replace(PLUGIN_API_VERSION, "99.0.0");
                    std::fs::write(path, text).unwrap();
                }
                _ => unreachable!(),
            }
        }
        let mut manager = manager();
        manager.require_plugins(&["example.required".into()]);
        assert!(manager.load_plugins(root.path()).is_err(), "{variant}");
        assert!(manager.initialize_startup(&mut world()).is_err());
        assert!(manager.plugins.is_empty());
    }
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager();
    manager.require_plugins(&["example.required".into()]);
    assert!(manager.load_plugins(&root.path().join("absent")).is_err());
}

#[test]
fn manifest_required_compile_and_initialization_failures_are_sticky() {
    for malformed_module in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let dir = plugin(
            root.path(),
            "example",
            "example.required",
            true,
            r#"
                i32.const 64 call @worlds@
                i32.const 64 i32.load8_u if unreachable end
                i32.const 68 i32.load i32.load i64.const 9 i32.const 96 call @[method]world-handle.schedule@
                i32.const 96 i32.load8_u if unreachable end
                unreachable
            "#,
        );
        if malformed_module {
            std::fs::write(dir.join("plugin.wasm"), b"invalid").unwrap();
        }
        let mut manager = manager();
        let loaded = manager.load_plugins(root.path());
        assert_eq!(loaded.is_err(), malformed_module);
        let mut world = world();
        world.insert_resource(manager.readiness.clone());
        assert!(!readiness::admission_allowed(&world));
        assert!(manager.initialize_startup(&mut world).is_err());
        assert!(manager.plugins.is_empty());
        assert!(world.resource_mut::<ActionQueue>().drain().is_empty());
        assert!(!manager.begin_tick(&mut world));
        manager.end_tick();
        assert!(!manager.begin_tick(&mut world));
        assert!(!readiness::admission_allowed(&world));
    }
}

#[test]
fn optional_broken_components_preserve_startup_and_discard_their_actions() {
    for corrupt_module in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let dir = plugin(
            root.path(),
            "example",
            "example.optional",
            false,
            r#"
                i32.const 64 call @worlds@
                i32.const 64 i32.load8_u if unreachable end
                i32.const 68 i32.load i32.load i64.const 9 i32.const 96 call @[method]world-handle.schedule@
                i32.const 96 i32.load8_u if unreachable end
                unreachable
            "#,
        );
        if corrupt_module {
            std::fs::write(dir.join("plugin.wasm"), b"invalid").unwrap();
        }
        let mut manager = manager();
        manager.load_plugins(root.path()).unwrap();
        let mut world = world();
        world.insert_resource(manager.readiness.clone());
        manager.initialize_startup(&mut world).unwrap();
        assert!(readiness::admission_allowed(&world));
        assert!(world.resource_mut::<ActionQueue>().drain().is_empty());
    }
}

#[test]
fn required_startup_preloads_immutable_opaque_configuration_without_filesystem_grant() {
    let root = tempfile::tempdir().unwrap();
    let dir = plugin(
        root.path(),
        "example",
        "example.required",
        true,
        "call $tick i64.const 42 i64.ne if unreachable end",
    );
    let manifest = dir.join("plugin.toml");
    let mut text = std::fs::read_to_string(&manifest).unwrap();
    text.push_str("config = \"settings.bin\"\n");
    std::fs::write(&manifest, text).unwrap();
    let bytes = [0, 255, b'{', b'\n'];
    std::fs::write(dir.join("settings.bin"), bytes).unwrap();
    let mut manager = manager();
    manager.load_plugins(root.path()).unwrap();
    assert_eq!(manager.pending[0].store.data().configuration, bytes);
    assert!(
        !manager.pending[0]
            .manifest
            .capabilities
            .contains(&crate::plugin::manifest::PluginCapability::Filesystem)
    );
    std::fs::write(dir.join("settings.bin"), b"changed").unwrap();
    let mut world = world();
    world.insert_resource(manager.readiness.clone());
    assert!(!readiness::admission_allowed(&world));
    manager.initialize_startup(&mut world).unwrap();
    assert!(readiness::admission_allowed(&world));
    let plugin = manager.plugins.values().next().unwrap();
    assert_eq!(plugin.store.data().configuration, bytes);
    assert!(plugin.store.data().world_ptr.is_null());
}

#[test]
fn configuration_is_relative_contained_regular_and_bounded() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("plugin");
    std::fs::create_dir(&dir).unwrap();
    assert!(readiness::configuration(&dir, None).unwrap().is_empty());
    std::fs::write(root.path().join("outside"), b"secret").unwrap();
    std::fs::write(dir.join("large"), vec![0; 1_048_577]).unwrap();
    std::fs::write(dir.join("limit"), vec![7; 1_048_576]).unwrap();
    for path in [
        Path::new(""),
        Path::new("missing"),
        Path::new("../outside"),
        Path::new("."),
        Path::new("large"),
        &root.path().join("outside"),
    ] {
        assert!(
            readiness::configuration(&dir, Some(path)).is_err(),
            "{path:?}"
        );
    }
    assert_eq!(
        readiness::configuration(&dir, Some(Path::new("limit")))
            .unwrap()
            .len(),
        1_048_576
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.path().join("outside"), dir.join("link")).unwrap();
        assert!(readiness::configuration(&dir, Some(Path::new("link"))).is_err());
    }
}

#[test]
fn duplicate_required_component_id_is_fatal() {
    let root = tempfile::tempdir().unwrap();
    plugin(root.path(), "first", "example.required", true, "");
    plugin(root.path(), "second", "example.required", false, "");
    let mut manager = manager();
    manager.require_plugins(&["example.required".into()]);
    assert!(manager.load_plugins(root.path()).is_err());
    assert!(manager.initialize_startup(&mut world()).is_err());
}
