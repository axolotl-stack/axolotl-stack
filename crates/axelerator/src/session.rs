//! Xbox Live session management for Axelerator.
//!
//! Axelerator always uses WebRTC (NetherNet) to advertise sessions to Xbox Live friends,
//! then transfers players to the actual RakNet server.

use crate::config::AxeleratorConfig;
use crate::token_cache::TokenCache;
use anyhow::{Context, Result};
use axolotl_xbl::{ExpandedSessionInfo, PlayFabClient, PresenceClient, SessionClient, SessionInfo};
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

        // Store session info
        {
            let mut info = self.session_info.write().await;
            *info = Some(session_info.clone());
        }

        // Step 4: Run WebRTC signaling and transfer players
        let playfab_token = token_cache
            .get_xbl_token(axolotl_xbl::auth::relying_party::PLAYFAB)
            .await?;

        self.run_signaling_loop(xbl_token, &playfab_token, &session_info, heartbeat)
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
        mut heartbeat_secs: u64,
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

        loop {
            tokio::select! {
                _ = self.shutdown_notify.notified() => {
                    info!("Shutdown signal received");
                    transfer_handle.abort();
                    break;
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

        Ok(())
    }

    /// Request shutdown.
    pub async fn shutdown(&self) {
        info!("Initiating graceful shutdown...");
        self.shutdown_notify.notify_waiters();
    }
}
