//! Xbox Live Session API.
//!
//! Create and manage Xbox Live sessions for Minecraft multiplayer.
//!
//! ## Session Monitoring
//!
//! Use [`SessionClient::get_session`] and [`SessionClient::get_session_via_handle`]
//! to monitor session state and detect tampering attacks.

use crate::auth::{XblToken, sign_request_for_url, update_server_time_from_header};
use crate::constants::{SERVICE_CONFIG_ID, TEMPLATE_NAME, TITLE_ID, endpoints};
use crate::error::{XblError, XblResult};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
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
    /// Used for invite handles - contains titleId
    #[serde(skip_serializing_if = "Option::is_none")]
    invite_attributes: Option<std::collections::HashMap<String, String>>,
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

// ============================================================================
// Session Monitoring Types
// ============================================================================

/// Snapshot of session state for tampering detection.
///
/// This captures the critical fields that an attacker might modify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    /// Version string (e.g., "1.21.50")
    pub version: String,
    /// Protocol version (e.g., 786)
    pub protocol: i32,
    /// NetherNet ID for WebRTC signaling
    pub nethernet_id: u64,
    /// Host name shown in friends list
    pub host_name: String,
    /// World name
    pub world_name: String,
    /// Session members (XUIDs)
    pub members: Vec<String>,
    /// Whether session is closed
    pub closed: bool,
}

/// Result of tampering detection.
#[derive(Debug, Clone)]
pub struct TamperResult {
    /// Whether tampering was detected
    pub tampered: bool,
    /// List of fields that were modified
    pub modified_fields: Vec<TamperField>,
}

/// A field that was tampered with.
#[derive(Debug, Clone)]
pub struct TamperField {
    /// Field name
    pub field: String,
    /// Expected value
    pub expected: String,
    /// Actual value found
    pub actual: String,
}

impl SessionSnapshot {
    /// Compare this snapshot to another and detect differences.
    ///
    /// Note: Member changes are NOT flagged as tampering because:
    /// - Xbox Live automatically adds members when friends join via WebRTC
    /// - This is expected behavior for public sessions
    /// - Only critical fields (nethernet_id, protocol, etc.) indicate actual tampering
    pub fn compare(&self, other: &SessionSnapshot) -> TamperResult {
        let mut modified = Vec::new();

        if self.version != other.version {
            modified.push(TamperField {
                field: "version".into(),
                expected: self.version.clone(),
                actual: other.version.clone(),
            });
        }

        if self.protocol != other.protocol {
            modified.push(TamperField {
                field: "protocol".into(),
                expected: self.protocol.to_string(),
                actual: other.protocol.to_string(),
            });
        }

        if self.nethernet_id != other.nethernet_id {
            modified.push(TamperField {
                field: "nethernet_id".into(),
                expected: self.nethernet_id.to_string(),
                actual: other.nethernet_id.to_string(),
            });
        }

        if self.host_name != other.host_name {
            modified.push(TamperField {
                field: "host_name".into(),
                expected: self.host_name.clone(),
                actual: other.host_name.clone(),
            });
        }

        if self.closed != other.closed {
            modified.push(TamperField {
                field: "closed".into(),
                expected: self.closed.to_string(),
                actual: other.closed.to_string(),
            });
        }

        // Note: We intentionally do NOT flag new session members as tampering.
        // When friends click "Join Game" and connect via WebRTC, Xbox Live
        // automatically adds them to the session. This is normal behavior.
        // Only actual session property modifications are considered tampering.

        TamperResult {
            tampered: !modified.is_empty(),
            modified_fields: modified,
        }
    }

    /// Create expected snapshot from our session info.
    pub fn from_expected(session: &ExpandedSessionInfo) -> Self {
        Self {
            version: session.info.version.clone(),
            protocol: session.info.protocol,
            nethernet_id: session.nethernet_id,
            host_name: session.info.host_name.clone(),
            world_name: session.info.world_name.clone(),
            members: vec![session.xuid.clone()],
            closed: false,
        }
    }
}

