//! Xbox Live session management for Axelerator.
//!
//! Axelerator always uses WebRTC (NetherNet) to advertise sessions to Xbox Live friends,
//! then transfers players to the actual RakNet server.

use crate::config::AxeleratorConfig;
use crate::screenshots::ScreenshotManager;
use crate::token_cache::TokenCache;
use crate::transfer::TransferStats;
use anyhow::{Context, Result};
use axolotl_xbl::{
    ExpandedSessionInfo, FriendsClient, PlayFabClient, PresenceClient, SessionClient, SessionInfo,
    discovery::DiscoveryClient,
};
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::sync::{Notify, RwLock};
use tracing::{debug, error, info, warn};

/// Presence refresh backoff configuration.
const PRESENCE_INITIAL_BACKOFF_SECS: u64 = 5;
const PRESENCE_MAX_BACKOFF_SECS: u64 = 300;
const PRESENCE_MAX_FAILURES: u32 = 5;

/// Main Axelerator broadcast server.
///
/// Advertises a Minecraft server via Xbox Live so friends can see and join it.
/// Uses WebRTC (NetherNet) for signaling, then transfers players to the actual server.
#[derive(Clone)]
pub struct Axelerator {
    config: AxeleratorConfig,
    session_info: Arc<RwLock<Option<ExpandedSessionInfo>>>,
    shutdown_notify: Arc<Notify>,
    transfer_shutdown: Arc<Notify>,
}

impl Axelerator {
    /// Create a new Axelerator instance.
    pub fn new(config: AxeleratorConfig) -> Self {
        Self {
            config,
            session_info: Arc::new(RwLock::new(None)),
            shutdown_notify: Arc::new(Notify::new()),
            transfer_shutdown: Arc::new(Notify::new()),
        }
    }

