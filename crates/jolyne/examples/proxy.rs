use jolyne::stream::server::ServerHandshakeConfig;
use jolyne::{BedrockListener, BedrockListenerConfig, BedrockStream};
use p384::SecretKey;
use rand::thread_rng;
use std::error::Error;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,jolyne=debug"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    let config = BedrockListenerConfig::default();

    // Proxy listens on 19133
    let mut listener = BedrockListener::bind("0.0.0.0:19133", config).await?;
    let server_key = SecretKey::random(&mut thread_rng());

    info!("Proxy started on 19133. Forwarding allowed users to 19132 (Start simple_server first!)");

    loop {
        match listener.accept().await {
            Ok(handshake) => {
                let key = server_key.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_proxy_client(handshake, key).await {
                        warn!("Proxy error: {:?}", e);
                    }
                });
            }
            Err(e) => warn!("Accept failed: {:?}", e),
        }
    }
}

async fn handle_proxy_client(
    stream: jolyne::ServerLogin,
    server_key: SecretKey,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let addr = stream.peer_addr();
    info!(%addr, "Client connected to Proxy");

    // 1. Handshake & Network Settings
    let login_stream = stream.accept_network_settings().await?;

    // 2. Authenticate
    // Here we can inspect the identity and decide whether to allow them.
    let (secure_pending, identity) = login_stream.authenticate().await?;
    let display_name = identity.display_name.as_deref().unwrap_or("Unknown");
    info!(%addr, user=%display_name, "Authenticated by Proxy");

    // 3. Complete Encryption Handshake
    // We terminate encryption here. The client talks encrypted to US.
    let handshake_config = ServerHandshakeConfig { server_key };
    let pack_stream = secure_pending
        .finish_handshake(&handshake_config, &identity.identity_public_key)
        .await?;

    // 4. "The Escape Hatch"
    // We drop the Jolyne state machine. We now have a raw, encrypted transport.
    let mut client_transport = pack_stream.into_transport();

    // 5. Connect to Upstream (Backend Server)
    // In a real proxy, we would now connect to the backend.
    // For this example, we assume simple_server is running on 19133.
    info!(%addr, "Connecting to upstream...");
    let upstream_stream = BedrockStream::connect("127.0.0.1:19132".parse()?).await?;

    // Let's do a quick login to upstream.
    let up_login = upstream_stream.request_settings().await?;
    // We use a new key/identity for the upstream connection
    let up_key = SecretKey::random(&mut thread_rng());
    let up_config = jolyne::stream::client::ClientHandshakeConfig {
        server_addr: "127.0.0.1:19132".parse()?,
        identity_key: up_key.clone(),
        display_name: "ProxyUser".to_string(),
        uuid: Uuid::new_v4(),
    };
    let up_secure = up_login.send_login(&up_config).await?;
    let up_packs = up_secure.await_handshake(&up_key).await?;

    // We are now at ResourcePacks state on upstream.
    // Client is at ResourcePacks state on downstream.

    // Drop both to transport and bridge!
    let mut upstream_transport = up_packs.into_transport();

    info!(%addr, "Bridging connections...");

    loop {
        tokio::select! {
            // Downstream -> Upstream
            pkt = client_transport.recv_packet() => {
                let pkt = pkt?;
                // Forward packet
                upstream_transport.send_batch(&[pkt]).await?;
            }
            // Upstream -> Downstream
            pkt = upstream_transport.recv_packet() => {
                let pkt = pkt?;
                client_transport.send_batch(&[pkt]).await?;
            }
        }
    }
}
