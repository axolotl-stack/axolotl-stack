//! Xbox authentication, ported from df-mc/go-xsapi xal/sisu/session.go.
//! SISU yields title/user/default-XSTS tokens; other relying parties use XSTS.

use super::{
    device_code::OAuthToken,
    nsal::NsalClient,
    signing::{SigningKeyPair, shared_nsal, shared_signing_key},
};
use crate::error::{XblError, XblResult, response_json};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use uuid::Uuid;

/// go-xsapi/xal/internal/token.go:ValidToken uses a one-minute safety margin.
const EXPIRY_BUFFER_SECS: u64 = 60;

#[derive(Clone, Serialize, Deserialize)]
pub struct XblToken {
    pub token: String,
    pub gamertag: String,
    pub xuid: String,
    pub user_hash: String,
    #[serde(default)]
    pub expires_at: u64,
}

impl std::fmt::Debug for XblToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XblToken")
            .field("token", &"[REDACTED]")
            .field("gamertag", &self.gamertag)
            .field("xuid", &self.xuid)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl XblToken {
    pub fn gamertag(&self) -> &str {
        &self.gamertag
    }
    pub fn xuid(&self) -> &str {
        &self.xuid
    }
    pub fn auth_header(&self) -> String {
        format!("XBL3.0 x={};{}", self.user_hash, self.token)
    }
    pub fn is_expired(&self) -> bool {
        self.token.is_empty() || now().saturating_add(EXPIRY_BUFFER_SECS) >= self.expires_at
    }
    pub fn is_valid(&self) -> bool {
        !self.is_expired()
    }
    pub fn seconds_until_expiry(&self) -> u64 {
        self.expires_at.saturating_sub(now())
    }
    pub fn parse_not_after(value: &str) -> Option<u64> {
        DateTime::parse_from_rfc3339(value)
            .ok()
            .and_then(|dt| dt.timestamp().try_into().ok())
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ServiceToken {
    token: String,
    not_after: String,
    #[serde(default)]
    display_claims: DisplayClaims,
}

impl ServiceToken {
    fn expires_at(&self) -> u64 {
        XblToken::parse_not_after(&self.not_after).unwrap_or(0)
    }
    fn valid(&self) -> bool {
        !self.token.is_empty() && now().saturating_add(EXPIRY_BUFFER_SECS) < self.expires_at()
    }
    fn validate(&self) -> XblResult<()> {
        if self.valid() {
            Ok(())
        } else {
            Err(XblError::Auth("Invalid or expired service token".into()))
        }
    }
    fn xbox_token(&self) -> XblResult<XblToken> {
        self.validate()?;
        let user = self
            .display_claims
            .xui
            .first()
            .ok_or_else(|| XblError::Auth("Missing Xbox user claims".into()))?;
        if user.uhs.is_empty() {
            return Err(XblError::Auth("Missing Xbox user hash".into()));
        }
        Ok(XblToken {
            token: self.token.clone(),
            gamertag: user.gtg.clone(),
            xuid: user.xid.clone(),
            user_hash: user.uhs.clone(),
            expires_at: self.expires_at(),
        })
    }
}

#[derive(Clone, Default, Deserialize)]
struct DisplayClaims {
    #[serde(default)]
    xui: Vec<UserInfo>,
}
#[derive(Clone, Deserialize)]
struct UserInfo {
    #[serde(default)]
    gtg: String,
    #[serde(default)]
    xid: String,
    #[serde(default)]
    uhs: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AuthorizationResponse {
    title_token: ServiceToken,
    user_token: ServiceToken,
    authorization_token: ServiceToken,
}

#[derive(Default)]
struct SessionState {
    device: Option<ServiceToken>,
    authorization: Option<AuthorizationResponse>,
    xsts: HashMap<String, XblToken>,
}

struct AuthEndpoints {
    device: std::borrow::Cow<'static, str>,
    sisu: std::borrow::Cow<'static, str>,
    xsts: std::borrow::Cow<'static, str>,
}

const AUTH_ENDPOINTS: AuthEndpoints = AuthEndpoints {
    device: std::borrow::Cow::Borrowed(super::DEVICE_AUTH_URL),
    sisu: std::borrow::Cow::Borrowed(super::SISU_AUTHORIZE_URL),
    xsts: std::borrow::Cow::Borrowed(super::XSTS_AUTHORIZE_URL),
};

/// Concurrent callers serialize acquisition, sharing device, title, user and RP tokens.
/// See go-xsapi/xal/sisu/session.go:Session, authorize, requestXSTS.
pub struct XblTokenClient {
    client: reqwest::Client,
    signing_key: Arc<SigningKeyPair>,
    nsal: Arc<NsalClient>,
    endpoints: AuthEndpoints,
    state: Mutex<SessionState>,
}

impl XblTokenClient {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("valid HTTP client configuration");
        Self {
            nsal: shared_nsal(),
            client,
            signing_key: shared_signing_key(),
            state: Mutex::new(SessionState::default()),
            endpoints: AUTH_ENDPOINTS,
        }
    }

    /// Resolve title-specific relying parties instead of guessing service scopes.
    /// go-xsapi/xal/nsal/resolver.go:Resolver.Resolve; go-playfab/xbox.go:LoginWithXbox.
    pub async fn get_xbl_token_for_url(
        &self,
        oauth: &OAuthToken,
        url: &str,
    ) -> XblResult<XblToken> {
        let url = url::Url::parse(url).map_err(|_| XblError::Auth("Invalid service URL".into()))?;
        let xbox = self
            .get_xbl_token(oauth, Some(relying_party::XBOX_LIVE))
            .await?;
        let resolved = self.nsal.resolve(&url, &xbox, &self.signing_key).await?;
        self.get_xbl_token(oauth, Some(&resolved.relying_party))
            .await
    }

    pub async fn get_xbl_token(
        &self,
        oauth: &OAuthToken,
        relying_party: Option<&str>,
    ) -> XblResult<XblToken> {
        let rp = relying_party.unwrap_or(relying_party::XBOX_LIVE);
        let mut state = self.state.lock().await;
        if let Some(token) = state.xsts.get(rp).filter(|t| t.is_valid()) {
            return Ok(token.clone());
        }
        let device = match state.device.as_ref().filter(|t| t.valid()) {
            Some(token) => token.clone(),
            None => {
                let token = self.request_device_token().await?;
                state.device = Some(token.clone());
                token
            }
        };
        if state.authorization.as_ref().is_none_or(|r| {
            !r.title_token.valid()
                || !r.user_token.valid()
                || (rp == relying_party::XBOX_LIVE && !r.authorization_token.valid())
        }) {
            state.authorization = Some(self.authorize(oauth, &device).await?);
        }
        let response = state
            .authorization
            .as_ref()
            .ok_or_else(|| XblError::Auth("Missing authorization state".into()))?;
        let token = if rp == relying_party::XBOX_LIVE {
            response.authorization_token.xbox_token()?
        } else {
            self.request_xsts(rp, &device, response)
                .await?
                .xbox_token()?
        };
        state.xsts.insert(rp.into(), token.clone());
        Ok(token)
    }

    /// Invalidate only a rejected token, not a newer concurrent replacement.
    /// go-xsapi/xal/sisu/session.go:Session.InvalidateXSTSToken.
    pub async fn invalidate_token(&self, relying_party: &str, rejected: &XblToken) {
        let mut state = self.state.lock().await;
        if state
            .xsts
            .get(relying_party)
            .is_some_and(|current| current.token == rejected.token)
        {
            state.xsts.remove(relying_party);
        }
        if state
            .authorization
            .as_ref()
            .is_some_and(|r| r.authorization_token.token == rejected.token)
        {
            state.authorization = None;
        }
    }
    /// Forget authorization and scoped tokens while retaining the device/proof key.
    pub async fn clear_cached_tokens(&self) {
        let mut state = self.state.lock().await;
        state.xsts.clear();
        state.authorization = None;
    }

    async fn request_device_token(&self) -> XblResult<ServiceToken> {
        // go-xsapi/xal/xasd/authenticate.go:Authenticate and example_test.go:MinecraftAndroid.
        let body = serde_json::json!({
            "Properties": { "AuthMethod":"ProofOfPossession", "DeviceType":"Android",
                "Id":format!("{{{}}}", Uuid::new_v4()), "ProofKey":self.signing_key.proof_key(), "Version":"13" },
            "RelyingParty":"http://auth.xboxlive.com", "TokenType":"JWT"
        });
        let token: ServiceToken = self
            .post(
                &self.endpoints.device,
                body,
                None,
                "Xbox device authentication",
            )
            .await?;
        token.validate()?;
        Ok(token)
    }

    async fn authorize(
        &self,
        oauth: &OAuthToken,
        device: &ServiceToken,
    ) -> XblResult<AuthorizationResponse> {
        // go-xsapi/xal/sisu/session.go:Session.authorize always requests default RP.
        let body = serde_json::json!({
            "AccessToken":format!("t={}",oauth.access_token), "AppId":super::MINECRAFT_CLIENT_ID,
            "DeviceToken":device.token, "Sandbox":"RETAIL", "UseModernGamertag":true,
            "SiteName":"user.auth.xboxlive.com", "ProofKey":self.signing_key.proof_key(),
            "RelyingParty":relying_party::XBOX_LIVE
        });
        let url = url::Url::parse(&self.endpoints.sisu)
            .map_err(|_| XblError::Auth("Invalid SISU URL".into()))?;
        let policy = self.nsal.resolve_default(&url).await?.policy;
        let response: AuthorizationResponse = self
            .post(
                &self.endpoints.sisu,
                body,
                Some(&policy),
                "Xbox SISU authorization",
            )
            .await?;
        response.title_token.validate()?;
        response.user_token.validate()?;
        response.authorization_token.validate()?;
        Ok(response)
    }

    async fn request_xsts(
        &self,
        rp: &str,
        device: &ServiceToken,
        response: &AuthorizationResponse,
    ) -> XblResult<ServiceToken> {
        // go-xsapi/xal/xsts/authorize.go:Authorize.
        let body = serde_json::json!({"Properties":{"SandboxId":"RETAIL","DeviceToken":device.token,
            "TitleToken":response.title_token.token,"UserTokens":[response.user_token.token]},
            "RelyingParty":rp,"TokenType":"JWT"});
        let token: ServiceToken = self
            .post(&self.endpoints.xsts, body, None, "Xbox XSTS authorization")
            .await?;
        token.validate()?;
        Ok(token)
    }

    async fn post<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: serde_json::Value,
        policy: Option<&super::nsal::SignaturePolicy>,
        operation: &'static str,
    ) -> XblResult<T> {
        // Go json.Encoder includes a trailing newline, and signs those exact bytes.
        let mut bytes = serde_json::to_vec(&body)?;
        bytes.push(b'\n');
        let mut request = self
            .client
            .post(url)
            .header("Content-Type", "application/json")
            .header("User-Agent", super::XAL_USER_AGENT)
            .header("x-xbl-contract-version", "1")
            .body(bytes)
            .build()?;
        super::signing::sign_http_request(
            &mut request,
            &self.signing_key,
            policy.unwrap_or(&super::nsal::AUTH_POLICY),
        )?;
        let response = self.client.execute(request).await?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(XblError::HttpStatus {
                status: response.status(),
                operation,
            });
        }
        response_json(response, operation).await
    }
}