    /// Run the Axelerator broadcast server.
    pub async fn run(&self) -> Result<()> {
        info!(
            host = %self.config.host_name,
            transfer_to = format!("{}:{}", self.config.server_ip, self.config.server_port),
            "Starting Axelerator (WebRTC transfer mode)"
        );

        // Step 1: Authenticate with Xbox Live using TokenCache
        let token_cache = Arc::new(TokenCache::new(&self.config.token_cache_path));
        let xbl_token = token_cache
            .get_or_authenticate()
            .await
            .context("Failed to authenticate with Xbox Live")?;

        info!(
            gamertag = %xbl_token.gamertag(),
            xuid = %xbl_token.xuid,
            "Authenticated with Xbox Live"
        );

        // Step 1.5: Start RTA and Friend Manager
        let rta_client = Arc::new(axolotl_xbl::RtaClient::new(xbl_token.clone()));
        let friends_client = Arc::new(axolotl_xbl::FriendsClient::new());
        let session_client = Arc::new(SessionClient::new());

        // Track processed XUIDs to avoid duplicate invites (shared across RTA handler and periodic sync)
        let processed_xuids: Arc<RwLock<HashSet<String>>> = Arc::new(RwLock::new(HashSet::new()));

        let token_cache_for_rta = token_cache.clone();
        let friends_client_clone = friends_client.clone();
        let session_client_clone = session_client.clone();
        let session_info_clone = self.session_info.clone();
        let processed_xuids_clone = processed_xuids.clone();

        // Handle RTA events
        rta_client
            .on_event(move |data| {
                if let Some(msg_type) = data.get("NotificationType").and_then(|v| v.as_str())
                    && msg_type == "IncomingFriendRequestCountChanged"
                {
                    info!("Received friend request notification");
                    let friends = friends_client_clone.clone();
                    let sessions = session_client_clone.clone();
                    let token_cache = token_cache_for_rta.clone();
                    let session_info = session_info_clone.clone();
                    let processed = processed_xuids_clone.clone();
                    tokio::spawn(async move {
                        accept_friends_and_invite(
                            &friends,
                            &sessions,
                            &token_cache,
                            session_info,
                            &processed,
                        )
                        .await;
                    });
                }
            })
            .await;

        // Store RTA handle for reconnection
        // For long-running services, we retry indefinitely with capped backoff
        let rta_run = rta_client.clone();
        let rta_shutdown = self.shutdown_notify.clone();
        let rta_handle = tokio::spawn(async move {
            let mut retry_count = 0u32;
            let mut backoff_ms = 1000u64;
            const MAX_BACKOFF_MS: u64 = 5 * 60 * 1000; // 5 minutes max

            loop {
                tokio::select! {
                    _ = rta_shutdown.notified() => {
                        info!("RTA client shutdown requested");
                        break;
                    }
                    result = rta_run.connect_and_run() => {
                        match result {
                            Ok(_) => {
                                info!("RTA client disconnected normally");
                                // Reset backoff on clean disconnect (might just be server maintenance)
                                backoff_ms = 1000;
                            }
                            Err(e) => {
                                warn!("RTA client error: {}", e);
                            }
                        }

                        retry_count += 1;

                        // Log more urgently after many failures
                        let delay = Duration::from_millis(backoff_ms);
                        if retry_count <= 5 {
                            info!(attempt = retry_count, delay_secs = delay.as_secs_f32(), "Reconnecting RTA client...");
                        } else if retry_count.is_multiple_of(10) {
                            warn!(attempt = retry_count, delay_secs = delay.as_secs_f32(), "RTA client reconnecting (many failures)...");
                        } else {
                            debug!(attempt = retry_count, delay_secs = delay.as_secs_f32(), "Reconnecting RTA client...");
                        }

                        tokio::time::sleep(delay).await;
                        backoff_ms = (backoff_ms * 2).min(MAX_BACKOFF_MS);
                    }
                }
            }
        });

        // Wait for RTA to connect and get ID
        info!("Waiting for RTA connection...");
        let connection_id = match rta_client.wait_for_connection_id().await {
            Ok(id) => id,
            Err(e) => {
                warn!(
                    "RTA connection timed out, using fallback UUID (this may cause issues): {}",
                    e
                );
                uuid::Uuid::new_v4().to_string()
            }
        };

        // Step 2: Set presence to active
        let presence = PresenceClient::new();
        let heartbeat = presence
            .set_active(&xbl_token)
            .await
            .context("Failed to set presence")?;
        info!(heartbeat, "Presence set to active");

        // Initial friend sync (invites sent after session is created below)

        // Step 3: Create session (always WebRTC mode)
        let mut session_info = self.create_session_info(&xbl_token.xuid);
        session_info.connection_id = connection_id;

        session_client
            .create_session(&xbl_token, &session_info)
            .await
            .context("Failed to create session")?;

        let handle_id = session_client
            .create_handle(&xbl_token, &session_info)
            .await
            .context("Failed to create session handle")?;

        info!(
            session_id = %session_info.session_id,
            handle_id = %handle_id,
            nethernet_id = session_info.nethernet_id,
            "Session created - server is now visible to friends!"
        );

        // Store session info with handle_id
        session_info.handle_id = Some(handle_id.clone());
        {
            let mut info = self.session_info.write().await;
            *info = Some(session_info.clone());
        }

        // Initial friend sync - now that session is created, accept pending requests and send invites
        accept_friends_and_invite(
            &friends_client,
            &session_client,
            &token_cache,
            self.session_info.clone(),
            &processed_xuids,
        )
        .await;

        // Also sync followers on startup (catches people who followed us while we were offline)
        sync_followers(
            &friends_client,
            &session_client,
            &token_cache,
            &self.session_info,
            &processed_xuids,
        )
        .await;

        // Start periodic friend sync task (catches followers that RTA might miss, e.g. PS5 users)
        let friend_sync_handle = if self.config.friend_sync_interval > 0 {
            let interval = Duration::from_secs(self.config.friend_sync_interval);
            let friends = friends_client.clone();
            let sessions = session_client.clone();
            let token_cache_for_sync = token_cache.clone();
            let session_info_for_sync = self.session_info.clone();
            let shutdown = self.shutdown_notify.clone();
            let processed_for_sync = processed_xuids.clone();

            Some(tokio::spawn(async move {
                run_periodic_friend_sync(
                    interval,
                    friends,
                    sessions,
                    token_cache_for_sync,
                    session_info_for_sync,
                    shutdown,
                    processed_for_sync,
                )
                .await;
            }))
        } else {
            None
        };

        // Step 4: Run WebRTC signaling and transfer players
        let playfab_token = token_cache
            .get_xbl_token(axolotl_xbl::auth::relying_party::PLAYFAB)
            .await?;

        let result = self
            .run_signaling_loop(
                &xbl_token,
                &playfab_token,
                &session_info,
                &handle_id,
                heartbeat,
                &session_client,
            )
            .await;

        // Graceful cleanup
        info!("Cleaning up session...");

        // Stop RTA
        rta_client.shutdown().await;
        rta_handle.abort();

        // Stop friend sync
        if let Some(handle) = friend_sync_handle {
            handle.abort();
        }

        // Set presence to inactive (best effort)
        if let Err(e) = presence.set_inactive(&xbl_token).await {
            warn!("Failed to set presence inactive: {}", e);
        } else {
            debug!("Presence set to inactive");
        }

        info!("Cleanup complete");
        result
    }

