use jolyne::BedrockListenerConfig;
use jolyne::protocol::{packets::PacketRequestNetworkSettings, types::mcpe::McpePacket};
use jolyne::stream::{BedrockStream, ConnectionSide};
use std::error::Error;
use tokio_raknet::RaknetStream;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let addr = "127.0.0.1:19133".parse()?;
    println!("Connecting to {}...", addr);

    let rak_stream = RaknetStream::connect(addr).await?;
    // Errors are structured (JolyneError::Auth(AuthError), etc.), so callers
    // can match on variants instead of parsing strings.
    let mut cfg = BedrockListenerConfig::default();
    cfg.compression_threshold = 512;
    cfg.compression_level = 7;

    let mut stream = BedrockStream::new(rak_stream, ConnectionSide::Client, cfg);
    println!("Connected!");

    let packet = McpePacket::from(PacketRequestNetworkSettings {
        client_protocol: jolyne::protocol::PROTOCOL_VERSION,
    });

    stream.send(packet).await?;
    println!("Sent RequestNetworkSettings");

    // Wait for response
    let response = stream.recv().await?;
    println!("Received: {:?}", response.data.packet_id());

    Ok(())
}
