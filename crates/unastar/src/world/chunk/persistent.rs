//! Palette entries of persistent block storages, the form BDS saves sub-chunks in.
//!
//! Each entry is a rooted little-endian NBT compound
//! `{name: string, states: {property: byte | int | string, ...}, version: int}`
//! (BDS 1.26.51 writes exactly these three fields, in that order). The reader
//! is flat and bounded: it accepts only that shape and resolves the entry to a
//! canonical runtime state through the block registry, so an entry it cannot
//! read or the registry does not know is an error, never a substitute block.

use crate::registry::block::{BlockRegistry, SavedStateValue};

const TAG_END: u8 = 0;
const TAG_BYTE: u8 = 1;
const TAG_INT: u8 = 3;
const TAG_STRING: u8 = 8;
const TAG_COMPOUND: u8 = 10;

/// Read the palette entry at `*offset`, advance past it and return its
/// canonical runtime state.
pub(super) fn read_palette_entry(data: &[u8], offset: &mut usize) -> Result<u32, String> {
    let mut reader = Reader {
        data,
        offset: *offset,
    };
    let (name, states) = reader.entry()?;
    *offset = reader.offset;
    BlockRegistry::vanilla()
        .state_by_properties(name, &states)
        .map_err(|error| error.to_string())
}

type States<'a> = Vec<(&'a str, SavedStateValue<'a>)>;

struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, len: usize) -> Result<&'a [u8], String> {
        let bytes = self
            .data
            .get(self.offset..self.offset + len)
            .ok_or("palette entry is truncated")?;
        self.offset += len;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.bytes(1)?[0])
    }

    fn i32(&mut self) -> Result<i32, String> {
        let bytes = self.bytes(4)?;
        Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// NBT string: u16 LE byte length, then UTF-8.
    fn string(&mut self) -> Result<&'a str, String> {
        let len = self.bytes(2)?;
        let len = u16::from_le_bytes([len[0], len[1]]);
        std::str::from_utf8(self.bytes(usize::from(len))?)
            .map_err(|_| "palette entry string is not UTF-8".to_owned())
    }

    /// The root compound: `name` and `states` required, `version` optional
    /// (it selects upgrades of older saves, which are not supported).
    fn entry(&mut self) -> Result<(&'a str, States<'a>), String> {
        let tag = self.u8()?;
        if tag != TAG_COMPOUND {
            return Err(format!("palette entry is NBT tag {tag}, not a compound"));
        }
        self.string()?; // Root name, empty in BDS saves.
        let (mut name, mut states, mut version) = (None, None, None);
        loop {
            let tag = self.u8()?;
            if tag == TAG_END {
                break;
            }
            let key = self.string()?;
            match (key, tag) {
                ("name", TAG_STRING) if name.is_none() => name = Some(self.string()?),
                ("states", TAG_COMPOUND) if states.is_none() => states = Some(self.states()?),
                ("version", TAG_INT) if version.is_none() => version = Some(self.i32()?),
                _ => {
                    return Err(format!(
                        "palette entry has an unexpected field `{key}` (NBT tag {tag})"
                    ));
                }
            }
        }
        match (name, states) {
            (Some(name), Some(states)) => Ok((name, states)),
            _ => Err("palette entry lacks `name` or `states`".to_owned()),
        }
    }

    /// The `states` compound: block properties are bytes, ints or strings.
    fn states(&mut self) -> Result<States<'a>, String> {
        let mut states = Vec::new();
        loop {
            let tag = self.u8()?;
            if tag == TAG_END {
                return Ok(states);
            }
            let key = self.string()?;
            let value = match tag {
                TAG_BYTE => SavedStateValue::Byte(self.u8()?),
                TAG_INT => SavedStateValue::Int(self.i32()?),
                TAG_STRING => SavedStateValue::Str(self.string()?),
                _ => return Err(format!("block state `{key}` is NBT tag {tag}")),
            };
            states.push((key, value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string(out: &mut Vec<u8>, text: &str) {
        out.extend_from_slice(&(text.len() as u16).to_le_bytes());
        out.extend_from_slice(text.as_bytes());
    }

    /// `{name, states, version}` as BDS 1.26.51 writes it.
    fn entry(name: &str, states: &[(&str, SavedStateValue<'_>)]) -> Vec<u8> {
        let mut out = vec![TAG_COMPOUND];
        string(&mut out, "");
        out.push(TAG_STRING);
        string(&mut out, "name");
        string(&mut out, name);
        out.push(TAG_COMPOUND);
        string(&mut out, "states");
        for (key, value) in states {
            match value {
                SavedStateValue::Byte(byte) => {
                    out.push(TAG_BYTE);
                    string(&mut out, key);
                    out.push(*byte);
                }
                SavedStateValue::Int(int) => {
                    out.push(TAG_INT);
                    string(&mut out, key);
                    out.extend_from_slice(&int.to_le_bytes());
                }
                SavedStateValue::Str(text) => {
                    out.push(TAG_STRING);
                    string(&mut out, key);
                    string(&mut out, text);
                }
            }
        }
        out.push(TAG_END);
        out.push(TAG_INT);
        string(&mut out, "version");
        out.extend_from_slice(&18_168_865i32.to_le_bytes());
        out.push(TAG_END);
        out
    }

    fn read(bytes: &[u8]) -> Result<(u32, usize), String> {
        let mut offset = 0;
        read_palette_entry(bytes, &mut offset).map(|state| (state, offset))
    }

    fn state(name: &str, property: &str, value: unastar_data::blocks::StateValue) -> u32 {
        let block = BlockRegistry::vanilla().get_by_name(name).unwrap();
        (block.min_state_id..=block.max_state_id)
            .find(|&id| block.state_value(id, property) == Some(value))
            .unwrap()
    }

    #[test]
    fn entries_resolve_by_name_and_typed_states() {
        use unastar_data::blocks::StateValue;
        let log = entry(
            "minecraft:oak_log",
            &[("pillar_axis", SavedStateValue::Str("x"))],
        );
        assert_eq!(
            read(&log),
            Ok((
                state("minecraft:oak_log", "pillar_axis", StateValue::Str("x")),
                log.len()
            ))
        );
        let bedrock = entry(
            "minecraft:bedrock",
            &[("infiniburn_bit", SavedStateValue::Byte(1))],
        );
        assert_eq!(
            read(&bedrock).unwrap().0,
            state("minecraft:bedrock", "infiniburn_bit", StateValue::Byte(1))
        );
        let water = entry(
            "minecraft:water",
            &[("liquid_depth", SavedStateValue::Int(0))],
        );
        assert_eq!(read(&water).unwrap().0, *crate::world::chunk::blocks::WATER);
        let air = entry("minecraft:air", &[]);
        assert_eq!(read(&air).unwrap().0, *crate::world::chunk::blocks::AIR);
    }

    #[test]
    fn unknown_or_mistyped_entries_are_errors_not_substitutes() {
        let unknown = entry("minecraft:not_a_block", &[]);
        assert_eq!(
            read(&unknown),
            Err("unknown block `minecraft:not_a_block`".to_owned())
        );
        let bad_value = entry(
            "minecraft:oak_log",
            &[("pillar_axis", SavedStateValue::Str("w"))],
        );
        assert_eq!(
            read(&bad_value),
            Err("block `minecraft:oak_log` has no state {pillar_axis=\"w\"}".to_owned())
        );
        // The same value with the wrong NBT type.
        let int_bit = entry(
            "minecraft:bedrock",
            &[("infiniburn_bit", SavedStateValue::Int(0))],
        );
        assert!(read(&int_bit).unwrap_err().contains("has no state"));
        let missing = entry("minecraft:bedrock", &[]);
        assert!(read(&missing).unwrap_err().contains("has no state {}"));
        let extra = entry("minecraft:dirt", &[("foo", SavedStateValue::Byte(1))]);
        assert!(read(&extra).unwrap_err().contains("has no state"));
        let repeated = entry(
            "minecraft:bedrock",
            &[
                ("infiniburn_bit", SavedStateValue::Byte(0)),
                ("infiniburn_bit", SavedStateValue::Byte(0)),
            ],
        );
        assert!(read(&repeated).unwrap_err().contains("has no state"));
    }

    #[test]
    fn entries_of_another_shape_are_errors() {
        let log = entry(
            "minecraft:oak_log",
            &[("pillar_axis", SavedStateValue::Str("x"))],
        );
        for cut in 0..log.len() {
            assert_eq!(
                read(&log[..cut]),
                Err("palette entry is truncated".to_owned()),
                "{cut}"
            );
        }
        assert_eq!(
            read(&[TAG_STRING, 0, 0, 0, 0]),
            Err("palette entry is NBT tag 8, not a compound".to_owned())
        );
        // A legacy `val` field (pre-flattening saves) is not read.
        let mut legacy = vec![TAG_COMPOUND, 0, 0, TAG_STRING];
        string(&mut legacy, "name");
        string(&mut legacy, "minecraft:stone");
        legacy.push(2);
        string(&mut legacy, "val");
        legacy.extend_from_slice(&0i16.to_le_bytes());
        legacy.push(TAG_END);
        assert_eq!(
            read(&legacy),
            Err("palette entry has an unexpected field `val` (NBT tag 2)".to_owned())
        );
        let mut no_states = vec![TAG_COMPOUND, 0, 0, TAG_STRING];
        string(&mut no_states, "name");
        string(&mut no_states, "minecraft:stone");
        no_states.push(TAG_END);
        assert_eq!(
            read(&no_states),
            Err("palette entry lacks `name` or `states`".to_owned())
        );
        let mut float_state = vec![TAG_COMPOUND, 0, 0, TAG_COMPOUND];
        string(&mut float_state, "states");
        float_state.push(5);
        string(&mut float_state, "height");
        float_state.extend_from_slice(&1f32.to_le_bytes());
        assert_eq!(
            read(&float_state),
            Err("block state `height` is NBT tag 5".to_owned())
        );
    }
}
