//! Stress test: spawns multiple clients to test server capacity.
//!
//! Usage:
//!   cargo run --example stress_test -- --clients 10 --duration 30
//!
//! This creates N bot clients that connect to the server, complete the join
//! sequence, and stay connected for the specified duration. Useful for testing
//! multi-player broadcasting and server capacity.

use clap::Parser;
use jolyne::BedrockStream;
use jolyne::stream::client::ClientHandshakeConfig;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{Level, error, info};
use tracing_subscriber::FmtSubscriber;

#[derive(Parser, Debug)]
#[command(name = "stress_test")]
#[command(about = "Stress test a Bedrock server with multiple bot clients")]
struct Args {
    /// Target server address
    #[arg(short, long, default_value = "127.0.0.1:19132")]
    addr: String,

    /// Number of clients to spawn
    #[arg(short, long, default_value = "10")]
    clients: u32,

    /// Delay between client spawns (milliseconds)
    #[arg(short, long, default_value = "100")]
    delay_ms: u64,

    /// Duration to keep clients connected (seconds)
    #[arg(long, default_value = "30")]
    duration: u64,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Setup logging
    let level = match args.log_level.to_lowercase().as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        _ => Level::INFO,
    };

    FmtSubscriber::builder()
        .with_max_level(level)
        .with_target(false)
        .compact()
        .init();

    let addr: SocketAddr = args.addr.parse()?;
    info!(
        "Stress testing {} with {} clients for {}s...",
        addr, args.clients, args.duration
    );

    let connected = Arc::new(AtomicU32::new(0));
    let failed = Arc::new(AtomicU32::new(0));
    let start_time = Instant::now();

    let mut handles = vec![];

    for i in 0..args.clients {
        let name = format!("StressBot_{}", i);
        let connected = connected.clone();
        let failed = failed.clone();
        let duration = args.duration;

        let handle = tokio::spawn(async move {
            match spawn_client(addr, &name, duration).await {
                Ok(_) => {
                    connected.fetch_add(1, Ordering::Relaxed);
                    info!("[{}] Connected successfully", name);
                }
                Err(e) => {
                    failed.fetch_add(1, Ordering::Relaxed);
                    error!("[{}] Failed: {:?}", name, e);
                }
            }
        });

        handles.push(handle);
        sleep(Duration::from_millis(args.delay_ms)).await;
    }

    // Wait for all clients to complete
    for handle in handles {
        let _ = handle.await;
    }

    let elapsed = start_time.elapsed();
    let total_connected = connected.load(Ordering::Relaxed);
    let total_failed = failed.load(Ordering::Relaxed);

    info!("=== Stress Test Complete ===");
    info!("Total clients spawned: {}", args.clients);
    info!("Successfully connected: {}", total_connected);
    info!("Failed: {}", total_failed);
    info!("Total time: {:.2?}", elapsed);
    info!(
        "Connection rate: {:.2} clients/sec",
        total_connected as f64 / elapsed.as_secs_f64()
    );

    Ok(())
}

async fn spawn_client(addr: SocketAddr, name: &str, duration: u64) -> anyhow::Result<()> {
    use jolyne::valentine::{PlayerAuthInputPacket, PlayerInputTick, Vec2, Vec3};

    // Connect
    let handshake = BedrockStream::connect(addr).await?;

    // Configure with random identity
    let config = ClientHandshakeConfig::random(addr, name);

    // Join (handles settings, auth, encryption, packs, start game)
    let (mut play, _game_data) = handshake.join(config).await?;

    // Stay connected and simulate movement for the specified duration
    let deadline = Instant::now() + Duration::from_secs(duration);
    let mut tick: i64 = 0;

    // Spawn position + circular walk with radius 3 blocks
    let spawn_x: f32 = 0.5;
    let spawn_z: f32 = 0.5;
    let walk_radius: f32 = 3.0;
    let rotation_speed: f32 = 0.1; // radians per tick

    while Instant::now() < deadline {
        // Send movement input every ~50ms (20 TPS)
        sleep(Duration::from_millis(50)).await;
        tick += 1;

        // Circular walk around spawn point
        let angle = (tick as f32) * rotation_speed;
        let pos_x = spawn_x + angle.cos() * walk_radius;
        let pos_z = spawn_z + angle.sin() * walk_radius;

        // Calculate velocity for server-side physics simulation
        let delta_x = -angle.sin() * walk_radius * rotation_speed;
        let delta_z = angle.cos() * walk_radius * rotation_speed;

        let input_packet = PlayerAuthInputPacket {
            player_rotation: Vec2 {
                x: 0.0,
                y: angle.to_degrees(),
            },
            position: Vec3 {
                x: pos_x,
                y: 17.62, // Eye level (ground Y=16 + 1.62 eye height)
                z: pos_z,
            },
            move_vector: Vec2 {
                x: angle.cos(),
                y: angle.sin(),
            },
            player_head_rotation: angle.to_degrees(),
            input_data: None,
            input_mode: Default::default(),
            play_mode: Default::default(),
            new_interaction_model: Default::default(),
            interact_rotation: Vec2::default(),
            client_tick: PlayerInputTick {
                inputtick: tick as u64,
            },
            pos_delta: Vec3 {
                x: delta_x,
                y: 0.0,
                z: delta_z,
            },
            item_use_transaction: None,
            item_stack_request: None,
            player_block_actions: None,
            vehicle_rotation: None,
            client_predicted_vehicle: None,
            analog_move_vector: Vec2::default(),
            camera_orientation: Vec3::default(),
            raw_move_vector: Vec2::default(),
        };

        if let Err(e) = play.send_packet(input_packet.into()).await {
            return Err(anyhow::anyhow!("Failed to send movement: {:?}", e));
        }

        // Drain ALL incoming packets to prevent RakNet backpressure
        // This is critical - if we don't read, the receive buffer fills up
        // and RakNet flow control throttles the connection
        loop {
            match tokio::time::timeout(Duration::from_millis(1), play.recv_packet()).await {
                Ok(Ok(_)) => continue, // Got a packet, try for more
                Ok(Err(e)) => {
                    return Err(anyhow::anyhow!("Connection error: {:?}", e));
                }
                Err(_) => break, // Timeout = no more packets waiting
            }
        }
    }

    info!("[{}] Duration complete, sending disconnect", name);

    // Send a clean disconnect packet
    use jolyne::valentine::{
        DisconnectPacket, DisconnectPacketMessages, EnumsConnectionDisconnectFailReason,
    };

    let disconnect = DisconnectPacket {
        reason: EnumsConnectionDisconnectFailReason::Unknown,
        messages: DisconnectPacketMessages::default(),
    };

    let _ = play.send_packet(disconnect.into()).await;
    let _ = play.flush().await;

    Ok(())
}
