//! Minecraft multiplayer JWT verification using issuer-owned OpenID discovery.
//!
//! Discovery and signing keys are cached. Unknown key IDs may refresh keys once
//! every 30 minutes; invalid signatures for known IDs never trigger a refresh.
//! Errors deliberately discard remote bodies, tokens, URLs, and crypto details.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::future::{BoxFuture, FutureExt, Shared};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::{Mutex, OnceCell};

use crate::error::{XblError, XblResult, check_status, response_json};

const AUDIENCE: &str = "api://auth-minecraft-services/multiplayer";
const REFRESH_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// Verifies Minecraft multiplayer tokens against a trusted authorization issuer.
///
/// Construct this from trusted service discovery, never from a token's `iss`,
/// `jku`, or `x5u` header. Claims are returned only after signature, algorithm,
/// issuer, audience, expiration, and not-before checks. Tokens are never logged.
pub struct MultiplayerTokenVerifier {
    issuer: String,
    discovery_url: reqwest::Url,
    client: reqwest::Client,
    configuration: OnceCell<Configuration>,
    keys: Mutex<KeyCache>,
}

struct Configuration {
    jwks_url: reqwest::Url,
    algorithms: Vec<Algorithm>,
}

struct Key {
    id: Option<String>,
    decoding: DecodingKey,
}

// Only safe, copyable metadata can cross the shared refresh boundary.
#[derive(Clone)]
enum FetchError {
    Http(Option<reqwest::StatusCode>),
    Status(reqwest::StatusCode),
    Json(serde_json::error::Category),
    Invalid,
}

impl From<XblError> for FetchError {
    fn from(error: XblError) -> Self {
        match error {
            XblError::Http(status) => Self::Http(status),
            XblError::HttpStatus { status, .. } => Self::Status(status),
            XblError::Json(category) => Self::Json(category),
            _ => Self::Invalid,
        }
    }
}

impl From<FetchError> for XblError {
    fn from(error: FetchError) -> Self {
        match error {
            FetchError::Http(status) => Self::Http(status),
            FetchError::Status(status) => Self::HttpStatus {
                status,
                operation: "Multiplayer signing keys",
            },
            FetchError::Json(category) => Self::Json(category),
            FetchError::Invalid => Self::InvalidToken,
        }
    }
}

type Refresh = Shared<BoxFuture<'static, Result<Arc<Vec<Key>>, FetchError>>>;

#[derive(Default)]
struct KeyCache {
    keys: Arc<Vec<Key>>,
    last_fetch: Option<Instant>,
    inflight: Option<Refresh>,
    generation: u64,
}

impl MultiplayerTokenVerifier {
    /// Creates a lazy verifier; network requests occur on the first verification.
    /// The HTTP client applies a 30-second timeout to discovery and JWKS requests.
    pub fn new(issuer: &str) -> XblResult<Self> {
        let mut issuer_url = reqwest::Url::parse(issuer).map_err(|_| XblError::InvalidToken)?;
        if !matches!(issuer_url.scheme(), "http" | "https")
            || issuer_url.host_str().is_none()
            || !issuer_url.username().is_empty()
            || issuer_url.password().is_some()
            || issuer_url.query().is_some()
            || issuer_url.fragment().is_some()
        {
            return Err(XblError::InvalidToken);
        }
        if !issuer_url.path().ends_with('/') {
            issuer_url.set_path(&format!("{}/", issuer_url.path()));
        }
        let discovery_url = issuer_url
            .join(".well-known/openid-configuration")
            .map_err(|_| XblError::InvalidToken)?;
        Ok(Self {
            issuer: issuer_url.to_string(),
            discovery_url,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()?,
            configuration: OnceCell::new(),
            keys: Mutex::new(KeyCache::default()),
        })
    }

