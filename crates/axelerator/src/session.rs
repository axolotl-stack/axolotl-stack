//! Xbox Live session management for Axelerator.
//!
//! Axelerator always uses WebRTC (NetherNet) to advertise sessions to Xbox Live friends,
//! then transfers players to the actual RakNet server.

use crate::config::AxeleratorConfig;
use crate::token_cache::TokenCache;
use anyhow::{Context, Result};
use axolotl_xbl::{
    ExpandedSessionInfo, FriendsClient, PlayFabClient, PresenceClient, SessionClient, SessionInfo,
};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::{Notify, RwLock};
use tracing::{debug, error, info, warn};

/// Main Axelerator broadcast server.
///
/// Advertises a Minecraft server via Xbox Live so friends can see and join it.
/// Uses WebRTC (NetherNet) for signaling, then transfers players to the actual server.
#[derive(Clone)]
pub struct Axelerator {
    config: AxeleratorConfig,
    session_info: Arc<RwLock<Option<ExpandedSessionInfo>>>,
    shutdown_notify: Arc<Notify>,
}

impl Axelerator {
    /// Create a new Axelerator instance.
    pub fn new(config: AxeleratorConfig) -> Self {
        Self {
            config,
            session_info: Arc::new(RwLock::new(None)),
            shutdown_notify: Arc::new(Notify::new()),
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
        let token_cache = TokenCache::new(&self.config.token_cache_path);
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
        let rta_token = xbl_token.clone();
        let friends_client_clone = friends_client.clone();

        // Handle RTA events
        rta_client
            .on_event(move |data| {
                if let Some(msg_type) = data.get("NotificationType").and_then(|v| v.as_str()) {
                    if msg_type == "IncomingFriendRequestCountChanged" {
                        info!("Received friend request notification");
                        let client = friends_client_clone.clone();
                        let token = rta_token.clone();
                        tokio::spawn(async move {
                            if let Ok(requests) = client.get_incoming_requests(&token).await {
                                if !requests.is_empty() {
                                    info!("Accepting {} friend requests...", requests.len());
                                    if let Err(e) = client.accept_requests(&token, requests).await {
                                        warn!("Failed to accept requests: {}", e);
                                    } else {
                                        info!("Friend requests accepted!");
                                    }
                                }
                            }
                        });
                    }
                }
            })
            .await;

        let rta_run = rta_client.clone();
        tokio::spawn(async move {
            if let Err(e) = rta_run.connect_and_run().await {
                warn!("RTA client error: {}", e);
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
            .set_active(xbl_token)
            .await
            .context("Failed to set presence")?;
        info!(heartbeat, "Presence set to active");

        // Initial friend sync
        if let Ok(requests) = friends_client.get_incoming_requests(&xbl_token).await {
            if !requests.is_empty() {
                info!("Found {} pending friend requests", requests.len());
                friends_client
                    .accept_requests(&xbl_token, requests)
                    .await
                    .ok();
            }
        }

        // Step 3: Create session (always WebRTC mode)
        let mut session_info = self.create_session_info(&xbl_token.xuid);
        session_info.connection_id = connection_id; // Set the real RTA connection ID

        let session_client = SessionClient::new();

        session_client
            .create_session(xbl_token, &session_info)
            .await
            .context("Failed to create session")?;

        let handle_id = session_client
            .create_handle(xbl_token, &session_info)
            .await
            .context("Failed to create session handle")?;

        info!(
            session_id = %session_info.session_id,
            handle_id = %handle_id,
            nethernet_id = session_info.nethernet_id,
            "Session created - server is now visible to friends!"
        );

        // Store session info with handle_id
        let mut session_info = session_info;
        session_info.handle_id = Some(handle_id.clone());
        {
            let mut info = self.session_info.write().await;
            *info = Some(session_info.clone());
        }

        // Step 4: Run WebRTC signaling and transfer players
        let playfab_token = token_cache
            .get_xbl_token(axolotl_xbl::auth::relying_party::PLAYFAB)
            .await?;

        self.run_signaling_loop(xbl_token, &playfab_token, &session_info, &handle_id, heartbeat, session_client)
            .await?;

        rta_client.shutdown().await;

        Ok(())
    }

    /// Create session info from config (always WebRTC mode).
    fn create_session_info(&self, xuid: &str) -> ExpandedSessionInfo {
        let info = SessionInfo {
            host_name: self.config.host_name.clone(),
            world_name: self.config.world_name.clone(),
            version: self.config.version.clone(),
            protocol: self.config.protocol,
            players: 1,
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
        mut heartbeat_secs: u64,
        session_client: SessionClient,
    ) -> Result<()> {
        // Get PlayFab token for signaling
        // NOTE: Must use playfab_token's user_hash (not xbl_token's) - they differ per RP!
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

        // Spawn the transfer server that handles incoming WebRTC connections
        // Uses the builder API which connects to Xbox signaling internally
        let config = self.config.clone();
        let nethernet_id = session.nethernet_id;
        let mc_token_clone = mc_token.clone();
        let mut transfer_handle = tokio::spawn(async move {
            if let Err(e) =
                crate::transfer::run_transfer_server(nethernet_id, &mc_token_clone, &config).await
            {
                tracing::error!("Transfer server error: {:?}", e);
            }
        });

        let presence = PresenceClient::new();
        let friends_client = FriendsClient::new();

        // Tracking for tampering detection
        let monitor_enabled = self.config.monitor_tampering;
        let monitor_interval = self.config.monitor_interval;
        let auto_block = self.config.auto_block_attackers;
        let mut last_monitor_check = std::time::Instant::now();
        let mut tamper_count: u32 = 0;
        let mut hijack_count: u32 = 0;
        let mut blocked_xuids: HashSet<String> = HashSet::new();

        if monitor_enabled {
            info!(
                interval = monitor_interval,
                auto_block = auto_block,
                "Session monitoring enabled (tampering + hijacking detection)"
            );
        }

        loop {
            tokio::select! {
                _ = self.shutdown_notify.notified() => {
                    info!("Shutdown signal received");
                    transfer_handle.abort();
                    break;
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                    // Check if we need to refresh presence
                    // (This now runs every second to allow more frequent monitoring)

                    // Monitoring checks
                    if monitor_enabled && last_monitor_check.elapsed().as_secs() >= monitor_interval {
                        last_monitor_check = std::time::Instant::now();

                        // 1. Session tampering detection
                        match session_client.check_tampering_via_handle(xbl_token, session, handle_id).await {
                            Ok(result) => {
                                if result.tampered {
                                    tamper_count += 1;
                                    error!(
                                        "🚨 SESSION TAMPERING DETECTED! (occurrence #{})",
                                        tamper_count
                                    );

                                    // Identify potential attacker XUIDs from unauthorized members
                                    let mut attacker_xuids = Vec::new();
                                    for field in &result.modified_fields {
                                        error!(
                                            "  Field '{}': expected '{}', got '{}'",
                                            field.field, field.expected, field.actual
                                        );

                                        // If an unknown member joined, they might be the attacker
                                        if field.field == "members" && field.actual.contains("unknown member joined:") {
                                            if let Some(xuid) = field.actual.strip_prefix("unknown member joined: ") {
                                                attacker_xuids.push(xuid.to_string());
                                            }
                                        }
                                    }

                                    // Handle attacker identification and blocking
                                    for attacker_xuid in &attacker_xuids {
                                        if blocked_xuids.contains(attacker_xuid) {
                                            continue; // Already blocked
                                        }

                                        // Get attacker's gamertag for logging
                                        let gamertag = friends_client
                                            .get_gamertag(xbl_token, attacker_xuid)
                                            .await
                                            .unwrap_or_else(|_| "Unknown".to_string());

                                        error!(
                                            "⚠️  POTENTIAL ATTACKER: {} (XUID: {})",
                                            gamertag, attacker_xuid
                                        );

                                        if auto_block {
                                            info!("Auto-blocking attacker {}...", gamertag);
                                            match friends_client.force_remove_follower(xbl_token, attacker_xuid).await {
                                                Ok(_) => {
                                                    info!("✅ Blocked attacker: {} ({})", gamertag, attacker_xuid);
                                                    blocked_xuids.insert(attacker_xuid.clone());
                                                }
                                                Err(e) => {
                                                    warn!("Failed to block attacker {}: {}", attacker_xuid, e);
                                                }
                                            }
                                        }
                                    }

                                    // Auto-repair by re-pushing our session data
                                    info!("Attempting auto-repair...");
                                    match session_client.repair_session(xbl_token, session).await {
                                        Ok(_) => info!("✅ Session repaired successfully"),
                                        Err(e) => error!("❌ Failed to repair session: {}", e),
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
                            handle_id,
                            &session.session_id,
                        ).await {
                            Ok(result) => {
                                if result.hijack_detected {
                                    hijack_count += 1;
                                    error!(
                                        "🚨 HANDLE HIJACKING DETECTED! (occurrence #{})",
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

                                    // If our handle is gone, recreate it
                                    if !result.our_handle_exists {
                                        warn!("Our handle was deleted! Recreating...");
                                        match session_client.create_handle(xbl_token, session).await {
                                            Ok(new_handle) => {
                                                info!("✅ Handle recreated: {}", new_handle);
                                                // Note: handle_id is borrowed, can't update it
                                                // In production, you'd want to update the stored handle_id
                                            }
                                            Err(e) => {
                                                error!("❌ Failed to recreate handle: {}", e);
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
                _ = tokio::time::sleep(std::time::Duration::from_secs(heartbeat_secs)) => {
                    // Refresh presence periodically
                    match presence.set_active(xbl_token).await {
                        Ok(new_heartbeat) => {
                            heartbeat_secs = new_heartbeat;
                            debug!(heartbeat = heartbeat_secs, "Presence refreshed");
                        }
                        Err(e) => {
                            warn!("Failed to refresh presence: {}", e);
                        }
                    }
                }
                res = &mut transfer_handle => {
                    match res {
                        Ok(_) => warn!("Transfer server exited unexpectedly"),
                        Err(e) => error!("Transfer server panicked: {}", e),
                    }
                    break;
                }
            }
        }

        // Summary at shutdown
        if monitor_enabled {
            if tamper_count > 0 || hijack_count > 0 {
                warn!(
                    "Security summary: {} tampering event(s), {} hijacking event(s), {} attacker(s) blocked",
                    tamper_count, hijack_count, blocked_xuids.len()
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
        self.shutdown_notify.notify_waiters();
    }
}
