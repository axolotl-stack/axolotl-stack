//! Worldgen benchmark: ms/chunk and allocations/chunk, single-threaded.
//!
//! Every worldgen slice reruns this and records the numbers (they only ratchet down).
//!
//! ```text
//! cargo run -p unastar --example worldgen_bench                 # debug
//! cargo run -p unastar --example worldgen_bench --release       # release
//! cargo run -p unastar --example worldgen_bench -- --kind flat  # other generators
//! ```
//!
//! Timing rules (hybrid CPU): pin the process to one performance core and alternate A/B
//! builds over at least 3 rounds; unpinned runs drift 2-3x. On Windows:
//! `powershell -c "$p = Start-Process target\debug\examples\worldgen_bench.exe -PassThru -NoNewWindow; $p.ProcessorAffinity = 4; $p.WaitForExit()"`
//! (affinity mask 4 = logical CPU 2). Compare only runs taken under the same power
//! settings.
//!
//! Workload: the 68 chunks of the BDS seed-12345 fixture (three separate areas), generated
//! from a cold pipeline in fixture order. Reported:
//! - `proto`: terrain stages only (`Generator::generate_proto`), per chunk;
//! - `finished`: `ChunkPipeline::generate`, amortised per requested chunk, including the
//!   proto ring and decoration the 3x3 stage needs;
//! - allocations and bytes allocated per finished chunk.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use unastar::world::ChunkPos;
use unastar::world::generator::{ChunkPipeline, FlatGenerator, Generator, JavaGenerator};

struct CountingAlloc;

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

/// Chunks of `tests/fixtures/bds_1_26_51_seed_12345.json`, in fixture order.
const CHUNKS: &[(i32, i32)] = &[
    (-4, -4),
    (-4, -3),
    (-4, -2),
    (-4, -1),
    (-4, 1),
    (-4, 2),
    (-4, 3),
    (-3, -3),
    (-3, -2),
    (-3, 0),
    (-3, 1),
    (-3, 3),
    (-2, -4),
    (-2, -3),
    (-2, -2),
    (-2, -1),
    (-2, 0),
    (-2, 1),
    (-2, 2),
    (-1, -4),
    (-1, -3),
    (-1, -2),
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (-1, 2),
    (-1, 3),
    (0, -4),
    (0, -3),
    (0, -2),
    (0, 3),
    (1, -3),
    (1, -2),
    (1, -1),
    (1, 0),
    (1, 1),
    (1, 2),
    (1, 3),
    (2, -3),
    (2, -2),
    (2, -1),
    (2, 0),
    (2, 2),
    (2, 3),
    (3, -4),
    (3, -3),
    (3, -2),
    (3, -1),
    (3, 2),
    (3, 3),
    (64, 66),
    (64, 67),
    (65, 65),
    (66, 64),
    (67, 64),
    (67, 65),
    (67, 66),
    (67, 67),
    (-128, 34),
    (-128, 35),
    (-127, 32),
    (-127, 33),
    (-127, 34),
    (-127, 35),
    (-126, 33),
    (-126, 35),
    (-125, 34),
    (-125, 35),
];

fn counters() -> (u64, u64) {
    (
        ALLOCS.load(Ordering::Relaxed),
        BYTES.load(Ordering::Relaxed),
    )
}

fn main() {
    // Terrain generation needs a large stack in debug builds on Windows.
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(run)
        .expect("spawn bench thread")
        .join()
        .expect("bench thread panicked");
}

fn run() {
    let args: Vec<String> = std::env::args().collect();
    let kind = args
        .iter()
        .position(|a| a == "--kind")
        .map_or("java", |i| args[i + 1].as_str());
    let seed = 12345;

    let started = Instant::now();
    let generator: std::sync::Arc<dyn Generator> = match kind {
        "java" => std::sync::Arc::new(JavaGenerator::new(seed)),
        "flat" => std::sync::Arc::new(FlatGenerator),
        #[cfg(feature = "bedrock-worldgen")]
        "bedrock" => std::sync::Arc::new(
            unastar::world::generator::bedrock::BedrockGenerator::new(seed),
        ),
        #[cfg(not(feature = "bedrock-worldgen"))]
        "bedrock" => {
            panic!("--kind bedrock requires --features bedrock-worldgen and UNASTAR_BDS_DIR")
        }
        other => panic!("unknown --kind {other} (java, bedrock, flat)"),
    };
    let setup_ms = started.elapsed().as_secs_f64() * 1e3;

    let started = Instant::now();
    for &(x, z) in CHUNKS {
        std::hint::black_box(generator.generate_proto(ChunkPos::new(x, z)));
    }
    let proto_ms = started.elapsed().as_secs_f64() * 1e3 / CHUNKS.len() as f64;

    let pipeline = ChunkPipeline::new(generator);
    let (a0, b0) = counters();
    let started = Instant::now();
    for &(x, z) in CHUNKS {
        std::hint::black_box(pipeline.generate(ChunkPos::new(x, z))
            .expect("worldgen benchmark produced an unrepresentable native actor"));
    }
    let finished_ms = started.elapsed().as_secs_f64() * 1e3 / CHUNKS.len() as f64;
    let (a1, b1) = counters();
    let n = CHUNKS.len() as u64;

    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!(
        "| kind | profile | chunks | setup ms | proto ms/chunk | finished ms/chunk | protos made | allocs/chunk | KiB alloc/chunk |"
    );
    println!("|---|---|---:|---:|---:|---:|---:|---:|---:|");
    println!(
        "| {kind} | {profile} | {} | {setup_ms:.1} | {proto_ms:.2} | {finished_ms:.2} | {} | {} | {} |",
        CHUNKS.len(),
        pipeline.cached_protos(),
        (a1 - a0) / n,
        (b1 - b0) / n / 1024,
    );
}
