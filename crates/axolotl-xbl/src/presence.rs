//! Xbox Live Presence API.
//!
//! Update user presence to show as online and playing Minecraft.

use crate::auth::XblToken;
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

    /// Update user presence.
    ///
    /// Returns the recommended heartbeat interval in seconds.
    pub async fn update(&self, token: &XblToken, state: PresenceState) -> XblResult<u64> {
        let url = endpoints::USER_PRESENCE_FMT.replace("{}", &token.xuid);
        let body = PresenceUpdate {
            state: state.as_str().into(),
        };

        debug!(state = ?state, "Updating presence");

        let response = self
            .client
            .post(&url)
            .header("Authorization", token.auth_header())
            .header("Content-Type", "application/json")
            .header("x-xbl-contract-version", "3")
            .json(&body)
            .send()
            .await?;

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