/// Raw session response from Xbox API (for parsing).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResponse {
    pub properties: Option<SessionResponseProperties>,
    pub members: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct SessionResponseProperties {
    pub system: Option<SessionResponseSystem>,
    pub custom: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct SessionResponseSystem {
    pub closed: Option<bool>,
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

    fn signature(method: &str, url: &str, authorization: &str, body: &[u8]) -> XblResult<String> {
        sign_request_for_url(method, url, authorization, body)
            .map_err(|e| XblError::Auth(format!("Failed to sign Xbox request: {}", e)))
    }

    fn sync_server_time(response: &reqwest::Response) {
        if let Some(date) = response.headers().get("Date").and_then(|v| v.to_str().ok()) {
            update_server_time_from_header(date);
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
        let body_bytes = serde_json::to_vec(&body)?;
        let authorization = token.auth_header();
        let signature = Self::signature("PUT", &url, &authorization, &body_bytes)?;

        debug!(session_id = %session.session_id, "Creating session");

        let response = self
            .client
            .put(&url)
            .header("Authorization", authorization)
            .header("Signature", signature)
            .header("Content-Type", "application/json")
            .header("x-xbl-contract-version", "107")
            .body(body_bytes)
            .send()
            .await?;
        Self::sync_server_time(&response);

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
            invite_attributes: None,
        };
        let body_bytes = serde_json::to_vec(&body)?;
        let authorization = token.auth_header();
        let signature = Self::signature(
            "POST",
            endpoints::CREATE_HANDLE,
            &authorization,
            &body_bytes,
        )?;

        debug!("Creating session handle");

        let response = self
            .client
            .post(endpoints::CREATE_HANDLE)
            .header("Authorization", authorization)
            .header("Signature", signature)
            .header("Content-Type", "application/json")
            .header("x-xbl-contract-version", "107")
            .body(body_bytes)
            .send()
            .await?;
        Self::sync_server_time(&response);

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
        let mut invite_attributes = std::collections::HashMap::new();
        invite_attributes.insert("titleId".to_string(), TITLE_ID.to_string());

        let body = CreateHandleRequest {
            version: 1,
            handle_type: "invite".into(),
            session_ref: SessionRef {
                scid: SERVICE_CONFIG_ID.into(),
                template_name: TEMPLATE_NAME.into(),
                name: session.session_id.clone(),
            },
            invited_xuid: Some(xuid.into()),
            invite_attributes: Some(invite_attributes),
        };
        let body_bytes = serde_json::to_vec(&body)?;
        let authorization = token.auth_header();
        let signature = Self::signature(
            "POST",
            endpoints::CREATE_HANDLE,
            &authorization,
            &body_bytes,
        )?;

        let response = self
            .client
            .post(endpoints::CREATE_HANDLE)
            .header("Authorization", authorization)
            .header("Signature", signature)
            .header("Content-Type", "application/json")
            .header("x-xbl-contract-version", "107")
            .body(body_bytes)
            .send()
            .await?;
        Self::sync_server_time(&response);

        if !response.status().is_success() {
            return Err(XblError::XboxLive(format!(
                "Send invite failed: {}",
                response.status()
            )));
        }

        info!(xuid = xuid, "Invite sent");
        Ok(())
    }

    // ========================================================================
    // Session Monitoring
    // ========================================================================

    /// Get current session state directly from Xbox.
    ///
    /// This fetches the session using its session ID, which is what we created.
    pub async fn get_session(
        &self,
        token: &XblToken,
        session_id: &str,
    ) -> XblResult<serde_json::Value> {
        let url = format!("{}{}", endpoints::CREATE_SESSION_FMT, session_id);
        let authorization = token.auth_header();
        let signature = Self::signature("GET", &url, &authorization, b"")?;

        let response = self
            .client
            .get(&url)
            .header("Authorization", authorization)
            .header("Signature", signature)
            .header("x-xbl-contract-version", "107")
            .send()
            .await?;
        Self::sync_server_time(&response);

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(XblError::XboxLive(format!(
                "Get session failed ({}): {}",
                status, body
            )));
        }

        let json: serde_json::Value = response.json().await?;
        Ok(json)
    }

    /// Get session state via handle (what clients see when joining).
    ///
    /// This is the endpoint clients use when clicking "Join Game".
    /// If an attacker tampered with the session, this is where we'd see it.
    pub async fn get_session_via_handle(
        &self,
        token: &XblToken,
        handle_id: &str,
    ) -> XblResult<serde_json::Value> {
        let url = endpoints::JOIN_SESSION_FMT.replace("{}", handle_id);
        let authorization = token.auth_header();
        let signature = Self::signature("GET", &url, &authorization, b"")?;

        let response = self
            .client
            .get(&url)
            .header("Authorization", authorization)
            .header("Signature", signature)
            .header("x-xbl-contract-version", "107")
            .send()
            .await?;
        Self::sync_server_time(&response);

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(XblError::XboxLive(format!(
                "Get session via handle failed ({}): {}",
                status, body
            )));
        }

        let json: serde_json::Value = response.json().await?;
        Ok(json)
    }

    /// Parse session JSON into a snapshot for comparison.
    pub fn parse_session_snapshot(json: &serde_json::Value) -> Option<SessionSnapshot> {
        let props = json.get("properties")?.get("custom")?;

        let version = props.get("version")?.as_str()?.to_string();
        let protocol = props.get("protocol")?.as_i64()? as i32;
        let host_name = props.get("hostName")?.as_str()?.to_string();
        let world_name = props.get("worldName")?.as_str()?.to_string();

        // Debug: Log the full SupportedConnections array to detect injection attacks
        if let Some(connections) = props.get("SupportedConnections")
            && let Some(arr) = connections.as_array()
        {
            if arr.len() > 1 {
                warn!(
                    "🚨 MULTIPLE SupportedConnections detected ({} entries)! Possible injection attack:",
                    arr.len()
                );
                for (i, conn) in arr.iter().enumerate() {
                    warn!("  Connection[{}]: {}", i, conn);
                }
            } else {
                debug!("SupportedConnections: {}", connections);
            }
        }

        // Extract nethernet_id from SupportedConnections
        let nethernet_id = props
            .get("SupportedConnections")?
            .as_array()?
            .first()?
            .get("NetherNetId")
            .or_else(|| {
                props
                    .get("SupportedConnections")?
                    .as_array()?
                    .first()?
                    .get("netherNetId")
            })?
            .as_u64()?;

        // Extract members
        let mut members = Vec::new();
        if let Some(members_obj) = json.get("members").and_then(|m| m.as_object()) {
            for (_, member) in members_obj {
                if let Some(xuid) = member
                    .get("constants")
                    .and_then(|c| c.get("system"))
                    .and_then(|s| s.get("xuid"))
                    .and_then(|x| x.as_str())
                {
                    members.push(xuid.to_string());
                }
            }
        }

        // Get closed status
        let closed = json
            .get("properties")
            .and_then(|p| p.get("system"))
            .and_then(|s| s.get("closed"))
            .and_then(|c| c.as_bool())
            .unwrap_or(false);

        Some(SessionSnapshot {
            version,
            protocol,
            nethernet_id,
            host_name,
            world_name,
            members,
            closed,
        })
    }

    /// Check if session has been tampered with.
    ///
    /// Compares the expected session state against what's actually on Xbox.
    /// Returns tampering details if detected.
    pub async fn check_tampering(
        &self,
        token: &XblToken,
        session: &ExpandedSessionInfo,
    ) -> XblResult<TamperResult> {
        let expected = SessionSnapshot::from_expected(session);

        // Get current session state
        let json = self.get_session(token, &session.session_id).await?;

        let actual = Self::parse_session_snapshot(&json)
            .ok_or_else(|| XblError::XboxLive("Failed to parse session response".into()))?;

        let result = expected.compare(&actual);

        if result.tampered {
            warn!(
                "Session tampering detected! Modified fields: {:?}",
                result.modified_fields
            );
        }

        Ok(result)
    }

    /// Check tampering via handle (what clients see).
    ///
    /// This is more accurate for detecting attacks since clients
    /// fetch session data via handle, not session ID.
    pub async fn check_tampering_via_handle(
        &self,
        token: &XblToken,
        session: &ExpandedSessionInfo,
        handle_id: &str,
    ) -> XblResult<TamperResult> {
        let expected = SessionSnapshot::from_expected(session);

        // Get session via handle (what clients see)
        let json = self.get_session_via_handle(token, handle_id).await?;

        // Debug: Log raw session custom properties for attack analysis
        if let Some(custom) = json.get("properties").and_then(|p| p.get("custom")) {
            debug!("Raw session custom properties: {}", custom);
        }

        let actual = Self::parse_session_snapshot(&json).ok_or_else(|| {
            // Log full JSON on parse failure - might indicate attack
            warn!("Failed to parse session! Raw JSON: {}", json);
            XblError::XboxLive("Failed to parse session response".into())
        })?;

        let result = expected.compare(&actual);

        if result.tampered {
            warn!(
                "Session tampering detected (via handle)! Modified fields: {:?}",
                result.modified_fields
            );
            // Log full raw JSON when tampering detected
            warn!("Full session JSON for analysis: {}", json);
        }

        Ok(result)
    }

    /// Repair a tampered session by re-pushing our data.
    ///
    /// Call this if tampering is detected to overwrite attacker's changes.
    pub async fn repair_session(
        &self,
        token: &XblToken,
        session: &ExpandedSessionInfo,
    ) -> XblResult<()> {
        info!(
            session_id = %session.session_id,
            "Repairing potentially tampered session"
        );
        self.create_session(token, session).await
    }

    // ========================================================================
    // Handle Verification
    // ========================================================================

    /// Get handle metadata (not the session, just the handle info).
    ///
    /// Returns the handle's session reference to verify it points to our session.
    pub async fn get_handle(
        &self,
        token: &XblToken,
        handle_id: &str,
    ) -> XblResult<serde_json::Value> {
        let url = endpoints::GET_HANDLE_FMT.replace("{}", handle_id);
        let authorization = token.auth_header();
        let signature = Self::signature("GET", &url, &authorization, b"")?;

        let response = self
            .client
            .get(&url)
            .header("Authorization", authorization)
            .header("Signature", signature)
            .header("x-xbl-contract-version", "107")
            .send()
            .await?;
        Self::sync_server_time(&response);

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(XblError::XboxLive(format!(
                "Get handle failed ({}): {}",
                status, body
            )));
        }

        let json: serde_json::Value = response.json().await?;
        Ok(json)
    }

    /// Verify our handle still exists and points to our session.
    ///
    /// Returns `HandleVerification` with details about handle state.
    pub async fn verify_handle(
        &self,
        token: &XblToken,
        expected_handle_id: &str,
        expected_session_id: &str,
    ) -> XblResult<HandleVerification> {
        // Check if handle still exists
        let handle_result = self.get_handle(token, expected_handle_id).await;

        match handle_result {
            Ok(handle_json) => {
                // Handle exists, verify it points to our session
                let session_name = handle_json
                    .get("sessionRef")
                    .and_then(|s| s.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("");

                if session_name == expected_session_id {
                    Ok(HandleVerification {
                        handle_exists: true,
                        points_to_correct_session: true,
                        actual_session_id: Some(session_name.to_string()),
                        issue: None,
                    })
                } else {
                    warn!(
                        "Handle {} points to wrong session! Expected {}, got {}",
                        expected_handle_id, expected_session_id, session_name
                    );
                    Ok(HandleVerification {
                        handle_exists: true,
                        points_to_correct_session: false,
                        actual_session_id: Some(session_name.to_string()),
                        issue: Some(format!(
                            "Handle points to different session: {}",
                            session_name
                        )),
                    })
                }
            }
            Err(e) => {
                // Handle might have been deleted
                warn!("Handle {} no longer exists: {}", expected_handle_id, e);
                Ok(HandleVerification {
                    handle_exists: false,
                    points_to_correct_session: false,
                    actual_session_id: None,
                    issue: Some("Handle was deleted or expired".to_string()),
                })
            }
        }
    }

    /// Check for handle hijacking by verifying our handle exists and points to our session.
    ///
    /// Note: Xbox API doesn't allow querying all handles for a XUID (403 Forbidden),
    /// so we can only verify our own handle. We detect:
    /// - Handle deleted (attacker removed it)
    /// - Handle redirected (points to different session)
    pub async fn check_handle_hijacking(
        &self,
        token: &XblToken,
        _our_xuid: &str,
        our_handle_id: &str,
        our_session_id: &str,
    ) -> XblResult<HandleHijackResult> {
        // Verify our handle exists and points to correct session
        let verification = self
            .verify_handle(token, our_handle_id, our_session_id)
            .await?;

        let mut suspicious_handles = Vec::new();

        // Check for issues
        if !verification.handle_exists {
            suspicious_handles.push(SuspiciousHandle {
                handle_id: our_handle_id.to_string(),
                session_id: String::new(),
                reason: "Our handle was deleted!".to_string(),
            });
        } else if !verification.points_to_correct_session {
            suspicious_handles.push(SuspiciousHandle {
                handle_id: our_handle_id.to_string(),
                session_id: verification.actual_session_id.clone().unwrap_or_default(),
                reason: format!(
                    "Handle redirected to different session: {}",
                    verification
                        .actual_session_id
                        .as_deref()
                        .unwrap_or("unknown")
                ),
            });
        }

        let hijack_detected = !suspicious_handles.is_empty();

        Ok(HandleHijackResult {
            our_handle_exists: verification.handle_exists,
            hijack_detected,
            suspicious_handles,
            total_handles_for_xuid: if verification.handle_exists { 1 } else { 0 },
        })
    }
}