    /// Create session info from config (always WebRTC mode).
    fn create_session_info(&self, xuid: &str) -> ExpandedSessionInfo {
        let info = SessionInfo {
            host_name: self.config.host_name.clone(),
            world_name: self.config.world_name.clone(),
            version: self.config.version.clone(),
            protocol: self.config.protocol,
            players: self.config.display_players.max(1), // Cosmetic player count
            max_players: self.config.max_players,
            ip: self.config.server_ip.clone(),
            port: self.config.server_port,
        };

        // Always use WebRTC mode (is_raknet = false)
        ExpandedSessionInfo::new(xuid.to_string(), info)
    }

    /// Run the WebRTC signaling loop and transfer players to the actual server.
    async fn run_signaling_loop(
        &self,
        xbl_token: &axolotl_xbl::XblToken,
        playfab_token: &axolotl_xbl::XblToken,
        session: &ExpandedSessionInfo,
        handle_id: &str,
        initial_heartbeat_secs: u64,
        session_client: &SessionClient,
    ) -> Result<()> {
        // Get PlayFab token for signaling
        let playfab = PlayFabClient::new();
        let playfab_ticket = playfab
            .login(&playfab_token.user_hash, &playfab_token.token)
            .await
            .context("PlayFab login failed")?;

        let mc_token = playfab
            .start_session(&session.device_id, &playfab_ticket)
            .await
            .context("Minecraft session start failed")?;

        info!("Got Minecraft token for signaling");

        // Discover signaling endpoint for regional routing
        let discovery = DiscoveryClient::new();
        let endpoints = discovery
            .discover(&self.config.version)
            .await
            .context("Failed to discover service endpoints")?;

        let nethernet_id = session.nethernet_id;
        let signaling_url = endpoints.signaling.websocket_url(nethernet_id);

        info!(
            signaling_uri = %endpoints.signaling.service_uri,
            "Discovered signaling endpoint"
        );

        // Create transfer stats tracker
        let transfer_stats = Arc::new(TransferStats::new());

        // Spawn the transfer server with proper shutdown handling
        let config = self.config.clone();
        let mc_token_clone = mc_token.clone();
        let signaling_url_clone = signaling_url.clone();
        let transfer_shutdown = self.transfer_shutdown.clone();
        let stats_clone = transfer_stats.clone();

        let mut transfer_handle = tokio::spawn(async move {
            crate::transfer::run_transfer_server(
                &signaling_url_clone,
                nethernet_id,
                &mc_token_clone,
                &config,
                transfer_shutdown,
                Some(stats_clone),
            )
            .await
        });

        let presence = PresenceClient::new();
        let friends_client = FriendsClient::new();

        // Presence refresh with exponential backoff
        let mut heartbeat_secs = initial_heartbeat_secs;
        let mut presence_failures = 0u32;
        let mut presence_backoff_secs = PRESENCE_INITIAL_BACKOFF_SECS;
        let mut last_presence_refresh = Instant::now();

        // Tracking for tampering detection
        let monitor_enabled = self.config.monitor_tampering;
        let monitor_interval = self.config.monitor_interval;
        let auto_block = self.config.auto_block_attackers;
        let mut last_monitor_check = Instant::now();
        let mut tamper_count: u32 = 0;
        let mut hijack_count: u32 = 0;
        let mut blocked_xuids: HashSet<String> = HashSet::new();
        const MAX_BLOCKED_XUIDS: usize = 1000; // Prevent unbounded growth

        // Current handle_id (mutable for recreation)
        let mut current_handle_id = handle_id.to_string();

        // Screenshot/showcase image management
        let mut screenshot_manager = ScreenshotManager::new(self.config.screenshots.clone());
        if let Err(e) = screenshot_manager.init() {
            warn!("Failed to initialize screenshot manager: {}", e);
        }
        let mut last_screenshot_check = Instant::now();

        if screenshot_manager.is_enabled() {
            info!(
                count = screenshot_manager.image_count(),
                cycle_interval = screenshot_manager.cycle_interval_secs(),
                "Screenshot feature enabled"
            );
        }

        if monitor_enabled {
            info!(
                interval = monitor_interval,
                auto_block = auto_block,
                "Session monitoring enabled (tampering + hijacking detection)"
            );
        }

        loop {
            tokio::select! {
                biased;

                // Prioritize shutdown signal
                _ = self.shutdown_notify.notified() => {
                    info!("Shutdown signal received");
                    self.transfer_shutdown.notify_waiters();
                    // Give transfer server time to shutdown gracefully
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    break;
                }

                // Transfer server completion
                res = &mut transfer_handle => {
                    match res {
                        Ok(Ok(reason)) => {
                            use crate::transfer::ShutdownReason;
                            match reason {
                                ShutdownReason::Requested => {
                                    info!("Transfer server shutdown completed");
                                }
                                ShutdownReason::AuthenticationError => {
                                    // Token expired - in a production setup, we'd refresh here
                                    // For now, just log and exit (restart will get fresh token)
                                    error!("Transfer server authentication failed - token may be expired");
                                    error!("Restart the service to refresh tokens");
                                }
                                ShutdownReason::ListenerClosed | ShutdownReason::FatalError => {
                                    error!("Transfer server stopped unexpectedly: {:?}", reason);
                                }
                            }
                        }
                        Ok(Err(e)) => error!("Transfer server error: {}", e),
                        Err(e) => error!("Transfer server task panicked: {}", e),
                    }
                    // Transfer server died - this is a critical failure
                    // The outer retry logic should have handled transient errors
                    error!("Transfer server stopped - shutting down");
                    break;
                }

                // Main tick (1 second interval)
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    let now = Instant::now();

                    // Log transfer stats periodically (only compute summary if debug is enabled)
                    if tracing::enabled!(tracing::Level::DEBUG)
                        && transfer_stats.connections_accepted.load(Ordering::Relaxed) > 0
                    {
                        debug!(stats = %transfer_stats.summary(), "Transfer server stats");
                    }

                    // Presence refresh with backoff
                    if now.duration_since(last_presence_refresh).as_secs() >= heartbeat_secs {
                        match presence.set_active(xbl_token).await {
                            Ok(new_heartbeat) => {
                                heartbeat_secs = new_heartbeat;
                                presence_failures = 0;
                                presence_backoff_secs = PRESENCE_INITIAL_BACKOFF_SECS;
                                last_presence_refresh = now;
                                debug!(heartbeat = heartbeat_secs, "Presence refreshed");
                            }
                            Err(e) => {
                                presence_failures += 1;
                                warn!(
                                    failures = presence_failures,
                                    max = PRESENCE_MAX_FAILURES,
                                    "Failed to refresh presence: {}", e
                                );

                                if presence_failures >= PRESENCE_MAX_FAILURES {
                                    error!("Presence refresh failed too many times, session may be invisible");
                                    // Reset counter but don't crash - session might still work
                                    presence_failures = 0;
                                }

                                // Use backoff for next attempt
                                heartbeat_secs = presence_backoff_secs;
                                presence_backoff_secs = (presence_backoff_secs * 2).min(PRESENCE_MAX_BACKOFF_SECS);
                                last_presence_refresh = now;
                            }
                        }
                    }

                    // Screenshot upload check
                    if screenshot_manager.is_enabled() {
                        let should_upload = if screenshot_manager.image_count() > 1 {
                            // Multiple images - check cycle interval
                            screenshot_manager.should_upload()
                                || now.duration_since(last_screenshot_check).as_secs()
                                    >= screenshot_manager.cycle_interval_secs()
                        } else {
                            // Single image - only upload on startup if enabled
                            screenshot_manager.should_upload()
                        };

                        if should_upload {
                            last_screenshot_check = now;
                            match screenshot_manager
                                .upload_screenshot(&mc_token, &session.xuid)
                                .await
                            {
                                Ok(uploaded) => {
                                    if uploaded {
                                        debug!("Screenshot uploaded/cycled");
                                    }
                                }
                                Err(e) => {
                                    warn!("Failed to upload screenshot: {}", e);
                                }
                            }
                        }
                    }

                    // Monitoring checks
                    if monitor_enabled && now.duration_since(last_monitor_check).as_secs() >= monitor_interval {
                        last_monitor_check = now;

                        // 1. Session tampering detection
                        match session_client.check_tampering_via_handle(xbl_token, session, &current_handle_id).await {
                            Ok(result) => {
                                if result.tampered {
                                    tamper_count += 1;
                                    error!(
                                        "SESSION TAMPERING DETECTED! (occurrence #{})",
                                        tamper_count
                                    );

                                    let mut attacker_xuids = Vec::new();
                                    for field in &result.modified_fields {
                                        error!(
                                            "  Field '{}': expected '{}', got '{}'",
                                            field.field, field.expected, field.actual
                                        );

                                        if field.field == "members" && field.actual.contains("unknown member joined:") && let Some(xuid) = field.actual.strip_prefix("unknown member joined: ") {
                                                attacker_xuids.push(xuid.to_string());
                                        }
                                    }

                                    for attacker_xuid in &attacker_xuids {
                                        if blocked_xuids.contains(attacker_xuid) {
                                            continue;
                                        }

                                        let gamertag = friends_client
                                            .get_gamertag(xbl_token, attacker_xuid)
                                            .await
                                            .unwrap_or_else(|_| "Unknown".to_string());

                                        error!(
                                            "POTENTIAL ATTACKER: {} (XUID: {})",
                                            gamertag, attacker_xuid
                                        );

                                        if auto_block {
                                            info!("Auto-blocking attacker {}...", gamertag);
                                            match friends_client.force_remove_follower(xbl_token, attacker_xuid).await {
                                                Ok(_) => {
                                                    info!("Blocked attacker: {} ({})", gamertag, attacker_xuid);
                                                    // Limit blocked_xuids to prevent unbounded memory growth
                                                    if blocked_xuids.len() < MAX_BLOCKED_XUIDS {
                                                        blocked_xuids.insert(attacker_xuid.clone());
                                                    } else {
                                                        warn!("Blocked XUIDs limit reached ({}), not tracking additional blocked users", MAX_BLOCKED_XUIDS);
                                                    }
                                                }
                                                Err(e) => {
                                                    warn!("Failed to block attacker {}: {}", attacker_xuid, e);
                                                }
                                            }
                                        }
                                    }

                                    info!("Attempting auto-repair...");
                                    match session_client.repair_session(xbl_token, session).await {
                                        Ok(_) => info!("Session repaired successfully"),
                                        Err(e) => error!("Failed to repair session: {}", e),
                                    }
                                } else {
                                    debug!("Session integrity check passed");
                                }
                            }
                            Err(e) => {
                                warn!("Failed to check session tampering: {}", e);
                            }
                        }

                        // 2. Handle hijacking detection
                        match session_client.check_handle_hijacking(
                            xbl_token,
                            &session.xuid,
                            &current_handle_id,
                            &session.session_id,
                        ).await {
                            Ok(result) => {
                                if result.hijack_detected {
                                    hijack_count += 1;
                                    error!(
                                        "HANDLE HIJACKING DETECTED! (occurrence #{})",
                                        hijack_count
                                    );
                                    error!(
                                        "  Our handle exists: {}, Total handles for XUID: {}",
                                        result.our_handle_exists, result.total_handles_for_xuid
                                    );

                                    for suspicious in &result.suspicious_handles {
                                        error!(
                                            "  Suspicious handle: {} -> session {} ({})",
                                            suspicious.handle_id, suspicious.session_id, suspicious.reason
                                        );
                                    }

                                    if !result.our_handle_exists {
                                        warn!("Our handle was deleted! Recreating...");
                                        match session_client.create_handle(xbl_token, session).await {
                                            Ok(new_handle) => {
                                                info!("Handle recreated: {}", new_handle);
                                                current_handle_id = new_handle;
                                            }
                                            Err(e) => {
                                                error!("Failed to recreate handle: {}", e);
                                            }
                                        }
                                    }
                                } else {
                                    debug!("Handle integrity check passed");
                                }
                            }
                            Err(e) => {
                                warn!("Failed to check handle hijacking: {}", e);
                            }
                        }
                    }
                }
            }
        }

