//! Dump compact worldgen reference fixtures from Java Edition server worlds.
//!
//! Java counterpart of `bds_worldgen_reference`: the Java-style generator is
//! compared against these fixtures. Regenerate them once per Java version
//! (currently 1.21.11, official `server.jar` sha1
//! `64bb6d763bed0a9f1d632ec347938594144943ed`, run on JDK 21). Two worlds
//! with the same seed are generated outside the repo:
//!
//! 1. Decorated world: in an empty directory write `eula.txt` (`eula=true`)
//!    and `server.properties` with `level-seed=12345`, `level-name=world`,
//!    `server-ip=127.0.0.1`, `server-port=25599`, `enable-query=false`,
//!    `enable-rcon=false`, `online-mode=false`, `spawn-protection=0`,
//!    `view-distance=2`, `simulation-distance=2`, `generate-structures=true`,
//!    `sync-chunk-writes=true`, `max-players=1`, `enable-status=false`.
//!    Start `java -Xmx3G -jar server.jar nogui` with stdin piped and wait for
//!    `Done (`. Stop post-generation block changes with
//!    `gamerule random_tick_speed 0`, `gamerule spawn_mobs false`,
//!    `gamerule advance_weather false` and `weather clear`, then force-load
//!    the fixture areas (block coordinates): `forceload add -64 -64 63 63`,
//!    `forceload add 1024 1024 1087 1087`, `forceload add -2048 512 -1985 575`.
//!    Repeat `save-all flush` every 30 s until the region files stop
//!    changing, then `stop`.
//! 2. Undecorated world: same procedure in a second directory, except
//!    `generate-structures=false`, plus `initial-enabled-packs=vanilla,file/nofeatures`
//!    and `pause-when-empty-seconds=0`, a data pack created before the first
//!    start at `world/datapacks/nofeatures` (`pack.mcmeta` with
//!    `min_format`/`max_format` `[94, 1]`, and every
//!    `data/minecraft/worldgen/biome/*.json` of the server jar copied with
//!    `"features": []`; `carvers` and everything else untouched), and
//!    `tick freeze` sent before the `forceload` commands so no block, fluid
//!    or entity tick touches the generated chunks.
//! 3. `cargo run -p unastar --example java_worldgen_reference -- <decorated world>
//!    tests/fixtures/java_1_21_11_seed_12345.json <undecorated world>
//!    tests/fixtures/java_1_21_11_seed_12345_proto.json
//!    -4:-4:3:3 64:64:67:67 -128:32:-125:35`
//!
//! The main fixture has the same shape as the Bedrock one: per chunk column
//! (index `z * 16 + x`, as in the Bedrock fixture) the surface height
//! (highest block that is not `air`, `cave_air` or `void_air`), the terrain
//! height (highest block accepted by [`is_terrain_block`]), the surface block
//! name and the surface biome (the biome of the 4x4x4 cell containing the
//! surface block, as a Java biome identifier), plus a per-chunk histogram of
//! non-air block names and the saved `Heightmaps` (absolute Y = stored
//! value + `MIN_Y`, i.e. one above the highest matching block, `-64` for an
//! empty column; index `z * 16 + x`). Only `minecraft:full` chunks of the
//! requested rectangles are written.
//!
//! The proto fixture stores exact pre-decoration voxels (noise, surface rules
//! and carvers only) from the undecorated world: up to [`PROTO_CHUNKS`]
//! `minecraft:full` chunks inside the requested rectangles, split evenly
//! between them, nearest to each rectangle's centre first. Chunks saved at
//! `minecraft:carvers` in the decorated world are not usable for this: they
//! always border decorated chunks whose features (ores, cave vegetation,
//! seagrass) spill into them. Blocks are run-length encoded over the
//! 16x384x16 volume in (y, z, x) order as `[palette index, run length, ...]`
//! against a shared palette of block names (fluids keep `[level=N]` when
//! N != 0); biomes are the 4x96x4 cells in (y, z, x) order as indices into
//! `biome_palette`; heightmaps are encoded as in the main fixture.

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::Path;

use flate2::read::{GzDecoder, ZlibDecoder};

