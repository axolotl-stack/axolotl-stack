//! Durable world-item ownership stored in the same atomic record set as player inventory.

use super::{StorageError, StorageResult, StoredItemStack};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// A persisted native world item, independent of its current runtime entity ID.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistentItem {
    pub id: Uuid,
    pub world: Uuid,
    pub dimension: i32,
    pub feet: [f64; 3],
    pub stack: StoredItemStack,
}

/// Conditional item changes committed with one complete canonical account record.
#[derive(Clone, Debug)]
pub enum ItemMutation {
    Create(Vec<PersistentItem>),
    Take {
        expected: PersistentItem,
        remaining: u8,
    },
}

const INDEX: &[u8] = b"unastar:durable-items:v1:index";
const ROW_PREFIX: &[u8] = b"unastar:durable-items:v1:row:";
const MAX_ROWS: usize = 65_536;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_ACTIVE: usize = 4096;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    version: u32,
    ids: Vec<Uuid>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    version: u32,
    item: Option<PersistentItem>,
}
struct Cached {
    item: Option<PersistentItem>,
    bytes: usize,
}
#[derive(Default)]
pub(crate) struct ItemLedger {
    rows: BTreeMap<Uuid, Cached>,
    bytes: usize,
    active: usize,
}

/// An accepted mutation copies only changed rows. Permanent identity is never evicted.
pub(crate) struct PreparedItems {
    changes: Vec<(Uuid, Option<PersistentItem>, Vec<u8>)>,
    index: Option<Vec<u8>>,
    bytes: usize,
    active: usize,
}

fn invalid() -> StorageError {
    StorageError::Deserialization("invalid durable item ownership".into())
}
fn key(id: Uuid) -> Vec<u8> {
    [ROW_PREFIX, id.as_bytes()].concat()
}
fn valid(item: &PersistentItem) -> bool {
    !item.id.is_nil()
        && !item.world.is_nil()
        && item.feet.iter().all(|n| n.is_finite())
        && !item.stack.item_id.is_empty()
        && item.stack.item_id.len() <= 256
        && (1..=64).contains(&item.stack.count)
        && item.stack.damage >= 0
        && item.stack.runtime_id >= 0
        && item
            .stack
            .nbt
            .as_ref()
            .is_none_or(|bytes| bytes.len() <= 131_072)
}

impl ItemLedger {
    /// Read the bounded stable index and each checked record before exposing the provider.
    pub(crate) fn open(db: &bleveldb::DB) -> StorageResult<Self> {
        let bytes = db
            .get(INDEX, &bleveldb::ReadOptions::new())
            .map_err(StorageError::Database)?;
        let ids = match bytes {
            None => {
                let mut iterator = db.iter(&bleveldb::ReadOptions::new());
                iterator.seek(ROW_PREFIX);
                if iterator
                    .key()
                    .is_some_and(|key| key.starts_with(ROW_PREFIX))
                {
                    return Err(invalid());
                }
                Vec::new()
            }
            Some(bytes) => {
                if bytes.len() > 4 * 1024 * 1024 {
                    return Err(invalid());
                }
                let index: Index = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
                if index.version != 1 || index.ids.len() > MAX_ROWS {
                    return Err(invalid());
                }
                index.ids
            }
        };
        let mut ledger = Self::default();
        for id in ids {
            if id.is_nil() || ledger.rows.contains_key(&id) {
                return Err(invalid());
            }
            let bytes = db
                .get(&key(id), &bleveldb::ReadOptions::new())
                .map_err(StorageError::Database)?
                .ok_or_else(invalid)?;
            if bytes.len() > 132_096 {
                return Err(invalid());
            }
            let row: Row = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
            if row.version != 1
                || row
                    .item
                    .as_ref()
                    .is_some_and(|item| item.id != id || !valid(item))
            {
                return Err(invalid());
            }
            ledger.active += usize::from(row.item.is_some());
            ledger.bytes += bytes.len();
            ledger.rows.insert(
                id,
                Cached {
                    item: row.item,
                    bytes: bytes.len(),
                },
            );
        }
        if ledger.bytes > MAX_BYTES || ledger.active > MAX_ACTIVE {
            return Err(invalid());
        }
        // A valid discovery index cannot hide additional physical ownership rows.
        // This scan runs only at open, never during ordinary pickup or retirement.
        let mut iterator = db.iter(&bleveldb::ReadOptions::new());
        iterator.seek(ROW_PREFIX);
        while let Some(key) = iterator.key().filter(|key| key.starts_with(ROW_PREFIX)) {
            let id = Uuid::from_slice(&key[ROW_PREFIX.len()..]).map_err(|_| invalid())?;
            if !ledger.rows.contains_key(&id) {
                return Err(invalid());
            }
            iterator.next_native();
        }
        Ok(ledger)
    }

