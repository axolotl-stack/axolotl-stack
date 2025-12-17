//! Example: NetherNet (WebRTC/LAN) Server using Jolyne
//!
//! This example demonstrates using NetherNet transport instead of RakNet.
//! NetherNet uses WebRTC for transport, which provides its own DTLS encryption,
//! so we set `encryption_enabled: false` in the config.
//!
//! Run with: `cargo run -p jolyne --example nethernet_server --features discovery -- --localhost`

use jolyne::WorldTemplate;
use jolyne::config::BedrockListenerConfig;
use jolyne::stream::transport::{BedrockTransport, NetherNetTransport};
use jolyne::stream::{BedrockStream, Handshake, Server};
use p384::SecretKey;
use rand::thread_rng;
use std::env;
use std::sync::Arc;
use tokio_nethernet::{
    NetherNetListener, NetherNetListenerConfig,
    discovery::{DiscoveryListener, DiscoveryListenerConfig, ServerData, TransportLayer},
};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,jolyne=debug,tokio_nethernet=debug"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    println!("=== Jolyne NetherNet Server ===\n");

    let localhost_mode = env::args().any(|a| a == "--localhost");

    // Discovery configuration
    let discovery_config = if localhost_mode {
        println!("[LOCALHOST MODE]\n");
        DiscoveryListenerConfig {
            broadcast_addr: "127.0.0.1:7552".parse().unwrap(),
            ..Default::default()
        }
    } else {
        DiscoveryListenerConfig::default()
    };

    let bind_addr = if localhost_mode {
        "127.0.0.1:7551"
    } else {
        "0.0.0.0:7551"
    };

    let network_id = discovery_config.network_id;
    let discovery = DiscoveryListener::bind(bind_addr, discovery_config).await?;

    // Set server data for LAN discovery
    discovery
        .set_server_data(
            ServerData::builder()
                .name("Jolyne NetherNet Server")
                .level("NetherNet World")
                .players(1, 10)
                .transport(TransportLayer::NetherNet)
                .build(),
        )
        .await;

    info!("Discovery bound to: {}", bind_addr);
    info!("Network ID: {}", network_id);

    // Create NetherNet listener with ergonomic API
    let mut listener =
        NetherNetListener::bind_with_signaling(discovery, NetherNetListenerConfig::default());

    // Prepare static data
    let template = Arc::new(WorldTemplate::default());
    let server_key = SecretKey::random(&mut thread_rng());

    // Bedrock config with encryption DISABLED (WebRTC/DTLS handles it)
    let bedrock_config = Arc::new(BedrockListenerConfig {
        encryption_enabled: false, // NetherNet uses DTLS, not Bedrock encryption
        online_mode: false,        // Use offline mode for testing
        ..Default::default()
    });

    println!("Waiting for connections...\n");

    loop {
        match listener.accept().await {
            Ok(stream) => {
                info!("NetherNet connection established!");

                // Wrap in transport layer and create BedrockStream using constructor
                let transport = BedrockTransport::new(NetherNetTransport::new(stream));
                let handshake_stream =
                    <BedrockStream<Handshake, Server, NetherNetTransport>>::from_transport(
                        transport,
                        bedrock_config.clone(),
                    );

                let template = template.clone();
                let key = server_key.clone();

                tokio::spawn(async move {
                    match handshake_stream.accept_join_sequence(&template, &key).await {
                        Ok((mut play_stream, identity)) => {
                            let display_name =
                                identity.display_name.as_deref().unwrap_or("Unknown");
                            info!(identity = %display_name, "Player joined via NetherNet!");

                            // Play Loop
                            while let Ok(packet) = play_stream.recv_packet().await {
                                info!(id=?packet.data.packet_id(), "Recv Packet");
                            }
                            info!("Player disconnected");
                        }
                        Err(e) => {
                            tracing::error!("Handshake failed: {:?}", e);
                        }
                    }
                });
            }
            Err(e) => {
                tracing::error!("Accept error: {:?}", e);
                break;
            }
        }
    }

    Ok(())
}