/// Information about a session handle.
#[derive(Debug, Clone)]
pub struct HandleInfo {
    /// Handle ID (GUID).
    pub id: String,
    /// Handle type (usually "activity").
    pub handle_type: String,
    /// Session name/ID this handle points to.
    pub session_name: String,
    /// Service config ID.
    pub scid: String,
}

/// Result of handle verification.
#[derive(Debug, Clone)]
pub struct HandleVerification {
    /// Whether the handle still exists.
    pub handle_exists: bool,
    /// Whether it points to the correct session.
    pub points_to_correct_session: bool,
    /// The actual session ID it points to (if any).
    pub actual_session_id: Option<String>,
    /// Description of any issue found.
    pub issue: Option<String>,
}

/// A suspicious handle that might be a hijacking attempt.
#[derive(Debug, Clone)]
pub struct SuspiciousHandle {
    /// The handle ID.
    pub handle_id: String,
    /// The session it points to.
    pub session_id: String,
    /// Why this is suspicious.
    pub reason: String,
}

/// Result of handle hijacking check.
#[derive(Debug, Clone)]
pub struct HandleHijackResult {
    /// Whether our expected handle still exists.
    pub our_handle_exists: bool,
    /// Whether potential hijacking was detected.
    pub hijack_detected: bool,
    /// List of suspicious handles found.
    pub suspicious_handles: Vec<SuspiciousHandle>,
    /// Total number of handles found for our XUID.
    pub total_handles_for_xuid: usize,
}

