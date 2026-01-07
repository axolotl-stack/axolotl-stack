//! Xbox Live NetherNet Client - Test Tool
//!
//! This binary connects to a friend's Minecraft server via Xbox Live signaling.
//! Use it to debug connection issues by seeing exactly what happens from the client side.
//!
//! Usage:
//!   cargo run -p axelerator --bin xbox_client -- --token friend_token.json --target-xuid 12345678
//!
//! This will:
//! 1. Authenticate with Xbox Live using the friend's token
//! 2. Query the target XUID's activities to find their NetherNet ID
//! 3. Connect via Xbox signaling WebSocket
//! 4. Perform Bedrock handshake
//! 5. Log everything that happens

use anyhow::{Context, Result};
use axelerator::TokenCache;
use axolotl_xbl::{PlayFabClient, SessionClient};
use clap::Parser;
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::stream::transport::{BedrockTransport, NetherNetTransport};
use jolyne::stream::{BedrockStream, Client, Handshake};
use p384::SecretKey;
use rand::thread_rng;
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio_nethernet::signaling::SignalingChannel;
use tokio_nethernet::{NetherNetDialer, NetherNetDialerConfig, XboxSignaling};
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "xbox_client")]
#[command(about = "Connect to a friend's Minecraft server via Xbox Live")]
struct Args {
    /// Path to OAuth token cache file (for the connecting account)
    #[arg(long, default_value = "token.client.json")]
    token: PathBuf,

    /// XUID of the friend hosting the server
    #[arg(long)]
    target_xuid: String,

    /// Skip Bedrock handshake (just test WebRTC connection)
    #[arg(long)]
    webrtc_only: bool,

    /// Display name to use when joining
    #[arg(long, default_value = "DebugClient")]
    display_name: String,

