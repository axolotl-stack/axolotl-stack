//! Transfer packet handling for Axelerator.
//!
//! Handles incoming WebRTC connections from Xbox Live friends and
//! redirects them to the actual Minecraft server.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use jolyne::stream::{BedrockStream, Play, Server};
use jolyne::valentine::{McpePacket, TransferPacket};
use jolyne::{BedrockListener, WorldTemplate};
use p384::SecretKey;
use rand::thread_rng;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::AxeleratorConfig;
use crate::player_history::PlayerHistory;

/// Maximum retry attempts before giving up completely.
const MAX_RETRY_ATTEMPTS: u32 = 10;

/// Initial backoff delay in milliseconds.
const INITIAL_BACKOFF_MS: u64 = 1000;

/// Maximum backoff delay in milliseconds (5 minutes).
const MAX_BACKOFF_MS: u64 = 300_000;

/// How long a bound listener that accepted nobody must stay up before its
/// end counts as a working session that resets the retry budget.
const MIN_HEALTHY_UPTIME: Duration = Duration::from_secs(60);

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
/// * `Ok(ShutdownReason::ListenerClosed)` - `MAX_RETRY_ATTEMPTS` consecutive binds or
///   sessions failed; the caller recovers with a fresh token and restarts it
/// * `Err(_)` - Unrecoverable configuration error (invalid signaling URL)
pub async fn run_transfer_server(
    signaling_url: &str,
    nethernet_id: u64,
    mc_token: &str,
    config: &AxeleratorConfig,
    shutdown: CancellationToken,
    stats: Option<Arc<TransferStats>>,
    player_history: Option<PlayerHistory>,
) -> anyhow::Result<ShutdownReason> {
    let stats = stats.unwrap_or_else(|| Arc::new(TransferStats::new()));
    let mut retry = RetryState::new();

    loop {
        // Check for shutdown before attempting connection
        if shutdown.is_cancelled() {
            info!("Transfer server shutdown requested before connect");
            return Ok(ShutdownReason::Requested);
        }

        let mut run = ListenerRun::default();
        match run_transfer_server_inner(
            signaling_url,
            nethernet_id,
            mc_token,
            config,
            shutdown.clone(),
            stats.clone(),
            player_history.clone(),
            &mut run,
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
            Err(e) => match classify_error(&e) {
                ErrorClass::Authentication => {
                    warn!("Transfer server authentication error: {}", e);
                    return Ok(ShutdownReason::AuthenticationError);
                }
                ErrorClass::Fatal => {
                    error!("Transfer server fatal error (not retrying): {}", e);
                    return Err(e);
                }
                ErrorClass::Transient => {
                    warn!("Transfer server error (will retry): {}", e);
                }
            },
        }

        stats.reconnect_attempts.fetch_add(1, Ordering::Relaxed);

        let Some(delay) = retry.next_delay(run.was_healthy()) else {
            error!(
                "Transfer server failed {} times in a row, handing back for recovery",
                MAX_RETRY_ATTEMPTS
            );
            return Ok(ShutdownReason::ListenerClosed);
        };

        info!(
            attempt = retry.attempts(),
            max_attempts = MAX_RETRY_ATTEMPTS,
            delay_secs = delay.as_secs_f32(),
            "Reconnecting transfer server..."
        );

        // Wait with shutdown check
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = shutdown.cancelled() => {
                info!("Shutdown requested during reconnect backoff");
                return Ok(ShutdownReason::Requested);
            }
        }
    }
}

/// What one listener session achieved before it ended.
#[derive(Debug, Default)]
struct ListenerRun {
    /// When `bind()` succeeded.
    bound_at: Option<tokio::time::Instant>,
    /// Whether at least one friend connection was accepted.
    accepted: bool,
}

impl ListenerRun {
    /// Whether this session proved the token and signaling URL work: it
    /// accepted a connection or stayed up for [`MIN_HEALTHY_UPTIME`]. A bind
    /// alone proves nothing, because the service accepts the WebSocket and
    /// then drops it at once when, for example, the Minecraft token expired.
    fn was_healthy(&self) -> bool {
        self.accepted
            || self
                .bound_at
                .is_some_and(|bound_at| bound_at.elapsed() >= MIN_HEALTHY_UPTIME)
    }
}

