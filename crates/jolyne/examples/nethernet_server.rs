//! Example: NetherNet (WebRTC/LAN) Server using Jolyne
//!
//! This example demonstrates using NetherNet transport instead of RakNet.
//! NetherNet uses WebRTC for transport, which provides its own DTLS encryption,
//! so we set `encryption_enabled: false` in the config.
//!
//! Run with: `cargo run -p jolyne --example nethernet_server --features discovery -- --localhost`

use jolyne::config::BedrockListenerConfig;
use jolyne::{BedrockListener, WorldTemplate};
use p384::SecretKey;
use rand::thread_rng;
use std::env;
use std::sync::Arc;
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

    let bind_addr = if localhost_mode {
        println!("[LOCALHOST MODE]\n");
        "127.0.0.1:7551"
    } else {
        "0.0.0.0:7551"
    };

    // Bedrock config with encryption DISABLED (WebRTC/DTLS handles it)
    let bedrock_config = BedrockListenerConfig {
        encryption_enabled: false,
        online_mode: false,
        ..Default::default()
    };

    // Create listener with the new builder API
    let mut listener = BedrockListener::nethernet()
        .lan(bind_addr)
        .config(bedrock_config)
        .bind()
        .await?;

    info!("NetherNet server bound to: {}", bind_addr);
    println!("Waiting for connections...\n");

    // Prepare static data
    let template = Arc::new(WorldTemplate::default());
    let server_key = SecretKey::random(&mut thread_rng());

    loop {
        match listener.accept().await {
            Ok(handshake_stream) => {
                info!("NetherNet connection established!");

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
