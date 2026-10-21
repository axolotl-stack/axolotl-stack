//! PlayFab authentication for Minecraft services.
//!
//! Go references: go-playfab v2 and gophertunnel minecraft/service at
//! be557e33fe6f8d0c0db7ef05b5fd068725c325ad.

use crate::constants::endpoints;
use crate::error::{XblError, XblResult, check_status, response_json};
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use tracing::{debug, info};

/// gophertunnel `minecraft/service/internal/result.go:UserAgent`.
pub(crate) const SERVICE_USER_AGENT: &str = "libhttpclient/1.0.0.0";

/// Xbox login payload from go-playfab `xbox.go:XBLIdentityProvider.Login`
/// and `login.go:LoginRequest`. Unused optional fields are omitted, not null.
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct PlayFabLoginRequest {
    create_account: bool,
    title_id: String,
    xbox_token: String,
}

/// Login envelope from go-playfab `internal/request.go:Post`.
#[derive(Deserialize)]
pub struct PlayFabLoginResponse {
    pub data: Option<PlayFabLoginData>,
}

/// Login identity from go-playfab `login.go:LoginResult`.
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PlayFabLoginData {
    pub session_ticket: String,
    #[serde(rename = "PlayFabId")]
    pub playfab_id: String,
    pub entity_token: EntityToken,
}

#[derive(Deserialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    treatment_overrides: Option<Vec<String>>,
    #[serde(rename = "type")]
    device_type: String,
}

#[derive(Serialize)]
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

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResult {
    pub authorization_header: String,
    pub valid_until: DateTime<Utc>,
    #[serde(skip)]
    pub player_messaging_id: uuid::Uuid,
}

impl SessionResult {
    /// gophertunnel `minecraft/service/token.go:Token.Valid`: one-minute margin.
    pub fn is_valid(&self) -> bool {
        !self.authorization_header.is_empty()
            && self.valid_until.signed_duration_since(Utc::now()) > chrono::Duration::minutes(1)
    }

    /// gophertunnel `token.go:decodeClaims/Claims.Validate`. Claims from the HTTPS
    /// service response are parsed, not signature-verified (matching upstream).
    fn decode_claims(&mut self) -> XblResult<()> {
        #[derive(Deserialize)]
        struct Claims {
            pmid: String,
            exp: Option<i64>,
            nbf: Option<i64>,
            iat: Option<i64>,
        }
        let invalid = || XblError::Auth("Invalid Minecraft service token claims".into());
        let jwt = self
            .authorization_header
            .strip_prefix("MCToken ")
            .unwrap_or(&self.authorization_header);
        let mut parts = jwt.split('.');
        let _header = parts.next().ok_or_else(invalid)?;
        let payload = parts.next().ok_or_else(invalid)?;
        if parts.next().is_none() || parts.next().is_some() {
            return Err(invalid());
        }
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| invalid())?;
        let claims: Claims = serde_json::from_slice(&payload).map_err(|_| invalid())?;
        let pmid = uuid::Uuid::parse_str(&claims.pmid).map_err(|_| invalid())?;
        let now = Utc::now().timestamp();
        // go-jose jwt.Claims.Validate uses a default one-minute clock leeway.
        if pmid.is_nil()
            || claims.exp.is_some_and(|v| v < now - 60)
            || claims.nbf.is_some_and(|v| v > now + 60)
            || claims.iat.is_some_and(|v| v > now + 60)
        {
            return Err(invalid());
        }
        self.player_messaging_id = pmid;
        Ok(())
    }
}

// Redact entire authentication objects, including untrusted server strings.
macro_rules! redacted_debug {
    ($($ty:ty),+ $(,)?) => {$(
        impl std::fmt::Debug for $ty {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(concat!(stringify!($ty), " { [REDACTED] }"))
            }
        }
    )+};
}

redacted_debug!(
    PlayFabLoginRequest,
    PlayFabLoginResponse,
    PlayFabLoginData,
    EntityToken,
    SessionUser,
    SessionResult
);

