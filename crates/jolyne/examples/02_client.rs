//! Tier 1 (crate root): a Bedrock client that joins, chats and leaves.
//!
//! `BedrockStream::connect` opens the RakNet session and `join` runs the
//! whole vanilla join (settings, login, encryption, packs, `StartGame`,
//! spawn), returning the play stream plus the `GameData` captured on the
//! way. Who the client logs in as is a [`ClientHandshakeConfig`]: a random
//! offline identity, or Xbox Live for online-mode servers.
//!
//! Connects to 127.0.0.1:19132 by default, i.e. `01_server` on this machine.
//! `--xbox` logs in with Xbox Live (device-code sign-in, tokens cached in
//! `token.json`) through PlayFab, which online servers such as `01_server
//! --online` or BDS require.
//!
//! ```text
//! cargo run -p jolyne --example 02_client -- [ADDR=127.0.0.1:19132] [--xbox]
//! ```

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axelerator::TokenCache;
use axolotl_xbl::discovery::DiscoveryClient;
use axolotl_xbl::{PlayFabClient, XblError, XblToken};
use futures::future::BoxFuture;
use jolyne::error::AuthError;
use jolyne::stream::client::MultiplayerTokenSource;
use jolyne::valentine::{
    EnumsConnectionDisconnectFailReason, TextPacket, TextPacketBody,
    TextPacketPayloadAuthorAndMessage, TextPacketPayloadAuthorAndMessageMessageType,
};
use jolyne::{BedrockStream, ClientHandshakeConfig, GAME_VERSION, JolyneError};
use p384::SecretKey;
use tokio::net::lookup_host;
use uuid::Uuid;

/// How many play packets to print before leaving.
const READ_PACKETS: usize = 20;
/// Leave early once the server has been quiet this long.
const READ_IDLE: Duration = Duration::from_secs(1);

/// Joins `addr` as `identity`, says hello in chat, prints the first few
/// packets the server sends and disconnects.
pub async fn run_client(
    addr: SocketAddr,
    identity: ClientHandshakeConfig,
) -> Result<(), JolyneError> {
    let name = identity.display_name.clone();
    let stream = BedrockStream::connect(addr).await?;
    let (mut play, game_data) = stream.join(identity).await?;
    println!(
        "joined {:?} as {name} (runtime id {}, {} items)",
        &*game_data.start_game.level_name,
        play.runtime_id(),
        game_data.item_registry.item_data.len(),
    );

    // A chat message, as the vanilla client sends it.
    let chat = TextPacket {
        body: TextPacketBody::AuthorAndMessage(TextPacketPayloadAuthorAndMessage {
            message_type: TextPacketPayloadAuthorAndMessageMessageType::Chat,
            player_name: name.into(),
            message: "Hello from jolyne!".into(),
        }),
        ..TextPacket::default()
    };
    play.send_packet(chat.into()).await?;

    for _ in 0..READ_PACKETS {
        // `recv_packet` is cancel-safe: a timeout never loses a packet.
        match tokio::time::timeout(READ_IDLE, play.recv_packet()).await {
            Ok(Ok(packet)) => println!("received {:?}", packet.data.packet_id()),
            Ok(Err(error)) if error.is_recoverable() => println!("skipped packet: {error}"),
            Ok(Err(error)) => return Err(error),
            Err(_) => break,
        }
    }

    // Tells the server we left on purpose, drains pending writes and closes
    // the RakNet session (`close()` would skip the `Disconnect`).
    play.disconnect(EnumsConnectionDisconnectFailReason::Disconnected, "")
        .await
}

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

/// An Xbox Live identity: signs in (or reuses `token.json`), then issues a
/// multiplayer token for the connection key through PlayFab.
async fn xbox_live_identity(addr: SocketAddr) -> anyhow::Result<ClientHandshakeConfig> {
    let token_cache = TokenCache::new("token.json");
    let xbl_token = token_cache.get_or_authenticate().await?;
    println!(
        "signed in as {} (XUID {})",
        xbl_token.gamertag(),
        xbl_token.xuid
    );
    // The discovered authorization environment picks the PlayFab title; its
    // login needs a PlayFab-scoped Xbox Live token.
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
    Ok(ClientHandshakeConfig::with_xbox_live(
        addr,
        SecretKey::random(&mut rand::thread_rng()),
        xbl_token.gamertag().to_string(),
        Uuid::new_v4(),
        tokens,
    ))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let xbox = args.iter().any(|arg| arg == "--xbox");
    let host = args
        .iter()
        .find(|arg| !arg.starts_with("--"))
        .map_or("127.0.0.1:19132", String::as_str);
    let addr = lookup_host(host)
        .await?
        .next()
        .ok_or_else(|| anyhow::anyhow!("{host} did not resolve"))?;

    let identity = if xbox {
        xbox_live_identity(addr).await?
    } else {
        // Offline: accepted by offline-mode servers (`01_server` default).
        ClientHandshakeConfig::random(addr, "JolyneClient")
    };
    run_client(addr, identity).await?;
    Ok(())
}
