//! Template block actors compiled to native ids and network NBT children.
//!
//! Unknown fields retain their original NBT types and values. This is a lossless
//! transport boundary, not a claim that Bedrock interprets every Java-only field.

use crate::nbt::{self, Tag};
use crate::{Error, Result};

type CompileResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Clone)]
pub struct CompiledBlockEntity {
    pub id: &'static str,
    /// Named NETWORK-endian child tags, without root header/end or id/xyz.
    pub fields: Vec<u8>,
    pub loot: Option<(String, i64)>,
    pub spawner_entity: Option<String>,
}

/// Explicit actor identities, including every non-marker id in the pinned
/// 1.21.11 server's 1202 templates. Unknown identities must not become chests.
fn native_id(id: &str) -> CompileResult<&'static str> {
    Ok(match id {
        "Chest" | "minecraft:chest" | "minecraft:trapped_chest" => "Chest",
        "Barrel" | "minecraft:barrel" => "Barrel",
        "Beacon" | "minecraft:beacon" => "Beacon",
        "Dispenser" | "minecraft:dispenser" => "Dispenser",
        "Dropper" | "minecraft:dropper" => "Dropper",
        "MobSpawner" | "minecraft:mob_spawner" | "minecraft:spawner" => "MobSpawner",
        "TrialSpawner" | "minecraft:trial_spawner" => "TrialSpawner",
        "Campfire" | "minecraft:campfire" => "Campfire",
        "Furnace" | "minecraft:furnace" => "Furnace",
        "BlastFurnace" | "minecraft:blast_furnace" => "BlastFurnace",
        "Smoker" | "minecraft:smoker" => "Smoker",
        "BrewingStand" | "minecraft:brewing_stand" => "BrewingStand",
        "Bed" | "minecraft:bed" => "Bed",
        "Bell" | "minecraft:bell" => "Bell",
        "Lectern" | "minecraft:lectern" => "Lectern",
        "Banner" | "minecraft:banner" => "Banner",
        "Sign" | "minecraft:sign" => "Sign",
        "Vault" | "minecraft:vault" => "Vault",
        "DecoratedPot" | "minecraft:decorated_pot" => "DecoratedPot",
        "Hopper" | "minecraft:hopper" => "Hopper",
        "EnderChest" | "minecraft:ender_chest" => "EnderChest",
        "Skull" | "minecraft:skull" => "Skull",
        "Comparator" | "minecraft:comparator" => "Comparator",
        "SculkSensor" | "minecraft:sculk_sensor" => "SculkSensor",
        "SculkCatalyst" | "minecraft:sculk_catalyst" => "SculkCatalyst",
        "SculkShrieker" | "minecraft:sculk_shrieker" => "SculkShrieker",
        // Native actor identifier: https://minecraft.wiki/w/Copper_Golem_Statue#ID
        "CopperGolemStatue" | "minecraft:copper_golem_statue" => "CopperGolemStatue",
        // Native identities observed in a supported-block BDS 1.26.51 control.
        "CreakingHeart" | "minecraft:creaking_heart" => "CreakingHeart",
        "Shelf" | "minecraft:shelf" => "Shelf",
        "PotentSulfurBlock" => "PotentSulfurBlock",
        "StructureBlock" | "minecraft:structure_block" => "StructureBlock",
        "BrushableBlock" | "minecraft:brushable_block" => "BrushableBlock",
        "ShulkerBox" | "minecraft:shulker_box" => "ShulkerBox",
        // Explicit Bedrock actor identities, not casing rules applied to an
        // arbitrary Java id. See https://minecraft.wiki/w/Block_entity and the
        // Bedrock savegame-id tables on the corresponding block pages.
        "Beehive" | "minecraft:beehive" => "Beehive",
        "CalibratedSculkSensor" | "minecraft:calibrated_sculk_sensor" => "CalibratedSculkSensor",
        "ChiseledBookshelf" | "minecraft:chiseled_bookshelf" => "ChiseledBookshelf",
        "CommandBlock" | "minecraft:command_block" => "CommandBlock",
        "Conduit" | "minecraft:conduit" => "Conduit",
        "Crafter" | "minecraft:crafter" => "Crafter",
        "DaylightDetector" | "minecraft:daylight_detector" => "DaylightDetector",
        "EnchantTable" | "minecraft:enchanting_table" => "EnchantTable",
        "EndGateway" | "minecraft:end_gateway" => "EndGateway",
        "EndPortal" | "minecraft:end_portal" => "EndPortal",
        "HangingSign" | "minecraft:hanging_sign" => "HangingSign",
        "JigsawBlock" | "minecraft:jigsaw" => "JigsawBlock",
        "Jukebox" | "minecraft:jukebox" => "Jukebox",
        _ => return Err(format!("unsupported block actor id {id:?}").into()),
    })
}

/// Actor field schema, independent of the generator that consumes the template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Java,
    Bedrock,
}

/// Compile Java or native Bedrock actor metadata with contextual diagnostics.
pub fn compile(origin: &str, tag: &Tag, source: Source) -> Result<CompiledBlockEntity> {
    compile_inner(tag, source).map_err(|error| Error::Data {
        origin: origin.to_owned(),
        reason: error.to_string(),
    })
}

