use std::time::Duration;

use anyhow::{Context, Result};
use bytes::{Bytes, BytesMut};
use tokio::net::lookup_host;
use tokio::time::sleep;
use tokio_raknet::transport::RaknetStream;
use valentine::bedrock::{
    codec::BedrockCodec,
    context::BedrockSession,
    protocol::v1_21_130::{
        packets::PacketRequestNetworkSettings,
        types::mcpe::McpePacket,
        types::mcpe::McpePacketData, // Still need McpePacketData for matching
    },
};

const PROTOCOL_VERSION: i32 = 860;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let host = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "play.lbsg.net:19132".to_string());

    println!("Resolving {}...", host);
    let mut addrs = lookup_host(&host).await?;
    let remote_addr = addrs.next().context("Failed to resolve host")?;
    println!("Resolved to: {}", remote_addr);
    println!("Connecting to {}...", remote_addr);

    let mut client = RaknetStream::connect(remote_addr)
        .await
        .expect("Failed to connect to server.");

    println!("Successfully connected to Minecraft server!");
    println!("Peer address: {}", remote_addr);

    // Build and send the first Game packet: RequestNetworkSettings.
    // Use McpePacket::from_payload_with_subclients for explicit header control.
    let network_settings_req = McpePacket::from_payload_with_subclients(
        PacketRequestNetworkSettings {
            client_protocol: PROTOCOL_VERSION,
        },
        0, // from_subclient
        0, // to_subclient
    );

    let mut buf = BytesMut::new();
    network_settings_req.encode(&mut buf)?; // Uses encode_game_frame internally with default header

    if let Err(e) = client.send(Bytes::from(buf)).await {
        println!("Send failed: {e:?}");
    }
    println!("Waiting for NetworkSettings response (15s timeout)...");

    // Create a default session for decoding args
    let session = BedrockSession { shield_item_id: 0 };

    tokio::select! {
        _ = sleep(Duration::from_secs(15)) => {
            println!("Timeout waiting for packets (expected if server drops handshake).");
        }
        _ = async {
            while let Some(result) = client.recv().await {
                match result {
                    Ok(bytes) => {
                        let mut buf = bytes;
                        // Decode into McpePacket struct which contains header + data
                        match McpePacket::decode(&mut buf, (&session).into()) {
                            Ok(packet) => {
                                // Match on packet.data which is the enum
                                match packet.data {
                                    McpePacketData::PacketNetworkSettings(settings) => {
                                        println!(
                                            "NetworkSettings: threshold={}, algo={:?}, throttle={}, thresh={}, scalar={}",
                                            settings.compression_threshold,
                                            settings.compression_algorithm,
                                            settings.client_throttle,
                                            settings.client_throttle_threshold,
                                            settings.client_throttle_scalar
                                        );
                                        println!("Handshake succeeded; send LoginPacket next using agreed compression.");
                                        return;
                                    }
                                    other => {
                                        println!(
                                            "Received non-NetworkSettings game packet: {:?} (header: {:?})",
                                            other.packet_id(),
                                            packet.header
                                        );
                                    }
                                }
                            }
                            Err(err) => eprintln!("Failed to decode game packet: {err:?}"),
                        }
                    }
                    Err(e) => {
                        println!("Connection closed with error: {:?}", e);
                        break;
                    }
                }
            }
        } => {}
    }

    println!("Closing connection.");
    Ok(())
}
