//! Real jar templates pass through Java placement, region replay and LevelDB.

#![cfg(feature = "java-worldgen")]

use std::collections::{BTreeMap, HashMap};
use unastar::storage::{ChunkColumn, LevelDBWorldProvider, WorldProvider};
use unastar::world::block_entity::BlockEntity;
use unastar::world::chunk::BlockStateSpace;
use unastar::world::generator::{JavaDimension, JavaGenerator};
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
        .map(|(dx, dz)| ProtoChunk::empty_in_space(
            ChunkPos::new(origin.x + dx, origin.z + dz), 0, BlockStateSpace::Java,
        ));
    let mut region = DecorationRegion::new(
        seed,
        origin,
        std::array::from_fn(|i| &protos[i]),
        &unastar_noise::java_blocks::JAVA_STATE_CLASSES,
    );
    let generator = JavaGenerator::new(seed);
    let mut world = RegionWorld::new(&mut region, JavaDimension::Overworld, &generator);
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
    let mut chunk = Chunk::new_in_space(origin.x, origin.z, BlockStateSpace::Java);
    region.into_writes().apply_to(0, 0, &mut chunk);
    {
        use unastar_noise::structures::native_actor::{Tag, key, string};
        fn compound<const N: usize>(values: [(&str, Tag); N]) -> Tag {
            Tag::Compound(values.into_iter().map(|(k, v)| (key(k), v)).collect())
        }
        // Native template saves at the ProtoChunk boundary, before LevelChunk
        // persistence adds keepPacked. No Bedrock fields have been materialized.
        let expected = BTreeMap::from([
            ([-22, 84, 25], compound([
                ("id", string("minecraft:chest")), ("x", Tag::Int(-22)),
                ("y", Tag::Int(84)), ("z", Tag::Int(25)),
                ("LootTable", string("minecraft:chests/pillager_outpost")),
                ("LootTableSeed", Tag::Long(loot_seed)),
                ("components", compound([])),
            ])),
            ([-29, 101, 20], compound([
                ("id", string("minecraft:mob_spawner")), ("x", Tag::Int(-29)),
                ("y", Tag::Int(101)), ("z", Tag::Int(20)),
                ("components", compound([])),
                ("Delay", Tag::Short(0)), ("MinSpawnDelay", Tag::Short(200)),
                ("MaxSpawnDelay", Tag::Short(800)), ("SpawnCount", Tag::Short(4)),
                ("MaxNearbyEntities", Tag::Short(6)), ("RequiredPlayerRange", Tag::Short(16)),
                ("SpawnRange", Tag::Short(4)),
                ("SpawnData", compound([("entity", compound([("id", string("minecraft:spider"))]))])),
                ("SpawnPotentials", Tag::List(10, vec![compound([
                    ("data", compound([("entity", compound([("id", string("minecraft:spider"))]))])),
                    ("weight", Tag::Int(1)),
                ])])),
            ])),
        ]);
        for (position, expected) in expected {
            let actor = chunk.block_entities().iter().find(|a| a.position() == position).unwrap();
            assert_eq!(Tag::read_root(&actor.java_nbt().unwrap()).unwrap(), expected);
            actor.canonical_java_nbt().unwrap();
        }
    }
    chunk.convert_to_bedrock().unwrap();
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

