use super::*;
use crate::{
    storage::{LevelDBWorldProvider, WorldProvider},
    world::{Chunk, chunk::blocks},
};
use bleveldb::{DB, Options};
use std::path::PathBuf;

struct Temp(PathBuf);
impl Temp {
    /// Unique independent real database path.
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("unastar-columns-{}", uuid::Uuid::new_v4())))
    }
    /// Open a real LevelDB handle, waiting only for an owned writer to close.
    fn db(&self) -> Arc<DB> {
        let options = Options::new();
        options.create_if_missing(true);
        for _ in 0..200 {
            if let Ok(db) = DB::open(&self.0, &options) {
                return Arc::new(db);
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("owned database did not close");
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
/// A complete owned column, with one observable stone cell.
fn column(x: i32, dim: i32, stone: bool) -> ColumnWrite {
    let mut chunk = Chunk::new(x, 0);
    if stone {
        chunk.set_block(1, 1, 1, *blocks::STONE);
    }
    ColumnWrite {
        position: ChunkPos::new(x, 0),
        dimension: dim,
        column: ChunkColumn::new(chunk),
    }
}
/// Resolve accepted IO without releasing the native publication fence implicitly.
async fn durable(
    writer: &ColumnWriter,
    id: &str,
    columns: &[ColumnWrite],
) -> Result<ColumnWriteAck, ColumnWriteError> {
    let stamp = writer.capture()?;
    writer.reserve()?;
    let result = match writer.durable(stamp, columns, id) {
        Ok(receiver) => receiver.await.unwrap(),
        Err(error) => Err(error),
    };
    if !matches!(result, Err(ColumnWriteError::RecoveryRequired)) {
        writer.release();
    }
    result
}

#[tokio::test]
async fn durable_batch_reopens_all_columns_and_rejects_old_epoch_autosave() {
    let dir = Temp::new();
    {
        let writer = ColumnWriter::open(dir.db()).unwrap();
        let old = writer.capture().unwrap();
        let set = [column(0, 0, true), column(3, 1, true)];
        let ack = durable(&writer, "operation/1", &set).await.unwrap();
        assert_eq!(ack.epoch, 1);
        assert_eq!(
            writer
                .ordinary(old, &[column(0, 0, false)])
                .unwrap()
                .await
                .unwrap()
                .unwrap_err(),
            ColumnWriteError::Stale
        );
        let new = writer.capture().unwrap();
        writer
            .ordinary(new, &[column(3, 1, true)])
            .unwrap()
            .await
            .unwrap()
            .unwrap();
    }
    let writer = ColumnWriter::open(dir.db()).unwrap();
    let receipt = writer.receipt("operation/1").await.unwrap().unwrap();
    assert_eq!(receipt.outcome, ReceiptOutcome::Applied);
    assert_eq!(receipt.epoch, 1);
    drop(writer);
    let provider = loop {
        if let Ok(p) = LevelDBWorldProvider::open(&dir.0, 0) {
            break p;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    };
    for (x, dim) in [(0, 0), (3, 1)] {
        assert_eq!(
            provider
                .load_column(ChunkPos::new(x, 0), dim)
                .await
                .unwrap()
                .unwrap()
                .chunk
                .get_block(1, 1, 1),
            *blocks::STONE
        );
    }
    assert!(matches!(
        provider
            .save_column(ChunkPos::new(0, 0), 0, &column(0, 0, false).column)
            .await,
        Err(StorageError::ColumnWrite(ColumnWriteError::Stale))
    ));
}

#[tokio::test]
async fn original_capture_order_and_provider_identity_cannot_be_rewritten() {
    let a = Temp::new();
    let b = Temp::new();
    let writer = ColumnWriter::open(a.db()).unwrap();
    let other = ColumnWriter::open(b.db()).unwrap();
    let first = writer.capture().unwrap();
    let second = writer.capture().unwrap();
    writer
        .ordinary(second, &[column(0, 0, true)])
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        writer
            .ordinary(first, &[column(0, 0, false)])
            .unwrap()
            .await
            .unwrap()
            .unwrap_err(),
        ColumnWriteError::Stale
    );
    assert_eq!(
        other.ordinary(first, &[column(0, 0, false)]).unwrap_err(),
        ColumnWriteError::Stale
    );
}

#[tokio::test]
async fn receipt_replay_does_not_reapply_historical_terrain_and_conflicts_are_known() {
    let dir = Temp::new();
    let writer = ColumnWriter::open(dir.db()).unwrap();
    let set = [column(0, 0, true)];
    durable(&writer, "operation/2", &set).await.unwrap();
    let stamp = writer.capture().unwrap();
    writer
        .ordinary(stamp, &[column(0, 0, false)])
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    assert!(durable(&writer, "operation/2", &set).await.unwrap().replay);
    assert_eq!(
        durable(&writer, "operation/2", &[column(0, 0, false)])
            .await
            .unwrap_err(),
        ColumnWriteError::Conflict
    );
    let actual = writer
        .read(|db| {
            db.get(
                &crate::storage::keys::entity_ids_key(ChunkPos::new(0, 0), 0),
                &bleveldb::ReadOptions::new(),
            )
            .map_err(StorageError::Database)
        })
        .unwrap();
    assert!(actual.is_none());
}

#[tokio::test]
async fn dropped_completion_still_commits_owned_work() {
    let dir = Temp::new();
    let writer = ColumnWriter::open(dir.db()).unwrap();
    let stamp = writer.capture().unwrap();
    writer.reserve().unwrap();
    drop(
        writer
            .durable(stamp, &[column(0, 0, true)], "operation/3")
            .unwrap(),
    );
    for _ in 0..500 {
        if writer.receipt("operation/3").await.unwrap().is_some() {
            assert!(writer.reserved());
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    panic!("accepted work lost with receiver");
}

#[tokio::test]
async fn prepared_and_post_commit_faults_reconcile_differently_after_restart() {
    for point in [1, 2] {
        let dir = Temp::new();
        {
            let writer = ColumnWriter::open(dir.db()).unwrap();
            writer.store.set_fault(point);
            assert_eq!(
                durable(&writer, "operation/fault", &[column(0, 0, true)])
                    .await
                    .unwrap_err(),
                ColumnWriteError::RecoveryRequired
            );
            assert_eq!(
                writer.capture().unwrap_err(),
                ColumnWriteError::RecoveryRequired
            );
            assert!(writer.read(|_| Ok(())).is_err());
        }
        let writer = ColumnWriter::open(dir.db()).unwrap();
        let receipt = writer.receipt("operation/fault").await.unwrap().unwrap();
        assert_eq!(
            receipt.outcome,
            if point == 1 {
                ReceiptOutcome::NotApplied
            } else {
                ReceiptOutcome::Applied
            }
        );
        assert_eq!(receipt.epoch, if point == 1 { 0 } else { 1 });
    }
}

#[tokio::test]
async fn duplicate_actor_and_removed_live_actor_sets_reject_before_prepare() {
    let dir = Temp::new();
    let writer = ColumnWriter::open(dir.db()).unwrap();
    let mut first = column(0, 0, false);
    let mut second = column(1, 0, false);
    let actor = crate::storage::StoredActor {
        unique_id: 17,
        nbt: vec![10, 0, 0, 0],
    };
    first.column.actors.push(actor.clone());
    second.column.actors.push(actor);
    assert_eq!(
        durable(&writer, "duplicate", &[first.clone(), second])
            .await
            .unwrap_err(),
        ColumnWriteError::Invalid
    );
    writer.release();
    first.column.removed_actors.push(17);
    assert_eq!(
        durable(&writer, "contradiction", &[first])
            .await
            .unwrap_err(),
        ColumnWriteError::Invalid
    );
    assert!(writer.receipt("duplicate").await.unwrap().is_none());
}

#[test]
fn malformed_or_incomplete_ledger_fails_before_any_column_load() {
    let dir = Temp::new();
    let db = dir.db();
    db.put(
        b"unastar:column-writer:v1",
        br#"{"version":1,"epoch":1,"receipts":{},"prepared":null}"#,
        &bleveldb::WriteOptions::new(),
    )
    .unwrap();
    assert!(ColumnWriter::open(db.clone()).is_err());
    db.put(
        b"unastar:column-writer:v1",
        b"broken",
        &bleveldb::WriteOptions::new(),
    )
    .unwrap();
    assert!(ColumnWriter::open(db).is_err());
}

#[tokio::test]
async fn empty_sections_delete_old_storage_and_actor_records_follow_the_batch() {
    let dir = Temp::new();
    let writer = ColumnWriter::open(dir.db()).unwrap();
    let mut old = column(0, 0, true);
    old.column.chunk.set_block(1, 200, 1, *blocks::STONE);
    old.column.actors.push(crate::storage::StoredActor {
        unique_id: 92,
        nbt: vec![10, 0, 0, 0],
    });
    durable(&writer, "present", &[old]).await.unwrap();
    let mut new = column(0, 0, false);
    new.column.chunk.set_block_bounds(0..128);
    new.column.removed_actors.push(92);
    durable(&writer, "removed", &[new]).await.unwrap();
    writer
        .read(|db| {
            let opts = bleveldb::ReadOptions::new();
            for key in [
                crate::storage::keys::entity_data_key(92),
                crate::storage::keys::entity_ids_key(ChunkPos::new(0, 0), 0),
                crate::storage::keys::subchunk_key(ChunkPos::new(0, 0), 0, 12),
            ] {
                assert!(
                    db.get(&key, &opts).unwrap().is_none(),
                    "obsolete key {key:?}"
                );
            }
            let data = db
                .get(
                    &crate::storage::keys::subchunk_key(ChunkPos::new(0, 0), 0, 0),
                    &opts,
                )
                .unwrap()
                .unwrap();
            let mut restored = Chunk::new(0, 0);
            restored.decode_subchunk(0, &data).unwrap();
            assert_eq!(restored.get_block(1, 1, 1), *blocks::AIR);
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
async fn bounded_validation_rejects_unavailable_sets_without_ownership_or_io() {
    let dir = Temp::new();
    let writer = ColumnWriter::open(dir.db()).unwrap();
    let stamp = writer.capture().unwrap();
    assert_eq!(
        writer.ordinary(stamp, &[]).unwrap_err(),
        ColumnWriteError::Invalid
    );
    assert_eq!(
        writer
            .ordinary(stamp, &vec![column(0, 0, true); MAX_COLUMNS + 1])
            .unwrap_err(),
        ColumnWriteError::Invalid
    );
    let mut invalid = column(0, 0, false);
    invalid.column.actors.push(crate::storage::StoredActor {
        unique_id: 1,
        nbt: vec![10, 0, 0, 0, 9],
    });
    assert_eq!(
        writer.ordinary(stamp, &[invalid]).unwrap_err(),
        ColumnWriteError::Invalid
    );
    let mut too_large = column(0, 0, false);
    too_large.column.actors.push(crate::storage::StoredActor {
        unique_id: 1,
        nbt: vec![0; MAX_WRITE_BYTES + 1],
    });
    assert_eq!(
        writer.ordinary(stamp, &[too_large]).unwrap_err(),
        ColumnWriteError::Invalid
    );
    assert!(!writer.reserved());
    writer
        .ordinary(stamp, &[column(0, 0, true)])
        .unwrap()
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn close_drains_previously_accepted_io_and_rejects_new_snapshots() {
    let dir = Temp::new();
    let writer = ColumnWriter::open(dir.db()).unwrap();
    let stamp = writer.capture().unwrap();
    let completion = writer.ordinary(stamp, &[column(0, 0, true)]).unwrap();
    writer.close().await.unwrap();
    completion.await.unwrap().unwrap();
    assert_eq!(writer.capture().unwrap_err(), ColumnWriteError::Closed);
}

#[tokio::test]
async fn provider_epoch_protects_global_actor_records_from_another_columns_old_snapshot() {
    use zuri_nbt::{NBTTag, encoding::LittleEndian, tag};
    let dir = Temp::new();
    let writer = ColumnWriter::open(dir.db()).unwrap();
    let actor = |marker: i32| {
        let mut nbt = Vec::new();
        NBTTag::Compound(
            tag::Compound::builder()
                .with_long("UniqueID", 719i64)
                .with_int("Marker", marker)
                .build(),
        )
        .write(&mut nbt, LittleEndian)
        .unwrap();
        crate::storage::StoredActor {
            unique_id: 719,
            nbt,
        }
    };
    let mut previous = column(7, 0, false);
    previous.column.actors.push(actor(1));
    let stale = writer.capture().unwrap();
    let mut current = column(3, 0, true);
    current.column.actors.push(actor(2));
    let expected = current.column.actors[0].nbt.clone();
    durable(&writer, "actor/moved", &[current]).await.unwrap();
    assert_eq!(
        writer
            .ordinary(stale, &[previous])
            .unwrap()
            .await
            .unwrap()
            .unwrap_err(),
        ColumnWriteError::Stale
    );
    writer
        .read(|db| {
            assert_eq!(
                db.get(
                    &crate::storage::keys::entity_data_key(719),
                    &bleveldb::ReadOptions::new()
                )
                .unwrap(),
                Some(expected)
            );
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
async fn queued_writes_keep_their_capacity_after_completion_drop() {
    let dir = Temp::new();
    let writer = ColumnWriter::open(dir.db()).unwrap();
    let pause = writer.pause_next();
    let first = writer.capture().unwrap();
    drop(writer.ordinary(first, &[column(0, 0, true)]).unwrap());
    for _ in 0..1000 {
        if pause.entered.load(Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert!(pause.entered.load(Ordering::Acquire));
    for x in 1..=MAX_PENDING_WRITES as i32 {
        let stamp = writer.capture().unwrap();
        drop(writer.ordinary(stamp, &[column(x, 0, true)]).unwrap());
    }
    let used = writer.queued_bytes.load(Ordering::Acquire);
    assert!(used > 0);
    let rejected = writer.capture().unwrap();
    assert_eq!(
        writer
            .ordinary(rejected, &[column(99, 0, true)])
            .unwrap_err(),
        ColumnWriteError::Busy
    );
    assert_eq!(writer.queued_bytes.load(Ordering::Acquire), used);
    pause.release();
    writer.flush().await.unwrap();
    assert_eq!(writer.queued_bytes.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn directory_lease_outlives_a_dropped_writer_until_accepted_io_finishes() {
    let dir = Temp::new();
    let writer = ColumnWriter::open(dir.db()).unwrap();
    let lock_path = dir.0.join("ownership.lock");
    let lease = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();
    fs2::FileExt::try_lock_exclusive(&lease).unwrap();
    writer.attach_lease(Arc::new(lease)).unwrap();
    let contender = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    let pause = writer.pause_next();
    let stamp = writer.capture().unwrap();
    drop(writer.ordinary(stamp, &[column(0, 0, true)]).unwrap());
    for _ in 0..1000 {
        if pause.entered.load(Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert!(pause.entered.load(Ordering::Acquire));
    drop(writer);
    assert!(fs2::FileExt::try_lock_exclusive(&contender).is_err());
    pause.release();
    for _ in 0..1000 {
        if fs2::FileExt::try_lock_exclusive(&contender).is_ok() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    panic!("directory lease was not released after owned IO completed");
}

#[tokio::test]
async fn semantically_identical_nbt_field_order_replays_the_same_durable_receipt() {
    /// Construct explicit fixed-width NBT field order, rather than depending on
    /// a map's randomized iteration order to make the two physical encodings.
    fn actor_nbt(id_first: bool) -> Vec<u8> {
        let mut bytes = vec![10, 0, 0];
        let mut field = |kind: u8, name: &str, value: &[u8]| {
            bytes.push(kind);
            bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
            bytes.extend_from_slice(name.as_bytes());
            bytes.extend_from_slice(value);
        };
        if id_first {
            field(4, "UniqueID", &712i64.to_le_bytes());
            field(3, "Marker", &1i32.to_le_bytes());
        } else {
            field(3, "Marker", &1i32.to_le_bytes());
            field(4, "UniqueID", &712i64.to_le_bytes());
        }
        bytes.push(0);
        bytes
    }
    let dir = Temp::new();
    let first = actor_nbt(true);
    let second = actor_nbt(false);
    assert_ne!(first, second);
    {
        let writer = ColumnWriter::open(dir.db()).unwrap();
        let mut set = column(0, 0, true);
        set.column.actors.push(crate::storage::StoredActor {
            unique_id: 712,
            nbt: first,
        });
        durable(&writer, "canonical/actor", &[set]).await.unwrap();
    }
    let writer = ColumnWriter::open(dir.db()).unwrap();
    let mut set = column(0, 0, true);
    set.column.actors.push(crate::storage::StoredActor {
        unique_id: 712,
        nbt: second,
    });
    let ack = durable(&writer, "canonical/actor", &[set]).await.unwrap();
    assert!(ack.replay);
    assert_eq!(ack.epoch, 1);
}
