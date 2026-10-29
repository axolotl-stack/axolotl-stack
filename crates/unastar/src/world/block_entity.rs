//! Sparse immutable actors. Java generation retains typed native recipes until
//! the explicit chunk Java-to-Bedrock boundary; Bedrock actors retain rooted
//! network NBT. Persistence never reconstructs Java from lossy Bedrock fields.

use std::io::{self, Read};

use unastar_noise::features::eval::BlockEntityData;
use unastar_noise::structures::native_actor::{self, Tag as JavaTag};
use valentine::bedrock::codec::Nbt;
use zuri_nbt::{
    NBTTag,
    encoding::{LittleEndian, NetworkLittleEndian},
    reader::Reader,
};

include!("native_actor_blocks.rs");

/// An actor at an absolute block position, in its source edition's data space.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockEntity {
    position: [i32; 3],
    data: ActorData,
}

#[derive(Clone, Debug, PartialEq)]
enum ActorData {
    Bedrock(Nbt),
    Java(JavaRecipe),
    /// WorldGenRegion's uninstantiated ProtoChunk actor; the source recipe is
    /// retained separately from the exact native DUMMY save payload.
    JavaPending(JavaRecipe),
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct JavaRecipe {
    block: &'static str,
    data: BlockEntityData,
}

/// A compiler-lifted loot table and its already-drawn seed.
#[derive(Clone, Copy, Debug)]
pub enum LootTable<'a> {
    Java(&'a str, i64),
    Bedrock(&'a str, i64),
}

impl BlockEntity {
    pub fn position(&self) -> [i32; 3] {
        self.position
    }

    pub fn network_nbt(&self) -> &Nbt {
        match &self.data {
            ActorData::Bedrock(nbt) => nbt,
            ActorData::Java(_) | ActorData::JavaPending(_) => panic!(
                "native Java actor must be explicitly converted before Bedrock network encoding"
            ),
        }
    }

    /// Validate externally supplied rooted network NBT, including the entire input.
    pub fn from_network_nbt(nbt: Nbt) -> io::Result<Self> {
        let mut input = nbt.0.as_ref();
        let tag = NBTTag::read(&mut input, NetworkReader).map_err(invalid_nbt)?;
        if !input.is_empty() {
            return Err(invalid_nbt("trailing block-entity NBT bytes"));
        }
        Ok(Self {
            position: position_of(&tag)?,
            data: ActorData::Bedrock(nbt),
        })
    }

    /// Read one standard Bedrock block actor; leave subsequent actors in `input`.
    pub fn read_disk_nbt(input: &mut &[u8]) -> io::Result<Self> {
        let tag = NBTTag::read(input, LittleEndian).map_err(invalid_nbt)?;
        let position = position_of(&tag)?;
        let mut bytes = Vec::new();
        tag.write(&mut bytes, NetworkLittleEndian)
            .map_err(invalid_nbt)?;
        Ok(Self {
            position,
            data: ActorData::Bedrock(Nbt(bytes.into())),
        })
    }

    /// Append one fixed-width little-endian rooted compound for the 0x31 value.
    pub fn write_disk_nbt(&self, out: &mut Vec<u8>) -> io::Result<()> {
        let ActorData::Bedrock(nbt) = &self.data else {
            return Err(invalid_nbt(
                "native Java actor must be explicitly converted before Bedrock persistence",
            ));
        };
        let tag = NBTTag::read(nbt.0.as_ref(), NetworkReader).map_err(invalid_nbt)?;
        tag.write(out, LittleEndian).map_err(invalid_nbt)
    }

    /// Construct a trusted Bedrock actor without decoding its AOT child prefix.
    ///
    /// `fields` excludes root framing, id, xyz and lifted loot/spawner fields.
    pub fn generated(
        position: [i32; 3],
        id: &str,
        fields: &[u8],
        loot: Option<LootTable<'_>>,
        spawner: Option<&str>,
    ) -> Self {
        let loot_len = match loot {
            Some(LootTable::Java(path, _)) | Some(LootTable::Bedrock(path, _)) => path.len(),
            None => 0,
        };
        let mut out = Vec::with_capacity(
            fields.len() + id.len() + loot_len + spawner.map_or(0, str::len) + 160,
        );
        out.extend_from_slice(&[10, 0]);
        string_tag(&mut out, "id", id);
        out.extend_from_slice(fields);
        if let Some(loot) = loot {
            named(&mut out, 8, "LootTable");
            let seed = match loot {
                LootTable::Java(path, seed) => {
                    let path = path.strip_prefix("minecraft:").unwrap_or(path);
                    let prefix = b"loot_tables/";
                    let suffix = b".json";
                    varuint(&mut out, (prefix.len() + path.len() + suffix.len()) as u64);
                    out.extend_from_slice(prefix);
                    out.extend_from_slice(path.as_bytes());
                    out.extend_from_slice(suffix);
                    seed
                }
                LootTable::Bedrock(path, seed) => {
                    string(&mut out, path);
                    seed
                }
            };
            named(&mut out, 4, "LootTableSeed");
            varuint(&mut out, ((seed as u64) << 1) ^ ((seed >> 63) as u64));
        }
        if let Some(entity) = spawner {
            string_tag(&mut out, "EntityIdentifier", entity);
        }
        for (name, value) in ["x", "y", "z"].into_iter().zip(position) {
            named(&mut out, 3, name);
            varuint(
                &mut out,
                (((value as u32) << 1) ^ ((value >> 31) as u32)) as u64,
            );
        }
        out.push(0);
        Self {
            position,
            data: ActorData::Bedrock(Nbt(out.into())),
        }
    }

    /// Retain source-native metadata, without materializing any Bedrock bytes.
    pub fn generated_java(position: [i32; 3], block: &'static str, data: BlockEntityData) -> Self {
        Self {
            position,
            data: ActorData::Java(JavaRecipe { block, data }),
        }
    }

    /// Queue the native ProtoChunk marker, without instantiating constructor
    /// defaults or translating its id into an edition-specific actor id.
    pub fn pending_java(position: [i32; 3], block: &'static str) -> Self {
        Self {
            position,
            data: ActorData::JavaPending(JavaRecipe {
                block,
                data: BlockEntityData {
                    loot: None,
                    spawner_entity: None,
                    template_nbt: None,
                },
            }),
        }
    }

    pub fn is_java_pending(&self) -> bool {
        matches!(self.data, ActorData::JavaPending(_))
    }

    /// Native LevelChunk promotion of a ProtoChunk `DUMMY` marker. The actor is
    /// instantiated from the block *currently* at its position; a block without
    /// a native constructor (including `moving_piston`'s null factory) drops it.
    /// Returns whether the actor remains. Live actors are unchanged.
    pub(crate) fn promote_java_pending(&mut self, current_block: &'static str) -> bool {
        if !self.is_java_pending() {
            return true;
        }
        let constructs = NATIVE_ACTOR_BLOCKS
            .binary_search_by_key(&current_block, |&(block, _)| block)
            .is_ok_and(|index| NATIVE_ACTOR_BLOCKS[index].1.is_some());
        if constructs {
            self.data = ActorData::Java(JavaRecipe {
                block: current_block,
                data: BlockEntityData {
                    loot: None,
                    spawner_entity: None,
                    template_nbt: None,
                },
            });
        }
        constructs
    }

    /// Native feature setters mutate the instantiated actor, retaining its
    /// original type/template even if another actor block now occupies the cell.
    pub(crate) fn with_java_feature_data(&self, data: BlockEntityData) -> Self {
        assert!(
            data.template_nbt.is_none(),
            "template loads replace, rather than mutate, an actor"
        );
        let mut recipe = match self.data {
            ActorData::Java(recipe) | ActorData::JavaPending(recipe) => recipe,
            ActorData::Bedrock(_) => panic!("native feature cannot mutate a Bedrock actor"),
        };
        if data.loot.is_some() {
            recipe.data.loot = data.loot;
        }
        if data.spawner_entity.is_some() {
            recipe.data.spawner_entity = data.spawner_entity;
        }
        Self {
            position: self.position,
            data: ActorData::Java(recipe),
        }
    }

    /// Complete native ProtoChunk save NBT, including native defaults/components
    /// for live actors, exact pending markers and loot seed semantics.
    /// Returns an error for Bedrock actors; no inverse conversion exists.
    pub fn java_nbt(&self) -> io::Result<Vec<u8>> {
        self.java_tag()?.write_root()
    }

    /// Export the reference capture's lossless typed `{type,value}` schema.
    /// Compound keys are sorted; float values are their IEEE bits and lists
    /// include their native element type, even when empty.
    pub fn canonical_java_nbt(&self) -> io::Result<serde_json::Value> {
        canonical_tag(&self.java_tag()?)
    }

    /// Save an instantiated native actor at the LevelChunk boundary. Pending
    /// markers require promotion using their chunk's current block context;
    /// exporting them as either live or packed without that context is invalid.
    pub fn canonical_java_level_nbt(&self) -> io::Result<serde_json::Value> {
        if self.is_java_pending() {
            return Err(invalid_nbt(
                "pending actor requires native LevelChunk promotion context",
            ));
        }
        let mut tag = self.java_tag()?;
        tag.compound_mut()?
            .insert(native_actor::key("keepPacked"), JavaTag::Byte(0));
        canonical_tag(&tag)
    }

    fn java_tag(&self) -> io::Result<JavaTag> {
        let recipe = match self.data {
            ActorData::Java(recipe) => recipe,
            ActorData::JavaPending(_) => {
                return Ok(JavaTag::Compound(std::collections::BTreeMap::from([
                    (native_actor::key("id"), native_actor::string("DUMMY")),
                    (native_actor::key("x"), JavaTag::Int(self.position[0])),
                    (native_actor::key("y"), JavaTag::Int(self.position[1])),
                    (native_actor::key("z"), JavaTag::Int(self.position[2])),
                ])));
            }
            ActorData::Bedrock(_) => {
                return Err(invalid_nbt("Bedrock actor has no canonical Java payload"));
            }
        };
        let mut tag = if let Some(template) = recipe.data.template_nbt {
            let mut source = JavaTag::read_root(template.source)?;
            native_actor::apply_normalization(&mut source, template.normalization)?;
            source
        } else {
            let block = recipe
                .block
                .strip_prefix("minecraft:")
                .unwrap_or(recipe.block);
            let variant = if block == "spawner" {
                match recipe.data.spawner_entity {
                    Some("minecraft:zombie") => "spawner/zombie",
                    Some("minecraft:skeleton") => "spawner/skeleton",
                    Some("minecraft:spider") => "spawner/spider",
                    Some("minecraft:cave_spider") => "spawner/cave_spider",
                    Some("minecraft:blaze") => "spawner/blaze",
                    Some("minecraft:silverfish") => "spawner/silverfish",
                    None => "spawner",
                    Some(other) => {
                        return Err(invalid_nbt(format!(
                            "unmeasured native runtime spawner {other}"
                        )));
                    }
                }
            } else {
                block
            };
            // Both tables are native constructor saves; the promoted table covers
            // every registered actor block and is checked against the direct one.
            let bytes = native_actor::runtime_default(variant)
                .or_else(|| {
                    (variant == block)
                        .then(|| native_actor::promoted_default(recipe.block))
                        .flatten()
                })
                .ok_or_else(|| invalid_nbt(format!("unmeasured native runtime actor {variant}")))?;
            JavaTag::read_root(bytes)?
        };
        let fields = tag.compound_mut()?;
        // Constructor/template observations also cover LevelChunk disk saves.
        // ProtoChunk saves live actors directly, before LevelChunk adds this flag.
        fields.remove(&native_actor::key("keepPacked"));
        if let Some((table, seed)) = recipe.data.loot {
            fields.remove(&native_actor::key("Items"));
            fields.insert(native_actor::key("LootTable"), native_actor::string(table));
            // Native RandomizableContainer/Brushable/DecoratedPot saves omit
            // the seed tag when it is zero, and never materialize loot Items.
            fields.remove(&native_actor::key("LootTableSeed"));
            if seed != 0 {
                fields.insert(native_actor::key("LootTableSeed"), JavaTag::Long(seed));
            }
        }
        if recipe.block == "minecraft:spawner"
            && recipe.data.template_nbt.is_some()
            && let Some(entity) = recipe.data.spawner_entity
        {
            // BaseSpawner.setEntityId changes SpawnData.entity.id only. In
            // particular, existing weighted SpawnPotentials are not rebuilt.
            let spawn = fields
                .get_mut(&native_actor::key("SpawnData"))
                .ok_or_else(|| invalid_nbt("native spawner save has no SpawnData"))?
                .compound_mut()?;
            let entity_data = spawn
                .get_mut(&native_actor::key("entity"))
                .ok_or_else(|| invalid_nbt("native SpawnData has no entity"))?
                .compound_mut()?;
            entity_data.insert(native_actor::key("id"), native_actor::string(entity));
        }
        for (name, value) in ["x", "y", "z"].into_iter().zip(self.position) {
            fields.insert(native_actor::key(name), JavaTag::Int(value));
        }
        Ok(tag)
    }

    /// The sole Java actor adaptation boundary. Reuse the existing Bedrock
    /// compiler on original source fields, not on normalized Java save fields:
    /// this preserves Bedrock's established template behavior by construction.
    pub fn convert_to_bedrock(&mut self) -> io::Result<()> {
        let recipe = match self.data {
            ActorData::Java(recipe) | ActorData::JavaPending(recipe) => recipe,
            ActorData::Bedrock(_) => return Ok(()),
        };
        let source = if let Some(template) = recipe.data.template_nbt {
            packs_tag(JavaTag::read_root(template.source)?)?
        } else {
            let id = NATIVE_ACTOR_BLOCKS
                .binary_search_by_key(&recipe.block, |&(block, _)| block)
                .ok()
                .and_then(|index| NATIVE_ACTOR_BLOCKS[index].1)
                .ok_or_else(|| {
                    invalid_nbt(format!(
                        "native actor {} has no constructor for Bedrock adaptation",
                        recipe.block
                    ))
                })?;
            unastar_packs::nbt::Tag::Compound(std::collections::BTreeMap::from([(
                "id".into(),
                unastar_packs::nbt::Tag::String(id.into()),
            )]))
        };
        let compiled = unastar_packs::block_entity::compile(
            recipe.block,
            &source,
            unastar_packs::block_entity::Source::Java,
        )
        .map_err(invalid_nbt)?;
        let loot = recipe
            .data
            .loot
            .map(|(table, seed)| LootTable::Java(table, seed))
            .or_else(|| {
                compiled
                    .loot
                    .as_ref()
                    .map(|(table, seed)| LootTable::Java(table, *seed))
            });
        let entity = recipe
            .data
            .spawner_entity
            .or(compiled.spawner_entity.as_deref());
        *self = Self::generated(self.position, compiled.id, &compiled.fields, loot, entity);
        Ok(())
    }
}

fn java_string(units: &[u16]) -> io::Result<String> {
    String::from_utf16(units).map_err(invalid_nbt)
}

fn canonical_tag(tag: &JavaTag) -> io::Result<serde_json::Value> {
    use serde_json::json;
    let (kind, value) = match tag {
        JavaTag::Byte(v) => ("Byte", json!(v)),
        JavaTag::Short(v) => ("Short", json!(v)),
        JavaTag::Int(v) => ("Int", json!(v)),
        JavaTag::Long(v) => ("Long", json!(v)),
        JavaTag::Float(v) => ("Float", json!(v)),
        JavaTag::Double(v) => ("Double", json!(v)),
        JavaTag::ByteArray(v) => ("ByteArray", json!(v)),
        JavaTag::String(v) => ("Str", json!(java_string(v)?)),
        JavaTag::List(element, values) => (
            "List",
            json!([
                element,
                values
                    .iter()
                    .map(canonical_tag)
                    .collect::<io::Result<Vec<_>>>()?
            ]),
        ),
        JavaTag::Compound(fields) => (
            "Compound",
            json!(
                fields
                    .iter()
                    .map(|(name, value)| Ok(json!([java_string(name)?, canonical_tag(value)?])))
                    .collect::<io::Result<Vec<_>>>()?
            ),
        ),
        JavaTag::IntArray(v) => ("IntArray", json!(v)),
        JavaTag::LongArray(v) => ("LongArray", json!(v)),
    };
    Ok(json!({ "type": kind, "value": value }))
}

fn packs_tag(tag: JavaTag) -> io::Result<unastar_packs::nbt::Tag> {
    use unastar_packs::nbt::Tag;
    Ok(match tag {
        JavaTag::Byte(v) => Tag::Byte(v),
        JavaTag::Short(v) => Tag::Short(v),
        JavaTag::Int(v) => Tag::Int(v),
        JavaTag::Long(v) => Tag::Long(v),
        JavaTag::Float(v) => Tag::Float(f32::from_bits(v)),
        JavaTag::Double(v) => Tag::Double(f64::from_bits(v)),
        JavaTag::ByteArray(v) => Tag::ByteArray(v),
        JavaTag::String(v) => Tag::String(java_string(&v)?),
        JavaTag::List(_, values) => Tag::List(
            values
                .into_iter()
                .map(packs_tag)
                .collect::<io::Result<_>>()?,
        ),
        JavaTag::Compound(fields) => Tag::Compound(
            fields
                .into_iter()
                .map(|(name, value)| Ok((java_string(&name)?, packs_tag(value)?)))
                .collect::<io::Result<_>>()?,
        ),
        JavaTag::IntArray(v) => Tag::IntArray(v),
        JavaTag::LongArray(v) => Tag::LongArray(v),
    })
}

fn invalid_nbt(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

fn position_of(tag: &NBTTag) -> io::Result<[i32; 3]> {
    let NBTTag::Compound(fields) = tag else {
        return Err(invalid_nbt("block-entity root must be a compound"));
    };
    if !matches!(fields.0.get("id"), Some(NBTTag::String(id)) if !id.0.is_empty()) {
        return Err(invalid_nbt("block entity needs a nonempty string id"));
    }
    let mut position = [0; 3];
    for (index, name) in ["x", "y", "z"].into_iter().enumerate() {
        let Some(NBTTag::Int(value)) = fields.0.get(name) else {
            return Err(invalid_nbt(format!("block entity needs Int {name}")));
        };
        position[index] = value.0;
    }
    Ok(position)
}

fn varuint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 128 {
        out.push(value as u8 | 128);
        value >>= 7;
    }
    out.push(value as u8);
}

fn string(out: &mut Vec<u8>, value: &str) {
    varuint(out, value.len() as u64);
    out.extend_from_slice(value.as_bytes());
}

fn named(out: &mut Vec<u8>, kind: u8, name: &str) {
    out.push(kind);
    string(out, name);
}

fn string_tag(out: &mut Vec<u8>, name: &str, value: &str) {
    named(out, 8, name);
    string(out, value);
}

// zuri_nbt 0.4's network reader loses one on every negative zigzag value.
// Override signed varints at the persistence boundary, including nested arrays/lists.
struct NetworkReader;

impl NetworkReader {
    fn unsigned(&mut self, input: &mut impl Read, bits: u32) -> zuri_nbt::reader::Res<u64> {
        let mut value = 0u64;
        for shift in (0..bits).step_by(7) {
            let byte = self.u8(input)?;
            if shift + 7 > bits && (byte as u64) >= (1u64 << (bits - shift)) {
                break;
            }
            value |= ((byte & 127) as u64) << shift;
            if byte & 128 == 0 {
                return Ok(value);
            }
        }
        Err(zuri_nbt::err::NBTError::new(
            zuri_nbt::err::ReadError::Custom("NBT varint overflow".into()),
        ))
    }
}

impl Reader for NetworkReader {
    fn i16<R: Read>(&mut self, input: &mut R) -> zuri_nbt::reader::Res<i16> {
        NetworkLittleEndian.i16(input)
    }
    fn i32<R: Read>(&mut self, input: &mut R) -> zuri_nbt::reader::Res<i32> {
        let value = self.unsigned(input, 32)? as u32;
        Ok(((value >> 1) as i32) ^ -((value & 1) as i32))
    }
    fn i64<R: Read>(&mut self, input: &mut R) -> zuri_nbt::reader::Res<i64> {
        let value = self.unsigned(input, 64)?;
        Ok(((value >> 1) as i64) ^ -((value & 1) as i64))
    }
    fn f32<R: Read>(&mut self, input: &mut R) -> zuri_nbt::reader::Res<f32> {
        NetworkLittleEndian.f32(input)
    }
    fn f64<R: Read>(&mut self, input: &mut R) -> zuri_nbt::reader::Res<f64> {
        NetworkLittleEndian.f64(input)
    }
    fn string<R: Read>(&mut self, input: &mut R) -> zuri_nbt::reader::Res<String> {
        NetworkLittleEndian.string(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_live_proto_and_level_save_boundaries_preserve_typed_payloads() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/native_live_actor_save_boundaries_1_21_11.json"
        ))
        .unwrap();
        for case in fixture["records"].as_array().unwrap() {
            let position = std::array::from_fn(|axis| {
                i32::try_from(case["position"][axis].as_i64().unwrap()).unwrap()
            });
            let block = match case["block"].as_str().unwrap() {
                "minecraft:chest" => "minecraft:chest",
                "minecraft:spawner" => "minecraft:spawner",
                other => panic!("unmeasured native live actor {other}"),
            };
            let loot = case.get("loot_table").map(|table| {
                assert_eq!(table.as_str(), Some("minecraft:chests/simple_dungeon"));
                (
                    "minecraft:chests/simple_dungeon",
                    case["loot_seed"].as_i64().unwrap(),
                )
            });
            let spawner_entity =
                case.get("spawner_entity")
                    .map(|entity| match entity.as_str().unwrap() {
                        "minecraft:zombie" => "minecraft:zombie",
                        "minecraft:cave_spider" => "minecraft:cave_spider",
                        other => panic!("unmeasured native live spawner {other}"),
                    });
            let actor = BlockEntity::generated_java(
                position,
                block,
                BlockEntityData {
                    loot,
                    spawner_entity,
                    template_nbt: None,
                },
            );
            assert_eq!(actor.canonical_java_nbt().unwrap(), case["proto_saved"]);
            assert_eq!(
                actor.canonical_java_level_nbt().unwrap(),
                case["level_saved"]
            );
        }
    }

    fn semantic(entity: &BlockEntity) -> NBTTag {
        let mut disk = Vec::new();
        entity.write_disk_nbt(&mut disk).unwrap();
        NBTTag::read(disk.as_slice(), LittleEndian).unwrap()
    }

    fn java_compound<const N: usize>(fields: [(&str, JavaTag); N]) -> JavaTag {
        JavaTag::Compound(
            fields
                .into_iter()
                .map(|(k, v)| (native_actor::key(k), v))
                .collect(),
        )
    }

    #[test]
    fn native_saved_dungeon_chests_keep_exact_seeds_until_explicit_conversion() {
        // Preserve the stopped seed-12345 dungeon actors' source loot and seeds
        // at the earlier ProtoChunk boundary (before LevelChunk adds keepPacked).
        for (position, seed) in [
            ([-7, 36, -16], -6_788_257_060_774_402_705),
            ([-4, 36, -16], 720_486_539_052_466_001),
        ] {
            let table = "minecraft:chests/simple_dungeon";
            let mut actor = BlockEntity::generated_java(
                position,
                "minecraft:chest",
                BlockEntityData::loot(table, seed),
            );
            let expected = java_compound([
                ("LootTable", native_actor::string(table)),
                ("LootTableSeed", JavaTag::Long(seed)),
                ("components", java_compound([])),
                ("id", native_actor::string("minecraft:chest")),
                ("x", JavaTag::Int(position[0])),
                ("y", JavaTag::Int(position[1])),
                ("z", JavaTag::Int(position[2])),
            ]);
            assert_eq!(
                JavaTag::read_root(&actor.java_nbt().unwrap()).unwrap(),
                expected
            );
            assert_eq!(
                actor.canonical_java_nbt().unwrap(),
                canonical_tag(&expected).unwrap()
            );
            assert!(actor.write_disk_nbt(&mut Vec::new()).is_err());
            actor.convert_to_bedrock().unwrap();
            let established = BlockEntity::generated(
                position,
                "Chest",
                &[],
                Some(LootTable::Java(table, seed)),
                None,
            );
            assert_eq!(semantic(&actor), semantic(&established));
            assert!(
                actor.canonical_java_nbt().is_err(),
                "conversion cannot be inverted"
            );
        }
    }

    #[test]
    fn native_pending_recipes_cover_measured_registry_and_fail_atomically() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/java_1_21_11_native_actor_transitions.json",
        )))
        .unwrap();
        let observed = fixture["actor_ids"].as_object().unwrap();
        assert_eq!(NATIVE_ACTOR_BLOCKS.len(), observed.len());
        for &(block, native_id) in NATIVE_ACTOR_BLOCKS {
            assert_eq!(
                observed.get(block).unwrap(),
                &serde_json::json!(native_id),
                "{block}"
            );
            let mut actor = BlockEntity::pending_java([1, 200, 1], block);
            assert_eq!(
                actor.canonical_java_nbt().unwrap(),
                fixture["transitions"][0]["saved"],
                "{block}"
            );
            let before = actor.clone();
            if native_id.is_none()
                || matches!(
                    block,
                    "minecraft:test_block" | "minecraft:test_instance_block"
                )
            {
                assert!(
                    actor.convert_to_bedrock().is_err(),
                    "{block} has no representable native actor"
                );
                assert_eq!(
                    actor, before,
                    "failed conversion must leave the source-native actor intact"
                );
            } else {
                actor.convert_to_bedrock().unwrap();
                assert!(
                    actor.canonical_java_nbt().is_err(),
                    "one-way boundary: {block}"
                );
                BlockEntity::from_network_nbt(actor.network_nbt().clone()).unwrap();
            }
        }
    }

    #[test]
    fn native_pending_variants_use_actual_source_actor_type_at_final_adapter() {
        for (block, expected_id) in [
            ("minecraft:bee_nest", "Beehive"),
            ("minecraft:soul_campfire", "Campfire"),
            ("minecraft:oak_wall_hanging_sign", "HangingSign"),
            ("minecraft:red_bed", "Bed"),
            ("minecraft:blue_wall_banner", "Banner"),
            ("minecraft:sculk_catalyst", "SculkCatalyst"),
            ("minecraft:sculk_shrieker", "SculkShrieker"),
            ("minecraft:calibrated_sculk_sensor", "CalibratedSculkSensor"),
            ("minecraft:end_gateway", "EndGateway"),
            ("minecraft:oak_shelf", "Shelf"),
        ] {
            let mut actor = BlockEntity::pending_java([-1, -2, -3], block);
            assert!(actor.is_java_pending());
            actor.convert_to_bedrock().unwrap();
            let NBTTag::Compound(fields) = semantic(&actor) else {
                panic!("actor is not a compound");
            };
            assert!(
                matches!(fields.0.get("id"), Some(NBTTag::String(id)) if id.0 == expected_id),
                "{block}"
            );
        }
    }

    #[test]
    fn native_zero_loot_seed_and_actual_set_entity_spawner_defaults() {
        let actor = BlockEntity::generated_java(
            [0, 0, 0],
            "minecraft:chest",
            BlockEntityData::loot("minecraft:chests/simple_dungeon", 0),
        );
        let saved = JavaTag::read_root(&actor.java_nbt().unwrap()).unwrap();
        assert!(
            !saved
                .compound()
                .unwrap()
                .contains_key(&native_actor::key("LootTableSeed"))
        );
        assert!(
            !saved
                .compound()
                .unwrap()
                .contains_key(&native_actor::key("Items"))
        );
        // Fresh native setEntityId saves SpawnData, but an End-typed empty
        // SpawnPotentials list. Loading an empty template instead would save
        // a Compound-typed singleton potential, a consumer-visible difference.
        for entity in [
            "minecraft:zombie",
            "minecraft:skeleton",
            "minecraft:spider",
            "minecraft:cave_spider",
            "minecraft:blaze",
            "minecraft:silverfish",
        ] {
            let actor = BlockEntity::generated_java(
                [1, -2, 3],
                "minecraft:spawner",
                BlockEntityData::spawner(entity),
            );
            let expected = java_compound([
                ("components", java_compound([])),
                ("id", native_actor::string("minecraft:mob_spawner")),
                ("x", JavaTag::Int(1)),
                ("y", JavaTag::Int(-2)),
                ("z", JavaTag::Int(3)),
                ("Delay", JavaTag::Short(20)),
                ("MinSpawnDelay", JavaTag::Short(200)),
                ("MaxSpawnDelay", JavaTag::Short(800)),
                ("SpawnCount", JavaTag::Short(4)),
                ("MaxNearbyEntities", JavaTag::Short(6)),
                ("RequiredPlayerRange", JavaTag::Short(16)),
                ("SpawnRange", JavaTag::Short(4)),
                (
                    "SpawnData",
                    java_compound([(
                        "entity",
                        java_compound([("id", native_actor::string(entity))]),
                    )]),
                ),
                ("SpawnPotentials", JavaTag::List(0, vec![])),
            ]);
            assert_eq!(
                JavaTag::read_root(&actor.java_nbt().unwrap()).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn generated_prefix_loot_spawner_and_signed_values_roundtrip() {
        let original = BlockEntity::generated(
            [-17, -64, -1],
            "Chest",
            &[1, 6, b'C', b'u', b's', b't', b'o', b'm', 7],
            Some(LootTable::Java("minecraft:chests/test", i64::MIN)),
            Some("minecraft:pig"),
        );
        let tag = semantic(&original);
        let NBTTag::Compound(fields) = &tag else {
            panic!()
        };
        assert_eq!(
            fields.0.get("LootTable"),
            Some(&NBTTag::String("loot_tables/chests/test.json".into()))
        );
        assert_eq!(
            fields.0.get("LootTableSeed"),
            Some(&NBTTag::Long(i64::MIN.into()))
        );
        assert_eq!(
            fields.0.get("EntityIdentifier"),
            Some(&NBTTag::String("minecraft:pig".into()))
        );
        assert_eq!(fields.0.get("Custom"), Some(&NBTTag::Byte(7.into())));
        let mut disk = Vec::new();
        original.write_disk_nbt(&mut disk).unwrap();
        original.write_disk_nbt(&mut disk).unwrap();
        let mut input = disk.as_slice();
        for _ in 0..2 {
            let decoded = BlockEntity::read_disk_nbt(&mut input).unwrap();
            assert_eq!(decoded.position(), [-17, -64, -1]);
            assert_eq!(semantic(&decoded), tag);
            assert_eq!(
                BlockEntity::from_network_nbt(decoded.network_nbt().clone())
                    .unwrap()
                    .position(),
                [-17, -64, -1]
            );
        }
        assert!(input.is_empty());
    }

    #[test]
    fn external_network_input_requires_compound_id_int_coordinates_and_no_tail() {
        for bytes in [vec![10, 0, 0], vec![8, 0, 0]] {
            assert!(BlockEntity::from_network_nbt(Nbt(bytes.into())).is_err());
        }
        let entity = BlockEntity::generated([0, 0, 0], "Chest", &[], None, None);
        let mut bytes = entity.network_nbt().0.to_vec();
        bytes.push(0);
        assert!(BlockEntity::from_network_nbt(Nbt(bytes.into())).is_err());
        let mut invalid = semantic(&entity);
        let NBTTag::Compound(fields) = &mut invalid else {
            panic!()
        };
        fields.0.insert("x".into(), NBTTag::Long(0.into()));
        let mut bytes = Vec::new();
        invalid.write(&mut bytes, NetworkLittleEndian).unwrap();
        assert!(BlockEntity::from_network_nbt(Nbt(bytes.into())).is_err());
    }
}

#[cfg(test)]
mod signed_boundary_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn persistence_transcodes_all_signed_boundaries_and_nested_payloads_semantically() {
        let custom = NBTTag::Compound(
            HashMap::from([
                (
                    "Ints".into(),
                    NBTTag::IntArray(vec![i32::MIN, -7, -1, 0, i32::MAX].into()),
                ),
                (
                    "Longs".into(),
                    NBTTag::LongArray(vec![i64::MIN, -7, -1, 0, i64::MAX].into()),
                ),
                (
                    "Nested".into(),
                    NBTTag::List(
                        vec![NBTTag::Compound(
                            HashMap::from([(
                                "Negative".into(),
                                NBTTag::Long((-1234567890123i64).into()),
                            )])
                            .into(),
                        )]
                        .into(),
                    ),
                ),
            ])
            .into(),
        );
        let mut compiled = Vec::new();
        custom.write(&mut compiled, NetworkLittleEndian).unwrap();
        let prefix = &compiled[2..compiled.len() - 1];
        for seed in [i64::MIN, -1, 0, i64::MAX] {
            let entity = BlockEntity::generated(
                [i32::MIN, -1, i32::MAX],
                "Chest",
                prefix,
                Some(LootTable::Bedrock("loot_tables/chests/test.json", seed)),
                None,
            );
            let mut disk = Vec::new();
            entity.write_disk_nbt(&mut disk).unwrap();
            let expected = NBTTag::read(disk.as_slice(), LittleEndian).unwrap();
            let NBTTag::Compound(fields) = &expected else {
                panic!()
            };
            let NBTTag::Compound(custom_fields) = &custom else {
                panic!()
            };
            for (name, value) in &custom_fields.0 {
                assert_eq!(fields.0.get(name), Some(value));
            }
            assert_eq!(
                fields.0.get("LootTableSeed"),
                Some(&NBTTag::Long(seed.into()))
            );
            let mut input = disk.as_slice();
            let reopened = BlockEntity::read_disk_nbt(&mut input).unwrap();
            assert!(input.is_empty());
            assert_eq!(reopened.position(), [i32::MIN, -1, i32::MAX]);
            let validated = BlockEntity::from_network_nbt(reopened.network_nbt().clone()).unwrap();
            assert_eq!(validated.position(), entity.position());
            assert_eq!(
                NBTTag::read(validated.network_nbt().0.as_ref(), NetworkReader).unwrap(),
                expected
            );
        }
    }
}
