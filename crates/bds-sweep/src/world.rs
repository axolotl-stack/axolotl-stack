//! The blocks around the scripted player, as BDS sends them, so its
//! movement can collide with the world like a vanilla client's.
//!
//! BDS 1.26.51 sends the overworld in sub-chunk request mode: `LevelChunk`
//! carries only biomes and the number of sub-chunks from the bottom that hold
//! blocks; the client asks for those with `SubChunkRequest` and gets them in
//! `SubChunk` packets. Sub-chunks above that count are air until an
//! `UpdateBlock` or `UpdateSubChunkBlocks` changes them. Palette entries are
//! hashed block network ids (`block-network-ids-are-hashes`), resolved to
//! states, collision boxes and friction through `unastar-data`.
//!
//! Only what player movement needs is kept: layer 0 (blocks) and layer 1
//! (the water of waterlogged blocks). Fences and walls collide with their
//! unconnected box (no neighbour connections are computed).

use anyhow::{Context, Result, bail, ensure};
use jolyne::valentine::{
    EnumsSubChunkPacketPayloadSubChunkRequestResult as SubChunkResult, LevelChunkPacket,
    SubChunkPacket, SubChunkPacketPayloadSubChunkPosOffset, SubChunkPos, SubChunkRequestPacket,
};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use unastar_data::blocks::{self, BlockData};

use crate::physics::{Aabb, Block as Motion, BlockKind, Terrain};

/// Lowest sub-chunk index of the overworld (y -64).
const MIN_INDEX: i32 = -4;
/// Number of sub-chunks in an overworld column (y -64..320).
const COLUMN: i32 = 24;
/// Chunks within this many chunks of the player have their sub-chunks
/// requested.
const RADIUS: i32 = 2;

/// Block network hash to canonical state id.
static STATES: LazyLock<HashMap<u32, u32>> = LazyLock::new(|| {
    let mut states = HashMap::new();
    for block in blocks::ALL_BLOCKS.iter() {
        for (offset, hash) in block.network_hashes.iter().enumerate() {
            states.insert(*hash, block.min_state_id + offset as u32);
        }
    }
    states
});

/// The canonical state of a block network hash.
fn state_of(hash: u32) -> Result<u32> {
    STATES
        .get(&hash)
        .copied()
        .with_context(|| format!("unknown block network hash {hash:#010x}"))
}

/// The air state.
fn air() -> u32 {
    blocks::get("minecraft:air").expect("air").min_state_id
}

/// One sub-chunk: layer 0 and, when present, layer 1, each 4096 states
/// indexed `x << 8 | z << 4 | y`.
#[derive(Clone)]
struct SubChunk {
    layers: [Box<[u32; 4096]>; 2],
}

impl SubChunk {
    fn air() -> Self {
        let air = air();
        Self {
            layers: [Box::new([air; 4096]), Box::new([air; 4096])],
        }
    }
}

fn index(x: i32, y: i32, z: i32) -> usize {
    (((x & 15) << 8) | ((z & 15) << 4) | (y & 15)) as usize
}

/// A signed (zigzag) varint32.
fn read_varint(data: &[u8], at: &mut usize) -> Result<i32> {
    let mut value = 0u32;
    for shift in (0..35).step_by(7) {
        let byte = *data.get(*at).context("varint past the end")?;
        *at += 1;
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(((value >> 1) as i32) ^ -((value & 1) as i32));
        }
    }
    bail!("varint longer than 5 bytes")
}

