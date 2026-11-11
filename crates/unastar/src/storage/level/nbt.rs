//! Little-endian NBT as BDS saves level data: `level.dat` and the world
//! database's level records.
//!
//! BDS 1.26.51 writes every compound with its keys in byte order (measured on
//! `level.dat`, `scoreboard`, `tickingarea_*`, `structuretemplate_*` and
//! player records), so [`Compound`] is a sorted map and a tag read from BDS
//! encodes back to the same bytes. Reading is bounded: nesting depth, list
//! lengths and strings are checked against the bytes that remain, so a corrupt
//! record is an error, never a large allocation or a panic.

use std::collections::BTreeMap;

/// Deepest nesting a record may use (BDS's own records nest a few levels).
pub const MAX_DEPTH: usize = 512;

/// A compound: tags by name, in byte order of the names.
pub type Compound = BTreeMap<String, Tag>;

/// One NBT tag.
#[derive(Debug, Clone, PartialEq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    /// A list and its element type id (kept for empty lists, which BDS
    /// writes with element type 0).
    List(u8, Vec<Tag>),
    Compound(Compound),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

const END: u8 = 0;
const BYTE: u8 = 1;
const SHORT: u8 = 2;
const INT: u8 = 3;
const LONG: u8 = 4;
const FLOAT: u8 = 5;
const DOUBLE: u8 = 6;
const BYTE_ARRAY: u8 = 7;
const STRING: u8 = 8;
const LIST: u8 = 9;
const COMPOUND: u8 = 10;
const INT_ARRAY: u8 = 11;
const LONG_ARRAY: u8 = 12;

impl Tag {
    /// The tag's type id.
    pub fn id(&self) -> u8 {
        match self {
            Self::Byte(_) => BYTE,
            Self::Short(_) => SHORT,
            Self::Int(_) => INT,
            Self::Long(_) => LONG,
            Self::Float(_) => FLOAT,
            Self::Double(_) => DOUBLE,
            Self::ByteArray(_) => BYTE_ARRAY,
            Self::String(_) => STRING,
            Self::List(..) => LIST,
            Self::Compound(_) => COMPOUND,
            Self::IntArray(_) => INT_ARRAY,
            Self::LongArray(_) => LONG_ARRAY,
        }
    }

    /// A list of `items`, all of tag type `element` (0 when empty).
    pub fn list(items: Vec<Tag>) -> Self {
        let element = items.first().map_or(END, Tag::id);
        Self::List(element, items)
    }

    pub fn as_compound(&self) -> Option<&Compound> {
        match self {
            Self::Compound(compound) => Some(compound),
            _ => None,
        }
    }
}

