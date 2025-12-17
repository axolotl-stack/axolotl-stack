//! Example: LAN Discovery Client
//!
//! This example shows how to discover servers on the local network
//! and connect to one using NetherNet.
//!
//! Run with: `cargo run --example discovery_client --features discovery -- --localhost`

use std::env;
use std::time::Duration;

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use tokio_nethernet::{
    Message, NetherNetDialer, NetherNetDialerConfig,
    discovery::{DiscoveryListener, DiscoveryListenerConfig, ServerData},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("tokio_nethernet=debug".parse()?),
        )
        .init();

    println!("=== NetherNet LAN Discovery Client ===\n");

    // Check for localhost testing mode
    let localhost_mode = env::args().any(|a| a == "--localhost");

    let (bind_addr, broadcast_addr) = if localhost_mode {
        ("127.0.0.1:7552", "127.0.0.1:7551")
    } else {
        ("0.0.0.0:0", "255.255.255.255:7551")
    };

    let config = DiscoveryListenerConfig {
        broadcast_addr: broadcast_addr.parse().unwrap(),
        broadcast_interval: Duration::from_millis(500),
        ..Default::default()
    };
    let discovery = DiscoveryListener::bind(bind_addr, config).await?;

    println!("Bound to {}", bind_addr);
    println!("Broadcasting to {}", broadcast_addr);

    if localhost_mode {
        println!("[LOCALHOST MODE] Make sure server is running with --localhost\n");
    }

    println!("Discovering servers...");

    // Wait for discovery broadcasts
    for i in 0..5 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let responses = discovery.responses().await;
        if !responses.is_empty() {
            break;
        }
        println!("  Scanning... ({}/5)", i + 1);
    }

    // Check for discovered servers
    let responses = discovery.responses().await;
    if responses.is_empty() {
        println!("\nNo servers found.");
        println!("Make sure discovery_server is running!");
        if !localhost_mode {
            println!("\nFor same-machine testing, use: --localhost");
        }
        return Ok(());
    }

    println!("\nFound {} server(s):", responses.len());
    for (network_id, data) in &responses {
        if let Some(server_data) = ServerData::decode(data) {
            println!(
                "  - {} (ID: {}) - {}/{} players - {}",
                server_data.server_name,
                network_id,
                server_data.player_count,
                server_data.max_player_count,
                server_data.level_name
            );
        } else {
            println!("  - Unknown server (ID: {})", network_id);
        }
    }

    // Connect to the first server
    let (server_id, _) = responses.iter().next().unwrap();
    println!("\nConnecting to server {}...", server_id);

    // ✓ NEW API: One-liner dialer creation - no manual signal pump!
    let dialer =
        NetherNetDialer::connect_with_signaling(discovery, NetherNetDialerConfig::default());

    // Dial the server
    let mut stream = dialer.dial(server_id.to_string()).await?;

    println!("✓ Connected!\n");

    // Send a test message
    println!("Sending test message...");
    stream
        .send(Message::reliable(Bytes::from("Hello from Rust client!")))
        .await?;
    println!("✓ Message sent\n");

    // Wait for responses
    println!("Waiting for responses (Ctrl+C to exit)...\n");
    while let Some(result) = stream.next().await {
        match result {
            Ok(msg) => {
                let channel = if msg.reliable {
                    "reliable"
                } else {
                    "unreliable"
                };
                println!(
                    "[RECV {}] {} bytes: {:?}",
                    channel,
                    msg.buffer.len(),
                    String::from_utf8_lossy(&msg.buffer)
                );
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                break;
            }
        }
    }

    Ok(())
}
