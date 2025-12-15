use jolyne::protocol::McpePacket;
use jolyne::stream::BedrockStream;
use jolyne::stream::client::ClientHandshakeConfig;
use p384::SecretKey;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let addr = "127.0.0.1:19133".parse()?;
    println!("Connecting to {}...", addr);

    // 1. Connect (Returns BedrockStream<Handshake, Client>)
    let handshake_stream = BedrockStream::connect(addr).await?;
    println!("Connected!");

    // 2. Negotiate Settings
    let login_stream = handshake_stream.request_settings().await?;
    println!("Settings negotiated, sending login...");

    // 3. Login
    let key = SecretKey::random(&mut rand::thread_rng());
    let config = ClientHandshakeConfig {
        server_addr: addr,
        identity_key: key,
    };

    let secure_pending = login_stream.send_login(&config).await?;
    println!("Login sent, waiting for handshake...");

    // 4. Secure Handshake
    let packs_stream = secure_pending.await_handshake().await?;
    println!("Logged in, negotiating packs...");

    // 5. Resource Packs
    let start_stream = packs_stream.handle_packs().await?;
    println!("Packs negotiated, waiting for StartGame...");

    // 6. Start Game
    let mut play_stream = start_stream.await_start_game().await?;
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
