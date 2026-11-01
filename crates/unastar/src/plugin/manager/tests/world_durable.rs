//! Actual scoped components use real LevelDB and encoded native command ingress.

use super::*;
use crate::storage::{
    LevelDBWorldProvider, WorldProvider,
    column_writer::{ColumnWriteError, ColumnWriter},
};

/// Build the actual production SDK against this source's exact WIT schema.
fn durable_component() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/durable-world-blocks/Cargo.toml");
        let target = tempfile::tempdir().unwrap();
        let output = Command::new(env!("CARGO"))
            .args([
                "build",
                "--locked",
                "--target",
                "wasm32-wasip2",
                "--manifest-path",
            ])
            .arg(manifest)
            .env("CARGO_TARGET_DIR", target.path())
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("RUSTFLAGS")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "durable component build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::read(
            target
                .path()
                .join("wasm32-wasip2/debug/durable_world_blocks_fixture.wasm"),
        )
        .unwrap()
    })
}
struct DurableHarness {
    h: Harness,
    writer: Arc<ColumnWriter>,
    directory: tempfile::TempDir,
}
impl DurableHarness {
    /// The metadata identity and provider exist before guest initialization/admission.
    async fn new(pending: u32) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let provider =
            Arc::new(LevelDBWorldProvider::open(directory.path().join("db"), 0).unwrap());
        let writer = provider.column_writer().unwrap();
        let identity = crate::world::WorldIdentity {
            id: uuid::Uuid::from_u128(912),
            name: "primary".into(),
            dimension: 0,
        };
        let h = Harness::with_fixture(
            true,
            pending,
            b"",
            durable_component(),
            Some((provider, identity)),
            &[],
        )
        .await;
        Self {
            h,
            writer,
            directory,
        }
    }
    /// Reopen after dropping the old native world and every provider handle;
    /// the same configured identity and actual on-disk ledger are preserved.
    async fn reopen(self) -> Self {
        let Self {
            h,
            writer,
            directory,
        } = self;
        drop(h);
        writer.close().await.unwrap();
        drop(writer);
        let provider =
            Arc::new(LevelDBWorldProvider::open(directory.path().join("db"), 0).unwrap());
        let writer = provider.column_writer().unwrap();
        let identity = crate::world::WorldIdentity {
            id: uuid::Uuid::from_u128(912),
            name: "primary".into(),
            dimension: 0,
        };
        let h = Harness::with_fixture(
            true,
            16,
            b"",
            durable_component(),
            Some((provider, identity)),
            &[],
        )
        .await;
        Self {
            h,
            writer,
            directory,
        }
    }
    /// Replace an unloaded component with a fresh real instance of the same module.
    fn reload(&mut self, manifest: PluginManifest) {
        let engine = self.h.manager.lock().unwrap().engine.clone();
        let store = store(&engine, &manifest.limits);
        self.h.manager.lock().unwrap().pending.push(PendingPlugin {
            component: Component::new(&engine, durable_component()).unwrap(),
            store,
            manifest,
        });
        self.h.router.initialize_plugins().unwrap();
    }

    /// Completion is observed through actual router phases rather than a test status substitute.
    async fn finish(&mut self) {
        for _ in 0..1000 {
            self.h.tick().await;
            self.h.command("/status");
            if self
                .h
                .logs()
                .iter()
                .any(|log| log.contains("status:Ok(Committed"))
            {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        panic!("durable operation did not commit: {:?}", self.h.logs());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_sdk_waits_for_disk_ack_then_publishes_the_whole_native_batch() {
    let mut h = DurableHarness::new(16).await;
    let pause = h.writer.pause_next();
    h.h.command("/build operation-1");
    assert_eq!(h.h.block(1), "minecraft:air");
    assert_eq!(h.h.block(2), "minecraft:air");
    h.h.tick().await;
    for _ in 0..1000 {
        if pause.entered.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert!(pause.entered.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(h.h.block(1), "minecraft:air");
    assert_eq!(h.h.block(2), "minecraft:air");
    assert!(
        crate::server::game::blocks::try_set_block(
            h.h.world_mut(),
            IVec3::new(1, 21, 1),
            *crate::world::chunk::blocks::STONE,
            false
        )
        .is_err()
    );
    pause.release();
    h.finish().await;
    assert_eq!(h.h.block(1), "minecraft:obsidian");
    assert_eq!(h.h.block(2), "minecraft:obsidian");
    h.h.command("/receipt operation-1");
    assert!(
        h.h.logs()
            .iter()
            .any(|log| log.contains("receipt:Ok(Some(Committed"))
    );
    assert!(
        h.h.router
            .world_mut(0)
            .unwrap()
            .try_save_all_chunks()
            .await
            .unwrap()
            >= 1
    );
    assert_eq!(
        h.h.router
            .world_mut(0)
            .unwrap()
            .try_save_all_chunks()
            .await
            .unwrap(),
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn callback_trap_never_submits_io_and_unknown_receipt_never_claims_success() {
    let mut h = DurableHarness::new(16).await;
    h.h.command("/trap operation-1");
    h.h.tick().await;
    assert_eq!(h.h.block(1), "minecraft:air");
    assert!(!h.writer.reserved());
    let manifest =
        h.h.manager
            .lock()
            .unwrap()
            .plugins
            .remove(&PluginId("example.terrain".into()))
            .unwrap()
            .manifest;
    h.reload(manifest);
    h.h.command("/receipt operation-1");
    assert!(
        h.h.logs()
            .iter()
            .any(|log| log.contains("receipt:Ok(None)"))
    );
    h.h.command("/resume operation-1");
    assert!(
        h.h.logs()
            .iter()
            .any(|log| log.contains("RecoveryRequired"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stale_last_cell_rejects_the_entire_durable_proposal_before_io() {
    let mut h = DurableHarness::new(16).await;
    h.h.command("/build operation-1");
    h.h.put(IVec3::new(2, 21, 1), "minecraft:stone");
    h.h.tick().await;
    assert_eq!(h.h.block(1), "minecraft:air");
    assert_eq!(h.h.block(2), "minecraft:stone");
    h.h.command("/status");
    assert!(h.h.logs().iter().any(|log| log.contains("NotApplied")));
    assert!(!h.writer.reserved());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guest_drop_does_not_release_pending_capacity_or_cancel_accepted_io() {
    let mut h = DurableHarness::new(1).await;
    h.h.command("/quota operation-1");
    assert!(
        h.h.logs().iter().any(|log| log.contains("limit-exceeded")),
        "{:?}",
        h.h.logs()
    );
    for _ in 0..1000 {
        h.h.tick().await;
        if h.h.block(1) == "minecraft:obsidian" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    h.h.command("/receipt operation-1");
    assert!(
        h.h.logs()
            .iter()
            .any(|log| log.contains("receipt:Ok(Some(Committed"))
    );
    assert_eq!(h.h.block(1), "minecraft:obsidian");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_durable_capability_and_explicit_cancel_have_zero_storage_effects() {
    let mut h = DurableHarness::new(16).await;
    h.h.manager
        .lock()
        .unwrap()
        .plugins
        .get_mut(&PluginId("example.terrain".into()))
        .unwrap()
        .store
        .data_mut()
        .capabilities
        .remove(&PluginCapability::WriteDurableBlocks);
    h.h.command("/build operation-1");
    h.h.tick().await;
    assert_eq!(h.h.block(1), "minecraft:air");
    assert!(!h.writer.reserved());
    assert!(
        h.h.logs()
            .iter()
            .any(|log| log.contains("permission-denied"))
    );
    h.h.manager
        .lock()
        .unwrap()
        .plugins
        .get_mut(&PluginId("example.terrain".into()))
        .unwrap()
        .store
        .data_mut()
        .capabilities
        .insert(PluginCapability::WriteDurableBlocks);
    h.h.command("/cancel operation-1");
    h.h.tick().await;
    assert_eq!(h.h.block(1), "minecraft:air");
    assert!(!h.writer.reserved());
    h.h.command("/receipt operation-1");
    assert!(
        h.h.logs()
            .iter()
            .any(|log| log.contains("receipt:Ok(None)"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owner_drop_after_acceptance_requires_receipt_backed_resume_in_a_fresh_component() {
    let mut h = DurableHarness::new(16).await;
    let pause = h.writer.pause_next();
    h.h.command("/build operation-1");
    h.h.tick().await;
    for _ in 0..1000 {
        if pause.entered.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert!(pause.entered.load(std::sync::atomic::Ordering::Acquire));
    let manifest =
        h.h.manager
            .lock()
            .unwrap()
            .plugins
            .remove(&PluginId("example.terrain".into()))
            .unwrap()
            .manifest;
    pause.release();
    for _ in 0..100 {
        h.h.tick().await;
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert_eq!(h.h.block(1), "minecraft:air");
    assert!(h.writer.reserved());
    h.reload(manifest);
    h.h.command("/receipt operation-1");
    assert!(
        h.h.logs()
            .iter()
            .any(|log| log.contains("RecoveryRequired"))
    );
    h.h.command("/resume operation-1");
    h.finish().await;
    assert_eq!(h.h.block(1), "minecraft:obsidian");
    assert!(!h.writer.reserved());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uncertain_post_commit_ack_stays_fenced_until_reopen_and_real_receipt_query() {
    let mut h = DurableHarness::new(16).await;
    h.writer.set_fault(2);
    h.h.command("/build operation-1");
    for _ in 0..100 {
        h.h.tick().await;
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert_eq!(h.h.block(1), "minecraft:air");
    assert!(h.writer.reserved());
    h.h.command("/status");
    assert!(
        h.h.logs()
            .iter()
            .any(|log| log.contains("RecoveryRequired"))
    );
    // The poisoned provider's flush correctly refuses a fake acknowledgement.
    let DurableHarness {
        h: old,
        writer,
        directory,
    } = h;
    drop(old);
    assert!(writer.close().await.is_err());
    drop(writer);
    let provider = Arc::new(LevelDBWorldProvider::open(directory.path().join("db"), 0).unwrap());
    let writer = provider.column_writer().unwrap();
    let identity = crate::world::WorldIdentity {
        id: uuid::Uuid::from_u128(912),
        name: "primary".into(),
        dimension: 0,
    };
    let mut reopened = Harness::with_fixture(
        true,
        16,
        b"",
        durable_component(),
        Some((provider, identity)),
        &[],
    )
    .await;
    assert_eq!(reopened.block(1), "minecraft:obsidian");
    reopened.command("/receipt operation-1");
    assert!(
        reopened
            .logs()
            .iter()
            .any(|log| log.contains("receipt:Ok(Some(Committed"))
    );
    reopened.command("/build operation-1");
    assert!(reopened.logs().iter().any(|log| log.contains("Conflict")));
    assert!(!writer.reserved());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_receipt_acknowledges_the_existing_write_without_reapplying_current_state() {
    let mut h = DurableHarness::new(16).await;
    h.h.command("/build operation-1");
    h.finish().await;
    h.h.put(IVec3::new(1, 21, 1), "minecraft:stone");
    h.h.router
        .world_mut(0)
        .unwrap()
        .try_save_all_chunks()
        .await
        .unwrap();
    let mut h = h.reopen().await;
    h.h.command("/receipt operation-1");
    assert!(
        h.h.logs()
            .iter()
            .any(|log| log.contains("receipt:Ok(Some(Committed"))
    );
    assert_eq!(h.h.block(1), "minecraft:stone");
    assert!(!h.writer.reserved());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_persistent_world_identity_rejects_staged_work_before_io() {
    let mut h = DurableHarness::new(16).await;
    h.h.command("/build operation-1");
    h.h.world_mut()
        .resource_mut::<crate::world::WorldIdentity>()
        .id = uuid::Uuid::from_u128(913);
    h.h.tick().await;
    assert_eq!(h.h.block(1), "minecraft:air");
    assert!(!h.writer.reserved());
    h.h.command("/status");
    assert!(h.h.logs().iter().any(|log| log.contains("NotApplied")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepted_io_survives_actual_world_closure_and_reopens_only_canonical_receipt_state() {
    let mut h = DurableHarness::new(16).await;
    let pause = h.writer.pause_next();
    h.h.command("/build operation-1");
    h.h.tick().await;
    for _ in 0..1000 {
        if pause.entered.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert!(pause.entered.load(std::sync::atomic::Ordering::Acquire));
    let DurableHarness {
        h: old,
        writer,
        directory,
    } = h;
    drop(old);
    assert!(matches!(
        writer.capture(),
        Err(ColumnWriteError::RecoveryRequired)
    ));
    pause.release();
    writer.close().await.unwrap();
    drop(writer);
    let provider = Arc::new(LevelDBWorldProvider::open(directory.path().join("db"), 0).unwrap());
    let identity = crate::world::WorldIdentity {
        id: uuid::Uuid::from_u128(912),
        name: "primary".into(),
        dimension: 0,
    };
    let mut reopened = Harness::with_fixture(
        true,
        16,
        b"",
        durable_component(),
        Some((provider, identity)),
        &[],
    )
    .await;
    assert_eq!(reopened.block(1), "minecraft:obsidian");
    reopened.command("/receipt operation-1");
    assert!(
        reopened
            .logs()
            .iter()
            .any(|log| log.contains("receipt:Ok(Some(Committed"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_different_real_module_cannot_observe_another_modules_receipt_namespace() {
    let mut h = DurableHarness::new(16).await;
    h.h.command("/build operation-1");
    h.finish().await;
    let mut manifest =
        h.h.manager
            .lock()
            .unwrap()
            .plugins
            .remove(&PluginId("example.terrain".into()))
            .unwrap()
            .manifest;
    manifest.id = PluginId("example.other".into());
    manifest.name = "Other".into();
    h.reload(manifest);
    h.h.command("/receipt operation-1");
    let manager = h.h.manager.lock().unwrap();
    let logs = &manager.plugins[&PluginId("example.other".into())]
        .store
        .data()
        .test_logs;
    assert!(logs.iter().any(|log| log.contains("receipt:Ok(None)")));
    drop(manager);
}
