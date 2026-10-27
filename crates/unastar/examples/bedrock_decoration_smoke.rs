//! Smoke run of the Bedrock-style decoration stage on real terrain.
//!
//! Generates a 3x3 block of chunks with the existing terrain generator, then
//! carves and decorates the centre chunk once per overworld biome id (the
//! whole region pretends to be that biome), printing what was placed and how
//! long it took. Catches panics in any biome's compiled rules and features.
//!
//! `cargo run -p unastar --features bedrock-worldgen --example bedrock_decoration_smoke [seed] [chunk_x] [chunk_z]`

use std::collections::BTreeMap;
use std::time::Instant;
use unastar::registry::BlockRegistry;
use unastar::world::ChunkPos;
use unastar::world::chunk::{Chunk, MIN_Y};
use unastar::world::generator::bedrock::BiomeSet;
use unastar::world::generator::bedrock::adapter::RegionWorld;
use unastar::world::generator::bedrock::decorate::Decorator;
use unastar::world::generator::bedrock::generated::BIOMES;
use unastar::world::generator::bedrock::rng::BedrockRandom;
use unastar::world::generator::bedrock::surface::{
    self, ColumnInput, SurfaceColumn, SurfaceNoises,
};
use unastar::world::generator::bedrock::world::{BlockPos, BlockWorld};
use unastar::world::generator::{DecorationRegion, JavaGenerator, ProtoChunk, QuartBiomes};

struct Column<'a> {
    chunk: &'a mut Chunk,
    x: u8,
    z: u8,
}

impl SurfaceColumn for Column<'_> {
    fn min_y(&self) -> i32 {
        MIN_Y
    }
    fn max_y(&self) -> i32 {
        MIN_Y + 384
    }
    fn get(&self, y: i32) -> u32 {
        self.chunk.get_block(self.x, y as i16, self.z)
    }
    fn set(&mut self, y: i32, id: u32) {
        self.chunk.set_block(self.x, y as i16, self.z, id);
    }
}

fn histogram(chunk: &Chunk) -> BTreeMap<u32, u32> {
    let mut out = BTreeMap::new();
    for x in 0..16u8 {
        for z in 0..16u8 {
            for y in MIN_Y..MIN_Y + 384 {
                *out.entry(chunk.get_block(x, y as i16, z)).or_default() += 1;
            }
        }
    }
    out
}

fn main() {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}

