//! Compile a real Rust WASI component so standard-library stream adapters run.

use super::*;
use crate::ecs::events::{ActionQueue, EventBuffer, EventKind};
use std::{path::Path, process::Command, sync::OnceLock};
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;

/// Build in a separate target: the outer host test may still own Cargo's artifact lock.
fn component_bytes() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let manifest =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasi-stdio/Cargo.toml");
        let target = tempfile::tempdir().unwrap();
        let output = Command::new(env!("CARGO"))
            .args([
                "build",
                "--locked",
                "--target",
                "wasm32-wasip2",
                "--manifest-path",
            ])
            .arg(&manifest)
            .env("CARGO_TARGET_DIR", target.path())
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("RUSTFLAGS")
            .output()
            .expect("build the WASI fixture with the pinned Rust toolchain");
        assert!(
            output.status.success(),
            "WASI fixture build failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::read(
            target
                .path()
                .join("wasm32-wasip2/debug/wasi_stdio_fixture.wasm"),
        )
        .unwrap()
    })
}

/// Normal manager initialization and dispatch with bounded in-memory WASI streams.
fn fixture(
    mode: &str,
) -> (
    PluginManager,
    bevy_ecs::world::World,
    MemoryOutputPipe,
    MemoryOutputPipe,
) {
    let engine = engine();
    let manifest = PluginManifest {
        id: PluginId("sample.stdio".into()),
        name: "Stdio fixture".into(),
        version: "1".into(),
        api_version: PLUGIN_API_VERSION.into(),
        required: true,
        config: None,
        capabilities: Default::default(),
        subscriptions: [EventKind::Tick].into(),
        limits: PluginLimits::default(),
    };
    let stdout = MemoryOutputPipe::new(16_384);
    let stderr = MemoryOutputPipe::new(16_384);
    let wasi = WasiCtx::builder()
        .stdout(stdout.clone())
        .stderr(stderr.clone())
        .build();
    let mut store = create_store(&engine, wasi, manifest.id.clone(), &manifest.limits).unwrap();
    store.data_mut().configuration = mode.as_bytes().to_vec();
    let pending = PendingPlugin {
        component: Component::new(&engine, component_bytes()).unwrap(),
        store,
        manifest,
    };
    let manager = PluginManager {
        engine,
        plugins: Default::default(),
        pending: vec![pending],
        tick_prepared: false,
        tick_budget_valid: true,
        readiness: Default::default(),
        storage_registry: Default::default(),
        command_roots: Default::default(),
        ui_registry: Default::default(),
        view_registry: Default::default(),
    };
    let mut world = bevy_ecs::world::World::new();
    world.insert_resource(crate::ecs::TickCounter { current: 42 });
    world.insert_resource(ActionQueue::default());
    world.insert_resource(EventBuffer::default());
    world.insert_resource(crate::world::ChunkManager::new(
        crate::world::WorldConfig::void_spawn_platform(1),
    ));
    world.insert_resource(manager.readiness.clone());
    (manager, world, stdout, stderr)
}

/// Decode only the fixture's own stream output.
fn text(stream: &MemoryOutputPipe) -> String {
    String::from_utf8(stream.contents().to_vec()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_wasi_stdout_stderr_run_during_load_and_buffered_tick() {
    let (mut manager, mut world, stdout, stderr) = fixture("");
    manager.initialize_startup(&mut world).unwrap();
    manager.tick(&mut world);
    assert_eq!(
        text(&stdout),
        "fixture stdout on-load\nfixture stdout on-tick\n"
    );
    assert_eq!(
        text(&stderr),
        "fixture stderr on-load\nfixture stderr on-tick\n"
    );
    assert!(manager.readiness.check().is_ok());
    assert!(
        manager
            .plugins
            .values()
            .all(|plugin| plugin.store.data().world_ptr.is_null())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_wasi_load_panic_is_a_sticky_required_failure_without_native_panic() {
    let (mut manager, mut world, stdout, stderr) = fixture("load-trap");
    assert!(manager.initialize_startup(&mut world).is_err());
    assert!(text(&stdout).contains("fixture stdout on-load"));
    assert!(text(&stderr).contains("fixture load panic"));
    assert!(manager.plugins.is_empty());
    assert!(!manager.begin_tick(&mut world));
    assert!(!readiness::admission_allowed(&world));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_wasi_callback_panic_is_a_managed_trap_and_is_not_retried() {
    let (mut manager, mut world, stdout, stderr) = fixture("tick-trap");
    manager.initialize_startup(&mut world).unwrap();
    manager.tick(&mut world);
    assert!(text(&stdout).contains("fixture stdout on-tick"));
    assert!(text(&stderr).contains("fixture tick panic"));
    assert!(manager.readiness.check().is_err());
    assert!(!readiness::admission_allowed(&world));
    assert!(
        manager
            .plugins
            .values()
            .all(|plugin| plugin.store.data().world_ptr.is_null())
    );
    let before = (stdout.contents(), stderr.contents());
    manager.tick(&mut world);
    assert_eq!(before, (stdout.contents(), stderr.contents()));
}

#[tokio::test(flavor = "current_thread")]
async fn current_thread_startup_rejects_before_any_wasi_stream_or_guest_work() {
    let (mut manager, mut world, stdout, stderr) = fixture("");
    let error = manager.initialize_startup(&mut world).unwrap_err();
    assert!(error.to_string().contains("multi-thread Tokio runtime"));
    assert!(stdout.contents().is_empty());
    assert!(stderr.contents().is_empty());
    assert!(manager.plugins.is_empty());
    assert!(!readiness::admission_allowed(&world));
}

#[test]
fn current_thread_callback_is_rejected_before_guest_entry_and_clears_world_attachment() {
    let (mut manager, mut world, stdout, stderr) = fixture("");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        manager.initialize_startup(&mut world).unwrap();
    });
    let before = (stdout.contents(), stderr.contents());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        manager.tick(&mut world);
    });
    assert_eq!(before, (stdout.contents(), stderr.contents()));
    assert!(
        manager
            .readiness
            .check()
            .unwrap_err()
            .contains("multi-thread Tokio runtime")
    );
    assert!(
        manager
            .plugins
            .values()
            .all(|plugin| plugin.store.data().world_ptr.is_null())
    );
}
