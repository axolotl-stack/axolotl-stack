//! Transfer packet handling for Axelerator.
//!
//! Handles incoming WebRTC connections from Xbox Live friends and
//! redirects them to the actual Minecraft server.

// use std::net::SocketAddr;
use std::sync::Arc;

use jolyne::WorldTemplate;
use jolyne::config::BedrockListenerConfig;
use jolyne::protocol::{McpePacket, PacketTransfer};
use jolyne::stream::transport::{BedrockTransport, NetherNetTransport};
use jolyne::stream::{BedrockStream, Handshake, Play, Server};
use p384::SecretKey;
use rand::thread_rng;
use tokio_nethernet::{NetherNetListener, NetherNetListenerConfig, XboxSignaling};
use tracing::{debug, error, info, warn};

use crate::AxeleratorConfig;

/// Runs the mini-server that accepts WebRTC connections and transfers players.
///
/// This performs a minimal Bedrock handshake (just enough to send packets),
/// then sends a Transfer packet with the downstream server address.
/// Runs the mini-server that accepts WebRTC connections and transfers players.
///
/// This performs a minimal Bedrock handshake (just enough to send packets),
/// then sends a Transfer packet with the downstream server address.
pub async fn run_transfer_server(
    signaling: Arc<XboxSignaling>,
    config: &AxeleratorConfig,
) -> anyhow::Result<()> {
    // Create NetherNet listener with Xbox signaling
    // Use new() which returns (listener, signal_tx) for manual signal routing
    let (mut listener, signal_tx) =
        NetherNetListener::new(signaling.clone(), NetherNetListenerConfig::default());

    // Spawn signal pump to route signals from XboxSignaling to listener
    tokio::spawn(async move {
        loop {
            if let Some(signal) = signaling.recv().await {
                if signal_tx.send(signal).await.is_err() {
                    break;
                }
            } else {
                break;
            }
        }
    });

    // Setup for handshake
    let template = Arc::new(WorldTemplate::default());
    let server_key = SecretKey::random(&mut thread_rng());

    // Bedrock config - NetherNet uses DTLS, so no Bedrock encryption needed
    let bedrock_config = Arc::new(BedrockListenerConfig {
        encryption_enabled: false,
        online_mode: false,
        ..Default::default()
    });

    // Store target as struct to pass into handlers
    let target_host = config.server_ip.clone();
    let target_port = config.server_port;

    info!(
        downstream = %target_host,
        port = target_port,
        "Transfer server ready, waiting for friend connections..."
    );

    loop {
        match listener.accept().await {
            Ok(stream) => {
                info!("Friend connected via WebRTC!");

                let transport = BedrockTransport::new(NetherNetTransport::new(stream));
                let handshake_stream =
                    <BedrockStream<Handshake, Server, NetherNetTransport>>::from_transport(
                        transport,
                        bedrock_config.clone(),
                    );

                let template = template.clone();
                let key = server_key.clone();
                let host = target_host.clone();
                let port = target_port;

                tokio::spawn(async move {
                    handle_transfer_connection(handshake_stream, &template, &key, host, port).await;
                });
            }
            Err(e) => {
                error!("Accept error: {:?} - Listener shutting down", e);
                break;
            }
        }
    }

    Ok(())
}

/// Handles a single connection: performs handshake, resolves DNS, and sends transfer packet.
async fn handle_transfer_connection(
    handshake_stream: BedrockStream<Handshake, Server, NetherNetTransport>,
    template: &WorldTemplate,
    key: &SecretKey,
    target_host: String,
    target_port: u16,
) {
    debug!("Starting Bedrock handshake...");

    match handshake_stream.accept_join_sequence(template, key).await {
        Ok((mut play_stream, identity)) => {
            let display_name = identity.display_name.as_deref().unwrap_or("Unknown");

            info!(
                identity = %display_name,
                downstream = %target_host,
                port = target_port,
                "Transferring player to downstream server"
            );

            // Send Transfer packet with raw string (allows IP, Domain, or NetherNet ID)
            if let Err(e) = send_transfer_packet(&mut play_stream, &target_host, target_port).await
            {
                error!("Failed to send transfer packet: {:?}", e);
            } else {
                info!(identity = %display_name, "Transfer packet sent successfully!");
            }

            // Give client time to process the transfer
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        Err(e) => {
            warn!("Handshake failed: {:?}", e);
        }
    }
}

/// Sends a Transfer packet to redirect the client.
async fn send_transfer_packet(
    stream: &mut BedrockStream<Play, Server, NetherNetTransport>,
    addr_str: &str,
    port: u16,
) -> anyhow::Result<()> {
    // Create Transfer packet using generated protocol type
    let transfer = PacketTransfer {
        server_address: addr_str.to_string(),
        port,
        reload_world: false,
    };

    let packet = McpePacket::from(transfer);
    stream
        .send_batch_with_reliability(&[packet], true)
        .await
        .map_err(|e| anyhow::anyhow!("Send failed: {}", e))?;

    Ok(())
}
