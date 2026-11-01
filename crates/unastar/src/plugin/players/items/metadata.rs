//! Deterministic encoding for changed item compounds, retaining native envelope tails.

use super::*;
use std::io::Cursor;
use zuri_nbt::encoding::LittleEndian;

/// Preserve block restrictions and other envelope fields without exposing encoded buffers.
pub(super) fn replace(stack: &Stack, compound: tag::Compound) -> Result<Stack, wit::AccessError> {
    let mut bytes = if compound.0.is_empty() {
        vec![0, 0]
    } else {
        let mut bytes = vec![255, 255, 1, 10, 0, 0];
        payload(&mut bytes, &NBTTag::Compound(compound.clone()), 0)?;
        bytes
    };
    let updated = crate::item::enchant::with_item_tag(stack, compound);
    let buffer = updated.user_data();
    let mut cursor = Cursor::new(&buffer[3..]);
    NBTTag::read(&mut cursor, LittleEndian).map_err(|_| wit::AccessError::InvalidArgument)?;
    let tail = buffer
        .get(3 + cursor.position() as usize..)
        .ok_or(wit::AccessError::InvalidArgument)?;
    bytes.extend_from_slice(tail);
    if bytes.len() > MAX_ITEM_BYTES {
        return Err(wit::AccessError::LimitExceeded);
    }
    Ok(stack.clone().with_nbt(bytes))
}

/// Write compounds in key order and recurse through lists; primitives use the public codec.
fn payload(bytes: &mut Vec<u8>, value: &NBTTag, depth: usize) -> Result<(), wit::AccessError> {
    if depth > 32 || bytes.len() > MAX_ITEM_BYTES {
        return Err(wit::AccessError::LimitExceeded);
    }
    match value {
        NBTTag::Compound(compound) => {
            let mut entries: Vec<_> = compound.0.iter().collect();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            for (name, value) in entries {
                bytes.push(kind(value));
                let length =
                    i16::try_from(name.len()).map_err(|_| wit::AccessError::LimitExceeded)?;
                bytes.extend_from_slice(&length.to_le_bytes());
                bytes.extend_from_slice(name.as_bytes());
                payload(bytes, value, depth + 1)?;
            }
            bytes.push(0);
        }
        NBTTag::List(values) => {
            let first = values.0.first().map_or(1, kind);
            bytes.push(first);
            let length =
                i32::try_from(values.0.len()).map_err(|_| wit::AccessError::LimitExceeded)?;
            bytes.extend_from_slice(&length.to_le_bytes());
            for value in &values.0 {
                if kind(value) != first {
                    return Err(wit::AccessError::InvalidArgument);
                }
                payload(bytes, value, depth + 1)?;
            }
        }
        _ => bytes.extend_from_slice(&leaf(value)?[3..]),
    }
    if bytes.len() > MAX_ITEM_BYTES {
        return Err(wit::AccessError::LimitExceeded);
    }
    Ok(())
}

/// Container framing uses NBT's public tag discriminants, independently of map order.
fn kind(value: &NBTTag) -> u8 {
    match value {
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

/// Delegate scalar representation and validation to the pinned codec dependency.
fn leaf(value: &NBTTag) -> Result<Vec<u8>, wit::AccessError> {
    let mut bytes = Vec::new();
    value
        .write(&mut bytes, LittleEndian)
        .map_err(|_| wit::AccessError::InvalidArgument)?;
    Ok(bytes)
}
