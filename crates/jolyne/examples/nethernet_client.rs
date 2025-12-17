//! Example: NetherNet (WebRTC/LAN) Client using Jolyne
//!
//! This example demonstrates connecting to a NetherNet server using LAN discovery.
//! NetherNet uses WebRTC, which provides DTLS encryption, so Bedrock-level
//! encryption is not needed.
//!
//! Run with: `cargo run -p jolyne --example nethernet_client --features discovery -- --localhost`

use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::stream::transport::{BedrockTransport, NetherNetTransport};
use jolyne::stream::{BedrockStream, Client, Handshake};
use p384::SecretKey;
use rand::thread_rng;
use std::env;
use std::net::SocketAddr;
use std::time::Duration;
use tokio_nethernet::{
    NetherNetDialer, NetherNetDialerConfig,
    discovery::{DiscoveryListener, DiscoveryListenerConfig, ServerData},
};
use tracing::info;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,jolyne=debug,tokio_nethernet=debug"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    println!("=== Jolyne NetherNet Client ===\n");

    // Check for localhost testing mode
    let localhost_mode = env::args().any(|a| a == "--localhost");

    let (bind_addr, broadcast_addr) = if localhost_mode {
        println!("[LOCALHOST MODE]\n");
        ("127.0.0.1:7552", "127.0.0.1:7551")
    } else {
        ("0.0.0.0:0", "255.255.255.255:7551")
    };

    // Create discovery client
    let discovery_config = DiscoveryListenerConfig {
        broadcast_addr: broadcast_addr.parse().unwrap(),
        broadcast_interval: Duration::from_millis(500),
        ..Default::default()
    };
    let discovery = DiscoveryListener::bind(bind_addr, discovery_config).await?;

    println!("Discovering servers...");

    // Wait for discovery
    for i in 0..5 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let responses = discovery.responses().await;
        if !responses.is_empty() {
            break;
        }
        println!("  Scanning... ({}/5)", i + 1);
    }

    // Check for servers
    let responses = discovery.responses().await;
    if responses.is_empty() {
        println!("\nNo servers found. Run nethernet_server first!");
        return Ok(());
    }

    println!("\nFound {} server(s):", responses.len());
    for (network_id, data) in &responses {
        if let Some(server_data) = ServerData::decode(data) {
            println!(
                "  - {} (ID: {}) - {}/{} players",
                server_data.server_name,
                network_id,
                server_data.player_count,
                server_data.max_player_count,
            );
        }
    }

    // Connect to first server
    let (server_id, _) = responses.iter().next().unwrap();
    println!("\nConnecting to server {}...", server_id);

    // Create dialer with ergonomic API
    let dialer =
        NetherNetDialer::connect_with_signaling(discovery, NetherNetDialerConfig::default());

    let stream = dialer.dial(server_id.to_string()).await?;
    println!("✓ NetherNet connected!\n");

    // Wrap in transport layer and create BedrockStream using constructor
    let transport = BedrockTransport::new(NetherNetTransport::new(stream));
    let handshake_stream =
        <BedrockStream<Handshake, Client, NetherNetTransport>>::from_transport(transport);

    // Client config - server_addr is a placeholder for NetherNet (WebRTC has no traditional address)
    let client_config = ClientHandshakeConfig {
        server_addr: "0.0.0.0:19132".parse::<SocketAddr>().unwrap(),
        display_name: "NetherNet Client".to_string(),
        uuid: Uuid::new_v4(),
        identity_key: SecretKey::random(&mut thread_rng()),
    };

    // Join the server
    info!("Starting Jolyne handshake...");
    let mut play_stream = handshake_stream.join(client_config).await?;
    println!("✓ Joined game!\n");

    // Play loop
    println!("In-game! Receiving packets...\n");
    while let Ok(packet) = play_stream.recv_packet().await {
        info!(id=?packet.data.packet_id(), "Recv Packet");
    }

    println!("Disconnected.");
    Ok(())
}
