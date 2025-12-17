//! Example: LAN Discovery Server for Minecraft Bedrock
//!
//! This example creates a NetherNet server visible to real MCPE clients on LAN.
//!
//! Run with: `cargo run --example discovery_server --features discovery`
//!
//! For localhost testing between two Rust examples, use: `--localhost`

use std::env;

use futures::StreamExt;
use tokio_nethernet::{
    NetherNetListener, NetherNetListenerConfig,
    discovery::{
        DEFAULT_PORT, DiscoveryListener, DiscoveryListenerConfig, ServerData, TransportLayer,
    },
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("tokio_nethernet=debug".parse()?),
        )
        .init();

    println!("=== NetherNet LAN Discovery Server ===\n");

    let localhost_mode = env::args().any(|a| a == "--localhost");

    // For real MCPE: bind to 0.0.0.0:7551, broadcast to 255.255.255.255:7551
    // For localhost testing: use separate ports to avoid binding conflicts
    let config = if localhost_mode {
        println!("[LOCALHOST MODE] Using separate ports for same-machine testing\n");
        DiscoveryListenerConfig {
            broadcast_addr: "127.0.0.1:7552".parse().unwrap(),
            ..Default::default()
        }
    } else {
        // Real LAN mode - MCPE clients broadcast to port 7551
        DiscoveryListenerConfig::default()
    };

    let bind_addr = if localhost_mode {
        "127.0.0.1:7551"
    } else {
        // Bind to all interfaces on port 7551 for real MCPE
        "0.0.0.0:7551"
    };

    let network_id = config.network_id;
    let discovery = DiscoveryListener::bind(bind_addr, config).await?;

    // Set server info using the new builder API
    discovery
        .set_server_data(
            ServerData::builder()
                .name("Rust NetherNet Server")
                .level("Test World")
                .players(1, 10)
                .transport(TransportLayer::NetherNet)
                .build(),
        )
        .await;

    println!("Bound to: {}", bind_addr);
    println!("Network ID: {}", network_id);
    println!("Discovery port: {}\n", DEFAULT_PORT);

    if localhost_mode {
        println!("Run client with: --localhost");
    } else {
        println!("Real MCPE clients should see this server in LAN Games!");
        println!("If not, check firewall allows UDP port 7551.");
    }

    // ✓ NEW API: One-liner listener creation - no manual signal pump!
    let mut listener =
        NetherNetListener::bind_with_signaling(discovery, NetherNetListenerConfig::default());

    println!("\nWaiting for connections...\n");

    // Accept connections
    loop {
        match listener.accept().await {
            Ok(mut stream) => {
                println!("✓ New connection established!");

                tokio::spawn(async move {
                    while let Some(result) = stream.next().await {
                        match result {
                            Ok(msg) => {
                                let channel = if msg.reliable {
                                    "reliable"
                                } else {
                                    "unreliable"
                                };
                                println!("[RECV {}] {} bytes", channel, msg.buffer.len());
                            }
                            Err(e) => {
                                eprintln!("Error: {}", e);
                                break;
                            }
                        }
                    }
                    println!("Connection closed.");
                });
            }
            Err(e) => {
                eprintln!("Accept error: {}", e);
                break;
            }
        }
    }

    Ok(())
}
