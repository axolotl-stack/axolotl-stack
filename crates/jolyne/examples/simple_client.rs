use axelerator::TokenCache;
use axolotl_xbl::discovery::DiscoveryClient;
use axolotl_xbl::{PlayFabClient, XblError, XblToken};
use futures::future::BoxFuture;
use jolyne::BedrockStream;
use jolyne::error::AuthError;
use jolyne::stream::client::{ClientHandshakeConfig, MultiplayerTokenSource};
use jolyne::valentine::{GAME_VERSION, McpePacket};
use p384::SecretKey;
use rand::thread_rng;
use std::error::Error;
use std::sync::Arc;
use tokio::net::lookup_host;
use uuid::Uuid;

/// Multiplayer tokens through PlayFab, as gophertunnel's dialer does:
/// PlayFab-scoped XBL token -> PlayFab login -> Minecraft service token ->
/// per-key multiplayer token.
struct PlayFabTokens {
    playfab: PlayFabClient,
    playfab_xbl: XblToken,
    device_id: String,
}

impl MultiplayerTokenSource for PlayFabTokens {
    fn multiplayer_token<'a>(
        &'a self,
        public_key_der: &'a [u8],
    ) -> BoxFuture<'a, Result<String, AuthError>> {
        let error = |e: XblError| AuthError::TokenAcquisition(e.to_string());
        Box::pin(async move {
            let ticket = self
                .playfab
                .login(&self.playfab_xbl.user_hash, &self.playfab_xbl.token)
                .await
                .map_err(error)?;
            let service = self
                .playfab
                .service_token(&self.device_id, &ticket, GAME_VERSION)
                .await
                .map_err(error)?;
            self.playfab
                .multiplayer_token(&service, public_key_der)
                .await
                .map_err(error)
        })
    }
}

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

    // 2. Select the discovered authorization environment and obtain the
    // PlayFab-scoped Xbox Live token used for the PlayFab login.
    let environment = DiscoveryClient::new()
        .discover_authorization(GAME_VERSION)
        .await?;
    let playfab = PlayFabClient::with_environment(&environment)?;
    let playfab_xbl = token_cache
        .get_xbl_token_for_url(playfab.login_url())
        .await?;
    let tokens = Arc::new(PlayFabTokens {
        playfab,
        playfab_xbl,
        device_id: Uuid::new_v4().to_string(),
    });

    // 3. Connect; Login sends AuthenticationType 0 with the multiplayer token.
    let handshake_stream = BedrockStream::connect(addr).await?;
    println!("Connected!");
    let config = ClientHandshakeConfig::with_xbox_live(
        addr,
        SecretKey::random(&mut thread_rng()),
        xbl_token.gamertag().to_string(),
        Uuid::new_v4(),
        tokens,
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
