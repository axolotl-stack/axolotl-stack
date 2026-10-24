//! Tier 1 (crate root): a complete Bedrock server in a few lines.
//!
//! Tier 1 is the default path: bind a [`BedrockListener`], `accept()` each
//! connection and let `accept_join_sequence` run the whole vanilla join
//! (network settings, login, encryption, resource packs, `StartGame`,
//! spawn). What a server usually wants to change (player cap, MOTD,
//! online/offline auth, packs, admission) is [`BedrockListenerConfig`], not
//! a different API. Drop to `03_custom_join` (Tier 2) only for custom join
//! flows.
//!
//! Offline by default so `02_client`, `04_proxy` and `stress_test` can join
//! without Xbox Live; `--online` requires Xbox Live authentication (what a
//! public server wants). Binds 127.0.0.1 by default: pass `0.0.0.0:19132`
//! to accept players from other machines.
//!
//! ```text
//! cargo run -p jolyne --example 01_server -- [ADDR=127.0.0.1:19132] [--online]
//! ```

use std::sync::Arc;

use jolyne::status::StaticStatus;
use jolyne::stream::raknet_types::ServerHandshakeStream;
use jolyne::valentine::{McpePacketData, TextPacketBody};
use jolyne::{BedrockListener, BedrockListenerConfig, JolyneError, WorldTemplate};
use p384::SecretKey;
use tokio_raknet::transport::RaknetListener;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

/// The listener `BedrockListener::raknet().bind()` returns.
pub type Listener = BedrockListener<RaknetListener>;

/// The listener configuration: the knobs a Tier 1 server turns.
pub fn config(online_mode: bool) -> BedrockListenerConfig {
    BedrockListenerConfig {
        // `true` accepts only Xbox Live-authenticated players; `false` takes
        // any self-signed identity (names are then client-chosen).
        online_mode,
        // The 21st player is refused with "server full" before login
        // verification runs.
        max_players: 20,
        // The MOTD and player counts shown in the server list (RakNet pong);
        // implement `StatusProvider` for a dynamic one.
        status: Some(Arc::new(StaticStatus::new("Jolyne example", "01_server"))),
        ..BedrockListenerConfig::default()
    }
}

/// Binds a RakNet listener on `addr` (`127.0.0.1:0` picks a free port).
pub async fn bind(addr: &str, online_mode: bool) -> Result<Listener, JolyneError> {
    BedrockListener::raknet()
        .addr(addr)
        .config(config(online_mode))
        .bind()
        .await
}

/// The accept loop: one task per player, so a slow join never blocks the
/// next `accept()`. Returns only when the listener shuts down.
pub async fn serve(mut listener: Listener) -> Result<(), JolyneError> {
    // Shared by every join: the world sent in `StartGame`, and the server's
    // P-384 key for the encryption handshake (one per listener).
    let world = Arc::new(WorldTemplate::default());
    let server_key = SecretKey::random(&mut rand::thread_rng());
    loop {
        let stream = listener.accept().await?;
        let peer = stream.peer_addr();
        let world = world.clone();
        let server_key = server_key.clone();
        tokio::spawn(async move {
            match handle_player(stream, &world, &server_key).await {
                Ok(chats) => info!(%peer, chats, "player left"),
                Err(error) => warn!(%peer, %error, "player dropped"),
            }
        });
    }
}

/// Joins one player, then logs its chat until it disconnects. Returns how
/// many chat messages it sent.
pub async fn handle_player(
    stream: ServerHandshakeStream,
    world: &WorldTemplate,
    server_key: &SecretKey,
) -> Result<usize, JolyneError> {
    // The whole vanilla join. On failure the client has already been sent a
    // reasoned `Disconnect`.
    let (mut play, login) = stream.accept_join_sequence(world, server_key).await?;
    let name = login.identity.display_name().to_owned();
    info!(%name, authenticated = login.identity.is_authenticated(), "player joined");

    let mut chats = 0;
    loop {
        match play.recv_packet().await {
            Ok(packet) => {
                if let McpePacketData::TextPacket(text) = &packet.data
                    && let TextPacketBody::AuthorAndMessage(chat) = &text.body
                {
                    chats += 1;
                    info!(%name, message = &*chat.message, "chat");
                }
            }
            // A packet valentine cannot decode; the connection is still fine.
            Err(error) if error.is_recoverable() => warn!(%name, %error, "skipped packet"),
            // The player left (`Disconnect` packet or the transport closed).
            Err(JolyneError::Disconnected(_) | JolyneError::ConnectionClosed) => return Ok(chats),
            Err(error) => return Err(error),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let online = args.iter().any(|arg| arg == "--online");
    let addr = args
        .iter()
        .find(|arg| !arg.starts_with("--"))
        .map_or("127.0.0.1:19132", String::as_str);

    let listener = bind(addr, online).await?;
    info!(addr = ?listener.local_addr(), online, "listening");
    serve(listener).await?;
    Ok(())
}
