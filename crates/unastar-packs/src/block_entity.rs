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
        "SculkCatalyst" => "SculkCatalyst",
        "SculkShrieker" => "SculkShrieker",
        // Native actor identifier: https://minecraft.wiki/w/Copper_Golem_Statue#ID
        "CopperGolemStatue" | "minecraft:copper_golem_statue" => "CopperGolemStatue",
        "minecraft:creaking_heart" => "minecraft:creaking_heart",
        "PotentSulfurBlock" => "PotentSulfurBlock",
        "StructureBlock" | "minecraft:structure_block" => "StructureBlock",
        "BrushableBlock" | "minecraft:brushable_block" => "BrushableBlock",
        "ShulkerBox" | "minecraft:shulker_box" => "ShulkerBox",
        _ => return Err(format!("unsupported block actor id {id:?}").into()),
    })
}

/// Compile Java or native Bedrock actor metadata with contextual diagnostics.
pub fn compile(origin: &str, tag: &Tag) -> Result<CompiledBlockEntity> {
    compile_inner(tag).map_err(|error| Error::Data {
        origin: origin.to_owned(),
        reason: error.to_string(),
    })
}

fn compile_inner(tag: &Tag) -> CompileResult<CompiledBlockEntity> {
    let mut fields = tag
        .as_compound()
        .ok_or("block actor is not a compound")?
        .clone();
    let id = fields.remove("id").ok_or("block actor has no id")?;
    let id = native_id(id.as_str().ok_or("block actor id is not a string")?)?;
    for key in ["x", "y", "z"] {
        fields.remove(key);
    }
    let loot = match fields.remove("LootTable") {
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
