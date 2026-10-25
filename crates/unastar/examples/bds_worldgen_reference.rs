//! Dump a compact worldgen reference fixture from a BDS-generated world.
//!
//! The parity harness (`tests/worldgen_parity.rs`) compares unastar's vanilla
//! generator against this fixture. Regenerate it once per Bedrock version:
//!
//! 1. Start the shared BDS runtime with `level-type=DEFAULT`, the fixture seed
//!    and a fresh `level-name`.
//! 2. Force generation from the server console with preloaded ticking areas,
//!    e.g. `tickingarea add -64 0 -64 63 0 63 origin true`, wait, then `stop`.
//! 3. `cargo run -p unastar --example bds_worldgen_reference -- <world dir> <out.json> <x0:z0:x1:z1>...`
//!    Add `--geology` before the world directory to export per-y block counts
//!    from -64 through 16 instead, for measuring base-layer transitions.
//!
//! The fixture stores, per chunk column, the surface height (highest non-air
//! block), the terrain height (highest block produced by noise, surface rules
//! and ore veins, see [`is_terrain_block`]), the surface block name, and the
//! surface biome id, plus a per-chunk block-name histogram. This keeps the file
//! small while still separating terrain shape from decoration.

use std::collections::BTreeMap;
use std::path::Path;

use bleveldb::{DB, Options, ReadOptions};

const MIN_Y: i32 = -64;
const SUBCHUNKS: i32 = 24;

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    let geology = args.get(1).is_some_and(|arg| arg == "--geology");
    if geology {
        args.remove(1);
    }
    if args.len() < 4 {
        eprintln!(
            "usage: bds_worldgen_reference [--geology] <world dir> <out.json> <x0:z0:x1:z1>..."
        );
        std::process::exit(2);
    }
    let world = Path::new(&args[1]);
    let out = Path::new(&args[2]);
    let mut regions = Vec::new();
    for spec in &args[3..] {
        let v: Vec<i32> = spec
            .split(':')
            .map(|p| p.parse().expect("region bounds must be integers"))
            .collect();
        assert_eq!(
            v.len(),
            4,
            "region must be x0:z0:x1:z1 (inclusive chunk coords)"
        );
        regions.push((v[0], v[1], v[2], v[3]));
    }

    let level = std::fs::read(world.join("level.dat")).expect("read level.dat");
    let level_nbt = Nbt::read_root(&level[8..]);
    let seed = level_nbt.long("RandomSeed").expect("RandomSeed");
    let spawn = (
        level_nbt.int("SpawnX").expect("SpawnX"),
        level_nbt.int("SpawnZ").expect("SpawnZ"),
    );

    let options = Options::new();
    options.create_if_missing(false);
    let db = DB::open(world.join("db"), &options).expect("open BDS db");
    let read = ReadOptions::new();

    let mut palette: Vec<String> = Vec::new();
    let mut palette_index = BTreeMap::new();
    let mut chunks_json = Vec::new();
    let mut missing = Vec::new();
    for (x0, z0, x1, z1) in regions {
        for cx in x0..=x1 {
            for cz in z0..=z1 {
                match load_column(&db, &read, cx, cz) {
                    Some(col) => chunks_json.push(if geology {
                        col.geology_json(cx, cz)
                    } else {
                        col.to_json(cx, cz, &mut palette, &mut palette_index)
                    }),
                    None => missing.push((cx, cz)),
                }
            }
        }
    }
    if !missing.is_empty() {
        eprintln!(
            "{} requested chunks are not in the BDS world: {missing:?}",
            missing.len()
        );
    }

    let palette_json: Vec<String> = palette.iter().map(|n| format!("\"{n}\"")).collect();
    let json = format!(
        "{{\n\"seed\": {seed},\n\"spawn\": [{}, {}],\n\"palette\": [{}],\n\"chunks\": [\n{}\n]\n}}\n",
        spawn.0,
        spawn.1,
        palette_json.join(","),
        chunks_json.join(",\n")
    );
    std::fs::write(out, json).expect("write fixture");
    println!("wrote {} chunks to {}", chunks_json.len(), out.display());
}

