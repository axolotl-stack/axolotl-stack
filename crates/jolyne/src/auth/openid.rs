//! Minecraft multiplayer-token (OpenID) verification.
//!
//! [`OpenIdVerifier`] is an immutable, synchronous key-set snapshot.
//! [`OpenIdProvider`] caches one snapshot and refreshes it from a
//! [`KeySetSource`] without serializing logins:
//!
//! - Validation clones an `Arc` snapshot under a short read lock and verifies
//!   without holding any lock. No lock is held across HTTP.
//! - Refreshes are single-flight: concurrent callers that need fresh keys wait
//!   for the one in-flight fetch and share its outcome, success or failure.
//! - An unknown `kid` triggers a refresh at most once per
//!   [`RefreshPolicy::unknown_kid_interval`] (gophertunnel
//!   `minecraft/service/jwks.go:refreshingKeySet`, 30 minutes).
//! - Snapshots older than [`RefreshPolicy::max_age`] are revalidated by one
//!   caller while others keep using the cached keys. If the refresh fails,
//!   cached keys keep serving until [`RefreshPolicy::max_stale`], then logins
//!   fail closed.
//! - Failed fetches are not retried for [`RefreshPolicy::retry_backoff`], and
//!   every fetch is bounded by [`RefreshPolicy::fetch_timeout`].

use crate::auth::client_data::ClientData;
use crate::auth::types::{AuthenticatedLogin, OpenIdClaims, Trust, ValidatedIdentity};
use crate::auth::util::key_from_base64;
use crate::error::AuthError;
use futures::future::BoxFuture;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use parking_lot::RwLock;
use serde::{Deserialize, de::DeserializeOwned};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::time::Instant;

const MOJANG_AUDIENCE: &str = "api://auth-minecraft-services/multiplayer";
const CLOCK_SKEW: u64 = 60;
const DISCOVERY_URL: &str =
    "https://client.discovery.minecraft-services.net/api/v1.0/discovery/MinecraftPE/builds/";

#[derive(Deserialize)]
struct DiscoveryEnvelope {
    result: DiscoveryResult,
}
#[derive(Deserialize)]
struct DiscoveryResult {
    #[serde(rename = "serviceEnvironments")]
    service_environments: Environments,
}
#[derive(Deserialize)]
struct Environments {
    auth: AuthEnvironments,
}
#[derive(Deserialize)]
struct AuthEnvironments {
    prod: AuthEnvironment,
}
#[derive(Deserialize)]
struct AuthEnvironment {
    issuer: String,
}
#[derive(Deserialize)]
struct Configuration {
    issuer: String,
    jwks_uri: String,
    #[serde(default = "default_algorithms")]
    id_token_signing_alg_values_supported: Vec<Algorithm>,
}
fn default_algorithms() -> Vec<Algorithm> {
    vec![Algorithm::RS256]
}
#[derive(Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}
#[derive(Deserialize)]
struct Jwk {
    kid: Option<String>,
    kty: String,
    #[serde(rename = "use")]
    usage: Option<String>,
    alg: Option<Algorithm>,
    n: Option<String>,
    e: Option<String>,
    crv: Option<String>,
    x: Option<String>,
    y: Option<String>,
}
struct TrustedKey {
    kid: Option<String>,
    alg: Algorithm,
    key: DecodingKey,
}

/// A trusted provider snapshot, never derived from a login JWT header.
/// Production obtains this through Minecraft discovery; callers may inject a local
/// JWKS for deterministic tests or a separately authenticated provider source.
/// Mirrors gophertunnel minecraft/service/token.go:VerifierContext and
/// minecraft/service/jwks.go:newRefreshingKeySet (RS256 default, published algorithms).
pub struct OpenIdVerifier {
    issuer: String,
    keys: Vec<TrustedKey>,
}

