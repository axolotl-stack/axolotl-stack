//! Generated chest/spawner metadata must survive real provider close/reopen.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use unastar::registry::BlockRegistry;
use unastar::storage::{BlazeDBProvider, ChunkColumn, LevelDBWorldProvider, WorldProvider};
use unastar::world::block_entity::{BlockEntity, LootTable};
use unastar::world::{Chunk, ChunkPos};
use zuri_nbt::{NBTTag, encoding::LittleEndian};

fn entity(id: &str, pos: [i32; 3], extra: &[(&str, NBTTag)]) -> NBTTag {
    let mut fields = HashMap::from([
        ("id".into(), NBTTag::String(id.to_owned().into())),
        ("x".into(), NBTTag::Int(pos[0].into())),
        ("y".into(), NBTTag::Int(pos[1].into())),
        ("z".into(), NBTTag::Int(pos[2].into())),
    ]);
    fields.extend(
        extra
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone())),
    );
    NBTTag::Compound(fields.into())
}

fn position(tag: &NBTTag) -> [i32; 3] {
    let NBTTag::Compound(compound) = tag else {
        panic!("block entity must be a compound")
    };
    ["x", "y", "z"].map(|key| match compound.0.get(key) {
        Some(NBTTag::Int(value)) => value.0,
        other => panic!("block entity {key} must be an Int, got {other:?}"),
    })
}

fn generated_column(pos: ChunkPos) -> (ChunkColumn, Vec<NBTTag>) {
    let mut chunk = Chunk::new(pos.x, pos.z);
    let registry = BlockRegistry::vanilla();
    let chest = registry
        .default_state_id_by_name("minecraft:chest")
        .unwrap();
    let spawner = registry
        .default_state_id_by_name("minecraft:mob_spawner")
        .unwrap();
    chunk.set_block(15, 70, 15, chest).unwrap();
    chunk.set_block(14, 68, 14, spawner).unwrap();
    chunk.set_block(13, -64, 13, chest).unwrap();
    let java_pos = [pos.x * 16 + 15, 70, pos.z * 16 + 15];
    let spawner_pos = [pos.x * 16 + 14, 68, pos.z * 16 + 14];
    let bedrock_pos = [pos.x * 16 + 13, -64, pos.z * 16 + 13];
    let seed = 140737488355329i64;
    assert!(chunk.set_block_entity(BlockEntity::generated(
        java_pos,
        "Chest",
        &[],
        Some(LootTable::Java("minecraft:chests/simple_dungeon", seed)),
        None,
    )));
    assert!(chunk.set_block_entity(BlockEntity::generated(
        spawner_pos,
        "MobSpawner",
        &[],
        None,
        Some("minecraft:zombie"),
    )));
    assert!(chunk.set_block_entity(BlockEntity::generated(
        bedrock_pos,
        "Chest",
        &[],
        Some(LootTable::Bedrock(
            "loot_tables/chests/abandoned_mineshaft.json",
            -seed
        )),
        None,
    )));
    let expected = vec![
        entity(
            "Chest",
            java_pos,
            &[
                (
                    "LootTable",
                    NBTTag::String("loot_tables/chests/simple_dungeon.json".to_owned().into()),
                ),
                ("LootTableSeed", NBTTag::Long(seed.into())),
            ],
        ),
        entity(
            "MobSpawner",
            spawner_pos,
            &[(
                "EntityIdentifier",
                NBTTag::String("minecraft:zombie".to_owned().into()),
            )],
        ),
        entity(
            "Chest",
            bedrock_pos,
            &[
                (
                    "LootTable",
                    NBTTag::String(
                        "loot_tables/chests/abandoned_mineshaft.json"
                            .to_owned()
                            .into(),
                    ),
                ),
                ("LootTableSeed", NBTTag::Long((-seed).into())),
            ],
        ),
    ];
    (ChunkColumn::new(chunk), expected)
}

fn assert_metadata(column: &ChunkColumn, expected: &[NBTTag]) {
    let mut actual: Vec<_> = column
        .chunk
        .block_entities()
        .iter()
        .map(|entity| {
            // Validate the actual network root, then transcode with the production
            // signed-varint-safe reader rather than zuri 0.4's stock network reader.
            let entity = BlockEntity::from_network_nbt(entity.network_nbt().clone()).unwrap();
            let mut bytes = Vec::new();
            entity.write_disk_nbt(&mut bytes).unwrap();
            let mut input = bytes.as_slice();
            let tag = NBTTag::read(&mut input, LittleEndian).unwrap();
            assert!(input.is_empty(), "trailing block-entity bytes");
            assert_eq!(position(&tag), entity.position());
            tag
        })
        .collect();
    let mut expected = expected.to_vec();
    actual.sort_by_key(position);
    expected.sort_by_key(position);
    assert_eq!(actual, expected);
}