/// Decoded BDS chunk column: block names per position plus surface biomes.
struct Column {
    /// Block name index per (x, y, z), 16x384x16, `None` = air.
    blocks: Vec<Option<String>>,
    /// Biome id per (x, y, z) at block resolution from Data3D.
    biomes: Vec<u32>,
    /// Finalisation state (2 = fully populated).
    finalized: u32,
}

fn idx(x: usize, y: usize, z: usize) -> usize {
    (y * 16 + z) * 16 + x
}

impl Column {
    fn geology_json(&self, cx: i32, cz: i32) -> String {
        let layers: Vec<_> = (-64..=16)
            .map(|y| {
                let mut counts: BTreeMap<&str, u32> = BTreeMap::new();
                for z in 0..16 {
                    for x in 0..16 {
                        let name = self.blocks[idx(x, (y - MIN_Y) as usize, z)]
                            .as_deref()
                            .unwrap_or("minecraft:air");
                        *counts.entry(name).or_default() += 1;
                    }
                }
                serde_json::json!({ "y": y, "counts": counts })
            })
            .collect();
        let bedrock_masks: Vec<u8> = (0..256)
            .map(|i| {
                let (x, z) = (i % 16, i / 16);
                let mut mask = 0;
                for y in 0..6 {
                    if self.blocks[idx(x, y, z)].as_deref() == Some("minecraft:bedrock") {
                        mask |= 1 << y;
                    }
                }
                mask
            })
            .collect();
        serde_json::json!({ "x": cx, "z": cz, "layers": layers, "bedrock_masks": bedrock_masks })
            .to_string()
    }

    fn to_json(
        &self,
        cx: i32,
        cz: i32,
        palette: &mut Vec<String>,
        palette_index: &mut BTreeMap<String, usize>,
    ) -> String {
        let height = (SUBCHUNKS * 16) as usize;
        let mut surface = Vec::with_capacity(256);
        let mut terrain = Vec::with_capacity(256);
        let mut surface_block = Vec::with_capacity(256);
        let mut surface_biome = Vec::with_capacity(256);
        let mut histogram: BTreeMap<&str, u32> = BTreeMap::new();
        for z in 0..16 {
            for x in 0..16 {
                let mut top = None;
                let mut top_terrain = None;
                for y in (0..height).rev() {
                    if let Some(name) = &self.blocks[idx(x, y, z)] {
                        if top.is_none() {
                            top = Some((y, name.clone()));
                        }
                        if is_terrain_block(name) {
                            top_terrain = Some(y);
                            break;
                        }
                    }
                }
                let (sy, sname) = top.unwrap_or((0, "minecraft:air".into()));
                surface.push(sy as i32 + MIN_Y);
                terrain.push(top_terrain.map_or(MIN_Y - 1, |y| y as i32 + MIN_Y));
                let next = palette.len();
                let pi = *palette_index.entry(sname.clone()).or_insert_with(|| {
                    palette.push(sname.clone());
                    next
                });
                surface_block.push(pi);
                surface_biome.push(self.biomes[idx(x, sy, z)]);
            }
        }
        for b in self.blocks.iter().flatten() {
            *histogram.entry(b.as_str()).or_default() += 1;
        }
        let hist: Vec<String> = histogram
            .iter()
            .map(|(k, v)| format!("\"{k}\": {v}"))
            .collect();
        let join = |v: &[i32]| {
            v.iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(",")
        };
        let joinu = |v: &[usize]| {
            v.iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(",")
        };
        let biomes: Vec<String> = surface_biome.iter().map(u32::to_string).collect();
        format!(
            "{{\"x\": {cx}, \"z\": {cz}, \"finalized\": {}, \"surface\": [{}], \"terrain\": [{}], \"surface_block\": [{}], \"surface_biome\": [{}], \"histogram\": {{{}}}}}",
            self.finalized,
            join(&surface),
            join(&terrain),
            joinu(&surface_block),
            biomes.join(","),
            hist.join(", ")
        )
    }
}