    /// Returns cryptographically verified claims, or a non-secret typed error.
    pub async fn verify(&self, token: &str) -> XblResult<Value> {
        let config = self
            .configuration
            .get_or_try_init(|| self.discover())
            .await?;
        let header = decode_header(token).map_err(|_| XblError::InvalidToken)?;
        if !config.algorithms.contains(&header.alg) {
            return Err(XblError::InvalidToken);
        }
        let kid = header.kid.as_deref().filter(|id| !id.is_empty());
        let mut validation = Validation::new(header.alg);
        // OIDC has zero expiration leeway but five minutes of nbf leeway.
        // Validate both explicitly after cryptographic verification.
        validation.required_spec_claims.clear();
        validation.validate_exp = false;
        validation.validate_nbf = false;
        validation.validate_aud = false;
        let (refresh, generation) = {
            let mut cache = self.keys.lock().await;
            if let Some(claims) = verify_signature(token, kid, &cache.keys, &validation) {
                return self.validate_claims(claims);
            }
            let missing_id =
                kid.is_some_and(|id| !cache.keys.iter().any(|key| key.id.as_deref() == Some(id)));
            if !(cache.keys.is_empty()
                || (missing_id
                    && cache
                        .last_fetch
                        .is_some_and(|time| time.elapsed() >= REFRESH_INTERVAL)))
            {
                return Err(XblError::InvalidToken);
            }
            if cache.inflight.is_none() {
                let (client, url) = (self.client.clone(), config.jwks_url.clone());
                // Dropping a caller does not cancel the shared HTTP refresh.
                let task = tokio::spawn(async move {
                    let value = get_json(&client, url, "Multiplayer signing keys").await?;
                    decode_keys(value).map(Arc::new)
                });
                cache.inflight = Some(
                    async move {
                        task.await
                            .map_err(|_| FetchError::Invalid)?
                            .map_err(FetchError::from)
                    }
                    .boxed()
                    .shared(),
                );
                cache.generation = cache.generation.wrapping_add(1);
            }
            (
                cache
                    .inflight
                    .as_ref()
                    .expect("refresh initialized")
                    .clone(),
                cache.generation,
            )
        };
        let result = refresh.await;
        {
            let mut cache = self.keys.lock().await;
            if cache.generation == generation && cache.inflight.is_some() {
                if let Ok(keys) = &result {
                    cache.keys = keys.clone();
                    cache.last_fetch = Some(Instant::now());
                }
                cache.inflight = None;
            }
        }
        let keys = result.map_err(XblError::from)?;
        let claims =
            verify_signature(token, kid, &keys, &validation).ok_or(XblError::InvalidToken)?;
        self.validate_claims(claims)
    }

    async fn discover(&self) -> XblResult<Configuration> {
        #[derive(Deserialize)]
        struct Discovery {
            jwks_uri: String,
            #[serde(default)]
            id_token_signing_alg_values_supported: Vec<String>,
        }
        let value = get_json(
            &self.client,
            self.discovery_url.clone(),
            "Multiplayer OpenID discovery",
        )
        .await?;
        let discovery: Discovery = serde_json::from_value(value)?;
        let jwks_url =
            reqwest::Url::parse(&discovery.jwks_uri).map_err(|_| XblError::InvalidToken)?;
        if !matches!(jwks_url.scheme(), "http" | "https") || jwks_url.host_str().is_none() {
            return Err(XblError::InvalidToken);
        }
        let specified = discovery.id_token_signing_alg_values_supported;
        let algorithms = if specified.iter().all(String::is_empty) {
            vec![Algorithm::RS256]
        } else {
            // Unsupported advertised algorithms cannot authorize any token.
            specified
                .iter()
                .filter_map(|algorithm| algorithm.parse().ok())
                .collect()
        };
        Ok(Configuration {
            jwks_url,
            algorithms,
        })
    }

