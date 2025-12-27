//! LevelDB key encoding for Bedrock world format.
//!
//! Based on Dragonfly's mcdb key format.

use crate::world::ChunkPos;

// Per-chunk key suffixes (appended after chunk index).
pub const KEY_VERSION: u8 = b','; // 0x2c
pub const KEY_SUBCHUNK: u8 = b'/'; // 0x2f (followed by Y index)
pub const KEY_3D_DATA: u8 = b'+'; // 0x2b (biomes)
pub const KEY_BLOCK_ENTITIES: u8 = b'1'; // 0x31
pub const KEY_ENTITIES_OLD: u8 = b'2'; // 0x32 (legacy)
pub const KEY_FINALISATION: u8 = b'6'; // 0x36

// Entity keys.
pub const KEY_ENTITY_IDS: &[u8] = b"digp";
pub const KEY_ENTITY_DATA: &[u8] = b"actorprefix";

/// Chunk version we write.
pub const CHUNK_VERSION: u8 = 41;

/// Encode chunk index bytes (x, z, dim).
///
/// Overworld (dim=0) uses 8-byte keys, other dimensions use 12-byte keys.
fn chunk_index(pos: ChunkPos, dim: i32) -> Vec<u8> {
    let mut buf = Vec::with_capacity(12);
    buf.extend_from_slice(&(pos.x as u32).to_le_bytes());
    buf.extend_from_slice(&(pos.z as u32).to_le_bytes());
    if dim != 0 {
        buf.extend_from_slice(&(dim as u32).to_le_bytes());
    }
    buf
}

/// Build a chunk key with suffix bytes.
pub fn chunk_key(pos: ChunkPos, dim: i32, suffix: &[u8]) -> Vec<u8> {
    let mut key = chunk_index(pos, dim);
    key.extend_from_slice(suffix);
    key
}

/// Build the version key for a chunk.
pub fn version_key(pos: ChunkPos, dim: i32) -> Vec<u8> {
    chunk_key(pos, dim, &[KEY_VERSION])
}

/// Build the subchunk key for a specific Y index.
pub fn subchunk_key(pos: ChunkPos, dim: i32, y_index: i8) -> Vec<u8> {
    chunk_key(pos, dim, &[KEY_SUBCHUNK, y_index as u8])
}

/// Build the 3D biome data key.
pub fn biome_key(pos: ChunkPos, dim: i32) -> Vec<u8> {
    chunk_key(pos, dim, &[KEY_3D_DATA])
}

/// Build the block entities key.
pub fn block_entities_key(pos: ChunkPos, dim: i32) -> Vec<u8> {
    chunk_key(pos, dim, &[KEY_BLOCK_ENTITIES])
}

/// Build the finalisation key.
pub fn finalisation_key(pos: ChunkPos, dim: i32) -> Vec<u8> {
    chunk_key(pos, dim, &[KEY_FINALISATION])
}

/// Build the entity IDs key for a chunk (digp prefix).
pub fn entity_ids_key(pos: ChunkPos, dim: i32) -> Vec<u8> {
    let mut key = KEY_ENTITY_IDS.to_vec();
    key.extend_from_slice(&chunk_index(pos, dim));
    key
}

/// Build the entity data key for a specific entity ID.
pub fn entity_data_key(entity_id: i64) -> Vec<u8> {
    let mut key = KEY_ENTITY_DATA.to_vec();
    key.extend_from_slice(&(entity_id as u64).to_le_bytes());
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_key_overworld() {
        let pos = ChunkPos::new(1, 2);
        let key = version_key(pos, 0);
        // 4 bytes x + 4 bytes z + 1 byte suffix = 9 bytes
        assert_eq!(key.len(), 9);
        assert_eq!(key[8], KEY_VERSION);
    }

    #[test]
    fn test_chunk_key_nether() {
        let pos = ChunkPos::new(1, 2);
        let key = version_key(pos, 1);
        // 4 bytes x + 4 bytes z + 4 bytes dim + 1 byte suffix = 13 bytes
        assert_eq!(key.len(), 13);
        assert_eq!(key[12], KEY_VERSION);
    }

    #[test]
    fn test_subchunk_key() {
        let pos = ChunkPos::new(0, 0);
        let key = subchunk_key(pos, 0, -4);
        assert_eq!(key[8], KEY_SUBCHUNK);
        assert_eq!(key[9] as i8, -4);
    }

    #[test]
    fn test_negative_coords() {
        let pos = ChunkPos::new(-10, -20);
        let key = version_key(pos, 0);
        // Should still be 9 bytes for overworld
        assert_eq!(key.len(), 9);
        // Verify x is encoded as little-endian u32 (negative becomes large positive)
        let x_bytes: [u8; 4] = key[0..4].try_into().unwrap();
        assert_eq!(u32::from_le_bytes(x_bytes), (-10i32) as u32);
    }

    #[test]
    fn test_biome_key_format() {
        let pos = ChunkPos::new(5, 10);
        let key = biome_key(pos, 0);
        assert_eq!(key.len(), 9);
        assert_eq!(key[8], KEY_3D_DATA);
    }

    #[test]
    fn test_finalisation_key() {
        let pos = ChunkPos::new(0, 0);
        let key = finalisation_key(pos, 0);
        assert_eq!(key[8], KEY_FINALISATION);
    }

    #[test]
    fn test_block_entities_key() {
        let pos = ChunkPos::new(0, 0);
        let key = block_entities_key(pos, 0);
        assert_eq!(key[8], KEY_BLOCK_ENTITIES);
    }

    #[test]
    fn test_entity_ids_key() {
        let pos = ChunkPos::new(1, 2);
        let key = entity_ids_key(pos, 0);
        // "digp" (4) + chunk index (8) = 12 bytes for overworld
        assert_eq!(key.len(), 12);
        assert_eq!(&key[0..4], KEY_ENTITY_IDS);
    }

    #[test]
    fn test_entity_data_key() {
        let key = entity_data_key(12345);
        // "actorprefix" (11) + u64 (8) = 19 bytes
        assert_eq!(key.len(), 19);
        assert_eq!(&key[0..11], KEY_ENTITY_DATA);
    }

    #[test]
    fn test_end_dimension() {
        let pos = ChunkPos::new(0, 0);
        let key = version_key(pos, 2);
        // End (dim=2) should use 12-byte index + 1 suffix = 13 bytes
        assert_eq!(key.len(), 13);
        let dim_bytes: [u8; 4] = key[8..12].try_into().unwrap();
        assert_eq!(u32::from_le_bytes(dim_bytes), 2);
    }
}
