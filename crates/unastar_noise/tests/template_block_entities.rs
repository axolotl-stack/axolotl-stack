//! AOT conversion boundaries: lifted fields, lossless unknown data and failures.

use unastar_packs::{block_entity, nbt};

use nbt::Tag;
use std::collections::BTreeMap;

fn compound<const N: usize>(fields: [(&str, Tag); N]) -> Tag {
    Tag::Compound(
        fields
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

#[test]
fn lifts_loot_without_duplicate_keys_and_preserves_unknown_network_fields() {
    let source = compound([
        ("id", Tag::String("minecraft:barrel".into())),
        ("x", Tag::Int(123)),
        ("y", Tag::Int(-64)),
        ("z", Tag::Int(456)),
        (
            "LootTable",
            Tag::String("minecraft:chests/village/village_fisher".into()),
        ),
        ("LootTableSeed", Tag::Long(i64::MIN + 1)),
        ("a", Tag::Long(-1)),
        ("b", Tag::Short(-2)),
        (
            "c",
            Tag::Compound(BTreeMap::from([("d".into(), Tag::Int(-3))])),
        ),
    ]);
    let compiled =
        block_entity::compile("barrel.nbt", &source, block_entity::Source::Java).unwrap();
    assert_eq!(compiled.id, "Barrel");
    assert_eq!(
        compiled.loot,
        Some((
            "minecraft:chests/village/village_fisher".into(),
            i64::MIN + 1
        ))
    );
    assert_eq!(compiled.spawner_entity, None);
    // Named children only; network signed varints, LE short, nested compound end.
    // No root/end, Java id/xyz, LootTable or LootTableSeed remains in this prefix.
    assert_eq!(
        compiled.fields,
        [
            4, 1, b'a', 1, 2, 1, b'b', 254, 255, 10, 1, b'c', 3, 1, b'd', 5, 0
        ]
    );
}

#[test]
fn lifts_spawner_identity_without_discarding_spawn_metadata() {
    let source = compound([
        ("id", Tag::String("minecraft:mob_spawner".into())),
        (
            "SpawnData",
            compound([
                (
                    "entity",
                    compound([
                        ("id", Tag::String("minecraft:spider".into())),
                        ("CustomFlag", Tag::Byte(7)),
                    ]),
                ),
                ("custom_spawn_rules", compound([("a", Tag::Int(5))])),
            ]),
        ),
        (
            "SpawnPotentials",
            Tag::List(vec![compound([("weight", Tag::Int(2))])]),
        ),
    ]);
    let compiled =
        block_entity::compile("spawner.nbt", &source, block_entity::Source::Java).unwrap();
    assert_eq!(compiled.id, "MobSpawner");
    assert_eq!(compiled.spawner_entity.as_deref(), Some("minecraft:spider"));
    let expected = BTreeMap::from([
        (
            "SpawnData".into(),
            compound([
                ("entity", compound([("CustomFlag", Tag::Byte(7))])),
                ("custom_spawn_rules", compound([("a", Tag::Int(5))])),
            ]),
        ),
        (
            "SpawnPotentials".into(),
            Tag::List(vec![compound([("weight", Tag::Int(2))])]),
        ),
    ]);
    assert_eq!(
        compiled.fields,
        nbt::encode_network_fields(&expected).unwrap()
    );
}

#[test]
fn unknown_actor_and_malformed_lifted_fields_fail_clearly() {
    let invalid = [
        (
            compound([("id", Tag::String("minecraft:unknown_actor".into()))]),
            "unsupported block actor id",
        ),
        (
            compound([
                ("id", Tag::String("minecraft:chest".into())),
                ("LootTable", Tag::Int(1)),
            ]),
            "LootTable is not a string",
        ),
        (
            compound([
                ("id", Tag::String("minecraft:chest".into())),
                ("LootTableSeed", Tag::Long(1)),
            ]),
            "LootTableSeed without LootTable",
        ),
        (
            compound([
                ("id", Tag::String("minecraft:mob_spawner".into())),
                (
                    "SpawnData",
                    compound([("entity", Tag::String("invalid".into()))]),
                ),
            ]),
            "SpawnData.entity is not a compound",
        ),
    ];
    for (source, message) in invalid {
        let error = block_entity::compile("invalid.nbt", &source, block_entity::Source::Java)
            .expect_err("must reject invalid conversion")
            .to_string();
        assert!(error.contains(message), "{error}");
    }
    let mut source = nbt::encode_java(&compound([("id", Tag::String("minecraft:chest".into()))]));
    source.push(0);
    assert!(
        nbt::read_compound("trailing.nbt", &source, nbt::Encoding::BigEndian)
            .unwrap_err()
            .to_string()
            .contains("trailing bytes")
    );
}
