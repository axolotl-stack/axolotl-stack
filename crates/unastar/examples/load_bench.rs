//! End-to-end chunk loading benchmark over loopback RakNet.
//!
//! Real jolyne clients join a server, request a render radius and record
//! when every chunk of their view arrives; optionally they then fly in a
//! straight line at elytra speed and record how far the terrain lags behind.
//! The server is either a release `unastar` executable that this benchmark
//! launches in a scratch directory (`--server-exe`), or any running Bedrock
//! server (`--addr`, e.g. BDS 1.26.51 on a non-default port).
//!
//! ```text
//! cargo build --release -p unastar --features java-worldgen --example load_bench
//! target/release/examples/load_bench --server-exe target/release/unastar.exe \
//!     --generator java --radius 12 --players 1 --world fresh --fly-seconds 20
//! ```
//!
//! Views: the server streams the square `|dx|, |dz| <= r` around the player;
//! the client renders the disk `dx² + dz² <= r²`. Both completion times are
//! reported. `--world generated` first loads the same views (and flight path)
//! once, stops the server with the `stop` console command so the chunks are
//! saved, restarts it and measures the second join.
//!
//! See `tools/perf/README.md` for the recorded results and method.

#![allow(clippy::too_many_arguments)]

use std::collections::HashSet;
use std::io::Write as _;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use jolyne::stream::RakNetTransport;
use jolyne::valentine::types::{
    EnumsPlayerAuthInputPacketPayloadInputData, PlayerInputTick, Vec2, Vec3,
};
use jolyne::valentine::{McpePacket, McpePacketData, McpePacketName, PlayerAuthInputPacket};
use jolyne::{BedrockStream, Client, ClientConfig, ClientHandshakeConfig, Play};
use serde::Serialize;
use tokio::time::Instant;

/// Elytra cruising speed in blocks per second (a rocket-boosted glide).
const DEFAULT_FLY_SPEED: f32 = 30.0;
/// Client input rate (one `PlayerAuthInput` per game tick).
const INPUT_INTERVAL: Duration = Duration::from_millis(50);
/// Overworld sub-chunks per column (y -64..320) and the lowest one's index.
const SUBCHUNKS_PER_COLUMN: i32 = 24;
const LOWEST_SUBCHUNK: i32 = -4;
/// Sub-chunk offsets per `SubChunkRequest` (unastar answers at most 1024).
const MAX_SUBCHUNK_OFFSETS: usize = 1024;
/// After the render disk is complete, how long to wait for the rest of the
/// streamed square before concluding the server only streams the disk.
const SQUARE_GRACE: Duration = Duration::from_secs(3);
/// Bound for joining and loading one view.
const VIEW_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
struct Args {
    server_exe: Option<PathBuf>,
    addr: Option<SocketAddr>,
    generator: String,
    seed: i64,
    radius: i32,
    players: usize,
    world: String,
    fly_seconds: f32,
    fly_speed: f32,
    generation_threads: usize,
    label: String,
    json: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut args = Args {
        server_exe: None,
        addr: None,
        generator: "java".into(),
        seed: 12345,
        radius: 12,
        players: 1,
        world: "fresh".into(),
        fly_seconds: 0.0,
        fly_speed: DEFAULT_FLY_SPEED,
        generation_threads: 0,
        label: String::new(),
        json: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().unwrap_or_else(|| panic!("{flag} needs a value"));
        match flag.as_str() {
            "--server-exe" => args.server_exe = Some(value().into()),
            "--addr" => args.addr = Some(value().parse().expect("--addr host:port")),
            "--generator" => args.generator = value(),
            "--seed" => args.seed = value().parse().expect("--seed integer"),
            "--radius" => args.radius = value().parse().expect("--radius integer"),
            "--players" => args.players = value().parse().expect("--players integer"),
            "--world" => args.world = value(),
            "--fly-seconds" => args.fly_seconds = value().parse().expect("--fly-seconds number"),
            "--fly-speed" => args.fly_speed = value().parse().expect("--fly-speed number"),
            "--generation-threads" => {
                args.generation_threads = value().parse().expect("--generation-threads integer")
            }
            "--label" => args.label = value(),
            "--json" => args.json = Some(value().into()),
            other => panic!("unknown argument {other}"),
        }
    }
    assert!(
        args.server_exe.is_some() != args.addr.is_some(),
        "give exactly one of --server-exe or --addr"
    );
    assert!(
        matches!(args.world.as_str(), "fresh" | "generated"),
        "--world is fresh or generated"
    );
    assert!(
        args.addr.is_none() || args.world == "fresh",
        "--world generated needs --server-exe (the benchmark restarts the server)"
    );
    args
}

