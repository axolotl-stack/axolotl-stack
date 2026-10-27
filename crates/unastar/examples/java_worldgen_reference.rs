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
//! against a shared palette of canonical Java states. Every property is retained,
//! including default fluid levels and waterlogging; property keys are sorted.
//! Biomes are the 4x96x4 cells in (y, z, x) order as indices into
//! `biome_palette`; heightmaps are encoded as in the main fixture.

mod java_canonical_inputs;
#[path = "java_canonical_inputs/capture.rs"]
mod java_canonical_capture;

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::Path;

use flate2::read::{GzDecoder, ZlibDecoder};

const MIN_Y: i32 = -64;
const SECTIONS: i32 = 24;
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
    if args.get(1).is_some_and(|arg| arg == "--canonical") {
        export_canonical(&args[2..]);
        return;
    }
    if args.get(1).is_some_and(|arg| arg == "--inventory") {
        inventory(&args[2..]);
        return;
    }
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
                        let col = Column::decode(nbt, cx, cz, air, &mut states, &mut biomes, Layout::OVERWORLD);
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
            Layout::OVERWORLD,
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
        "{{\n\"edition\": \"java\",\n\"state_encoding\": \"java-canonical-properties-v1\",\n\"dimension\": \"minecraft:overworld\",\n\"min_y\": -64,\n\"height\": 384,\n\"version\": \"{}\",\n\"seed\": {},\n\"decoration\": \"none (datapack empties biome features)\",\n\"structures\": {},\n\"datapacks\": [{}],\n\"areas\": [{}],\n\"palette\": [{}],\n\"biome_palette\": [{}],\n\"chunks\": [\n{}\n]\n}}\n",
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

/// Storage bounds, not the generation depth (the Nether also saves above its roof).
#[derive(Clone, Copy)]
struct Layout {
    min_y: i32,
    sections: i32,
}

impl Layout {
    const OVERWORLD: Self = Self { min_y: MIN_Y, sections: SECTIONS };

    fn height(self) -> usize {
        self.sections as usize * 16
    }
}

/// Export every requested voxel at the saved status; never sample or silently skip.
fn export_canonical(args: &[String]) {
    assert!(args.len() >= 6,
        "usage: --canonical <world> <out.json> <dimension> <status> <capture-description> <x0:z0:x1:z1>...");
    let world = Path::new(&args[0]);
    let info = WorldInfo::load(world);
    assert_eq!(info.version, "1.21.11", "reference version must remain pinned");
    let (folder, layout) = dimension_layout(&args[2]);
    assert!(STATUS_ORDER.contains(&args[3].as_str()), "unknown saved generation status");
    assert!(!args[4].trim().is_empty(), "capture description must declare tick/datapack controls");
    let chunks = load_region_chunks(&world.join(folder));
    let mut states = Interner::new();
    let air = states.id("minecraft:air");
    let mut biomes = Interner::new();
    let mut palette = Interner::new();
    let mut biome_palette = Interner::new();
    let mut selected = BTreeMap::new();
    for rect in &args[5..] {
        let bounds: Vec<i32> = rect.split(':').map(|v| v.parse().expect("integer bounds")).collect();
        assert_eq!(bounds.len(), 4, "expected x0:z0:x1:z1");
        let [x0, z0, x1, z1] = bounds[..] else { unreachable!() };
        assert!(x0 <= x1 && z0 <= z1, "reversed chunk bounds");
        for x in x0..=x1 {
            for z in z0..=z1 {
                let nbt = chunks.get(&(x, z)).unwrap_or_else(|| panic!("missing requested chunk {x},{z}"));
                assert_eq!(status_of(nbt), args[3], "chunk {x},{z} status mismatch");
                selected.insert((x, z), nbt);
            }
        }
    }
    let columns: Vec<_> = selected.into_iter().map(|((x, z), nbt)| {
        Column::decode(nbt, x, z, air, &mut states, &mut biomes, layout)
            .to_proto_json(x, z, &states, &biomes, &mut palette, &mut biome_palette)
    }).collect();
    let inputs = java_canonical_inputs::Inputs {
        worldgen_settings: info.worldgen_settings,
        enabled_features: info.enabled_features,
        data_profile: java_canonical_capture::inspect_profile(world, &info.datapacks)
            .expect("fingerprint actual native datapack inputs"),
    };
    let text = format!(
        "{{\"edition\":\"java\",\"state_encoding\":\"java-canonical-properties-v1\",\"version\":\"{}\",\"seed\":{},\"dimension\":{},\"min_y\":{},\"height\":{},\"capture\":{},\"structures\":{},\"datapacks\":[{}],\"inputs\":{},\"palette\":[{}],\"biome_palette\":[{}],\"chunks\":[{}]}}\n",
        info.version, info.seed, serde_json::to_string(&args[2]).unwrap(), layout.min_y,
        layout.height(), serde_json::to_string(&args[4]).unwrap(), info.structures,
        quoted(&info.datapacks), serde_json::to_string(&inputs).unwrap(),
        quoted(&palette.names), quoted(&biome_palette.names), columns.join(",")
    );
    std::fs::write(&args[1], text).expect("write canonical fixture");
    println!("wrote {} complete {} chunks at {} to {}", columns.len(), args[2], args[3], args[1]);
}

