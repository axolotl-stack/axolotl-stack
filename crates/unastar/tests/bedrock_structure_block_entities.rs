//! Authored structure metadata exercises native placement, replay and persistence.
#![cfg(feature = "bedrock-worldgen")]

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;
use unastar::registry::BlockRegistry;
use unastar::storage::{BlazeDBProvider, ChunkColumn, LevelDBWorldProvider, WorldProvider};
use unastar::world::block_entity::BlockEntity;
use unastar::world::generator::bedrock::adapter::RegionWorld;
use unastar::world::generator::bedrock::decorate::MolangNoise;
use unastar::world::generator::bedrock::rng::BedrockRandom;
use unastar::world::generator::bedrock::structures::{
    KEEP_LAYER, Template, TemplateBlockEntity, TemplateState,
};
use unastar::world::generator::bedrock::world::{BlockPos, BlockWorld, Ctx};
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
    static PALETTE: std::sync::LazyLock<[TemplateState; 2]> = std::sync::LazyLock::new(|| {
        let registry = BlockRegistry::vanilla();
        let chest = registry
            .default_state_id_by_name("minecraft:chest")
            .unwrap();
        let spawner = registry
            .default_state_id_by_name("minecraft:mob_spawner")
            .unwrap();
        let air = registry.default_state_id_by_name("minecraft:air").unwrap();
        [chest, spawner].map(|primary| TemplateState {
            primary: [primary; 4],
            secondary: [air; 4],
        })
    });
    static CHEST_FIELDS: std::sync::LazyLock<Vec<u8>> = std::sync::LazyLock::new(|| {
        use unastar_packs::{
            block_entity::{self, Source},
            nbt::Tag,
        };
        let source = Tag::Compound(BTreeMap::from([
            ("id".into(), Tag::String("Chest".into())),
            ("CustomName".into(), Tag::String("cache".into())),
            (
                "LootTable".into(),
                Tag::String("loot_tables/chests/simple_dungeon.json".into()),
            ),
            ("LootTableSeed".into(), Tag::Int(-314159)),
        ]));
        block_entity::compile("authored-native-template", &source, Source::Bedrock)
            .unwrap()
            .fields
    });
    static ACTORS: std::sync::LazyLock<[TemplateBlockEntity; 2]> = std::sync::LazyLock::new(|| {
        [
            TemplateBlockEntity {
                cell: 0,
                id: "Chest",
                fields: &CHEST_FIELDS,
                loot: None,
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
        ]
    });
    let template = Template {
        size: [3, 2, 2],
        palette: &*PALETTE,
        cells: &[0, 0, 0, 0, 0, 2, 1, 1, 1, 0],
        connectors: &[],
        block_entities: &*ACTORS,
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
                ("LootTableSeed", NBTTag::Int((-314159).into())),
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

fn layer_provider(path: &Path, blaze: bool) -> Arc<dyn WorldProvider> {
    if blaze {
        BlazeDBProvider::open(path, None).unwrap()
    } else {
        Arc::new(LevelDBWorldProvider::open(path, 0).unwrap())
    }
}

fn assert_layers(
    chunk: &Chunk,
    pos: ChunkPos,
    expected: &[(BlockPos, [u32; 2])],
    cleared_air: Option<u32>,
) {
    for &(at, mut layers) in expected {
        if at.x >> 4 != pos.x || at.z >> 4 != pos.z {
            continue;
        }
        if let Some(air) = cleared_air {
            layers[1] = air;
        }
        let actual = [0, 1].map(|layer| {
            chunk.get_block_layer((at.x & 15) as u8, at.y as i16, (at.z & 15) as u8, layer)
        });
        assert_eq!(actual, layers, "layers at {at:?}");
    }
}

#[tokio::test]
async fn template_layer_actions_survive_replay_reopen_network_and_dry_overwrite() {
    let registry = BlockRegistry::vanilla();
    let air = registry.default_state_id_by_name("minecraft:air").unwrap();
    let water = registry
        .default_state_id_by_name("minecraft:water")
        .unwrap();
    let bars = registry
        .default_state_id_by_name("minecraft:iron_bars")
        .unwrap();
    let glass = registry
        .default_state_id_by_name("minecraft:glass_pane")
        .unwrap();
    static PALETTE: std::sync::LazyLock<[TemplateState; 4]> = std::sync::LazyLock::new(|| {
        let registry = BlockRegistry::vanilla();
        let air = registry.default_state_id_by_name("minecraft:air").unwrap();
        let water = registry
            .default_state_id_by_name("minecraft:water")
            .unwrap();
        let bars = registry
            .default_state_id_by_name("minecraft:iron_bars")
            .unwrap();
        [
            TemplateState {
                primary: [bars; 4],
                secondary: [water; 4],
            },
            TemplateState {
                primary: [bars; 4],
                secondary: [KEEP_LAYER; 4],
            },
            TemplateState {
                primary: [KEEP_LAYER; 4],
                secondary: [water; 4],
            },
            TemplateState {
                primary: [bars; 4],
                secondary: [air; 4],
            },
        ]
    });
    let template = Template {
        size: [2, 2, 1],
        palette: &*PALETTE,
        cells: &[0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 2, 0, 1, 1, 0, 3, 0],
        connectors: &[],
        block_entities: &[],
    };
    let origin = ChunkPos::new(-2, 1);
    let mut protos = SLOT_OFFSETS
        .map(|(dx, dz)| ProtoChunk::empty(ChunkPos::new(origin.x + dx, origin.z + dz), 0));
    let mut expected = Vec::new();
    for y in [-64, 318] {
        for (at, primary, secondary) in [
            (BlockPos::new(-17, y + 1, 16), bars, Some(water)),
            (BlockPos::new(-16, y, 16), glass, None),
            (BlockPos::new(-16, y + 1, 16), bars, Some(water)),
        ] {
            let slot = SLOT_OFFSETS
                .iter()
                .position(|&(dx, dz)| origin.x + dx == at.x >> 4 && origin.z + dz == at.z >> 4)
                .unwrap();
            assert!(
                protos[slot]
                    .chunk
                    .set_block_layers(
                        (at.x & 15) as u8,
                        at.y as i16,
                        (at.z & 15) as u8,
                        primary,
                        secondary,
                    )
                    .is_some()
            );
        }
        expected.extend([
            (BlockPos::new(-17, y, 16), [bars, water]),
            (BlockPos::new(-17, y + 1, 16), [bars, water]),
            (BlockPos::new(-16, y, 16), [glass, water]),
            (BlockPos::new(-16, y + 1, 16), [bars, air]),
        ]);
    }
    let mut region = DecorationRegion::new(
        12345,
        origin,
        std::array::from_fn(|i| &protos[i]),
        &unastar_noise::java_blocks::BEDROCK_STATE_CLASSES,
    );
    {
        let mut world = RegionWorld::new(&mut region);
        let mut rng = BedrockRandom::new(12345);
        let noise = MolangNoise::new(12345);
        let mut ctx = Ctx {
            world: &mut world,
            rng: &mut rng,
            noise: &noise,
            seed: 12345,
        };
        for y in [-64, 318] {
            template.place(&mut ctx, BlockPos::new(-17, y, 16), 0);
        }
        for outside in [
            BlockPos::new(-17, -65, 16),
            BlockPos::new(-17, 320, 16),
            BlockPos::new(0, 70, 16),
        ] {
            assert!(!ctx.world.set_layers(outside, bars, Some(water)));
        }
    }
    let writes = region.into_writes();
    let columns = [(0, 0), (1, 0)].map(|(dx, dz)| {
        let slot = SLOT_OFFSETS
            .iter()
            .position(|&offset| offset == (dx, dz))
            .unwrap();
        let pos = ChunkPos::new(origin.x + dx, origin.z + dz);
        let mut chunk = protos[slot].chunk.clone();
        writes.apply_to(dx, dz, &mut chunk);
        assert_layers(&chunk, pos, &expected, None);
        (pos, ChunkColumn::new(chunk))
    });
    // The journal must not mutate cached source protos while decorating.
    let neighbor = SLOT_OFFSETS
        .iter()
        .position(|&offset| offset == (1, 0))
        .unwrap();
    assert_eq!(protos[neighbor].chunk.get_block_layer(0, -64, 0, 1), air);
    assert_eq!(protos[neighbor].chunk.get_block_layer(0, -63, 0, 1), water);
    for blaze in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let provider = layer_provider(directory.path(), blaze);
        for (pos, column) in &columns {
            provider.save_column(*pos, 0, column).await.unwrap();
        }
        provider.close().await.unwrap();
        drop(provider);
        let provider = layer_provider(directory.path(), blaze);
        for (pos, _) in &columns {
            let mut restored = provider.load_column(*pos, 0).await.unwrap().unwrap();
            assert_layers(&restored.chunk, *pos, &expected, None);
            let mut received = Chunk::new(pos.x, pos.z);
            for section in [-4, 19] {
                let bytes = restored.chunk.encode_subchunk(section).unwrap();
                received.decode_subchunk(section, &bytes).unwrap();
            }
            assert_layers(&received, *pos, &expected, None);
            for &(at, layers) in &expected {
                if at.x >> 4 == pos.x && at.z >> 4 == pos.z {
                    restored
                        .chunk
                        .set_block((at.x & 15) as u8, at.y as i16, (at.z & 15) as u8, layers[0])
                        .unwrap();
                }
            }
            provider.save_column(*pos, 0, &restored).await.unwrap();
        }
        provider.close().await.unwrap();
        drop(provider);
        let provider = layer_provider(directory.path(), blaze);
        for (pos, _) in &columns {
            let restored = provider.load_column(*pos, 0).await.unwrap().unwrap();
            assert_layers(&restored.chunk, *pos, &expected, Some(air));
        }
        provider.close().await.unwrap();
    }
}