impl Default for SessionClient {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// MPSD Enumeration API (Security Research)
// ============================================================================

/// Known Minecraft Service Configuration IDs (SCIDs).
/// Different sources report different SCIDs - we test both.
pub mod scids {
    /// SCID used in axolotl-stack (from reverse engineering).
    pub const AXOLOTL: &str = "4fc10100-5f7a-4470-899b-280835760c07";
    /// SCID reported in security research discussions.
    pub const RESEARCH: &str = "4fc10100-5f7a-4470-899b-034571447731";
}

/// Known Minecraft session template names.
pub mod templates {
    /// Main lobby template for Minecraft sessions.
    pub const MINECRAFT_LOBBY: &str = "MinecraftLobby";
    /// Realms template (discovered via enumeration).
    pub const MINECRAFT_REALMS: &str = "MinecraftRealms";
    /// All known valid templates.
    pub const ALL: &[&str] = &[MINECRAFT_LOBBY, MINECRAFT_REALMS];
}

/// A session entry returned from bulk queries.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntry {
    /// Session name/ID.
    pub name: Option<String>,
    /// Session reference.
    #[serde(rename = "sessionRef")]
    pub session_ref: Option<SessionRefResponse>,
    /// Custom properties (contains NetherNet ID, version, etc).
    pub custom_properties: Option<serde_json::Value>,
    /// When this entry expires.
    pub expiration_time: Option<String>,
}

