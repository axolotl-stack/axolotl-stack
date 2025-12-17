//! Xbox Live token types and acquisition.
//!
//! After OAuth authentication, we need to:
//! 1. Get a device token
//! 2. Exchange for an XBL token with gamertag/XUID

use crate::error::{XblError, XblResult};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};
use uuid::Uuid;

use super::device_code::OAuthToken;
use super::signing::SigningKeyPair;
use super::{DEVICE_AUTH_URL, SISU_AUTHORIZE_URL};

/// Xbox Live token with user information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XblToken {
    /// The authorization token string.
    pub token: String,
    /// User's Xbox gamertag.
    pub gamertag: String,
    /// User's Xbox User ID.
    pub xuid: String,
    /// User hash for authorization header.
    pub user_hash: String,
}

impl XblToken {
    /// Get the gamertag.
    pub fn gamertag(&self) -> &str {
        &self.gamertag
    }

    /// Get the XUID.
    pub fn xuid(&self) -> &str {
        &self.xuid
    }

    /// Format the authorization header value.
    pub fn auth_header(&self) -> String {
        format!("XBL3.0 x={};{}", self.user_hash, self.token)
    }
}

/// Internal device token response.
#[derive(Debug, Deserialize)]
struct DeviceTokenResponse {
    #[serde(rename = "Token")]
    token: String,
}

/// Internal XBL authorization response.
#[derive(Debug, Deserialize)]
struct XblAuthResponse {
    #[serde(rename = "AuthorizationToken")]
    authorization_token: AuthorizationToken,
}

#[derive(Debug, Deserialize)]
struct AuthorizationToken {
    #[serde(rename = "DisplayClaims")]
    display_claims: DisplayClaims,
    #[serde(rename = "Token")]
    token: String,
}

#[derive(Debug, Deserialize)]
struct DisplayClaims {
    xui: Vec<UserInfo>,
}

#[derive(Debug, Deserialize)]
struct UserInfo {
    /// Gamertag
    gtg: Option<String>,
    /// XUID
    xid: Option<String>,
    /// User hash
    uhs: Option<String>,
}

/// Acquires Xbox Live tokens from OAuth tokens.
pub struct XblTokenClient {
    client: reqwest::Client,
    signing_key: SigningKeyPair,
    /// Cached device token for reuse across multiple RP requests.
    cached_device_token: tokio::sync::OnceCell<String>,
}