impl Default for XblTokenClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Values defined by go-xsapi/xal/sisu/session.go:defaultRelyingParty and
/// go-xsapi/xal/nsal/resolver.go:Resolver.Resolve. Title endpoints resolve dynamically.
pub mod relying_party {
    pub const XBOX_LIVE: &str = "http://xboxlive.com";
    pub const JAVA_EDITION: &str = "rp://api.minecraftservices.com/";
    pub const BEDROCK_MULTIPLAYER: &str = "https://multiplayer.minecraft.net/";
    pub const POCKET_REALMS: &str = "https://pocket.realms.minecraft.net/";
    pub const PLAYFAB: &str = "http://playfab.xboxlive.com/";
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_service_tokens_are_not_reused() {
        for (token, expires_at) in [
            ("", u64::MAX),
            ("credential", 0),
            ("credential", now() + 59),
        ] {
            let token = XblToken {
                token: token.into(),
                expires_at,
                gamertag: String::new(),
                xuid: String::new(),
                user_hash: String::new(),
            };
            assert!(token.is_expired());
        }
    }
    #[test]
    fn pre_epoch_not_after_is_invalid() {
        assert_eq!(XblToken::parse_not_after("1960-01-01T00:00:00Z"), None);
    }
    #[test]
    fn malformed_claims_do_not_panic_or_leak() {
        let token: ServiceToken = serde_json::from_value(serde_json::json!({"Token":"secret",
            "NotAfter":"2099-01-01T00:00:00Z","DisplayClaims":{"xui":[]}}))
        .unwrap();
        let error = token.xbox_token().unwrap_err();
        assert!(!format!("{error} {error:?}").contains("secret"));
    }