fn run() {
    let args: Vec<i64> = std::env::args()
        .skip(1)
        .map(|a| a.parse().expect("integer argument"))
        .collect();
    let seed = args.first().copied().unwrap_or(12345);
    let (cx, cz) = (
        args.get(1).copied().unwrap_or(0) as i32,
        args.get(2).copied().unwrap_or(0) as i32,
    );
    let terrain = JavaGenerator::new(seed);
    let t = Instant::now();
    let base: Vec<Chunk> = (0..9)
        .map(|i| {
            let mut chunk = terrain
                .generate_noise_proto(ChunkPos::new(cx + (i % 3) - 1, cz + (i / 3) - 1))
                .chunk;
            chunk.convert_to_bedrock().expect("native terrain conversion");
            chunk
        })
        .collect();
    println!("terrain 3x3: {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    let decorator = Decorator::new(seed);
    let surface_noises = SurfaceNoises::new(seed);
    let registry = BlockRegistry::vanilla();
    // Snapshot solid heights before applying any biome's surface, so slopes
    // cannot depend on the column traversal order.
    let heights: Vec<i32> = (0..48 * 48)
        .map(|i| {
            let (x, z) = (i % 48, i / 48);
            let chunk = &base[z / 16 * 3 + x / 16];
            (MIN_Y..MIN_Y + 384)
                .rev()
                .find(|&y| {
                    let id = chunk.get_block((x % 16) as u8, y as i16, (z % 16) as u8);
                    id != unastar::world::generator::bedrock::generated::bb::AIR
                        && !unastar::world::generator::bedrock::generated::props::is_liquid(id)
                })
                .unwrap_or(MIN_Y)
        })
        .collect();
    let mut total = std::time::Duration::ZERO;
    let mut runs = 0;
    for biome in BIOMES.iter().filter(|b| b.overworld) {
        let mut chunks = base.clone();
        let mut surface_rng = BedrockRandom::new(seed as u32);
        for (i, chunk) in chunks.iter_mut().enumerate() {
            for z in 0..16u8 {
                for x in 0..16u8 {
                    let (rx, rz) = (i % 3 * 16 + x as usize, i / 3 * 16 + z as usize);
                    let height = |x: usize, z: usize| heights[z * 48 + x];
                    let input = ColumnInput {
                        x: chunk.x * 16 + x as i32,
                        z: chunk.z * 16 + z as i32,
                        biome,
                        sea_level: 63,
                        neighbour_heights: [
                            height(rx, rz.saturating_sub(1)),
                            height(rx, (rz + 1).min(47)),
                            height(rx.saturating_sub(1), rz),
                            height((rx + 1).min(47), rz),
                        ],
                    };
                    surface::build_column(
                        &mut Column { chunk, x, z },
                        &input,
                        &surface_noises,
                        &mut surface_rng,
                    );
                }
            }
        }
        let before = histogram(&chunks[4]);
        let id = biome.id;
        let mut set = BiomeSet::default();
        set.insert(id);
        let mut protos: Vec<_> = chunks
            .into_iter()
            .map(|chunk| ProtoChunk {
                pos: ChunkPos::new(chunk.x, chunk.z),
                chunk,
                biomes: QuartBiomes::filled(id),
            })
            .collect();
        let t = Instant::now();
        decorator.carve(&mut protos[4], cx, cz, &set);
        let mut region = DecorationRegion::new(
            seed,
            ChunkPos::new(cx, cz),
            std::array::from_fn(|i| &protos[i]),
            &unastar::world::generator::bedrock::generated::props::STATE_CLASSES,
        );
        let mut world = RegionWorld::new(&mut region);
        decorator.decorate(&mut world, cx, cz, &set);
        let elapsed = t.elapsed();
        total += elapsed;
        runs += 1;
        let top = world.heightmap(cx * 16 + 8, cz * 16 + 8);
        let probe = world.get(BlockPos::new(cx * 16 + 8, top - 1, cz * 16 + 8));
        drop(world);
        let writes = region.into_writes();
        let mut center = protos.swap_remove(4).chunk;
        writes.apply_to(0, 0, &mut center);
        let after = histogram(&center);
        let mut changes: Vec<(i64, String)> = after
            .iter()
            .map(|(id, n)| (*n as i64 - *before.get(id).unwrap_or(&0) as i64, *id))
            .filter(|(d, _)| *d > 0)
            .map(|(d, id)| {
                (
                    d,
                    registry
                        .get_by_runtime_id(id)
                        .map_or(format!("#{id}"), |b| b.string_id.clone()),
                )
            })
            .collect();
        changes.sort_by_key(|b| std::cmp::Reverse(b.0));
        let summary: Vec<String> = changes
            .iter()
            .take(6)
            .map(|(d, n)| format!("{}+{d}", n.trim_start_matches("minecraft:")))
            .collect();
        println!(
            "{:<40} {:>7.2} ms  top {} {:<22} {}",
            biome.identifier,
            elapsed.as_secs_f64() * 1e3,
            top,
            registry.get_by_runtime_id(probe).map_or("?".into(), |b| b
                .string_id
                .trim_start_matches("minecraft:")
                .to_owned()),
            summary.join(" ")
        );
    }
    println!(
        "decoration: {runs} biomes, mean {:.2} ms/chunk",
        total.as_secs_f64() * 1e3 / runs as f64
    );
}
