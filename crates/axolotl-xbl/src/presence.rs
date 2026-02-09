//! Xbox Live Presence API.
//!
//! Update user presence to show as online and playing Minecraft.

use crate::auth::{XblToken, sign_request_for_url, update_server_time_from_header};
use crate::constants::endpoints;
use crate::error::{XblError, XblResult};
use serde::Serialize;
use tracing::{debug, info};

/// Presence state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceState {
    Active,
    Inactive,
}

impl PresenceState {
    fn as_str(&self) -> &'static str {
        match self {
            PresenceState::Active => "active",
            PresenceState::Inactive => "inactive",
        }
    }
}

/// Presence update request.
#[derive(Debug, Serialize)]
pub struct PresenceUpdate {
    state: String,
}

/// Xbox Live Presence client.
pub struct PresenceClient {
    client: reqwest::Client,
}

impl PresenceClient {
    /// Create a new presence client.
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    fn signature(method: &str, url: &str, authorization: &str, body: &[u8]) -> XblResult<String> {
        sign_request_for_url(method, url, authorization, body)
            .map_err(|e| XblError::Auth(format!("Failed to sign Xbox request: {}", e)))
    }

    fn sync_server_time(response: &reqwest::Response) {
        if let Some(date) = response.headers().get("Date").and_then(|v| v.to_str().ok()) {
            update_server_time_from_header(date);
        }
    }

    /// Update user presence.
    ///
    /// Returns the recommended heartbeat interval in seconds.
    pub async fn update(&self, token: &XblToken, state: PresenceState) -> XblResult<u64> {
        let url = endpoints::USER_PRESENCE_FMT.replace("{}", &token.xuid);
        debug!(state = ?state, "Updating presence");
        let response = match state {
            PresenceState::Active => {
                let body = PresenceUpdate {
                    state: state.as_str().into(),
                };
                let body_bytes = serde_json::to_vec(&body)?;
                let authorization = token.auth_header();
                let signature = Self::signature("POST", &url, &authorization, &body_bytes)?;

                self.client
                    .post(&url)
                    .header("Authorization", authorization)
                    .header("Signature", signature)
                    .header("Content-Type", "application/json")
                    .header("x-xbl-contract-version", "3")
                    .body(body_bytes)
                    .send()
                    .await?
            }
            // Presence API supports DELETE for inactivation.
            PresenceState::Inactive => {
                let authorization = token.auth_header();
                let signature = Self::signature("DELETE", &url, &authorization, b"")?;

                self.client
                    .delete(&url)
                    .header("Authorization", authorization)
                    .header("Signature", signature)
                    .header("x-xbl-contract-version", "3")
                    .send()
                    .await?
            }
        };
        Self::sync_server_time(&response);

        if !response.status().is_success() {
            return Err(XblError::XboxLive(format!(
                "Presence update failed: {}",
                response.status()
            )));
        }

        // Get heartbeat interval from header (default 300 seconds)
        let heartbeat = response
            .headers()
            .get("X-Heartbeat-After")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(300);

        info!(heartbeat = heartbeat, "Presence updated");
        Ok(heartbeat)
    }

    /// Set presence to active.
    pub async fn set_active(&self, token: &XblToken) -> XblResult<u64> {
        self.update(token, PresenceState::Active).await
    }

    /// Set presence to inactive.
    pub async fn set_inactive(&self, token: &XblToken) -> XblResult<u64> {
        self.update(token, PresenceState::Inactive).await
    }
}

impl Default for PresenceClient {
    fn default() -> Self {
        Self::new()
    }
}
