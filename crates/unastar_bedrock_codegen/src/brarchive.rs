//! Reader for BDS `.brarchive` containers.
//!
//! BDS packs ship most JSON families (`biomes`, `features`, `feature_rules`,
//! `spawn_rules`, ...) bundled in `__brarchive/<family>.brarchive` files.
//! Layout (little endian), as found in every BDS 1.26.51 vanilla pack:
//!
//! | offset            | size  | field                                        |
//! |-------------------|-------|----------------------------------------------|
//! | 0                 | 8     | magic `7d 27 25 b1 a0 52 70 26`              |
//! | 8                 | 4     | entry count `n`                              |
//! | 12                | 4     | format version (`1`)                         |
//! | 16 + i*256        | 256   | entry: u8 name length, 247-byte name, u32 data offset, u32 data size |
//! | 16 + n*256        | ...   | data blob; entry offsets are relative to it  |
//!
//! Entry payloads are plain JSON that may contain `//` comments. Some archives
//! (`structures/*.brarchive`) list names with empty payloads because the real
//! files are shipped loose next to the archive.

use crate::error::{Error, Result};
use std::path::Path;

/// Magic bytes at the start of every `.brarchive`.
pub const MAGIC: [u8; 8] = [0x7d, 0x27, 0x25, 0xb1, 0xa0, 0x52, 0x70, 0x26];
/// The only container version BDS 1.26.51 ships.
pub const VERSION: u32 = 1;
const HEADER_LEN: usize = 16;
const ENTRY_LEN: usize = 256;
const NAME_CAPACITY: usize = 247;

/// One named payload of an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// File name of the entry, e.g. `coal_ore_feature.json`.
    pub name: String,
    /// Raw payload bytes.
    pub data: Vec<u8>,
}

/// Read and decode a `.brarchive` file.
pub fn read_file(path: &Path) -> Result<Vec<Entry>> {
    let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
    parse(&bytes).map_err(|reason| Error::Brarchive {
        path: path.to_path_buf(),
        reason,
    })
}

/// Decode an in-memory `.brarchive`.
///
/// Fails on a bad magic/version, truncated header or entry table, names that
/// overflow their slot or are not UTF-8, and payloads outside the data blob.
pub fn parse(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    if bytes.len() < HEADER_LEN {
        return Err(format!("{} bytes is shorter than the header", bytes.len()));
    }
    if bytes[..8] != MAGIC {
        return Err(format!("bad magic {:02x?}", &bytes[..8]));
    }
    let count = u32_at(bytes, 8) as usize;
    let version = u32_at(bytes, 12);
    if version != VERSION {
        return Err(format!("unsupported version {version}"));
    }
    let data_start = count
        .checked_mul(ENTRY_LEN)
        .and_then(|table| table.checked_add(HEADER_LEN))
        .filter(|&start| start <= bytes.len())
        .ok_or_else(|| format!("entry table of {count} entries exceeds the file"))?;
    let blob = &bytes[data_start..];
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let at = HEADER_LEN + index * ENTRY_LEN;
        let name_len = bytes[at] as usize;
        if name_len > NAME_CAPACITY {
            return Err(format!("entry {index}: name length {name_len} overflows its slot"));
        }
        let name = std::str::from_utf8(&bytes[at + 1..at + 1 + name_len])
            .map_err(|e| format!("entry {index}: name is not UTF-8: {e}"))?
            .to_owned();
        let offset = u32_at(bytes, at + 1 + NAME_CAPACITY) as usize;
        let size = u32_at(bytes, at + 5 + NAME_CAPACITY) as usize;
        let data = offset
            .checked_add(size)
            .and_then(|end| blob.get(offset..end))
            .ok_or_else(|| format!("entry {index} ({name}): payload {offset}+{size} outside data"))?
            .to_vec();
        entries.push(Entry { name, data });
    }
    Ok(entries)
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4-byte slice"))
}

/// Encode entries into the `.brarchive` layout (used by tests and fixtures).
pub fn encode(entries: &[Entry]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    out.extend_from_slice(&VERSION.to_le_bytes());
    let mut offset = 0u32;
    for entry in entries {
        assert!(entry.name.len() <= NAME_CAPACITY, "entry name too long");
        let mut slot = [0u8; ENTRY_LEN];
        slot[0] = entry.name.len() as u8;
        slot[1..1 + entry.name.len()].copy_from_slice(entry.name.as_bytes());
        slot[1 + NAME_CAPACITY..5 + NAME_CAPACITY].copy_from_slice(&offset.to_le_bytes());
        slot[5 + NAME_CAPACITY..9 + NAME_CAPACITY]
            .copy_from_slice(&(entry.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&slot);
        offset += entry.data.len() as u32;
    }
    for entry in entries {
        out.extend_from_slice(&entry.data);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, data: &str) -> Entry {
        Entry {
            name: name.to_owned(),
            data: data.as_bytes().to_vec(),
        }
    }

    #[test]
    fn roundtrips_entries_in_order() {
        let entries = vec![
            entry("a.json", "{\"a\": 1}"),
            entry("empty.nbt", ""),
            entry("b.json", "// comment\n{}"),
        ];
        assert_eq!(parse(&encode(&entries)).unwrap(), entries);
    }

    #[test]
    fn data_offsets_are_relative_to_the_blob() {
        let bytes = encode(&[entry("x", "XY"), entry("y", "Z")]);
        assert_eq!(bytes.len(), HEADER_LEN + 2 * ENTRY_LEN + 3);
        assert_eq!(u32_at(&bytes, HEADER_LEN + ENTRY_LEN + 248), 2);
        assert_eq!(parse(&bytes).unwrap()[1].data, b"Z");
    }

    #[test]
    fn rejects_bad_magic_version_and_truncation() {
        let mut bytes = encode(&[entry("x", "1")]);
        assert!(parse(&bytes[..10]).unwrap_err().contains("shorter"));
        assert!(parse(&bytes[..HEADER_LEN + 10]).unwrap_err().contains("exceeds"));
        bytes[12] = 2;
        assert!(parse(&bytes).unwrap_err().contains("version 2"));
        bytes[12] = 1;
        bytes[0] = 0;
        assert!(parse(&bytes).unwrap_err().contains("magic"));
    }

    #[test]
    fn rejects_payload_outside_blob() {
        let mut bytes = encode(&[entry("x", "1")]);
        bytes[HEADER_LEN + 252] = 9;
        assert!(parse(&bytes).unwrap_err().contains("outside data"));
    }
}