impl XblTokenClient {
    /// Create a new XBL token client.
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            signing_key: SigningKeyPair::generate(),
            cached_device_token: tokio::sync::OnceCell::new(),
        }
    }

    /// Get an XBL token from an OAuth token for a specific relying party.
    ///
    /// This performs the full flow:
    /// 1. Request device token (or reuse cached one)
    /// 2. Request XBL token with SISU
    pub async fn get_xbl_token(
        &self,
        oauth_token: &OAuthToken,
        relying_party: Option<&str>,
    ) -> XblResult<XblToken> {
        let device_token = self.get_or_create_device_token().await?;

        debug!(rp = ?relying_party, "Requesting XBL token");
        let xbl_token = self
            .get_xbl_token_internal(oauth_token, &device_token, relying_party)
            .await?;

        info!(gamertag = %xbl_token.gamertag, xuid = %xbl_token.xuid, "XBL token acquired");
        Ok(xbl_token)
    }

    /// Get or create device token (cached for reuse).
    async fn get_or_create_device_token(&self) -> XblResult<String> {
        self.cached_device_token
            .get_or_try_init(|| async {
                debug!("Requesting device token");
                self.get_device_token().await
            })
            .await
            .cloned()
    }

    /// Request a device token.
    async fn get_device_token(&self) -> XblResult<String> {
        // Matches gophertunnel/Broadcaster key order (sorted)
        let body = serde_json::json!({
            "Properties": {
                "AuthMethod": "ProofOfPossession",
                "DeviceType": "Android",
                "Id": format!("{{{}}}", Uuid::new_v4()),
                "ProofKey": self.signing_key.proof_key(),
                "Version": "10",
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        });

        let body_bytes = serde_json::to_vec(&body)?;

        debug!(
            "Device Token Body: {}",
            String::from_utf8_lossy(&body_bytes)
        );

        let signature =
            self.signing_key
                .sign_request("POST", "/device/authenticate", "", &body_bytes);

        debug!("Device Token Signature: {}", signature);

        let response = self
            .client
            .post(DEVICE_AUTH_URL)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .header("User-Agent", "axolotl-client/1.0")
            .header("x-xbl-contract-version", "1")
            .header("Signature", signature)
            .body(body_bytes)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(XblError::Auth(format!(
                "Device token request failed ({}): {}",
                status, body
            )));
        }

        let device_response: DeviceTokenResponse = response.json().await?;
        Ok(device_response.token)
    }

    /// Request an XBL token using SISU.
    async fn get_xbl_token_internal(
        &self,
        oauth_token: &OAuthToken,
        device_token: &str,
        relying_party: Option<&str>,
    ) -> XblResult<XblToken> {
        let app_id = super::MINECRAFT_CLIENT_ID;

        // Base body
        let mut body = serde_json::json!({
            "AccessToken": format!("t={}", oauth_token.access_token),
            "AppId": app_id,
            "DeviceToken": device_token,
            "Sandbox": "RETAIL",
            "UseModernGamertag": true,
            "SiteName": "user.auth.xboxlive.com",
            "ProofKey": self.signing_key.proof_key(),
        });

        // Add RelyingParty if specified
        if let Some(rp) = relying_party {
            body.as_object_mut().unwrap().insert(
                "RelyingParty".to_string(),
                serde_json::Value::String(rp.to_string()),
            );
        }

        let body_bytes = serde_json::to_vec(&body)?;
        let signature = self
            .signing_key
            .sign_request("POST", "/authorize", "", &body_bytes);

        let response = self
            .client
            .post(SISU_AUTHORIZE_URL)
            .header("Content-Type", "application/json")
            .header("x-xbl-contract-version", "1")
            .header("Signature", signature)
            .body(body_bytes)
            .send()
            .await?;

        let status = response.status();

        // Check for Xbox-specific error codes
        if let Some(error_code) = response.headers().get("x-err") {
            if let Ok(code) = error_code.to_str() {
                return Err(XblError::from_xbox_error_code(code));
            }
        }

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(XblError::Auth(format!(
                "XBL token request failed ({}): {}",
                status, body
            )));
        }

        let response_text = response.text().await?;
        tracing::trace!(response_body = %response_text, "Raw SISU response");

        let xbl_response: XblAuthResponse = serde_json::from_str(&response_text).map_err(|e| {
            XblError::Auth(format!(
                "Failed to parse XBL response: {} - Body: {}",
                e, response_text
            ))
        })?;

        tracing::debug!(
            token_len = xbl_response.authorization_token.token.len(),
            xui_count = xbl_response.authorization_token.display_claims.xui.len(),
            "Parsed XBL auth response"
        );

        let user = xbl_response
            .authorization_token
            .display_claims
            .xui
            .into_iter()
            .next()
            .ok_or_else(|| XblError::Auth("No user info in XBL response".into()))?;

        tracing::debug!(
            gtg = ?user.gtg,
            xid = ?user.xid,
            uhs = ?user.uhs,
            "User display claims"
        );

        Ok(XblToken {
            token: xbl_response.authorization_token.token,
            gamertag: user.gtg.unwrap_or_default(),
            xuid: user.xid.unwrap_or_default(),
            user_hash: user.uhs.unwrap_or_default(),
        })
    }
}

impl Default for XblTokenClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Common relying party values.
pub mod relying_party {
    /// For Xbox Live (MPSD, Profile, etc).
    pub const XBOX_LIVE: &str = "http://xboxlive.com";
    /// For Java Edition services (Mojang API).
    pub const JAVA_EDITION: &str = "rp://api.minecraftservices.com/";
    /// For Bedrock Edition multiplayer (NetherNet).
    pub const BEDROCK_MULTIPLAYER: &str = "https://multiplayer.minecraft.net/";
    /// For Pocket Realms.
    pub const POCKET_REALMS: &str = "https://pocket.realms.minecraft.net/";
    /// For PlayFab (Minecraft Bedrock).
    /// Uses go-playfab's relying party: http://playfab.xboxlive.com/
    pub const PLAYFAB: &str = "http://playfab.xboxlive.com/";
}