/// Compare every native template actor against independent ProtoChunk saves and
/// the original LevelChunk-boundary golden, without mutating either capture.
#[test]
#[ignore = "requires UNASTAR_NATIVE_ACTOR_CAPTURE and UNASTAR_NATIVE_PROTO_ACTOR_CAPTURE"]
fn all_template_actor_exports_match_native_saved_payloads() {
    use std::io::Read;
    use unastar_noise::features::eval::BlockEntityData;
    use unastar_noise::structures::native_actor::Tag as JavaTag;
    use unastar_noise::structures::template_data::{BlockEntityInfo, TEMPLATE_NAMES};

    fn bytes(input: &mut &[u8], width: usize) -> Vec<u8> {
        let mut prefix = [0; 4];
        input.read_exact(&mut prefix[..width]).unwrap();
        let length = prefix[..width].iter().fold(0usize, |n, &byte| (n << 8) | byte as usize);
        let mut value = vec![0; length];
        input.read_exact(&mut value).unwrap();
        value
    }
    let capture = std::env::var_os("UNASTAR_NATIVE_ACTOR_CAPTURE")
        .expect("set UNASTAR_NATIVE_ACTOR_CAPTURE to NativeActorOracle's native-actor-saves.bin");
    let capture = std::fs::read(capture).unwrap();
    let mut input = capture.as_slice();
    let mut magic = [0; 8];
    input.read_exact(&mut magic).unwrap();
    assert_eq!(&magic, b"NATJAVA2");
    for pin in [
        "64bb6d763bed0a9f1d632ec347938594144943ed",
        "5621e9253f05fd57872bbe7f8ddf5f9a7d525955",
        "ec47239a8de246335e1d54f6ac319bd35641778eb4b6a6da06372840d02fcebc",
    ] {
        assert_eq!(bytes(&mut input, 2), pin.as_bytes());
    }
    let proto_capture = std::env::var_os("UNASTAR_NATIVE_PROTO_ACTOR_CAPTURE")
        .expect("set UNASTAR_NATIVE_PROTO_ACTOR_CAPTURE to the NativePendingActorProbe consumer export");
    let proto_capture: serde_json::Value = serde_json::from_slice(&std::fs::read(proto_capture).unwrap()).unwrap();
    assert_eq!(proto_capture["server_sha1"], "64bb6d763bed0a9f1d632ec347938594144943ed");
    assert_eq!(proto_capture["mappings_sha1"], "5621e9253f05fd57872bbe7f8ddf5f9a7d525955");
    assert_eq!(proto_capture["inner_sha256"], "ec47239a8de246335e1d54f6ac319bd35641778eb4b6a6da06372840d02fcebc");
    assert_eq!(proto_capture["native_tick_count"], 0);
    let mut proto_observations: BTreeMap<String, BTreeMap<Vec<u8>, serde_json::Value>> = BTreeMap::new();
    let records = proto_capture["records"].as_array().unwrap();
    assert_eq!(records.len(), 889);
    for record in records {
        let source: Vec<u8> = serde_json::from_value(record["source_bytes"].clone()).unwrap();
        let source = JavaTag::read_root(&source).unwrap().write_root().unwrap();
        let name = record["template"].as_str().unwrap()
            .strip_prefix("data/minecraft/structure/").unwrap().strip_suffix(".nbt").unwrap();
        let by_source = proto_observations.entry(format!("minecraft:{name}")).or_default();
        if let Some(previous) = by_source.insert(source, record["saved"].clone()) {
            assert_eq!(previous, record["saved"], "equal sources must have equal native ProtoChunk saves");
        }
    }
    let mut observations: BTreeMap<String, BTreeMap<Vec<u8>, JavaTag>> = BTreeMap::new();
    let mut observed_count = 0;
    while !input.is_empty() {
        let name = String::from_utf8(bytes(&mut input, 4)).unwrap();
        let source = JavaTag::read_root(&bytes(&mut input, 4)).unwrap();
        let saved = JavaTag::read_root(&bytes(&mut input, 4)).unwrap();
        let Some(name) = name.strip_prefix("data/minecraft/structure/") else { continue; };
        let (name, _) = name.rsplit_once(".nbt#").unwrap();
        let source = source.write_root().unwrap();
        let by_source = observations.entry(format!("minecraft:{name}")).or_default();
        if let Some(previous) = by_source.get(&source) {
            assert_eq!(previous, &saved, "equal sources must have equal native saves");
        } else {
            by_source.insert(source, saved);
        }
        observed_count += 1;
    }
    let mut compared = 0;
    for &name in &TEMPLATE_NAMES {
        let template = template(name).unwrap();
        for (index, info) in template.block_entities.iter().enumerate() {
            let BlockEntityInfo::Other { actor, loot, spawner_entity } = info else { continue; };
            let block = template.palettes[0].iter()
                .find(|b| b.block_entity == Some(index as u16)).unwrap().state.block().name();
            let mut actual = BlockEntity::generated_java([0; 3], block, BlockEntityData {
                template_nbt: Some(*actor), loot: *loot, spawner_entity: *spawner_entity,
            });
            let expected_proto = proto_observations.get(name).unwrap().get(actor.source).unwrap();
            assert_eq!(&actual.canonical_java_nbt().unwrap(), expected_proto, "{name} actor {index} native ProtoChunk");
            // The old capture is still a LevelChunk-boundary golden. Check that
            // view separately, adding its external unpacked flag to the actual
            // ProtoChunk result rather than stripping or repinning the golden.
            let mut level_save = JavaTag::read_root(&actual.java_nbt().unwrap()).unwrap();
            level_save.compound_mut().unwrap().insert(
                unastar_noise::structures::native_actor::key("keepPacked"), JavaTag::Byte(0),
            );
            let expected_level = observations.get(name).unwrap().get(actor.source).unwrap();
            assert_eq!(&level_save, expected_level, "{name} actor {index} native LevelChunk");

            // The explicit conversion still produces the established compiler's
            // Bedrock payload from original source, not Java saved defaults.
            let source = unastar_packs::nbt::read_compound(
                name, actor.source, unastar_packs::nbt::Encoding::BigEndian,
            ).unwrap();
            let compiled = unastar_packs::block_entity::compile(
                name, &source, unastar_packs::block_entity::Source::Java,
            ).unwrap();
            let established = BlockEntity::generated(
                [0; 3], compiled.id, &compiled.fields,
                compiled.loot.as_ref().map(|(table, seed)| unastar::world::block_entity::LootTable::Java(table, *seed)),
                compiled.spawner_entity.as_deref(),
            );
            actual.convert_to_bedrock().unwrap();
            assert_eq!(semantic(&actual), semantic(&established), "{name} actor {index} Bedrock payload");
            compared += 1;
        }
    }
    assert_eq!(observed_count, 889, "pinned native observation coverage");
    assert_eq!(compared, observed_count);
}

