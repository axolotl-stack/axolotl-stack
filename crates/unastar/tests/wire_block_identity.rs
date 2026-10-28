//! Finished chunks decoded from their exact SubChunk wire bytes, as a client does,
//! in both block network id modes `StartGame` can advertise.
//!
//! Sequential: a client numbers its *own* palette, its built-in block states plus
//! the blocks StartGame defines, ordered by the FNV-1 64 hash of the block name.
//! The vanilla data-driven blocks (behavior pack `blocks/`) are not built in, so a
//! server that does not define them sees every later id resolve to a different
//! block. Hashed: a client derives each known state's id itself, as the FNV-1a
//! 32 hash of its little-endian `{name, states}` NBT.
//!
//! Both client models are rebuilt here from the StartGame definitions the server
//! sends, independently of the server's own id tables, and resolve the bytes the
//! real `ChunkPipeline` and network sub-chunk encoder produce, with a decoder
//! independent of `Chunk`'s own.

use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

use unastar::registry::BlockRegistry;
use unastar::registry::block::{BlockEntry, BlockNetworkIds};
use unastar::world::generator::ChunkPipeline;
use unastar::world::{ChunkPos, WorldGenerator};
use unastar_data::blocks::StateValue;

fn fnv1_64(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        hash.wrapping_mul(0x0000_0100_0000_01b3) ^ u64::from(byte)
    })
}

/// Block identifiers of the vanilla behavior pack's data-driven blocks.
fn data_driven_blocks() -> BTreeSet<String> {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../unastar-data/data/vanilla_bp/behavior_pack/blocks"
    );
    std::fs::read_dir(dir)
        .expect("vanilla behavior pack blocks")
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(|path| {
            let definition: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            definition["minecraft:block"]["description"]["identifier"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}

/// Registry blocks a client knows after this server's StartGame: built-in blocks
/// plus the data-driven blocks StartGame defines.
fn client_blocks() -> Vec<&'static BlockEntry> {
    let registry = BlockRegistry::vanilla();
    let defined: BTreeSet<String> = registry
        .to_block_properties()
        .iter()
        .map(|property| property.block_name.to_string())
        .collect();
    let data_driven = data_driven_blocks();
    registry
        .iter()
        .filter(|block| {
            !data_driven.contains(&block.string_id) || defined.contains(&block.string_id)
        })
        .collect()
}

/// The sequential palette a client builds: known blocks by name hash, each
/// block's states in canonical order.
static CLIENT_PALETTE: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    let mut blocks = client_blocks();
    blocks.sort_by_key(|block| fnv1_64(&block.string_id));
    blocks
        .into_iter()
        .flat_map(|block| {
            std::iter::repeat_n(block.string_id.as_str(), block.state_id_count as usize)
        })
        .collect()
});

fn put_string(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as u16).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
}

/// FNV-1a 32 of a state's little-endian `{name, states}` NBT, properties by key.
fn state_hash(block: &BlockEntry, runtime_id: u32) -> u32 {
    let mut nbt = vec![10, 0, 0, 8];
    put_string(&mut nbt, "name");
    put_string(&mut nbt, &block.string_id);
    nbt.push(10);
    put_string(&mut nbt, "states");
    for property in block.data.properties {
        match block.state_value(runtime_id, property.name).unwrap() {
            StateValue::Byte(value) => {
                nbt.push(1);
                put_string(&mut nbt, property.name);
                nbt.push(value);
            }
            StateValue::Int(value) => {
                nbt.push(3);
                put_string(&mut nbt, property.name);
                nbt.extend_from_slice(&value.to_le_bytes());
            }
            StateValue::Str(value) => {
                nbt.push(8);
                put_string(&mut nbt, property.name);
                put_string(&mut nbt, value);
            }
        }
    }
    nbt.extend_from_slice(&[0, 0]);
    nbt.iter().fold(0x811c_9dc5, |hash, &byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// The hashed ids a client resolves: every state of every known block.
static CLIENT_HASHES: LazyLock<HashMap<u32, &'static str>> = LazyLock::new(|| {
    client_blocks()
        .into_iter()
        .flat_map(|block| {
            (block.min_state_id..=block.max_state_id)
                .map(move |state| (state_hash(block, state), block.string_id.as_str()))
        })
        .collect()
});

fn varint(data: &mut &[u8]) -> i32 {
    let mut value = 0u32;
    for shift in (0..35).step_by(7) {
        let byte = data[0];
        *data = &data[1..];
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return ((value >> 1) as i32) ^ -((value & 1) as i32);
        }
    }
    panic!("varint too long")
}

