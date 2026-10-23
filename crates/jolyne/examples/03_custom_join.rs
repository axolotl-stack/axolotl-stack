//! Tier 2 (`jolyne::stream`): the server join, one step at a time.
//!
//! `accept_join_sequence` (Tier 1, `01_server`) is these five calls in a
//! row. Calling them yourself is for what configuration cannot express:
//!
//! - async admission between `authenticate` and `finish_handshake` (a
//!   database or HTTP lookup; `AdmissionHook` is synchronous),
//! - a resource pack set chosen per player (`negotiate_packs_with`),
//! - work between phases (loading the player's save before `StartGame`),
//! - answering a failed step your way: every step returns `Failed`, which
//!   still owns the connection; `Failed::disconnect()` tells the client why.
//!
//! The compiler enforces the order: each step consumes the stream and
//! returns the next phase's type.
//!
//! ```text
//! cargo run -p jolyne --example 03_custom_join -- [ADDR=127.0.0.1:19132]
//!     [--pack FILE.mcpack --pack-uuid UUID [--pack-version 1.0.0]]
//! ```
//!
//! Offline mode, so `02_client` can join. The optional pack is offered to
//! desktop and console players only; mobile players skip the download.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use jolyne::proto::packs::{ResourcePack, ServerPacks};
use jolyne::stream::Failed;
use jolyne::stream::RakNetTransport;
use jolyne::stream::raknet_types::{ServerHandshakeStream, ServerPlayStream};
use jolyne::valentine::{EnumsConnectionDisconnectFailReason, Vec3};
use jolyne::{
    AuthenticatedLogin, BedrockListener, BedrockListenerConfig, JolyneError, WorldTemplate,
};
use p384::SecretKey;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[derive(Parser)]
#[command(about = "Tier 2: a step-by-step server join")]
struct Args {
    /// Address to listen on.
    #[arg(default_value = "127.0.0.1:19132")]
    addr: String,
    /// A `.mcpack` offered to non-mobile players.
    #[arg(long, requires = "pack_uuid")]
    pack: Option<PathBuf>,
    /// The pack's UUID (`header.uuid` in its manifest).
    #[arg(long)]
    pack_uuid: Option<Uuid>,
    /// The pack's version (`header.version` in its manifest, dotted).
    #[arg(long, default_value = "1.0.0")]
    pack_version: String,
}

/// The two pack sets players are sorted into.
struct PackSets {
    /// Offered to desktop and console players.
    full: Arc<ServerPacks>,
    /// Offered to mobile players: nothing to download.
    mobile: Arc<ServerPacks>,
}

impl PackSets {
    fn load(args: &Args) -> anyhow::Result<Self> {
        let mut full = ServerPacks::default();
        if let (Some(path), Some(uuid)) = (&args.pack, args.pack_uuid) {
            let content = std::fs::read(path)?;
            full.packs.push(Arc::new(ResourcePack::new(
                uuid,
                args.pack_version.as_str(),
                content.into(),
            )?));
        }
        Ok(Self {
            full: Arc::new(full),
            mobile: Arc::new(ServerPacks::default()),
        })
    }

    /// Picks the pack set from the client data sent with the login.
    fn for_player(&self, login: &AuthenticatedLogin) -> Arc<ServerPacks> {
        const ANDROID: i32 = 1;
        const IOS: i32 = 2;
        match login.client_data.device_os {
            ANDROID | IOS => self.mobile.clone(),
            _ => self.full.clone(),
        }
    }
}

/// Your own async admission, e.g. a ban list in a database. Runs inside the
/// login deadline (`BedrockListenerConfig::login_timeout`): the client is
/// waiting for the encryption handshake meanwhile.
async fn check_bans(login: &AuthenticatedLogin) -> Result<(), String> {
    tokio::time::sleep(Duration::from_millis(10)).await; // the "query"
    // The identity is not proven until `finish_handshake` (online mode) or
    // never (offline mode), so `claimed_xuid()` is only a claim. Refusing on
    // a claim is safe, an impostor only refuses itself; granting anything
    // must wait for `is_authenticated()` after `finish_handshake`.
    let banned = login.identity.display_name() == "Griefer"
        || login.identity.claimed_xuid() == Some("2535400000000000");
    if banned {
        Err("You are banned from this server.".into())
    } else {
        Ok(())
    }
}