/// Consecutive-failure budget and backoff for rebinding the listener.
///
/// A session that worked (see [`ListenerRun::was_healthy`]) starts a fresh
/// sequence when it ends: the budget limits consecutive failures, not
/// failures over the process lifetime.
struct RetryState {
    attempts: u32,
    backoff_ms: u64,
}

impl RetryState {
    fn new() -> Self {
        Self {
            attempts: 0,
            backoff_ms: INITIAL_BACKOFF_MS,
        }
    }

    fn attempts(&self) -> u32 {
        self.attempts
    }

    /// Records a failure and returns the delay before the next attempt
    /// (backoff plus up to 25 % jitter), or `None` once
    /// `MAX_RETRY_ATTEMPTS` consecutive attempts have failed. `was_healthy`
    /// says the session that just ended worked, which restarts the count.
    fn next_delay(&mut self, was_healthy: bool) -> Option<Duration> {
        if was_healthy {
            *self = Self::new();
        }
        self.attempts += 1;
        if self.attempts >= MAX_RETRY_ATTEMPTS {
            return None;
        }
        let backoff_ms = self.backoff_ms;
        self.backoff_ms = (backoff_ms * 2).min(MAX_BACKOFF_MS);
        let jitter = rand::random::<u64>() % (backoff_ms / 4 + 1);
        Some(Duration::from_millis(backoff_ms + jitter))
    }
}

/// Reason the server shut down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownReason {
    /// Graceful shutdown was requested.
    Requested,
    /// The listener kept failing; restart it with a fresh token.
    ListenerClosed,
    /// Authentication error - token may need refresh.
    AuthenticationError,
}

/// Inner transfer server loop - handles a single connection session.
#[allow(clippy::too_many_arguments)]
async fn run_transfer_server_inner(
    signaling_url: &str,
    nethernet_id: u64,
    mc_token: &str,
    config: &AxeleratorConfig,
    shutdown: CancellationToken,
    stats: Arc<TransferStats>,
    player_history: Option<PlayerHistory>,
    run: &mut ListenerRun,
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
    run.bound_at = Some(tokio::time::Instant::now());

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
            _ = shutdown.cancelled() => {
                info!("Transfer server received shutdown signal");
                return Ok(ShutdownReason::Requested);
            }

            // Accept connections
            result = listener.accept() => {
                match result {
                    Ok(handshake_stream) => {
                        run.accepted = true;
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
                        let history = player_history.clone();

                        tokio::spawn(async move {
                            handle_transfer_connection(
                                handshake_stream,
                                &template,
                                &key,
                                host,
                                port,
                                stats,
                                history,
                            )
                            .await;
                        });
                    }
                    Err(e) => match accept_error_outcome(&e) {
                        Some(reason) => {
                            warn!("Transfer listener stopped accepting: {e}");
                            return Ok(reason);
                        }
                        None => warn!("Transient accept error, continuing: {e}"),
                    },
                }
            }
        }
    }
}

/// What an `accept()` error means for the transfer listener.
///
/// `ConnectionClosed` is how the listener reports that it stopped for good
/// (for example the franchise WebSocket closed); every later `accept()`
/// returns it again at once, so it must end this listener and trigger a
/// reconnect. Only errors jolyne marks recoverable keep the listener.
fn accept_error_outcome(error: &jolyne::JolyneError) -> Option<ShutdownReason> {
    if error.is_recoverable() {
        None
    } else {
        Some(ShutdownReason::ListenerClosed)
    }
}

/// How a transfer-server error is handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ErrorClass {
    /// The Minecraft token was rejected; the caller refreshes it.
    Authentication,
    /// Retrying cannot help.
    Fatal,
    /// Retry with backoff.
    Transient,
}

