/// Minecraft service config ID for session directory.
pub const SERVICE_CONFIG_ID: &str = "4fc10100-5f7a-4470-899b-280835760c07";

/// Session template name for Minecraft lobbies.
pub const TEMPLATE_NAME: &str = "MinecraftLobby";

/// Title ID for Minecraft Windows Edition.
pub const TITLE_ID: &str = "896928775";

/// Maximum friends allowed on Xbox Live.
pub const MAX_FRIENDS: usize = 2000;

// NOTE: Connection types (RakNetV1, RakNetV2, WebRTC, Lan) are defined in
// `tokio_nethernet::ConnectionType` to avoid duplication.

/// API Endpoints
pub mod endpoints {
    /// Session directory base URL.
    pub const SESSION_DIRECTORY: &str = "https://sessiondirectory.xboxlive.com";

    /// Create/update session URL (format with session ID).
    pub const CREATE_SESSION_FMT: &str = "https://sessiondirectory.xboxlive.com/serviceconfigs/4fc10100-5f7a-4470-899b-280835760c07/sessionTemplates/MinecraftLobby/sessions/";

    /// Session handles endpoint.
    pub const CREATE_HANDLE: &str = "https://sessiondirectory.xboxlive.com/handles";

    /// Join session via handle (format with handle ID).
    pub const JOIN_SESSION_FMT: &str = "https://sessiondirectory.xboxlive.com/handles/{}/session";

    /// PlayFab login for Minecraft.
    pub const PLAYFAB_LOGIN: &str = "https://20ca2.playfabapi.com/Client/LoginWithXbox";

    /// Minecraft services session start.
    pub const MC_SESSION_START: &str =
        "https://authorization.franchise.minecraft-services.net/api/v1.0/session/start";

    /// RTC WebSocket for signaling (format with nethernet ID).
    pub const RTC_WEBSOCKET_FMT: &str =
        "wss://signal.franchise.minecraft-services.net/ws/v1.0/signaling/";

    /// RTA WebSocket for presence/connections.
    pub const RTA_WEBSOCKET: &str = "wss://rta.xboxlive.com/connect";

    /// People endpoint for friend operations (format with XUID).
    pub const PEOPLE_FMT: &str = "https://social.xboxlive.com/users/me/people/xuid({})";

    /// User presence endpoint (format with XUID).
    pub const USER_PRESENCE_FMT: &str =
        "https://userpresence.xboxlive.com/users/xuid({})/devices/current/titles/current";

    /// Get followers (people following you).
    pub const FOLLOWERS: &str = "https://peoplehub.xboxlive.com/users/me/people/followers";

    /// Get social (people you follow).
    pub const SOCIAL: &str = "https://peoplehub.xboxlive.com/users/me/people/social";

    /// Social summary (friend counts).
    pub const SOCIAL_SUMMARY: &str = "https://social.xboxlive.com/users/me/summary";

    /// Force unfollow a follower (format with XUID).
    pub const FOLLOWER_FMT: &str = "https://social.xboxlive.com/users/me/people/follower/xuid({})";

    /// Pending friend requests.
    pub const FRIEND_REQUESTS: &str =
        "https://peoplehub.xboxlive.com/users/me/people/friendrequests(received)";

    /// Bulk friend add.
    pub const BULK_ADD_FRIENDS: &str =
        "https://social.xboxlive.com/bulk/users/me/people/friends/v2?method=add";

    /// Profile settings (format with XUID).
    pub const PROFILE_SETTINGS_FMT: &str =
        "https://profile.xboxlive.com/users/xuid({})/profile/settings?settings=Gamertag";

    /// Gallery for showcase images.
    pub const GALLERY: &str = "https://persona.franchise.minecraft-services.net/api/v1.0/gallery";
}
