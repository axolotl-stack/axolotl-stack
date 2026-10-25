//! Authored structure metadata exercises native placement, replay and persistence.
#![cfg(feature = "bedrock-worldgen")]

use std::collections::{BTreeMap, HashMap};
use unastar::registry::BlockRegistry;
use unastar::storage::{ChunkColumn, LevelDBWorldProvider, WorldProvider};
use unastar::world::block_entity::BlockEntity;
use unastar::world::generator::bedrock::adapter::RegionWorld;
use unastar::world::generator::bedrock::decorate::MolangNoise;
use unastar::world::generator::bedrock::rng::BedrockRandom;
use unastar::world::generator::bedrock::structures::{Template, TemplateBlockEntity};
use unastar::world::generator::bedrock::world::{BlockPos, Ctx};
use unastar::world::generator::proto::ProtoChunk;
use unastar::world::generator::region::{DecorationRegion, SLOT_OFFSETS};
use unastar::world::{Chunk, ChunkPos};
use zuri_nbt::{NBTTag, encoding::LittleEndian};

fn string(value: &str) -> NBTTag {
    NBTTag::String(value.to_owned().into())
}

fn compound<const N: usize>(fields: [(&str, NBTTag); N]) -> NBTTag {
    NBTTag::Compound(
        fields
            .into_iter()
            .map(|(key, tag)| (key.to_owned(), tag))
            .collect::<HashMap<_, _>>()
            .into(),
    )
}

fn metadata(chunk: &Chunk) -> BTreeMap<[i32; 3], NBTTag> {
    chunk
        .block_entities()
        .iter()
        .map(|actor| {
            let actor = BlockEntity::from_network_nbt(actor.network_nbt().clone()).unwrap();
            let mut bytes = Vec::new();
            actor.write_disk_nbt(&mut bytes).unwrap();
            let mut input = bytes.as_slice();
            let tag = NBTTag::read(&mut input, LittleEndian).unwrap();
            assert!(input.is_empty());
            (actor.position(), tag)
        })
        .collect()
}

#[tokio::test]
async fn rotated_bedrock_chest_and_spawner_survive_region_replay_and_provider_reopen() {
    let origin = ChunkPos::new(-2, 1);
    let protos = SLOT_OFFSETS
        .map(|(dx, dz)| ProtoChunk::empty(ChunkPos::new(origin.x + dx, origin.z + dz), 0));
    let mut region = DecorationRegion::new(
        12345,
        origin,
        std::array::from_fn(|i| &protos[i]),
        &unastar_noise::java_blocks::BEDROCK_STATE_CLASSES,
    );
    static PALETTE: std::sync::LazyLock<[[u32; 4]; 2]> = std::sync::LazyLock::new(|| {
        let registry = BlockRegistry::vanilla();
        let chest = registry
            .default_state_id_by_name("minecraft:chest")
            .unwrap();
        let spawner = registry
            .default_state_id_by_name("minecraft:mob_spawner")
            .unwrap();
        [[chest; 4], [spawner; 4]]
    });
    let template = Template {
        size: [3, 2, 2],
        palette: &*PALETTE,
        cells: &[0, 0, 0, 0, 0, 2, 1, 1, 1, 0],
        connectors: &[],
        block_entities: &[
            TemplateBlockEntity {
                cell: 0,
                id: "Chest",
                fields: b"\x08\x0aCustomName\x05cache",
                loot: Some(("loot_tables/chests/simple_dungeon.json", -314159)),
                java_loot: false,
                spawner_entity: None,
            },
            TemplateBlockEntity {
                cell: 1,
                id: "MobSpawner",
                fields: b"\x02\x05Delay\xf9\xff",
                loot: None,
                java_loot: false,
                spawner_entity: Some("minecraft:zombie"),
            },
        ],
    };
    let mut rng = BedrockRandom::new(9234);
    let mut expected_rng = rng.clone();
    {
        let noise = MolangNoise::new(12345);
        let mut world = RegionWorld::new(&mut region);
        let mut ctx = Ctx {
            world: &mut world,
            rng: &mut rng,
            noise: &noise,
            seed: 12345,
        };
        template.place(&mut ctx, BlockPos::new(-31, -63, 17), 1);
    }
    assert_eq!(
        rng.next_u32(),
        expected_rng.next_u32(),
        "metadata adds no RNG draws"
    );
    let mut chunk = Chunk::new(origin.x, origin.z);
    region.into_writes().apply_to(0, 0, &mut chunk);
    let expected = BTreeMap::from([
        (
            [-31, -63, 17],
            compound([
                ("id", string("Chest")),
                ("x", NBTTag::Int((-31).into())),
                ("y", NBTTag::Int((-63).into())),
                ("z", NBTTag::Int(17.into())),
                ("CustomName", string("cache")),
                (
                    "LootTable",
                    string("loot_tables/chests/simple_dungeon.json"),
                ),
                ("LootTableSeed", NBTTag::Long((-314159).into())),
            ]),
        ),
        (
            [-32, -62, 19],
            compound([
                ("id", string("MobSpawner")),
                ("x", NBTTag::Int((-32).into())),
                ("y", NBTTag::Int((-62).into())),
                ("z", NBTTag::Int(19.into())),
                ("Delay", NBTTag::Short((-7).into())),
                ("EntityIdentifier", string("minecraft:zombie")),
            ]),
        ),
    ]);
    assert_eq!(metadata(&chunk), expected);
    let directory = tempfile::tempdir().unwrap();
    {
        let provider = LevelDBWorldProvider::open(directory.path(), 0).unwrap();
        provider
            .save_column(origin, 0, &ChunkColumn::new(chunk))
            .await
            .unwrap();
        provider.close().await.unwrap();
    }
    let provider = LevelDBWorldProvider::open(directory.path(), 0).unwrap();
    let restored = provider.load_column(origin, 0).await.unwrap().unwrap();
    assert_eq!(metadata(&restored.chunk), expected);
    provider.close().await.unwrap();
}
