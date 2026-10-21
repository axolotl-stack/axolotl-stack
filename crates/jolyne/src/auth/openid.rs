use crate::auth::types::{ClientDataClaims, OpenIdClaims, ValidatedIdentity};
use crate::auth::util::{decode_unverified_claims, key_from_base64};
use crate::error::{AuthError, JolyneError};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::{Deserialize, de::DeserializeOwned};
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const KEY_TTL: Duration = Duration::from_secs(30 * 60);
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
/// Mirrors gophertunnel be557e3 minecraft/service/token.go:VerifierContext and
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
    ) -> Result<Self, JolyneError> {
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
            return Err(AuthError::BadSignature("No trusted signing keys or issuer".into()).into());
        }
        Ok(Self {
            issuer: issuer.into(),
            keys,
        })
    }

    /// Verify provider identity and bind client data to its authenticated `cpk`.
    /// `iss`, `aud`, `exp` and `nbf` are mandatory; skew is at most 60 seconds.
    pub fn validate(
        &self,
        token: &str,
        client_data_jwt: &str,
    ) -> Result<ValidatedIdentity, JolyneError> {
        let header = decode_header(token).map_err(|e| AuthError::InvalidHeader(e.to_string()))?;
        let mut validation = Validation::new(header.alg);
        validation.set_required_spec_claims(&["iss", "aud", "exp", "nbf"]);
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[MOJANG_AUDIENCE]);
        validation.validate_nbf = true;
        validation.leeway = CLOCK_SKEW;
        let mut last_error =
            AuthError::BadSignature("No matching trusted provider key".into()).into();
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
                Ok(data) => return identity_from_claims(data.claims, client_data_jwt, true),
                Err(err) => last_error = jwt_error(err),
            }
        }
        Err(last_error)
    }
}

fn jwt_error(err: jsonwebtoken::errors::Error) -> JolyneError {
    use jsonwebtoken::errors::ErrorKind;
    match err.kind() {
        ErrorKind::ExpiredSignature | ErrorKind::ImmatureSignature => AuthError::TemporalValidation,
        ErrorKind::InvalidIssuer => AuthError::InvalidIssuer(err.to_string()),
        ErrorKind::InvalidAudience => AuthError::InvalidAudience(err.to_string()),
        _ => AuthError::BadSignature(err.to_string()),
    }
    .into()
}

/// Client data is client-controlled, but must prove possession of the identity key.
/// Like gophertunnel minecraft/protocol/login/request.go:parseFullClaim, it uses
/// ES384 and does not require exp/aud (the client-data payload has neither).
pub(crate) fn verify_client_data(
    identity: &ValidatedIdentity,
    jwt: &str,
) -> Result<ClientDataClaims, JolyneError> {
    let mut validation = Validation::new(Algorithm::ES384);
    validation.required_spec_claims.clear();
    validation.validate_exp = false;
    validation.validate_aud = false;
    let key = key_from_base64(&identity.identity_public_key)?;
    let claims = decode::<ClientDataClaims>(jwt, &key, &validation)
        .map_err(jwt_error)?
        .claims;
    if let Some(client_key) = &claims.identity_public_key
        && client_key != &identity.identity_public_key
    {
        return Err(AuthError::BadSignature("Client identity key mismatch".into()).into());
    }
    Ok(claims)
}

fn identity_from_claims(
    claims: OpenIdClaims,
    client_data_jwt: &str,
    authenticated: bool,
) -> Result<ValidatedIdentity, JolyneError> {
    let identity = ValidatedIdentity {
        xuid: claims.xuid.or_else(|| {
            claims
                .extra_data
                .as_ref()
                .and_then(|extra| extra.xuid.clone())
        }),
        display_name: claims.xbox_username.or_else(|| {
            claims
                .extra_data
                .as_ref()
                .and_then(|extra| extra.display_name.clone())
        }),
        uuid: claims.legacy_uuid.or_else(|| {
            claims
                .extra_data
                .as_ref()
                .and_then(|extra| extra.uuid.clone())
        }),
        identity_public_key: claims
            .identity_public_key
            .filter(|key| !key.is_empty())
            .ok_or(AuthError::MissingIdentityKey)?,
        authenticated,
    };
    verify_client_data(&identity, client_data_jwt)?;
    Ok(identity)
}

static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .https_only(true)
        .timeout(Duration::from_secs(15))
        .user_agent("jolyne-auth/0.1")
        .build()
        .expect("reqwest client")
});
static PROVIDER: LazyLock<Mutex<Option<(Instant, OpenIdVerifier)>>> =
    LazyLock::new(|| Mutex::new(None));
async fn get_json<T: DeserializeOwned>(url: &str) -> Result<T, JolyneError> {
    HTTP_CLIENT
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| AuthError::BadSignature(format!("Auth discovery HTTP: {e}")))?
        .json()
        .await
        .map_err(|e| AuthError::BadSignature(format!("Auth discovery JSON: {e}")).into())
}

/// Only the fixed Minecraft build discovery endpoint bootstraps production trust.
/// See gophertunnel minecraft/service/discovery.go:Default and
/// minecraft/service/token.go:configuration. No header URLs, static fallback,
/// stale keys after expiry, or swallowed discovery failures.
async fn fetch_verifier() -> Result<OpenIdVerifier, JolyneError> {
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
        return Err(AuthError::InvalidIssuer(config.issuer).into());
    }
    let jwks: serde_json::Value = get_json(&config.jwks_uri).await?;
    OpenIdVerifier::from_jwks(
        &issuer,
        &config.id_token_signing_alg_values_supported,
        &jwks.to_string(),
    )
}

pub async fn validate_open_id(
    token: &str,
    client_data_jwt: &str,
    online_mode: bool,
) -> Result<ValidatedIdentity, JolyneError> {
    if !online_mode {
        let claims = decode_unverified_claims(token).ok_or(AuthError::InvalidJson)?;
        return identity_from_claims(claims, client_data_jwt, false);
    }
    let mut cache = PROVIDER.lock().await;
    if cache
        .as_ref()
        .is_none_or(|(fetched, _)| fetched.elapsed() >= KEY_TTL)
    {
        *cache = Some((Instant::now(), fetch_verifier().await?));
    }
    cache
        .as_ref()
        .expect("populated above")
        .1
        .validate(token, client_data_jwt)
}
