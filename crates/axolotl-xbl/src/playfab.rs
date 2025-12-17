//! PlayFab authentication for Minecraft services.
//!
//! Used to obtain the Minecraft token for signaling WebSocket.

use crate::auth::XblToken;
use crate::constants::endpoints;
use crate::error::{XblError, XblResult};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

/// PlayFab login request.
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct PlayFabLoginRequest {
    create_account: bool,
    encrypted_request: Option<String>,
    info_request_parameters: InfoRequestParameters,
    player_secret: Option<String>,
    title_id: String,
    xbox_token: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct InfoRequestParameters {
    get_character_inventories: bool,
    get_character_list: bool,
    get_player_profile: bool,
    get_player_statistics: bool,
    get_title_data: bool,
    get_user_account_info: bool,
    get_user_data: bool,
    get_user_inventory: bool,
    get_user_read_only_data: bool,
    get_user_virtual_currency: bool,
    player_statistic_names: Option<Vec<String>>,
    profile_constraints: Option<String>,
    title_data_keys: Option<Vec<String>>,
    user_data_keys: Option<Vec<String>>,
    user_read_only_data_keys: Option<Vec<String>>,
}

/// PlayFab login response.
/// Note: Response uses lowercase keys like "code", "status", "data"
#[derive(Debug, Deserialize)]
pub struct PlayFabLoginResponse {
    pub data: Option<PlayFabLoginData>,
    pub code: Option<i32>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PlayFabLoginData {
    pub session_ticket: String,
    pub entity_token: EntityToken,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct EntityToken {
    pub entity_token: String,
}

/// Minecraft session start request.
#[derive(Debug, Serialize)]
pub struct SessionStartRequest {
    device: SessionDevice,
    user: SessionUser,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDevice {
    application_type: String,
    capabilities: Vec<String>,
    game_version: String,
    id: String,
    memory: String,
    platform: String,
    play_fab_title_id: String,
    store_platform: String,
    treatment_overrides: Option<Vec<String>>,
    #[serde(rename = "type")]
    device_type: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUser {
    language: String,
    language_code: String,
    region_code: String,
    token: String,
    token_type: String,
}

/// Minecraft session start response.
#[derive(Debug, Deserialize)]
pub struct SessionStartResponse {
    pub result: SessionResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResult {
    pub authorization_header: String,
}

/// PlayFab/Minecraft auth client.
pub struct PlayFabClient {
    client: reqwest::Client,
}

impl PlayFabClient {
    /// Create a new PlayFab client.
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Login to PlayFab with Xbox token info.
    ///
    /// * `user_hash`: The User Hash (uhs) from the PlayFab-scoped Xbox Live token.
    /// * `xbl_token`: The token string from the PlayFab-scoped Xbox Live token.
    pub async fn login(&self, user_hash: &str, xbl_token: &str) -> XblResult<String> {
        let xbox_token_str = format!("XBL3.0 x={};{}", user_hash, xbl_token);

        tracing::debug!(
            user_hash_len = user_hash.len(),
            token_len = xbl_token.len(),
            xbox_token_prefix = %xbox_token_str.chars().take(50).collect::<String>(),
            "Preparing PlayFab login request"
        );

        let body = PlayFabLoginRequest {
            create_account: true,
            encrypted_request: None,
            info_request_parameters: InfoRequestParameters {
                get_character_inventories: false,
                get_character_list: false,
                get_player_profile: true,
                get_player_statistics: false,
                get_title_data: false,
                get_user_account_info: true,
                get_user_data: false,
                get_user_inventory: false,
                get_user_read_only_data: false,
                get_user_virtual_currency: false,
                player_statistic_names: None,
                profile_constraints: None,
                title_data_keys: None,
                user_data_keys: None,
                user_read_only_data_keys: None,
            },
            player_secret: None,
            title_id: "20CA2".into(),
            xbox_token: xbox_token_str,
        };

        debug!("Logging into PlayFab");

        let response = self
            .client
            .post(endpoints::PLAYFAB_LOGIN)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            tracing::error!(status = %status, body = %body_text, "PlayFab login failed");
            return Err(XblError::Auth(format!(
                "PlayFab login failed: {} - {}",
                status, body_text
            )));
        }

        let response_text = response.text().await?;
        tracing::debug!(response = %response_text, "PlayFab response");

        let login: PlayFabLoginResponse = serde_json::from_str(&response_text).map_err(|e| {
            XblError::Auth(format!(
                "Failed to parse PlayFab response: {} - {}",
                e, response_text
            ))
        })?;
        let data = login.data.ok_or_else(|| {
            XblError::Auth(format!("No data in PlayFab response: {}", response_text))
        })?;

        info!("PlayFab login successful");
        Ok(data.session_ticket)
    }

    /// Start Minecraft session to get authorization token.
    pub async fn start_session(&self, device_id: &str, playfab_ticket: &str) -> XblResult<String> {
        let body = SessionStartRequest {
            device: SessionDevice {
                application_type: "MinecraftPE".into(),
                capabilities: vec![],
                game_version: "1.21.20".into(),
                id: device_id.into(),
                memory: "8589934592".into(),
                platform: "Windows10".into(),
                play_fab_title_id: "20CA2".into(),
                store_platform: "uwp.store".into(),
                treatment_overrides: None,
                device_type: "Windows10".into(),
            },
            user: SessionUser {
                language: "en".into(),
                language_code: "en-US".into(),
                region_code: "US".into(),
                token: playfab_ticket.into(),
                token_type: "PlayFab".into(),
            },
        };

        debug!("Starting Minecraft session");

        let response = self
            .client
            .post(endpoints::MC_SESSION_START)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(XblError::Auth(format!(
                "Minecraft session start failed ({}): {}",
                status, body
            )));
        }

        let session: SessionStartResponse = response.json().await?;
        info!("Minecraft session started");
        Ok(session.result.authorization_header)
    }
}

impl Default for PlayFabClient {
    fn default() -> Self {
        Self::new()
    }
}
