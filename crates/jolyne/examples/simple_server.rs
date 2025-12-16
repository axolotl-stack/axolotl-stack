use jolyne::{BedrockListener, BedrockListenerConfig, WorldTemplate};
use p384::SecretKey;
use rand::thread_rng;
use std::error::Error;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize tracing
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,jolyne=debug"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    // Config
    let config = BedrockListenerConfig::default();

    let mut listener = BedrockListener::bind("0.0.0.0:19132", config).await?;
    info!("Server started on {}", listener.local_addr()?);

    // Prepare static data
    let template = Arc::new(WorldTemplate::default());
    let server_key = SecretKey::random(&mut thread_rng());

    loop {
        // Accept returns BedrockStream<Handshake, Server>
        match listener.accept().await {
            Ok(handshake_stream) => {
                let addr = handshake_stream.peer_addr();
                info!(%addr, "Connection accepted, starting handshake...");

                let template = template.clone();
                let key = server_key.clone();

                tokio::spawn(async move {
                    // Run the full join sequence
                    match handshake_stream.accept_join_sequence(&template, &key).await {
                        Ok((mut play_stream, identity)) => {
                            let display_name =
                                identity.display_name.as_deref().unwrap_or("Unknown");
                            info!(%addr, identity = %display_name, "Joined Game!");

                            // OPTIONAL: Enable Manual Flush for higher performance (Server Tick Mode).
                            // By default, jolyne sends every packet immediately (Low Latency).
                            // For a real server, you want to buffer packets and flush once per tick (50ms).
                            // play_stream.set_auto_flush(false);

                            // Play Loop
                            while let Ok(packet) = play_stream.recv_packet().await {
                                info!(%addr, id=?packet.data.packet_id(), "Recv Packet");
                                
                                // If using manual flush, you would call this at the end of your tick:
                                // play_stream.flush().await?;
                            }
                            info!(%addr, "Disconnected");
                        }
                        Err(e) => {
                            tracing::error!(%addr, "Handshake failed: {:?}", e);
                        }
                    }
                });
            }
            Err(e) => {
                tracing::error!("Listener accept failed: {:?}", e);
            }
        }
    }
}
