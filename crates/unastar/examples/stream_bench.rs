//! Tick time and send queue while a player's whole view changes at once.
//!
//! Headless: a Java-generated world is generated around two areas 4000
//! blocks apart and saved (LevelDB, scratch directory); then a fresh server
//! over that storage runs the scenario with one radius-12 player whose
//! client and link are simulated (`tests/support/stream_sim.rs`):
//!
//! 1. join at area A and load the view (from storage);
//! 2. die and respawn at a spawn point in area B (stored, not in memory);
//! 3. `/tp` back to A (still in memory);
//! 4. wait until B's chunks unloaded, then `/tp` to B again (storage).
//!
//! Ticks run at the real 50 ms cadence. Each phase reports ticks until the
//! view is complete, tick time p50/p99/max, the slowest stages of the worst
//! tick, and the link's peak queue. `--assert` exits with an error when a tick
//! exceeds 50 ms or the queue reaches the session limit.
//!
//! ```text
//! cargo build --release -p unastar --features java-worldgen --example stream_bench
//! target/release/examples/stream_bench --link-kib-per-tick 200 --assert
//! ```

#[path = "../tests/support/stream_sim.rs"]
mod stream_sim;

use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::DVec3;
use serde::Serialize;
use stream_sim::{SESSION_QUEUE_LIMIT, SimPlayer};
use unastar::world::ecs::ChunkManager;
use unastar::world::{WorldConfig, WorldGenerator};
use unastar::{CommandSource, GameServer, ServerConfig};

const RADIUS: i32 = 12;
const TICK: Duration = Duration::from_millis(50);
const AREA_A: DVec3 = DVec3::new(0.5, 140.0, 0.5);
const AREA_B: DVec3 = DVec3::new(4000.5, 140.0, 0.5);

#[derive(Serialize)]
struct Phase {
    name: &'static str,
    ticks_to_view: Option<usize>,
    p50_ms: f64,
    p99_ms: f64,
    max_ms: f64,
    worst_tick_stages: String,
    peak_queue_kib: f64,
}

#[derive(Serialize)]
struct Report {
    radius: i32,
    link_kib_per_tick: usize,
    phases: Vec<Phase>,
}

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let report = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(run)
        .expect("benchmark thread")
        .join()
        .expect("benchmark panicked");
    println!("{}", serde_json::to_string_pretty(&report).expect("json"));
    let assert = std::env::args().any(|a| a == "--assert");
    let failed = report
        .phases
        .iter()
        .any(|p| p.max_ms > 50.0 || p.peak_queue_kib * 1024.0 >= SESSION_QUEUE_LIMIT as f64);
    if assert && failed {
        eprintln!("a tick exceeded 50 ms or the send queue reached the session limit");
        std::process::exit(1);
    }
}

fn server(dir: &std::path::Path) -> GameServer {
    let mut server = GameServer::with_config(ServerConfig {
        world: WorldConfig {
            generator: WorldGenerator::Java { seed: 12345 },
            ..WorldConfig::default()
        },
        max_chunk_radius: RADIUS,
        ..ServerConfig::default()
    });
    let provider = unastar::storage::LevelDBWorldProvider::open(dir, 0).expect("world db");
    server
        .ecs
        .world_mut()
        .resource_mut::<ChunkManager>()
        .set_provider(Arc::new(provider));
    server
}

fn command(server: &mut GameServer, line: &str) {
    let output = unastar::command::execute(server.ecs.world_mut(), CommandSource::server(), line);
    assert!(output.success_count > 0, "{line}: {:?}", output.messages);
}