/// Classifies an error by its text (the signaling handshake reports HTTP
/// status only as text).
fn classify_error(e: &anyhow::Error) -> ErrorClass {
    let error_str = format!("{:?}", e);

    // Authentication failures - token may need refresh
    if error_str.contains("401")
        || error_str.contains("403")
        || error_str.contains("Unauthorized")
        || error_str.contains("Forbidden")
    {
        return ErrorClass::Authentication;
    }

    // Invalid configuration - truly fatal
    if error_str.contains("Invalid URL") || error_str.contains("missing host") {
        return ErrorClass::Fatal;
    }

    // Otherwise, assume it's transient/recoverable
    ErrorClass::Transient
}

/// Handles a single connection: performs handshake, resolves DNS, and sends transfer packet.
async fn handle_transfer_connection<T: jolyne::stream::transport::Transport>(
    handshake_stream: BedrockStream<jolyne::stream::Handshake, Server, T>,
    template: &WorldTemplate,
    key: &SecretKey,
    target_host: String,
    target_port: u16,
    stats: Arc<TransferStats>,
    player_history: Option<PlayerHistory>,
) {
    debug!("Starting Bedrock handshake...");

    match handshake_stream.accept_join_sequence(template, key).await {
        Ok((mut play_stream, login)) => {
            let display_name = login.identity.display_name();

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

                // Update player history on successful transfer
                if let Some(ref history) = player_history
                    && let Some(xuid) = login.identity.xuid()
                {
                    history.update_last_seen(xuid).await;
                    debug!(xuid, display_name, "Updated player history last-seen");
                }
            }

            // Give the client time to process the transfer, then close cleanly.
            tokio::time::sleep(Duration::from_millis(500)).await;
            if let Err(e) = play_stream.close().await {
                debug!("Closing transferred connection failed: {e}");
            }
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
        server_address: addr_str.into(),
        server_port: port,
        reload_world: false,
        gatherings_configuration: None,
    };

    let packet = McpePacket::from(transfer);
    stream
        .send_batch(&[packet], true)
        .await
        .map_err(|e| anyhow::anyhow!("Send failed: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The listener's `ConnectionClosed` used to be classified by matching
    /// lowercase "closed" in its Debug text ("ConnectionClosed"), so it was
    /// treated as transient and the accept loop spun forever.
    #[test]
    fn closed_listener_ends_the_accept_loop() {
        assert_eq!(
            accept_error_outcome(&jolyne::JolyneError::ConnectionClosed),
            Some(ShutdownReason::ListenerClosed)
        );
        assert_eq!(
            accept_error_outcome(&jolyne::JolyneError::Io(std::io::Error::other("boom"))),
            Some(ShutdownReason::ListenerClosed)
        );
    }

    /// The retry count used to grow for the whole process lifetime, so ten
    /// listener drops spread over weeks ended the transfer server.
    #[test]
    fn retry_budget_resets_after_a_successful_bind() {
        let mut retry = RetryState::new();
        for _ in 1..MAX_RETRY_ATTEMPTS {
            assert!(retry.next_delay(false).is_some());
        }
        // The listener worked for a while, then dropped.
        let delay = retry.next_delay(true).expect("a fresh failure sequence");
        assert_eq!(retry.attempts(), 1);
        assert!(delay < Duration::from_millis(INITIAL_BACKOFF_MS * 5 / 4 + 1));
    }

    /// Only a session that accepted a friend or stayed up for a minute
    /// counts as working; a bind alone does not.
    #[tokio::test(start_paused = true)]
    async fn healthy_session_needs_uptime_or_a_connection() {
        assert!(!ListenerRun::default().was_healthy());
        let run = ListenerRun {
            bound_at: Some(tokio::time::Instant::now()),
            accepted: false,
        };
        assert!(!run.was_healthy());
        tokio::time::advance(MIN_HEALTHY_UPTIME - Duration::from_secs(1)).await;
        assert!(!run.was_healthy());
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(run.was_healthy());
        let served = ListenerRun {
            bound_at: Some(tokio::time::Instant::now()),
            accepted: true,
        };
        assert!(served.was_healthy());
    }

    #[test]
    fn consecutive_failures_exhaust_the_budget() {
        let mut retry = RetryState::new();
        for _ in 1..MAX_RETRY_ATTEMPTS {
            assert!(retry.next_delay(false).is_some());
        }
        assert!(retry.next_delay(false).is_none());
    }

    #[test]
    fn backoff_doubles_up_to_the_cap() {
        let mut retry = RetryState::new();
        let first = retry.next_delay(false).unwrap();
        let second = retry.next_delay(false).unwrap();
        assert!(first >= Duration::from_millis(INITIAL_BACKOFF_MS));
        assert!(second >= Duration::from_millis(INITIAL_BACKOFF_MS * 2));
        retry.backoff_ms = MAX_BACKOFF_MS;
        assert!(retry.next_delay(false).unwrap() <= Duration::from_millis(MAX_BACKOFF_MS * 5 / 4));
    }

    #[test]
    fn handshake_errors_are_classified_by_status() {
        assert_eq!(
            classify_error(&anyhow::anyhow!("HTTP error: 401 Unauthorized")),
            ErrorClass::Authentication
        );
        assert_eq!(
            classify_error(&anyhow::anyhow!("Invalid URL: relative URL without a base")),
            ErrorClass::Fatal
        );
        assert_eq!(
            classify_error(&anyhow::anyhow!("connection reset")),
            ErrorClass::Transient
        );
    }

    /// A local franchise-signaling stand-in that completes the WebSocket
    /// handshake and closes it at once, as the service does when it drops
    /// the gateway's socket.
    async fn closing_signaling_stub() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "ws://{}/ws/v1.0/signaling/7",
            listener.local_addr().unwrap()
        );
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    if let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await {
                        let _ = socket.close(None).await;
                    }
                });
            }
        });
        url
    }

    /// End to end: when the franchise WebSocket closes, the listener must
    /// end with `ListenerClosed` (so it is rebound) instead of spinning on
    /// the `ConnectionClosed` that every later `accept()` returns at once.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn closed_signaling_socket_ends_the_listener() {
        let url = closing_signaling_stub().await;
        let task = tokio::spawn(async move {
            let mut run = ListenerRun::default();
            let reason = run_transfer_server_inner(
                &url,
                7,
                "test-token",
                &AxeleratorConfig::default(),
                CancellationToken::new(),
                Arc::new(TransferStats::new()),
                None,
                &mut run,
            )
            .await;
            (reason, run)
        });
        let (reason, run) = tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .expect("transfer listener kept accepting on a closed signaling socket")
            .unwrap();
        assert!(
            run.bound_at.is_some(),
            "the stub completes the WebSocket handshake"
        );
        assert!(!run.was_healthy(), "an immediate close proves nothing");
        assert_eq!(reason.unwrap(), ShutdownReason::ListenerClosed);
    }

    /// A service that accepts the WebSocket and drops it at once (for
    /// example on an expired Minecraft token) must not count as a working
    /// listener: after `MAX_RETRY_ATTEMPTS` such binds the server hands back
    /// `ListenerClosed` so the session recovers with a fresh token. Binding
    /// alone used to reset the budget, so this rebound forever.
    #[tokio::test(start_paused = true)]
    async fn bind_then_immediate_close_exhausts_the_retry_budget() {
        let url = closing_signaling_stub().await;
        let stats = Arc::new(TransferStats::new());
        // The paused clock auto-advances whenever the test waits on socket
        // I/O, so a virtual-time timeout would fire early. Bound the test in
        // real time instead: a rebind loop that never gives up is stopped by
        // this watchdog and reports `Requested`.
        let shutdown = CancellationToken::new();
        let watchdog = shutdown.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(20));
            watchdog.cancel();
        });
        let reason = run_transfer_server(
            &url,
            7,
            "test-token",
            &AxeleratorConfig::default(),
            shutdown,
            Some(stats.clone()),
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            reason,
            ShutdownReason::ListenerClosed,
            "a listener that binds and closes at once was rebound until shutdown ({} reconnects)",
            stats.reconnect_attempts.load(Ordering::Relaxed)
        );
        assert_eq!(
            stats.reconnect_attempts.load(Ordering::Relaxed),
            u64::from(MAX_RETRY_ATTEMPTS)
        );
    }
}
