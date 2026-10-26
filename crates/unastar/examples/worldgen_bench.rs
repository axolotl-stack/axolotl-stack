//! Cold fixture cost, exclusive stage allocations/times, and real-pool throughput.
//!
//! See tools/worldgen-perf/README.md for affinity, A/B and profiling recipes.
//! Stage instrumentation requires `--features worldgen-profile`; normal builds have
//! no stage timers. Use normal builds for throughput and A/B wall-time comparisons.

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use unastar::world::ChunkPos;
use unastar::world::ecs::generation_worker::ChunkGenerationWorker;
use unastar::world::generator::{ChunkPipeline, FlatGenerator, Generator};
use unastar_noise::profile::{self, Stage};

struct CountingAlloc;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
// Throughput must not serialize workers on allocation-counter cache lines.
static COUNTING: AtomicBool = AtomicBool::new(false);

fn allocation(bytes: usize, ptr: *mut u8) {
    if !ptr.is_null() && COUNTING.load(Ordering::Relaxed) {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(bytes as u64, Ordering::Relaxed);
        profile::allocation(bytes);
    }
}

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        allocation(layout.size(), ptr);
        ptr
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        allocation(layout.size(), ptr);
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let ptr = unsafe { System.realloc(ptr, layout, new_size) };
        allocation(new_size, ptr);
        ptr
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

#[derive(Deserialize)]
struct Fixture {
    seed: i64,
    chunks: Vec<FixturePos>,
}
#[derive(Deserialize)]
struct FixturePos {
    x: i32,
    z: i32,
}

struct Options {
    kind: String,
    mode: String,
    workers: Vec<usize>,
    json: bool,
    ratchet: Option<String>,
}

impl Options {
    fn parse() -> Result<Self> {
        let mut options = Self {
            kind: "java".into(),
            mode: "serial".into(),
            workers: vec![1, 4, 8, 16],
            json: false,
            ratchet: None,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--json" => options.json = true,
                "--kind" => options.kind = args.next().context("--kind needs a value")?,
                "--mode" => options.mode = args.next().context("--mode needs a value")?,
                "--ratchet" => {
                    options.ratchet = Some(args.next().context("--ratchet needs a path")?)
                }
                "--workers" => {
                    options.workers = args
                        .next()
                        .context("--workers needs a comma-separated list")?
                        .split(',')
                        .map(str::parse)
                        .collect::<std::result::Result<_, _>>()?;
                    ensure!(
                        options.workers.iter().all(|n| (1..=16).contains(n)),
                        "workers must be in 1..=16"
                    );
                }
                "--help" | "-h" => {
                    println!(
                        "worldgen_bench [--kind java|bedrock|flat] [--mode serial|throughput|spawn] [--workers 1,4,8,16] [--json] [--ratchet path]\nStage breakdown: build with --features worldgen-profile. Ratchets only accept serial stage-enabled runs."
                    );
                    std::process::exit(0);
                }
                _ => bail!("unknown argument {arg}"),
            }
        }
        ensure!(
            matches!(options.mode.as_str(), "serial" | "throughput" | "spawn"),
            "unknown mode {}",
            options.mode
        );
        ensure!(
            options.ratchet.is_none() || (options.mode == "serial" && profile::ENABLED),
            "--ratchet requires serial mode and --features worldgen-profile"
        );
        Ok(options)
    }
}

fn generator(kind: &str, _seed: i64) -> Result<Arc<dyn Generator>> {
    Ok(match kind {
        #[cfg(feature = "java-worldgen")]
        "java" => Arc::new(unastar::world::generator::JavaGenerator::new(_seed)),
        #[cfg(not(feature = "java-worldgen"))]
        "java" => {
            bail!("--kind java requires --features java-worldgen and UNASTAR_JAVA_SERVER_JAR")
        }
        "flat" => Arc::new(FlatGenerator),
        #[cfg(feature = "bedrock-worldgen")]
        "bedrock" => Arc::new(unastar::world::generator::bedrock::BedrockGenerator::new(
            _seed,
        )),
        #[cfg(not(feature = "bedrock-worldgen"))]
        "bedrock" => {
            bail!("--kind bedrock requires --features bedrock-worldgen and UNASTAR_BDS_DIR")
        }
        other => bail!("unknown kind {other}"),
    })
}

fn counters() -> (u64, u64) {
    (
        ALLOCS.load(Ordering::Relaxed),
        BYTES.load(Ordering::Relaxed),
    )
}