        // Summary at shutdown
        info!(stats = %transfer_stats.summary(), "Transfer server final stats");

        if monitor_enabled {
            if tamper_count > 0 || hijack_count > 0 {
                warn!(
                    "Security summary: {} tampering event(s), {} hijacking event(s), {} attacker(s) blocked",
                    tamper_count,
                    hijack_count,
                    blocked_xuids.len()
                );
            }
            if !blocked_xuids.is_empty() {
                info!("Blocked attackers: {:?}", blocked_xuids);
            }
        }

        Ok(())
    }

    /// Request shutdown.
    pub async fn shutdown(&self) {
        info!("Initiating graceful shutdown...");
        self.transfer_shutdown.notify_waiters();
        self.shutdown_notify.notify_waiters();
    }

    /// Get current session info (if available).
    pub async fn session_info(&self) -> Option<ExpandedSessionInfo> {
        self.session_info.read().await.clone()
    }
}

/// Accept pending friend requests and send game invites to each new friend.
///
/// This matches the behavior of Broadcaster (mcxboxbroadcast) which sends an initial
/// game invite when someone adds the account as a friend.
///
/// On 401/403 errors, the XBL token is refreshed and the operation is retried once.
async fn accept_friends_and_invite(
    friends_client: &FriendsClient,
    session_client: &SessionClient,
    token_cache: &TokenCache,
    session_info: Arc<RwLock<Option<ExpandedSessionInfo>>>,
    processed_xuids: &Arc<RwLock<HashSet<String>>>,
) {
    // Get current token
    let token = match token_cache.get_or_authenticate().await {
        Ok(t) => t,
        Err(e) => {
            warn!("Failed to get XBL token: {}", e);
            return;
        }
    };

    // Get pending friend requests (with retry on auth error)
    let requests = match friends_client.get_incoming_requests(&token).await {
        Ok(r) => r,
        Err(e) if e.is_auth_error() => {
            warn!("Auth error getting friend requests, refreshing token...");
            let token = match token_cache.force_refresh_xbl().await {
                Ok(t) => t,
                Err(e) => {
                    warn!("Failed to refresh XBL token: {}", e);
                    return;
                }
            };
            match friends_client.get_incoming_requests(&token).await {
                Ok(r) => r,
                Err(e) => {
                    warn!("Failed to get friend requests after token refresh: {}", e);
                    return;
                }
            }
        }
        Err(e) => {
            warn!("Failed to get friend requests: {}", e);
            return;
        }
    };

    if requests.is_empty() {
        return;
    }

    info!("Accepting {} friend requests...", requests.len());

    // Get fresh token for accepting requests
    let token = match token_cache.get_or_authenticate().await {
        Ok(t) => t,
        Err(e) => {
            warn!("Failed to get XBL token: {}", e);
            return;
        }
    };

    // Accept all requests (with retry on auth error)
    if let Err(e) = friends_client
        .accept_requests(&token, requests.clone())
        .await
    {
        if e.is_auth_error() {
            warn!("Auth error accepting friend requests, refreshing token...");
            let token = match token_cache.force_refresh_xbl().await {
                Ok(t) => t,
                Err(e) => {
                    warn!("Failed to refresh XBL token: {}", e);
                    return;
                }
            };
            if let Err(e) = friends_client
                .accept_requests(&token, requests.clone())
                .await
            {
                warn!(
                    "Failed to accept friend requests after token refresh: {}",
                    e
                );
                return;
            }
        } else {
            warn!("Failed to accept friend requests: {}", e);
            return;
        }
    }

    info!("Friend requests accepted!");

    // Mark all as processed
    {
        let mut processed = processed_xuids.write().await;
        for xuid in &requests {
            processed.insert(xuid.clone());
        }
    }

    // Get session info for sending invites
    let session = match session_info.read().await.clone() {
        Some(s) => s,
        None => {
            debug!("Session not yet created, skipping invites");
            return;
        }
    };

    // Get fresh token for invites
    let token = match token_cache.get_or_authenticate().await {
        Ok(t) => t,
        Err(e) => {
            warn!("Failed to get XBL token for invites: {}", e);
            return;
        }
    };

    // Send invite to each newly accepted friend (with retry on auth error)
    for xuid in requests {
        match session_client.send_invite(&token, &session, &xuid).await {
            Ok(_) => info!("Sent game invite to {}", xuid),
            Err(e) if e.is_auth_error() => {
                warn!("Auth error sending invite to {}, refreshing token...", xuid);
                let token = match token_cache.force_refresh_xbl().await {
                    Ok(t) => t,
                    Err(e) => {
                        warn!("Failed to refresh XBL token: {}", e);
                        continue;
                    }
                };
                match session_client.send_invite(&token, &session, &xuid).await {
                    Ok(_) => info!("Sent game invite to {} (after token refresh)", xuid),
                    Err(e) => warn!(
                        "Failed to send invite to {} after token refresh: {}",
                        xuid, e
                    ),
                }
            }
            Err(e) => warn!("Failed to send invite to {}: {}", xuid, e),
        }
    }
}