    pub(crate) fn world(&self, world: Uuid, dimension: i32) -> Vec<PersistentItem> {
        self.rows
            .values()
            .filter_map(|row| row.item.as_ref())
            .filter(|item| item.world == world && item.dimension == dimension)
            .cloned()
            .collect()
    }

    /// Check all conditional rows without cloning or serializing the ownership history.
    pub(crate) fn prepare(&self, mutation: &ItemMutation) -> StorageResult<PreparedItems> {
        let mut changes = BTreeMap::new();
        let creation = match mutation {
            ItemMutation::Create(items) => {
                if items.len() > 256 {
                    return Err(StorageError::ItemQuotaExceeded);
                }
                for item in items {
                    if !valid(item) {
                        return Err(invalid());
                    }
                    if self.rows.contains_key(&item.id)
                        || changes.insert(item.id, Some(item.clone())).is_some()
                    {
                        return Err(StorageError::PlayerRevisionConflict);
                    }
                }
                true
            }
            ItemMutation::Take {
                expected,
                remaining,
            } => {
                if *remaining >= expected.stack.count
                    || self
                        .rows
                        .get(&expected.id)
                        .and_then(|row| row.item.as_ref())
                        != Some(expected)
                {
                    return Err(StorageError::PlayerRevisionConflict);
                }
                let next = if *remaining == 0 {
                    None
                } else {
                    let mut next = expected.clone();
                    next.stack.count = *remaining;
                    Some(next)
                };
                changes.insert(expected.id, next);
                false
            }
        };
        if creation && self.rows.len() + changes.len() > MAX_ROWS {
            return Err(StorageError::ItemQuotaExceeded);
        }
        let mut prepared = PreparedItems {
            changes: Vec::with_capacity(changes.len()),
            index: None,
            bytes: self.bytes,
            active: self.active,
        };
        for (id, item) in changes {
            let bytes = serde_json::to_vec(&Row {
                version: 1,
                item: item.clone(),
            })
            .map_err(|error| StorageError::Serialization(error.to_string()))?;
            if let Some(old) = self.rows.get(&id) {
                prepared.bytes -= old.bytes;
                prepared.active -= usize::from(old.item.is_some());
            }
            prepared.bytes += bytes.len();
            prepared.active += usize::from(item.is_some());
            prepared.changes.push((id, item, bytes));
        }
        if prepared.bytes > MAX_BYTES || prepared.active > MAX_ACTIVE {
            return Err(StorageError::ItemQuotaExceeded);
        }
        if creation && !prepared.changes.is_empty() {
            let ids = self
                .rows
                .keys()
                .copied()
                .chain(prepared.changes.iter().map(|(id, _, _)| *id))
                .collect();
            prepared.index = Some(
                serde_json::to_vec(&Index { version: 1, ids })
                    .map_err(|error| StorageError::Serialization(error.to_string()))?,
            );
        }
        Ok(prepared)
    }

    /// In-memory ownership changes only after this exact batch is durably acknowledged.
    pub(crate) fn acknowledge(&mut self, prepared: PreparedItems) {
        for (id, item, bytes) in prepared.changes {
            self.rows.insert(
                id,
                Cached {
                    item,
                    bytes: bytes.len(),
                },
            );
        }
        self.bytes = prepared.bytes;
        self.active = prepared.active;
    }
}