/// Ticks at the real cadence until the view around `at` is complete (or
/// `ticks` ran out when `until_complete` is false).
fn run_phase(
    server: &mut GameServer,
    player: &mut SimPlayer,
    name: &'static str,
    at: DVec3,
    ticks: usize,
    until_complete: bool,
) -> Phase {
    player.link.peak = player.link.queued();
    let mut times = Vec::new();
    let mut worst = (0.0f64, String::new());
    let mut ticks_to_view = None;
    for tick in 0..ticks {
        let started = Instant::now();
        player.send_requests(server);
        server.tick();
        let ecs_end = Instant::now();
        let ms = started.elapsed().as_secs_f64() * 1e3;
        if ms > worst.0 {
            let stages = server
                .ecs
                .world()
                .resource::<unastar::ecs::TickStages>()
                .slowest(ecs_end, 4);
            worst = (ms, stages);
        }
        times.push(ms);
        player.receive();
        if ticks_to_view.is_none() && player.missing(at, RADIUS) == 0 {
            ticks_to_view = Some(tick + 1);
            if until_complete {
                break;
            }
        }
        std::thread::sleep(TICK.saturating_sub(started.elapsed()));
    }
    let mut sorted = times.clone();
    sorted.sort_by(f64::total_cmp);
    let pct = |p: f64| sorted[((sorted.len() as f64 - 1.0) * p).round() as usize];
    Phase {
        name,
        ticks_to_view,
        p50_ms: pct(0.5),
        p99_ms: pct(0.99),
        max_ms: *sorted.last().expect("ticks"),
        worst_tick_stages: worst.1,
        peak_queue_kib: player.link.peak as f64 / 1024.0,
    }
}

fn run() -> Report {
    let link_kib: usize = arg("--link-kib-per-tick")
        .map(|v| v.parse().expect("integer"))
        .unwrap_or(200);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .thread_stack_size(16 << 20)
        .enable_all()
        .build()
        .expect("runtime");
    let _guard = runtime.enter();
    let dir = tempfile::tempdir().expect("scratch dir");

    // Generate both areas once and save them.
    {
        let mut server = server(dir.path());
        let mut player = SimPlayer::join(&mut server, 1, AREA_A, RADIUS, usize::MAX / 4);
        let a = run_phase(&mut server, &mut player, "generate A", AREA_A, 2000, true);
        assert!(a.ticks_to_view.is_some(), "area A never loaded");
        command(&mut server, "tp SimPlayer1 4000.5 140 0.5");
        player.forget_terrain();
        let b = run_phase(&mut server, &mut player, "generate B", AREA_B, 2000, true);
        assert!(b.ticks_to_view.is_some(), "area B never loaded");
        let saved = runtime.block_on(server.save_all_chunks());
        eprintln!("generated and saved {saved} chunks");
    }

    let mut server = server(dir.path());
    let mut player = SimPlayer::join(&mut server, 1, AREA_A, RADIUS, link_kib * 1024);
    let mut phases = vec![run_phase(
        &mut server,
        &mut player,
        "join at A (storage)",
        AREA_A,
        600,
        true,
    )];
    // Let the join settle, then die and respawn at B.
    phases.push(run_phase(
        &mut server,
        &mut player,
        "idle at A",
        AREA_A,
        40,
        false,
    ));
    {
        let world = server.ecs.world_mut();
        world.entity_mut(player.entity).insert(
            unastar::entity::player_life::respawn::PlayerSpawnPoint(AREA_B),
        );
        assert!(unastar::entity::player_life::kill_player(
            world,
            player.entity
        ));
        unastar::entity::player_life::respawn::respawn(world, player.entity);
    }
    player.forget_terrain();
    phases.push(run_phase(
        &mut server,
        &mut player,
        "respawn at B (storage)",
        AREA_B,
        600,
        true,
    ));
    command(&mut server, "tp SimPlayer1 0.5 140 0.5");
    player.forget_terrain();
    phases.push(run_phase(
        &mut server,
        &mut player,
        "tp to A (memory)",
        AREA_A,
        600,
        true,
    ));
    // B's chunks unload after their grace period.
    phases.push(run_phase(
        &mut server,
        &mut player,
        "idle at A",
        AREA_A,
        160,
        false,
    ));
    command(&mut server, "tp SimPlayer1 4000.5 140 0.5");
    player.forget_terrain();
    phases.push(run_phase(
        &mut server,
        &mut player,
        "tp to B (storage)",
        AREA_B,
        600,
        true,
    ));
    Report {
        radius: RADIUS,
        link_kib_per_tick: link_kib,
        phases,
    }
}