    /// Verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize tracing
    let filter = if args.verbose {
        "debug,tokio_nethernet=trace,jolyne=trace,axolotl_xbl=debug"
    } else {
        "info,tokio_nethernet=debug,jolyne=debug"
    };
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter));
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .init();

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║             XBOX LIVE NETHERNET CLIENT TEST                  ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  This tool connects to a friend's server via Xbox signaling ║");
    println!("║  to debug connection issues from the client's perspective.  ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // Step 1: Authenticate
    info!("Step 1: Authenticating with Xbox Live...");
    let token_cache = TokenCache::new(&args.token);
    let xbl_token = token_cache
        .get_or_authenticate()
        .await
        .context("Failed to authenticate")?;

    info!(
        gamertag = %xbl_token.gamertag(),
        xuid = %xbl_token.xuid,
        "Authenticated as"
    );

    // Step 2: Find target's session
    info!("Step 2: Querying target XUID {} for active sessions...", args.target_xuid);
    let session_client = SessionClient::new();
    let activities = session_client
        .query_activities_by_xuid(xbl_token, &args.target_xuid)
        .await
        .context("Failed to query activities")?;

    if activities.is_empty() {
        error!("No active sessions found for XUID {}", args.target_xuid);
        error!("Make sure the target is running axelerator or hosting a game!");
        return Ok(());
    }

    println!();
    println!("Found {} session(s):", activities.len());
    for (i, activity) in activities.iter().enumerate() {
        println!(
            "  [{}] Session: {} | Handle: {} | NetherNet: {:?} | Host: {:?}",
            i + 1,
            activity.session_id,
            activity.handle_id,
            activity.nethernet_id,
            activity.host_name
        );
    }
    println!();

    // Use first session with a nethernet_id
    let target_activity = activities
        .iter()
        .find(|a| a.nethernet_id.is_some())
        .context("No session with NetherNet ID found")?;

    let nethernet_id = target_activity.nethernet_id.unwrap();
    info!(
        session_id = %target_activity.session_id,
        nethernet_id = nethernet_id,
        host_name = ?target_activity.host_name,
        "Selected target session"
    );

    // Step 3: Get PlayFab token for signaling
    info!("Step 3: Getting PlayFab token for signaling...");
    let playfab_token = token_cache
        .get_xbl_token(axolotl_xbl::auth::relying_party::PLAYFAB)
        .await
        .context("Failed to get PlayFab token")?;

    let playfab = PlayFabClient::new();
    let playfab_ticket = playfab
        .login(&playfab_token.user_hash, &playfab_token.token)
        .await
        .context("PlayFab login failed")?;

    // Use a random device ID like the real client
    let device_id = format!("{:032x}", rand::random::<u128>());
    let mc_token = playfab
        .start_session(&device_id, &playfab_ticket)
        .await
        .context("Minecraft session start failed")?;

    info!("Got Minecraft token for signaling");

    // Step 3.5: Get BEDROCK_MULTIPLAYER token for Minecraft authentication
    info!("Step 3.5: Getting Bedrock Multiplayer token for authentication...");
    let bedrock_token = token_cache
        .get_xbl_token(axolotl_xbl::auth::relying_party::BEDROCK_MULTIPLAYER)
        .await
        .context("Failed to get Bedrock Multiplayer token")?;

    info!(
        gamertag = %bedrock_token.gamertag,
        "Got Bedrock Multiplayer token"
    );

    // Step 4: Connect via Xbox signaling
    info!("Step 4: Connecting to Xbox signaling WebSocket...");

    // We need our own nethernet_id to connect to signaling
    // Generate one based on XUID like the real client does
    let our_nethernet_id: u64 = xbl_token.xuid.parse().unwrap_or_else(|_| rand::random());

    info!(
        our_nethernet_id = our_nethernet_id,
        target_nethernet_id = nethernet_id,
        "Connecting to signaling"
    );

    let signaling = XboxSignaling::connect(our_nethernet_id, &mc_token)
        .await
        .context("Failed to connect to Xbox signaling")?;

    info!("Connected to Xbox signaling WebSocket");

    // Step 5: Dial the target
    info!("Step 5: Dialing target nethernet_id {}...", nethernet_id);

    // Use the Arc-based new() API - we need to spawn a signal pump
    let signaling_clone = signaling.clone();
    let (dialer, signal_tx) = NetherNetDialer::new(signaling.clone(), NetherNetDialerConfig::default());

    // Spawn signal pump to route incoming signals to the dialer
    tokio::spawn(async move {
        if let Some(mut rx) = signaling_clone.take_signal_receiver().await {
            while let Some(signal) = rx.recv().await {
                if signal_tx.send(signal).await.is_err() {
                    break;
                }
            }
        }
    });

    let dial_start = std::time::Instant::now();
    let stream = match dialer.dial(nethernet_id.to_string()).await {
        Ok(s) => s,
        Err(e) => {
            error!(
                elapsed_ms = dial_start.elapsed().as_millis() as u64,
                error = %e,
                "DIAL FAILED"
            );
            return Err(e.into());
        }
    };

    info!(
        elapsed_ms = dial_start.elapsed().as_millis() as u64,
        "WebRTC connection established!"
    );

    if args.webrtc_only {
        info!("--webrtc-only specified, skipping Bedrock handshake");
        info!("Connection successful! Waiting 5 seconds then disconnecting...");
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        return Ok(());
    }

    // Step 6: Bedrock handshake
    info!("Step 6: Starting Bedrock handshake...");

    let transport = BedrockTransport::new(NetherNetTransport::new(stream));
    let handshake_stream =
        <BedrockStream<Handshake, Client, NetherNetTransport>>::from_transport(transport);

    // Create XBL credentials for Minecraft authentication
    let xbl_credentials = jolyne::stream::client::XblCredentials::new(
        &bedrock_token.token,
        &bedrock_token.user_hash,
    );

    let client_config = ClientHandshakeConfig::with_xbox_live(
        "0.0.0.0:19132".parse::<SocketAddr>().unwrap(),
        SecretKey::random(&mut thread_rng()),
        args.display_name.clone(),
        Uuid::new_v4(),
        xbl_credentials,
    );

    let handshake_start = std::time::Instant::now();
    match handshake_stream.join(client_config).await {
        Ok((mut play_stream, game_data)) => {
            let world_name = &game_data.start_game.world_name;
            info!(
                elapsed_ms = handshake_start.elapsed().as_millis() as u64,
                world_name = %world_name,
                "Joined game successfully!"
            );

            println!();
            println!("╔══════════════════════════════════════════════════════════════╗");
            println!("║                    CONNECTION SUCCESSFUL!                    ║");
            println!("╠══════════════════════════════════════════════════════════════╣");
            println!("║  World: {:<51} ║", world_name);
            println!(
                "║  Game Mode: {:<47} ║",
                format!("{:?}", game_data.start_game.player_gamemode)
            );
            println!("╚══════════════════════════════════════════════════════════════╝");
            println!();

            info!("Listening for packets (press Ctrl+C to disconnect)...");
            println!();

            // Receive packets until disconnect
            loop {
                match play_stream.recv_packet().await {
                    Ok(packet) => {
                        debug!(
                            packet_id = ?packet.data.packet_id(),
                            "Received packet"
                        );

                        // Check for disconnect packet
                        if let jolyne::valentine::McpePacketData::PacketDisconnect(disconnect) =
                            &packet.data
                        {
                            warn!(
                                hide_reason = ?disconnect.hide_disconnect_reason,
                                reason = ?disconnect.reason,
                                "Server sent DISCONNECT"
                            );
                            break;
                        }

                        // Check for transfer packet
                        if let jolyne::valentine::McpePacketData::PacketTransfer(transfer) =
                            &packet.data
                        {
                            info!(
                                address = %transfer.server_address,
                                port = transfer.port,
                                "Server sent TRANSFER"
                            );
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "Connection error");
                        break;
                    }
                }
            }
        }
        Err(e) => {
            error!(
                elapsed_ms = handshake_start.elapsed().as_millis() as u64,
                error = %e,
                "HANDSHAKE FAILED"
            );
            return Err(e.into());
        }
    }

    info!("Disconnected from server");
    Ok(())
}