/// Why bytes are not the NBT a record needs.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NbtError {
    #[error("NBT ends early at byte {0}")]
    Truncated(usize),
    #[error("unknown NBT tag type {tag} at byte {at}")]
    UnknownTag { tag: u8, at: usize },
    #[error("NBT nests deeper than {MAX_DEPTH} levels")]
    TooDeep,
    #[error("negative NBT length {0}")]
    NegativeLength(i32),
    #[error("NBT string is not UTF-8")]
    BadString,
    #[error("NBT compound repeats the name {0:?}")]
    DuplicateName(String),
    #[error("list of tag type {element} holds a tag of type {found}")]
    MixedList { element: u8, found: u8 },
    #[error("root tag is type {0}, not a compound")]
    RootNotCompound(u8),
    #[error("{0} bytes follow the root compound")]
    TrailingBytes(usize),
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], NbtError> {
        let end = self
            .at
            .checked_add(n)
            .filter(|end| *end <= self.data.len())
            .ok_or(NbtError::Truncated(self.at))?;
        let bytes = &self.data[self.at..end];
        self.at = end;
        Ok(bytes)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], NbtError> {
        Ok(self.take(N)?.try_into().expect("N bytes"))
    }

    fn u8(&mut self) -> Result<u8, NbtError> {
        Ok(self.take(1)?[0])
    }

    /// A length prefix, checked against the bytes left for `item` bytes each.
    fn length(&mut self, item: usize) -> Result<usize, NbtError> {
        let n = i32::from_le_bytes(self.array()?);
        let n = usize::try_from(n).map_err(|_| NbtError::NegativeLength(n))?;
        if n.saturating_mul(item.max(1)) > self.data.len() - self.at {
            return Err(NbtError::Truncated(self.at));
        }
        Ok(n)
    }

    fn string(&mut self) -> Result<String, NbtError> {
        let n = u16::from_le_bytes(self.array()?) as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| NbtError::BadString)
    }

    fn payload(&mut self, tag: u8, depth: usize) -> Result<Tag, NbtError> {
        if depth > MAX_DEPTH {
            return Err(NbtError::TooDeep);
        }
        Ok(match tag {
            BYTE => Tag::Byte(self.u8()? as i8),
            SHORT => Tag::Short(i16::from_le_bytes(self.array()?)),
            INT => Tag::Int(i32::from_le_bytes(self.array()?)),
            LONG => Tag::Long(i64::from_le_bytes(self.array()?)),
            FLOAT => Tag::Float(f32::from_le_bytes(self.array()?)),
            DOUBLE => Tag::Double(f64::from_le_bytes(self.array()?)),
            BYTE_ARRAY => {
                let n = self.length(1)?;
                Tag::ByteArray(self.take(n)?.iter().map(|b| *b as i8).collect())
            }
            STRING => Tag::String(self.string()?),
            LIST => {
                let element = self.u8()?;
                // Elements of an empty list type take at least one byte each
                // except End, which no element may have.
                let n = self.length(1)?;
                if n > 0 && element == END {
                    return Err(NbtError::UnknownTag {
                        tag: END,
                        at: self.at,
                    });
                }
                let mut items = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    items.push(self.payload(element, depth + 1)?);
                }
                Tag::List(element, items)
            }
            COMPOUND => {
                let mut compound = Compound::new();
                loop {
                    let child = self.u8()?;
                    if child == END {
                        break;
                    }
                    let name = self.string()?;
                    let value = self.payload(child, depth + 1)?;
                    if compound.contains_key(&name) {
                        return Err(NbtError::DuplicateName(name));
                    }
                    compound.insert(name, value);
                }
                Tag::Compound(compound)
            }
            INT_ARRAY => {
                let n = self.length(4)?;
                Tag::IntArray(
                    self.take(n * 4)?
                        .chunks_exact(4)
                        .map(|b| i32::from_le_bytes(b.try_into().expect("4 bytes")))
                        .collect(),
                )
            }
            LONG_ARRAY => {
                let n = self.length(8)?;
                Tag::LongArray(
                    self.take(n * 8)?
                        .chunks_exact(8)
                        .map(|b| i64::from_le_bytes(b.try_into().expect("8 bytes")))
                        .collect(),
                )
            }
            tag => {
                return Err(NbtError::UnknownTag {
                    tag,
                    at: self.at - 1,
                });
            }
        })
    }
}

/// Reads one rooted compound (type, name, payload) at the start of `data`;
/// returns its name, the compound and the bytes it used.
pub fn read_root(data: &[u8]) -> Result<(String, Compound, usize), NbtError> {
    let mut reader = Reader { data, at: 0 };
    let tag = reader.u8()?;
    if tag != COMPOUND {
        return Err(NbtError::RootNotCompound(tag));
    }
    let name = reader.string()?;
    let Tag::Compound(compound) = reader.payload(COMPOUND, 0)? else {
        unreachable!("a compound payload reads as a compound")
    };
    Ok((name, compound, reader.at))
}

/// Reads a record that is exactly one rooted compound.
pub fn read_record(data: &[u8]) -> Result<Compound, NbtError> {
    let (_, compound, used) = read_root(data)?;
    if used != data.len() {
        return Err(NbtError::TrailingBytes(data.len() - used));
    }
    Ok(compound)
}

fn write_string(out: &mut Vec<u8>, text: &str) {
    let bytes = text.as_bytes();
    let len = u16::try_from(bytes.len()).expect("NBT strings are at most 65535 bytes");
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(bytes);
}

