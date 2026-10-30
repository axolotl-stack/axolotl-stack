//! Java actor recipes and lossless Java NBT. Strings retain UTF-16 code units,
//! floats retain their bits, and lists retain their declared element type.
use std::collections::BTreeMap;
use std::io::{self, Read};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(u32),
    Double(u64),
    ByteArray(Vec<i8>),
    String(Vec<u16>),
    List(u8, Vec<Tag>),
    Compound(BTreeMap<Vec<u16>, Tag>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

pub fn key(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}
pub fn string(value: &str) -> Tag {
    Tag::String(key(value))
}
pub fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

impl Tag {
    pub fn compound(&self) -> io::Result<&BTreeMap<Vec<u16>, Tag>> {
        if let Self::Compound(fields) = self {
            Ok(fields)
        } else {
            Err(invalid("expected Java compound"))
        }
    }
    pub fn compound_mut(&mut self) -> io::Result<&mut BTreeMap<Vec<u16>, Tag>> {
        if let Self::Compound(fields) = self {
            Ok(fields)
        } else {
            Err(invalid("expected Java compound"))
        }
    }
    pub fn kind(&self) -> u8 {
        match self {
            Self::Byte(_) => 1,
            Self::Short(_) => 2,
            Self::Int(_) => 3,
            Self::Long(_) => 4,
            Self::Float(_) => 5,
            Self::Double(_) => 6,
            Self::ByteArray(_) => 7,
            Self::String(_) => 8,
            Self::List(..) => 9,
            Self::Compound(_) => 10,
            Self::IntArray(_) => 11,
            Self::LongArray(_) => 12,
        }
    }
    pub fn read_root(bytes: &[u8]) -> io::Result<Self> {
        let mut input = bytes;
        if byte(&mut input)? != 10 {
            return Err(invalid("Java actor root is not a compound"));
        }
        read_string(&mut input)?;
        let tag = payload(&mut input, 10, 0)?;
        if !input.is_empty() {
            return Err(invalid("trailing Java actor NBT"));
        }
        Ok(tag)
    }
    pub fn write_root(&self) -> io::Result<Vec<u8>> {
        self.compound()?;
        let mut out = vec![10, 0, 0];
        self.write_payload(&mut out)?;
        Ok(out)
    }
    fn write_payload(&self, out: &mut Vec<u8>) -> io::Result<()> {
        match self {
            Self::Byte(v) => out.push(*v as u8),
            Self::Short(v) => out.extend(v.to_be_bytes()),
            Self::Int(v) => out.extend(v.to_be_bytes()),
            Self::Long(v) => out.extend(v.to_be_bytes()),
            Self::Float(v) => out.extend(v.to_be_bytes()),
            Self::Double(v) => out.extend(v.to_be_bytes()),
            Self::String(v) => write_string(out, v)?,
            Self::ByteArray(v) => {
                length(out, v.len())?;
                out.extend(v.iter().map(|v| *v as u8));
            }
            Self::IntArray(v) => {
                length(out, v.len())?;
                for n in v {
                    out.extend(n.to_be_bytes());
                }
            }
            Self::LongArray(v) => {
                length(out, v.len())?;
                for n in v {
                    out.extend(n.to_be_bytes());
                }
            }
            Self::List(kind, values) => {
                out.push(*kind);
                length(out, values.len())?;
                for v in values {
                    if v.kind() != *kind {
                        return Err(invalid("heterogeneous Java NBT list"));
                    }
                    v.write_payload(out)?;
                }
            }
            Self::Compound(fields) => {
                for (k, v) in fields {
                    out.push(v.kind());
                    write_string(out, k)?;
                    v.write_payload(out)?;
                }
                out.push(0);
            }
        }
        Ok(())
    }
}
fn length(out: &mut Vec<u8>, n: usize) -> io::Result<()> {
    out.extend(i32::try_from(n).map_err(invalid)?.to_be_bytes());
    Ok(())
}
fn fixed<const N: usize>(input: &mut &[u8]) -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}
fn byte(input: &mut &[u8]) -> io::Result<u8> {
    Ok(fixed::<1>(input)?[0])
}
fn count(input: &mut &[u8]) -> io::Result<usize> {
    let n = i32::from_be_bytes(fixed(input)?);
    if n < 0 || n as usize > input.len() {
        return Err(invalid("invalid Java NBT length"));
    }
    Ok(n as usize)
}
fn read_string(input: &mut &[u8]) -> io::Result<Vec<u16>> {
    let n = u16::from_be_bytes(fixed(input)?) as usize;
    if n > input.len() {
        return Err(invalid("truncated modified UTF-8"));
    }
    let (bytes, rest) = input.split_at(n);
    *input = rest;
    let mut units = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        let b = bytes[i];
        i += 1;
        let unit = if b & 0x80 == 0 {
            b as u16
        } else if b & 0xe0 == 0xc0 {
            let c = *bytes
                .get(i)
                .ok_or_else(|| invalid("truncated modified UTF-8"))?;
            i += 1;
            if c & 0xc0 != 0x80 {
                return Err(invalid("invalid modified UTF-8 continuation"));
            }
            ((b as u16 & 31) << 6) | (c as u16 & 63)
        } else if b & 0xf0 == 0xe0 {
            let c = *bytes
                .get(i)
                .ok_or_else(|| invalid("truncated modified UTF-8"))?;
            let d = *bytes
                .get(i + 1)
                .ok_or_else(|| invalid("truncated modified UTF-8"))?;
            i += 2;
            if c & 0xc0 != 0x80 || d & 0xc0 != 0x80 {
                return Err(invalid("invalid modified UTF-8 continuation"));
            }
            ((b as u16 & 15) << 12) | ((c as u16 & 63) << 6) | (d as u16 & 63)
        } else {
            return Err(invalid("invalid modified UTF-8 leading byte"));
        };
        units.push(unit);
    }
    Ok(units)
}
fn write_string(out: &mut Vec<u8>, value: &[u16]) -> io::Result<()> {
    let n: usize = value
        .iter()
        .map(|&u| {
            if (1..=127).contains(&u) {
                1
            } else if u <= 2047 {
                2
            } else {
                3
            }
        })
        .sum();
    out.extend(u16::try_from(n).map_err(invalid)?.to_be_bytes());
    for &u in value {
        if (1..=127).contains(&u) {
            out.push(u as u8);
        } else if u <= 2047 {
            out.extend([0xc0 | (u >> 6) as u8, 0x80 | (u & 63) as u8]);
        } else {
            out.extend([
                0xe0 | (u >> 12) as u8,
                0x80 | ((u >> 6) & 63) as u8,
                0x80 | (u & 63) as u8,
            ]);
        }
    }
    Ok(())
}
fn payload(input: &mut &[u8], kind: u8, depth: usize) -> io::Result<Tag> {
    if depth > 512 {
        return Err(invalid("Java NBT nesting exceeds 512"));
    }
    Ok(match kind {
        1 => Tag::Byte(byte(input)? as i8),
        2 => Tag::Short(i16::from_be_bytes(fixed(input)?)),
        3 => Tag::Int(i32::from_be_bytes(fixed(input)?)),
        4 => Tag::Long(i64::from_be_bytes(fixed(input)?)),
        5 => Tag::Float(u32::from_be_bytes(fixed(input)?)),
        6 => Tag::Double(u64::from_be_bytes(fixed(input)?)),
        7 => {
            let n = count(input)?;
            Tag::ByteArray(
                (0..n)
                    .map(|_| byte(input).map(|v| v as i8))
                    .collect::<io::Result<_>>()?,
            )
        }
        8 => Tag::String(read_string(input)?),
        9 => {
            let child = byte(input)?;
            let n = count(input)?;
            if child > 12 || (child == 0 && n != 0) {
                return Err(invalid("invalid Java list element type"));
            }
            Tag::List(
                child,
                (0..n)
                    .map(|_| payload(input, child, depth + 1))
                    .collect::<io::Result<_>>()?,
            )
        }
        10 => {
            let mut fields = BTreeMap::new();
            loop {
                let child = byte(input)?;
                if child == 0 {
                    break;
                }
                let name = read_string(input)?;
                let value = payload(input, child, depth + 1)?;
                if fields.insert(name, value).is_some() {
                    return Err(invalid("duplicate Java NBT compound key"));
                }
            }
            Tag::Compound(fields)
        }
        11 => {
            let n = count(input)?;
            Tag::IntArray(
                (0..n)
                    .map(|_| fixed(input).map(i32::from_be_bytes))
                    .collect::<io::Result<_>>()?,
            )
        }
        12 => {
            let n = count(input)?;
            Tag::LongArray(
                (0..n)
                    .map(|_| fixed(input).map(i64::from_be_bytes))
                    .collect::<io::Result<_>>()?,
            )
        }
        _ => return Err(invalid(format!("invalid Java NBT tag {kind}"))),
    })
}

