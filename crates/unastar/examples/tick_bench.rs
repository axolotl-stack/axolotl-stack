//! Server tick cost with players and mobs, headless (no network).
//!
//! A superflat `GameServer` with `--players` players spaced 96 blocks apart
//! (each with its own simulation area), `--cows` cows and `--zombies` zombies
//! summoned evenly around them, and natural spawning on. After the players'
//! views load and the mobs are summoned, the benchmark runs `--ticks` ticks
//! and reports ms per tick (mean, p50, p99, max) and allocations per tick.
//! Night time is set so zombies stay active.
//!
//! ```text
//! cargo build --release -p unastar --example tick_bench
//! target/release/examples/tick_bench --players 1 --cows 500 --zombies 500 --ticks 400
//! ```
//!
//! See `tools/perf/README.md` for recorded results.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use glam::DVec3;
use serde::Serialize;
use unastar::world::{WorldConfig, WorldGenerator};
use unastar::{CommandSource, GameServer, PlayerSpawnData, ServerConfig};

struct CountingAlloc;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static COUNTING: AtomicBool = AtomicBool::new(false);

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

/// Feet height on the superflat surface (bedrock, two dirt, grass at y 0..=3).
const SURFACE: f64 = 4.0;
/// Distance between players: beyond each other's simulation distance.
const PLAYER_SPACING: f64 = 96.0;

#[derive(Serialize)]
struct Result {
    players: usize,
    cows: usize,
    zombies: usize,
    ticks: usize,
    /// Mobs alive when measuring started (summoned plus naturally spawned).
    mobs_at_start: usize,
    mean_ms: f64,
    p50_ms: f64,
    p99_ms: f64,
    max_ms: f64,
    allocations_per_tick: f64,
    kib_per_tick: f64,
}

fn arg(name: &str, default: usize) -> usize {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .map(|i| args[i + 1].parse().expect("integer argument"))
        .unwrap_or(default)
}

fn main() {
    let result = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(run)
        .expect("benchmark thread")
        .join()
        .expect("benchmark panicked");
    println!(
        "{}",
        serde_json::to_string_pretty(&result).expect("serialize")
    );
}

fn run() -> Result {
    let players = arg("--players", 1);
    let cows = arg("--cows", 0);
    let zombies = arg("--zombies", 0);
    let ticks = arg("--ticks", 400);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let _guard = runtime.enter();

    let mut server = GameServer::with_config(ServerConfig {
        world: WorldConfig {
            generator: WorldGenerator::SuperFlat,
            ..WorldConfig::default()
        },
        max_chunk_radius: 8,
        ..ServerConfig::default()
    });
    let mut receivers = Vec::new();
    let homes: Vec<DVec3> = (0..players)
        .map(|i| DVec3::new(i as f64 * PLAYER_SPACING + 0.5, SURFACE + 1.62, 0.5))
        .collect();
    for (i, home) in homes.iter().enumerate() {
        let (tx, rx) = tokio::sync::mpsc::channel(1 << 16);
        receivers.push(rx);
        server
            .spawn_player(PlayerSpawnData {
                session_id: i as u64 + 1,
                display_name: format!("TickBot{i}"),
                xuid: None,
                uuid: None,
                runtime_id: i as i64 + 1,
                position: *home,
                saved_data: None,
                outbound_tx: tx,
                chunk_radius: 8,
                enchantment_seed: None,
            })
            .expect("player spawns");
    }
    let drain = |receivers: &mut Vec<tokio::sync::mpsc::Receiver<_>>| {
        for rx in receivers.iter_mut() {
            while rx.try_recv().is_ok() {}
        }
    };
    // Load the views.
    for _ in 0..100 {
        server.tick();
        drain(&mut receivers);
    }
    let world = server.ecs.world_mut();
    let run = |world: &mut bevy_ecs::world::World, line: &str| {
        let output = unastar::command::execute(world, CommandSource::server(), line);
        assert!(output.success_count > 0, "{line}: {:?}", output.messages);
    };
    run(world, "time set night");
    for (kind, count) in [("minecraft:cow", cows), ("minecraft:zombie", zombies)] {
        for i in 0..count {
            let home = homes[i % homes.len().max(1)];
            // A 48x48 square around the player, inside simulation distance.
            let dx = (i * 7919 % 48) as f64 - 24.0;
            let dz = (i * 104_729 % 48) as f64 - 24.0;
            run(
                world,
                &format!("summon {kind} {} {SURFACE} {}", home.x + dx, home.z + dz),
            );
        }
    }
    // Let spawning settle before measuring.
    for _ in 0..40 {
        server.tick();
        drain(&mut receivers);
    }
    let mobs_at_start = mob_count(&mut server);

    let mut times = Vec::with_capacity(ticks);
    let (a0, b0) = (
        ALLOCS.load(Ordering::Relaxed),
        BYTES.load(Ordering::Relaxed),
    );
    for _ in 0..ticks {
        COUNTING.store(true, Ordering::Relaxed);
        let started = Instant::now();
        server.tick();
        times.push(started.elapsed().as_secs_f64() * 1e3);
        COUNTING.store(false, Ordering::Relaxed);
        drain(&mut receivers);
    }
    let (a1, b1) = (
        ALLOCS.load(Ordering::Relaxed),
        BYTES.load(Ordering::Relaxed),
    );
    let mean = times.iter().sum::<f64>() / ticks as f64;
    let mut sorted = times.clone();
    sorted.sort_by(f64::total_cmp);
    let pct = |p: f64| sorted[((ticks as f64 - 1.0) * p).round() as usize];
    Result {
        players,
        cows,
        zombies,
        ticks,
        mobs_at_start,
        mean_ms: mean,
        p50_ms: pct(0.5),
        p99_ms: pct(0.99),
        max_ms: *sorted.last().expect("ticks > 0"),
        allocations_per_tick: (a1 - a0) as f64 / ticks as f64,
        kib_per_tick: (b1 - b0) as f64 / ticks as f64 / 1024.0,
    }
}

fn mob_count(server: &mut GameServer) -> usize {
    let world = server.ecs.world_mut();
    let mut query = world.query::<&unastar::entity::mob::MobKind>();
    query.iter(world).count()
}