impl OpenIdVerifier {
    pub fn from_jwks(
        issuer: &str,
        algorithms: &[Algorithm],
        jwks_json: &str,
    ) -> Result<Self, AuthError> {
        let jwks: Jwks = serde_json::from_str(jwks_json).map_err(|_| AuthError::InvalidJson)?;
        let mut keys = Vec::new();
        for jwk in jwks.keys {
            if jwk.usage.as_deref().is_some_and(|usage| usage != "sig") {
                continue;
            }
            for &alg in algorithms {
                if jwk.alg.is_some_and(|key_alg| key_alg != alg) {
                    continue;
                }
                let key = match alg {
                    Algorithm::RS256 if jwk.kty == "RSA" => DecodingKey::from_rsa_components(
                        jwk.n.as_deref().ok_or(AuthError::MissingIdentityKey)?,
                        jwk.e.as_deref().ok_or(AuthError::MissingIdentityKey)?,
                    ),
                    Algorithm::ES384 if jwk.kty == "EC" && jwk.crv.as_deref() == Some("P-384") => {
                        DecodingKey::from_ec_components(
                            jwk.x.as_deref().ok_or(AuthError::MissingIdentityKey)?,
                            jwk.y.as_deref().ok_or(AuthError::MissingIdentityKey)?,
                        )
                    }
                    _ => continue,
                }
                .map_err(jwt_error)?;
                keys.push(TrustedKey {
                    kid: jwk.kid.clone(),
                    alg,
                    key,
                });
            }
        }
        if issuer.is_empty() || keys.is_empty() {
            return Err(AuthError::BadSignature(
                "No trusted signing keys or issuer".into(),
            ));
        }
        Ok(Self {
            issuer: issuer.into(),
            keys,
        })
    }

    /// Whether a key with this `kid` is part of the snapshot.
    pub fn knows_kid(&self, kid: &str) -> bool {
        self.keys.iter().any(|key| key.kid.as_deref() == Some(kid))
    }

    /// Verifies the provider token and returns its claims.
    /// `iss`, `aud`, `exp` and `nbf` are mandatory; skew is at most 60 seconds.
    /// The JWT `alg` must equal the trusted key's algorithm and, when the token
    /// names a `kid`, only that key is tried.
    pub fn verify_token(&self, token: &str) -> Result<OpenIdClaims, AuthError> {
        let header = decode_header(token).map_err(|e| AuthError::InvalidHeader(e.to_string()))?;
        let mut validation = Validation::new(header.alg);
        validation.set_required_spec_claims(&["iss", "aud", "exp", "nbf"]);
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[MOJANG_AUDIENCE]);
        validation.validate_nbf = true;
        validation.leeway = CLOCK_SKEW;
        let mut last_error = AuthError::BadSignature("No matching trusted provider key".into());
        for key in &self.keys {
            if header.alg != key.alg
                || header
                    .kid
                    .as_ref()
                    .is_some_and(|kid| key.kid.as_ref() != Some(kid))
            {
                continue;
            }
            match decode::<OpenIdClaims>(token, &key.key, &validation) {
                Ok(data) => return Ok(data.claims),
                Err(err) => last_error = jwt_error(err),
            }
        }
        Err(last_error)
    }

    /// Verifies the provider token, derives the authenticated identity and
    /// binds the client data to the token's `cpk`.
    pub fn validate(
        &self,
        token: &str,
        client_data_jwt: &str,
    ) -> Result<AuthenticatedLogin, AuthError> {
        login_from_claims(
            self.verify_token(token)?,
            client_data_jwt,
            Trust::Authenticated,
        )
    }
}

pub(crate) fn jwt_error(err: jsonwebtoken::errors::Error) -> AuthError {
    use jsonwebtoken::errors::ErrorKind;
    match err.kind() {
        ErrorKind::ExpiredSignature | ErrorKind::ImmatureSignature => AuthError::TemporalValidation,
        ErrorKind::InvalidIssuer => AuthError::InvalidIssuer(err.to_string()),
        ErrorKind::InvalidAudience => AuthError::InvalidAudience(err.to_string()),
        _ => AuthError::BadSignature(err.to_string()),
    }
}