    fn validate_claims(&self, claims: Value) -> XblResult<Value> {
        // Match go-oidc's typed claims decoder without retaining another copy of
        // the token or allowing malformed optional OIDC fields through.
        for field in ["sub", "nonce", "at_hash"] {
            if claims
                .get(field)
                .is_some_and(|value| !value.is_null() && !value.is_string())
            {
                return Err(XblError::InvalidToken);
            }
        }
        if claims
            .get("iat")
            .is_some_and(|value| numeric_date(value).is_none())
        {
            return Err(XblError::InvalidToken);
        }
        let sources = claims
            .get("_claim_sources")
            .filter(|value| !value.is_null());
        if let Some(sources) = sources {
            let sources = sources.as_object().ok_or(XblError::InvalidToken)?;
            for source in sources.values() {
                if !source.is_null() && !source.is_object() {
                    return Err(XblError::InvalidToken);
                }
                for field in ["endpoint", "access_token"] {
                    if source
                        .get(field)
                        .is_some_and(|value| !value.is_null() && !value.is_string())
                    {
                        return Err(XblError::InvalidToken);
                    }
                }
            }
        }
        if let Some(names) = claims.get("_claim_names").filter(|value| !value.is_null()) {
            let names = names.as_object().ok_or(XblError::InvalidToken)?;
            for source in names.values() {
                let source = source
                    .as_str()
                    .filter(|name| !name.is_empty())
                    .ok_or(XblError::InvalidToken)?;
                if sources.is_none_or(|sources| sources.get(source).is_none()) {
                    return Err(XblError::InvalidToken);
                }
            }
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| XblError::InvalidToken)?
            .as_secs_f64();
        let audience = match &claims["aud"] {
            Value::String(audience) => audience == AUDIENCE,
            Value::Array(audiences) => {
                audiences.iter().all(Value::is_string)
                    && audiences
                        .iter()
                        .any(|audience| audience.as_str() == Some(AUDIENCE))
            }
            _ => false,
        };
        if claims["iss"].as_str() != Some(self.issuer.as_str())
            || !audience
            || !numeric_date(&claims["exp"]).is_some_and(|expiry| expiry >= now)
            || claims
                .get("nbf")
                .filter(|value| !value.is_null())
                .is_some_and(|value| !numeric_date(value).is_some_and(|nbf| nbf <= now + 300.0))
        {
            return Err(XblError::InvalidToken);
        }
        Ok(claims)
    }
}

fn numeric_date(value: &Value) -> Option<f64> {
    // go-oidc accepts both JSON numbers and quoted numeric dates.
    let number = value
        .as_f64()
        .or_else(|| value.as_str()?.parse::<f64>().ok())?;
    number.is_finite().then_some(number.trunc())
}

fn verify_signature(
    token: &str,
    kid: Option<&str>,
    keys: &[Key],
    validation: &Validation,
) -> Option<Value> {
    keys.iter()
        .filter(|key| kid.is_none() || key.id.as_deref() == kid)
        .find_map(|key| {
            decode::<Value>(token, &key.decoding, validation)
                .ok()
                .map(|token| token.claims)
        })
}

async fn get_json(
    client: &reqwest::Client,
    url: reqwest::Url,
    operation: &'static str,
) -> XblResult<Value> {
    let response = client
        .get(url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("User-Agent", crate::playfab::SERVICE_USER_AGENT)
        .send()
        .await?;
    check_status(&response, operation)?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(XblError::HttpStatus {
            status: response.status(),
            operation,
        });
    }
    response_json(response, operation).await
}