/// Decode one paletted block storage at `data[*at..]` into `out`.
fn decode_storage(data: &[u8], at: &mut usize, out: &mut [u32; 4096]) -> Result<()> {
    let header = *data.get(*at).context("storage header past the end")?;
    *at += 1;
    let bits = u32::from(header >> 1);
    ensure!(
        header & 1 == 1,
        "storage header {header:#x} is not a runtime palette"
    );
    if bits == 0 {
        let state = state_of(read_varint(data, at)? as u32)?;
        out.fill(state);
        return Ok(());
    }
    ensure!(
        matches!(bits, 1 | 2 | 3 | 4 | 5 | 6 | 8 | 16),
        "unsupported palette width {bits}"
    );
    let per_word = 32 / bits;
    let words = 4096_usize.div_ceil(per_word as usize);
    let bytes = data
        .get(*at..*at + words * 4)
        .context("storage words past the end")?;
    *at += words * 4;
    let count = read_varint(data, at)?;
    ensure!(count > 0, "empty palette");
    let palette = (0..count)
        .map(|_| state_of(read_varint(data, at)? as u32))
        .collect::<Result<Vec<_>>>()?;
    let mask = (1u32 << bits) - 1;
    for (slot, state) in out.iter_mut().enumerate() {
        let word = slot / per_word as usize;
        let shift = (slot % per_word as usize) as u32 * bits;
        let word = u32::from_le_bytes(bytes[word * 4..word * 4 + 4].try_into().expect("4 bytes"));
        let entry = ((word >> shift) & mask) as usize;
        *state = *palette
            .get(entry)
            .with_context(|| format!("palette index {entry} of {count}"))?;
    }
    Ok(())
}

/// Decode a network sub-chunk (versions 8 and 9; block entities after the
/// storages are ignored).
fn decode_sub_chunk(data: &[u8]) -> Result<SubChunk> {
    let mut sub = SubChunk::air();
    let version = *data.first().context("empty sub-chunk")?;
    let mut at = match version {
        8 => 2,
        9 => 3,
        other => bail!("unsupported sub-chunk version {other}"),
    };
    let storages = *data.get(1).context("sub-chunk without a storage count")?;
    ensure!(storages <= 2, "{storages} storages in a sub-chunk");
    for layer in 0..usize::from(storages) {
        decode_storage(data, &mut at, &mut sub.layers[layer])?;
    }
    Ok(sub)
}

/// What a block is to a moving player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Block {
    pub state: u32,
    pub data: &'static BlockData,
}

impl Block {
    pub fn identifier(&self) -> &'static str {
        self.data.identifier
    }

    /// Water, still or flowing (layer 0 or the water of a waterlogged block).
    pub fn is_water(&self) -> bool {
        matches!(
            self.data.identifier,
            "minecraft:water" | "minecraft:flowing_water"
        )
    }

    /// Its collision boxes, in world coordinates, for the block at `pos`.
    pub fn boxes(&self, pos: [i32; 3]) -> Result<Vec<Aabb>> {
        let shape = unastar_data::shapes::collision(self.state)
            .with_context(|| format!("no collision shape for state {}", self.state))?
            .boxes()
            .with_context(|| format!("collision shape of {} unavailable", self.identifier()))?;
        let origin = pos.map(|v| v as f32);
        Ok(shape
            .iter()
            .map(|b| Aabb {
                min: [origin[0] + b[0], origin[1] + b[1], origin[2] + b[2]],
                max: [origin[0] + b[3], origin[1] + b[4], origin[2] + b[5]],
            })
            .collect())
    }
}

/// The blocks BDS has sent this client.
#[derive(Default)]
pub struct World {
    sub_chunks: HashMap<[i32; 3], SubChunk>,
    /// Chunk columns BDS announced, with the number of sub-chunks from the
    /// bottom that hold blocks (the rest are air).
    columns: HashMap<[i32; 2], i32>,
    /// Columns whose sub-chunks were requested.
    requested: HashSet<[i32; 2]>,
}

impl World {
    /// The block at `pos` (layer 0), or `None` while its sub-chunk is not
    /// loaded.
    pub fn block(&self, pos: [i32; 3]) -> Option<Block> {
        self.layer(pos, 0)
    }

