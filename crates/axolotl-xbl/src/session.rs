//! Xbox Live Session API.
//!
//! Create and manage Xbox Live sessions for Minecraft multiplayer.
//!
//! ## Session Monitoring
//!
//! Use [`SessionClient::get_session`] and [`SessionClient::get_session_via_handle`]
//! to monitor session state and detect tampering attacks.

use crate::auth::XblToken;
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

        let response = self
            .client
            .get(&url)
            .header("Authorization", token.auth_header())
            .header("x-xbl-contract-version", "107")
            .send()
            .await?;

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

        let response = self
            .client
            .get(&url)
            .header("Authorization", token.auth_header())
            .header("x-xbl-contract-version", "107")
            .send()
            .await?;

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

        let actual = Self::parse_session_snapshot(&json).ok_or_else(|| {
            XblError::XboxLive("Failed to parse session response".into())
        })?;

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

        let actual = Self::parse_session_snapshot(&json).ok_or_else(|| {
            XblError::XboxLive("Failed to parse session response".into())
        })?;

        let result = expected.compare(&actual);

        if result.tampered {
            warn!(
                "Session tampering detected (via handle)! Modified fields: {:?}",
                result.modified_fields
            );
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

        let response = self
            .client
            .get(&url)
            .header("Authorization", token.auth_header())
            .header("x-xbl-contract-version", "107")
            .send()
            .await?;

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
                    verification.actual_session_id.as_deref().unwrap_or("unknown")
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