/// Session reference in response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRefResponse {
    pub scid: Option<String>,
    pub template_name: Option<String>,
    pub name: Option<String>,
}

/// Response from session enumeration queries.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionQueryResponse {
    /// List of sessions found.
    pub results: Option<Vec<SessionEntry>>,
    /// Continuation token for pagination.
    pub continuation_token: Option<String>,
}

/// Response from template enumeration.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateQueryResponse {
    /// List of template names.
    pub results: Option<Vec<TemplateEntry>>,
}

/// A template entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateEntry {
    pub name: Option<String>,
}

/// Batch query request body (for /batch endpoints).
#[derive(Debug, Serialize)]
pub struct BatchQueryRequest {
    /// Array of XUIDs to query.
    pub xuids: Vec<String>,
}

/// Handle query request body (for /handles/query).
/// See: https://docs.microsoft.com/en-us/gaming/xbox-live/features/multiplayer/mpsd/concepts/live-mpsd-handles
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandleQueryRequest {
    /// Type of handles to query (e.g., "activity", "invite").
    #[serde(rename = "type")]
    pub handle_type: String,
    /// Owners to query - array of objects with "type" and "xuid".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owners: Option<HandleOwners>,
}

/// Owners specification for handle query.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandleOwners {
    /// Type of owner lookup (e.g., "user").
    #[serde(rename = "type")]
    pub owner_type: String,
    /// XUIDs of owners to query.
    pub xuids: Vec<String>,
}

/// Result of an enumeration attempt.
#[derive(Debug)]
pub struct EnumerationResult {
    /// The endpoint tested.
    pub endpoint: String,
    /// HTTP status code received.
    pub status: u16,
    /// Whether the query was successful.
    pub success: bool,
    /// Number of sessions returned (if any).
    pub session_count: usize,
    /// Raw response body (for analysis).
    pub raw_response: String,
    /// Parsed sessions (if successful).
    pub sessions: Vec<SessionEntry>,
    /// Error message (if failed).
    pub error: Option<String>,
}

/// MPSD Enumeration client for security research.
///
/// This client tests whether the Xbox Live MPSD API allows
/// enumeration of sessions beyond what should be accessible.
pub struct MpsdEnumerator {
    client: reqwest::Client,
}

impl MpsdEnumerator {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    fn signature(
        method: &str,
        url: &str,
        authorization: &str,
        body: &[u8],
    ) -> Result<String, String> {
        sign_request_for_url(method, url, authorization, body)
    }

    fn sync_server_time(response: &reqwest::Response) {
        if let Some(date) = response.headers().get("Date").and_then(|v| v.to_str().ok()) {
            update_server_time_from_header(date);
        }
    }

    /// Enumerate session templates for a SCID.
    ///
    /// GET /serviceconfigs/{scid}/sessiontemplates
    pub async fn enumerate_templates(&self, token: &XblToken, scid: &str) -> EnumerationResult {
        let url = endpoints::SESSION_TEMPLATES_FMT.replace("{scid}", scid);

        self.do_get_request(token, &url, "enumerate_templates")
            .await
    }

    /// Enumerate sessions by SCID only.
    ///
    /// GET /serviceconfigs/{scid}/sessions
    pub async fn enumerate_by_scid(&self, token: &XblToken, scid: &str) -> EnumerationResult {
        let url = endpoints::SCID_SESSIONS_FMT.replace("{scid}", scid);

        self.do_get_request(token, &url, "enumerate_by_scid").await
    }

