//! Xbox Live Session API.
//!
//! Create and manage Xbox Live sessions for Minecraft multiplayer.

use crate::auth::XblToken;
use crate::constants::{SERVICE_CONFIG_ID, TEMPLATE_NAME, TITLE_ID, endpoints};
use crate::error::{XblError, XblResult};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};
use uuid::Uuid;

/// WebRTC connection type for Xbox Live sessions.
/// This matches `tokio_nethernet::ConnectionType::WebRTC`.
const CONNECTION_TYPE_WEBRTC: i32 = 3;

/// Session information for broadcasting.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// Server name shown in friends list.
    pub host_name: String,
    /// World name.
    pub world_name: String,
    /// Minecraft version string.
    pub version: String,
    /// Protocol version.
    pub protocol: i32,
    /// Current player count.
    pub players: i32,
    /// Maximum players.
    pub max_players: i32,
    /// Server IP address.
    pub ip: String,
    /// Server port.
    pub port: u16,
}

impl Default for SessionInfo {
    fn default() -> Self {
        Self {
            host_name: "Axolotl Server".into(),
            world_name: "Minecraft World".into(),
            version: "1.21.50".into(),
            protocol: 786,
            players: 1,
            max_players: 10,
            ip: "127.0.0.1".into(),
            port: 19132,
        }
    }
}

/// Expanded session info with Xbox Live identifiers.
///
/// Sessions always use WebRTC (NetherNet) for Xbox Live friend visibility.
#[derive(Debug, Clone)]
pub struct ExpandedSessionInfo {
    /// Unique session ID.
    pub session_id: String,
    /// RTA connection ID.
    pub connection_id: String,
    /// Device ID.
    pub device_id: String,
    /// NetherNet ID for signaling.
    pub nethernet_id: u64,
    /// Handle ID after creation.
    pub handle_id: Option<String>,
    /// User XUID.
    pub xuid: String,
    /// Session information (includes target server IP/port for transfer).
    pub info: SessionInfo,
}

impl ExpandedSessionInfo {
    /// Create new expanded session info (always WebRTC mode).
    pub fn new(xuid: String, info: SessionInfo) -> Self {
        Self {
            session_id: Uuid::new_v4().to_string(),
            connection_id: Uuid::new_v4().to_string(),
            device_id: Uuid::new_v4().to_string(),
            nethernet_id: rand::random(),
            handle_id: None,
            xuid,
            info,
        }
    }
}

/// Session reference for handles.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRef {
    scid: String,
    template_name: String,
    name: String,
}

/// Create handle request.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateHandleRequest {
    version: i32,
    #[serde(rename = "type")]
    handle_type: String,
    session_ref: SessionRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    invited_xuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<serde_json::Value>,
}

/// Create handle response.
#[derive(Debug, Deserialize)]
pub struct CreateHandleResponse {
    pub id: String,
}

