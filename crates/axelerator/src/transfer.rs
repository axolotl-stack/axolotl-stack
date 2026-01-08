//! Transfer packet handling for Axelerator.
//!
//! Handles incoming WebRTC connections from Xbox Live friends and
//! redirects them to the actual Minecraft server.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use jolyne::stream::{BedrockStream, Play, Server};
use jolyne::valentine::{McpePacket, TransferPacket};
use jolyne::{BedrockListener, WorldTemplate};
use p384::SecretKey;
use rand::thread_rng;
use tokio::sync::Notify;
use tracing::{debug, error, info, warn};

use crate::AxeleratorConfig;

/// Maximum retry attempts before giving up completely.
const MAX_RETRY_ATTEMPTS: u32 = 10;

/// Initial backoff delay in milliseconds.
const INITIAL_BACKOFF_MS: u64 = 1000;

/// Maximum backoff delay in milliseconds (5 minutes).
const MAX_BACKOFF_MS: u64 = 300_000;

/// Statistics for the transfer server.
#[derive(Debug, Default)]
pub struct TransferStats {
    /// Total connections accepted.
    pub connections_accepted: AtomicU64,
    /// Successful transfers.
    pub transfers_successful: AtomicU64,
    /// Failed handshakes.
    pub handshakes_failed: AtomicU64,
    /// Failed transfers.
    pub transfers_failed: AtomicU64,
    /// Number of reconnection attempts.
    pub reconnect_attempts: AtomicU64,
    /// Whether the server is currently connected.
    pub is_connected: AtomicBool,
}

impl TransferStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn summary(&self) -> String {
        format!(
            "connections={}, successful={}, handshake_failed={}, transfer_failed={}, reconnects={}",
            self.connections_accepted.load(Ordering::Relaxed),
            self.transfers_successful.load(Ordering::Relaxed),
            self.handshakes_failed.load(Ordering::Relaxed),
            self.transfers_failed.load(Ordering::Relaxed),
            self.reconnect_attempts.load(Ordering::Relaxed),
        )
    }
}

/// Runs the mini-server that accepts WebRTC connections and transfers players.
///
/// This performs a minimal Bedrock handshake (just enough to send packets),
/// then sends a Transfer packet with the downstream server address.
///
/// The server automatically reconnects on transient failures with exponential backoff.
/// Returns `ShutdownReason` to allow caller to handle auth errors specially.
///
/// # Arguments
/// * `signaling_url` - Full WebSocket URL from discovery API
/// * `nethernet_id` - Your NetherNet network ID
/// * `mc_token` - Minecraft authorization token from PlayFab session
/// * `config` - Axelerator configuration
/// * `shutdown` - Notification to trigger graceful shutdown
/// * `stats` - Optional statistics tracker
///
/// # Returns
/// * `Ok(ShutdownReason::Requested)` - Graceful shutdown completed
/// * `Ok(ShutdownReason::AuthenticationError)` - Token needs refresh, caller should get new token
/// * `Err(_)` - Unrecoverable fatal error
pub async fn run_transfer_server(
    signaling_url: &str,
    nethernet_id: u64,
    mc_token: &str,
    config: &AxeleratorConfig,
    shutdown: Arc<Notify>,
    stats: Option<Arc<TransferStats>>,
) -> anyhow::Result<ShutdownReason> {
    let stats = stats.unwrap_or_else(|| Arc::new(TransferStats::new()));
    let mut retry_count: u32 = 0;
    let mut backoff_ms = INITIAL_BACKOFF_MS;

    loop {
        // Check for shutdown before attempting connection
        if is_shutdown_requested(&shutdown) {
            info!("Transfer server shutdown requested before connect");
            return Ok(ShutdownReason::Requested);
        }

        match run_transfer_server_inner(
            signaling_url,
            nethernet_id,
            mc_token,
            config,
            shutdown.clone(),
            stats.clone(),
        )
        .await
        {
            Ok(ShutdownReason::Requested) => {
                info!("Transfer server shutdown completed gracefully");
                return Ok(ShutdownReason::Requested);
            }
            Ok(ShutdownReason::ListenerClosed) => {
                // Listener closed without error - might be temporary
                warn!("Transfer server listener closed, will attempt reconnect");
            }
            Ok(ShutdownReason::AuthenticationError) => {
                // Don't retry auth errors internally - let caller handle token refresh
                warn!("Transfer server authentication error - token may need refresh");
                return Ok(ShutdownReason::AuthenticationError);
            }
            Ok(ShutdownReason::FatalError) => {
                error!("Transfer server encountered fatal error");
                return Err(anyhow::anyhow!("Transfer server fatal configuration error"));
            }
            Err(e) => {
                // Classify the error
                let reason = classify_error(&e);
                match reason {
                    ShutdownReason::AuthenticationError => {
                        warn!("Transfer server authentication error: {}", e);
                        return Ok(ShutdownReason::AuthenticationError);
                    }
                    ShutdownReason::FatalError => {
                        error!("Transfer server fatal error (not retrying): {}", e);
                        return Err(e);
                    }
                    _ => {
                        warn!("Transfer server error (will retry): {}", e);
                    }
                }
            }
        }

        // Update stats
        stats.is_connected.store(false, Ordering::Relaxed);
        stats.reconnect_attempts.fetch_add(1, Ordering::Relaxed);

        retry_count += 1;
        if retry_count >= MAX_RETRY_ATTEMPTS {
            error!(
                "Transfer server exceeded max retry attempts ({}), giving up",
                MAX_RETRY_ATTEMPTS
            );
            return Err(anyhow::anyhow!(
                "Transfer server failed after {} attempts",
                MAX_RETRY_ATTEMPTS
            ));
        }

        // Exponential backoff with jitter
        let jitter = rand::random::<u64>() % (backoff_ms / 4 + 1);
        let delay = Duration::from_millis(backoff_ms + jitter);

        info!(
            attempt = retry_count,
            max_attempts = MAX_RETRY_ATTEMPTS,
            delay_secs = delay.as_secs_f32(),
            "Reconnecting transfer server..."
        );

        // Wait with shutdown check
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = shutdown.notified() => {
                info!("Shutdown requested during reconnect backoff");
                return Ok(ShutdownReason::Requested);
            }
        }

        // Increase backoff for next attempt
        backoff_ms = (backoff_ms * 2).min(MAX_BACKOFF_MS);
    }
}