fn decode_keys(mut value: Value) -> XblResult<Vec<Key>> {
    let object = value.is_object();
    let keys = match value.get_mut("keys") {
        Some(Value::Array(keys)) => keys,
        None | Some(Value::Null) if object => return Ok(Vec::new()),
        _ => return Err(XblError::InvalidToken),
    };
    keys.iter_mut()
        .map(|value| {
            let fingerprint = value
                .get("x5t")
                .and_then(Value::as_str)
                .ok_or(XblError::InvalidToken)?;
            let fingerprint = match fingerprint.len() {
                40 => hex::decode(fingerprint).map_err(|_| XblError::InvalidToken)?,
                27 => URL_SAFE_NO_PAD
                    .decode(fingerprint)
                    .map_err(|_| XblError::InvalidToken)?,
                _ => return Err(XblError::InvalidToken),
            };
            if fingerprint.len() != 20 {
                return Err(XblError::InvalidToken);
            }
            value["x5t"] = Value::String(URL_SAFE_NO_PAD.encode(fingerprint));
            let jwk: jsonwebtoken::jwk::Jwk = serde_json::from_value(value.take())?;
            let decoding = DecodingKey::from_jwk(&jwk).map_err(|_| XblError::InvalidToken)?;
            Ok(Key {
                id: jwk.common.key_id,
                decoding,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header, encode};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // Public, test-only RSA key from jsonwebtoken's RSA interoperability fixtures.
    const PRIVATE_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDJETqse41HRBsc
7cfcq3ak4oZWFCoZlcic525A3FfO4qW9BMtRO/iXiyCCHn8JhiL9y8j5JdVP2Q9Z
IpfElcFd3/guS9w+5RqQGgCR+H56IVUyHZWtTJbKPcwWXQdNUX0rBFcsBzCRESJL
eelOEdHIjG7LRkx5l/FUvlqsyHDVJEQsHwegZ8b8C0fz0EgT2MMEdn10t6Ur1rXz
jMB/wvCg8vG8lvciXmedyo9xJ8oMOh0wUEgxziVDMMovmC+aJctcHUAYubwoGN8T
yzcvnGqL7JSh36Pwy28iPzXZ2RLhAyJFU39vLaHdljwthUaupldlNyCfa6Ofy4qN
ctlUPlN1AgMBAAECggEAdESTQjQ70O8QIp1ZSkCYXeZjuhj081CK7jhhp/4ChK7J
GlFQZMwiBze7d6K84TwAtfQGZhQ7km25E1kOm+3hIDCoKdVSKch/oL54f/BK6sKl
qlIzQEAenho4DuKCm3I4yAw9gEc0DV70DuMTR0LEpYyXcNJY3KNBOTjN5EYQAR9s
2MeurpgK2MdJlIuZaIbzSGd+diiz2E6vkmcufJLtmYUT/k/ddWvEtz+1DnO6bRHh
xuuDMeJA/lGB/EYloSLtdyCF6sII6C6slJJtgfb0bPy7l8VtL5iDyz46IKyzdyzW
tKAn394dm7MYR1RlUBEfqFUyNK7C+pVMVoTwCC2V4QKBgQD64syfiQ2oeUlLYDm4
CcKSP3RnES02bcTyEDFSuGyyS1jldI4A8GXHJ/lG5EYgiYa1RUivge4lJrlNfjyf
dV230xgKms7+JiXqag1FI+3mqjAgg4mYiNjaao8N8O3/PD59wMPeWYImsWXNyeHS
55rUKiHERtCcvdzKl4u35ZtTqQKBgQDNKnX2bVqOJ4WSqCgHRhOm386ugPHfy+8j
m6cicmUR46ND6ggBB03bCnEG9OtGisxTo/TuYVRu3WP4KjoJs2LD5fwdwJqpgtHl
yVsk45Y1Hfo+7M6lAuR8rzCi6kHHNb0HyBmZjysHWZsn79ZM+sQnLpgaYgQGRbKV
DZWlbw7g7QKBgQCl1u+98UGXAP1jFutwbPsx40IVszP4y5ypCe0gqgon3UiY/G+1
zTLp79GGe/SjI2VpQ7AlW7TI2A0bXXvDSDi3/5Dfya9ULnFXv9yfvH1QwWToySpW
Kvd1gYSoiX84/WCtjZOr0e0HmLIb0vw0hqZA4szJSqoxQgvF22EfIWaIaQKBgQCf
34+OmMYw8fEvSCPxDxVvOwW2i7pvV14hFEDYIeZKW2W1HWBhVMzBfFB5SE8yaCQy
pRfOzj9aKOCm2FjjiErVNpkQoi6jGtLvScnhZAt/lr2TXTrl8OwVkPrIaN0bG/AS
aUYxmBPCpXu3UjhfQiWqFq/mFyzlqlgvuCc9g95HPQKBgAscKP8mLxdKwOgX8yFW
GcZ0izY/30012ajdHY+/QK5lsMoxTnn0skdS+spLxaS5ZEO4qvPVb8RAoCkWMMal
2pOhmquJQVDPDLuZHdrIiKiDM20dy9sMfHygWcZjQ4WSxf/J7T9canLZIXFhHAZT
3wc9h4G8BBCtWN2TN/LsGZdB
-----END PRIVATE KEY-----
";
    const MODULUS: &str = "yRE6rHuNR0QbHO3H3Kt2pOKGVhQqGZXInOduQNxXzuKlvQTLUTv4l4sggh5_CYYi_cvI-SXVT9kPWSKXxJXBXd_4LkvcPuUakBoAkfh-eiFVMh2VrUyWyj3MFl0HTVF9KwRXLAcwkREiS3npThHRyIxuy0ZMeZfxVL5arMhw1SRELB8HoGfG_AtH89BIE9jDBHZ9dLelK9a184zAf8LwoPLxvJb3Il5nncqPcSfKDDodMFBIMc4lQzDKL5gvmiXLXB1AGLm8KBjfE8s3L5xqi-yUod-j8MtvIj812dkS4QMiRVN_by2h3ZY8LYVGrqZXZTcgn2ujn8uKjXLZVD5TdQ";

    fn jwk(kid: &str) -> Value {
        json!({"kty":"RSA", "n":MODULUS, "e":"AQAB", "kid":kid,
            "x5t":"00112233445566778899aabbccddeeff00112233"})
    }

    struct Server {
        issuer: String,
        response: Arc<Mutex<(u16, String)>>,
        keys: Arc<AtomicUsize>,
        config: Arc<AtomicUsize>,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    impl Server {
        async fn new() -> Self {
            Self::with_algorithm(None).await
        }

        async fn with_algorithm(algorithm: Option<&'static str>) -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let issuer = format!("http://{}/", listener.local_addr().unwrap());
            let response = Arc::new(Mutex::new((200, json!({"keys":[jwk("old")]}).to_string())));
            let keys = Arc::new(AtomicUsize::new(0));
            let config = Arc::new(AtomicUsize::new(0));
            let (r, k, c, base) = (
                response.clone(),
                keys.clone(),
                config.clone(),
                issuer.clone(),
            );
            let task = tokio::spawn(async move {
                loop {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let mut request = Vec::new();
                    loop {
                        let mut buf = [0; 1024];
                        let n = stream.read(&mut buf).await.unwrap();
                        if n == 0 {
                            break;
                        }
                        request.extend_from_slice(&buf[..n]);
                        if request.windows(4).any(|v| v == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let request = String::from_utf8(request).unwrap().to_lowercase();
                    assert!(request.contains("accept: application/json"));
                    assert!(request.contains("content-type: application/json"));
                    assert!(request.contains("user-agent: libhttpclient/1.0.0.0"));
                    let (status, body) = if request
                        .starts_with("get /.well-known/openid-configuration ")
                    {
                        c.fetch_add(1, Ordering::SeqCst);
                        let mut discovery = json!({"jwks_uri":format!("{base}keys")});
                        if let Some(algorithm) = algorithm {
                            discovery["id_token_signing_alg_values_supported"] = json!([algorithm]);
                        }
                        (200, discovery.to_string())
                    } else {
                        assert!(request.starts_with("get /keys "));
                        k.fetch_add(1, Ordering::SeqCst);
                        r.lock().await.clone()
                    };
                    let response = format!(
                        "HTTP/1.1 {status} Result\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    stream.write_all(response.as_bytes()).await.unwrap();
                }
            });
            Self {
                issuer,
                response,
                keys,
                config,
                task,
            }
        }
        fn claims(&self) -> Value {
            json!({"iss":self.issuer, "aud":AUDIENCE, "exp":4102444800u64, "sub":"player"})
        }
        fn token(&self, claims: &Value, kid: Option<&str>, alg: Algorithm) -> String {
            let mut header = Header::new(alg);
            header.kid = kid.map(str::to_owned);
            encode(
                &header,
                claims,
                &EncodingKey::from_rsa_pem(PRIVATE_KEY.as_bytes()).unwrap(),
            )
            .unwrap()
        }
    }

    #[tokio::test]
    async fn verifies_signed_claims_and_rejects_invalid_tokens() {
        let server = Server::new().await;
        let verifier = MultiplayerTokenVerifier::new(server.issuer.trim_end_matches('/')).unwrap();
        let claims = server.claims();
        let valid = server.token(&claims, Some("old"), Algorithm::RS256);
        assert_eq!(verifier.verify(&valid).await.unwrap(), claims);
        for (field, value) in [
            ("exp", json!(1)),
            ("aud", json!("wrong")),
            ("iss", json!("wrong")),
            ("nbf", json!(4102444800u64)),
        ] {
            let mut invalid = claims.clone();
            invalid[field] = value;
            assert!(
                verifier
                    .verify(&server.token(&invalid, Some("old"), Algorithm::RS256))
                    .await
                    .is_err()
            );
        }
        assert!(
            verifier
                .verify(&server.token(&claims, Some("old"), Algorithm::RS384))
                .await
                .is_err()
        );
        let mut mismatch = valid.into_bytes();
        let signature = mismatch.iter().rposition(|b| *b == b'.').unwrap() + 1;
        mismatch[signature] = if mismatch[signature] == b'A' {
            b'B'
        } else {
            b'A'
        };
        verifier.keys.lock().await.last_fetch = Some(Instant::now() - REFRESH_INTERVAL);
        assert!(
            verifier
                .verify(std::str::from_utf8(&mismatch).unwrap())
                .await
                .is_err()
        );
        assert_eq!(server.keys.load(Ordering::SeqCst), 1);
        assert_eq!(server.config.load(Ordering::SeqCst), 1);
        let mut skew = claims.clone();
        skew["nbf"] = json!(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 240
        );
        assert!(
            verifier
                .verify(&server.token(&skew, None, Algorithm::RS256))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn rotation_is_throttled_and_concurrent_refresh_is_shared() {
        let server = Server::new().await;
        let verifier = Arc::new(MultiplayerTokenVerifier::new(&server.issuer).unwrap());
        let old = server.token(&server.claims(), Some("old"), Algorithm::RS256);
        verifier.verify(&old).await.unwrap();
        *server.response.lock().await = (200, json!({"keys":[jwk("new")]}).to_string());
        let new = server.token(&server.claims(), Some("new"), Algorithm::RS256);
        assert!(verifier.verify(&new).await.is_err());
        assert_eq!(server.keys.load(Ordering::SeqCst), 1);
        // Age only the cache timestamp: no wall-clock sleeps or production bypass.
        verifier.keys.lock().await.last_fetch = Some(Instant::now() - REFRESH_INTERVAL);
        let mut tasks = Vec::new();
        for _ in 0..16 {
            let (v, token) = (verifier.clone(), new.clone());
            tasks.push(tokio::spawn(async move { v.verify(&token).await }));
        }
        for task in tasks {
            task.await.unwrap().unwrap();
        }
        assert_eq!(server.keys.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn malformed_and_non_ok_jwks_errors_never_disclose_secrets() {
        let server = Server::new().await;
        let token = server.token(&server.claims(), Some("old"), Algorithm::RS256);
        for (status, body) in [
            (503, "secret-body"),
            (200, "{secret-body"),
            (200, "{\"keys\":[{\"x5t\":\"secret-body\"}]}"),
            (201, "secret-body"),
        ] {
            *server.response.lock().await = (status, body.into());
            let verifier = MultiplayerTokenVerifier::new(&server.issuer).unwrap();
            let error = verifier.verify(&token).await.unwrap_err();
            assert!(!format!("{error:?} {error}").contains("secret-body"));
            assert!(std::error::Error::source(&error).is_none());
            if status != 200 {
                assert_eq!(error.status().unwrap().as_u16(), status);
            }
        }
    }

    #[tokio::test]
    async fn discovery_controls_algorithms_and_absent_kid_tries_every_key() {
        let server = Server::with_algorithm(Some("RS384")).await;
        let mut bad_key = jwk("wrong");
        bad_key["e"] = json!("Aw");
        *server.response.lock().await = (200, json!({"keys":[bad_key, jwk("right")]}).to_string());
        let verifier = MultiplayerTokenVerifier::new(&server.issuer).unwrap();
        let claims = server.claims();
        assert!(
            verifier
                .verify(&server.token(&claims, None, Algorithm::RS384))
                .await
                .is_ok()
        );
        assert!(
            verifier
                .verify(&server.token(&claims, None, Algorithm::RS256))
                .await
                .is_err()
        );
        assert!(
            verifier
                .verify(&server.token(&claims, Some("wrong"), Algorithm::RS384))
                .await
                .is_err()
        );
        assert_eq!(server.keys.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn rejects_missing_expiry_and_accepts_oidc_numeric_dates_and_audience_arrays() {
        let server = Server::new().await;
        let verifier = MultiplayerTokenVerifier::new(&server.issuer).unwrap();
        let mut claims = server.claims();
        claims.as_object_mut().unwrap().remove("exp");
        assert!(
            verifier
                .verify(&server.token(&claims, None, Algorithm::RS256))
                .await
                .is_err()
        );
        claims["exp"] = json!("4102444800");
        claims["aud"] = json!(["another-audience", AUDIENCE]);
        assert!(
            verifier
                .verify(&server.token(&claims, None, Algorithm::RS256))
                .await
                .is_ok()
        );
        for (field, value) in [
            ("iat", json!("invalid-date")),
            ("sub", json!({})),
            ("_claim_names", json!({"name":"missing-source"})),
            ("_claim_sources", json!({"source":{"endpoint":7}})),
        ] {
            let mut invalid = claims.clone();
            invalid[field] = value;
            assert!(
                verifier
                    .verify(&server.token(&invalid, None, Algorithm::RS256))
                    .await
                    .is_err()
            );
        }
        claims["iss"] = json!(server.issuer.trim_end_matches('/'));
        assert!(
            verifier
                .verify(&server.token(&claims, None, Algorithm::RS256))
                .await
                .is_err()
        );
    }

    #[test]
    fn thumbprints_require_exact_sha1_hex_or_unpadded_base64url() {
        for fingerprint in [
            "00112233445566778899AABBCCDDEEFF00112233",
            "ABEiM0RVZneImaq7zN3u_wARIjM",
        ] {
            let mut key = jwk("old");
            key["x5t"] = json!(fingerprint);
            assert!(decode_keys(json!({"keys":[key]})).is_ok());
        }
        for fingerprint in [
            Value::Null,
            json!(7),
            json!("secret"),
            json!("GG112233445566778899aabbccddeeff00112233"),
            json!("ABEiM0RVZneImaq7zN3u/wARIjM"),
            json!("ABEiM0RVZneImaq7zN3u_wARIjM="),
        ] {
            let mut key = jwk("old");
            key["x5t"] = fingerprint;
            assert!(decode_keys(json!({"keys":[key]})).is_err());
        }
        let mut key = jwk("old");
        key.as_object_mut().unwrap().remove("x5t");
        assert!(decode_keys(json!({"keys":[key]})).is_err());
    }
}