/// Connection properties in session.
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ConnectionProperties {
    pub system: ConnectionSystem,
    pub custom: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ConnectionSystem {
    pub active: bool,
    pub connection: String,
    pub subscription: ConnectionSubscription,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSubscription {
    pub id: String,
    pub change_types: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Connection {
    pub connection_type: i32,
    pub host_ip_address: String,
    pub host_port: u16,
    pub nether_net_id: u64,
}

impl Connection {
    /// Create a WebRTC (NetherNet) connection for Xbox Live sessions.
    pub fn new(nether_net_id: u64) -> Self {
        Self {
            connection_type: CONNECTION_TYPE_WEBRTC,
            host_ip_address: String::new(),
            host_port: 0,
            nether_net_id,
        }
    }
}

/// Session properties.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionProperties {
    pub system: SessionSystemProperties,
    pub custom: SessionCustomProperties,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSystemProperties {
    pub join_restriction: String,
    pub read_restriction: String,
    pub closed: bool,
}

#[derive(Debug, Serialize)]
pub struct SessionCustomProperties {
    #[serde(rename = "BroadcastSetting")]
    pub broadcast_setting: i32,
    #[serde(rename = "CrossPlayDisabled")]
    pub cross_play_disabled: bool,
    #[serde(rename = "Joinability")]
    pub joinability: String,
    #[serde(rename = "LanGame")]
    pub lan_game: bool,
    #[serde(rename = "MaxMemberCount")]
    pub max_member_count: i32,
    #[serde(rename = "MemberCount")]
    pub member_count: i32,
    #[serde(rename = "OnlineCrossPlatformGame")]
    pub online_cross_platform_game: bool,
    #[serde(rename = "SupportedConnections")]
    pub supported_connections: Vec<Connection>,
    #[serde(rename = "TitleId")]
    pub title_id: i32,
    #[serde(rename = "TransportLayer")]
    pub transport_layer: i32,
    #[serde(rename = "levelId")]
    pub level_id: String,
    #[serde(rename = "hostName")]
    pub host_name: String,
    #[serde(rename = "ownerId")]
    pub owner_id: String,
    #[serde(rename = "rakNetGUID")]
    pub rak_net_guid: String,
    #[serde(rename = "worldName")]
    pub world_name: String,
    #[serde(rename = "worldType")]
    pub world_type: String,
    pub protocol: i32,
    pub version: String,
    #[serde(rename = "isEditorWorld")]
    pub is_editor_world: bool,
    #[serde(rename = "isHardcore")]
    pub is_hardcore: bool,
}

/// Create session request body.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionRequest {
    pub properties: SessionProperties,
    pub members: serde_json::Value,
}

impl CreateSessionRequest {
    /// Build session request from expanded session info.
    pub fn from_session(session: &ExpandedSessionInfo) -> Self {
        Self {
            properties: SessionProperties {
                system: SessionSystemProperties {
                    join_restriction: "followed".into(),
                    read_restriction: "followed".into(),
                    closed: false,
                },
                custom: SessionCustomProperties {
                    broadcast_setting: 3,
                    cross_play_disabled: false,
                    joinability: "joinable_by_friends".into(),
                    lan_game: false,
                    max_member_count: session.info.max_players,
                    member_count: session.info.players.max(1),
                    online_cross_platform_game: true,
                    supported_connections: vec![Connection::new(session.nethernet_id)],
                    title_id: 0,
                    transport_layer: 2, // WebRTC
                    level_id: "level".into(),
                    host_name: session.info.host_name.clone(),
                    owner_id: session.xuid.clone(),
                    rak_net_guid: String::new(),
                    world_name: session.info.world_name.clone(),
                    world_type: "Survival".into(),
                    protocol: session.info.protocol,
                    version: session.info.version.clone(),
                    is_editor_world: false,
                    is_hardcore: false,
                },
            },
            members: serde_json::json!({
                "me": {
                    "constants": {
                        "system": {
                            "xuid": session.xuid,
                            "initialize": true
                        }
                    },
                    "properties": {
                        "system": {
                            "active": true,
                            "connection": session.connection_id,
                            "subscription": {
                                "id": "845CC784-7348-4A27-BCDE-C083579DD113",
                                "changeTypes": ["everything"]
                            }
                        },
                        "custom": {}
                    }
                }
            }),
        }
    }
}

/// Xbox Live Session client.
pub struct SessionClient {
    client: reqwest::Client,
}

impl SessionClient {
    /// Create a new session client.
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Create or update a session.
    pub async fn create_session(
        &self,
        token: &XblToken,
        session: &ExpandedSessionInfo,
    ) -> XblResult<()> {
        let url = format!("{}{}", endpoints::CREATE_SESSION_FMT, session.session_id);
        let body = CreateSessionRequest::from_session(session);

        debug!(session_id = %session.session_id, "Creating session");

        let response = self
            .client
            .put(&url)
            .header("Authorization", token.auth_header())
            .header("Content-Type", "application/json")
            .header("x-xbl-contract-version", "107")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(XblError::XboxLive(format!(
                "Create session failed ({}): {}",
                status, body
            )));
        }

        info!(session_id = %session.session_id, "Session created");
        Ok(())
    }

    /// Create a session handle (makes session visible to friends).
    pub async fn create_handle(
        &self,
        token: &XblToken,
        session: &ExpandedSessionInfo,
    ) -> XblResult<String> {
        let body = CreateHandleRequest {
            version: 1,
            handle_type: "activity".into(),
            session_ref: SessionRef {
                scid: SERVICE_CONFIG_ID.into(),
                template_name: TEMPLATE_NAME.into(),
                name: session.session_id.clone(),
            },
            invited_xuid: None,
            context: None,
        };

        debug!("Creating session handle");

        let response = self
            .client
            .post(endpoints::CREATE_HANDLE)
            .header("Authorization", token.auth_header())
            .header("Content-Type", "application/json")
            .header("x-xbl-contract-version", "107")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(XblError::XboxLive(format!(
                "Create handle failed ({}): {}",
                status, body
            )));
        }

        let handle: CreateHandleResponse = response.json().await?;
        info!(handle_id = %handle.id, "Session handle created");
        Ok(handle.id)
    }

    /// Send game invite to a user.
    pub async fn send_invite(
        &self,
        token: &XblToken,
        session: &ExpandedSessionInfo,
        xuid: &str,
    ) -> XblResult<()> {
        let body = CreateHandleRequest {
            version: 1,
            handle_type: "invite".into(),
            session_ref: SessionRef {
                scid: SERVICE_CONFIG_ID.into(),
                template_name: TEMPLATE_NAME.into(),
                name: session.session_id.clone(),
            },
            invited_xuid: Some(xuid.into()),
            context: Some(serde_json::json!({ "titleId": TITLE_ID })),
        };

        let response = self
            .client
            .post(endpoints::CREATE_HANDLE)
            .header("Authorization", token.auth_header())
            .header("Content-Type", "application/json")
            .header("x-xbl-contract-version", "107")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(XblError::XboxLive(format!(
                "Send invite failed: {}",
                response.status()
            )));
        }

        info!(xuid = xuid, "Invite sent");
        Ok(())
    }
}

impl Default for SessionClient {
    fn default() -> Self {
        Self::new()
    }
}