fn compile_inner(tag: &Tag, source: Source) -> CompileResult<CompiledBlockEntity> {
    let mut fields = tag
        .as_compound()
        .ok_or("block actor is not a compound")?
        .clone();
    let id = fields.remove("id").ok_or("block actor has no id")?;
    let id = native_id(id.as_str().ok_or("block actor id is not a string")?)?;
    for key in ["x", "y", "z"] {
        fields.remove(key);
    }
    // Native fields are already in the required schema. In particular, BDS
    // stores LootTableSeed as Int, unlike Java's Long. Retain native loot in
    // the pre-encoded body rather than widening it or reinterpreting its seed.
    let loot = if source == Source::Java {
        match fields.remove("LootTable") {
            Some(Tag::String(table)) => {
                let seed = match fields.remove("LootTableSeed") {
                    None => 0,
                    Some(Tag::Long(seed)) => seed,
                    Some(_) => return Err("LootTableSeed is not a Long".into()),
                };
                Some((table, seed))
            }
            Some(_) => return Err("LootTable is not a string".into()),
            None => {
                if fields.contains_key("LootTableSeed") {
                    return Err("LootTableSeed without LootTable".into());
                }
                None
            }
        }
    } else {
        None
    };
    let mut spawner_entity = match fields.remove("EntityIdentifier") {
        Some(Tag::String(entity)) => Some(entity),
        Some(_) => return Err("EntityIdentifier is not a string".into()),
        None => None,
    };
    if id == "MobSpawner" {
        // Modern Java stores SpawnData.entity.id; old templates used SpawnData.id.
        // Lift only the identity. Other entity/spawn metadata stays intact.
        if let Some(spawn_data) = fields.get_mut("SpawnData") {
            let Tag::Compound(data) = spawn_data else {
                return Err("SpawnData is not a compound".into());
            };
            let nested = data.contains_key("entity");
            let entity = if nested {
                let Some(Tag::Compound(entity)) = data.get_mut("entity") else {
                    return Err("SpawnData.entity is not a compound".into());
                };
                entity
            } else {
                &mut *data
            };
            if let Some(value) = entity.remove("id") {
                let Tag::String(value) = value else {
                    return Err("spawner entity id is not a string".into());
                };
                if spawner_entity
                    .as_ref()
                    .is_some_and(|existing| existing != &value)
                {
                    return Err("conflicting spawner entity identifiers".into());
                }
                spawner_entity = Some(value);
            }
            if nested && entity.is_empty() {
                data.remove("entity");
            }
            if data.is_empty() {
                fields.remove("SpawnData");
            }
        }
    }
    Ok(CompiledBlockEntity {
        id,
        fields: nbt::encode_network_fields(&fields)?,
        loot,
        spawner_entity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn controlled_native_actor_identities_match_java_template_conversion() {
        // Authored identities observed in the supported-block BDS 1.26.51
        // control volume, not copied actor payloads or structure templates.
        for (java_id, native_id) in [
            ("minecraft:creaking_heart", "CreakingHeart"),
            ("minecraft:shelf", "Shelf"),
        ] {
            let actor = |id: &str| {
                Tag::Compound(BTreeMap::from([
                    ("id".into(), Tag::String(id.into())),
                    ("x".into(), Tag::Int(10)),
                    ("y".into(), Tag::Int(250)),
                    ("z".into(), Tag::Int(10)),
                ]))
            };
            let native = compile(
                "authored-native-control",
                &actor(native_id),
                Source::Bedrock,
            )
            .unwrap();
            let converted =
                compile("authored-java-identity", &actor(java_id), Source::Java).unwrap();
            assert_eq!(converted.id, native_id);
            assert_eq!(native.id, converted.id);
        }
    }

    #[test]
    fn native_chest_loot_keeps_observed_int_seed_in_compiled_fields() {
        // Authored minimal input matching the tag types of three seed-12345
        // natural chests captured from BDS 1.26.51 (not a copied template).
        let source = Tag::Compound(BTreeMap::from([
            ("id".into(), Tag::String("Chest".into())),
            ("x".into(), Tag::Int(540)),
            ("y".into(), Tag::Int(-35)),
            ("z".into(), Tag::Int(645)),
            (
                "LootTable".into(),
                Tag::String("loot_tables/chests/simple_dungeon.json".into()),
            ),
            ("LootTableSeed".into(), Tag::Int(1_278_802_385)),
        ]));
        let compiled = compile("authored-native-chest", &source, Source::Bedrock).unwrap();
        assert_eq!(
            compiled.loot, None,
            "native static loot is not Java seed data"
        );
        let mut network = vec![10, 0];
        network.extend_from_slice(&compiled.fields);
        network.push(0);
        let tag = nbt::read_compound(
            "compiled fields",
            &network,
            nbt::Encoding::NetworkLittleEndian,
        )
        .unwrap();
        assert_eq!(
            tag,
            Tag::Compound(BTreeMap::from([
                (
                    "LootTable".into(),
                    Tag::String("loot_tables/chests/simple_dungeon.json".into())
                ),
                ("LootTableSeed".into(), Tag::Int(1_278_802_385)),
            ]))
        );
    }
}