#[derive(Debug, Default, Serialize)]
struct PlayerResult {
    name: String,
    /// RakNet connected to `join` returned (`PlayStatus(PlayerSpawn)` seen);
    /// every time below shares this origin.
    join_ms: f64,
    /// First `LevelChunk` (chunks that arrive during the join are only
    /// visible to the session when it returns, so this is at least `join_ms`
    /// in that case).
    first_chunk_ms: Option<f64>,
    /// Every chunk of the client's render disk received.
    disk_view_ms: Option<f64>,
    /// Every chunk of the streamed square received.
    square_view_ms: Option<f64>,
    disk_chunks: usize,
    square_chunks: usize,
    /// `LevelChunk` packets received until the square view completed. A
    /// column counts as loaded once its requested sub-chunks have arrived.
    chunks_received: usize,
    /// Radius the server granted (`ChunkRadiusUpdated`).
    granted_radius: i32,
    /// Render-disk columns still missing when the view bound expired.
    disk_missing_at_timeout: Option<usize>,
    /// Distinct complete columns when standing ended (shows whether the
    /// server streams the square or the disk).
    columns_received: usize,
    /// Uncompressed `LevelChunk` and `SubChunk` packet bytes received until
    /// the square view completed.
    terrain_bytes: usize,
    flight: Option<FlightResult>,
}

#[derive(Debug, Default, Serialize)]
struct FlightResult {
    seconds: f32,
    speed: f32,
    blocks_flown: f32,
    /// New `LevelChunk`s per second while flying.
    chunks_per_second: f64,
    /// Share of the render disk around the player still missing, sampled
    /// every input tick: mean and worst.
    missing_disk_mean: f64,
    missing_disk_max: f64,
    /// Nearest missing chunk (in chunks from the player) at the worst sample.
    nearest_missing_at_worst: Option<f64>,
    /// After stopping: time until the render disk is complete again.
    catch_up_ms: Option<f64>,
}

#[derive(Debug, Serialize)]
struct RunResult {
    label: String,
    server: String,
    generator: String,
    seed: i64,
    radius: i32,
    players: usize,
    world: String,
    generation_threads: usize,
    players_result: Vec<PlayerResult>,
    /// Slowest player's disk view, the number a group of players feels.
    worst_disk_view_ms: Option<f64>,
    worst_square_view_ms: Option<f64>,
}

fn main() {
    let args = parse_args();
    let result = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .thread_stack_size(64 << 20)
                .enable_all()
                .build()
                .expect("benchmark runtime")
                .block_on(run(args))
        })
        .expect("benchmark thread")
        .join()
        .expect("benchmark panicked");
    let text = serde_json::to_string_pretty(&result).expect("serialize result");
    println!("{text}");
}

