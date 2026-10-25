//! Real jar templates pass through Java placement, region replay and LevelDB.

#![cfg(feature = "java-worldgen")]

use std::collections::{BTreeMap, HashMap};
use unastar::storage::{ChunkColumn, LevelDBWorldProvider, WorldProvider};
use unastar::world::block_entity::BlockEntity;
use unastar::world::generator::density::NoiseRegistry;
use unastar::world::generator::features::RegionWorld;
use unastar::world::generator::proto::ProtoChunk;
use unastar::world::generator::region::{DecorationRegion, SLOT_OFFSETS};
use unastar::world::{Chunk, ChunkPos};
use unastar_noise::features::BlockPos;
use unastar_noise::features::random::FeatureRandom;
use unastar_noise::structures::template::{PlaceSettings, place_in_world};
use unastar_noise::structures::template_data::template;
use unastar_noise::structures::template_piece::record_block_entities;
use unastar_noise::structures::transform::{Mirror, Rotation};
use zuri_nbt::{NBTTag, encoding::LittleEndian};

fn compound<const N: usize>(fields: [(&str, NBTTag); N]) -> NBTTag {
    NBTTag::Compound(
        fields
            .into_iter()
            .map(|(key, tag)| (key.to_owned(), tag))
            .collect::<HashMap<_, _>>()
            .into(),
    )
}

fn string(value: &str) -> NBTTag {
    NBTTag::String(value.to_owned().into())
}

fn semantic(entity: &BlockEntity) -> NBTTag {
    // Exercise the rooted NETWORK payload through the same signed-varint-safe
    // transcoder used by persistence (zuri 0.4's stock network reader is lossy).
    let mut bytes = Vec::new();
    entity.write_disk_nbt(&mut bytes).unwrap();
    let mut input = bytes.as_slice();
    let tag = NBTTag::read(&mut input, LittleEndian).unwrap();
    assert!(input.is_empty());
    tag
}

fn chest_or_spawner(tag: &NBTTag) -> bool {
    let NBTTag::Compound(fields) = tag else {
        return false;
    };
    matches!(fields.0.get("id"), Some(NBTTag::String(id)) if matches!(id.0.as_str(), "Chest" | "MobSpawner"))
}

#[tokio::test]
async fn java_structure_chest_and_spawner_survive_region_replay_and_provider_reopen() {
    let seed = 12345;
    let origin = ChunkPos::new(-2, 1);
    let protos = SLOT_OFFSETS
        .map(|(dx, dz)| ProtoChunk::empty(ChunkPos::new(origin.x + dx, origin.z + dz), 0));
    let mut region = DecorationRegion::new(
        seed,
        origin,
        std::array::from_fn(|i| &protos[i]),
        &unastar_noise::java_blocks::BEDROCK_STATE_CLASSES,
    );
    let noises = NoiseRegistry::new(seed);
    let mut world = RegionWorld::new(&mut region, &noises);
    let mut random = FeatureRandom::new(9234);
    let mut expected_random = random.clone();
    let loot_seed = expected_random.next_long();
    for (name, offset) in [
        (
            "minecraft:pillager_outpost/watchtower",
            BlockPos::new(-32, 70, 16),
        ),
        (
            "minecraft:woodland_mansion/1x1_as2",
            BlockPos::new(-32, 100, 16),
        ),
    ] {
        let mut settings = PlaceSettings::new(template(name).unwrap());
        settings.mirror = Mirror::LeftRight;
        settings.rotation = Rotation::Clockwise90;
        settings.apply_waterlogging = false;
        let mut actors = Vec::new();
        assert!(place_in_world(
            &mut world,
            offset,
            offset,
            &mut settings,
            &mut random,
            &mut |actor| actors.push(actor),
        ));
        record_block_entities(&mut world, &actors);
    }
    assert_eq!(
        random.next_long(),
        expected_random.next_long(),
        "metadata adds no RNG draws"
    );
    let mut chunk = Chunk::new(origin.x, origin.z);
    region.into_writes().apply_to(0, 0, &mut chunk);
    let expected = BTreeMap::from([
        (
            [-22, 84, 25],
            compound([
                ("id", string("Chest")),
                ("x", NBTTag::Int((-22).into())),
                ("y", NBTTag::Int(84.into())),
                ("z", NBTTag::Int(25.into())),
                (
                    "LootTable",
                    string("loot_tables/chests/pillager_outpost.json"),
                ),
                ("LootTableSeed", NBTTag::Long(loot_seed.into())),
            ]),
        ),
        (
            [-29, 101, 20],
            compound([
                ("id", string("MobSpawner")),
                ("x", NBTTag::Int((-29).into())),
                ("y", NBTTag::Int(101.into())),
                ("z", NBTTag::Int(20.into())),
                ("EntityIdentifier", string("minecraft:spider")),
                ("Delay", NBTTag::Short(0.into())),
                ("MinSpawnDelay", NBTTag::Short(200.into())),
                ("MaxSpawnDelay", NBTTag::Short(800.into())),
                ("SpawnCount", NBTTag::Short(4.into())),
                ("MaxNearbyEntities", NBTTag::Short(6.into())),
                ("RequiredPlayerRange", NBTTag::Short(16.into())),
                ("SpawnRange", NBTTag::Short(4.into())),
                (
                    "SpawnPotentials",
                    NBTTag::List(
                        vec![compound([
                            (
                                "data",
                                compound([(
                                    "entity",
                                    compound([("id", string("minecraft:spider"))]),
                                )]),
                            ),
                            ("weight", NBTTag::Int(1.into())),
                        ])]
                        .into(),
                    ),
                ),
            ]),
        ),
    ]);
    let generated: BTreeMap<_, _> = chunk
        .block_entities()
        .iter()
        .map(|actor| (actor.position(), semantic(actor)))
        .filter(|(_, tag)| chest_or_spawner(tag))
        .collect();
    assert_eq!(generated, expected);
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
    let actual: BTreeMap<_, _> = restored
        .chunk
        .block_entities()
        .iter()
        .map(|actor| {
            let network = BlockEntity::from_network_nbt(actor.network_nbt().clone()).unwrap();
            (network.position(), semantic(&network))
        })
        .filter(|(_, tag)| chest_or_spawner(tag))
        .collect();
    assert_eq!(actual, expected);
    provider.close().await.unwrap();
}