fn dimension_layout(dimension: &str) -> (&'static str, Layout) {
    match dimension {
        "minecraft:overworld" => ("region", Layout::OVERWORLD),
        "minecraft:the_nether" => ("DIM-1/region", Layout { min_y: 0, sections: 16 }),
        "minecraft:the_end" => ("DIM1/region", Layout { min_y: 0, sections: 16 }),
        other => panic!("unsupported reference dimension {other}"),
    }
}

/// Locate saved stage snapshots without guessing coordinates or changing a world.
fn inventory(args: &[String]) {
    assert!((2..=3).contains(&args.len()),
        "usage: --inventory <world> <dimension> [saved-status]");
    let world = Path::new(&args[0]);
    let info = WorldInfo::load(world);
    assert_eq!(info.version, "1.21.11", "reference version must remain pinned");
    let (folder, _) = dimension_layout(&args[1]);
    let chunks = load_region_chunks(&world.join(folder));
    let rows: Vec<_> = chunks.iter()
        .filter(|(_, nbt)| args.get(2).is_none_or(|status| status_of(nbt) == status))
        .map(|(&(x, z), nbt)| serde_json::json!({"x":x,"z":z,"status":status_of(nbt)}))
        .collect();
    println!("{}", serde_json::json!({
        "edition":"java", "version":info.version, "seed":info.seed,
        "dimension":args[1], "structures":info.structures, "datapacks":info.datapacks,
        "chunks":rows,
    }));
}