#[derive(Serialize, Deserialize)]
struct StageResult {
    calls: u64,
    allocations: u64,
    bytes: u64,
    ms: f64,
}

#[derive(Serialize, Deserialize)]
struct SerialResult {
    kind: String,
    profile: String,
    instrumented: bool,
    seed: i64,
    chunks: usize,
    setup_ms: f64,
    proto_ms: f64,
    finished_ms: f64,
    cached_protos: usize,
    allocations: u64,
    bytes: u64,
    stages: BTreeMap<String, StageResult>,
}

fn serial(options: &Options, fixture: &Fixture) -> Result<()> {
    let started = Instant::now();
    let generator = generator(&options.kind, fixture.seed)?;
    let setup_ms = started.elapsed().as_secs_f64() * 1e3;
    let started = Instant::now();
    for pos in &fixture.chunks {
        std::hint::black_box(generator.generate_proto(ChunkPos::new(pos.x, pos.z)));
    }
    let proto_ms = started.elapsed().as_secs_f64() * 1e3;
    // Match the historical workload: noise globals are warm, both pipeline caches
    // and structure state are cold. Setup and proto allocations are not in the gate.
    let pipeline = ChunkPipeline::new(generator);
    profile::reset();
    let (a0, b0) = counters();
    COUNTING.store(true, Ordering::Relaxed);
    let started = Instant::now();
    {
        let _scope = profile::scope(Stage::Pipeline);
        for pos in &fixture.chunks {
            std::hint::black_box(pipeline.generate(ChunkPos::new(pos.x, pos.z)));
        }
    }
    let finished_ms = started.elapsed().as_secs_f64() * 1e3;
    let (a1, b1) = counters();
    COUNTING.store(false, Ordering::Relaxed);
    let snapshot = profile::snapshot();
    let result = SerialResult {
        kind: options.kind.clone(),
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
        .into(),
        instrumented: profile::ENABLED,
        seed: fixture.seed,
        chunks: fixture.chunks.len(),
        setup_ms,
        proto_ms,
        finished_ms,
        cached_protos: pipeline.cached_protos(),
        allocations: a1 - a0,
        bytes: b1 - b0,
        stages: if profile::ENABLED {
            Stage::ALL
                .into_iter()
                .map(|stage| {
                    let c = snapshot[stage as usize];
                    (
                        stage.name().to_owned(),
                        StageResult {
                            calls: c.calls,
                            allocations: c.allocations,
                            bytes: c.bytes,
                            ms: c.elapsed.as_secs_f64() * 1e3,
                        },
                    )
                })
                .collect()
        } else {
            BTreeMap::new()
        },
    };
    if let Some(path) = &options.ratchet {
        let baseline: SerialResult = serde_json::from_slice(&std::fs::read(path)?)?;
        ensure!(
            baseline.kind == result.kind
                && baseline.seed == result.seed
                && baseline.chunks == result.chunks
                && baseline.instrumented
                && baseline.profile == result.profile,
            "ratchet workload/build mismatch"
        );
        ensure!(
            result.allocations <= baseline.allocations && result.bytes <= baseline.bytes,
            "finished allocations/bytes regressed: {}/{} > {}/{}",
            result.allocations,
            result.bytes,
            baseline.allocations,
            baseline.bytes
        );
        for (name, stage) in &result.stages {
            let base = baseline
                .stages
                .get(name)
                .with_context(|| format!("ratchet missing {name}"))?;
            ensure!(
                stage.allocations <= base.allocations && stage.bytes <= base.bytes,
                "{name} allocations/bytes regressed: {}/{} > {}/{}",
                stage.allocations,
                stage.bytes,
                base.allocations,
                base.bytes
            );
        }
    }
    if options.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        let n = result.chunks as f64;
        println!(
            "| kind | profile | instrumented | chunks | setup ms | proto ms/chunk | finished ms/chunk | protos | allocs/chunk | KiB/chunk |"
        );
        println!("|---|---|---|---:|---:|---:|---:|---:|---:|---:|");
        println!(
            "| {} | {} | {} | {} | {:.1} | {:.3} | {:.3} | {} | {:.2} | {:.2} |",
            result.kind,
            result.profile,
            result.instrumented,
            result.chunks,
            result.setup_ms,
            result.proto_ms / n,
            result.finished_ms / n,
            result.cached_protos,
            result.allocations as f64 / n,
            result.bytes as f64 / n / 1024.0
        );
        if profile::ENABLED {
            println!(
                "\n| exclusive stage | calls | ms/requested chunk | allocations/requested chunk | bytes/requested chunk |"
            );
            println!("|---|---:|---:|---:|---:|");
            for (name, c) in &result.stages {
                println!(
                    "| {name} | {} | {:.3} | {:.2} | {:.2} |",
                    c.calls,
                    c.ms / n,
                    c.allocations as f64 / n,
                    c.bytes as f64 / n
                );
            }
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct ThroughputResult {
    kind: String,
    instrumented: bool,
    workers: usize,
    workload: &'static str,
    chunks: usize,
    elapsed_ms: f64,
    chunks_per_second: f64,
    spawn: Option<(i32, i32, i32)>,
}

fn pool_run(
    options: &Options,
    fixture: &Fixture,
    workers: usize,
    spawn: bool,
) -> Result<ThroughputResult> {
    // Cold-world wall time includes generator setup, spawn search, worker startup,
    // requests, all neighbour generation, and completion of the final requested chunk.
    let started = Instant::now();
    let pipeline = Arc::new(ChunkPipeline::new(generator(&options.kind, fixture.seed)?));
    let center = spawn.then(|| pipeline.find_spawn());
    let positions: Vec<_> = if let Some(center) = center {
        let (cx, cz) = (center.x.div_euclid(16), center.z.div_euclid(16));
        // Center outward, matching the server's distance-prioritized load queue.
        let mut positions: Vec<_> = (-8..=8)
            .flat_map(|dx| (-8..=8).map(move |dz| (dx, dz)))
            .collect();
        positions.sort_by_key(|&(dx, dz)| (dx * dx + dz * dz, dx, dz));
        positions
            .into_iter()
            .map(|(dx, dz)| (cx + dx, cz + dz))
            .collect()
    } else {
        fixture.chunks.iter().map(|p| (p.x, p.z)).collect()
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let pool = ChunkGenerationWorker::spawn(pipeline, workers);
        let receivers: Vec<_> = positions
            .iter()
            .map(|&(x, z)| {
                pool.generate(x, z, None, 0)
                    .context("generation pool stopped")
            })
            .collect::<Result<_>>()?;
        for receiver in receivers {
            let (chunk, loaded) = receiver.await.context("generation worker panicked")??;
            ensure!(!loaded, "cold benchmark unexpectedly loaded storage");
            std::hint::black_box(chunk);
        }
        Ok::<_, anyhow::Error>(())
    })?;
    let elapsed = started.elapsed();
    Ok(ThroughputResult {
        kind: options.kind.clone(),
        instrumented: profile::ENABLED,
        workers,
        workload: if spawn {
            "cold_spawn_radius_8_square"
        } else {
            "cold_fixture"
        },
        chunks: positions.len(),
        elapsed_ms: elapsed.as_secs_f64() * 1e3,
        chunks_per_second: positions.len() as f64 / elapsed.as_secs_f64(),
        spawn: center.map(|p| (p.x, p.y, p.z)),
    })
}

fn run() -> Result<()> {
    let options = Options::parse()?;
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bds_1_26_51_seed_12345.json");
    let fixture: Fixture = serde_json::from_slice(&std::fs::read(path)?)?;
    match options.mode.as_str() {
        "serial" => serial(&options, &fixture),
        _ => {
            let mut results = Vec::new();
            for &workers in &options.workers {
                if options.mode == "throughput" {
                    results.push(pool_run(&options, &fixture, workers, false)?);
                }
                results.push(pool_run(&options, &fixture, workers, true)?);
            }
            if options.json {
                println!("{}", serde_json::to_string_pretty(&results)?);
            } else {
                println!(
                    "| kind | instrumented | workers | workload | chunks | elapsed ms | chunks/s | spawn |"
                );
                println!("|---|---|---:|---|---:|---:|---:|---|");
                for r in results {
                    println!(
                        "| {} | {} | {} | {} | {} | {:.2} | {:.2} | {:?} |",
                        r.kind,
                        r.instrumented,
                        r.workers,
                        r.workload,
                        r.chunks,
                        r.elapsed_ms,
                        r.chunks_per_second,
                        r.spawn
                    );
                }
            }
            Ok(())
        }
    }
}

fn main() -> Result<()> {
    // Terrain generation and spawn search need a large debug stack on Windows.
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(run)?
        .join()
        .map_err(|_| anyhow::anyhow!("benchmark thread panicked"))?
}