    /// Enumerate sessions by SCID and template.
    ///
    /// GET /serviceconfigs/{scid}/sessiontemplates/{templateName}/sessions
    pub async fn enumerate_by_template(
        &self,
        token: &XblToken,
        scid: &str,
        template: &str,
    ) -> EnumerationResult {
        let url = endpoints::TEMPLATE_SESSIONS_FMT
            .replace("{scid}", scid)
            .replace("{template}", template);

        self.do_get_request(token, &url, "enumerate_by_template")
            .await
    }

    /// Batch query at SCID level with list of XUIDs.
    ///
    /// POST /serviceconfigs/{scid}/batch
    pub async fn batch_query_scid(
        &self,
        token: &XblToken,
        scid: &str,
        xuids: &[&str],
    ) -> EnumerationResult {
        let url = endpoints::SCID_BATCH_FMT.replace("{scid}", scid);

        let body = BatchQueryRequest {
            xuids: xuids.iter().map(|s| s.to_string()).collect(),
        };

        self.do_post_request(token, &url, &body, "batch_query_scid")
            .await
    }

    /// Batch query at template level with list of XUIDs.
    ///
    /// POST /serviceconfigs/{scid}/sessiontemplates/{templateName}/batch
    pub async fn batch_query_template(
        &self,
        token: &XblToken,
        scid: &str,
        template: &str,
        xuids: &[&str],
    ) -> EnumerationResult {
        let url = endpoints::TEMPLATE_BATCH_FMT
            .replace("{scid}", scid)
            .replace("{template}", template);

        let body = BatchQueryRequest {
            xuids: xuids.iter().map(|s| s.to_string()).collect(),
        };

        self.do_post_request(token, &url, &body, "batch_query_template")
            .await
    }

    /// Query session handles by type.
    ///
    /// POST /handles/query
    pub async fn query_handles(
        &self,
        token: &XblToken,
        handle_type: &str,
        xuids: &[&str],
    ) -> EnumerationResult {
        let body = HandleQueryRequest {
            handle_type: handle_type.to_string(),
            owners: Some(HandleOwners {
                owner_type: "user".to_string(),
                xuids: xuids.iter().map(|s| s.to_string()).collect(),
            }),
        };

        self.do_post_request(token, endpoints::QUERY_HANDLES, &body, "query_handles")
            .await
    }

    /// Enumerate sessions by keyword search.
    /// This is the potential vulnerability vector - can we search for sessions
    /// that don't belong to our friends?
    ///
    /// GET /serviceconfigs/{scid}/sessiontemplates/{templateName}/sessions?keyword=...
    pub async fn enumerate_by_keyword(
        &self,
        token: &XblToken,
        scid: &str,
        template: &str,
        keyword: &str,
    ) -> EnumerationResult {
        self.enumerate_with_params(token, scid, template, &[("keyword", keyword)], "107")
            .await
    }

    /// Query PeopleHub for friend presence with decorations.
    /// This might expose session/game info for friends.
    pub async fn query_peoplehub_presence(&self, token: &XblToken) -> EnumerationResult {
        // Simple friends list with presence
        let url = "https://peoplehub.xboxlive.com/users/me/people/social";

        self.do_get_request_with_headers(
            token,
            &url,
            "peoplehub_presence",
            &[
                ("x-xbl-contract-version", "5"),
                ("Accept-Encoding", "identity"), // Disable compression
            ],
        )
        .await
    }

    /// Query activity feed for potential session exposure.
    pub async fn query_activity_feed(&self, token: &XblToken) -> EnumerationResult {
        // Correct activity endpoint
        let url = "https://userpresence.xboxlive.com/users/me";

        self.do_get_request_with_headers(
            token,
            &url,
            "activity_feed",
            &[("x-xbl-contract-version", "3")],
        )
        .await
    }

    /// Get rich presence for a specific XUID - might contain session handle.
    pub async fn get_user_presence(&self, token: &XblToken, xuid: &str) -> EnumerationResult {
        // Get presence record for a user
        let url = format!("https://userpresence.xboxlive.com/users/xuid({})", xuid);

        self.do_get_request_with_headers(
            token,
            &url,
            "user_presence",
            &[("x-xbl-contract-version", "3")],
        )
        .await
    }

    /// Get multiplayer activity for friends - might expose session handles
    pub async fn get_multiplayer_activity(&self, token: &XblToken) -> EnumerationResult {
        // Try POST instead of GET
        let url = "https://sessiondirectory.xboxlive.com/handles/query";

        let body = serde_json::json!({
            "type": "activity",
            "scid": SERVICE_CONFIG_ID
        });

        self.do_post_request(token, &url, &body, "multiplayer_activity")
            .await
    }

    // ========================================================================
    // Minecraft Signaling Server Tests
    // ========================================================================

    /// Test signaling server HTTP endpoints.
    /// The signaling server is at signal.franchise.minecraft-services.net
    pub async fn test_signaling_discovery(&self, token: &XblToken) -> EnumerationResult {
        // Try the root endpoint
        let url = "https://signal.franchise.minecraft-services.net/";

        self.do_get_request(token, &url, "signaling_root").await
    }

