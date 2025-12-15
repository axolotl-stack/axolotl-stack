use jolyne::{BedrockListener, BedrockListenerConfig};
use std::error::Error;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize tracing
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,jolyne=trace"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    // Config
    let config = BedrockListenerConfig {
        encryption_enabled: true,
        online_mode: true,
        ..Default::default()
    };

    // Bind (None = Default StartGameConfig)
    let mut listener = BedrockListener::bind("0.0.0.0:19133", config, None).await?;
    info!("Server started on {}", listener.local_addr()?);

    loop {
        match listener.accept().await {
            Ok((mut play_stream, identity)) => {
                let addr = play_stream.peer_addr();
                let display_name = identity.display_name.as_deref().unwrap_or("Unknown");
                info!(%addr, identity = %display_name, "Accepted connection (Handshake Auto-Completed)");

                tokio::spawn(async move {
                    info!(%addr, "In-Game! Listening for packets...");

                    // Play Loop
                    while let Ok(packet) = play_stream.recv_packet().await {
                        info!(%addr, id=?packet.data.packet_id(), "Recv Packet");
                    }
                    info!(%addr, "Disconnected");
                });
            }
            Err(e) => {
                tracing::error!("Accept failed: {:?}", e);
            }
        }
    }
}
