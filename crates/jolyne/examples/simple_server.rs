use jolyne::{BedrockListener, BedrockListenerConfig};
use std::error::Error;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize tracing with env filter (e.g., RUST_LOG=info,jolyne=debug)
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,jolyne=trace"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    // Bind to a port
    let addr = "0.0.0.0:19133".parse()?;
    // Configurable compression; errors bubble up as structured JolyneError (Auth/Protocol).
    let mut config = BedrockListenerConfig::default();
    // Demonstrate tuning compression knobs
    config.compression_threshold = 512;
    config.compression_level = 7;
    // With encryption disabled and packs optional, StartGame is sent as soon as the
    // client replies ResourcePackClientResponse::Completed (or Refused if packs stay optional).
    config.encryption_enabled = true; // set true to require encryption.
    config.require_resource_packs = false; // set true to require packs (currently empty list)
    config.handle_client_cache_status = true;
    config.send_block_palette = false; // set true to send empty UpdateBlockProperties after StartGame

    let mut listener = BedrockListener::bind_with_config(addr, config).await?;
    info!(
        listening = %listener.local_addr()?,
        compression_threshold = listener.config().compression_threshold,
        compression_level = listener.config().compression_level,
        encryption_enabled = listener.config().encryption_enabled,
        require_resource_packs = listener.config().require_resource_packs,
        handle_client_cache_status = listener.config().handle_client_cache_status,
        send_block_palette = listener.config().send_block_palette,
        "Server started"
    );

    while let Ok(stream) = listener.accept().await {
        let addr = stream.peer_addr()?;
        info!(%addr, "Accepted connection");

        // Move stream into the spawned task
        tokio::spawn(async move {
            let mut stream = stream; // Make mutable here
            loop {
                match stream.recv().await {
                    Ok(packet) => match &packet.data {
                        jolyne::protocol::types::mcpe::McpePacketData::PacketPlayStatus(ps) => {
                            info!(%addr, status = ?ps.status, "Received PlayStatus");
                        }
                        jolyne::protocol::types::mcpe::McpePacketData::PacketStartGame(_) => {
                            info!(%addr, "Received StartGame");
                        }
                        _ => {
                            info!(%addr, packet = ?packet.data.packet_id(), "Received packet");
                        }
                    },
                    Err(e) => {
                        error!(%addr, error = ?e, "Stream closed");
                        break;
                    }
                }
            }
        });
    }
    Ok(())
}
