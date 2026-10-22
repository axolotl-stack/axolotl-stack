//! A transparent RakNet proxy: players join `0.0.0.0:19133`, the proxy joins
//! the upstream server as them (offline identity with the same name) and
//! relays packets both ways, logging chat.
//!
//! ```text
//! cargo run -p jolyne --example simple_server   # upstream on 19132
//! cargo run -p jolyne --example proxy -- 127.0.0.1:19132
//! ```

use std::error::Error;
use std::net::SocketAddr;

use jolyne::proxy::{self, Side, Verdict};
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::valentine::McpePacketName;
use jolyne::{BedrockListener, BedrockListenerConfig, BedrockStream, ClientConfig};
use p384::SecretKey;
use rand::thread_rng;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,jolyne=debug"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    let upstream: SocketAddr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:19132".into())
        .parse()?;
    let mut listener = BedrockListener::raknet()
        .addr("0.0.0.0:19133")
        .config(proxy::proxy_listener_config(BedrockListenerConfig {
            online_mode: false,
            ..Default::default()
        }))
        .bind()
        .await?;
    let server_key = SecretKey::random(&mut thread_rng());
    info!(%upstream, "proxy listening on 0.0.0.0:19133");

    loop {
        let stream = listener.accept().await?;
        let server_key = server_key.clone();
        tokio::spawn(async move {
            let peer = stream.peer_addr();
            let session = proxy::accept(stream, &server_key, |login| {
                let name = login.identity.display_name.clone().unwrap_or_default();
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
