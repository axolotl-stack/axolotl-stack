use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use bytes::{BufMut, Bytes, BytesMut};
use tokio::net::lookup_host;
use tokio::time::sleep;
use tokio_raknet::transport::RaknetStream;
use valentine::bedrock::{
    codec::BedrockCodec,
    protocol::v1_21_124::{
        packets::PacketNetworkSettings,
        types::McpePacketName,
    },
};
use valentine::protocol::wire::{read_var_u32, write_var_u32};

const PROTOCOL_VERSION: i32 = 860;
const GAME_PACKET_ID: u8 = 0xFE;
const SUBCLIENT_SENDER: u32 = 0;
const SUBCLIENT_TARGET: u32 = 0;

/// Encode a single Bedrock payload with a game-packet header.
/// Header layout (varuint32):
/// bits 0..10: packet id (10 bits)
/// bits 10..12: sender subclient (2 bits)
/// bits 12..14: target subclient (2 bits)
fn encode_game_packet(packet_id: u32, payload: &impl BedrockCodec) -> Vec<u8> {
    let mut packet_buf = BytesMut::new();
    payload
        .encode(&mut packet_buf)
        .expect("bedrock packet encode should succeed");

    let header = packet_id | (SUBCLIENT_SENDER << 10) | (SUBCLIENT_TARGET << 12);
    let mut header_buf = BytesMut::new();
    write_var_u32(&mut header_buf, header);

    // Total length is header varint + payload bytes.
    let total_len = header_buf.len() + packet_buf.len();

    let mut out = BytesMut::new();
    out.put_u8(GAME_PACKET_ID);
    write_var_u32(&mut out, total_len as u32);
    out.extend_from_slice(&header_buf);
    out.extend_from_slice(&packet_buf);
    out.to_vec()
}

/// Decode a single game packet and return its id plus raw payload bytes.
fn decode_game_packet(data: &[u8]) -> Result<(u32, Bytes)> {
    if data.len() < 3 {
        return Err(anyhow!("packet too small"));
    }

    if data[0] != GAME_PACKET_ID {
        return Err(anyhow!("unexpected game packet id 0x{:02x}", data[0]));
    }

    let mut buf = Bytes::from(data[1..].to_vec());
    let declared_len = read_var_u32(&mut buf)? as usize;
    if buf.len() < declared_len {
        return Err(anyhow!(
            "declared game packet length {} exceeds available {}",
            declared_len,
            buf.len()
        ));
    }

    let mut content = buf.split_to(declared_len);
    let header = read_var_u32(&mut content)?;
    let packet_id = header & 0x3FF;
    Ok((packet_id, content))
}

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
    let network_settings_req =
        valentine::bedrock::protocol::v1_21_124::packets::PacketRequestNetworkSettings {
            client_protocol: PROTOCOL_VERSION,
        };

    let first_packet = encode_game_packet(
        McpePacketName::RequestNetworkSettings as u32,
        &network_settings_req,
    );
    if let Err(e) = client.send(Bytes::from(first_packet)).await {
        println!("Send failed: {e:?}");
    }
    println!("Waiting for NetworkSettings response (15s timeout)...");

    tokio::select! {
        _ = sleep(Duration::from_secs(15)) => {
            println!("Timeout waiting for packets (expected if server drops handshake).");
        }
        _ = async {
            while let Some(result) = client.recv().await {
                match result {
                    Ok(bytes) => {
                        match decode_game_packet(&bytes) {
                            Ok((packet_id, mut payload)) => {
                                if packet_id == McpePacketName::NetworkSettings as u32 {
                                    match PacketNetworkSettings::decode(&mut payload, ()) {
                                        Ok(settings) => {
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
                                        Err(err) => {
                                            eprintln!("Failed to decode NetworkSettings payload: {err:?}");
                                        }
                                    }
                                } else {
                                    println!(
                                        "Received non-NetworkSettings game packet id={packet_id}"
                                    );
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