/// One network paletted storage, indexed `(x << 8) | (z << 4) | y`.
fn storage(data: &mut &[u8]) -> Vec<u32> {
    let header = data[0];
    *data = &data[1..];
    assert_eq!(header & 1, 1, "network storages use runtime-id palettes");
    let bits = u32::from(header >> 1);
    if bits == 0 {
        return vec![varint(data) as u32; 4096];
    }
    let per_word = 32 / bits;
    let words = 4096u32.div_ceil(per_word) as usize;
    let packed: Vec<u32> = (0..words)
        .map(|i| u32::from_le_bytes(data[i * 4..i * 4 + 4].try_into().unwrap()))
        .collect();
    *data = &data[words * 4..];
    let len = varint(data);
    let palette: Vec<u32> = (0..len).map(|_| varint(data) as u32).collect();
    (0..4096u32)
        .map(|i| {
            let word = packed[(i / per_word) as usize];
            palette[((word >> ((i % per_word) * bits)) & ((1 << bits) - 1)) as usize]
        })
        .collect()
}

/// Primary block storage of every section, decoded from `Chunk::encode_subchunk`
/// in one wire id mode.
struct WireChunk {
    ids: BlockNetworkIds,
    sections: Vec<Vec<u32>>,
}

impl WireChunk {
    /// One finished chunk, encoded in both wire id modes.
    fn generate(generator: WorldGenerator, pos: ChunkPos) -> [Self; 2] {
        let chunk = ChunkPipeline::new(generator.build())
            .generate(pos)
            .expect("finished chunk");
        [BlockNetworkIds::Sequential, BlockNetworkIds::Hashed].map(|ids| {
            let sections = (-4..20)
                .map(|y_index| {
                    let wire = chunk
                        .encode_subchunk(y_index, ids)
                        .expect("in-range section");
                    let mut data = wire.as_slice();
                    assert_eq!(data[0], 9, "sub-chunk version");
                    assert_eq!(i32::from(data[2] as i8), y_index);
                    data = &data[3..];
                    storage(&mut data)
                })
                .collect();
            Self { ids, sections }
        })
    }

    /// The block a client resolves the wire id at this position to.
    fn name(&self, x: usize, y: i32, z: usize) -> &'static str {
        let id = self.sections[((y + 64) >> 4) as usize][(x << 8) | (z << 4) | (y & 15) as usize];
        match self.ids {
            BlockNetworkIds::Sequential => {
                let client = CLIENT_PALETTE
                    .get(id as usize)
                    .unwrap_or_else(|| panic!("network id {id} is outside the client palette"));
                let server = BlockRegistry::vanilla()
                    .get_by_runtime_id(id)
                    .map(|block| block.string_id.as_str());
                assert_eq!(
                    Some(*client),
                    server,
                    "client and server disagree on network id {id}"
                );
                client
            }
            BlockNetworkIds::Hashed => CLIENT_HASHES
                .get(&id)
                .unwrap_or_else(|| panic!("hashed network id {id} is unknown to the client")),
        }
    }

    /// Height and name of a column's top non-air block.
    fn surface(&self, x: usize, z: usize) -> (i32, &'static str) {
        (-64..320)
            .rev()
            .map(|y| (y, self.name(x, y, z)))
            .find(|(_, name)| *name != "minecraft:air")
            .expect("column has a non-air block")
    }
}

/// Worldgen recursion needs more than the default test thread stack.
fn on_large_stack(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn super_flat_wire_column_is_bedrock_dirt_grass() {
    for wire in WireChunk::generate(WorldGenerator::SuperFlat, ChunkPos::new(0, 0)) {
        let column: Vec<_> = (-1..5).map(|y| wire.name(3, y, 7)).collect();
        assert_eq!(
            column,
            [
                "minecraft:air",
                "minecraft:bedrock",
                "minecraft:dirt",
                "minecraft:dirt",
                "minecraft:grass_block",
                "minecraft:air",
            ],
            "{:?}",
            wire.ids
        );
    }
}

#[test]
fn java_wire_blocks_are_natural_terrain() {
    on_large_stack(|| {
        let generator = WorldGenerator::Java { seed: 12345 };
        let [ocean, land] = [ChunkPos::new(0, 0), ChunkPos::new(3, -2)]
            .map(|pos| WireChunk::generate(generator, pos));
        for ocean in ocean {
            assert_eq!(ocean.name(0, -64, 0), "minecraft:bedrock");
            assert_eq!(ocean.name(5, -20, 5), "minecraft:deepslate");
            assert_eq!(ocean.surface(8, 8), (62, "minecraft:water"));
        }
        for land in land {
            assert_eq!(land.name(5, 10, 5), "minecraft:stone");
        }
    });
}

#[cfg(feature = "bedrock-worldgen")]
#[test]
fn bedrock_wire_blocks_are_natural_terrain() {
    on_large_stack(|| {
        for wire in
            WireChunk::generate(WorldGenerator::Bedrock { seed: 12345 }, ChunkPos::new(0, 0))
        {
            assert_eq!(wire.name(0, -64, 0), "minecraft:bedrock");
            assert_eq!(wire.name(5, -20, 5), "minecraft:deepslate");
            assert_eq!(wire.surface(8, 8), (62, "minecraft:water"));
        }
    });
}