    /// The block at `pos` in `layer`.
    fn layer(&self, pos: [i32; 3], layer: usize) -> Option<Block> {
        let key = [pos[0] >> 4, pos[1] >> 4, pos[2] >> 4];
        let state = match self.sub_chunks.get(&key) {
            Some(sub) => sub.layers[layer][index(pos[0], pos[1], pos[2])],
            None => {
                let limit = *self.columns.get(&[key[0], key[2]])?;
                // Sub-chunks above the announced count are air; those below it
                // are unknown until their SubChunk arrives.
                if key[1] < MIN_INDEX + limit && key[1] >= MIN_INDEX {
                    return None;
                }
                air()
            }
        };
        Some(Block {
            state,
            data: blocks::by_runtime_id(state).expect("decoded states are canonical"),
        })
    }

    /// Whether `pos` holds water in either layer; `None` while not loaded.
    pub fn water_at(&self, pos: [i32; 3]) -> Option<bool> {
        Some(self.block(pos)?.is_water() || self.layer(pos, 1)?.is_water())
    }

    /// Whether every block in `area` is loaded.
    pub fn loaded(&self, area: &Aabb) -> bool {
        let (min, max) = area.block_range();
        (min[0]..=max[0]).all(|x| {
            (min[1]..=max[1]).all(|y| (min[2]..=max[2]).all(|z| self.block([x, y, z]).is_some()))
        })
    }

    /// The collision boxes of the blocks that can touch `area` (one layer
    /// below it too: fences and walls reach 1.5 blocks up). Unloaded blocks
    /// count as empty; callers check [`World::loaded`] first.
    pub fn collisions(&self, area: &Aabb) -> Result<Vec<Aabb>> {
        let (min, max) = area.block_range();
        let mut boxes = Vec::new();
        for x in min[0]..=max[0] {
            for y in (min[1] - 1)..=max[1] {
                for z in min[2]..=max[2] {
                    let Some(block) = self.block([x, y, z]) else {
                        continue;
                    };
                    for b in block.boxes([x, y, z])? {
                        if b.intersects(area) {
                            boxes.push(b);
                        }
                    }
                }
            }
        }
        Ok(boxes)
    }

    /// A `LevelChunk`: remember the column; returns the sub-chunk requests a
    /// client near it sends.
    pub fn level_chunk(
        &mut self,
        packet: &LevelChunkPacket,
        player: [f32; 3],
    ) -> Result<Vec<SubChunkRequestPacket>> {
        let column = [packet.chunk_position.x, packet.chunk_position.z];
        let Some(limit) = packet.client_request_sub_chunk_limit else {
            bail!(
                "LevelChunk {column:?} carries {} sub-chunks inline; only request mode is supported",
                packet.subchunks_count
            );
        };
        let limit = if limit < 0 { COLUMN } else { limit.min(COLUMN) };
        self.columns.insert(column, limit);
        // A resent column replaces what was known of it.
        self.sub_chunks.retain(|key, _| [key[0], key[2]] != column);
        self.requested.remove(&column);
        Ok(self.request_near(player))
    }

    /// Requests for the announced columns near `player` not requested yet.
    pub fn request_near(&mut self, player: [f32; 3]) -> Vec<SubChunkRequestPacket> {
        let center = [
            (player[0].floor() as i32) >> 4,
            (player[2].floor() as i32) >> 4,
        ];
        let mut requests = Vec::new();
        let due: Vec<[i32; 2]> = self
            .columns
            .keys()
            .filter(|c| {
                (c[0] - center[0]).abs() <= RADIUS
                    && (c[1] - center[1]).abs() <= RADIUS
                    && !self.requested.contains(*c)
            })
            .copied()
            .collect();
        for column in due {
            self.requested.insert(column);
            let limit = self.columns[&column];
            if limit == 0 {
                continue;
            }
            requests.push(SubChunkRequestPacket {
                dimension_type: Default::default(),
                center_pos: SubChunkPos {
                    subchunk_position_x: column[0],
                    subchunk_position_y: 0,
                    subchunk_position_z: column[1],
                },
                sub_chunk_position_offset_list: (MIN_INDEX..MIN_INDEX + limit)
                    .map(|y| SubChunkPacketPayloadSubChunkPosOffset {
                        subchunk_offset_x: 0,
                        subchunk_offset_y: y as i8,
                        subchunk_offset_z: 0,
                    })
                    .collect(),
            });
        }
        requests
    }

