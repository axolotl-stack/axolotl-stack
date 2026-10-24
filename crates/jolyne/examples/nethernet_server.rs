//! Tier 1 over NetherNet (WebRTC, LAN discovery): `01_server` with another
//! transport.
//!
//! Only the builder changes: `BedrockListener::nethernet().lan(addr)`
//! answers LAN discovery on `addr` and accepts WebRTC connections; accept
//! and `accept_join_sequence` are the same as over RakNet. WebRTC's DTLS
//! already encrypts the channel, so Bedrock encryption is off (as on
//! retail).
//!
//! Binds 127.0.0.1:7551 by default, which `nethernet_client` discovers on
//! this machine. Pass `0.0.0.0:7551` to answer discovery from the LAN.
//!
//! ```text
//! cargo run -p jolyne --example nethernet_server --features discovery -- [ADDR=127.0.0.1:7551]
//! ```

use std::sync::Arc;

use jolyne::status::StaticStatus;
use jolyne::{BedrockListener, BedrockListenerConfig, WorldTemplate};
use p384::SecretKey;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tokio_nethernet=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:7551".into());
    let mut listener = BedrockListener::nethernet()
        .lan(addr.as_str())
        .config(BedrockListenerConfig {
            // `.config` replaces the NetherNet builder's defaults, so keep
            // Bedrock encryption off explicitly: DTLS encrypts.
            encryption_enabled: false,
            online_mode: false,
            // The name shown in LAN discovery.
            status: Some(Arc::new(StaticStatus::new("Jolyne NetherNet", "LAN"))),
            ..BedrockListenerConfig::default()
        })
        .bind()
        .await?;
    info!(%addr, "NetherNet listening for LAN discovery");

    let world = Arc::new(WorldTemplate::default());
    let server_key = SecretKey::random(&mut rand::thread_rng());
    loop {
        let stream = listener.accept().await?;
        let (world, server_key) = (world.clone(), server_key.clone());
        tokio::spawn(async move {
            let (mut play, login) = match stream.accept_join_sequence(&world, &server_key).await {
                Ok(joined) => joined,
                Err(error) => {
                    warn!(%error, "join failed");
                    return;
                }
            };
            let name = login.identity.display_name().to_owned();
            info!(%name, "joined over NetherNet");
            loop {
                match play.recv_packet().await {
                    Ok(packet) => info!(%name, id = ?packet.data.packet_id(), "packet"),
                    Err(error) if error.is_recoverable() => warn!(%name, %error, "skipped packet"),
                    Err(error) => {
                        info!(%name, %error, "left");
                        break;
                    }
                }
            }
        });
    }
}
