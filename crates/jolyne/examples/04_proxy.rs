//! Tier 3 (`jolyne::proxy`): a transparent RakNet proxy.
//!
//! Players join the proxy; for each one the proxy joins the upstream server
//! as them (an offline identity with the same name) and relays packets both
//! ways as their original frames, logging chat. Encryption is terminated on
//! each side (the proxy owns both key exchanges), and the relay works on raw
//! packets (Tier 3): only the header is parsed, so unknown and custom IDs
//! pass through untouched.
//!
//! The upstream must accept offline identities, e.g. `01_server` in its
//! default offline mode:
//!
//! ```text
//! cargo run -p jolyne --example 01_server                    # upstream on 127.0.0.1:19132
//! cargo run -p jolyne --example 04_proxy -- [UPSTREAM=127.0.0.1:19132] [LISTEN=127.0.0.1:19133]
//! ```

use std::net::SocketAddr;

use jolyne::proxy::{self, Side, Verdict};
use jolyne::valentine::McpePacketName;
use jolyne::{
    BedrockListener, BedrockListenerConfig, BedrockStream, ClientConfig, ClientHandshakeConfig,
};
use p384::SecretKey;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let mut args = std::env::args().skip(1);
    let upstream: SocketAddr = args
        .next()
        .as_deref()
        .unwrap_or("127.0.0.1:19132")
        .parse()?;
    let listen = args.next().unwrap_or_else(|| "127.0.0.1:19133".into());

    // `proxy_listener_config` lets unknown packet IDs pass through instead
    // of disconnecting the player.
    let mut listener = BedrockListener::raknet()
        .addr(listen.as_str())
        .config(proxy::proxy_listener_config(BedrockListenerConfig {
            online_mode: false,
            ..BedrockListenerConfig::default()
        }))
        .bind()
        .await?;
    let server_key = SecretKey::random(&mut rand::thread_rng());
    info!(%upstream, %listen, "proxy listening");

    loop {
        let stream = listener.accept().await?;
        let server_key = server_key.clone();
        tokio::spawn(async move {
            let peer = stream.peer_addr();
            // Logs the player in downstream; with its verified login, the
            // closure opens the upstream leg as that player. The player is
            // then spawned with the upstream's world.
            let session = proxy::accept(stream, &server_key, |login| {
                let name = login.identity.display_name().to_owned();
                async move {
                    let upstream_stream =
                        BedrockStream::connect_with(upstream, ClientConfig::default()).await?;
                    Ok((
                        upstream_stream,
                        ClientHandshakeConfig::random(upstream, name),
                    ))
                }
            })
            .await;
            let session = match session {
                Ok(session) => session,
                Err(error) => {
                    warn!(%peer, "proxy join failed: {error}");
                    return;
                }
            };
            info!(%peer, "relaying");
            // `relay` shows every packet, header-parsed, to the filter; its
            // `Verdict` forwards, drops or replaces it.
            let end = session
                .relay(|side, packet| {
                    if packet.is(McpePacketName::TextPacket) {
                        info!(%peer, ?side, "chat packet ({} bytes)", packet.body().len());
                    }
                    if side == Side::Downstream && packet.name().is_none() {
                        info!(%peer, id = packet.id(), "forwarding unknown packet");
                    }
                    Verdict::Forward
                })
                .await;
            info!(%peer, side = ?end.side, "relay ended: {}", end.error);
        });
    }
}