/// Client data is client-controlled, but must prove possession of the identity key.
/// Like gophertunnel minecraft/protocol/login/request.go:parseFullClaim, it uses
/// ES384 and does not require exp/aud (the client-data payload has neither).
/// The decoded data is then validated like gophertunnel `ClientData.Validate`.
pub(crate) fn verify_client_data(
    identity_public_key: &str,
    jwt: &str,
) -> Result<ClientData, AuthError> {
    let mut validation = Validation::new(Algorithm::ES384);
    validation.required_spec_claims.clear();
    validation.validate_exp = false;
    validation.validate_aud = false;
    let key = key_from_base64(identity_public_key)?;
    let mut data = decode::<ClientData>(jwt, &key, &validation)
        .map_err(jwt_error)?
        .claims;
    if data
        .identity_public_key
        .as_deref()
        .is_some_and(|echo| echo != identity_public_key)
    {
        return Err(AuthError::BadSignature(
            "Client identity key mismatch".into(),
        ));
    }
    data.normalize_server_address();
    data.validate()?;
    Ok(data)
}

/// Builds the identity from token claims and verifies the client data against
/// the token's `cpk`.
pub(crate) fn login_from_claims(
    claims: OpenIdClaims,
    client_data_jwt: &str,
    trust: Trust,
) -> Result<AuthenticatedLogin, AuthError> {
    let identity_public_key = claims
        .identity_public_key
        .clone()
        .filter(|key| !key.is_empty())
        .ok_or(AuthError::MissingIdentityKey)?;
    let identity = ValidatedIdentity::new(claims.into(), identity_public_key, trust)?;
    let client_data = verify_client_data(identity.identity_public_key(), client_data_jwt)?;
    Ok(AuthenticatedLogin {
        identity,
        client_data,
    })
}

/// Produces trusted key-set snapshots. Implementations must only use trust
/// anchors outside the login request (never JWT header URLs or keys).
pub trait KeySetSource: Send + Sync {
    fn fetch(&self) -> BoxFuture<'_, Result<OpenIdVerifier, AuthError>>;
}

/// Refresh timing for [`OpenIdProvider`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshPolicy {
    /// Minimum age of the snapshot before an unknown `kid` may trigger a
    /// refresh (gophertunnel `KeyRefreshInterval`, 30 minutes).
    pub unknown_kid_interval: Duration,
    /// Age after which one caller revalidates the snapshot in the background of
    /// other logins.
    pub max_age: Duration,
    /// Age after which a snapshot is no longer used when refreshing fails.
    pub max_stale: Duration,
    /// Minimum delay after a failed fetch before another fetch is attempted.
    pub retry_backoff: Duration,
    /// Upper bound for one fetch (all HTTP requests of a refresh).
    pub fetch_timeout: Duration,
}

impl Default for RefreshPolicy {
    fn default() -> Self {
        Self {
            unknown_kid_interval: Duration::from_secs(30 * 60),
            max_age: Duration::from_secs(60 * 60),
            max_stale: Duration::from_secs(24 * 60 * 60),
            retry_backoff: Duration::from_secs(30),
            fetch_timeout: Duration::from_secs(30),
        }
    }
}

struct Snapshot {
    verifier: OpenIdVerifier,
    fetched_at: Instant,
}

#[derive(Default)]
struct CacheState {
    snapshot: Option<Arc<Snapshot>>,
    /// Number of completed fetch attempts; waiters use it to share an outcome.
    attempts: u64,
    last_attempt: Option<Instant>,
    /// Error of the most recent attempt, cleared on success.
    last_error: Option<AuthError>,
}

impl CacheState {
    fn retry_allowed(&self, now: Instant, backoff: Duration) -> bool {
        self.last_error.is_none()
            || self
                .last_attempt
                .is_none_or(|at| now.saturating_duration_since(at) >= backoff)
    }
}