fn write_payload(out: &mut Vec<u8>, tag: &Tag) {
    let len = |n: usize| {
        i32::try_from(n)
            .expect("NBT lengths fit an i32")
            .to_le_bytes()
    };
    match tag {
        Tag::Byte(v) => out.push(*v as u8),
        Tag::Short(v) => out.extend_from_slice(&v.to_le_bytes()),
        Tag::Int(v) => out.extend_from_slice(&v.to_le_bytes()),
        Tag::Long(v) => out.extend_from_slice(&v.to_le_bytes()),
        Tag::Float(v) => out.extend_from_slice(&v.to_le_bytes()),
        Tag::Double(v) => out.extend_from_slice(&v.to_le_bytes()),
        Tag::ByteArray(v) => {
            out.extend_from_slice(&len(v.len()));
            out.extend(v.iter().map(|b| *b as u8));
        }
        Tag::String(v) => write_string(out, v),
        Tag::List(element, items) => {
            out.push(*element);
            out.extend_from_slice(&len(items.len()));
            for item in items {
                debug_assert_eq!(item.id(), *element, "a list holds one tag type");
                write_payload(out, item);
            }
        }
        Tag::Compound(compound) => write_compound(out, compound),
        Tag::IntArray(v) => {
            out.extend_from_slice(&len(v.len()));
            for value in v {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        Tag::LongArray(v) => {
            out.extend_from_slice(&len(v.len()));
            for value in v {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
}

fn write_compound(out: &mut Vec<u8>, compound: &Compound) {
    for (name, tag) in compound {
        out.push(tag.id());
        write_string(out, name);
        write_payload(out, tag);
    }
    out.push(END);
}

/// One rooted compound with an empty name, as BDS writes level data.
pub fn write_record(compound: &Compound) -> Vec<u8> {
    let mut out = vec![COMPOUND];
    write_string(&mut out, "");
    write_compound(&mut out, compound);
    out
}

/// Typed access to a compound's fields, for decoders that need a field of
/// one type and fail on anything else.
pub trait CompoundExt {
    fn byte(&self, name: &str) -> Result<i8, FieldError>;
    fn int(&self, name: &str) -> Result<i32, FieldError>;
    fn long(&self, name: &str) -> Result<i64, FieldError>;
    fn float(&self, name: &str) -> Result<f32, FieldError>;
    fn string(&self, name: &str) -> Result<&str, FieldError>;
    fn compound(&self, name: &str) -> Result<&Compound, FieldError>;
    /// A list's items (any element type; an empty list of any type).
    fn list(&self, name: &str) -> Result<&[Tag], FieldError>;
    /// `Ok(None)` when the field is absent; an error when it has another type.
    fn opt_byte(&self, name: &str) -> Result<Option<i8>, FieldError>;
}

/// A field a decoder needs is missing or has another type.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("field {name:?} is {problem}")]
pub struct FieldError {
    pub name: String,
    pub problem: &'static str,
}

fn field<'a, T>(
    compound: &'a Compound,
    name: &str,
    kind: &'static str,
    pick: impl FnOnce(&'a Tag) -> Option<T>,
) -> Result<T, FieldError> {
    match compound.get(name) {
        None => Err(FieldError {
            name: name.to_owned(),
            problem: "missing",
        }),
        Some(tag) => pick(tag).ok_or(FieldError {
            name: name.to_owned(),
            problem: kind,
        }),
    }
}

impl CompoundExt for Compound {
    fn byte(&self, name: &str) -> Result<i8, FieldError> {
        field(self, name, "not a Byte", |t| match t {
            Tag::Byte(v) => Some(*v),
            _ => None,
        })
    }

    fn int(&self, name: &str) -> Result<i32, FieldError> {
        field(self, name, "not an Int", |t| match t {
            Tag::Int(v) => Some(*v),
            _ => None,
        })
    }

    fn long(&self, name: &str) -> Result<i64, FieldError> {
        field(self, name, "not a Long", |t| match t {
            Tag::Long(v) => Some(*v),
            _ => None,
        })
    }

    fn float(&self, name: &str) -> Result<f32, FieldError> {
        field(self, name, "not a Float", |t| match t {
            Tag::Float(v) => Some(*v),
            _ => None,
        })
    }

    fn string(&self, name: &str) -> Result<&str, FieldError> {
        field(self, name, "not a String", |t| match t {
            Tag::String(v) => Some(v.as_str()),
            _ => None,
        })
    }

    fn compound(&self, name: &str) -> Result<&Compound, FieldError> {
        field(self, name, "not a Compound", Tag::as_compound)
    }

    fn list(&self, name: &str) -> Result<&[Tag], FieldError> {
        field(self, name, "not a List", |t| match t {
            Tag::List(_, items) => Some(items.as_slice()),
            _ => None,
        })
    }

    fn opt_byte(&self, name: &str) -> Result<Option<i8>, FieldError> {
        match self.get(name) {
            None => Ok(None),
            Some(_) => self.byte(name).map(Some),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Compound {
        let mut inner = Compound::new();
        inner.insert("b".into(), Tag::Byte(-1));
        inner.insert("Z".into(), Tag::Short(300));
        let mut root = Compound::new();
        root.insert("world_policies".into(), Tag::Compound(Compound::new()));
        root.insert("worldStartCount".into(), Tag::Long(4_294_967_294));
        root.insert("abilities".into(), Tag::Compound(inner));
        root.insert("Time".into(), Tag::Long(4321));
        root.insert("rainLevel".into(), Tag::Float(0.5));
        root.insert("Criteria".into(), Tag::list(vec![]));
        root.insert(
            "lastOpenedWithVersion".into(),
            Tag::list(vec![Tag::Int(1), Tag::Int(26)]),
        );
        root.insert("block_indices".into(), Tag::IntArray(vec![0, 1, -1]));
        root.insert("seeds".into(), Tag::LongArray(vec![i64::MIN]));
        root.insert("raw".into(), Tag::ByteArray(vec![1, -2]));
        root.insert("d".into(), Tag::Double(0.25));
        root.insert("name".into(), Tag::String("é".into()));
        root
    }

    #[test]
    fn records_round_trip_with_names_in_byte_order() {
        let bytes = write_record(&sample());
        assert_eq!(read_record(&bytes).unwrap(), sample());
        // Upper case sorts before lower case, `S` before `_` (BDS's order:
        // `worldStartCount` is written before `world_policies`).
        let text = String::from_utf8_lossy(&bytes);
        let order: Vec<usize> = [
            "Criteria",
            "Time",
            "abilities",
            "worldStartCount",
            "world_policies",
        ]
        .iter()
        .map(|name| text.find(name).expect("name written"))
        .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
    }

    #[test]
    fn empty_lists_keep_element_type_zero() {
        let bytes = write_record(&sample());
        let at = bytes
            .windows(8)
            .position(|w| w == b"Criteria")
            .expect("Criteria");
        // list tag, name, then element type 0 and length 0.
        assert_eq!(&bytes[at + 8..at + 13], &[0, 0, 0, 0, 0]);
    }

    #[test]
    fn corrupt_records_are_errors() {
        let bytes = write_record(&sample());
        for cut in 0..bytes.len() {
            assert!(read_record(&bytes[..cut]).is_err(), "cut at {cut}");
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert_eq!(read_record(&trailing), Err(NbtError::TrailingBytes(1)));
        // A list claiming more elements than bytes remain.
        let mut root = vec![COMPOUND, 0, 0, LIST, 1, 0, b'x', INT];
        root.extend_from_slice(&i32::MAX.to_le_bytes());
        root.push(END);
        assert!(matches!(read_record(&root), Err(NbtError::Truncated(_))));
        // A negative array length.
        let mut negative = vec![COMPOUND, 0, 0, INT_ARRAY, 1, 0, b'x'];
        negative.extend_from_slice(&(-1i32).to_le_bytes());
        negative.push(END);
        assert_eq!(read_record(&negative), Err(NbtError::NegativeLength(-1)));
        // A name given twice.
        let duplicate = [
            COMPOUND, 0, 0, BYTE, 1, 0, b'a', 1, BYTE, 1, 0, b'a', 2, END,
        ];
        assert_eq!(
            read_record(&duplicate),
            Err(NbtError::DuplicateName("a".into()))
        );
        // Unknown tag type and a root that is not a compound.
        assert!(matches!(
            read_record(&[COMPOUND, 0, 0, 13, 1, 0, b'a', END]),
            Err(NbtError::UnknownTag { tag: 13, .. })
        ));
        assert_eq!(
            read_record(&[INT, 0, 0, 1, 0, 0, 0]),
            Err(NbtError::RootNotCompound(INT))
        );
    }

    #[test]
    fn nesting_is_bounded() {
        let mut deep = vec![COMPOUND, 0, 0];
        for _ in 0..=MAX_DEPTH + 1 {
            deep.extend_from_slice(&[COMPOUND, 1, 0, b'a']);
        }
        deep.extend(std::iter::repeat_n(END, MAX_DEPTH + 3));
        assert_eq!(read_record(&deep), Err(NbtError::TooDeep));
    }

    #[test]
    fn typed_fields_fail_on_other_types() {
        let root = sample();
        assert_eq!(root.long("Time"), Ok(4321));
        assert_eq!(root.int("Time").unwrap_err().problem, "not an Int");
        assert_eq!(root.int("nope").unwrap_err().problem, "missing");
        assert_eq!(root.opt_byte("nope"), Ok(None));
        assert!(root.list("Criteria").unwrap().is_empty());
    }
}