/// Reason the server shut down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownReason {
    /// Graceful shutdown was requested.
    Requested,
    /// Listener closed (may be recoverable).
    ListenerClosed,
    /// Authentication error - token may need refresh.
    AuthenticationError,
    /// Fatal unrecoverable error.
    FatalError,
}

/// Inner transfer server loop - handles a single connection session.
async fn run_transfer_server_inner(
    signaling_url: &str,
    nethernet_id: u64,
    mc_token: &str,
    config: &AxeleratorConfig,
    shutdown: Arc<Notify>,
    stats: Arc<TransferStats>,
) -> anyhow::Result<ShutdownReason> {
    // Create listener with builder API
    let mut builder = BedrockListener::nethernet().xbox(signaling_url, nethernet_id, mc_token);

    if config.monitor_tampering {
        use tokio_nethernet::SignalMonitorConfig;
        info!("Signal-level monitoring ENABLED for WebRTC signaling");
        builder = builder.with_signal_monitoring(SignalMonitorConfig {
            enabled: true,
            verbose: false,
            ..Default::default()
        });
    }

    let mut listener = builder.bind().await?;

    // Mark as connected
    stats.is_connected.store(true, Ordering::Relaxed);

    // Setup for handshake
    let template = Arc::new(WorldTemplate::default());
    let server_key = SecretKey::random(&mut thread_rng());

    let target_host = config.server_ip.clone();
    let target_port = config.server_port;

    info!(
        downstream = %target_host,
        port = target_port,
        "Transfer server ready, waiting for friend connections..."
    );

    loop {
        tokio::select! {
            biased;

            // Prioritize shutdown signal
            _ = shutdown.notified() => {
                info!("Transfer server received shutdown signal");
                return Ok(ShutdownReason::Requested);
            }

            // Accept connections
            result = listener.accept() => {
                match result {
                    Ok(handshake_stream) => {
                        stats.connections_accepted.fetch_add(1, Ordering::Relaxed);
                        info!(
                            total_connections = stats.connections_accepted.load(Ordering::Relaxed),
                            "Friend connected via WebRTC!"
                        );

                        let template = template.clone();
                        let key = server_key.clone();
                        let host = target_host.clone();
                        let port = target_port;
                        let stats = stats.clone();

                        tokio::spawn(async move {
                            handle_transfer_connection(
                                handshake_stream,
                                &template,
                                &key,
                                host,
                                port,
                                stats,
                            )
                            .await;
                        });
                    }
                    Err(e) => {
                        // Log but don't immediately fail - could be transient
                        error!("Accept error: {:?}", e);

                        // Check if this looks like a fatal error vs transient
                        let error_str = format!("{:?}", e);
                        if error_str.contains("closed")
                            || error_str.contains("shutdown")
                            || error_str.contains("WebSocket")
                        {
                            // Likely a connection issue, return to trigger reconnect
                            return Ok(ShutdownReason::ListenerClosed);
                        }

                        // For other errors, log and continue trying
                        warn!("Transient accept error, continuing...");
                    }
                }
            }
        }
    }
}