async fn run(args: Args) -> RunResult {
    let players_result;
    let server_name;
    match (&args.server_exe, args.addr) {
        (Some(exe), None) => {
            server_name = format!("unastar {}", exe.display());
            let dir = tempfile::Builder::new()
                .prefix("unastar-load-bench-")
                .tempdir()
                .expect("scratch directory");
            let addr = free_loopback_port();
            write_config(dir.path(), &args, addr);
            if args.world == "generated" {
                let mut server = ServerProcess::start(exe, dir.path());
                eprintln!("warm-up: generating the views once");
                let _ = measure_players(addr, &args).await;
                server.stop();
            }
            let mut server = ServerProcess::start(exe, dir.path());
            players_result = measure_players(addr, &args).await;
            server.stop();
        }
        (None, Some(addr)) => {
            server_name = format!("external {addr}");
            players_result = measure_players(addr, &args).await;
        }
        _ => unreachable!("argument check"),
    }
    let worst = |f: fn(&PlayerResult) -> Option<f64>| {
        players_result
            .iter()
            .map(f)
            .collect::<Option<Vec<_>>>()
            .and_then(|all| all.into_iter().reduce(f64::max))
    };
    let result = RunResult {
        label: args.label.clone(),
        server: server_name,
        generator: args.generator.clone(),
        seed: args.seed,
        radius: args.radius,
        players: args.players,
        world: args.world.clone(),
        generation_threads: args.generation_threads,
        worst_disk_view_ms: worst(|p| p.disk_view_ms),
        worst_square_view_ms: worst(|p| p.square_view_ms),
        players_result,
    };
    if let Some(path) = &args.json {
        let mut runs: Vec<serde_json::Value> = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        runs.push(serde_json::to_value(&result).expect("result json"));
        std::fs::write(
            path,
            serde_json::to_string_pretty(&runs).expect("json") + "\n",
        )
        .expect("write --json file");
    }
    result
}

fn free_loopback_port() -> SocketAddr {
    let port = std::net::UdpSocket::bind("127.0.0.1:0")
        .and_then(|socket| socket.local_addr())
        .expect("free loopback port")
        .port();
    ([127, 0, 0, 1], port).into()
}

fn write_config(dir: &Path, args: &Args, addr: SocketAddr) {
    let mut config = unastar::UnastarConfig::default();
    config.server.bind_address = addr.to_string();
    config.server.online_mode = false;
    config.server.max_players = args.players as u32 + 1;
    config.server.max_chunk_radius = args.radius;
    config.server.max_chunk_radius = args.radius;
    config.server.default_gamemode = unastar::entity::components::GameMode::Creative;
    config.players.data_dir = dir.join("playerdata");
    config.world.generation_threads = args.generation_threads;
    config.world.generator = match args.generator.as_str() {
        "java" => unastar::world::WorldGenerator::Java { seed: args.seed },
        "bedrock" => unastar::world::WorldGenerator::Bedrock { seed: args.seed },
        other => panic!("unsupported --generator {other} for this build"),
    };
    config.validate().expect("benchmark config is valid");
    let text = toml_edit::ser::to_string_pretty(&config).expect("serialize config");
    std::fs::write(dir.join("unastar.toml"), text).expect("write config");
}

/// A release server in its own process: the production runtime, isolated
/// from the clients' CPU accounting.
struct ServerProcess {
    child: Child,
}

impl ServerProcess {
    fn start(exe: &Path, dir: &Path) -> Self {
        let child = Command::new(exe)
            .current_dir(dir)
            .env("UNASTAR_CONFIG", dir.join("unastar.toml"))
            .env(
                "RUST_LOG",
                std::env::var("LOAD_BENCH_SERVER_LOG").unwrap_or_else(|_| "warn".into()),
            )
            .stdin(Stdio::piped())
            // `LOAD_BENCH_SERVER_LOG=<filter>` with `LOAD_BENCH_SERVER_STDOUT=<file>`
            // keeps the server's tracing output for diagnosis.
            .stdout(match std::env::var("LOAD_BENCH_SERVER_STDOUT") {
                Ok(path) => Stdio::from(
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                        .expect("server log file"),
                ),
                Err(_) => Stdio::null(),
            })
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start the server executable");
        Self { child }
    }