/// Your own async storage: where the player logged out last time.
async fn load_spawn_position(uuid: Uuid) -> Vec3 {
    tokio::time::sleep(Duration::from_millis(10)).await; // the "read"
    let offset = (uuid.as_u128() % 16) as f32;
    Vec3 {
        x: 0.5 + offset,
        y: 17.62,
        z: 0.5,
    }
}

/// The join, step by step. `Ok(None)`: refused by `check_bans`.
async fn join(
    stream: ServerHandshakeStream,
    world: &WorldTemplate,
    server_key: &SecretKey,
    packs: &PackSets,
) -> Result<Option<ServerPlayStream>, Failed<RakNetTransport>> {
    // 1. `RequestNetworkSettings` -> `NetworkSettings`; compression is on
    //    from here.
    let login = stream.accept_network_settings().await?;

    // 2. `Login`: enforces `max_players`, verifies the identity and runs the
    //    synchronous `AdmissionHook`, if one is configured.
    let secure = login.authenticate().await?;

    // Custom admission: `login()` is available, and nothing costly
    // (ECDH, packs, world data) has been spent on the player yet.
    if let Err(reason) = check_bans(secure.login()).await {
        info!(name = secure.login().identity.display_name(), %reason, "refused");
        // Any phase can send its own `Disconnect` and close.
        if let Err(error) = secure
            .disconnect(EnumsConnectionDisconnectFailReason::Kicked, &reason)
            .await
        {
            warn!(%error, "refusal not delivered");
        }
        return Ok(None);
    }

    // 3. ECDH handshake and `PlayStatus(LoginSuccess)`. An online identity
    //    is authenticated once this step completes.
    let packs_phase = secure.finish_handshake(server_key).await?;
    let identity = &packs_phase.login().identity;
    info!(
        name = identity.display_name(),
        trust = ?identity.trust(),
        "handshake finished"
    );

    // 4. Resource packs, chosen for this player.
    let pack_set = packs.for_player(packs_phase.login());
    let start = packs_phase.negotiate_packs_with(pack_set).await?;

    // Work between phases: the client sits on the loading screen until
    // `StartGame`, so load what the world needs for this player now.
    let mut params = world.to_join_params(rand::random());
    params.start_game.position = load_spawn_position(start.login().identity.uuid()).await;

    // 5. `StartGame` and definitions; returns once the client reports it
    //    has spawned (`SetLocalPlayerAsInitialized`).
    let play = start.start_game(params).await?;
    Ok(Some(play))
}

async fn handle_player(
    stream: ServerHandshakeStream,
    world: &WorldTemplate,
    server_key: &SecretKey,
    packs: &PackSets,
) -> Result<(), JolyneError> {
    let mut play = match join(stream, world, server_key, packs).await {
        Ok(Some(play)) => play,
        Ok(None) => return Ok(()),
        // Sends the `Disconnect` matching the error (a fixed message that
        // leaks no internals), closes, and hands the error back for logging.
        // `failed.into_parts()` would keep the connection instead.
        Err(failed) => return Err(failed.disconnect().await),
    };
    let name = play.identity().display_name().to_owned();
    info!(%name, runtime_id = play.runtime_id(), "spawned");
    loop {
        match play.recv_packet().await {
            Ok(packet) => info!(%name, id = ?packet.data.packet_id(), "packet"),
            Err(error) if error.is_recoverable() => warn!(%name, %error, "skipped packet"),
            Err(JolyneError::Disconnected(_) | JolyneError::ConnectionClosed) => {
                info!(%name, "left");
                return Ok(());
            }
            Err(error) => return Err(error),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let args = Args::parse();
    let packs = Arc::new(PackSets::load(&args)?);
    let mut listener = BedrockListener::raknet()
        .addr(args.addr.as_str())
        .config(BedrockListenerConfig {
            online_mode: false,
            ..BedrockListenerConfig::default()
        })
        .bind()
        .await?;
    info!(addr = ?listener.local_addr(), "listening");

    let world = Arc::new(WorldTemplate::default());
    let server_key = SecretKey::random(&mut rand::thread_rng());
    loop {
        let stream = listener.accept().await?;
        let peer = stream.peer_addr();
        let (world, server_key, packs) = (world.clone(), server_key.clone(), packs.clone());
        tokio::spawn(async move {
            if let Err(error) = handle_player(stream, &world, &server_key, &packs).await {
                warn!(%peer, %error, "join failed");
            }
        });
    }
}