/// World-level settings read from `level.dat`.
struct WorldInfo {
    seed: i64,
    version: String,
    spawn: (i32, i32),
    structures: bool,
    datapacks: Vec<String>,
    worldgen_settings: serde_json::Value,
    enabled_features: Vec<String>,
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
            worldgen_settings: serde_json::to_value(settings.expect("worldgen settings")).expect("typed native inputs"),
            enabled_features: data.list("enabled_features").iter().map(|value| match value {
                Nbt::Str(value) => value.clone(),
                _ => panic!("non-string native feature flag"),
            }).collect(),
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

/// Block name without its canonical property suffix.
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
    /// Canonical Java state id per (y, z, x) over 16x384x16.
    blocks: Vec<u16>,
    /// Biome id per 4x4x4 cell in (y, z, x) order over 4x96x4.
    biomes: Vec<u16>,
    /// Saved heightmaps as absolute Y per column (index `z * 16 + x`).
    heightmaps: Vec<(String, Vec<i32>)>,
    layout: Layout,
    block_entities: serde_json::Value,
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
        layout: Layout,
    ) -> Column {
        assert_eq!(
            (nbt.int("xPos"), nbt.int("zPos")),
            (Some(cx), Some(cz)),
            "chunk position mismatch"
        );
        let mut blocks = vec![air; 16 * 16 * layout.height()];
        let mut biomes = vec![u16::MAX; 4 * 4 * layout.height() / 4];
        let sections = match nbt.get("sections") {
            Some(Nbt::List(_, s)) => s.as_slice(),
            _ => &[],
        };
        for section in sections {
            let sy = section.byte("Y").expect("section without Y") as i32 - (layout.min_y >> 4);
            if !(0..layout.sections).contains(&sy) {
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
                let bits = ceil_log2(layout.height() + 1);
                let heights = unpack(data, bits, 256)
                    .into_iter()
                    .map(|h| h as i32 + layout.min_y)
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
            layout,
            block_entities: {
                let mut entities: Vec<_> = nbt.list("block_entities").iter().collect();
                entities.sort_by_key(|entity| (entity.int("x"), entity.int("y"), entity.int("z")));
                serde_json::to_value(entities).expect("serialize typed Java block entities")
            },
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
                for y in (0..self.layout.height()).rev() {
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
                surface.push(sy as i32 + self.layout.min_y);
                terrain.push(top_terrain.map_or(self.layout.min_y - 1, |y| y as i32 + self.layout.min_y));
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
            "{{\"x\": {cx}, \"z\": {cz}, \"status\": \"{}\", \"heightmaps\": {}, \"blocks\": [{}], \"biomes\": [{}], \"block_entities\": {}}}",
            self.status,
            self.heightmaps_json(),
            runs.join(","),
            biomes.join(","),
            self.block_entities,
        )
    }
}

fn join(v: &[i32]) -> String {
    v.iter().map(i32::to_string).collect::<Vec<_>>().join(",")
}

/// Canonical Java identity, independent of NBT compound property order.
fn state_name(entry: &Nbt) -> String {
    let mut name = entry
        .string("Name")
        .expect("block palette entry without Name");
    if let Some(properties) = entry.get("Properties") {
        let Nbt::Compound(properties) = properties else {
            panic!("block Properties must be an NBT compound");
        };
        let mut sorted: Vec<_> = properties.iter().collect();
        sorted.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        if !sorted.is_empty() {
            name.push('[');
            for (index, (key, value)) in sorted.into_iter().enumerate() {
                let Nbt::Str(value) = value else {
                    panic!("block property {key} must be an NBT string");
                };
                if index != 0 {
                    name.push(',');
                }
                name.push_str(key);
                name.push('=');
                name.push_str(value);
            }
            name.push(']');
        }
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

/// Typed big-endian NBT; floating values retain their exact IEEE bits.
#[derive(serde::Serialize)]
#[serde(tag = "type", content = "value")]
enum Nbt {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(u32),
    Double(u64),
    ByteArray(Vec<i8>),
    Str(String),
    List(u8, Vec<Nbt>),
    Compound(Vec<(String, Nbt)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
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
            Some(Nbt::List(_, v)) => v,
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
        let s = decode_java_string(&self.buf[self.off..self.off + n]);
        self.off += n;
        s
    }


    fn payload(&mut self, tag: u8) -> Nbt {
        match tag {
            1 => Nbt::Byte(i8::from_be_bytes(self.take())),
            2 => Nbt::Short(i16::from_be_bytes(self.take())),
            3 => Nbt::Int(i32::from_be_bytes(self.take())),
            4 => Nbt::Long(i64::from_be_bytes(self.take())),
            5 => Nbt::Float(u32::from_be_bytes(self.take())),
            6 => Nbt::Double(u64::from_be_bytes(self.take())),
            7 => {
                let n = self.count();
                Nbt::ByteArray((0..n).map(|_| i8::from_be_bytes(self.take())).collect())
            }
            8 => Nbt::Str(self.string()),
            9 => {
                let [et] = self.take();
                let n = self.count();
                Nbt::List(et, (0..n).map(|_| self.payload(et)).collect())
            }
            10 => {
                let mut entries = Vec::new();
                loop {
                    let [t] = self.take();
                    if t == 0 {
                        entries.sort_by(|a: &(String, Nbt), b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
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
/// Java NBT strings use modified UTF-8 over UTF-16 code units, not UTF-8.
fn decode_java_string(bytes: &[u8]) -> String {
    let mut index = 0;
    let units = std::iter::from_fn(|| {
        if index == bytes.len() {
            return None;
        }
        let first = bytes[index];
        let (unit, length) = match first {
            0x00..=0x7f => (u16::from(first), 1),
            0xc0..=0xdf => {
                let second = *bytes.get(index + 1).expect("truncated Java NBT string");
                assert_eq!(second & 0xc0, 0x80, "invalid Java NBT continuation");
                ((u16::from(first & 0x1f) << 6) | u16::from(second & 0x3f), 2)
            }
            0xe0..=0xef => {
                let second = *bytes.get(index + 1).expect("truncated Java NBT string");
                let third = *bytes.get(index + 2).expect("truncated Java NBT string");
                assert!(second & 0xc0 == 0x80 && third & 0xc0 == 0x80, "invalid Java NBT continuation");
                ((u16::from(first & 0x0f) << 12) | (u16::from(second & 0x3f) << 6) | u16::from(third & 0x3f), 3)
            }
            _ => panic!("invalid Java NBT modified UTF-8 lead byte"),
        };
        index += length;
        Some(unit)
    });
    let mut text = String::with_capacity(bytes.len());
    for character in char::decode_utf16(units) {
        text.push(character.expect("unpaired surrogate in Java NBT string cannot be represented as JSON text"));
    }
    text
}

#[cfg(test)]
mod canonical_tests {
    use super::*;

    #[test]
    fn java_string_preserves_encoded_null_and_supplementary_characters() {
        assert_eq!(decode_java_string(&[b'a', 0xc0, 0x80, 0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80]), "a\0\u{1f600}");
    }

    #[test]
    fn actor_compound_keys_use_native_utf16_order() {
        // U+10000 sorts before U+E000 in Java's UTF-16 order, but after it
        // in UTF-8/scalar order. The two valid modified-UTF-8 keys must agree
        // with the runtime actor encoder and native snapshot helper.
        let root = [
            10, 0, 0,
            1, 0, 3, 0xee, 0x80, 0x80, 1,
            1, 0, 6, 0xed, 0xa0, 0x80, 0xed, 0xb0, 0x80, 2,
            0,
        ];
        let actual = serde_json::to_value(Nbt::read_root(&root)).unwrap();
        assert_eq!(actual, serde_json::json!({
            "type": "Compound",
            "value": [
                ["\u{10000}", {"type": "Byte", "value": 2}],
                ["\u{e000}", {"type": "Byte", "value": 1}],
            ],
        }));
    }

    fn palette_entry(properties: &[(&str, &str)]) -> Nbt {
        Nbt::Compound(vec![
            ("Name".into(), Nbt::Str("minecraft:oak_stairs".into())),
            ("Properties".into(), Nbt::Compound(properties.iter().map(|(key, value)| {
                ((*key).into(), Nbt::Str((*value).into()))
            }).collect())),
        ])
    }

    #[test]
    fn palette_identity_keeps_all_properties_in_stable_order() {
        let a = palette_entry(&[
            ("waterlogged", "true"), ("shape", "straight"), ("half", "bottom"), ("facing", "north"),
        ]);
        let b = palette_entry(&[
            ("facing", "north"), ("half", "bottom"), ("shape", "straight"), ("waterlogged", "true"),
        ]);
        let dry = palette_entry(&[
            ("facing", "north"), ("half", "bottom"), ("shape", "straight"), ("waterlogged", "false"),
        ]);
        assert_eq!(state_name(&a), "minecraft:oak_stairs[facing=north,half=bottom,shape=straight,waterlogged=true]");
        assert_eq!(state_name(&a), state_name(&b));
        assert_ne!(state_name(&a), state_name(&dry));
    }

    #[test]
    fn source_fluid_levels_are_not_elided() {
        let water = Nbt::Compound(vec![
            ("Name".into(), Nbt::Str("minecraft:water".into())),
            ("Properties".into(), Nbt::Compound(vec![("level".into(), Nbt::Str("0".into()))])),
        ]);
        assert_eq!(state_name(&water), "minecraft:water[level=0]");
    }
}