    /// `stop` on the console saves players and chunks, then exits.
    fn stop(&mut self) {
        if let Some(stdin) = self.child.stdin.as_mut() {
            let _ = stdin.write_all(b"stop\n");
            let _ = stdin.flush();
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(120);
        loop {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            if std::time::Instant::now() > deadline {
                eprintln!("server did not stop; killing it");
                let _ = self.child.kill();
                let _ = self.child.wait();
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

async fn measure_players(addr: SocketAddr, args: &Args) -> Vec<PlayerResult> {
    let tasks: Vec<_> = (0..args.players)
        .map(|index| {
            let args = args.clone();
            tokio::spawn(async move { measure_player(addr, &args, index).await })
        })
        .collect();
    let mut results = Vec::new();
    for task in tasks {
        results.push(task.await.expect("player task"));
    }
    results
}

/// The eight compass directions players fly in, one per player.
fn flight_direction(index: usize) -> (f32, f32) {
    let angle = index as f32 * std::f32::consts::FRAC_PI_4;
    (angle.cos(), angle.sin())
}

/// Connects (retrying while the server starts) and joins. Returns when the
/// RakNet connection was established, and the joined session.
async fn connect(
    addr: SocketAddr,
    radius: i32,
    name: &str,
) -> (
    Instant,
    BedrockStream<Play, Client, RakNetTransport>,
    jolyne::GameData,
) {
    let started = Instant::now();
    loop {
        let config = ClientConfig {
            chunk_radius: radius,
            ..ClientConfig::default()
        };
        match tokio::time::timeout(
            Duration::from_secs(3),
            BedrockStream::connect_with(addr, config),
        )
        .await
        {
            Ok(Ok(stream)) => {
                let connected = Instant::now();
                let (play, game_data) = stream
                    .join(ClientHandshakeConfig::random(addr, name))
                    .await
                    .expect("join completes");
                return (connected, play, game_data);
            }
            Ok(Err(_)) | Err(_) if started.elapsed() < Duration::from_secs(60) => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Ok(Err(error)) => panic!("could not connect to {addr}: {error}"),
            Err(_) => panic!("could not connect to {addr}: timed out"),
        }
    }
}

/// What one client knows about its terrain.
struct Terrain {
    received: HashSet<(i32, i32)>,
    radius: i32,
}

impl Terrain {
    fn square(&self, center: (i32, i32)) -> impl Iterator<Item = (i32, i32)> + '_ {
        let r = self.radius;
        (-r..=r).flat_map(move |dx| (-r..=r).map(move |dz| (center.0 + dx, center.1 + dz)))
    }

    fn disk(&self, center: (i32, i32)) -> impl Iterator<Item = (i32, i32)> + '_ {
        let r = self.radius;
        self.square(center)
            .filter(move |&(x, z)| (x - center.0).pow(2) + (z - center.1).pow(2) <= r * r)
    }

    fn missing_disk(&self, center: (i32, i32)) -> (usize, usize, Option<f64>) {
        let mut total = 0;
        let mut missing = 0;
        let mut nearest: Option<f64> = None;
        for chunk in self.disk(center) {
            total += 1;
            if !self.received.contains(&chunk) {
                missing += 1;
                let d = f64::from((chunk.0 - center.0).pow(2) + (chunk.1 - center.1).pow(2)).sqrt();
                nearest = Some(nearest.map_or(d, |n| n.min(d)));
            }
        }
        (missing, total, nearest)
    }
}

enum TerrainEvent {
    /// A `LevelChunk` arrived (its terrain may still be requested).
    Header,
    /// Columns whose terrain is now complete.
    Complete(Vec<(i32, i32)>),
}

/// Handles terrain packets like a client: a `LevelChunk` in sub-chunk request
/// mode queues its column's sub-chunks, from the bottom up to the server's
/// limit, for the next [`flush_requests`]; the column is complete when the
/// `SubChunk` answer arrives. A `LevelChunk` that carries its sub-chunks,
/// or whose limit is zero, is complete on arrival.
async fn terrain_packet(
    pending: &mut Vec<((i32, i32), i32)>,
    raw: jolyne::raw::RawPacket,
) -> Option<TerrainEvent> {
    match raw.name()? {
        McpePacketName::LevelChunkPacket => {
            let chunk = match raw.decode() {
                Ok(packet) => match packet.data {
                    McpePacketData::LevelChunkPacket(chunk) => chunk,
                    _ => return None,
                },
                Err(error) => {
                    eprintln!("undecodable LevelChunk: {error}");
                    return None;
                }
            };
            if std::env::var_os("LOAD_BENCH_TRACE").is_some() {
                eprintln!(
                    "LevelChunk {:?} count {} limit {:?}",
                    chunk.chunk_position,
                    chunk.subchunks_count,
                    chunk.client_request_sub_chunk_limit
                );
            }
            let column = (chunk.chunk_position.x, chunk.chunk_position.z);
            // Without a request limit the chunk carries its sub-chunks.
            let limit = chunk.client_request_sub_chunk_limit.unwrap_or(0);
            if limit <= 0 {
                return Some(TerrainEvent::Complete(vec![column]));
            }
            pending.push((column, limit.min(SUBCHUNKS_PER_COLUMN)));
            Some(TerrainEvent::Header)
        }
        McpePacketName::SubChunkPacket => {
            let answer = match raw.decode() {
                Ok(packet) => match packet.data {
                    McpePacketData::SubChunkPacket(answer) => answer,
                    _ => return None,
                },
                Err(error) => {
                    eprintln!("undecodable SubChunk: {error}");
                    return None;
                }
            };
            if std::env::var_os("LOAD_BENCH_TRACE").is_some() {
                let mut results = std::collections::BTreeMap::new();
                for entry in &answer.sub_chunk_data {
                    *results
                        .entry(format!("{:?}", entry.sub_chunk_request_result))
                        .or_insert(0) += 1;
                }
                eprintln!("SubChunk at {:?}: {results:?}", answer.center_pos);
            }
            let center = &answer.center_pos;
            let mut columns: Vec<(i32, i32)> = answer
                .sub_chunk_data
                .iter()
                .map(|entry| {
                    (
                        center.subchunk_position_x
                            + i32::from(entry.sub_chunk_pos_offset.subchunk_offset_x),
                        center.subchunk_position_z
                            + i32::from(entry.sub_chunk_pos_offset.subchunk_offset_z),
                    )
                })
                .collect();
            columns.sort_unstable();
            columns.dedup();
            Some(TerrainEvent::Complete(columns))
        }
        _ => None,
    }
}

/// Sends the queued sub-chunk requests of this client tick, relative to the
/// player's chunk, at most [`MAX_SUBCHUNK_OFFSETS`] offsets per packet.
async fn flush_requests(
    play: &mut BedrockStream<Play, Client, RakNetTransport>,
    pending: &mut Vec<((i32, i32), i32)>,
    center: (i32, i32),
) {
    use jolyne::valentine::SubChunkRequestPacket;
    use jolyne::valentine::types::{
        DimensionType, SubChunkPacketPayloadSubChunkPosOffset, SubChunkPos,
    };
    let offsets: Vec<SubChunkPacketPayloadSubChunkPosOffset> = pending
        .drain(..)
        .flat_map(|((x, z), limit)| {
            let dx = i8::try_from(x - center.0).expect("column within 127 chunks");
            let dz = i8::try_from(z - center.1).expect("column within 127 chunks");
            (0..limit).map(move |dy| SubChunkPacketPayloadSubChunkPosOffset {
                subchunk_offset_x: dx,
                subchunk_offset_y: dy as i8,
                subchunk_offset_z: dz,
            })
        })
        .collect();
    for batch in offsets.chunks(MAX_SUBCHUNK_OFFSETS) {
        let request = SubChunkRequestPacket {
            dimension_type: DimensionType { value: 0 },
            sub_chunk_position_offset_list: batch.to_vec(),
            center_pos: SubChunkPos {
                subchunk_position_x: center.0,
                subchunk_position_y: LOWEST_SUBCHUNK,
                subchunk_position_z: center.1,
            },
        };
        let _ = play.send_packet(McpePacket::from(request)).await;
    }
}

async fn measure_player(addr: SocketAddr, args: &Args, index: usize) -> PlayerResult {
    let name = format!("LoadBot{index}");
    let (started, mut play, game_data) =
        tokio::time::timeout(VIEW_TIMEOUT, connect(addr, args.radius, &name))
            .await
            .expect("join within the timeout");
    let join_ms = ms(started.elapsed());
    let radius = play.chunk_radius().unwrap_or(args.radius);
    let spawn = game_data.start_game.position.clone();
    let mut result = PlayerResult {
        name,
        join_ms,
        ..PlayerResult::default()
    };
    let mut terrain = Terrain {
        received: HashSet::new(),
        radius,
    };
    result.granted_radius = radius;
    let center = |x: f32, z: f32| ((x.floor() as i32) >> 4, (z.floor() as i32) >> 4);
    let spawn_chunk = center(spawn.x, spawn.z);
    result.square_chunks = terrain.square(spawn_chunk).count();
    result.disk_chunks = terrain.disk(spawn_chunk).count();
    let mut position = spawn.clone();
    let mut tick: u64 = 0;
    let mut input = tokio::time::interval(INPUT_INTERVAL);
    // Sub-chunk requests queued since the last client tick.
    let mut requests = Vec::new();

    // Standing at spawn until the whole square view has arrived, or the disk
    // has and nothing more came for `SQUARE_GRACE` (a server that streams
    // only the render disk).
    let deadline = started + VIEW_TIMEOUT;
    let mut last_column = Instant::now();
    while result.square_view_ms.is_none()
        && !(result.disk_view_ms.is_some() && last_column.elapsed() > SQUARE_GRACE)
    {
        tokio::select! {
            raw = play.recv_packet_raw() => {
                let raw = raw.expect("connection stays open while loading");
                if matches!(
                    raw.name(),
                    Some(McpePacketName::LevelChunkPacket | McpePacketName::SubChunkPacket)
                ) {
                    result.terrain_bytes += raw.inner_frame().len();
                }
                let Some(event) = terrain_packet(&mut requests, raw).await else { continue };
                let now = ms(started.elapsed());
                match event {
                    TerrainEvent::Header => {
                        result.first_chunk_ms.get_or_insert(now);
                        result.chunks_received += 1;
                    }
                    TerrainEvent::Complete(columns) => {
                        last_column = Instant::now();
                        terrain.received.extend(columns);
                        if result.disk_view_ms.is_none()
                            && terrain.disk(spawn_chunk).all(|c| terrain.received.contains(&c))
                        {
                            result.disk_view_ms = Some(now);
                        }
                        if terrain.square(spawn_chunk).all(|c| terrain.received.contains(&c)) {
                            result.square_view_ms = Some(now);
                        }
                    }
                }
            }
            _ = input.tick() => {
                tick += 1;
                send_input(&mut play, &mut position, (0.0, 0.0), tick).await;
                flush_requests(&mut play, &mut requests, spawn_chunk).await;
            }
        }
        if Instant::now() >= deadline {
            let (missing, _, nearest) = terrain.missing_disk(spawn_chunk);
            eprintln!(
                "{}: render disk incomplete after {VIEW_TIMEOUT:?}: {missing} columns missing, nearest {nearest:?} chunks away",
                result.name
            );
            result.disk_missing_at_timeout = Some(missing);
            break;
        }
    }
    result.columns_received = terrain.received.len();

    if args.fly_seconds > 0.0 {
        result.flight = Some(
            fly(
                &mut play,
                &mut terrain,
                &mut position,
                &mut tick,
                args,
                index,
            )
            .await,
        );
    }
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        play.disconnect(
            jolyne::valentine::EnumsConnectionDisconnectFailReason::Disconnected,
            "",
        ),
    )
    .await;
    result
}

async fn fly(
    play: &mut BedrockStream<Play, Client, RakNetTransport>,
    terrain: &mut Terrain,
    position: &mut Vec3,
    tick: &mut u64,
    args: &Args,
    index: usize,
) -> FlightResult {
    let (dx, dz) = flight_direction(index);
    let step = args.fly_speed * INPUT_INTERVAL.as_secs_f32();
    // Fly well above the terrain.
    position.y += 40.0;
    let start = position.clone();
    let center = |p: &Vec3| ((p.x.floor() as i32) >> 4, (p.z.floor() as i32) >> 4);
    let flight_started = Instant::now();
    let mut new_chunks = 0usize;
    let mut samples = 0usize;
    let mut missing_sum = 0.0;
    let mut worst = (0.0f64, None);
    let mut requests = Vec::new();
    let mut input = tokio::time::interval(INPUT_INTERVAL);
    let flying_until = flight_started + Duration::from_secs_f32(args.fly_seconds);
    let mut stopped_at: Option<Instant> = None;
    let mut catch_up_ms = None;
    let deadline = flying_until + VIEW_TIMEOUT;
    loop {
        tokio::select! {
            raw = play.recv_packet_raw() => {
                let raw = raw.expect("connection stays open while flying");
                if let Some(TerrainEvent::Complete(columns)) = terrain_packet(&mut requests, raw).await {
                    for column in columns {
                        if terrain.received.insert(column) && stopped_at.is_none() {
                            new_chunks += 1;
                        }
                    }
                }
            }
            _ = input.tick() => {
                *tick += 1;
                let flying = Instant::now() < flying_until;
                let velocity = if flying { (dx * step, dz * step) } else { (0.0, 0.0) };
                send_input(play, position, velocity, *tick).await;
                flush_requests(play, &mut requests, center(position)).await;
                let (missing, total, nearest) = terrain.missing_disk(center(position));
                if flying {
                    let share = missing as f64 / total as f64;
                    samples += 1;
                    missing_sum += share;
                    if share > worst.0 {
                        worst = (share, nearest);
                    }
                } else {
                    let stopped = *stopped_at.get_or_insert_with(Instant::now);
                    if missing == 0 {
                        catch_up_ms = Some(ms(stopped.elapsed()));
                        break;
                    }
                }
            }
        }
        if Instant::now() > deadline {
            break;
        }
    }
    let flown = ((position.x - start.x).powi(2) + (position.z - start.z).powi(2)).sqrt();
    FlightResult {
        seconds: args.fly_seconds,
        speed: args.fly_speed,
        blocks_flown: flown,
        chunks_per_second: new_chunks as f64 / f64::from(args.fly_seconds),
        missing_disk_mean: if samples == 0 {
            0.0
        } else {
            missing_sum / samples as f64
        },
        missing_disk_max: worst.0,
        nearest_missing_at_worst: worst.1,
        catch_up_ms,
    }
}

async fn send_input(
    play: &mut BedrockStream<Play, Client, RakNetTransport>,
    position: &mut Vec3,
    velocity: (f32, f32),
    tick: u64,
) {
    position.x += velocity.0;
    position.z += velocity.1;
    let moving = velocity != (0.0, 0.0);
    let yaw = (-velocity.0).atan2(velocity.1).to_degrees();
    let mut input_data = Vec::new();
    if moving {
        input_data.push(EnumsPlayerAuthInputPacketPayloadInputData::Up);
    }
    let _ = play
        .send_packet(McpePacket::from(PlayerAuthInputPacket {
            player_rotation: Vec2 { x: 0.0, y: yaw },
            position: position.clone(),
            move_vector: Vec2 {
                x: 0.0,
                y: if moving { 1.0 } else { 0.0 },
            },
            player_head_rotation: yaw,
            input_data,
            client_tick: PlayerInputTick { inputtick: tick },
            pos_delta: Vec3 {
                x: velocity.0,
                y: 0.0,
                z: velocity.1,
            },
            ..Default::default()
        }))
        .await;
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e3
}
