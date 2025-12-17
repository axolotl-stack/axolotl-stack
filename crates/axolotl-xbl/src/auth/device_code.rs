//! Device code authentication flow for Microsoft Live Connect.
//!
//! This implements the OAuth 2.0 device code flow, where users authenticate
//! by visiting a URL and entering a code on another device.

use crate::error::{XblError, XblResult};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, info};

use super::{LIVE_CONNECT_URL, LIVE_TOKEN_URL, MINECRAFT_CLIENT_ID, XBL_SCOPE};

/// Handles device code authentication flow.
pub struct DeviceCodeAuth {
    client: reqwest::Client,
}

/// Response from starting device code authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCodeResponse {
    /// The code the user must enter.
    pub user_code: String,
    /// Internal device code for polling.
    pub device_code: String,
    /// URL where user enters the code.
    pub verification_uri: String,
    /// Polling interval in seconds.
    pub interval: u64,
    /// Code expiration in seconds.
    pub expires_in: u64,
}

/// OAuth token response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    pub token_type: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub user_id: Option<String>,
}

/// Polling response (may contain error or token).
#[derive(Debug, Deserialize)]
struct PollResponse {
    error: Option<String>,
    error_description: Option<String>,
    access_token: Option<String>,
    token_type: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    user_id: Option<String>,
}

impl DeviceCodeAuth {
    /// Create a new device code authenticator.
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Start the device code flow.
    ///
    /// Returns a code the user must enter at the verification URL.
    pub async fn start(&self) -> XblResult<DeviceCodeResponse> {
        debug!("Starting device code authentication flow");

        let response = self
            .client
            .post(LIVE_CONNECT_URL)
            .form(&[
                ("client_id", MINECRAFT_CLIENT_ID),
                ("scope", XBL_SCOPE),
                ("response_type", "device_code"),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(XblError::Auth(format!(
                "Device code request failed: {}",
                response.status()
            )));
        }

        let device_code: DeviceCodeResponse = response.json().await?;
        info!(
            code = %device_code.user_code,
            url = %device_code.verification_uri,
            "Device code authentication started"
        );

        Ok(device_code)
    }

    /// Poll for authentication completion.
    ///
    /// Returns `Ok(Some(token))` when authenticated, `Ok(None)` if still pending.
    pub async fn poll(&self, device_code: &str) -> XblResult<Option<OAuthToken>> {
        let response = self
            .client
            .post(LIVE_TOKEN_URL)
            .form(&[
                ("client_id", MINECRAFT_CLIENT_ID),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", device_code),
            ])
            .send()
            .await?;

        let poll: PollResponse = response.json().await?;

        match poll.error.as_deref() {
            Some("authorization_pending") => Ok(None),
            Some("expired_token") => Err(XblError::DeviceCodeExpired),
            Some(error) => Err(XblError::Auth(format!(
                "{}: {}",
                error,
                poll.error_description.unwrap_or_default()
            ))),
            None => {
                // Authentication successful
                Ok(Some(OAuthToken {
                    access_token: poll.access_token.ok_or_else(|| {
                        XblError::Auth("Missing access_token in response".into())
                    })?,
                    token_type: poll.token_type.unwrap_or_else(|| "Bearer".into()),
                    refresh_token: poll.refresh_token.ok_or_else(|| {
                        XblError::Auth("Missing refresh_token in response".into())
                    })?,
                    expires_in: poll.expires_in.unwrap_or(3600),
                    user_id: poll.user_id,
                }))
            }
        }
    }

    /// Wait for user to complete authentication.
    ///
    /// Polls at the specified interval until authentication succeeds or expires.
    pub async fn wait_for_auth(&self, device_code: &DeviceCodeResponse) -> XblResult<OAuthToken> {
        let interval = Duration::from_secs(device_code.interval.max(1));
        let mut attempts = device_code.expires_in / device_code.interval.max(1);

        loop {
            tokio::time::sleep(interval).await;

            match self.poll(&device_code.device_code).await? {
                Some(token) => {
                    info!("Device code authentication successful");
                    return Ok(token);
                }
                None => {
                    debug!("Still waiting for user authentication...");
                    attempts = attempts.saturating_sub(1);
                    if attempts == 0 {
                        return Err(XblError::DeviceCodeExpired);
                    }
                }
            }
        }
    }

    /// Refresh an OAuth token.
    pub async fn refresh(&self, refresh_token: &str) -> XblResult<OAuthToken> {
        let response = self
            .client
            .post(LIVE_TOKEN_URL)
            .form(&[
                ("client_id", MINECRAFT_CLIENT_ID),
                ("scope", XBL_SCOPE),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(XblError::Auth(format!(
                "Token refresh failed: {}",
                response.status()
            )));
        }

        Ok(response.json().await?)
    }
}

impl Default for DeviceCodeAuth {
    fn default() -> Self {
        Self::new()
    }
}
