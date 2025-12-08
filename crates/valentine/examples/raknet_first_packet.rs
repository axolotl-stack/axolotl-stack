use std::error::Error;
use std::time::Duration;

use bytes::{BufMut, Bytes, BytesMut};
use flate2::{write::ZlibEncoder, Compression};
use std::io::Write;
use tokio::net::lookup_host;
use tokio::time::sleep;
use tokio_raknet::transport::RaknetStream;
use valentine::bedrock::codec::BedrockCodec;
use valentine::bedrock::protocol::v1_21_124::packets::{
    PacketDisconnect, PacketLogin, PacketNetworkSettings, PacketRequestNetworkSettings, PacketText,
};
use valentine::bedrock::protocol::v1_21_124::types::LoginTokens;
use valentine::protocol::wire::write_var_u32;

const PACKET_ID_LOGIN: u8 = 0x01;
const PACKET_ID_NETWORK_SETTINGS: u8 = 0x07;
const PACKET_ID_REQUEST_NETWORK_SETTINGS: u8 = 0x08;
const PACKET_ID_DISCONNECT: u8 = 0x05;
const PACKET_ID_TEXT: u8 = 0x09;
// Fixed protocol version (Bedrock 1.21.30 era).
const PROTOCOL_VERSION: i32 = 860;

#[derive(Clone, Copy, Debug)]
enum CompressionAlgo {
    Deflate,
    Snappy,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let host = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "play.lbsg.net:19132".to_string());

    println!("Resolving {}...", host);
    let mut addrs = lookup_host(&host).await?;
    let remote_addr = addrs.next().ok_or("failed to resolve host")?;
    println!("Resolved to: {}", remote_addr);
    println!("Connecting to {}...", remote_addr);

    let mut client = RaknetStream::connect(remote_addr)
        .await
        .expect("Failed to connect to server.");

    println!("Successfully connected to Minecraft server!");
    println!("Peer address: {}", remote_addr);
    println!("Sending RequestNetworkSettings + Login together (batched)... protocol={PROTOCOL_VERSION}");
    let handshake = build_handshake_batch(PROTOCOL_VERSION, CompressionAlgo::Deflate);
    if let Err(e) = client.send(handshake).await {
        println!("Send failed: {e:?}");
    }
    println!("Waiting for NetworkSettings and responses (15s timeout)...");

    let mut compression = CompressionAlgo::Deflate;
    tokio::select! {
        _ = sleep(Duration::from_secs(15)) => {
            println!("Timeout waiting for packets (expected if server drops handshake).");
        }
        _ = async {
            while let Some(result) = client.recv().await {
                match result {
                    Ok(bytes) => {
                        if let Some(new_comp) = handle_packet(&bytes) {
                            compression = new_comp;
                            // If compression differs, we could resend login with new algo.
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

fn handle_packet(bytes: &Bytes) -> Option<CompressionAlgo> {
    println!("Received packet of size: {}", bytes.len());
    if bytes.is_empty() {
        println!("Packet was empty.");
        return None;
    }

    let packet_id = bytes[0];
    println!("Packet ID: 0x{packet_id:02x}");

    match decode_known(packet_id, bytes.slice(1..)) {
        Some(Decoded::Disconnect(text)) => println!("{text}"),
        Some(Decoded::Text(text)) => println!("{text}"),
        Some(Decoded::NetworkSettings { compression }) => return Some(compression),
        None => {
            println!("No decoder registered for 0x{packet_id:02x}; raw hex:");
            for chunk in bytes.chunks(16) {
                let row = chunk
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                println!("{row}");
            }
        }
    }

    None
}

enum Decoded {
    Disconnect(String),
    Text(String),
    NetworkSettings { compression: CompressionAlgo },
}

fn decode_known(packet_id: u8, payload: Bytes) -> Option<Decoded> {
    let mut buf = payload.clone();
    match packet_id {
        PACKET_ID_NETWORK_SETTINGS => PacketNetworkSettings::decode(&mut buf, ()).ok().map(|p| {
            let algo = match p.compression_algorithm {
                valentine::bedrock::protocol::v1_21_124::packets::PacketNetworkSettingsCompressionAlgorithm::Deflate => CompressionAlgo::Deflate,
                valentine::bedrock::protocol::v1_21_124::packets::PacketNetworkSettingsCompressionAlgorithm::Snappy => CompressionAlgo::Snappy,
            };
            println!(
                "NetworkSettings: threshold={}, algo={:?}, throttle={}, thresh={}, scalar={}",
                p.compression_threshold, algo, p.client_throttle, p.client_throttle_threshold, p.client_throttle_scalar
            );
            Decoded::NetworkSettings { compression: algo }
        }),
        PACKET_ID_DISCONNECT => PacketDisconnect::decode(&mut buf, ()).ok().map(|p| {
            Decoded::Disconnect(format!("Disconnect: {:?}", p))
        }),
        PACKET_ID_TEXT => PacketText::decode(&mut buf, ()).ok().map(|p| {
            Decoded::Text(format!("Text: {:?}", p))
        }),
        _ => None,
    }
}

fn build_handshake_batch(protocol_version: i32, algo: CompressionAlgo) -> Bytes {
    // Build RequestNetworkSettings
    let mut req = BytesMut::new();
    req.put_u8(PACKET_ID_REQUEST_NETWORK_SETTINGS);
    let request = PacketRequestNetworkSettings {
        client_protocol: protocol_version,
    };
    let _ = request.encode(&mut req);

    // Build Login
    let mut login_buf = BytesMut::new();
    login_buf.put_u8(PACKET_ID_LOGIN);
    let login = PacketLogin {
        protocol_version,
        tokens: LoginTokens {
            identity: r#"{"chain":["dummy.jwt"]}"#.to_string(),
            client: "dummy.jwt".to_string(),
        },
    };
    let _ = login.encode(&mut login_buf);

    wrap_batch_multi(&[req, login_buf], algo)
}

fn wrap_batch_multi(inner_packets: &[BytesMut], algo: CompressionAlgo) -> Bytes {
    // Batch (0xfe): [0xfe][varuint len][compressed_payload]
    // compressed_payload = concatenation of [varuint len][packet_bytes] for each packet.
    let mut uncompressed = BytesMut::new();
    for pkt in inner_packets {
        write_var_u32(&mut uncompressed, pkt.len() as u32);
        uncompressed.extend_from_slice(pkt);
    }

    let compressed = match algo {
        CompressionAlgo::Deflate => {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
            let _ = encoder.write_all(&uncompressed);
            encoder.finish().unwrap_or_default()
        }
        CompressionAlgo::Snappy => {
            println!("Snappy not implemented; sending uncompressed batch payload.");
            uncompressed.to_vec()
        }
    };

    let mut out = BytesMut::new();
    out.put_u8(0xfe);
    write_var_u32(&mut out, compressed.len() as u32);
    out.extend_from_slice(&compressed);
    out.freeze()
}
