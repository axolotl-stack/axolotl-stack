//! Explicit duration limits and unlimited phases exercise actual canonical guest calls.

use super::*;
use std::sync::{
    Arc, Barrier, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

/// A real epoch clock keeps advancing while synchronous guest work owns its thread.
struct EpochClock {
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl EpochClock {
    /// Keep the test-owned engine clock alive until this guard is dropped.
    fn start(engine: &Engine) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let engine = engine.clone();
        let worker = std::thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(1));
                engine.increment_epoch();
            }
        });
        Self {
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for EpochClock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}

/// Force actual epoch checks before finite, fuel-measured guest work.
fn work(local: u32, iterations: u32) -> String {
    format!(
        "(local i32) i32.const 0 i32.const 1024 i32.const 0 call @log@ {}",
        finite_loop(local, iterations).replace("(local i32)", "")
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn zero_time_limits_run_real_long_startup_and_callbacks_without_hidden_epoch_leases() {
    let engine = engine();
    let _clock = EpochClock::start(&engine);
    let component = fixture::component(
        &engine,
        &[
            ("on-load", work(0, 100_000_000)),
            ("on-tick", work(1, 100_000_000)),
        ],
    );
    let limits = PluginLimits {
        fuel_per_tick: 0,
        fuel_per_initialization: 0,
        ..Default::default()
    };
    assert_eq!(
        (
            limits.initialization_timeout_ms,
            limits.callback_timeout_ms,
            limits.host_io_timeout_ms
        ),
        (0, 0, 0)
    );
    let mut manager = manager(engine.clone(), component, limits);
    manager.pending[0].store.data_mut().test_log_epoch_engine = Some(engine);
    let mut world = world();
    manager.initialize_startup(&mut world).unwrap();
    assert!(
        u64::MAX
            - manager
                .plugins
                .values_mut()
                .next()
                .unwrap()
                .store
                .get_fuel()
                .unwrap()
            > 100_000_000
    );
    for _ in 0..2 {
        assert!(manager.begin_tick(&mut world));
        manager.tick(&mut world);
        manager.check_required().unwrap();
        assert!(
            u64::MAX
                - manager
                    .plugins
                    .values_mut()
                    .next()
                    .unwrap()
                    .store
                    .get_fuel()
                    .unwrap()
                > 100_000_000
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_and_callback_time_opt_out_independently_and_finite_failure_stays_sticky() {
    for (initial, callback, busy_load, initial_fails, callback_fails) in [
        (0, 10, true, false, true),
        (10, 0, false, false, false),
        (10, 0, true, true, false),
    ] {
        let engine = engine();
        let component = fixture::component(
            &engine,
            &[
                (
                    "on-load",
                    if busy_load { work(0, 2) } else { String::new() },
                ),
                ("on-tick", work(1, 2)),
            ],
        );
        let limits = PluginLimits {
            initialization_timeout_ms: initial,
            callback_timeout_ms: callback,
            ..Default::default()
        };
        let mut manager = manager(engine.clone(), component, limits);
        manager.pending[0].store.data_mut().test_log_epoch_engine = Some(engine);
        let mut world = world();
        let started = manager.initialize_startup(&mut world);
        if initial_fails {
            assert!(started.unwrap_err().contains("deadline"));
            assert!(!manager.begin_tick(&mut world));
            continue;
        }
        started.unwrap();
        assert!(manager.begin_tick(&mut world));
        manager.tick(&mut world);
        if callback_fails {
            assert!(manager.check_required().unwrap_err().contains("deadline"));
            assert!(!manager.begin_tick(&mut world));
        } else {
            manager.check_required().unwrap();
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disabling_all_durations_does_not_hide_an_actual_guest_trap() {
    let engine = engine();
    let component = fixture::component(&engine, &[("on-tick", "unreachable".into())]);
    let mut manager = manager(
        engine,
        component,
        PluginLimits {
            fuel_per_tick: 0,
            fuel_per_initialization: 0,
            ..Default::default()
        },
    );
    let mut world = world();
    manager.initialize_startup(&mut world).unwrap();
    assert!(manager.begin_tick(&mut world));
    manager.tick(&mut world);
    assert!(
        manager
            .check_required()
            .unwrap_err()
            .contains("guest trapped")
    );
    assert!(!manager.begin_tick(&mut world));
}

/// Use actual storage import lock contention without adding a production delay hook.
async fn storage_wait(callback_ms: u32, io_ms: u32, wait_ms: u64, expected_error: bool) {
    use crate::plugin::storage::{Limits, Registry};
    let directory = tempfile::tempdir().unwrap();
    let registry = Arc::new(Mutex::new(Registry::new(
        directory.path().into(),
        Limits::default(),
    )));
    let namespace = registry
        .lock()
        .unwrap()
        .namespace("example.budget")
        .unwrap();
    let engine = engine();
    let _clock = EpochClock::start(&engine);
    let literal = "state"
        .bytes()
        .enumerate()
        .map(|(i, b)| format!("i32.const {} i32.const {b} i32.store8 ", 1024 + i))
        .collect::<String>();
    let read = format!(
        "i32.const 1024 i32.const 5 i32.const 160 call @storage.read@ i32.const 160 i32.load i32.const {} i32.ne if unreachable end",
        u8::from(expected_error)
    );
    let component = fixture::component(&engine, &[("on-load", literal), ("on-tick", read)]);
    let limits = PluginLimits {
        callback_timeout_ms: callback_ms,
        host_io_timeout_ms: io_ms,
        ..Default::default()
    };
    let mut manager = manager(engine, component, limits);
    manager.pending[0].store.data_mut().storage_registry = registry.clone();
    manager.storage_registry = registry;
    let mut world = world();
    manager.initialize_startup(&mut world).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let blocked = barrier.clone();
    let worker = std::thread::spawn(move || {
        let _guard = namespace.lock().unwrap();
        blocked.wait();
        std::thread::sleep(Duration::from_millis(wait_ms));
    });
    let before = Instant::now();
    barrier.wait();
    assert!(manager.begin_tick(&mut world));
    manager.tick(&mut world);
    let guest_elapsed = before.elapsed();
    worker.join().unwrap();
    assert!(
        guest_elapsed >= Duration::from_millis(wait_ms),
        "guest did not wait on its actual module namespace"
    );
    manager.check_required().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_storage_wait_beyond_the_old_io_cutoff_succeeds_when_time_is_unlimited() {
    storage_wait(0, 0, 350, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn finite_callback_excludes_confirmed_io_while_explicit_io_quota_returns_a_typed_error() {
    storage_wait(100, 0, 350, false).await;
    storage_wait(0, 25, 100, true).await;
}

#[test]
fn duration_defaults_and_explicit_disabled_fields_roundtrip_without_changing_resource_quotas() {
    let default = PluginLimits::default();
    let decoded: PluginLimits = toml_edit::de::from_str(
        "initialization_timeout_ms=0\ncallback_timeout_ms=0\nhost_io_timeout_ms=0\n",
    )
    .unwrap();
    assert_eq!(
        (
            decoded.initialization_timeout_ms,
            decoded.callback_timeout_ms,
            decoded.host_io_timeout_ms
        ),
        (0, 0, 0)
    );
    assert_eq!(decoded.max_memory, default.max_memory);
    assert_eq!(
        decoded.max_host_operations_per_tick,
        default.max_host_operations_per_tick
    );
    assert_eq!(decoded.max_owned_resources, default.max_owned_resources);
    assert_eq!(decoded.fuel_per_tick, default.fuel_per_tick);
    assert!(decoded.validate_execution().is_ok());
    let finite = PluginLimits {
        initialization_timeout_ms: 750,
        callback_timeout_ms: 25,
        host_io_timeout_ms: 50,
        ..Default::default()
    };
    let restored: PluginLimits =
        toml_edit::de::from_str(&toml_edit::ser::to_string(&finite).unwrap()).unwrap();
    assert_eq!(restored.host_io_timeout_ms, 50);
    assert!(restored.validate_execution().is_ok());
}
