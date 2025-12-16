use futures::StreamExt;
use std::error::Error;
use tokio::net::lookup_host;
use tokio_raknet::transport::RaknetStream;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Minecraft Bedrock default port is 19132
    let host = "play.lbsg.net:19132";

    println!("Resolving {}...", host);
    let mut addrs = lookup_host(host).await?;
    let remote_addr = addrs.next().ok_or("Failed to resolve host")?;

    println!("Resolved to: {}", remote_addr);
    println!("Connecting to {}...", remote_addr);

    // Minecraft usually uses a larger MTU, but 1400 is a safe starting point for RakNet negotiation.
    // The client will negotiate the actual MTU with the server during the offline handshake.
    let mut client = RaknetStream::connect(remote_addr)
        .await
        .expect("Failed to connect to server.");

    println!("Successfully connected to Minecraft server!");
    println!("Peer address: {}", remote_addr);

    // In a real Minecraft client, we would now start the Game Packet handshake (Login Packet, etc.)
    // For this example, we just want to prove the RakNet connection is established.

    // Let's listen for a bit to see if the server sends anything (it might send a game packet)
    // or just keep the connection alive.
    println!("Waiting for packets...");

    // Try to read a packet. Minecraft servers might disconnect us if we don't send a Login packet quickly,
    // but we should at least get the connection established.
    tokio::select! {
        _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
            println!("Timeout waiting for packets (expected if we don't send Login).");
        }
        result = client.next() => {
            match result {
                Some(Ok(p)) => {
                    println!("Received packet of size: {}", p.len());
                    if !p.is_empty() {
                        println!("Packet ID: 0x{:02x}", p[0]);
                    }
                }
                Some(Err(e)) => {
                    println!("Connection closed with error: {:?}", e);
                }
                None => {
                    println!("Connection closed by server (channel closed).");
                }
            }
        }
    }

    println!("Closing connection.");
    Ok(())
}
