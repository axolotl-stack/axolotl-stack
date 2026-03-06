//! Canonical block states parser for pmmp/BedrockData's canonical_block_states.nbt.
//!
//! Each entry in the file is a Network Little-Endian NBT compound tag:
//! ```nbt
//! {
//!   "name": "minecraft:stone",
//!   "states": { ... },
//!   "version": 18100737
//! }
//! ```
//! The index of each entry in the file IS the block runtime ID.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;

use tracing::{debug, info};

/// A single canonical block state entry.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CanonicalState {
    /// Block name (e.g., "minecraft:stone").
    pub name: String,
    /// Block state properties, sorted by key.
    pub states: BTreeMap<String, StateValue>,
    /// Version tag value.
    pub version: i32,
    /// Runtime ID = index in the canonical list.
    pub runtime_id: u32,
    /// Raw NBT bytes of this entry (for block palette blob).
    pub raw_nbt: Vec<u8>,
}

/// A block state property value.
#[derive(Debug, Clone, PartialEq)]
pub enum StateValue {
    Byte(u8),
    Int(i32),
    String(String),
}

/// Parse canonical_block_states.nbt into a list of CanonicalState entries.
///
/// The file is a sequence of Network Little-Endian NBT compound tags.
/// Each tag's index in the sequence is its runtime ID.
pub fn parse_canonical_block_states(
    path: &Path,
) -> Result<Vec<CanonicalState>, Box<dyn std::error::Error>> {
    let data = fs::read(path)?;
    let mut cursor = Cursor::new(&data[..]);
    let mut states = Vec::new();
    let mut runtime_id = 0u32;

    while (cursor.position() as usize) < data.len() {
        let start_pos = cursor.position() as usize;

        // Each entry starts with TAG_Compound (0x0a)
        let tag_type = read_u8(&mut cursor)?;
        if tag_type != 0x0a {
            return Err(format!(
                "Expected TAG_Compound (10) at offset {}, got {}",
                start_pos, tag_type
            )
            .into());
        }

        // Root compound has an empty name (VarInt length = 0)
        let _root_name = read_string(&mut cursor)?;

        // Parse the compound's children
        let mut name = String::new();
        let mut block_states = BTreeMap::new();
        let mut version = 0i32;

        parse_state_compound(&mut cursor, &mut name, &mut block_states, &mut version)?;

        let end_pos = cursor.position() as usize;
        let raw_nbt = data[start_pos..end_pos].to_vec();

        states.push(CanonicalState {
            name,
            states: block_states,
            version,
            runtime_id,
            raw_nbt,
        });

        runtime_id += 1;
    }

    info!(
        count = states.len(),
        "Parsed canonical block states from pmmp/BedrockData"
    );

    // Log some stats
    let unique_blocks: std::collections::HashSet<&str> =
        states.iter().map(|s| s.name.as_str()).collect();
    debug!(
        total_states = states.len(),
        unique_blocks = unique_blocks.len(),
        "Canonical block state stats"
    );

    Ok(states)
}

/// Parse the children of a state compound tag.
fn parse_state_compound(
    cursor: &mut Cursor<&[u8]>,
    name: &mut String,
    states: &mut BTreeMap<String, StateValue>,
    version: &mut i32,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let tag_id = read_u8(cursor)?;
        if tag_id == 0 {
            // TAG_End
            break;
        }

        let field_name = read_string(cursor)?;

        match (tag_id, field_name.as_str()) {
            (8, "name") => {
                // TAG_String: block name
                *name = read_string(cursor)?;
            }
            (3, "version") => {
                // TAG_Int (ZigZag32): version number
                *version = read_zigzag32(cursor)?;
            }
            (10, "states") => {
                // TAG_Compound: block state properties
                parse_states_inner(cursor, states)?;
            }
            _ => {
                // Skip unknown fields
                skip_payload(tag_id, cursor)?;
            }
        }
    }
    Ok(())
}

/// Parse the inner "states" compound.
fn parse_states_inner(
    cursor: &mut Cursor<&[u8]>,
    states: &mut BTreeMap<String, StateValue>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let tag_id = read_u8(cursor)?;
        if tag_id == 0 {
            break;
        }

        let prop_name = read_string(cursor)?;

        let value = match tag_id {
            1 => {
                // TAG_Byte
                StateValue::Byte(read_u8(cursor)?)
            }
            3 => {
                // TAG_Int (ZigZag32)
                StateValue::Int(read_zigzag32(cursor)?)
            }
            8 => {
                // TAG_String
                StateValue::String(read_string(cursor)?)
            }
            _ => {
                skip_payload(tag_id, cursor)?;
                continue;
            }
        };

        states.insert(prop_name, value);
    }
    Ok(())
}

