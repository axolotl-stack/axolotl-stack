use axelerator::TokenCache;
use jolyne::BedrockStream;
use jolyne::stream::client::{ClientHandshakeConfig, XblCredentials};
use jolyne::valentine::McpePacket;
use p384::SecretKey;
use rand::thread_rng;
use std::error::Error;
use tokio::net::lookup_host;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize tracing for debug output (trace level to see RakNet frames)
    tracing_subscriber::fmt()
        .with_env_filter("info,jolyne=trace")
        .init();

    // Get hostname from command line or use LBSG as default
    let hostname = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "play.lbsg.net:19132".to_string());
    println!("Resolving {}...", hostname);
    let addr = lookup_host(hostname)
        .await?
        .next()
        .ok_or("Failed to resolve hostname")?;
    println!("Connecting to {}...", addr);

    // 1. Authenticate with Xbox Live
    println!("Authenticating with Xbox Live...");
    let token_cache = TokenCache::new("token.json");
    let xbl_token = token_cache.get_or_authenticate().await?;
    println!(
        "Authenticated as {} (XUID: {})",
        xbl_token.gamertag(),
        xbl_token.xuid
    );

    // Get BEDROCK_MULTIPLAYER token for server authentication
    // Note: This RP doesn't return gamertag/xuid in display claims, so we use
    // the xuid from the initial auth token instead
    let bedrock_token = token_cache
        .get_xbl_token(axolotl_xbl::auth::relying_party::BEDROCK_MULTIPLAYER)
        .await?;

    // 2. Connect
    let handshake_stream = BedrockStream::connect(addr).await?;
    println!("Connected!");

    // 3. Configure with Xbox Live credentials
    // Use xuid from the initial xbl_token since BEDROCK_MULTIPLAYER doesn't return it
    let xbl_credentials = XblCredentials::new(
        &bedrock_token.token,
        &bedrock_token.user_hash,
        &xbl_token.xuid, // Use xuid from initial auth
    );

    let config = ClientHandshakeConfig::with_xbox_live(
        addr,
        SecretKey::random(&mut thread_rng()),
        xbl_token.gamertag().to_string(),
        Uuid::new_v4(),
        xbl_credentials,
    );

    // The join() helper handles settings, auth, encryption, packs, and start game.
    let (mut play_stream, game_data) = handshake_stream.join(config).await?;
    println!("Joined Game!");
    println!("  World: {}", game_data.start_game.level_name);
    println!("  Items: {}", game_data.item_registry.item_data.len());
    println!("  Blocks: {}", game_data.start_game.block_properties.len());

    // Send a chunk radius request
    let req = jolyne::valentine::proto::RequestChunkRadiusPacket {
        chunk_radius: 8,
        max_chunk_radius: 8,
    };
    play_stream.send_packet(McpePacket::from(req)).await?;

    // Recv loop
    while let Ok(pkt) = play_stream.recv_packet().await {
        println!("Recv: {:?}", pkt.data.packet_id());
    }

    Ok(())
}