    use serde_json::{Value, json};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct CapturedRequest {
        method: String,
        path: String,
        headers: HashMap<String, String>,
        body: Vec<u8>,
    }

    async fn auth_server(
        responses: Vec<(u16, Value)>,
    ) -> (String, tokio::task::JoinHandle<Vec<CapturedRequest>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut captured = Vec::new();
            for (status, response_body) in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let end = loop {
                    if let Some(position) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                        break position + 4;
                    }
                    let mut chunk = [0; 2048];
                    let count = stream.read(&mut chunk).await.unwrap();
                    assert_ne!(count, 0);
                    bytes.extend_from_slice(&chunk[..count]);
                };
                let head = std::str::from_utf8(&bytes[..end]).unwrap();
                let mut lines = head.split("\r\n");
                let mut first = lines.next().unwrap().split_whitespace();
                let method = first.next().unwrap().to_owned();
                let path = first.next().unwrap().to_owned();
                let headers: HashMap<String, String> = lines
                    .filter_map(|line| {
                        line.split_once(':').map(|(name, value)| {
                            (name.to_ascii_lowercase(), value.trim().to_owned())
                        })
                    })
                    .collect();
                let length = headers
                    .get("content-length")
                    .map(|v| v.parse::<usize>().unwrap())
                    .unwrap_or(0);
                while bytes.len() < end + length {
                    let mut chunk = [0; 2048];
                    let count = stream.read(&mut chunk).await.unwrap();
                    assert_ne!(count, 0);
                    bytes.extend_from_slice(&chunk[..count]);
                }
                captured.push(CapturedRequest {
                    method,
                    path,
                    headers,
                    body: bytes[end..end + length].to_vec(),
                });
                let body = response_body.to_string();
                stream.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            }
            captured
        });
        (base, task)
    }

    fn test_client(base: &str) -> XblTokenClient {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .unwrap();
        XblTokenClient {
            nsal: Arc::new(NsalClient::for_test(client.clone(), base.into())),
            client,
            signing_key: Arc::new(SigningKeyPair::generate()),
            state: Mutex::new(SessionState::default()),
            endpoints: AuthEndpoints {
                device: format!("{base}/device").into(),
                sisu: format!("{base}/sisu").into(),
                xsts: format!("{base}/xsts").into(),
            },
        }
    }

    fn oauth() -> OAuthToken {
        OAuthToken {
            access_token: "oauth-secret".into(),
            token_type: "Bearer".into(),
            refresh_token: String::new(),
            expires_in: 3600,
            user_id: None,
        }
    }

    fn service_token(value: &str) -> Value {
        json!({"Token":value,"NotAfter":"2099-01-01T00:00:00Z",
            "DisplayClaims":{"xui":[{"uhs":"user-hash","gtg":"Player","xid":"123"}]}})
    }

    fn sisu_response() -> Value {
        json!({"TitleToken":service_token("title-secret"),"UserToken":service_token("user-secret"),
            "AuthorizationToken":service_token("xbox-secret")})
    }

    fn nsal_response() -> Value {
        json!({"EndPoints":[{"Protocol":"http","Host":"127.0.0.1","HostType":"fqdn",
            "Path":"/sisu","RelyingParty":"http://xboxlive.com","SignaturePolicyIndex":0}],
            "SignaturePolicies":[{"Version":2,"MaxBodyBytes":3,
                "ExtraHeaders":["Content-Type","User-Agent","x-xbl-contract-version"]}]})
    }

    fn verify_signature(request: &CapturedRequest, proof: &Value, version: u32, max_body: usize) {
        use base64::{
            Engine,
            engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
        };
        use p256::ecdsa::{Signature, VerifyingKey, signature::DigestVerifier};
        use sha2::{Digest, Sha256};
        let mut public_key = vec![4];
        public_key.extend(
            URL_SAFE_NO_PAD
                .decode(proof["x"].as_str().unwrap())
                .unwrap(),
        );
        public_key.extend(
            URL_SAFE_NO_PAD
                .decode(proof["y"].as_str().unwrap())
                .unwrap(),
        );
        let key = VerifyingKey::from_sec1_bytes(&public_key).unwrap();
        let signature = STANDARD.decode(&request.headers["signature"]).unwrap();
        assert_eq!(signature.len(), 76);
        assert_eq!(&signature[..4], &version.to_be_bytes());
        let mut hash = Sha256::new();
        hash.update(&signature[..4]);
        hash.update([0]);
        hash.update(&signature[4..12]);
        hash.update([0]);
        hash.update(request.method.as_bytes());
        hash.update([0]);
        hash.update(request.path.as_bytes());
        hash.update([0]);
        hash.update(
            request
                .headers
                .get("authorization")
                .map(String::as_bytes)
                .unwrap_or(b""),
        );
        hash.update([0]);
        if version == 2 {
            for name in ["content-type", "user-agent", "x-xbl-contract-version"] {
                hash.update(
                    request
                        .headers
                        .get(name)
                        .map(String::as_bytes)
                        .unwrap_or(b""),
                );
                hash.update([0]);
            }
        }
        hash.update(
            &request.body[..if max_body == 0 {
                request.body.len()
            } else {
                request.body.len().min(max_body)
            }],
        );
        hash.update([0]);
        key.verify_digest(hash, &Signature::from_slice(&signature[12..]).unwrap())
            .unwrap();
    }

    #[tokio::test]
    async fn service_signature_covers_actual_outgoing_extra_headers() {
        let (base, captured) = auth_server(vec![(200, json!({}))]).await;
        let client = test_client(&base);
        let mut request = client
            .client
            .post(format!("{base}/service"))
            .header("Content-Type", "application/json")
            .header("User-Agent", "service-agent")
            .header("x-xbl-contract-version", "107")
            .header("Authorization", "XBL3.0 x=hash;secret")
            .body(b"abcdef".to_vec())
            .build()
            .unwrap();
        let policy = super::super::nsal::SignaturePolicy {
            version: 2,
            max_body_bytes: 3,
            extra_headers: vec![
                "Content-Type".into(),
                "User-Agent".into(),
                "x-xbl-contract-version".into(),
            ],
        };
        super::super::signing::sign_http_request(&mut request, &client.signing_key, &policy)
            .unwrap();
        client.client.execute(request).await.unwrap();
        let requests = captured.await.unwrap();
        verify_signature(&requests[0], &client.signing_key.proof_key(), 2, 3);
    }

    #[tokio::test]
    async fn acquisition_http_smoke_uses_nsal_sisu_and_scoped_xsts_with_one_proof_key() {
        let (base, requests) = auth_server(vec![
            (200, service_token("device-secret")),
            (200, nsal_response()),
            (200, sisu_response()),
            (200, service_token("scoped-secret")),
        ])
        .await;
        let client = test_client(&base);
        let xbox = client.get_xbl_token(&oauth(), None).await.unwrap();
        assert_eq!(xbox.token, "xbox-secret");
        let scoped = client
            .get_xbl_token(&oauth(), Some("rp://test-service"))
            .await
            .unwrap();
        assert_eq!(scoped.token, "scoped-secret");
        assert_eq!(scoped.user_hash, "user-hash");
        assert_eq!(scoped.xuid, "123");
        // The listener is dropped after four responses: an accidental extra
        // acquisition fails rather than being hidden by a permissive mock.
        assert_eq!(
            client
                .get_xbl_token(&oauth(), Some("rp://test-service"))
                .await
                .unwrap()
                .token,
            scoped.token
        );
        assert_eq!(
            client.get_xbl_token(&oauth(), None).await.unwrap().token,
            xbox.token
        );
        let requests = requests.await.unwrap();
        assert_eq!(
            requests.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
            [
                "/device",
                "/titles/default/endpoints?type=1",
                "/sisu",
                "/xsts"
            ]
        );
        let device: Value = serde_json::from_slice(&requests[0].body).unwrap();
        let sisu: Value = serde_json::from_slice(&requests[2].body).unwrap();
        let xsts: Value = serde_json::from_slice(&requests[3].body).unwrap();
        let proof = &device["Properties"]["ProofKey"];
        assert_eq!(proof, &client.signing_key.proof_key());
        assert_eq!(&sisu["ProofKey"], proof);
        assert_eq!(device["Properties"]["AuthMethod"], "ProofOfPossession");
        assert_eq!(device["Properties"]["DeviceType"], "Android");
        assert_eq!(device["Properties"]["Version"], "13");
        assert_eq!(device["RelyingParty"], "http://auth.xboxlive.com");
        assert_eq!(device["TokenType"], "JWT");
        assert!(
            Uuid::parse_str(
                device["Properties"]["Id"]
                    .as_str()
                    .unwrap()
                    .trim_matches(['{', '}'])
            )
            .is_ok()
        );
        assert_eq!(
            sisu,
            json!({"AccessToken":"t=oauth-secret","AppId":super::super::MINECRAFT_CLIENT_ID,
            "DeviceToken":"device-secret","Sandbox":"RETAIL","UseModernGamertag":true,
            "SiteName":"user.auth.xboxlive.com","ProofKey":proof,"RelyingParty":"http://xboxlive.com"})
        );
        assert_eq!(
            xsts,
            json!({"Properties":{"SandboxId":"RETAIL","DeviceToken":"device-secret",
            "TitleToken":"title-secret","UserTokens":["user-secret"]},"RelyingParty":"rp://test-service","TokenType":"JWT"})
        );
        assert!(!requests[1].headers.contains_key("authorization"));
        assert!(!requests[1].headers.contains_key("signature"));
        for (index, version, limit) in [(0, 1, 0), (2, 2, 3), (3, 1, 0)] {
            assert_eq!(requests[index].method, "POST");
            assert_eq!(requests[index].headers["content-type"], "application/json");
            assert_eq!(requests[index].headers["x-xbl-contract-version"], "1");
            assert_eq!(
                requests[index].headers["user-agent"],
                super::super::XAL_USER_AGENT
            );
            assert!(requests[index].body.ends_with(b"\n"));
            verify_signature(&requests[index], proof, version, limit);
        }
    }

    #[tokio::test]
    async fn acquisition_http_rejects_invalid_or_missing_expiry_at_each_stage() {
        for stage in ["device", "title", "user", "authorization", "xsts"] {
            for expiry in [Some(json!("invalid-date-secret")), None] {
                let mut bad = service_token("invalid-token-secret");
                match expiry {
                    Some(value) => {
                        bad["NotAfter"] = value;
                    }
                    None => {
                        bad.as_object_mut().unwrap().remove("NotAfter");
                    }
                }
                let mut responses = vec![
                    (200, service_token("device-secret")),
                    (200, nsal_response()),
                ];
                let rp = if stage == "xsts" {
                    Some("rp://test-service")
                } else {
                    None
                };
                if stage == "device" {
                    responses = vec![(200, bad)];
                } else {
                    let mut sisu = sisu_response();
                    match stage {
                        "title" => sisu["TitleToken"] = bad.clone(),
                        "user" => sisu["UserToken"] = bad.clone(),
                        "authorization" => sisu["AuthorizationToken"] = bad.clone(),
                        _ => {}
                    }
                    responses.push((200, sisu));
                    if stage == "xsts" {
                        responses.push((200, bad));
                    }
                }
                let (base, requests) = auth_server(responses).await;
                let error = test_client(&base)
                    .get_xbl_token(&oauth(), rp)
                    .await
                    .unwrap_err();
                let message = format!("{error} {error:?}");
                assert!(!message.contains("invalid-date-secret"));
                assert!(!message.contains("invalid-token-secret"));
                requests.await.unwrap();
            }
        }
    }

    #[tokio::test]
    async fn acquisition_http_rejects_missing_user_claims() {
        for scoped in [false, true] {
            for claims in [json!({}), json!({"xui":[]}), json!({"xui":[{}]})] {
                let mut bad = service_token("claim-token-secret");
                bad["DisplayClaims"] = claims;
                let mut sisu = sisu_response();
                if !scoped {
                    sisu["AuthorizationToken"] = bad.clone();
                }
                let mut responses = vec![
                    (200, service_token("device-secret")),
                    (200, nsal_response()),
                    (200, sisu),
                ];
                if scoped {
                    responses.push((200, bad));
                }
                let (base, requests) = auth_server(responses).await;
                let error = test_client(&base)
                    .get_xbl_token(
                        &oauth(),
                        if scoped {
                            Some("rp://test-service")
                        } else {
                            None
                        },
                    )
                    .await
                    .unwrap_err();
                assert!(!format!("{error} {error:?}").contains("claim-token-secret"));
                requests.await.unwrap();
            }
        }
    }

    #[tokio::test]
    async fn acquisition_http_status_errors_never_expose_remote_secrets() {
        let successful = [
            service_token("device-secret"),
            nsal_response(),
            sisu_response(),
            service_token("scoped-secret"),
        ];
        for failure_index in 0..4 {
            let mut responses: Vec<_> = successful[..failure_index]
                .iter()
                .cloned()
                .map(|body| (200, body))
                .collect();
            responses.push((403, json!({"error":"remote-secret","Token":"token-secret"})));
            let (base, requests) = auth_server(responses).await;
            let error = test_client(&base)
                .get_xbl_token(&oauth(), Some("rp://test-service"))
                .await
                .unwrap_err();
            assert_eq!(error.status(), Some(reqwest::StatusCode::FORBIDDEN));
            let rendered = format!("{error} {error:?}");
            assert!(!rendered.contains("remote-secret"));
            assert!(!rendered.contains("token-secret"));
            requests.await.unwrap();
        }
    }
}