/// Periodic friend sync loop.
///
/// This runs every `interval` and checks for followers who we haven't followed back.
/// This catches cases where RTA notifications are missed (common with PS5 users).
async fn run_periodic_friend_sync(
    interval: Duration,
    friends_client: Arc<FriendsClient>,
    session_client: Arc<SessionClient>,
    token_cache: Arc<TokenCache>,
    session_info: Arc<RwLock<Option<ExpandedSessionInfo>>>,
    shutdown: Arc<Notify>,
    processed_xuids: Arc<RwLock<HashSet<String>>>,
) {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // Skip first immediate tick (we already did initial sync)
    ticker.tick().await;

    loop {
        tokio::select! {
            biased;

            _ = shutdown.notified() => {
                debug!("Friend sync shutting down");
                break;
            }

            _ = ticker.tick() => {
                sync_followers(&friends_client, &session_client, &token_cache, &session_info, &processed_xuids).await;
            }
        }
    }
}

/// Check followers and auto-follow back anyone who follows us but we don't follow.
///
/// Only processes new followers that haven't been seen before (tracked in processed_xuids).
/// On 401/403 errors, the XBL token is refreshed and the operation is retried once.
async fn sync_followers(
    friends_client: &FriendsClient,
    session_client: &SessionClient,
    token_cache: &TokenCache,
    session_info: &Arc<RwLock<Option<ExpandedSessionInfo>>>,
    processed_xuids: &Arc<RwLock<HashSet<String>>>,
) {
    // Get current token
    let token = match token_cache.get_or_authenticate().await {
        Ok(t) => t,
        Err(e) => {
            debug!("Failed to get XBL token: {}", e);
            return;
        }
    };

    // Get our followers (with retry on auth error)
    let followers = match friends_client.get_followers(&token).await {
        Ok(f) => f,
        Err(e) if e.is_auth_error() => {
            warn!("Auth error getting followers, refreshing token...");
            let token = match token_cache.force_refresh_xbl().await {
                Ok(t) => t,
                Err(e) => {
                    warn!("Failed to refresh XBL token: {}", e);
                    return;
                }
            };
            match friends_client.get_followers(&token).await {
                Ok(f) => f,
                Err(e) => {
                    debug!("Failed to get followers after token refresh: {}", e);
                    return;
                }
            }
        }
        Err(e) => {
            debug!("Failed to get followers: {}", e);
            return;
        }
    };

    // Find people who follow us but we don't follow back AND haven't been processed yet
    let processed = processed_xuids.read().await;
    let to_follow: Vec<_> = followers
        .iter()
        .filter(|p| p.is_following_caller && !p.is_followed_by_caller)
        .filter(|p| !is_guest_account(&p.xuid)) // Skip split-screen/guest accounts
        .filter(|p| !processed.contains(&p.xuid)) // Skip already processed
        .cloned()
        .collect();
    drop(processed);

    if to_follow.is_empty() {
        return;
    }

    info!("Found {} new followers to follow back", to_follow.len());

    // Build XUID list and gamertag lookup
    let xuids: Vec<String> = to_follow.iter().map(|p| p.xuid.clone()).collect();
    let gamertag_map: std::collections::HashMap<String, String> = to_follow
        .iter()
        .map(|p| (p.xuid.clone(), p.gamertag.clone()))
        .collect();

    // Get fresh token for adding friends
    let token = match token_cache.get_or_authenticate().await {
        Ok(t) => t,
        Err(e) => {
            warn!("Failed to get XBL token: {}", e);
            return;
        }
    };

    // Bulk add all friends at once (with retry on auth error)
    let added_xuids = match friends_client.add_friends_bulk(&token, &xuids).await {
        Ok(added) => {
            for xuid in &added {
                let gamertag = gamertag_map
                    .get(xuid)
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");
                info!("Followed back {} ({})", gamertag, xuid);
            }
            added
        }
        Err(e) if e.is_auth_error() => {
            warn!("Auth error adding friends, refreshing token...");
            let token = match token_cache.force_refresh_xbl().await {
                Ok(t) => t,
                Err(e) => {
                    warn!("Failed to refresh XBL token: {}", e);
                    return;
                }
            };
            match friends_client.add_friends_bulk(&token, &xuids).await {
                Ok(added) => {
                    for xuid in &added {
                        let gamertag = gamertag_map
                            .get(xuid)
                            .map(|s| s.as_str())
                            .unwrap_or("Unknown");
                        info!(
                            "Followed back {} ({}) (after token refresh)",
                            gamertag, xuid
                        );
                    }
                    added
                }
                Err(e) => {
                    warn!("Failed to bulk add friends after token refresh: {}", e);
                    return;
                }
            }
        }
        Err(e) => {
            let err_str = e.to_string();
            if err_str.contains("Rate limited") {
                warn!("Rate limited while following friends, will retry next cycle");
            } else {
                warn!("Failed to bulk add friends: {}", e);
            }
            return;
        }
    };

    // Mark successfully added as processed
    {
        let mut processed = processed_xuids.write().await;
        for xuid in &added_xuids {
            processed.insert(xuid.clone());
        }
    }

    // Send invites to all successfully added friends
    let session = match session_info.read().await.clone() {
        Some(s) => s,
        None => {
            debug!("Session not yet created, skipping invites");
            return;
        }
    };

    // Get fresh token for invites
    let token = match token_cache.get_or_authenticate().await {
        Ok(t) => t,
        Err(e) => {
            warn!("Failed to get XBL token for invites: {}", e);
            return;
        }
    };

    for xuid in &added_xuids {
        let gamertag = gamertag_map
            .get(xuid)
            .map(|s| s.as_str())
            .unwrap_or("Unknown");
        match session_client.send_invite(&token, &session, xuid).await {
            Ok(_) => info!("Sent game invite to {}", gamertag),
            Err(e) if e.is_auth_error() => {
                warn!(
                    "Auth error sending invite to {}, refreshing token...",
                    gamertag
                );
                let token = match token_cache.force_refresh_xbl().await {
                    Ok(t) => t,
                    Err(e) => {
                        warn!("Failed to refresh XBL token: {}", e);
                        continue;
                    }
                };
                match session_client.send_invite(&token, &session, xuid).await {
                    Ok(_) => info!("Sent game invite to {} (after token refresh)", gamertag),
                    Err(e) => warn!(
                        "Failed to send invite to {} after token refresh: {}",
                        gamertag, e
                    ),
                }
            }
            Err(e) => warn!("Failed to send invite to {}: {}", gamertag, e),
        }
    }
}

/// Check if an XUID is a guest/split-screen account.
/// Guest accounts have bit 52 set to 1.
fn is_guest_account(xuid: &str) -> bool {
    xuid.parse::<u64>().map(|x| (x >> 52) == 1).unwrap_or(false)
}