/// Blocks placed by the noise router, surface rules and ore veins; decoration
/// (plants, trees, snow layers, disks) and fluids are excluded so the terrain
/// height compares terrain shape only.
fn is_terrain_block(name: &str) -> bool {
    let n = name.trim_start_matches("minecraft:");
    matches!(
        n,
        "stone"
            | "deepslate"
            | "dirt"
            | "coarse_dirt"
            | "grass_block"
            | "podzol"
            | "mycelium"
            | "sand"
            | "red_sand"
            | "sandstone"
            | "red_sandstone"
            | "gravel"
            | "terracotta"
            | "snow"
            | "ice"
            | "packed_ice"
            | "powder_snow"
            | "calcite"
            | "mud"
            | "bedrock"
            | "tuff"
            | "granite"
            | "raw_iron_block"
            | "raw_copper_block"
            | "iron_ore"
            | "copper_ore"
            | "deepslate_iron_ore"
            | "deepslate_copper_ore"
    ) || n.ends_with("_terracotta")
}

fn chunk_key(cx: i32, cz: i32, tag: u8, sub: Option<i8>) -> Vec<u8> {
    let mut k = Vec::with_capacity(10);
    k.extend_from_slice(&cx.to_le_bytes());
    k.extend_from_slice(&cz.to_le_bytes());
    k.push(tag);
    if let Some(y) = sub {
        k.push(y as u8);
    }
    k
}

fn load_column(db: &DB, read: &ReadOptions, cx: i32, cz: i32) -> Option<Column> {
    db.get(&chunk_key(cx, cz, b',', None), read)
        .unwrap_or_else(|e| panic!("read version of chunk {cx},{cz}: {e}"))?;
    let finalized = db
        .get(&chunk_key(cx, cz, b'6', None), read)
        .ok()
        .flatten()
        .map_or(0, |v| u32::from_le_bytes(v[..4].try_into().unwrap()));
    let height = (SUBCHUNKS * 16) as usize;
    let mut blocks = vec![None; 16 * 16 * height];
    for sy in 0..SUBCHUNKS {
        let y_index = (sy + (MIN_Y >> 4)) as i8;
        let Some(data) = db
            .get(&chunk_key(cx, cz, b'/', Some(y_index)), read)
            .expect("read subchunk")
        else {
            continue;
        };
        let names = decode_block_subchunk(&data);
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..16 {
                    let name = &names[(x << 8) | (z << 4) | y];
                    if name != "minecraft:air" {
                        blocks[idx(x, sy as usize * 16 + y, z)] = Some(name.clone());
                    }
                }
            }
        }
    }
    let data3d = db
        .get(&chunk_key(cx, cz, b'+', None), read)
        .expect("read Data3D")
        .expect("BDS chunk without Data3D biomes");
    let biomes = decode_biomes(&data3d[512..], height);
    Some(Column {
        blocks,
        biomes,
        finalized,
    })
}

/// Decode a persistent (disk) sub-chunk, layer 0 only, into block names in XZY order.
fn decode_block_subchunk(data: &[u8]) -> Vec<String> {
    let version = data[0];
    let mut off = match version {
        8 => 2,
        9 => 3,
        v => panic!("unsupported sub-chunk version {v}"),
    };
    let (indices, palette_len) = read_indices(data, &mut off);
    let mut names = Vec::with_capacity(palette_len);
    for _ in 0..palette_len {
        let (nbt, used) = Nbt::read_root_len(&data[off..]);
        off += used;
        names.push(nbt.string("name").expect("palette entry without name"));
    }
    indices
        .into_iter()
        .map(|i| names[i as usize].clone())
        .collect()
}

/// Read a paletted storage header + index words; returns indices and palette length.
fn read_indices(data: &[u8], off: &mut usize) -> (Vec<u32>, usize) {
    let header = data[*off];
    *off += 1;
    let bits = (header >> 1) as usize;
    if bits == 0 {
        return (vec![0; 4096], 1);
    }
    let per_word = 32 / bits;
    let words = 4096_usize.div_ceil(per_word);
    let mut indices = Vec::with_capacity(4096);
    'outer: for w in 0..words {
        let word = u32::from_le_bytes(data[*off + w * 4..*off + w * 4 + 4].try_into().unwrap());
        for i in 0..per_word {
            if indices.len() == 4096 {
                break 'outer;
            }
            indices.push((word >> (i * bits)) & ((1u32 << bits) - 1));
        }
    }
    *off += words * 4;
    let len = i32::from_le_bytes(data[*off..*off + 4].try_into().unwrap()) as usize;
    *off += 4;
    (indices, len)
}