    /// A `SubChunk` answer.
    pub fn sub_chunk(&mut self, packet: &SubChunkPacket) -> Result<()> {
        ensure!(!packet.cache_enabled, "SubChunk with the blob cache");
        let center = &packet.center_pos;
        for entry in &packet.sub_chunk_data {
            let offset = &entry.sub_chunk_pos_offset;
            let key = [
                center.subchunk_position_x + i32::from(offset.subchunk_offset_x),
                center.subchunk_position_y + i32::from(offset.subchunk_offset_y),
                center.subchunk_position_z + i32::from(offset.subchunk_offset_z),
            ];
            let sub = match entry.sub_chunk_request_result {
                SubChunkResult::Success => decode_sub_chunk(
                    entry
                        .serialized_sub_chunk
                        .as_deref()
                        .context("successful SubChunk entry without data")?,
                )
                .with_context(|| format!("sub-chunk {key:?}"))?,
                SubChunkResult::Successallair => SubChunk::air(),
                other => bail!("SubChunk {key:?} answered {other:?}"),
            };
            self.sub_chunks.insert(key, sub);
        }
        Ok(())
    }

    /// One block change (`UpdateBlock`, an `UpdateSubChunkBlocks` entry).
    pub fn set_block(&mut self, pos: [i32; 3], hash: u32, layer: usize) -> Result<()> {
        ensure!(layer < 2, "block layer {layer}");
        let state = state_of(hash)?;
        let key = [pos[0] >> 4, pos[1] >> 4, pos[2] >> 4];
        let column = [key[0], key[2]];
        if !self.sub_chunks.contains_key(&key) {
            let Some(&limit) = self.columns.get(&column) else {
                // A column this client never got: nothing to keep.
                return Ok(());
            };
            if key[1] < MIN_INDEX + limit {
                // Still on its way: the SubChunk answer will carry it.
                return Ok(());
            }
            self.sub_chunks.insert(key, SubChunk::air());
        }
        let sub = self.sub_chunks.get_mut(&key).expect("inserted above");
        sub.layers[layer][index(pos[0], pos[1], pos[2])] = state;
        Ok(())
    }
}

impl Terrain for World {
    fn collisions(&self, area: &Aabb, out: &mut Vec<Aabb>) {
        // A state without a collision shape is a data gap the movement
        // cannot guess around.
        out.extend(World::collisions(self, area).expect("collision shapes of BDS's blocks"));
    }

    fn loaded(&self, area: &Aabb) -> bool {
        World::loaded(self, area)
    }

    fn block(&self, pos: [i32; 3]) -> Motion {
        let Some(block) = self.block(pos) else {
            return Motion::AIR;
        };
        let kind = match block.identifier() {
            "minecraft:slime" => BlockKind::Slime,
            "minecraft:bed" => BlockKind::Bed,
            "minecraft:air" => BlockKind::Air,
            "minecraft:web" => BlockKind::Cobweb,
            "minecraft:powder_snow" => BlockKind::PowderSnow,
            _ => BlockKind::Plain,
        };
        Motion {
            friction: block.data.friction,
            kind,
            water: self.water_at(pos).unwrap_or(false),
        }
    }
}

/// Worlds for the client's own tests.
#[cfg(test)]
pub mod tests_support {
    use super::*;