/// Check if shutdown has been requested (non-blocking).
fn is_shutdown_requested(_shutdown: &Notify) -> bool {
    // Notify doesn't support non-blocking check, we rely on select
    // This function exists for potential future use with other sync primitives
    false
}

/// Classifies an error into a shutdown reason.
fn classify_error(e: &anyhow::Error) -> ShutdownReason {
    let error_str = format!("{:?}", e);

    // Authentication failures - token may need refresh
    if error_str.contains("401")
        || error_str.contains("403")
        || error_str.contains("Unauthorized")
        || error_str.contains("Forbidden")
    {
        return ShutdownReason::AuthenticationError;
    }

    // Invalid configuration - truly fatal
    if error_str.contains("Invalid URL") || error_str.contains("missing host") {
        return ShutdownReason::FatalError;
    }

    // Otherwise, assume it's transient/recoverable
    ShutdownReason::ListenerClosed
}

/// Handles a single connection: performs handshake, resolves DNS, and sends transfer packet.
async fn handle_transfer_connection<T: jolyne::stream::transport::Transport>(
    handshake_stream: BedrockStream<jolyne::stream::Handshake, Server, T>,
    template: &WorldTemplate,
    key: &SecretKey,
    target_host: String,
    target_port: u16,
    stats: Arc<TransferStats>,
) {
    debug!("Starting Bedrock handshake...");

    match handshake_stream.accept_join_sequence(template, key).await {
        Ok((mut play_stream, identity)) => {
            let display_name = identity.display_name.as_deref().unwrap_or("Unknown");

            info!(
                identity = %display_name,
                downstream = %target_host,
                port = target_port,
                "Transferring player to downstream server"
            );

            // Send Transfer packet with raw string (allows IP, Domain, or NetherNet ID)
            if let Err(e) = send_transfer_packet(&mut play_stream, &target_host, target_port).await
            {
                error!("Failed to send transfer packet: {:?}", e);
                stats.transfers_failed.fetch_add(1, Ordering::Relaxed);
            } else {
                info!(identity = %display_name, "Transfer packet sent successfully!");
                stats.transfers_successful.fetch_add(1, Ordering::Relaxed);
            }

            // Give client time to process the transfer
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Err(e) => {
            warn!("Handshake failed: {:?}", e);
            stats.handshakes_failed.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Sends a Transfer packet to redirect the client.
async fn send_transfer_packet<T: jolyne::stream::transport::Transport>(
    stream: &mut BedrockStream<Play, Server, T>,
    addr_str: &str,
    port: u16,
) -> anyhow::Result<()> {
    let transfer = TransferPacket {
        server_address: addr_str.to_string(),
        port,
        reload_world: false,
    };

    let packet = McpePacket::from(transfer);
    stream
        .send_batch_with_reliability(&[packet], true)
        .await
        .map_err(|e| anyhow::anyhow!("Send failed: {}", e))?;

    Ok(())
}

// Legacy function for backwards compatibility (deprecated)
#[deprecated(note = "Use run_transfer_server with shutdown notify instead")]
pub async fn run_transfer_server_legacy(
    signaling_url: &str,
    nethernet_id: u64,
    mc_token: &str,
    config: &AxeleratorConfig,
) -> anyhow::Result<()> {
    let shutdown = Arc::new(Notify::new());
    run_transfer_server(signaling_url, nethernet_id, mc_token, config, shutdown, None)
        .await
        .map(|_| ())
}