    /// Test signaling API endpoints
    pub async fn test_signaling_api(&self, token: &XblToken) -> EnumerationResult {
        let url = "https://signal.franchise.minecraft-services.net/api/v1.0/";

        self.do_get_request(token, &url, "signaling_api").await
    }

    /// Test authorization server session endpoints
    pub async fn test_auth_sessions(&self, token: &XblToken) -> EnumerationResult {
        // Try listing sessions endpoint
        let url = "https://authorization.franchise.minecraft-services.net/api/v1.0/sessions";

        self.do_get_request(token, &url, "auth_sessions").await
    }

    /// Test Minecraft services discovery
    pub async fn test_mc_discovery(&self, token: &XblToken) -> EnumerationResult {
        let url = "https://client.discovery.minecraft-services.net/api/v1.0/discovery/MinecraftPE";

        self.do_get_request(token, &url, "mc_discovery").await
    }

    /// Test Realms API
    pub async fn test_realms_api(&self, token: &XblToken) -> EnumerationResult {
        let url = "https://pocket.realms.minecraft.net/worlds";

        self.do_get_request_with_headers(
            token,
            &url,
            "realms_api",
            &[("Client-Version", "1.21.50"), ("User-Agent", "MCPE/UWP")],
        )
        .await
    }

    async fn do_get_request_with_headers(
        &self,
        token: &XblToken,
        url: &str,
        endpoint_name: &str,
        headers: &[(&str, &str)],
    ) -> EnumerationResult {
        debug!("GET {} with custom headers", url);
        let authorization = token.auth_header();
        let signature = match Self::signature("GET", url, &authorization, b"") {
            Ok(sig) => sig,
            Err(e) => {
                return EnumerationResult {
                    endpoint: format!("{} ({})", endpoint_name, url),
                    status: 0,
                    success: false,
                    session_count: 0,
                    raw_response: String::new(),
                    sessions: Vec::new(),
                    error: Some(format!("Failed to sign request: {}", e)),
                };
            }
        };

        let mut req = self
            .client
            .get(url)
            .header("Authorization", authorization.clone())
            .header("Signature", signature)
            .header("Accept", "application/json");

        for (key, value) in headers {
            req = req.header(*key, *value);
        }

        let response = match req.send().await {
            Ok(r) => {
                Self::sync_server_time(&r);
                r
            }
            Err(e) => {
                return EnumerationResult {
                    endpoint: format!("{} ({})", endpoint_name, url),
                    status: 0,
                    success: false,
                    session_count: 0,
                    raw_response: String::new(),
                    sessions: Vec::new(),
                    error: Some(format!("Request failed: {}", e)),
                };
            }
        };

        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();

        self.parse_response(endpoint_name, url, status, body)
    }

    /// Run all enumeration tests and return results.
    pub async fn run_all_tests(&self, token: &XblToken) -> Vec<EnumerationResult> {
        let mut results = Vec::new();
        let own_xuid = token.xuid();

        // Test both known SCIDs
        for scid in [scids::AXOLOTL, scids::RESEARCH] {
            info!("Testing SCID: {}", scid);

            // 1. Enumerate templates
            results.push(self.enumerate_templates(token, scid).await);

            // 2. Enumerate by SCID only
            results.push(self.enumerate_by_scid(token, scid).await);

            // 3. Enumerate by each template (with XUID)
            for template in templates::ALL {
                results.push(
                    self.enumerate_with_params(token, scid, template, &[("xuid", own_xuid)], "107")
                        .await,
                );
            }

            // 4. Keyword enumeration (the vulnerability test)
            for template in templates::ALL {
                for keyword in ["a", "Minecraft", "Server"] {
                    results.push(
                        self.enumerate_by_keyword(token, scid, template, keyword)
                            .await,
                    );
                }
            }

            // 5. Batch query at SCID level
            results.push(self.batch_query_scid(token, scid, &[own_xuid]).await);

            // 6. Batch query at template level
            for template in templates::ALL {
                results.push(
                    self.batch_query_template(token, scid, template, &[own_xuid])
                        .await,
                );
            }
        }

        // 7. Query handles
        results.push(self.query_handles(token, "activity", &[own_xuid]).await);

        results
    }

    /// Query sessions with specific parameters.
    ///
    /// GET /serviceconfigs/{scid}/sessiontemplates/{templateName}/sessions?keyword=...
    pub async fn enumerate_with_params(
        &self,
        token: &XblToken,
        scid: &str,
        template: &str,
        params: &[(&str, &str)],
        contract_version: &str,
    ) -> EnumerationResult {
        let mut url = endpoints::TEMPLATE_SESSIONS_FMT
            .replace("{scid}", scid)
            .replace("{template}", template);

        if !params.is_empty() {
            url.push('?');
            for (i, (k, v)) in params.iter().enumerate() {
                if i > 0 {
                    url.push('&');
                }
                url.push_str(k);
                url.push('=');
                url.push_str(v);
            }
        }

        self.do_get_request_versioned(token, &url, "enumerate_with_params", contract_version)
            .await
    }

