use jolyne::stream::{BedrockStream, ConnectionSide};
use jolyne::{BedrockListener, BedrockListenerConfig, JolyneError};
use std::error::Error;
use tokio::net::lookup_host;
use tokio_raknet::RaknetStream;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let listen_addr = "0.0.0.0:19133".parse()?;
    let target_host = "127.0.0.1:19132";

    println!("Proxy listening on {}", listen_addr);
    println!("Targeting {}", target_host);

    let mut config = BedrockListenerConfig::default();
    config.compression_threshold = 512;

    let mut listener = BedrockListener::bind_with_config(listen_addr, config).await?;

    while let Ok(client_stream) = listener.accept().await {
        let target_host = target_host.to_string();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(client_stream, target_host).await {
                // Structured errors (Auth/Protocol/RakNet) can be matched here.
                eprintln!("Connection error: {:?}", e);
            }
        });
    }

    Ok(())
}

async fn handle_connection(
    mut client_stream: BedrockStream,
    target_host: String,
) -> Result<(), Box<dyn Error>> {
    let client_addr = client_stream.peer_addr()?;
    println!("New connection from {}", client_addr);

    // Resolve and connect to server
    let mut addrs = lookup_host(&target_host).await?;
    let target_addr = addrs.next().ok_or("Invalid target address")?;

    let rak_server = RaknetStream::connect(target_addr).await?;
    let mut server_cfg = BedrockListenerConfig::default();
    server_cfg.compression_threshold = 512;
    let mut server_stream = BedrockStream::new(rak_server, ConnectionSide::Client, server_cfg);

    println!("Connected to server for client {}", client_addr);

    loop {
        tokio::select! {
            // Client -> Server
            packet_res = client_stream.recv() => {
                match packet_res {
                    Ok(packet) => {
                        let name = packet.data.packet_id();
                        println!("[C->S] {:?}", name);
                        // Forward to server
                        if let Err(e) = server_stream.send(packet).await {
                            eprintln!("Failed to send to server: {}", e);
                            break;
                        }
                    }
                    Err(JolyneError::ConnectionClosed) => {
                        println!("Client {} disconnected", client_addr);
                        break;
                    }
                    Err(e) => {
                        eprintln!("Error receiving from client {}: {:?}", client_addr, e);
                        break;
                    }
                }
            }

            // Server -> Client
            packet_res = server_stream.recv() => {
                match packet_res {
                    Ok(packet) => {
                        let name = packet.data.packet_id();
                        println!("[S->C] {:?}", name);
                        // Forward to client
                        if let Err(e) = client_stream.send(packet).await {
                            eprintln!("Failed to send to client: {}", e);
                            break;
                        }
                    }
                    Err(JolyneError::ConnectionClosed) => {
                        println!("Server disconnected for client {}", client_addr);
                        break;
                    }
                    Err(e) => {
                        eprintln!("Error receiving from server for {}: {:?}", client_addr, e);
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}