const MIN_Y: i32 = -64;
const SECTIONS: i32 = 24;
const HEIGHT: usize = (SECTIONS * 16) as usize;
const VOLUME: usize = 16 * 16 * HEIGHT;
const BIOME_CELLS: usize = 4 * 4 * HEIGHT / 4;
/// Keeps both fixtures together under 1.5 MB (one proto chunk is ~17 KB).
const PROTO_CHUNKS: usize = 18;
const FULL: &str = "minecraft:full";

/// Chunk statuses in generation order.
const STATUS_ORDER: [&str; 12] = [
    "minecraft:empty",
    "minecraft:structure_starts",
    "minecraft:structure_references",
    "minecraft:biomes",
    "minecraft:noise",
    "minecraft:surface",
    "minecraft:carvers",
    "minecraft:features",
    "minecraft:initialize_light",
    "minecraft:light",
    "minecraft:spawn",
    "minecraft:full",
];

type Rect = (i32, i32, i32, i32);

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 6 {
        eprintln!(
            "usage: java_worldgen_reference <world dir> <out.json> <undecorated world dir> <out_proto.json> <x0:z0:x1:z1>..."
        );
        std::process::exit(2);
    }
    let world = Path::new(&args[1]);
    let out = Path::new(&args[2]);
    let proto_world = Path::new(&args[3]);
    let out_proto = Path::new(&args[4]);
    let mut rects: Vec<Rect> = Vec::new();
    for spec in &args[5..] {
        let v: Vec<i32> = spec
            .split(':')
            .map(|p| p.parse().expect("region bounds must be integers"))
            .collect();
        assert_eq!(
            v.len(),
            4,
            "region must be x0:z0:x1:z1 (inclusive chunk coords)"
        );
        rects.push((v[0], v[1], v[2], v[3]));
    }

    let mut states = Interner::new();
    let air = states.id("minecraft:air");
    let mut biomes = Interner::new();

    // Main fixture: full chunks of the requested rectangles.
    let info = WorldInfo::load(world);
    let chunks = load_region_chunks(&world.join("region"));
    print_statuses(world, &info, &chunks);
    let mut palette = Interner::new();
    let mut chunks_json = Vec::new();
    let mut skipped = Vec::new();
    for &(x0, z0, x1, z1) in &rects {
        for cx in x0..=x1 {
            for cz in z0..=z1 {
                match chunks.get(&(cx, cz)) {
                    Some(nbt) if status_of(nbt) == FULL => {
                        let col = Column::decode(nbt, cx, cz, air, &mut states, &mut biomes);
                        chunks_json.push(col.to_json(cx, cz, &states, &biomes, &mut palette));
                    }
                    other => skipped.push((cx, cz, other.map_or("missing", status_of))),
                }
            }
        }
    }
    if !skipped.is_empty() {
        eprintln!(
            "{} requested chunks are not minecraft:full: {skipped:?}",
            skipped.len()
        );
    }
    let json = format!(
        "{{\n\"edition\": \"java\",\n\"version\": \"{}\",\n\"seed\": {},\n\"spawn\": [{}, {}],\n\"palette\": [{}],\n\"chunks\": [\n{}\n]\n}}\n",
        info.version,
        info.seed,
        info.spawn.0,
        info.spawn.1,
        quoted(&palette.names),
        chunks_json.join(",\n")
    );
    std::fs::write(out, json).expect("write fixture");
    println!("wrote {} chunks to {}", chunks_json.len(), out.display());

    // Proto fixture: full chunks of the undecorated world inside the rectangles.
    let proto_info = WorldInfo::load(proto_world);
    assert_eq!(
        (proto_info.seed, &proto_info.version),
        (info.seed, &info.version),
        "both worlds must share seed and version"
    );
    assert!(
        proto_info.datapacks.iter().any(|p| p.starts_with("file/")),
        "undecorated world has no feature-removing data pack enabled: {:?}",
        proto_info.datapacks
    );
    let proto_chunks = load_region_chunks(&proto_world.join("region"));
    print_statuses(proto_world, &proto_info, &proto_chunks);
    let picked = pick_proto_chunks(&proto_chunks, &rects);
    let mut proto_palette = Interner::new();
    let mut biome_palette = Interner::new();
    let mut proto_json = Vec::new();
    for &(cx, cz) in &picked {
        let col = Column::decode(
            &proto_chunks[&(cx, cz)],
            cx,
            cz,
            air,
            &mut states,
            &mut biomes,
        );
        proto_json.push(col.to_proto_json(
            cx,
            cz,
            &states,
            &biomes,
            &mut proto_palette,
            &mut biome_palette,
        ));
    }
    let areas: Vec<String> = rects
        .iter()
        .map(|(x0, z0, x1, z1)| format!("[{x0}, {z0}, {x1}, {z1}]"))
        .collect();
    let json = format!(
        "{{\n\"edition\": \"java\",\n\"version\": \"{}\",\n\"seed\": {},\n\"decoration\": \"none (datapack empties biome features)\",\n\"structures\": {},\n\"datapacks\": [{}],\n\"areas\": [{}],\n\"palette\": [{}],\n\"biome_palette\": [{}],\n\"chunks\": [\n{}\n]\n}}\n",
        proto_info.version,
        proto_info.seed,
        proto_info.structures,
        quoted(&proto_info.datapacks),
        areas.join(", "),
        quoted(&proto_palette.names),
        quoted(&biome_palette.names),
        proto_json.join(",\n")
    );
    std::fs::write(out_proto, json).expect("write proto fixture");
    println!(
        "wrote {} chunks to {}: {picked:?}",
        proto_json.len(),
        out_proto.display()
    );
}