/// Cached, concurrently usable OpenID key set (see module docs).
pub struct OpenIdProvider {
    source: Box<dyn KeySetSource>,
    policy: RefreshPolicy,
    state: RwLock<CacheState>,
    /// Single-flight guard; held only by the caller performing a fetch.
    flight: tokio::sync::Mutex<()>,
}

impl OpenIdProvider {
    pub fn new(source: impl KeySetSource + 'static, policy: RefreshPolicy) -> Self {
        Self {
            source: Box::new(source),
            policy,
            state: RwLock::new(CacheState::default()),
            flight: tokio::sync::Mutex::new(()),
        }
    }

    /// The process-wide provider bootstrapped from Minecraft build discovery.
    pub fn minecraft() -> &'static OpenIdProvider {
        static PROVIDER: LazyLock<OpenIdProvider> =
            LazyLock::new(|| OpenIdProvider::new(MinecraftDiscovery, RefreshPolicy::default()));
        &PROVIDER
    }

    /// Verifies a provider token and the client data it binds.
    pub async fn validate(
        &self,
        token: &str,
        client_data_jwt: &str,
    ) -> Result<AuthenticatedLogin, AuthError> {
        let kid = decode_header(token)
            .map_err(|e| AuthError::InvalidHeader(e.to_string()))?
            .kid;
        let mut snapshot = self.current_keys().await?;
        if let Some(kid) = kid.as_deref()
            && !snapshot.verifier.knows_kid(kid)
            && let Some(refreshed) = self.refresh_for_unknown_kid(&snapshot).await?
        {
            snapshot = refreshed;
        }
        snapshot.verifier.validate(token, client_data_jwt)
    }

    /// Returns usable keys: cached, revalidated, or (cold) freshly fetched.
    async fn current_keys(&self) -> Result<Arc<Snapshot>, AuthError> {
        let now = Instant::now();
        let (snapshot, attempts, retry_allowed) = {
            let state = self.state.read();
            (
                state.snapshot.clone(),
                state.attempts,
                state.retry_allowed(now, self.policy.retry_backoff),
            )
        };
        let Some(snapshot) = snapshot else {
            if !retry_allowed {
                return Err(self.last_error());
            }
            return self.refresh_shared(attempts).await;
        };
        let age = now.saturating_duration_since(snapshot.fetched_at);
        if age < self.policy.max_age {
            return Ok(snapshot);
        }
        let usable_stale = age < self.policy.max_stale;
        if !retry_allowed {
            return if usable_stale {
                Ok(snapshot)
            } else {
                Err(self.last_error())
            };
        }
        // Stale-while-revalidate: only one caller fetches, the rest continue.
        match self.flight.try_lock() {
            Ok(guard) => match self.fetch(guard, attempts).await {
                Ok(fresh) => Ok(fresh),
                Err(error) if usable_stale => {
                    tracing::warn!(%error, "OpenID key refresh failed; serving cached keys");
                    Ok(snapshot)
                }
                Err(error) => Err(error),
            },
            Err(_) if usable_stale => Ok(snapshot),
            Err(_) => self.refresh_shared(attempts).await,
        }
    }

    /// gophertunnel `refreshingKeySet.VerifySignature`: an unknown `kid`
    /// refreshes once the snapshot is old enough; otherwise the token is
    /// verified (and rejected) against the cached keys.
    async fn refresh_for_unknown_kid(
        &self,
        snapshot: &Snapshot,
    ) -> Result<Option<Arc<Snapshot>>, AuthError> {
        let now = Instant::now();
        let (attempts, retry_allowed) = {
            let state = self.state.read();
            if !state
                .snapshot
                .as_ref()
                .is_some_and(|current| std::ptr::eq(&**current, snapshot))
            {
                // Someone refreshed since this caller read the snapshot.
                return Ok(state.snapshot.clone());
            }
            (
                state.attempts,
                state.retry_allowed(now, self.policy.retry_backoff),
            )
        };
        if now.saturating_duration_since(snapshot.fetched_at) < self.policy.unknown_kid_interval
            || !retry_allowed
        {
            return Ok(None);
        }
        self.refresh_shared(attempts).await.map(Some)
    }

    /// Waits for the in-flight fetch or performs one. `observed_attempts` is
    /// the attempt counter seen before deciding to refresh; if an attempt
    /// completed meanwhile, its outcome is shared instead of fetching again.
    async fn refresh_shared(&self, observed_attempts: u64) -> Result<Arc<Snapshot>, AuthError> {
        let guard = self.flight.lock().await;
        self.fetch(guard, observed_attempts).await
    }

    async fn fetch(
        &self,
        _guard: tokio::sync::MutexGuard<'_, ()>,
        observed_attempts: u64,
    ) -> Result<Arc<Snapshot>, AuthError> {
        {
            let state = self.state.read();
            if state.attempts != observed_attempts {
                return match (&state.last_error, &state.snapshot) {
                    (None, Some(snapshot)) => Ok(snapshot.clone()),
                    (Some(error), _) => Err(error.clone()),
                    (None, None) => Err(AuthError::ProviderUnavailable("no key set".into())),
                };
            }
        }
        let result = tokio::time::timeout(self.policy.fetch_timeout, self.source.fetch())
            .await
            .unwrap_or_else(|_| {
                Err(AuthError::ProviderUnavailable(
                    "key set fetch timed out".into(),
                ))
            });
        let now = Instant::now();
        let mut state = self.state.write();
        state.attempts += 1;
        state.last_attempt = Some(now);
        match result {
            Ok(verifier) => {
                let snapshot = Arc::new(Snapshot {
                    verifier,
                    fetched_at: now,
                });
                state.snapshot = Some(snapshot.clone());
                state.last_error = None;
                Ok(snapshot)
            }
            Err(error) => {
                state.last_error = Some(error.clone());
                Err(error)
            }
        }
    }

    fn last_error(&self) -> AuthError {
        self.state
            .read()
            .last_error
            .clone()
            .unwrap_or_else(|| AuthError::ProviderUnavailable("no key set".into()))
    }
}