/// PlayFab/Minecraft auth client.
pub struct PlayFabClient {
    client: reqwest::Client,
    login_url: String,
    session_url: String,
    renew_url: String,
    multiplayer_url: String,
    title_id: String,
    login_cache: Mutex<Option<CachedLogin>>,
    service_cache: Mutex<Option<CachedService>>,
}

struct CachedLogin {
    user_hash: String,
    ticket: String,
    expires_at: std::time::Instant,
}

struct CachedService {
    device_id: String,
    game_version: String,
    ticket_fingerprint: [u8; 32],
    token: SessionResult,
}

impl PlayFabClient {
    /// Create a new PlayFab client.
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            login_url: endpoints::PLAYFAB_LOGIN.into(),
            session_url: endpoints::MC_SESSION_START.into(),
            renew_url: "https://authorization.franchise.minecraft-services.net/api/v1.0/session/renew".into(),
            multiplayer_url: "https://authorization.franchise.minecraft-services.net/api/v1.0/multiplayer/session/start".into(),
            title_id: "20CA2".into(),
            login_cache: Mutex::new(None),
            service_cache: Mutex::new(None),
        }
    }

    /// Select discovered authorization service and title.
    /// gophertunnel `token.go:AuthorizationEnvironment.Token/Renew` and
    /// go-playfab `title/title.go:Title.URL`.
    pub fn with_environment(environment: &crate::discovery::AuthEndpoint) -> XblResult<Self> {
        let invalid = || XblError::Auth("Invalid Minecraft authorization environment".into());
        let title = environment
            .playfab_title_id
            .as_deref()
            .filter(|title| !title.is_empty() && title.bytes().all(|c| c.is_ascii_alphanumeric()))
            .ok_or_else(invalid)?;
        let mut base = reqwest::Url::parse(&environment.service_uri).map_err(|_| invalid())?;
        if base.scheme() != "https"
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
        {
            return Err(invalid());
        }
        let mut client = Self::new();
        let base_path = base.path().trim_end_matches('/').to_owned();
        base.set_path(&format!(
            "{}/api/v1.0/session/start",
            base.path().trim_end_matches('/')
        ));
        client.session_url = base.to_string();
        base.set_path(&format!("{}/renew", base.path().trim_end_matches("/start")));
        client.renew_url = base.to_string();
        base.set_path(&format!("{base_path}/api/v1.0/multiplayer/session/start"));
        client.multiplayer_url = base.to_string();
        client.title_id = title.to_owned();
        client.login_url = format!(
            "https://{}.playfabapi.com/Client/LoginWithXbox",
            title.to_ascii_lowercase()
        );
        Ok(client)
    }

    pub fn login_url(&self) -> &str {
        &self.login_url
    }

    /// Login to PlayFab with Xbox token info.
    ///
    /// * `user_hash`: The User Hash (uhs) from the PlayFab-scoped Xbox Live token.
    /// * `xbl_token`: The token string from the PlayFab-scoped Xbox Live token.
    pub async fn login(&self, user_hash: &str, xbl_token: &str) -> XblResult<String> {
        let mut cache = self.login_cache.lock().await;
        if let Some(cached) = cache
            .as_ref()
            .filter(|c| c.user_hash == user_hash && std::time::Instant::now() < c.expires_at)
        {
            return Ok(cached.ticket.clone());
        }
        let ticket = self.login_at(&self.login_url, user_hash, xbl_token).await?;
        if cache.as_ref().is_some_and(|c| c.user_hash != user_hash) {
            *self.service_cache.lock().await = None;
        }
        *cache = Some(CachedLogin {
            user_hash: user_hash.into(),
            ticket: ticket.clone(),
            // go-playfab client.go:loginExpiration - loginExpirationDelta.
            expires_at: std::time::Instant::now()
                + std::time::Duration::from_secs(24 * 3600 - 15 * 60),
        });
        Ok(ticket)
    }

    async fn login_at(
        &self,
        endpoint: &str,
        user_hash: &str,
        xbl_token: &str,
    ) -> XblResult<String> {
        let body = PlayFabLoginRequest {
            create_account: true,
            title_id: self.title_id.clone(),
            xbox_token: format!("XBL3.0 x={user_hash};{xbl_token}"),
        };

        debug!("Logging into PlayFab");

        let response = self
            .client
            .post(endpoint)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        // go-playfab internal/request.go:Post accepts only HTTP 200.
        check_status(&response, "PlayFab login")?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(XblError::HttpStatus {
                status: response.status(),
                operation: "PlayFab login",
            });
        }
        let login: PlayFabLoginResponse = response_json(response, "PlayFab login").await?;
        let data = login
            .data
            .filter(|data| !data.session_ticket.is_empty() && !data.playfab_id.is_empty())
            .ok_or_else(|| XblError::Auth("Invalid PlayFab login response".into()))?;

        info!("PlayFab login successful");
        Ok(data.session_ticket)
    }

    /// Cached franchise authentication. See gophertunnel
    /// `minecraft/service/token_source.go:tokenSource.ServiceToken`.
    pub async fn start_session(
        &self,
        device_id: &str,
        playfab_ticket: &str,
        game_version: &str,
    ) -> XblResult<String> {
        Ok(self
            .service_token(device_id, playfab_ticket, game_version)
            .await?
            .authorization_header)
    }

    /// Reuse valid tokens, renew within the one-minute margin, and start fresh
    /// after hard expiry or rejected renewal, matching Go `ServiceToken`.
    pub async fn service_token(
        &self,
        device_id: &str,
        playfab_ticket: &str,
        game_version: &str,
    ) -> XblResult<SessionResult> {
        let ticket_fingerprint: [u8; 32] = Sha256::digest(playfab_ticket.as_bytes()).into();
        let mut cache = self.service_cache.lock().await;
        let previous = cache.as_ref().filter(|c| {
            c.device_id == device_id
                && c.game_version == game_version
                && c.ticket_fingerprint == ticket_fingerprint
        });
        if let Some(cached) = previous.filter(|c| c.token.is_valid()) {
            return Ok(cached.token.clone());
        }
        if playfab_ticket.is_empty() || game_version.is_empty() {
            return Err(XblError::Auth(
                "Missing Minecraft session credentials or version".into(),
            ));
        }
        if let Some(cached) = previous.filter(|c| {
            !c.token.authorization_header.is_empty() && Utc::now() < c.token.valid_until
        }) && let Ok(token) = self.renew(&cached.token, playfab_ticket).await
        {
            *cache = Some(CachedService {
                device_id: device_id.into(),
                game_version: game_version.into(),
                ticket_fingerprint,
                token: token.clone(),
            });
            return Ok(token);
        }
        let token = self
            .start_session_at(&self.session_url, device_id, playfab_ticket, game_version)
            .await?;
        *cache = Some(CachedService {
            device_id: device_id.into(),
            game_version: game_version.into(),
            ticket_fingerprint,
            token: token.clone(),
        });
        Ok(token)
    }

    /// Forget only the rejected token; a late failure must not erase a newer token.
    pub async fn invalidate_service_token(&self, rejected: &str) {
        let mut cache = self.service_cache.lock().await;
        if cache
            .as_ref()
            .is_some_and(|cached| cached.token.authorization_header == rejected)
        {
            *cache = None;
        }
    }

    /// Issue a fresh per-connection multiplayer token.
    /// gophertunnel `minecraft/service/token.go:AuthorizationEnvironment.MultiplayerToken`.
    /// `public_key_der` is the caller's PKIX/SPKI-encoded public key.
    pub async fn multiplayer_token(
        &self,
        service_token: &SessionResult,
        public_key_der: &[u8],
    ) -> XblResult<String> {
        let response = self
            .client
            .post(&self.multiplayer_url)
            .header("Accept", "application/json")
            .header("Authorization", &service_token.authorization_header)
            .json(&serde_json::json!({
                "publicKey": base64::engine::general_purpose::STANDARD.encode(public_key_der)
            }))
            .send()
            .await?;
        check_status(&response, "Minecraft multiplayer session start")?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(XblError::HttpStatus {
                status: response.status(),
                operation: "Minecraft multiplayer session start",
            });
        }
        #[derive(Deserialize)]
        struct Response {
            result: MultiplayerToken,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct MultiplayerToken {
            signed_token: String,
        }
        let result: Response =
            response_json(response, "Minecraft multiplayer session start").await?;
        if result.result.signed_token.is_empty() {
            return Err(XblError::Auth(
                "Invalid Minecraft multiplayer token response".into(),
            ));
        }
        Ok(result.result.signed_token)
    }

    fn user(ticket: &str) -> SessionUser {
        SessionUser {
            language: "en".into(),
            language_code: "en-US".into(),
            region_code: "US".into(),
            token: ticket.into(),
            token_type: "PlayFab".into(),
        }
    }

    /// gophertunnel `minecraft/service/token.go:AuthorizationEnvironment.Renew`.
    async fn renew(&self, token: &SessionResult, ticket: &str) -> XblResult<SessionResult> {
        let response = self
            .client
            .post(&self.renew_url)
            .header("Accept", "application/json")
            .header("Authorization", &token.authorization_header)
            .json(&Self::user(ticket))
            .send()
            .await?;
        Self::decode_session(response, "Minecraft session renew").await
    }

    async fn decode_session(
        response: reqwest::Response,
        operation: &'static str,
    ) -> XblResult<SessionResult> {
        check_status(&response, operation)?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(XblError::HttpStatus {
                status: response.status(),
                operation,
            });
        }
        let mut session: SessionStartResponse = response_json(response, operation).await?;
        if !session.result.is_valid() {
            return Err(XblError::Auth("Invalid Minecraft session response".into()));
        }
        session.result.decode_claims()?;
        Ok(session.result)
    }

    /// gophertunnel `token.go:AuthorizationEnvironment.Token/defaultDeviceConfig`.
    async fn start_session_at(
        &self,
        endpoint: &str,
        device_id: &str,
        playfab_ticket: &str,
        game_version: &str,
    ) -> XblResult<SessionResult> {
        let body = SessionStartRequest {
            device: SessionDevice {
                application_type: "MinecraftPE".into(),
                capabilities: vec![],
                game_version: game_version.into(),
                id: if device_id.is_empty() {
                    uuid::Uuid::new_v4().to_string()
                } else {
                    device_id.into()
                },
                memory: "17179869184".into(),
                platform: "Windows10".into(),
                play_fab_title_id: self.title_id.clone(),
                store_platform: "uwp.store".into(),
                treatment_overrides: None,
                device_type: "Windows10".into(),
            },
            user: Self::user(playfab_ticket),
        };

        debug!("Starting Minecraft session");

        let response = self
            .client
            .post(endpoint)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .header("User-Agent", SERVICE_USER_AGENT)
            .json(&body)
            .send()
            .await?;

        Self::decode_session(response, "Minecraft session start").await
    }
}

