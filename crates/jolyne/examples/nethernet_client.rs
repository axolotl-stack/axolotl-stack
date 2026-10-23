//! A NetherNet (WebRTC, LAN discovery) client: Tier 2 transport setup, then
//! the Tier 1 join.
//!
//! There is no `connect` shortcut for NetherNet: finding and dialing a
//! server goes through `tokio_nethernet` (LAN discovery here; Xbox Live
//! signaling elsewhere). The dialed stream is wrapped in a
//! `NetherNetTransport` and started with `stream::client::from_transport`
//! (Tier 2, for transports you construct); from there `join` is the same
//! as over RakNet.
//!
//! Broadcasts discovery to 127.0.0.1:7551 by default, i.e.
//! `nethernet_server` on this machine. Pass `255.255.255.255:7551` to find
//! servers on the LAN.
//!
//! ```text
//! cargo run -p jolyne --example nethernet_client --features discovery -- [DISCOVERY_ADDR=127.0.0.1:7551]
//! ```

use std::net::SocketAddr;
use std::time::Duration;

use jolyne::stream::{Handshake, NetherNetTransport};
use jolyne::{BedrockStream, Client, ClientConfig, ClientHandshakeConfig};
use tokio_nethernet::NetherNetDialer;
use tokio_nethernet::NetherNetDialerConfig;
use tokio_nethernet::discovery::{DiscoveryListener, DiscoveryListenerConfig, ServerData};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tokio_nethernet=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let broadcast: SocketAddr = std::env::args()
        .nth(1)
        .as_deref()
        .unwrap_or("127.0.0.1:7551")
        .parse()?;
    // Replies come back to the sender's address: stay on loopback when
    // discovering locally.
    let bind = if broadcast.ip().is_loopback() {
        "127.0.0.1:0"
    } else {
        "0.0.0.0:0"
    };
    let discovery = DiscoveryListener::bind(
        bind,
        DiscoveryListenerConfig {
            broadcast_addr: broadcast,
            broadcast_interval: Duration::from_millis(500),
            ..Default::default()
        },
    )
    .await?;

    println!("discovering servers via {broadcast}...");
    let mut responses = discovery.responses().await;
    for _ in 0..10 {
        if !responses.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        responses = discovery.responses().await;
    }
    let Some(server_id) = responses.keys().next().copied() else {
        println!("no server answered; start nethernet_server first");
        return Ok(());
    };
    for (network_id, data) in &responses {
        if let Some(server) = ServerData::decode(data) {
            println!(
                "  {} (id {network_id}): {}/{} players",
                server.server_name, server.player_count, server.max_player_count,
            );
        }
    }

    // WebRTC connection, signaled over the discovery socket.
    let dialer =
        NetherNetDialer::connect_with_signaling(discovery, NetherNetDialerConfig::default());
    let stream = dialer.dial(server_id.to_string()).await?;
    let handshake = BedrockStream::<Handshake, Client, _>::from_transport(
        NetherNetTransport::new(stream),
        ClientConfig::default(),
    )
    .await?;

    // WebRTC has no server socket address; the config's is only a label.
    let label = SocketAddr::from(([127, 0, 0, 1], 0));
    let (mut play, game_data) = handshake
        .join(ClientHandshakeConfig::random(label, "NetherNetClient"))
        .await?;
    println!("joined {:?}", &*game_data.start_game.level_name);

    loop {
        match play.recv_packet().await {
            Ok(packet) => info!(id = ?packet.data.packet_id(), "packet"),
            Err(error) if error.is_recoverable() => info!(%error, "skipped packet"),
            Err(error) => {
                println!("disconnected: {error}");
                return Ok(());
            }
        }
    }
}