impl PreparedItems {
    pub(crate) fn append(&self, batch: &mut bleveldb::WriteBatch) {
        for (id, _, bytes) in &self.changes {
            batch.put(&key(*id), bytes);
        }
        if let Some(index) = &self.index {
            batch.put(INDEX, index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: u128) -> PersistentItem {
        PersistentItem {
            id: Uuid::from_u128(id),
            world: Uuid::from_u128(1),
            dimension: 0,
            feet: [1.5, 70.0, 2.5],
            stack: StoredItemStack::from(&crate::item::test_support::stack(
                "minecraft:diamond",
                64,
            )),
        }
    }

    #[test]
    fn pickup_changes_only_one_row_without_reencoding_the_discovery_index() {
        let mut ledger = ItemLedger::default();
        let prepared = ledger
            .prepare(&ItemMutation::Create(vec![item(2), item(3)]))
            .unwrap();
        assert!(prepared.index.is_some());
        ledger.acknowledge(prepared);
        let prepared = ledger
            .prepare(&ItemMutation::Take {
                expected: item(2),
                remaining: 0,
            })
            .unwrap();
        assert!(prepared.index.is_none());
        assert_eq!(prepared.changes.len(), 1);
        assert!(
            prepared.changes[0].2.len() < 64,
            "retirement writes a compact tombstone, not item history"
        );
        assert_eq!(
            ledger.world(Uuid::from_u128(1), 0).len(),
            2,
            "preparation does not publish ownership"
        );
        ledger.acknowledge(prepared);
        assert_eq!(ledger.world(Uuid::from_u128(1), 0), [item(3)]);
        assert!(
            ledger
                .prepare(&ItemMutation::Create(vec![item(2)]))
                .is_err()
        );
    }

    #[test]
    fn duplicate_or_stale_rows_never_partially_change_ownership() {
        let mut ledger = ItemLedger::default();
        assert!(
            ledger
                .prepare(&ItemMutation::Create(vec![item(2), item(2)]))
                .is_err()
        );
        assert!(ledger.rows.is_empty());
        let prepared = ledger
            .prepare(&ItemMutation::Create(vec![item(2)]))
            .unwrap();
        ledger.acknowledge(prepared);
        let mut stale = item(2);
        stale.stack.damage = 1;
        assert!(
            ledger
                .prepare(&ItemMutation::Take {
                    expected: stale,
                    remaining: 0
                })
                .is_err()
        );
        assert_eq!(ledger.world(Uuid::from_u128(1), 0), [item(2)]);
    }

    #[test]
    fn corrupt_or_missing_indexed_row_fails_provider_open() {
        let directory = tempfile::tempdir().unwrap();
        {
            let options = bleveldb::Options::new();
            options.create_if_missing(true);
            let db = bleveldb::DB::open(directory.path(), &options).unwrap();
            let options = bleveldb::WriteOptions::new();
            options.sync(true);
            db.put(
                INDEX,
                &serde_json::to_vec(&Index {
                    version: 1,
                    ids: vec![Uuid::from_u128(2)],
                })
                .unwrap(),
                &options,
            )
            .unwrap();
        }
        assert!(crate::storage::LevelDBPlayerProvider::open(directory.path()).is_err());
    }

    #[test]
    fn existing_index_cannot_silently_hide_an_orphan_ownership_row() {
        let directory = tempfile::tempdir().unwrap();
        {
            let options = bleveldb::Options::new();
            options.create_if_missing(true);
            let db = bleveldb::DB::open(directory.path(), &options).unwrap();
            let options = bleveldb::WriteOptions::new();
            options.sync(true);
            db.put(
                INDEX,
                &serde_json::to_vec(&Index {
                    version: 1,
                    ids: Vec::new(),
                })
                .unwrap(),
                &options,
            )
            .unwrap();
            db.put(
                &key(Uuid::from_u128(2)),
                &serde_json::to_vec(&Row {
                    version: 1,
                    item: Some(item(2)),
                })
                .unwrap(),
                &options,
            )
            .unwrap();
        }
        assert!(crate::storage::LevelDBPlayerProvider::open(directory.path()).is_err());
    }
}
