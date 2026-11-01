//! Stable typed NBT fingerprints, independent of compound field order.

use super::ColumnWriteError;
use sha2::{Digest, Sha256};
use zuri_nbt::{NBTTag, encoding::LittleEndian};

/// Hash complete rooted records with explicit lengths and tag domains. Root
/// names do not identify block actors; their typed compound fields do.
pub(super) fn nbt_stream(hash: &mut Sha256, bytes: &[u8]) -> Result<(), ColumnWriteError> {
    let mut input = bytes;
    while !input.is_empty() {
        let value =
            NBTTag::read(&mut input, LittleEndian).map_err(|_| ColumnWriteError::Invalid)?;
        tag(hash, &value, 0)?;
    }
    hash.update([0]);
    Ok(())
}
/// Hash one bounded typed tree without allocating a second serialized payload.
fn tag(hash: &mut Sha256, value: &NBTTag, depth: usize) -> Result<(), ColumnWriteError> {
    if depth > 64 {
        return Err(ColumnWriteError::Invalid);
    }
    match value {
        NBTTag::Byte(value) => {
            hash.update([1, value.0]);
        }
        NBTTag::Short(value) => {
            hash.update([2]);
            hash.update(value.0.to_le_bytes());
        }
        NBTTag::Int(value) => {
            hash.update([3]);
            hash.update(value.0.to_le_bytes());
        }
        NBTTag::Long(value) => {
            hash.update([4]);
            hash.update(value.0.to_le_bytes());
        }
        NBTTag::Float(value) => {
            hash.update([5]);
            hash.update(value.0.to_bits().to_le_bytes());
        }
        NBTTag::Double(value) => {
            hash.update([6]);
            hash.update(value.0.to_bits().to_le_bytes());
        }
        NBTTag::ByteArray(value) => {
            hash.update([7]);
            bytes(hash, &value.0);
        }
        NBTTag::String(value) => {
            hash.update([8]);
            bytes(hash, value.0.as_bytes());
        }
        NBTTag::List(value) => {
            hash.update([9]);
            hash.update((value.0.len() as u64).to_le_bytes());
            for value in &value.0 {
                tag(hash, value, depth + 1)?;
            }
        }
        NBTTag::Compound(value) => {
            hash.update([10]);
            hash.update((value.0.len() as u64).to_le_bytes());
            let mut fields: Vec<_> = value.0.iter().collect();
            fields.sort_unstable_by(|a, b| a.0.cmp(b.0));
            for (name, value) in fields {
                bytes(hash, name.as_bytes());
                tag(hash, value, depth + 1)?;
            }
        }
        NBTTag::IntArray(value) => {
            hash.update([11]);
            hash.update((value.0.len() as u64).to_le_bytes());
            for value in &value.0 {
                hash.update(value.to_le_bytes());
            }
        }
        NBTTag::LongArray(value) => {
            hash.update([12]);
            hash.update((value.0.len() as u64).to_le_bytes());
            for value in &value.0 {
                hash.update(value.to_le_bytes());
            }
        }
    }
    Ok(())
}
/// Frame a string or byte array so adjacent values cannot share a fingerprint.
fn bytes(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}
