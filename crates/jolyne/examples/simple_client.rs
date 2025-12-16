use jolyne::BedrockStream;
use jolyne::protocol::McpePacket;
use jolyne::stream::client::ClientHandshakeConfig;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let addr = "127.0.0.1:19133".parse()?;
    println!("Connecting to {}...", addr);

    // 1. Connect
    let handshake_stream = BedrockStream::connect(addr).await?;
    println!("Connected!");

    // 2. Configure & Join
    let config = ClientHandshakeConfig::random(addr, "Steve");

    // The join() helper handles settings, auth, encryption, packs, and start game.
    let mut play_stream = handshake_stream.join(config).await?;
    println!("Joined Game!");

    // Send a chunk radius request
    let req = jolyne::protocol::packets::PacketRequestChunkRadius {
        chunk_radius: 8,
        max_radius: 8,
    };
    play_stream.send_packet(McpePacket::from(req)).await?;

    // Recv loop
    while let Ok(pkt) = play_stream.recv_packet().await {
        println!("Recv: {:?}", pkt.data.packet_id());
    }

    Ok(())
}