/// Skip an NBT payload based on tag type.
fn skip_payload(tag_id: u8, cursor: &mut Cursor<&[u8]>) -> Result<(), Box<dyn std::error::Error>> {
    match tag_id {
        1 => {
            read_u8(cursor)?;
        } // Byte
        2 => {
            skip(cursor, 2)?;
        } // Short
        3 => {
            read_zigzag32(cursor)?;
        } // Int
        4 => {
            read_zigzag64(cursor)?;
        } // Long
        5 => {
            skip(cursor, 4)?;
        } // Float
        6 => {
            skip(cursor, 8)?;
        } // Double
        7 => {
            // Byte Array
            let len = read_zigzag32(cursor)?;
            skip(cursor, len as usize)?;
        }
        8 => {
            // String
            read_string(cursor)?;
        }
        9 => {
            // List
            let inner_id = read_u8(cursor)?;
            let count = read_zigzag32(cursor)?;
            for _ in 0..count {
                skip_payload(inner_id, cursor)?;
            }
        }
        10 => {
            // Compound
            loop {
                let inner_tag = read_u8(cursor)?;
                if inner_tag == 0 {
                    break;
                }
                read_string(cursor)?; // skip name
                skip_payload(inner_tag, cursor)?;
            }
        }
        11 => {
            // Int Array
            let len = read_zigzag32(cursor)?;
            for _ in 0..len {
                read_zigzag32(cursor)?;
            }
        }
        12 => {
            // Long Array
            let len = read_zigzag32(cursor)?;
            for _ in 0..len {
                read_zigzag64(cursor)?;
            }
        }
        _ => return Err(format!("Unknown NBT tag type: {}", tag_id).into()),
    }
    Ok(())
}

// --- Low-level NBT reading helpers (Network Little-Endian) ---

fn read_u8(cursor: &mut Cursor<&[u8]>) -> Result<u8, Box<dyn std::error::Error>> {
    let mut buf = [0u8; 1];
    cursor.read_exact(&mut buf)?;
    Ok(buf[0])
}

fn read_string(cursor: &mut Cursor<&[u8]>) -> Result<String, Box<dyn std::error::Error>> {
    let len = read_var_u32(cursor)? as usize;
    let mut buf = vec![0u8; len];
    cursor.read_exact(&mut buf)?;
    Ok(String::from_utf8(buf)?)
}

fn skip(cursor: &mut Cursor<&[u8]>, n: usize) -> Result<(), Box<dyn std::error::Error>> {
    let mut buf = vec![0u8; n];
    cursor.read_exact(&mut buf)?;
    Ok(())
}

/// Read an unsigned VarInt (up to 32-bit).
fn read_var_u32(cursor: &mut Cursor<&[u8]>) -> Result<u32, Box<dyn std::error::Error>> {
    let mut result = 0u32;
    let mut shift = 0;
    loop {
        let byte = read_u8(cursor)?;
        result |= ((byte & 0x7F) as u32) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 35 {
            return Err("VarInt too large".into());
        }
    }
    Ok(result)
}

/// Read a ZigZag-encoded signed 32-bit integer.
fn read_zigzag32(cursor: &mut Cursor<&[u8]>) -> Result<i32, Box<dyn std::error::Error>> {
    let raw = read_var_u32(cursor)?;
    Ok(((raw >> 1) as i32) ^ -((raw & 1) as i32))
}

/// Read a ZigZag-encoded signed 64-bit integer.
fn read_zigzag64(cursor: &mut Cursor<&[u8]>) -> Result<i64, Box<dyn std::error::Error>> {
    let mut result = 0u64;
    let mut shift = 0;
    loop {
        let byte = read_u8(cursor)?;
        result |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 70 {
            return Err("VarLong too large".into());
        }
    }
    Ok(((result >> 1) as i64) ^ -((result & 1) as i64))
}

/// Generate the block_palette.bin file containing the serialized canonical block palette.
///
/// This is the complete palette blob that gets sent in StartGame.block_properties.
/// It's the raw concatenation of all canonical_block_states entries.
pub fn generate_block_palette_blob(
    canonical_states: &[CanonicalState],
    output_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;

    // Write the raw binary blob
    let bin_path = output_dir.join("block_palette.bin");
    let mut bin_file = fs::File::create(&bin_path)?;
    for state in canonical_states {
        bin_file.write_all(&state.raw_nbt)?;
    }

    // Write the Rust file that includes the blob
    let rs_path = output_dir.join("block_palette.rs");
    let mut rs_file = fs::File::create(&rs_path)?;
    writeln!(
        rs_file,
        "//! Generated canonical block palette blob for StartGame."
    )?;
    writeln!(rs_file, "//! Do not edit: regenerate with valentine_gen.")?;
    writeln!(rs_file)?;
    writeln!(
        rs_file,
        "/// Raw Network Little-Endian NBT blob of all canonical block states."
    )?;
    writeln!(rs_file, "/// Index in the list = block runtime ID.")?;
    writeln!(
        rs_file,
        "/// Total entries: {}, total bytes: {}",
        canonical_states.len(),
        canonical_states
            .iter()
            .map(|s| s.raw_nbt.len())
            .sum::<usize>()
    )?;
    writeln!(
        rs_file,
        "pub static BLOCK_PALETTE_NBT: &[u8] = include_bytes!(\"block_palette.bin\");"
    )?;

    info!(
        entries = canonical_states.len(),
        bytes = canonical_states
            .iter()
            .map(|s| s.raw_nbt.len())
            .sum::<usize>(),
        "Generated block palette blob"
    );

    Ok(())
}