impl Default for PlayFabClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn service_cache_binds_tokens_to_ticket_after_account_switch() {
        let mut client = PlayFabClient::new();
        let mut tickets = Vec::new();
        for account in ["account-a", "account-b"] {
            let body = serde_json::json!({"data": {
                "SessionTicket": format!("SECRET-{account}"),
                "PlayFabId": account,
                "EntityToken": {"EntityToken": "SECRET-entity"}
            }})
            .to_string();
            let (url, task) = server(200, &body).await;
            client.login_url = url;
            tickets.push(client.login(account, "SECRET-xsts").await.unwrap());
            task.await.unwrap();
        }
        // Account B has logged in, but a delayed caller still holds A's ticket.
        // A's late result must never satisfy B's service-token request.
        for (ticket_index, pmid) in [
            (0, "00112233-4455-6677-8899-aabbccddeeff"),
            (1, "11112233-4455-6677-8899-aabbccddeeff"),
            (0, "00112233-4455-6677-8899-aabbccddeeff"),
            (1, "11112233-4455-6677-8899-aabbccddeeff"),
        ] {
            let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(&serde_json::json!({"pmid": pmid, "exp": 4102444800_i64}))
                    .unwrap(),
            );
            let header = format!("MCToken e30.{payload}.signature");
            let body = serde_json::json!({"result": {
                "authorizationHeader": header,
                "validUntil": "2100-01-01T00:00:00Z"
            }})
            .to_string();
            let (url, task) = server(200, &body).await;
            client.session_url = url;
            let token = client
                .service_token("same-device", &tickets[ticket_index], "1.26.51")
                .await
                .unwrap();
            assert_eq!(token.authorization_header, header);
            assert_eq!(token.player_messaging_id.to_string(), pmid);
            let request = task.await.unwrap();
            let sent: serde_json::Value =
                serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(sent["user"]["token"], tickets[ticket_index]);
        }
    }

    fn token_fixture() -> String {
        use base64::Engine;
        let claims = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"pmid":"00112233-4455-6677-8899-aabbccddeeff","exp":4102444800}"#);
        serde_json::json!({"result": {
            "authorizationHeader": format!("MCToken e30.{claims}.signature"),
            "validUntil": "2100-01-01T00:00:00Z"
        }})
        .to_string()
    }

    #[tokio::test]
    async fn multiplayer_token_transmits_each_key_and_redacts_failures() {
        let token: SessionStartResponse = serde_json::from_str(&token_fixture()).unwrap();
        let mut client = PlayFabClient::new();
        for (key, encoded) in [(vec![1, 2, 3], "AQID"), (vec![255, 254], "//4=")] {
            let (url, task) = server(200, r#"{"result":{"signedToken":"SECRET-signed"}}"#).await;
            client.multiplayer_url = url;
            assert_eq!(
                client.multiplayer_token(&token.result, &key).await.unwrap(),
                "SECRET-signed"
            );
            let request = task.await.unwrap();
            let body: serde_json::Value =
                serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(body, serde_json::json!({"publicKey": encoded}));
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("authorization: mctoken ")
            );
        }
        for (status, body) in [
            (401, "SECRET rejected"),
            (200, r#"{"result":{"signedToken":{"SECRET":"SECRET"}}}"#),
            (200, r#"{"result":{"signedToken":""}}"#),
        ] {
            let (url, task) = server(status, body).await;
            client.multiplayer_url = url;
            let error = client
                .multiplayer_token(&token.result, &[1, 2, 3])
                .await
                .unwrap_err();
            assert_safe(&error, &error);
            if status == 401 {
                assert!(
                    matches!(error, XblError::HttpStatus { status, .. } if status == reqwest::StatusCode::UNAUTHORIZED)
                );
            }
            task.await.unwrap();
        }
    }

    #[tokio::test]
    async fn login_reuses_ticket_until_refresh_margin() {
        let body = r#"{"data":{"SessionTicket":"SECRET-ticket","PlayFabId":"player","EntityToken":{"EntityToken":"SECRET-entity"}}}"#;
        let (url, task) = server(200, body).await;
        let mut client = PlayFabClient::new();
        client.login_url = url;
        assert_eq!(
            client.login("user", "SECRET-xsts").await.unwrap(),
            "SECRET-ticket"
        );
        task.await.unwrap();
        assert_eq!(
            client.login("user", "rotated-SECRET-xsts").await.unwrap(),
            "SECRET-ticket"
        );
        client.login_cache.lock().await.as_mut().unwrap().expires_at = std::time::Instant::now();
        let (url, task) = server(200, body).await;
        client.login_url = url;
        assert_eq!(
            client.login("user", "rotated-SECRET-xsts").await.unwrap(),
            "SECRET-ticket"
        );
        task.await.unwrap();
    }

    #[tokio::test]
    async fn invalidation_does_not_erase_a_newer_token() {
        let client = PlayFabClient::new();
        let token: SessionStartResponse = serde_json::from_str(&token_fixture()).unwrap();
        let rejected = token.result.authorization_header.clone();
        *client.service_cache.lock().await = Some(CachedService {
            device_id: "device".into(),
            game_version: "1.26.51".into(),
            ticket_fingerprint: Sha256::digest(b"SECRET-ticket").into(),
            token: token.result,
        });
        client.invalidate_service_token("old-token").await;
        assert!(client.service_cache.lock().await.is_some());
        client.invalidate_service_token(&rejected).await;
        assert!(client.service_cache.lock().await.is_none());
    }

    #[tokio::test]
    async fn service_token_renews_before_hard_expiry_and_caches_result() {
        let fixture = token_fixture();
        let (url, task) = server(200, &fixture).await;
        let mut client = PlayFabClient::new();
        client.renew_url = url;
        let mut token: SessionStartResponse = serde_json::from_str(&fixture).unwrap();
        token.result.decode_claims().unwrap();
        token.result.valid_until = chrono::Utc::now() + chrono::Duration::seconds(30);
        *client.service_cache.lock().await = Some(CachedService {
            device_id: "device".into(),
            game_version: "1.26.51".into(),
            ticket_fingerprint: Sha256::digest(b"SECRET-ticket").into(),
            token: token.result,
        });
        let result = client
            .service_token("device", "SECRET-ticket", "1.26.51")
            .await
            .unwrap();
        assert_eq!(
            result.player_messaging_id.to_string(),
            "00112233-4455-6677-8899-aabbccddeeff"
        );
        let request = task.await.unwrap();
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: mctoken ")
        );
        let body: serde_json::Value =
            serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["token"], "SECRET-ticket");
        assert!(body.get("device").is_none());
        // The one-response server has closed: a second HTTP request would fail.
        assert_eq!(
            client
                .service_token("device", "SECRET-ticket", "1.26.51")
                .await
                .unwrap()
                .authorization_header,
            result.authorization_header
        );
    }

    #[tokio::test]
    async fn rejected_renewal_starts_fresh_session() {
        let fixture = token_fixture();
        let (renew_url, renewal) = server(401, "SECRET").await;
        let (start_url, start) = server(200, &fixture).await;
        let mut client = PlayFabClient::new();
        client.renew_url = renew_url;
        client.session_url = start_url;
        let mut token: SessionStartResponse = serde_json::from_str(&fixture).unwrap();
        token.result.valid_until = chrono::Utc::now() + chrono::Duration::seconds(30);
        *client.service_cache.lock().await = Some(CachedService {
            device_id: "device".into(),
            game_version: "1.26.51".into(),
            ticket_fingerprint: Sha256::digest(b"SECRET-ticket").into(),
            token: token.result,
        });
        client
            .service_token("device", "SECRET-ticket", "1.26.51")
            .await
            .unwrap();
        renewal.await.unwrap();
        let request = start.await.unwrap();
        let body: serde_json::Value =
            serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["device"]["memory"], "17179869184");
        assert_eq!(body["device"]["capabilities"], serde_json::json!([]));
        assert!(body["device"].get("treatmentOverrides").is_none());
    }

    pub(crate) async fn server(
        status: u16,
        body: &str,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let body = body.to_owned();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = [0; 2048];
            loop {
                let n = stream.read(&mut buf).await.unwrap();
                assert_ne!(n, 0);
                request.extend_from_slice(&buf[..n]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let response = format!(
                "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            String::from_utf8(request).unwrap()
        });
        (url, task)
    }

    pub(crate) fn assert_safe(error: &impl std::fmt::Debug, display: &impl std::fmt::Display) {
        assert!(!format!("{error:?}").contains("SECRET"));
        assert!(!display.to_string().contains("SECRET"));
    }

    #[tokio::test]
    async fn login_checks_status_before_parsing_and_redacts_errors() {
        for (status, body) in [
            (401, "SECRET unauthorized"),
            (
                200,
                r#"{"data":{"SessionTicket":"SECRET","EntityToken":"SECRET"}}"#,
            ),
            (200, r#"{"data":null,"status":"SECRET"}"#),
        ] {
            let (url, task) = server(status, body).await;
            let error = PlayFabClient::new()
                .login_at(&url, "uhs", "SECRET")
                .await
                .unwrap_err();
            assert_safe(&error, &error);
            if status == 401 {
                assert!(
                    matches!(error, XblError::HttpStatus { status, .. } if status == reqwest::StatusCode::UNAUTHORIZED)
                );
            } else {
                assert!(matches!(error, XblError::Auth(_) | XblError::Json(_)));
            }
            task.await.unwrap();
        }
    }

    #[tokio::test]
    async fn login_transmits_go_request_and_returns_ticket() {
        let (url, task) = server(200, r#"{"data":{"SessionTicket":"SECRET-ticket","PlayFabId":"player","EntityToken":{"EntityToken":"SECRET-entity"}}}"#).await;
        assert_eq!(
            PlayFabClient::new()
                .login_at(&url, "hash", "SECRET-xsts")
                .await
                .unwrap(),
            "SECRET-ticket"
        );
        let request = task.await.unwrap();
        let body: serde_json::Value =
            serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "CreateAccount": true, "TitleId": "20CA2", "XboxToken": "XBL3.0 x=hash;SECRET-xsts"
            })
        );
    }

    #[tokio::test]
    async fn session_transmits_configured_version_and_redacts_errors() {
        let fixture = token_fixture();
        let (url, task) = server(200, &fixture).await;
        let token = PlayFabClient::new()
            .start_session_at(&url, "device", "SECRET-ticket", "1.26.51")
            .await
            .unwrap();
        assert_eq!(
            token.player_messaging_id.to_string(),
            "00112233-4455-6677-8899-aabbccddeeff"
        );
        let request = task.await.unwrap();
        let body: serde_json::Value =
            serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["device"]["gameVersion"], "1.26.51");
        assert_eq!(body["user"]["token"], "SECRET-ticket");
        for (status, body) in [
            (503, "SECRET unavailable"),
            (
                200,
                r#"{"result":{"authorizationHeader":{"SECRET":"SECRET"}}}"#,
            ),
            (200, r#"{"result":{"authorizationHeader":""}}"#),
            (
                200,
                r#"{"result":{"authorizationHeader":"MCToken SECRET","validUntil":"2100-01-01T00:00:00Z"}}"#,
            ),
        ] {
            let (url, task) = server(status, body).await;
            let error = PlayFabClient::new()
                .start_session_at(&url, "device", "SECRET-ticket", "1.26.51")
                .await
                .unwrap_err();
            assert_safe(&error, &error);
            if status == 503 {
                assert!(
                    matches!(error, XblError::HttpStatus { status, .. } if status == reqwest::StatusCode::SERVICE_UNAVAILABLE)
                );
            } else {
                assert!(matches!(error, XblError::Auth(_) | XblError::Json(_)));
            }
            task.await.unwrap();
        }
    }

    #[test]
    fn secret_response_debug_is_redacted() {
        let login: PlayFabLoginResponse = serde_json::from_str(r#"{"data":{"SessionTicket":"SECRET-ticket","PlayFabId":"player","EntityToken":{"EntityToken":"SECRET-entity"}},"status":"SECRET-status"}"#).unwrap();
        let session: SessionStartResponse = serde_json::from_str(r#"{"result":{"authorizationHeader":"SECRET-auth","validUntil":"2100-01-01T00:00:00Z"}}"#).unwrap();
        assert!(!format!("{login:?}").contains("SECRET"));
        assert!(!format!("{session:?}").contains("SECRET"));
        let request = PlayFabLoginRequest {
            create_account: true,
            title_id: "20CA2".into(),
            xbox_token: "SECRET".into(),
        };
        assert!(!format!("{request:?}").contains("SECRET"));
        let user = SessionUser {
            language: "en".into(),
            language_code: "en-US".into(),
            region_code: "US".into(),
            token: "SECRET".into(),
            token_type: "PlayFab".into(),
        };
        assert!(!format!("{user:?}").contains("SECRET"));
    }
}
