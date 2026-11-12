//! Writing an item's NBT compound in the order BDS writes it.
//!
//! `zuri_nbt` keeps a compound in a `HashMap` and writes it in the map's
//! (random) order. BDS 1.26.51 writes compound keys sorted: a page of a book
//! is `photoname` then `text`, a signed book `author`, `generation`, `pages`,
//! `title`, `xuid` (2026-11-12 `book` probe,
//! `crates/bds-extractor/captures/1.26.51/protocol/book/`). Item user data is
//! little-endian NBT with `u16` string lengths and `i32` list lengths; an
//! empty list has element type `End` (0), as BDS writes it.

use zuri_nbt::{NBTTag, tag};

/// The NBT type id of a tag (NBT format numbers).
fn tag_id(tag: &NBTTag) -> u8 {
    match tag {
        NBTTag::Byte(_) => 1,
        NBTTag::Short(_) => 2,
        NBTTag::Int(_) => 3,
        NBTTag::Long(_) => 4,
        NBTTag::Float(_) => 5,
        NBTTag::Double(_) => 6,
        NBTTag::ByteArray(_) => 7,
        NBTTag::String(_) => 8,
        NBTTag::List(_) => 9,
        NBTTag::Compound(_) => 10,
        NBTTag::IntArray(_) => 11,
        NBTTag::LongArray(_) => 12,
    }
}

/// Why a compound could not be written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NbtWriteError {
    #[error("NBT string of {0} bytes is longer than {max}", max = u16::MAX)]
    StringTooLong(usize),
    #[error("NBT list or array of {0} entries is longer than {max}", max = i32::MAX)]
    TooManyEntries(usize),
    #[error("NBT list mixes tag types {first} and {other}")]
    MixedList { first: u8, other: u8 },
}

fn string(out: &mut Vec<u8>, value: &str) -> Result<(), NbtWriteError> {
    let len = u16::try_from(value.len()).map_err(|_| NbtWriteError::StringTooLong(value.len()))?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn length(out: &mut Vec<u8>, len: usize) -> Result<(), NbtWriteError> {
    let len = i32::try_from(len).map_err(|_| NbtWriteError::TooManyEntries(len))?;
    out.extend_from_slice(&len.to_le_bytes());
    Ok(())
}

fn payload(out: &mut Vec<u8>, value: &NBTTag) -> Result<(), NbtWriteError> {
    match value {
        NBTTag::Byte(v) => out.push(v.0),
        NBTTag::Short(v) => out.extend_from_slice(&v.0.to_le_bytes()),
        NBTTag::Int(v) => out.extend_from_slice(&v.0.to_le_bytes()),
        NBTTag::Long(v) => out.extend_from_slice(&v.0.to_le_bytes()),
        NBTTag::Float(v) => out.extend_from_slice(&v.0.to_le_bytes()),
        NBTTag::Double(v) => out.extend_from_slice(&v.0.to_le_bytes()),
        NBTTag::ByteArray(v) => {
            length(out, v.0.len())?;
            out.extend_from_slice(&v.0);
        }
        NBTTag::String(v) => string(out, &v.0)?,
        NBTTag::List(list) => {
            // BDS types an empty list as `End` (0): an opened, unwritten book
            // carries `pages` as an empty list of type 0 (zuri_nbt writes 1).
            let first = list.0.first().map_or(0, tag_id);
            out.push(first);
            length(out, list.0.len())?;
            for entry in &list.0 {
                let other = tag_id(entry);
                if other != first {
                    return Err(NbtWriteError::MixedList { first, other });
                }
                payload(out, entry)?;
            }
        }
        NBTTag::Compound(compound) => compound_payload(out, compound)?,
        NBTTag::IntArray(v) => {
            length(out, v.0.len())?;
            for entry in &v.0 {
                out.extend_from_slice(&entry.to_le_bytes());
            }
        }
        NBTTag::LongArray(v) => {
            length(out, v.0.len())?;
            for entry in &v.0 {
                out.extend_from_slice(&entry.to_le_bytes());
            }
        }
    }
    Ok(())
}

fn compound_payload(out: &mut Vec<u8>, compound: &tag::Compound) -> Result<(), NbtWriteError> {
    let mut entries: Vec<_> = compound.0.iter().collect();
    entries.sort_unstable_by_key(|(name, _)| *name);
    for (name, value) in entries {
        out.push(tag_id(value));
        string(out, name)?;
        payload(out, value)?;
    }
    out.push(0);
    Ok(())
}

/// Writes `compound` as an unnamed root compound, keys sorted at every level.
pub fn write_sorted(out: &mut Vec<u8>, compound: &tag::Compound) -> Result<(), NbtWriteError> {
    out.push(10);
    string(out, "")?;
    compound_payload(out, compound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use zuri_nbt::encoding::LittleEndian;

    fn compound(entries: Vec<(&str, NBTTag)>) -> tag::Compound {
        tag::Compound(
            entries
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect::<HashMap<_, _>>(),
        )
    }

    /// Sorted output reads back to the same compound with zuri_nbt.
    #[test]
    fn sorted_compounds_round_trip_through_zuri_nbt() {
        let page = compound(vec![
            ("text", NBTTag::String(tag::String("hello".into()))),
            ("photoname", NBTTag::String(tag::String(String::new()))),
        ]);
        let root = compound(vec![
            ("xuid", NBTTag::String(tag::String(String::new()))),
            ("generation", NBTTag::Int(tag::Int(0))),
            (
                "pages",
                NBTTag::List(tag::List(vec![NBTTag::Compound(page)])),
            ),
            ("Damage", NBTTag::Short(tag::Short(-3))),
            ("ids", NBTTag::LongArray(tag::LongArray(vec![1, -2]))),
        ]);
        let mut out = Vec::new();
        write_sorted(&mut out, &root).unwrap();
        let read = NBTTag::read(&mut std::io::Cursor::new(&out), LittleEndian).unwrap();
        assert_eq!(read, NBTTag::Compound(root));
        // Keys in byte order: `Damage` (capital) before the lower-case ones.
        let names: Vec<usize> = ["Damage", "generation", "ids", "pages", "xuid"]
            .iter()
            .map(|name| {
                out.windows(name.len())
                    .position(|window| window == name.as_bytes())
                    .unwrap()
            })
            .collect();
        assert!(names.windows(2).all(|pair| pair[0] < pair[1]), "{names:?}");
    }

    /// An empty list is typed `End`, as in the BDS book fixtures, and still
    /// reads back.
    #[test]
    fn empty_lists_are_typed_end() {
        let root = compound(vec![("pages", NBTTag::List(tag::List(Vec::new())))]);
        let mut out = Vec::new();
        write_sorted(&mut out, &root).unwrap();
        assert_eq!(
            out,
            [
                10, 0, 0, 9, 5, 0, b'p', b'a', b'g', b'e', b's', 0, 0, 0, 0, 0, 0
            ]
        );
        let read = NBTTag::read(&mut std::io::Cursor::new(&out), LittleEndian).unwrap();
        assert_eq!(read, NBTTag::Compound(root));
    }
}