enum Provider {
    LevelDB(LevelDBWorldProvider),
    BlazeDB(Arc<BlazeDBProvider>),
}

impl Provider {
    fn open(path: &Path, blaze: bool) -> Self {
        if blaze {
            Self::BlazeDB(BlazeDBProvider::open(path, None).unwrap())
        } else {
            Self::LevelDB(LevelDBWorldProvider::open(path, 0).unwrap())
        }
    }

    fn storage(&self) -> &dyn WorldProvider {
        match self {
            Self::LevelDB(provider) => provider,
            Self::BlazeDB(provider) => provider.as_ref(),
        }
    }

    async fn close(self) {
        self.storage().close().await.unwrap();
        // Drop the provider and its cache/database handles before opening again.
        drop(self);
    }
}

#[tokio::test]
async fn generated_chest_and_spawner_nbt_survive_both_provider_reopens() {
    let pos = ChunkPos::new(-2, 1);
    for blaze in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let (column, expected) = generated_column(pos);
        let provider = Provider::open(directory.path(), blaze);
        provider
            .storage()
            .save_column(pos, 0, &column)
            .await
            .unwrap();
        // Intentionally no sleep or cache read: close must drain queued BlazeDB writes.
        provider.close().await;

        let provider = Provider::open(directory.path(), blaze);
        let mut restored = provider
            .storage()
            .load_column(pos, 0)
            .await
            .unwrap()
            .unwrap();
        assert_metadata(&restored, &expected);
        let registry = BlockRegistry::vanilla();
        assert_eq!(
            restored.chunk.get_block(15, 70, 15),
            registry
                .default_state_id_by_name("minecraft:chest")
                .unwrap()
        );
        assert_eq!(
            restored.chunk.get_block(14, 68, 14),
            registry
                .default_state_id_by_name("minecraft:mob_spawner")
                .unwrap()
        );

        assert!(restored.chunk.remove_block_entity(-17, 70, 31));
        provider
            .storage()
            .save_column(pos, 0, &restored)
            .await
            .unwrap();
        provider.close().await;
        let provider = Provider::open(directory.path(), blaze);
        let restored = provider
            .storage()
            .load_column(pos, 0)
            .await
            .unwrap()
            .unwrap();
        assert_metadata(&restored, &expected[1..]);

        // An entirely empty sparse set must erase, not retain, the old disk value.
        let mut empty = restored;
        assert!(empty.chunk.remove_block_entity(-18, 68, 30));
        assert!(empty.chunk.remove_block_entity(-19, -64, 29));
        provider
            .storage()
            .save_column(pos, 0, &empty)
            .await
            .unwrap();
        provider.close().await;
        let provider = Provider::open(directory.path(), blaze);
        let restored = provider
            .storage()
            .load_column(pos, 0)
            .await
            .unwrap()
            .unwrap();
        assert_metadata(&restored, &[]);
        provider.close().await;
    }
}

#[tokio::test]
async fn flush_drains_queued_metadata_without_close_or_cache_reads() {
    let directory = tempfile::tempdir().unwrap();
    let pos = ChunkPos::new(-2, 1);
    let (column, expected) = generated_column(pos);
    let writer = BlazeDBProvider::open(directory.path(), None).unwrap();
    writer.save_column(pos, 0, &column).await.unwrap();
    writer.flush().await.unwrap();
    let reader = BlazeDBProvider::open(directory.path(), None).unwrap();
    let restored = reader.load_column(pos, 0).await.unwrap().unwrap();
    assert_metadata(&restored, &expected);
    reader.close().await.unwrap();
    writer.close().await.unwrap();
}