/// World-level settings read from `level.dat`.
struct WorldInfo {
    seed: i64,
    version: String,
    spawn: (i32, i32),
    structures: bool,
    datapacks: Vec<String>,
}

impl WorldInfo {
    fn load(world: &Path) -> WorldInfo {
        let level = gunzip(&std::fs::read(world.join("level.dat")).expect("read level.dat"));
        let root = Nbt::read_root(&level);
        let data = root.get("Data").expect("level.dat without Data");
        let settings = data.get("WorldGenSettings");
        let seed = settings
            .and_then(|w| w.long("seed"))
            .or_else(|| data.long("RandomSeed"))
            .expect("level.dat without a world seed");
        let structures = settings
            .and_then(|w| w.byte("generate_features"))
            .expect("level.dat without WorldGenSettings.generate_features")
            != 0;
        let version = data
            .get("Version")
            .and_then(|v| v.string("Name"))
            .expect("level.dat without Version.Name");
        let spawn = match data.get("spawn").and_then(|s| s.get("pos")) {
            Some(Nbt::IntArray(pos)) => (pos[0], pos[2]),
            _ => (
                data.int("SpawnX").expect("SpawnX"),
                data.int("SpawnZ").expect("SpawnZ"),
            ),
        };
        let datapacks = data
            .get("DataPacks")
            .map(|d| d.list("Enabled"))
            .unwrap_or_default()
            .iter()
            .filter_map(|p| match p {
                Nbt::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect();
        WorldInfo {
            seed,
            version,
            spawn,
            structures,
            datapacks,
        }
    }
}

fn print_statuses(world: &Path, info: &WorldInfo, chunks: &BTreeMap<(i32, i32), Nbt>) {
    let mut statuses: BTreeMap<&str, usize> = BTreeMap::new();
    for nbt in chunks.values() {
        *statuses.entry(status_of(nbt)).or_default() += 1;
    }
    println!(
        "{}: {}, seed {}, spawn {:?}, structures {}, data packs {:?}, {} chunks saved",
        world.display(),
        info.version,
        info.seed,
        info.spawn,
        info.structures,
        info.datapacks,
        chunks.len()
    );
    for status in STATUS_ORDER {
        if let Some(n) = statuses.get(status) {
            println!("  {status}: {n}");
        }
    }
    for (status, n) in &statuses {
        if !STATUS_ORDER.contains(status) {
            println!("  {status} (unknown): {n}");
        }
    }
}

fn quoted(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("\"{n}\""))
        .collect::<Vec<_>>()
        .join(",")
}

fn status_of(nbt: &Nbt) -> &str {
    match nbt.get("Status") {
        Some(Nbt::Str(s)) => s,
        _ => "missing",
    }
}

/// Up to [`PROTO_CHUNKS`] `minecraft:full` chunks, split evenly between the
/// rectangles, nearest to each rectangle's centre first (Chebyshev distance,
/// then Manhattan, then coordinates).
fn pick_proto_chunks(chunks: &BTreeMap<(i32, i32), Nbt>, rects: &[Rect]) -> Vec<(i32, i32)> {
    let per_rect = PROTO_CHUNKS / rects.len().max(1);
    let mut picked: Vec<(i32, i32)> = Vec::new();
    for &(x0, z0, x1, z1) in rects {
        // Doubled coordinates keep the centre of even-sized rectangles integral.
        let mut candidates: Vec<(i32, i32, (i32, i32))> = Vec::new();
        for cx in x0..=x1 {
            for cz in z0..=z1 {
                let full = chunks.get(&(cx, cz)).is_some_and(|c| status_of(c) == FULL);
                if full && !picked.contains(&(cx, cz)) {
                    let dx = (2 * cx - x0 - x1).abs();
                    let dz = (2 * cz - z0 - z1).abs();
                    candidates.push((dx.max(dz), dx + dz, (cx, cz)));
                }
            }
        }
        candidates.sort_unstable();
        picked.extend(candidates.iter().take(per_rect).map(|c| c.2));
    }
    picked
}

/// Interned strings with stable indices in first-use order.
struct Interner {
    names: Vec<String>,
    index: HashMap<String, u16>,
}

impl Interner {
    fn new() -> Self {
        Self {
            names: Vec::new(),
            index: HashMap::new(),
        }
    }

