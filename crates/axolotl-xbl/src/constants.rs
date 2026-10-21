//! Endpoint provenance refers exclusively to the cloned df-mc repositories.

/// df-mc/go-xsapi xal/sisu/session_test.go:serviceConfigID.
pub const SERVICE_CONFIG_ID: &str = "4fc10100-5f7a-4470-899b-280835760c07";

/// df-mc/go-xsapi xal/sisu/session_test.go:publishSession.
pub const TEMPLATE_NAME: &str = "MinecraftLobby";

/// df-mc/go-xsapi xal/sisu/session_test.go:publishSession (Invite title ID).
pub const TITLE_ID: &str = "896928775";

pub mod endpoints {
    /// df-mc/go-xsapi mpsd/session.go:SessionReference.URL; title configuration
    /// comes from xal/sisu/session_test.go:publishSession.
    pub const CREATE_SESSION_FMT: &str = "https://sessiondirectory.xboxlive.com/serviceconfigs/4fc10100-5f7a-4470-899b-280835760c07/sessionTemplates/MinecraftLobby/sessions/";

    /// df-mc/go-xsapi mpsd/activity.go:writeActivity, Invite.
    pub const CREATE_HANDLE: &str = "https://sessiondirectory.xboxlive.com/handles";

    /// df-mc/go-xsapi mpsd/join.go:Join.
    pub const JOIN_SESSION_FMT: &str = "https://sessiondirectory.xboxlive.com/handles/{}/session";

    /// df-mc/go-xsapi mpsd/activity.go:ActivityHandle.URL.
    pub const GET_HANDLE_FMT: &str = "https://sessiondirectory.xboxlive.com/handles/{}";

    /// df-mc/go-playfab xbox.go:XBLIdentityProvider.Login; Minecraft title configuration.
    pub const PLAYFAB_LOGIN: &str = "https://20ca2.playfabapi.com/Client/LoginWithXbox";

    /// Retained Minecraft authorization flow; not defined by go-xsapi.
    pub const MC_SESSION_START: &str =
        "https://authorization.franchise.minecraft-services.net/api/v1.0/session/start";

    /// df-mc/go-xsapi rta/dial.go:connectURL, subprotocol.
    pub const RTA_WEBSOCKET: &str = "wss://rta.xboxlive.com/connect";

    /// df-mc/go-xsapi presence/client.go:update, remove.
    pub const USER_PRESENCE_FMT: &str =
        "https://userpresence.xboxlive.com/users/xuid({})/devices/current/titles/current";

    /// Retained Minecraft gallery functionality; no equivalent in the cloned
    /// go-xsapi, go-playfab, or nethernet reference APIs.
    pub const GALLERY: &str = "https://persona.franchise.minecraft-services.net/api/v1.0/gallery";
}