/// Raw template actor plus only fields changed/removed by native load/save.
/// This retains the source for deferred Bedrock adaptation without a second
/// complete native payload, including source fields Java intentionally discards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TemplateActor {
    pub id: &'static str,
    pub source: &'static [u8],
    pub normalization: &'static [u8],
}

pub fn apply_normalization(source: &mut Tag, patch: &[u8]) -> io::Result<()> {
    let Tag::Compound(mut patch) = Tag::read_root(patch)? else {
        return Err(invalid("native actor normalization must be a compound"));
    };
    let target = source.compound_mut()?;
    let Some(Tag::List(8, removed)) = patch.remove(&key("remove")) else {
        return Err(invalid("missing native actor removal list"));
    };
    for name in removed {
        let Tag::String(name) = name else {
            return Err(invalid("invalid native actor removal key"));
        };
        target.remove(&name);
    }
    let Some(Tag::Compound(changed)) = patch.remove(&key("set")) else {
        return Err(invalid("missing native actor changed fields"));
    };
    target.extend(changed);
    Ok(())
}

// Captured constructor/setEntityId defaults, not Java template assets.
include!("../../java_data/native_actor_defaults.rs");
// Captured LevelChunk FULL promotions of pending actors, keyed by block id.
include!("../../java_data/native_promoted_actor_defaults.rs");

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_bits_typed_empty_lists_and_java_code_units_roundtrip() {
        let tag = Tag::Compound(BTreeMap::from([
            (key("float"), Tag::Float(0x7fc00001)),
            (key("double"), Tag::Double(0x8000000000000000)),
            (key("list"), Tag::List(10, vec![])),
            (key("java"), Tag::String(vec![0, 0xd800, 0xd83d, 0xde00])),
            (key("seed"), Tag::Long(i64::MIN)),
        ]));
        assert_eq!(Tag::read_root(&tag.write_root().unwrap()).unwrap(), tag);
    }

    /// Two independent native observations, direct `BlockEntityType.create`
    /// and FULL pending promotion, must describe the same constructor save.
    #[test]
    fn promoted_defaults_agree_with_direct_constructor_saves() {
        let normalized = |bytes| {
            let mut tag = Tag::read_root(bytes).unwrap();
            for name in ["x", "y", "z", "keepPacked"] {
                tag.compound_mut().unwrap().remove(&key(name));
            }
            tag
        };
        for block in [
            "chest",
            "trapped_chest",
            "barrel",
            "dispenser",
            "dropper",
            "spawner",
            "trial_spawner",
            "suspicious_sand",
            "suspicious_gravel",
            "decorated_pot",
        ] {
            let promoted = promoted_default(&format!("minecraft:{block}")).unwrap();
            assert_eq!(
                normalized(promoted),
                normalized(runtime_default(block).unwrap()),
                "{block}"
            );
        }
        assert!(promoted_default("minecraft:moving_piston").is_none());
    }
}