/// Decode Data3D biome storages (after the 512-byte height map) to block resolution.
fn decode_biomes(data: &[u8], height: usize) -> Vec<u32> {
    let mut out = vec![0u32; 16 * 16 * height];
    let mut off = 0;
    let mut last: Vec<u32> = vec![0; 4096];
    for sy in 0..height / 16 {
        let header = data[off];
        if header == 0xff {
            off += 1;
        } else if header >> 1 == 0 {
            off += 1;
            let v = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
            off += 4;
            last = vec![v; 4096];
        } else {
            let (indices, len) = read_indices(data, &mut off);
            let mut pal = Vec::with_capacity(len);
            for _ in 0..len {
                pal.push(u32::from_le_bytes(data[off..off + 4].try_into().unwrap()));
                off += 4;
            }
            last = indices.into_iter().map(|i| pal[i as usize]).collect();
        }
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..16 {
                    out[idx(x, sy * 16 + y, z)] = last[(x << 8) | (z << 4) | y];
                }
            }
        }
    }
    out
}

/// Minimal little-endian NBT reader: enough for level.dat and block palettes.
enum Nbt {
    Int(i32),
    Long(i64),
    Str(String),
    Compound(Vec<(String, Nbt)>),
    Other,
}

impl Nbt {
    fn read_root(buf: &[u8]) -> Nbt {
        Self::read_root_len(buf).0
    }

    fn read_root_len(buf: &[u8]) -> (Nbt, usize) {
        let mut off = 1;
        let _ = read_str(buf, &mut off);
        let v = read_payload(buf, buf[0], &mut off);
        (v, off)
    }

    fn get(&self, key: &str) -> Option<&Nbt> {
        match self {
            Nbt::Compound(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn string(&self, key: &str) -> Option<String> {
        match self.get(key)? {
            Nbt::Str(s) => Some(s.clone()),
            _ => None,
        }
    }

    fn int(&self, key: &str) -> Option<i32> {
        match self.get(key)? {
            Nbt::Int(v) => Some(*v),
            _ => None,
        }
    }

    fn long(&self, key: &str) -> Option<i64> {
        match self.get(key)? {
            Nbt::Long(v) => Some(*v),
            _ => None,
        }
    }
}

fn read_str(buf: &[u8], off: &mut usize) -> String {
    let n = u16::from_le_bytes([buf[*off], buf[*off + 1]]) as usize;
    let s = String::from_utf8_lossy(&buf[*off + 2..*off + 2 + n]).into_owned();
    *off += 2 + n;
    s
}

fn read_i32(buf: &[u8], off: &mut usize) -> i32 {
    let v = i32::from_le_bytes(buf[*off..*off + 4].try_into().unwrap());
    *off += 4;
    v
}

fn read_payload(buf: &[u8], tag: u8, off: &mut usize) -> Nbt {
    match tag {
        1 => {
            *off += 1;
            Nbt::Other
        }
        2 => {
            *off += 2;
            Nbt::Other
        }
        3 => Nbt::Int(read_i32(buf, off)),
        4 => {
            let v = i64::from_le_bytes(buf[*off..*off + 8].try_into().unwrap());
            *off += 8;
            Nbt::Long(v)
        }
        5 => {
            *off += 4;
            Nbt::Other
        }
        6 => {
            *off += 8;
            Nbt::Other
        }
        7 => {
            let n = read_i32(buf, off) as usize;
            *off += n;
            Nbt::Other
        }
        8 => Nbt::Str(read_str(buf, off)),
        9 => {
            let et = buf[*off];
            *off += 1;
            let n = read_i32(buf, off);
            for _ in 0..n.max(0) {
                read_payload(buf, et, off);
            }
            Nbt::Other
        }
        10 => {
            let mut entries = Vec::new();
            loop {
                let t = buf[*off];
                *off += 1;
                if t == 0 {
                    return Nbt::Compound(entries);
                }
                let name = read_str(buf, off);
                let v = read_payload(buf, t, off);
                entries.push((name, v));
            }
        }
        11 => {
            let n = read_i32(buf, off) as usize;
            *off += 4 * n;
            Nbt::Other
        }
        12 => {
            let n = read_i32(buf, off) as usize;
            *off += 8 * n;
            Nbt::Other
        }
        t => panic!("unknown NBT tag {t}"),
    }
}