    /// Stone from y -64 up to -61 (the floor's top at -60) in the chunks
    /// around the origin, air above.
    pub fn flat_stone_world() -> World {
        let stone = blocks::get("minecraft:stone").unwrap().min_state_id;
        let mut world = World::default();
        for cx in -2..=2 {
            for cz in -2..=2 {
                world.columns.insert([cx, cz], 1);
                let mut sub = SubChunk::air();
                for x in 0..16 {
                    for z in 0..16 {
                        for y in 0..4 {
                            sub.layers[0][index(x, y, z)] = stone;
                        }
                    }
                }
                world.sub_chunks.insert([cx, -4, cz], sub);
            }
        }
        world
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn varint(out: &mut Vec<u8>, value: i32) {
        let mut v = ((value << 1) ^ (value >> 31)) as u32;
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }

    fn hash(identifier: &str) -> u32 {
        blocks::get(identifier).unwrap().network_hashes[0]
    }

    /// A version 9 sub-chunk: stone below y 4, water at y 4, air above, in
    /// a 2-bit palette.
    fn sample() -> Vec<u8> {
        let mut out = vec![9, 1, 0, (2 << 1) | 1];
        let mut words = vec![0u32; 4096_usize.div_ceil(16)];
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..16 {
                    let entry = match y {
                        0..=3 => 1u32,
                        4 => 2,
                        _ => 0,
                    };
                    let slot = index(x, y, z);
                    words[slot / 16] |= entry << ((slot % 16) * 2);
                }
            }
        }
        for word in words {
            out.extend_from_slice(&word.to_le_bytes());
        }
        varint(&mut out, 3);
        for identifier in ["minecraft:air", "minecraft:stone", "minecraft:water"] {
            varint(&mut out, hash(identifier) as i32);
        }
        out
    }

    #[test]
    fn sub_chunks_decode_to_states_with_their_shapes() {
        let mut world = World::default();
        world.columns.insert([0, 0], 1);
        assert!(world.block([1, -60, 1]).is_none(), "not loaded yet");
        world
            .sub_chunks
            .insert([0, -4, 0], decode_sub_chunk(&sample()).unwrap());
        assert_eq!(
            world.block([3, -61, 7]).unwrap().identifier(),
            "minecraft:stone"
        );
        assert!(world.water_at([3, -60, 7]).unwrap());
        assert_eq!(
            world.block([3, -59, 7]).unwrap().identifier(),
            "minecraft:air"
        );
        // Above the announced count: air.
        assert_eq!(
            world.block([3, 40, 7]).unwrap().identifier(),
            "minecraft:air"
        );
        let feet = Aabb::from_feet([3.5, -61.0, 7.5], 0.6, 1.8);
        assert!(world.loaded(&feet));
        let boxes = world.collisions(&feet.expand([0.0, -0.1, 0.0])).unwrap();
        assert!(boxes.iter().any(|b| b.max[1] == -61.0), "{boxes:?}");
    }

    #[test]
    fn block_updates_reach_air_sub_chunks_above_the_announced_ones() {
        let mut world = World::default();
        world.columns.insert([0, 0], 1);
        world
            .set_block([2, 10, 2], hash("minecraft:slime"), 0)
            .unwrap();
        assert_eq!(
            world.block([2, 10, 2]).unwrap().identifier(),
            "minecraft:slime"
        );
        // Inside the announced sub-chunks: left to the SubChunk answer.
        world
            .set_block([2, -60, 2], hash("minecraft:slime"), 0)
            .unwrap();
        assert!(world.block([2, -60, 2]).is_none());
    }

    #[test]
    fn single_value_storages_have_no_palette_count() {
        let mut data = vec![9, 1, 0, 1];
        varint(&mut data, hash("minecraft:stone") as i32);
        let sub = decode_sub_chunk(&data).unwrap();
        let stone = blocks::get("minecraft:stone").unwrap().min_state_id;
        assert!(sub.layers[0].iter().all(|&s| s == stone));
    }
}
