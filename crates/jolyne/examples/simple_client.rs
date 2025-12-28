use jolyne::BedrockStream;
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::valentine::McpePacket;
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
    let (mut play_stream, game_data) = handshake_stream.join(config).await?;
    println!("Joined Game!");
    println!("  Items: {}", game_data.item_registry.itemstates.len());
    println!("  Blocks: {}", game_data.start_game.block_properties.len());

    // Send a chunk radius request
    let req = jolyne::valentine::proto::RequestChunkRadiusPacket {
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
