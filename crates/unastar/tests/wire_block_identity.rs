//! Finished chunks decoded from their exact SubChunk wire bytes, as a client does.
//!
//! StartGame advertises sequential (non-hashed) block network ids. A client then
//! numbers its *own* palette: its built-in block states plus the blocks StartGame
//! defines, ordered by the FNV-1 64 hash of the block name. The vanilla
//! data-driven blocks (behavior pack `blocks/`) are not built in, so a server that
//! does not define them sees every later id resolve to a different block. This
//! test rebuilds that client palette from the StartGame definitions the server
//! sends and resolves the bytes the real `ChunkPipeline` and network sub-chunk
//! encoder produce, with a decoder independent of `Chunk`'s own.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use unastar::registry::BlockRegistry;
use unastar::world::generator::ChunkPipeline;
use unastar::world::{ChunkPos, WorldGenerator};

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

/// The sequential palette a client builds after this server's StartGame:
/// built-in blocks plus defined blocks, each block's states in canonical order.
static CLIENT_PALETTE: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    let registry = BlockRegistry::vanilla();
    let defined: BTreeSet<String> = registry
        .to_block_properties()
        .iter()
        .map(|property| property.block_name.to_string())
        .collect();
    let data_driven = data_driven_blocks();
    let mut blocks: Vec<_> = registry
        .iter()
        .filter(|block| {
            !data_driven.contains(&block.string_id) || defined.contains(&block.string_id)
        })
        .collect();
    blocks.sort_by_key(|block| fnv1_64(&block.string_id));
    blocks
        .into_iter()
        .flat_map(|block| {
            std::iter::repeat_n(block.string_id.as_str(), block.state_id_count as usize)
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

/// Primary block storage of every section, decoded from `Chunk::encode_subchunk`.
struct WireChunk {
    sections: Vec<Vec<u32>>,
}

impl WireChunk {
    fn generate(generator: WorldGenerator, pos: ChunkPos) -> Self {
        let chunk = ChunkPipeline::new(generator.build())
            .generate(pos)
            .expect("finished chunk");
        let sections = (-4..20)
            .map(|y_index| {
                let wire = chunk.encode_subchunk(y_index).expect("in-range section");
                let mut data = wire.as_slice();
                assert_eq!(data[0], 9, "sub-chunk version");
                assert_eq!(i32::from(data[2] as i8), y_index);
                data = &data[3..];
                storage(&mut data)
            })
            .collect();
        Self { sections }
    }

    fn name(&self, x: usize, y: i32, z: usize) -> &'static str {
        let id = self.sections[((y + 64) >> 4) as usize][(x << 8) | (z << 4) | (y & 15) as usize];
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
    let wire = WireChunk::generate(WorldGenerator::SuperFlat, ChunkPos::new(0, 0));
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
        ]
    );
}

#[test]
fn java_wire_blocks_are_natural_terrain() {
    on_large_stack(|| {
        let generator = WorldGenerator::Java { seed: 12345 };
        let ocean = WireChunk::generate(generator, ChunkPos::new(0, 0));
        assert_eq!(ocean.name(0, -64, 0), "minecraft:bedrock");
        assert_eq!(ocean.name(5, -20, 5), "minecraft:deepslate");
        assert_eq!(ocean.surface(8, 8), (62, "minecraft:water"));
        let land = WireChunk::generate(generator, ChunkPos::new(3, -2));
        assert_eq!(land.name(5, 10, 5), "minecraft:stone");
    });
}

#[cfg(feature = "bedrock-worldgen")]
#[test]
fn bedrock_wire_blocks_are_natural_terrain() {
    on_large_stack(|| {
        let wire =
            WireChunk::generate(WorldGenerator::Bedrock { seed: 12345 }, ChunkPos::new(0, 0));
        assert_eq!(wire.name(0, -64, 0), "minecraft:bedrock");
        assert_eq!(wire.name(5, -20, 5), "minecraft:deepslate");
        assert_eq!(wire.surface(8, 8), (62, "minecraft:water"));
    });
}