#[test]
fn save_mode_structure_block_retains_native_saved_defaults() {
    use unastar_noise::features::eval::BlockEntityData;
    use unastar_noise::structures::native_actor::{Tag, key, string};
    use unastar_noise::structures::template_data::BlockEntityInfo;

    let template = template("minecraft:trial_chambers/chamber/addon/walkway_with_bridge_1").unwrap();
    let actor = template.block_entities.iter().find_map(|info| match info {
        BlockEntityInfo::Other { actor, .. } if actor.id == "minecraft:structure_block" => Some(*actor),
        _ => None,
    }).expect("SAVE-mode structure block is a retained actor, not a DATA marker");
    let actual = BlockEntity::generated_java([4, 5, 6], "minecraft:structure_block", BlockEntityData {
        template_nbt: Some(actor), loot: None, spawner_entity: None,
    });
    let saved = Tag::read_root(&actual.java_nbt().unwrap()).unwrap();
    let fields = saved.compound().unwrap();
    // Actual native loadStatic/saveWithFullMetadata observation for this actor:
    // strict is absent from its source, but saves explicitly as Byte0.
    assert_eq!(fields.get(&key("strict")), Some(&Tag::Byte(0)));
    assert!(!fields.contains_key(&key("keepPacked")), "ProtoChunk has no LevelChunk packing flag");
    assert_eq!(fields.get(&key("components")), Some(&Tag::Compound(BTreeMap::new())));
    assert_eq!(fields.get(&key("mode")), Some(&string("SAVE")));
    assert_eq!(fields.get(&key("name")), Some(&string("minecraft:trial_chambers/chamber/addon/walkway_with_bridge_1")));
    assert_eq!(fields.get(&key("integrity")), Some(&Tag::Float(1.0f32.to_bits())));
    for (name, value) in ["x", "y", "z"].into_iter().zip([4, 5, 6]) {
        assert_eq!(fields.get(&key(name)), Some(&Tag::Int(value)));
    }
    actual.canonical_java_nbt().unwrap();
}