    async fn do_get_request(
        &self,
        token: &XblToken,
        url: &str,
        endpoint_name: &str,
    ) -> EnumerationResult {
        self.do_get_request_versioned(token, url, endpoint_name, "107")
            .await
    }

    async fn do_get_request_versioned(
        &self,
        token: &XblToken,
        url: &str,
        endpoint_name: &str,
        contract_version: &str,
    ) -> EnumerationResult {
        debug!("GET {} (contract: {})", url, contract_version);
        let authorization = token.auth_header();
        let signature = match Self::signature("GET", url, &authorization, b"") {
            Ok(sig) => sig,
            Err(e) => {
                return EnumerationResult {
                    endpoint: format!("{} ({})", endpoint_name, url),
                    status: 0,
                    success: false,
                    session_count: 0,
                    raw_response: String::new(),
                    sessions: Vec::new(),
                    error: Some(format!("Failed to sign request: {}", e)),
                };
            }
        };
        let req = self
            .client
            .get(url)
            .header("Authorization", authorization.clone())
            .header("Signature", signature)
            .header("x-xbl-contract-version", contract_version)
            .header("Accept", "application/json");

        let response = match req.send().await {
            Ok(r) => {
                Self::sync_server_time(&r);
                r
            }
            Err(e) => {
                return EnumerationResult {
                    endpoint: format!("{} ({})", endpoint_name, url),
                    status: 0,
                    success: false,
                    session_count: 0,
                    raw_response: String::new(),
                    sessions: Vec::new(),
                    error: Some(format!("Request failed: {}", e)),
                };
            }
        };

        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();

        self.parse_response(endpoint_name, url, status, body)
    }

    async fn do_post_request<T: Serialize>(
        &self,
        token: &XblToken,
        url: &str,
        body: &T,
        endpoint_name: &str,
    ) -> EnumerationResult {
        debug!("POST {}", url);
        let body_bytes = match serde_json::to_vec(body) {
            Ok(b) => b,
            Err(e) => {
                return EnumerationResult {
                    endpoint: format!("{} ({})", endpoint_name, url),
                    status: 0,
                    success: false,
                    session_count: 0,
                    raw_response: String::new(),
                    sessions: Vec::new(),
                    error: Some(format!("Failed to serialize request body: {}", e)),
                };
            }
        };
        let authorization = token.auth_header();
        let signature = match Self::signature("POST", url, &authorization, &body_bytes) {
            Ok(sig) => sig,
            Err(e) => {
                return EnumerationResult {
                    endpoint: format!("{} ({})", endpoint_name, url),
                    status: 0,
                    success: false,
                    session_count: 0,
                    raw_response: String::new(),
                    sessions: Vec::new(),
                    error: Some(format!("Failed to sign request: {}", e)),
                };
            }
        };

        let req = self
            .client
            .post(url)
            .header("Authorization", authorization.clone())
            .header("Signature", signature)
            .header("x-xbl-contract-version", "107")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(body_bytes.clone());

        let response = match req.send().await {
            Ok(r) => {
                Self::sync_server_time(&r);
                r
            }
            Err(e) => {
                return EnumerationResult {
                    endpoint: format!("{} ({})", endpoint_name, url),
                    status: 0,
                    success: false,
                    session_count: 0,
                    raw_response: String::new(),
                    sessions: Vec::new(),
                    error: Some(format!("Request failed: {}", e)),
                };
            }
        };

        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();

        self.parse_response(endpoint_name, url, status, body)
    }

    fn parse_response(
        &self,
        endpoint_name: &str,
        url: &str,
        status: u16,
        body: String,
    ) -> EnumerationResult {
        let success = status >= 200 && status < 300;

        let sessions = if success {
            // Try to parse as session query response
            if let Ok(parsed) = serde_json::from_str::<SessionQueryResponse>(&body) {
                parsed.results.unwrap_or_default()
            } else if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body) {
                // Try to extract results from generic JSON
                if let Some(results) = parsed.get("results").and_then(|r| r.as_array()) {
                    results
                        .iter()
                        .filter_map(|v| serde_json::from_value(v.clone()).ok())
                        .collect()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let session_count = sessions.len();

        EnumerationResult {
            endpoint: format!("{} ({})", endpoint_name, url),
            status,
            success,
            session_count,
            raw_response: body,
            sessions,
            error: if success {
                None
            } else {
                Some(format!("HTTP {}", status))
            },
        }
    }
}

impl Default for MpsdEnumerator {
    fn default() -> Self {
        Self::new()
    }
}