static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .user_agent("jolyne-auth/0.1")
        .build()
        .expect("reqwest client")
});

async fn get_json<T: DeserializeOwned>(url: &str) -> Result<T, AuthError> {
    HTTP_CLIENT
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| AuthError::ProviderUnavailable(format!("HTTP: {e}")))?
        .json()
        .await
        .map_err(|e| AuthError::ProviderUnavailable(format!("JSON: {e}")))
}

/// Only the fixed Minecraft build discovery endpoint bootstraps production trust.
/// See gophertunnel minecraft/service/discovery.go:Default and
/// minecraft/service/token.go:configuration. No header URLs or static fallback.
struct MinecraftDiscovery;

impl KeySetSource for MinecraftDiscovery {
    fn fetch(&self) -> BoxFuture<'_, Result<OpenIdVerifier, AuthError>> {
        Box::pin(async {
            let discovery: DiscoveryEnvelope = get_json(&format!(
                "{DISCOVERY_URL}{}",
                crate::valentine::GAME_VERSION
            ))
            .await?;
            let issuer = discovery.result.service_environments.auth.prod.issuer;
            let issuer = format!("{}/", issuer.trim_end_matches('/'));
            let config: Configuration =
                get_json(&format!("{issuer}.well-known/openid-configuration")).await?;
            if config.issuer != issuer {
                return Err(AuthError::InvalidIssuer(config.issuer));
            }
            let jwks: serde_json::Value = get_json(&config.jwks_uri).await?;
            OpenIdVerifier::from_jwks(
                &issuer,
                &config.id_token_signing_alg_values_supported,
                &jwks.to_string(),
            )
        })
    }
}