    fn id(&mut self, name: &str) -> u16 {
        if let Some(&id) = self.index.get(name) {
            return id;
        }
        let id = u16::try_from(self.names.len()).expect("interner overflow");
        self.names.push(name.to_owned());
        self.index.insert(name.to_owned(), id);
        id
    }
}

/// Block name without the fluid `[level=N]` suffix.
fn bare(name: &str) -> &str {
    name.split('[').next().unwrap_or(name)
}

fn is_air(name: &str) -> bool {
    matches!(
        bare(name),
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

/// Decoded Java chunk column.
struct Column {
    status: String,
    /// State id per (y, z, x) over 16x384x16 (`Interner` of block names).
    blocks: Vec<u16>,
    /// Biome id per 4x4x4 cell in (y, z, x) order over 4x96x4.
    biomes: Vec<u16>,
    /// Saved heightmaps as absolute Y per column (index `z * 16 + x`).
    heightmaps: Vec<(String, Vec<i32>)>,
}

fn idx(x: usize, y: usize, z: usize) -> usize {
    (y * 16 + z) * 16 + x
}

impl Column {
    fn decode(
        nbt: &Nbt,
        cx: i32,
        cz: i32,
        air: u16,
        states: &mut Interner,
        biome_names: &mut Interner,
    ) -> Column {
        assert_eq!(
            (nbt.int("xPos"), nbt.int("zPos")),
            (Some(cx), Some(cz)),
            "chunk position mismatch"
        );
        let mut blocks = vec![air; VOLUME];
        let mut biomes = vec![u16::MAX; BIOME_CELLS];
        let sections = match nbt.get("sections") {
            Some(Nbt::List(s)) => s.as_slice(),
            _ => &[],
        };
        for section in sections {
            let sy = section.byte("Y").expect("section without Y") as i32 - (MIN_Y >> 4);
            if !(0..SECTIONS).contains(&sy) {
                continue;
            }
            let sy = sy as usize;
            if let Some(bs) = section.get("block_states") {
                let palette: Vec<u16> = bs
                    .list("palette")
                    .iter()
                    .map(|e| states.id(&state_name(e)))
                    .collect();
                let indices = unpack_palette(bs, palette.len(), 4096, 4);
                for (i, p) in indices.into_iter().enumerate() {
                    blocks[sy * 4096 + i] = palette[p as usize];
                }
            }
            if let Some(bm) = section.get("biomes") {
                let palette: Vec<u16> = bm
                    .list("palette")
                    .iter()
                    .map(|e| match e {
                        Nbt::Str(s) => biome_names.id(s),
                        _ => panic!("biome palette entry is not a string"),
                    })
                    .collect();
                let indices = unpack_palette(bm, palette.len(), 64, 0);
                for (i, p) in indices.into_iter().enumerate() {
                    biomes[sy * 64 + i] = palette[p as usize];
                }
            }
        }
        assert!(
            !biomes.contains(&u16::MAX),
            "chunk {cx},{cz} has sections without biomes"
        );
        let mut heightmaps = Vec::new();
        if let Some(Nbt::Compound(maps)) = nbt.get("Heightmaps") {
            for (name, value) in maps {
                let Nbt::LongArray(data) = value else {
                    panic!("heightmap {name} is not a long array");
                };
                let bits = ceil_log2(HEIGHT + 1);
                let heights = unpack(data, bits, 256)
                    .into_iter()
                    .map(|h| h as i32 + MIN_Y)
                    .collect();
                heightmaps.push((name.clone(), heights));
            }
        }
        heightmaps.sort_by(|a, b| a.0.cmp(&b.0));
        Column {
            status: status_of(nbt).to_owned(),
            blocks,
            biomes,
            heightmaps,
        }
    }

    fn heightmaps_json(&self) -> String {
        let maps: Vec<String> = self
            .heightmaps
            .iter()
            .map(|(name, h)| format!("\"{name}\": [{}]", join(h)))
            .collect();
        format!("{{{}}}", maps.join(", "))
    }

    fn to_json(
        &self,
        cx: i32,
        cz: i32,
        states: &Interner,
        biome_names: &Interner,
        palette: &mut Interner,
    ) -> String {
        let mut surface = Vec::with_capacity(256);
        let mut terrain = Vec::with_capacity(256);
        let mut surface_block = Vec::with_capacity(256);
        let mut surface_biome = Vec::with_capacity(256);
        for z in 0..16 {
            for x in 0..16 {
                let mut top = None;
                let mut top_terrain = None;
                for y in (0..HEIGHT).rev() {
                    let name = bare(&states.names[self.blocks[idx(x, y, z)] as usize]);
                    if is_air(name) {
                        continue;
                    }
                    if top.is_none() {
                        top = Some((y, name));
                    }
                    if is_terrain_block(name) {
                        top_terrain = Some(y);
                        break;
                    }
                }
                let (sy, sname) = top.unwrap_or((0, "minecraft:air"));
                surface.push(sy as i32 + MIN_Y);
                terrain.push(top_terrain.map_or(MIN_Y - 1, |y| y as i32 + MIN_Y));
                surface_block.push(i32::from(palette.id(sname)));
                let biome = self.biomes[((sy >> 2) * 4 + (z >> 2)) * 4 + (x >> 2)];
                surface_biome.push(format!("\"{}\"", biome_names.names[biome as usize]));
            }
        }
        let mut histogram: BTreeMap<&str, u32> = BTreeMap::new();
        for &b in &self.blocks {
            let name = bare(&states.names[b as usize]);
            if !is_air(name) {
                *histogram.entry(name).or_default() += 1;
            }
        }
        let hist: Vec<String> = histogram
            .iter()
            .map(|(k, v)| format!("\"{k}\": {v}"))
            .collect();
        format!(
            "{{\"x\": {cx}, \"z\": {cz}, \"status\": \"{}\", \"finalized\": {}, \"surface\": [{}], \"terrain\": [{}], \"surface_block\": [{}], \"surface_biome\": [{}], \"heightmaps\": {}, \"histogram\": {{{}}}}}",
            self.status,
            if self.status == FULL { 2 } else { 0 },
            join(&surface),
            join(&terrain),
            join(&surface_block),
            surface_biome.join(","),
            self.heightmaps_json(),
            hist.join(", ")
        )
    }

    fn to_proto_json(
        &self,
        cx: i32,
        cz: i32,
        states: &Interner,
        biome_names: &Interner,
        palette: &mut Interner,
        biome_palette: &mut Interner,
    ) -> String {
        let mut runs: Vec<String> = Vec::new();
        let mut start = 0;
        while start < self.blocks.len() {
            let id = self.blocks[start];
            let len = self.blocks[start..]
                .iter()
                .take_while(|&&b| b == id)
                .count();
            let p = palette.id(&states.names[id as usize]);
            runs.push(format!("{p},{len}"));
            start += len;
        }
        let biomes: Vec<String> = self
            .biomes
            .iter()
            .map(|&b| biome_palette.id(&biome_names.names[b as usize]).to_string())
            .collect();
        format!(
            "{{\"x\": {cx}, \"z\": {cz}, \"status\": \"{}\", \"heightmaps\": {}, \"blocks\": [{}], \"biomes\": [{}]}}",
            self.status,
            self.heightmaps_json(),
            runs.join(","),
            biomes.join(",")
        )
    }
}

fn join(v: &[i32]) -> String {
    v.iter().map(i32::to_string).collect::<Vec<_>>().join(",")
}

/// Block name of a palette entry; fluids keep a non-zero `level` property.
fn state_name(entry: &Nbt) -> String {
    let name = entry
        .string("Name")
        .expect("block palette entry without Name");
    if matches!(name.as_str(), "minecraft:water" | "minecraft:lava")
        && let Some(level) = entry.get("Properties").and_then(|p| p.string("level"))
        && level != "0"
    {
        return format!("{name}[level={level}]");
    }
    name
}

/// Blocks placed by the noise router, surface rules and ore veins; decoration
/// (plants, trees, snow layers, disks) and fluids are excluded so the terrain
/// height compares terrain shape only. Same list as the Bedrock tool; the
/// Bedrock full snow block `snow` is named `snow_block` in Java (Java `snow`
/// is the snow layer, which is decoration).
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
            | "snow_block"
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

fn ceil_log2(n: usize) -> usize {
    if n <= 1 {
        0
    } else {
        (usize::BITS - (n - 1).leading_zeros()) as usize
    }
}

/// Unpack `count` values of `bits` each; values never span two longs.
fn unpack(data: &[i64], bits: usize, count: usize) -> Vec<u32> {
    let per_long = 64 / bits;
    assert_eq!(
        data.len(),
        count.div_ceil(per_long),
        "packed array length does not match {bits} bits x {count} entries"
    );
    let mask = (1u64 << bits) - 1;
    (0..count)
        .map(|i| ((data[i / per_long] as u64 >> ((i % per_long) * bits)) & mask) as u32)
        .collect()
}

/// Palette indices of a paletted container (`palette` + optional `data`).
fn unpack_palette(container: &Nbt, palette_len: usize, count: usize, min_bits: usize) -> Vec<u32> {
    assert!(palette_len > 0, "empty palette");
    if palette_len == 1 {
        return vec![0; count];
    }
    let Some(Nbt::LongArray(data)) = container.get("data") else {
        panic!("paletted container with {palette_len} entries but no data");
    };
    let indices = unpack(data, ceil_log2(palette_len).max(min_bits), count);
    assert!(
        indices.iter().all(|&i| (i as usize) < palette_len),
        "palette index out of range"
    );
    indices
}

fn gunzip(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    GzDecoder::new(data)
        .read_to_end(&mut out)
        .expect("gzip decode");
    out
}

/// Every chunk stored in the `r.X.Z.mca` files of `dir`, keyed by chunk coords.
fn load_region_chunks(dir: &Path) -> BTreeMap<(i32, i32), Nbt> {
    let mut chunks = BTreeMap::new();
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .expect("read region dir")
        .map(|e| e.expect("region dir entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "mca"))
        .collect();
    files.sort();
    for path in files {
        let buf = std::fs::read(&path).expect("read region file");
        if buf.len() < 8192 {
            continue;
        }
        for slot in 0..1024 {
            let loc = u32::from_be_bytes(buf[slot * 4..slot * 4 + 4].try_into().unwrap());
            let sector = (loc >> 8) as usize;
            if sector == 0 {
                continue;
            }
            let off = sector * 4096;
            let len = u32::from_be_bytes(buf[off..off + 4].try_into().unwrap()) as usize;
            let compression = buf[off + 4];
            let payload = &buf[off + 5..off + 4 + len];
            let raw = match compression {
                1 => gunzip(payload),
                2 => {
                    let mut out = Vec::new();
                    ZlibDecoder::new(payload)
                        .read_to_end(&mut out)
                        .expect("zlib decode");
                    out
                }
                3 => payload.to_vec(),
                c => panic!(
                    "{}: chunk slot {slot} uses unsupported compression {c} (only gzip, zlib, none)",
                    path.display()
                ),
            };
            let nbt = Nbt::read_root(&raw);
            let pos = (
                nbt.int("xPos").expect("chunk without xPos"),
                nbt.int("zPos").expect("chunk without zPos"),
            );
            chunks.insert(pos, nbt);
        }
    }
    chunks
}

/// Minimal big-endian NBT tree: enough for level.dat and chunk data.
enum Nbt {
    Byte(i8),
    Int(i32),
    Long(i64),
    Str(String),
    List(Vec<Nbt>),
    Compound(Vec<(String, Nbt)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
    Other,
}

impl Nbt {
    fn read_root(buf: &[u8]) -> Nbt {
        let mut r = Reader { buf, off: 1 };
        let _ = r.string();
        r.payload(buf[0])
    }

    fn get(&self, key: &str) -> Option<&Nbt> {
        match self {
            Nbt::Compound(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn list(&self, key: &str) -> &[Nbt] {
        match self.get(key) {
            Some(Nbt::List(v)) => v,
            _ => &[],
        }
    }

    fn string(&self, key: &str) -> Option<String> {
        match self.get(key)? {
            Nbt::Str(s) => Some(s.clone()),
            _ => None,
        }
    }

    fn byte(&self, key: &str) -> Option<i8> {
        match self.get(key)? {
            Nbt::Byte(v) => Some(*v),
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

struct Reader<'a> {
    buf: &'a [u8],
    off: usize,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> [u8; N] {
        let v = self.buf[self.off..self.off + N].try_into().unwrap();
        self.off += N;
        v
    }

    fn count(&mut self) -> usize {
        i32::from_be_bytes(self.take()).max(0) as usize
    }

    fn string(&mut self) -> String {
        let n = u16::from_be_bytes(self.take()) as usize;
        let s = String::from_utf8_lossy(&self.buf[self.off..self.off + n]).into_owned();
        self.off += n;
        s
    }

    fn payload(&mut self, tag: u8) -> Nbt {
        match tag {
            1 => Nbt::Byte(i8::from_be_bytes(self.take())),
            2 => {
                self.off += 2;
                Nbt::Other
            }
            3 => Nbt::Int(i32::from_be_bytes(self.take())),
            4 => Nbt::Long(i64::from_be_bytes(self.take())),
            5 => {
                self.off += 4;
                Nbt::Other
            }
            6 => {
                self.off += 8;
                Nbt::Other
            }
            7 => {
                let n = self.count();
                self.off += n;
                Nbt::Other
            }
            8 => Nbt::Str(self.string()),
            9 => {
                let [et] = self.take();
                let n = self.count();
                Nbt::List((0..n).map(|_| self.payload(et)).collect())
            }
            10 => {
                let mut entries = Vec::new();
                loop {
                    let [t] = self.take();
                    if t == 0 {
                        return Nbt::Compound(entries);
                    }
                    let name = self.string();
                    let v = self.payload(t);
                    entries.push((name, v));
                }
            }
            11 => {
                let n = self.count();
                Nbt::IntArray((0..n).map(|_| i32::from_be_bytes(self.take())).collect())
            }
            12 => {
                let n = self.count();
                Nbt::LongArray((0..n).map(|_| i64::from_be_bytes(self.take())).collect())
            }
            t => panic!("unknown NBT tag {t}"),
        }
    }
}
