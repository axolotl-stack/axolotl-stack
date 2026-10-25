//! Sparse, immutable block actors shared by generation, persistence and request-mode networking.
//!
//! Generated actors splice AOT-compiled network NBT children without parsing templates.
//! Only persistence boundaries transcode between network NBT and Bedrock's fixed-LE 0x31 values.

use std::io::{self, Read};

use valentine::bedrock::codec::Nbt;
use zuri_nbt::{
    NBTTag,
    encoding::{LittleEndian, NetworkLittleEndian},
    reader::Reader,
};

/// An actor at an absolute block position, with a rooted network-little-endian compound.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockEntity {
    position: [i32; 3],
    nbt: Nbt,
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
        &self.nbt
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
            nbt,
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
            nbt: Nbt(bytes.into()),
        })
    }

    /// Append one fixed-width little-endian rooted compound for the 0x31 value.
    pub fn write_disk_nbt(&self, out: &mut Vec<u8>) -> io::Result<()> {
        let tag = NBTTag::read(self.nbt.0.as_ref(), NetworkReader).map_err(invalid_nbt)?;
        tag.write(out, LittleEndian).map_err(invalid_nbt)
    }

    /// Construct a trusted generated actor without decoding the AOT child-tag prefix.
    ///
    /// `fields` excludes root framing, id, xyz and the lifted loot/spawner fields.
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
            nbt: Nbt(out.into()),
        }
    }
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

    fn semantic(entity: &BlockEntity) -> NBTTag {
        let mut disk = Vec::new();
        entity.write_disk_nbt(&mut disk).unwrap();
        NBTTag::read(disk.as_slice(), LittleEndian).unwrap()
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