#[tokio::test]
async fn leveldb_uses_standard_fixed_le_roots_and_rejects_invalid_records() {
    use bleveldb::{DB, Options, ReadOptions, WriteOptions};
    use unastar::storage::block_entities_key;
    use zuri_nbt::encoding::LittleEndian;

    let directory = tempfile::tempdir().unwrap();
    let pos = ChunkPos::new(-2, 1);
    let (column, mut expected) = generated_column(pos);
    let provider = Provider::open(directory.path(), false);
    provider
        .storage()
        .save_column(pos, 0, &column)
        .await
        .unwrap();
    provider.close().await;
    let db = DB::open(directory.path(), &Options::new()).unwrap();
    let key = block_entities_key(pos, 0);
    let bytes = db.get(&key, &ReadOptions::new()).unwrap().unwrap();
    let mut input = bytes.as_slice();
    let mut actual = Vec::new();
    while !input.is_empty() {
        actual.push(NBTTag::read(&mut input, LittleEndian).unwrap());
    }
    actual.sort_by_key(position);
    expected.sort_by_key(position);
    assert_eq!(
        actual, expected,
        "0x31 must contain concatenated fixed-LE roots"
    );
    drop(db);

    let mut invalid = vec![vec![0xff], bytes[..bytes.len() - 1].to_vec()];
    let mut trailing = bytes.clone();
    trailing.push(0xff);
    invalid.push(trailing);
    for outside in [
        [pos.x * 16 - 1, 70, pos.z * 16],
        [-17, 320, 31],
        [-17, -65, 31],
    ] {
        let tag = entity("Chest", outside, &[]);
        let mut raw = Vec::new();
        tag.write(&mut raw, LittleEndian).unwrap();
        invalid.push(raw);
    }
    for raw in invalid {
        let db = DB::open(directory.path(), &Options::new()).unwrap();
        db.put(&key, &raw, &WriteOptions::new()).unwrap();
        db.flush();
        drop(db);
        let provider = Provider::open(directory.path(), false);
        assert!(provider.storage().load_column(pos, 0).await.is_err());
        provider.close().await;
    }

    // Empty overwrite deletes the actual key, not merely the in-memory records.
    let provider = Provider::open(directory.path(), false);
    provider
        .storage()
        .save_column(pos, 0, &ChunkColumn::new(Chunk::new(pos.x, pos.z)))
        .await
        .unwrap();
    provider.close().await;
    let db = DB::open(directory.path(), &Options::new()).unwrap();
    assert!(db.get(&key, &ReadOptions::new()).unwrap().is_none());
}

#[tokio::test]
async fn blazedb_reopen_rejects_corrupted_payload_instead_of_returning_cached_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let pos = ChunkPos::new(-2, 1);
    let (column, _) = generated_column(pos);
    let provider = Provider::open(directory.path(), true);
    provider
        .storage()
        .save_column(pos, 0, &column)
        .await
        .unwrap();
    provider.close().await;
    let path = directory.path().join("chunks.dat");
    let mut data = std::fs::read(&path).unwrap();
    *data.last_mut().unwrap() ^= 0x80;
    std::fs::write(path, data).unwrap();
    let provider = Provider::open(directory.path(), true);
    let result = provider.storage().load_column(pos, 0).await;
    assert!(result.is_err(), "corruption must fail the real disk load");
    provider.close().await;
}

#[tokio::test]
async fn blazedb_metadata_survives_obsolete_entries_between_live_chunks() {
    let directory = tempfile::tempdir().unwrap();
    let first_pos = ChunkPos::new(-2, 1);
    let second_pos = ChunkPos::new(3, -4);
    let (first, first_expected) = generated_column(first_pos);
    let (mut second, second_expected) = generated_column(second_pos);
    let provider = Provider::open(directory.path(), true);
    provider
        .storage()
        .save_column(first_pos, 0, &first)
        .await
        .unwrap();
    provider
        .storage()
        .save_column(second_pos, 0, &second)
        .await
        .unwrap();
    assert!(
        second
            .chunk
            .remove_block_entity(second_pos.x * 16 + 15, 70, second_pos.z * 16 + 15)
    );
    provider
        .storage()
        .save_column(second_pos, 0, &second)
        .await
        .unwrap();
    provider.close().await;

    // The old second-chunk entry remains between first and the live second
    // entry. Index repair must not mistake that legitimate gap for corruption.
    let provider = Provider::open(directory.path(), true);
    let restored_first = provider
        .storage()
        .load_column(first_pos, 0)
        .await
        .unwrap()
        .unwrap();
    let restored_second = provider
        .storage()
        .load_column(second_pos, 0)
        .await
        .unwrap()
        .unwrap();
    assert_metadata(&restored_first, &first_expected);
    assert_metadata(&restored_second, &second_expected[1..]);
    provider.close().await;
}
