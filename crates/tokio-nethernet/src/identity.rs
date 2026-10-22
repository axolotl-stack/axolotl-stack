//! NetherNet identity assertions binding authenticated JWT identities to SDP DTLS fingerprints.
// Wire constants follow df-mc/go-nethernet 215e464:
// identity.go:Identity.sign uses ES384 and idp protocol "default";
// identity.go:identityData.Valid requires "default", and identityData.verify requires ES384.
// identity.go:GenerateServerIdentity uses ES384 and a one-minute (60-second) expiry.
// identity.go:claimPublicKey -> tokenClaims.Validate -> go-jose/v4 jwt.Claims.Validate
// uses jwt.DefaultLeeway (60 seconds) for client exp/iat/nbf validation.
use std::{
    fmt,
    future::Future,
    pin::Pin,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use p384::{
    PublicKey,
    ecdsa::{
        Signature, SigningKey, VerifyingKey,
        signature::{Signer, Verifier},
    },
    pkcs8::{DecodePublicKey, EncodePublicKey},
};
use rtc_sdp::description::{common::Attribute, session::SessionDescription};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// An identity failure, including failures returned by the external authorization service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityError(pub String);

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for IdentityError {}

fn invalid(message: &str) -> IdentityError {
    IdentityError(message.to_owned())
}

/// Local P-384 key and its corresponding Minecraft authorization or self-signed token.
#[derive(Clone)]
pub struct Identity {
    pub private_key: SigningKey,
    pub token: String,
    pub domain: String,
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Identity")
            .field("domain", &self.domain)
            .finish_non_exhaustive()
    }
}

impl Identity {
    /// Generates the server's ES384 token, including JWK `cpk` and DER `x5u`.
    pub fn generate_server(
        private_key: SigningKey,
        domain: impl Into<String>,
    ) -> Result<Self, IdentityError> {
        generate_server_at(private_key, domain.into(), unix_time()?)
    }

    /// Signs the exact fingerprint used by NetherNet and inserts a session-level identity.
    /// Existing identity attributes are replaced, never appended alongside an old assertion.
    pub fn sign_sdp(&self, sdp: &str) -> Result<String, IdentityError> {
        let mut description = parse_sdp(sdp)?;
        let payload = description_fingerprint_payload(&description)?;
        let assertion = Assertion {
            fingerprints: sign_compact(&self.private_key, &json!({"alg":"ES384"}), &payload, true)?,
            token: self.token.clone(),
        };
        let data = IdentityData {
            assertion: serde_json::to_string(&assertion)
                .map_err(|_| invalid("encode identity assertion"))?,
            idp: IdentityProvider {
                domain: self.domain.clone(),
                protocol: "default".into(),
            },
        };
        let encoded =
            STANDARD.encode(serde_json::to_vec(&data).map_err(|_| invalid("encode identity"))?);
        description
            .attributes
            .retain(|attribute| attribute.key != "identity");
        for media in &mut description.media_descriptions {
            media
                .attributes
                .retain(|attribute| attribute.key != "identity");
        }
        description.attributes.push(Attribute {
            key: "identity".into(),
            value: Some(encoded),
        });
        Ok(description.marshal())
    }
}

/// The remote identity's role, not the local connection's role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerRole {
    Server,
    Client,
}

/// Optional policy hook authenticating the original peer JWT.
///
/// Implementations must verify its signature and issuer/audience according to the
/// service's requirements, and return an error on rejection or unavailable authentication.
/// Merely decoding a JWT or verifying its `cpk` signature is not client authentication.
/// For server peers this hook replaces the default self-signature check, allowing
/// callers to enforce their own token trust policy. Fingerprint binding is always checked.
pub trait TokenVerifier: Send + Sync {
    fn verify<'a>(
        &'a self,
        token: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), IdentityError>> + Send + 'a>>;
}

/// The presented token and key whose possession was proved by the SDP assertion.
///
/// Without a client token verifier this does not establish online authentication.
/// The Minecraft protocol layer must verify Login's GameServerToken and ensure
/// its `cpk` matches `public_key` before trusting the client's identity.
pub struct VerifiedIdentity {
    pub token: String,
    pub domain: String,
    pub public_key: VerifyingKey,
}

/// Verifies the identity's token policy and detached fingerprint assertion.
/// Missing identities fail closed.
///
/// Server tokens default to self-signature verification, without temporal validation,
/// matching Bedrock; a supplied verifier replaces that default token trust check.
/// Client tokens receive temporal validation (go-jose's 60-second tolerance)
/// and fingerprint key-possession verification by default, not JWT signature
/// authentication, matching upstream listener.go:ListenConfig.Listen's default
/// VerifyClientToken. Callers must verify Login's GameServerToken and bind its
/// `cpk` to the returned `public_key`, or supply an authenticating verifier here.
pub async fn verify_sdp(
    sdp: &str,
    role: PeerRole,
    verifier: Option<&dyn TokenVerifier>,
) -> Result<VerifiedIdentity, IdentityError> {
    let description = parse_sdp(sdp)?;
    verify_description(&description, role, verifier).await
}

/// Verifies a present identity, permitting absence only when explicitly requested.
///
/// Presence is determined by the transport's native SDP parser, including normalized
/// attribute whitespace. Malformed, duplicate, or media-level identities never count
/// as absent. Token trust policy is identical to [`verify_sdp`].
pub async fn verify_sdp_optional(
    sdp: &str,
    role: PeerRole,
    verifier: Option<&dyn TokenVerifier>,
    allow_identityless: bool,
) -> Result<Option<VerifiedIdentity>, IdentityError> {
    let description = parse_sdp(sdp)?;
    let has_identity = description
        .attributes
        .iter()
        .any(|attribute| attribute.key == "identity")
        || description.media_descriptions.iter().any(|media| {
            media
                .attributes
                .iter()
                .any(|attribute| attribute.key == "identity")
        });
    if !has_identity && allow_identityless {
        return Ok(None);
    }
    verify_description(&description, role, verifier)
        .await
        .map(Some)
}

async fn verify_description(
    description: &SessionDescription,
    role: PeerRole,
    verifier: Option<&dyn TokenVerifier>,
) -> Result<VerifiedIdentity, IdentityError> {
    let (data, assertion) = description_identity(description)?;
    let token = parse_token(&assertion.token)?;
    let public_key = parse_public_key(
        token
            .claims
            .get("cpk")
            .ok_or_else(|| invalid("missing cpk claim"))?,
    )?;
    match role {
        PeerRole::Server => {
            // Upstream dial.go:Dialer.DialContext uses VerifyServerToken when supplied;
            // its default invokes identity.go:claimPublicKey(token, true).
            if let Some(verifier) = verifier {
                verifier.verify(&assertion.token).await?;
            } else {
                verify_compact(&assertion.token, None, &public_key)?;
            }
        }
        PeerRole::Client => {
            // listener.go:ListenConfig.Listen defaults to claimPublicKey(token, false),
            // deferring JWT signature authentication and cpk binding to Minecraft Login.
            validate_client_time(&token.claims, unix_time()?)?;
            if let Some(verifier) = verifier {
                verifier.verify(&assertion.token).await?;
            }
        }
    }
    verify_compact(
        &assertion.fingerprints,
        Some(&description_fingerprint_payload(description)?),
        &public_key,
    )?;
    Ok(VerifiedIdentity {
        token: assertion.token,
        domain: data.idp.domain,
        public_key,
    })
}

#[derive(Serialize, Deserialize)]
struct IdentityData {
    // Upstream encodes this nested JSON object as a JSON STRING.
    assertion: String,
    idp: IdentityProvider,
}

#[derive(Serialize, Deserialize)]
struct IdentityProvider {
    domain: String,
    protocol: String,
}

#[derive(Serialize, Deserialize)]
struct Assertion {
    fingerprints: String,
    token: String,
}

fn parse_sdp(sdp: &str) -> Result<SessionDescription, IdentityError> {
    let description = SessionDescription::unmarshal(&mut std::io::Cursor::new(sdp.as_bytes()))
        .map_err(|_| invalid("invalid SDP"))?;
    if description.media_descriptions.len() != 1 {
        return Err(invalid(
            "identity requires exactly one SDP media description",
        ));
    }
    Ok(description)
}

#[cfg(test)]
fn extract_identity(sdp: &str) -> Result<(IdentityData, Assertion), IdentityError> {
    description_identity(&parse_sdp(sdp)?)
}

fn description_identity(
    description: &SessionDescription,
) -> Result<(IdentityData, Assertion), IdentityError> {
    let mut attributes = description
        .attributes
        .iter()
        .filter(|attribute| attribute.key == "identity");
    let encoded = attributes
        .next()
        .and_then(|attribute| attribute.value.as_deref());
    if attributes.next().is_some()
        || description.media_descriptions.iter().any(|media| {
            media
                .attributes
                .iter()
                .any(|attribute| attribute.key == "identity")
        })
    {
        return Err(invalid("identity must occur once at SDP session level"));
    }
    let bytes = STANDARD
        .decode(encoded.ok_or_else(|| invalid("missing SDP identity"))?)
        .map_err(|_| invalid("invalid identity base64"))?;
    let data: IdentityData =
        serde_json::from_slice(&bytes).map_err(|_| invalid("invalid identity JSON"))?;
    if data.idp.protocol != "default" {
        return Err(invalid("unsupported identity protocol"));
    }
    let assertion: Assertion = serde_json::from_str(&data.assertion)
        .map_err(|_| invalid("invalid nested identity assertion"))?;
    Ok((data, assertion))
}

#[cfg(test)]
fn fingerprint_payload(sdp: &str) -> Result<Vec<u8>, IdentityError> {
    description_fingerprint_payload(&parse_sdp(sdp)?)
}

fn description_fingerprint_payload(
    description: &SessionDescription,
) -> Result<Vec<u8>, IdentityError> {
    // First session attribute, then first media attribute (including
    // bare-attribute shadowing); conflicting valued fingerprints are rejected,
    // so the signed digest is the one `transport::sdp::parse` verifies in DTLS.
    let mut selected = description.attribute("fingerprint").map(String::as_str);
    for media in &description.media_descriptions {
        if let Some(value) = media.attribute("fingerprint").and_then(|value| value) {
            if selected.is_some_and(|previous| previous != value) {
                return Err(invalid("conflicting DTLS fingerprints"));
            }
            selected = Some(value);
        }
    }
    let value = selected.ok_or_else(|| invalid("missing DTLS fingerprint"))?;
    let mut parts = value.split(' ');
    let algorithm = parts.next().unwrap_or_default();
    let digest = parts
        .next()
        .ok_or_else(|| invalid("invalid DTLS fingerprint"))?;
    if parts.next().is_some() {
        return Err(invalid("invalid DTLS fingerprint"));
    }
    // Field order is part of the signature; do not serialize through an unordered map.
    Ok(format!(
        "{{\"fingerprint\":[{{\"algorithm\":{},\"digest\":{}}}]}}",
        serde_json::to_string(algorithm).map_err(|_| invalid("encode fingerprint algorithm"))?,
        serde_json::to_string(digest).map_err(|_| invalid("encode fingerprint digest"))?,
    )
    .into_bytes())
}

fn unix_time() -> Result<i64, IdentityError> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| invalid("system clock precedes Unix epoch"))?
            .as_secs(),
    )
    .map_err(|_| invalid("system clock is out of range"))
}

fn generate_server_at(
    private_key: SigningKey,
    domain: String,
    now: i64,
) -> Result<Identity, IdentityError> {
    let key = private_key.verifying_key();
    let header = json!({"alg":"ES384", "x5u":encode_public_key(key)?});
    let claims = json!({"cpk":public_key_jwk(key), "iat":now, "exp":now.checked_add(60).ok_or_else(|| invalid("expiration overflow"))?});
    let token = sign_compact(
        &private_key,
        &header,
        &serde_json::to_vec(&claims).map_err(|_| invalid("encode token claims"))?,
        false,
    )?;
    Ok(Identity {
        private_key,
        token,
        domain,
    })
}

fn encode_public_key(key: &VerifyingKey) -> Result<String, IdentityError> {
    let public = PublicKey::from_sec1_bytes(key.to_encoded_point(false).as_bytes())
        .map_err(|_| invalid("invalid P-384 key"))?;
    Ok(STANDARD.encode(
        public
            .to_public_key_der()
            .map_err(|_| invalid("encode PKIX public key"))?
            .as_bytes(),
    ))
}

fn public_key_jwk(key: &VerifyingKey) -> Value {
    let point = key.to_encoded_point(false);
    let bytes = point.as_bytes();
    json!({"kty":"EC", "crv":"P-384", "x":URL_SAFE_NO_PAD.encode(&bytes[1..49]), "y":URL_SAFE_NO_PAD.encode(&bytes[49..97])})
}

fn parse_public_key(cpk: &Value) -> Result<VerifyingKey, IdentityError> {
    if let Some(encoded) = cpk.as_str() {
        let der = STANDARD
            .decode(encoded)
            .map_err(|_| invalid("invalid cpk base64"))?;
        let key = PublicKey::from_public_key_der(&der)
            .map_err(|_| invalid("cpk is not a P-384 PKIX public key"))?;
        return Ok(VerifyingKey::from(key));
    }
    if cpk.get("kty").and_then(Value::as_str) != Some("EC")
        || cpk.get("crv").and_then(Value::as_str) != Some("P-384")
        || cpk.get("d").is_some()
    {
        return Err(invalid(
            "cpk must be an EC P-384 public JWK or base64 PKIX public key",
        ));
    }
    let mut bytes = [0u8; 97];
    bytes[0] = 4;
    let x = URL_SAFE_NO_PAD
        .decode(
            cpk.get("x")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("missing JWK x"))?,
        )
        .map_err(|_| invalid("invalid JWK x"))?;
    let y = URL_SAFE_NO_PAD
        .decode(
            cpk.get("y")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("missing JWK y"))?,
        )
        .map_err(|_| invalid("invalid JWK y"))?;
    if x.len() != 48 || y.len() != 48 {
        return Err(invalid("P-384 coordinates must be 48 bytes"));
    }
    bytes[1..49].copy_from_slice(&x);
    bytes[49..].copy_from_slice(&y);
    VerifyingKey::from_sec1_bytes(&bytes).map_err(|_| invalid("invalid P-384 curve point"))
}

struct ParsedToken {
    claims: Value,
}

fn compact_parts(compact: &str) -> Result<(&str, &str, &str), IdentityError> {
    let mut parts = compact.split('.');
    let header = parts.next().unwrap_or_default();
    let payload = parts
        .next()
        .ok_or_else(|| invalid("malformed compact JWS"))?;
    let signature = parts
        .next()
        .ok_or_else(|| invalid("malformed compact JWS"))?;
    if header.is_empty() || signature.is_empty() || parts.next().is_some() {
        return Err(invalid("malformed compact JWS"));
    }
    Ok((header, payload, signature))
}

fn decode_header(encoded: &str) -> Result<Value, IdentityError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| invalid("invalid JWS header base64"))?;
    let header: Value =
        serde_json::from_slice(&bytes).map_err(|_| invalid("invalid JWS header JSON"))?;
    if !header.is_object() || header.get("crit").is_some() || header.get("b64").is_some() {
        return Err(invalid("unsupported JWS header"));
    }
    Ok(header)
}

fn parse_token(token: &str) -> Result<ParsedToken, IdentityError> {
    let (header, payload, signature) = compact_parts(token)?;
    let header = decode_header(header)?;
    if !matches!(
        header.get("alg").and_then(Value::as_str),
        Some("ES384" | "RS256")
    ) {
        return Err(invalid("unsupported identity JWT algorithm"));
    }
    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| invalid("invalid JWT signature base64"))?;
    if signature.is_empty() {
        return Err(invalid("missing JWT signature"));
    }
    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| invalid("invalid JWT claims base64"))?;
    let claims: Value =
        serde_json::from_slice(&payload).map_err(|_| invalid("invalid JWT claims JSON"))?;
    if !claims.is_object() {
        return Err(invalid("JWT claims must be an object"));
    }
    Ok(ParsedToken { claims })
}

fn validate_client_time(claims: &Value, now: i64) -> Result<(), IdentityError> {
    for name in ["exp", "iat", "nbf"] {
        if let Some(value) = claims.get(name).filter(|v| !v.is_null()) {
            // go-jose accepts fractional NumericDates and truncates toward zero.
            let date = value
                .as_f64()
                .filter(|n| n.is_finite() && *n >= i64::MIN as f64 && *n < i64::MAX as f64)
                .ok_or_else(|| invalid("invalid JWT temporal claim"))?
                as i64;
            let invalid_time = if name == "exp" {
                now.saturating_sub(60) > date
            } else {
                now.saturating_add(60) < date
            };
            if invalid_time {
                return Err(invalid(
                    "client identity JWT is outside its validity period",
                ));
            }
        }
    }
    Ok(())
}

fn sign_compact(
    key: &SigningKey,
    header: &Value,
    payload: &[u8],
    detached: bool,
) -> Result<String, IdentityError> {
    let header = URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(header).map_err(|_| invalid("encode JWS header"))?);
    let encoded_payload = URL_SAFE_NO_PAD.encode(payload);
    let input = format!("{header}.{encoded_payload}");
    let signature: Signature = key.sign(input.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(signature.to_bytes());
    Ok(if detached {
        format!("{header}..{signature}")
    } else {
        format!("{input}.{signature}")
    })
}

fn verify_compact(
    compact: &str,
    detached: Option<&[u8]>,
    key: &VerifyingKey,
) -> Result<(), IdentityError> {
    let (encoded_header, encoded_payload, signature) = compact_parts(compact)?;
    let header = decode_header(encoded_header)?;
    if header.get("alg").and_then(Value::as_str) != Some("ES384") {
        return Err(invalid("identity assertion must use ES384"));
    }
    let payload = match detached {
        Some(payload) => {
            if !encoded_payload.is_empty() {
                return Err(invalid("fingerprint JWS must be detached"));
            }
            URL_SAFE_NO_PAD.encode(payload)
        }
        None => encoded_payload.to_owned(),
    };
    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| invalid("invalid JWS signature base64"))?;
    let signature = Signature::from_slice(&signature)
        .map_err(|_| invalid("invalid ES384 signature encoding"))?;
    key.verify(format!("{encoded_header}.{payload}").as_bytes(), &signature)
        .map_err(|_| invalid("identity signature verification failed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub(super) const SDP: &str = "v=0\r\no=- 1 1 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\na=fingerprint:sha-256 00:11:22:33:44:55:66:77\r\nm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\n";

    fn key() -> SigningKey {
        SigningKey::from_slice(&[1; 48]).unwrap()
    }

    fn identity() -> Identity {
        generate_server_at(key(), "self".into(), 1_700_000_000).unwrap()
    }

    #[test]
    fn canonical_fingerprints_match_go_bytes() {
        assert_eq!(
            fingerprint_payload(SDP).unwrap(),
            br#"{"fingerprint":[{"algorithm":"sha-256","digest":"00:11:22:33:44:55:66:77"}]}"#
        );
    }

    #[tokio::test]
    async fn signed_server_sdp_round_trip_ignores_expired_token() {
        let identity = identity();
        let signed = identity.sign_sdp(SDP).unwrap();
        let verified = verify_sdp(&signed, PeerRole::Server, None).await.unwrap();
        assert_eq!(verified.domain, "self");
        assert_eq!(&verified.public_key, identity.private_key.verifying_key());
        assert!(signed.find("a=identity:").unwrap() < signed.find("m=application").unwrap());
        let token = parse_token(&identity.token).unwrap();
        assert_eq!(token.claims["exp"], 1_700_000_060);
        let header = decode_header(compact_parts(&identity.token).unwrap().0).unwrap();
        assert_eq!(
            header["x5u"],
            encode_public_key(identity.private_key.verifying_key()).unwrap()
        );
    }

    #[tokio::test]
    async fn fingerprint_tampering_is_rejected() {
        let signed = identity().sign_sdp(SDP).unwrap();
        assert!(
            verify_sdp(&signed.replace("00:11", "FF:FF"), PeerRole::Server, None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn default_client_policy_rejects_expired_token_and_missing_identity() {
        let signed = identity().sign_sdp(SDP).unwrap();
        assert!(verify_sdp(&signed, PeerRole::Client, None).await.is_err());
        assert!(verify_sdp(SDP, PeerRole::Server, None).await.is_err());
    }

    #[test]
    fn jwk_and_legacy_der_cpk_parse_to_same_key() {
        let key = key();
        let public = key.verifying_key();
        assert_eq!(parse_public_key(&public_key_jwk(public)).unwrap(), *public);
        assert_eq!(
            parse_public_key(&json!(encode_public_key(public).unwrap())).unwrap(),
            *public
        );
        for malformed in [
            json!(null),
            json!(1),
            json!("!!!"),
            json!("AAAA"),
            json!({"kty":"oct","k":"AAAA"}),
            json!({"kty":"EC","crv":"P-384","x":"AA","y":"AA"}),
        ] {
            assert!(parse_public_key(&malformed).is_err());
        }
    }

    #[test]
    fn client_temporal_claims_use_go_sixty_second_leeway() {
        assert!(validate_client_time(&json!({"exp":939}), 1000).is_err());
        assert!(validate_client_time(&json!({"iat":1061}), 1000).is_err());
        assert!(validate_client_time(&json!({"nbf":1061}), 1000).is_err());
        assert!(validate_client_time(&json!({"exp":940,"iat":1060,"nbf":1060}), 1000).is_ok());
        assert!(validate_client_time(&json!({"exp":"1000"}), 1000).is_err());
    }

    #[tokio::test]
    async fn wrong_token_key_and_unsigned_token_are_rejected() {
        let mut identity = identity();
        identity.private_key = SigningKey::from_slice(&[2; 48]).unwrap();
        let signed = identity.sign_sdp(SDP).unwrap();
        assert!(verify_sdp(&signed, PeerRole::Server, None).await.is_err());
        assert!(parse_token("eyJhbGciOiJub25lIn0.e30.").is_err());
    }

    #[test]
    fn conflicting_fingerprints_and_duplicate_identity_are_rejected() {
        let sdp = format!("{SDP}a=fingerprint:sha-384 AA:BB\r\n");
        assert!(fingerprint_payload(&sdp).is_err());
        let signed = identity().sign_sdp(SDP).unwrap();
        let attribute = signed
            .lines()
            .find(|line| line.starts_with("a=identity:"))
            .unwrap();
        let duplicate = signed.replace(attribute, &format!("{attribute}\r\n{attribute}"));
        assert!(extract_identity(&duplicate).is_err());
    }

    #[tokio::test]
    async fn native_whitespace_fingerprint_cannot_bypass_binding() {
        let original = format!("{SDP}a=fingerprint:sha-256 00:11:22:33:44:55:66:77\r\n");
        let signed = identity().sign_sdp(&original).unwrap();
        for prefix in ["a= fingerprint:", "a=\tfingerprint:"] {
            let forged = signed.replace(
                "a=fingerprint:sha-256 00:11:22:33:44:55:66:77",
                &format!("{prefix}sha-256 FF:FF\r\na=fingerprint:sha-256 00:11:22:33:44:55:66:77"),
            );
            assert!(verify_sdp(&forged, PeerRole::Server, None).await.is_err());
        }
        for prefix in ["a= identity:", "a=\tidentity:"] {
            assert!(
                verify_sdp(
                    &signed.replace("a=identity:", prefix),
                    PeerRole::Server,
                    None
                )
                .await
                .is_ok()
            );
        }
    }

    #[tokio::test]
    async fn bare_media_fingerprint_cannot_hide_attacker_session_fingerprint() {
        let original = format!("{SDP}a=fingerprint:sha-256 00:11:22:33:44:55:66:77\r\n");
        let signed = identity().sign_sdp(&original).unwrap();
        let forged = signed
            .replacen(
                "a=fingerprint:sha-256 00:11:22:33:44:55:66:77",
                "a=fingerprint:sha-256 FF:FF",
                1,
            )
            .replace(
                "m=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\n",
                "m=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\na=fingerprint\r\n",
            );
        assert!(verify_sdp(&forged, PeerRole::Server, None).await.is_err());
    }
}

#[cfg(test)]
mod authentication_tests {
    use super::*;

    struct ServiceVerifier(VerifyingKey);

    impl TokenVerifier for ServiceVerifier {
        fn verify<'a>(
            &'a self,
            token: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<(), IdentityError>> + Send + 'a>> {
            Box::pin(async move { verify_compact(token, None, &self.0) })
        }
    }

    #[tokio::test]
    async fn custom_server_verifier_can_reject_self_signed_token() {
        let identity = generate_server_at(
            SigningKey::from_slice(&[1; 48]).unwrap(),
            "self".into(),
            1000,
        )
        .unwrap();
        let sdp = identity
            .sign_sdp(&super::tests::SDP.replace("00:11:22:33:44:55:66:77", "AA:BB"))
            .unwrap();
        let other_key = SigningKey::from_slice(&[2; 48]).unwrap();
        let verifier = ServiceVerifier(*other_key.verifying_key());
        assert!(verify_sdp(&sdp, PeerRole::Server, None).await.is_ok());
        assert!(
            verify_sdp(&sdp, PeerRole::Server, Some(&verifier))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn externally_authenticated_client_binds_its_own_key() {
        let client_key = SigningKey::from_slice(&[1; 48]).unwrap();
        let service_key = SigningKey::from_slice(&[2; 48]).unwrap();
        let claims =
            serde_json::to_vec(&json!({"cpk":public_key_jwk(client_key.verifying_key())})).unwrap();
        let token = sign_compact(&service_key, &json!({"alg":"ES384"}), &claims, false).unwrap();
        let client = Identity {
            private_key: client_key,
            token,
            domain: "authorization.example".into(),
        };
        let sdp = client
            .sign_sdp(&super::tests::SDP.replace("00:11:22:33:44:55:66:77", "AA:BB"))
            .unwrap();
        let verifier = ServiceVerifier(*service_key.verifying_key());
        assert!(
            verify_sdp(&sdp, PeerRole::Client, Some(&verifier))
                .await
                .is_ok()
        );
        let proof = verify_sdp(&sdp, PeerRole::Client, None).await.unwrap();
        assert_eq!(proof.public_key, *client.private_key.verifying_key());
        assert!(
            verify_sdp(&sdp.replace("AA:BB", "FF:FF"), PeerRole::Client, None)
                .await
                .is_err()
        );
        assert!(
            verify_sdp(&sdp, PeerRole::Server, Some(&verifier))
                .await
                .is_ok()
        );
        assert!(
            verify_sdp(
                &sdp.replace("AA:BB", "FF:FF"),
                PeerRole::Server,
                Some(&verifier)
            )
            .await
            .is_err()
        );
        let wrong_service = ServiceVerifier(*client.private_key.verifying_key());
        assert!(
            verify_sdp(&sdp, PeerRole::Client, Some(&wrong_service))
                .await
                .is_err()
        );
        assert!(verify_sdp(&sdp, PeerRole::Server, None).await.is_err());
    }

    #[test]
    fn modified_token_claims_do_not_verify() {
        let key = SigningKey::from_slice(&[1; 48]).unwrap();
        let original = generate_server_at(key, "self".into(), 1000).unwrap();
        let (header, _, signature) = compact_parts(&original.token).unwrap();
        let forged_claims = URL_SAFE_NO_PAD.encode(br#"{"cpk":null,"iat":1000}"#);
        let forged = format!("{header}.{forged_claims}.{signature}");
        assert!(verify_compact(&forged, None, original.private_key.verifying_key()).is_err());
    }

    #[tokio::test]
    async fn server_ignores_future_issued_at_and_not_before() {
        let key = SigningKey::from_slice(&[1; 48]).unwrap();
        let claims = serde_json::to_vec(&json!({
            "cpk":public_key_jwk(key.verifying_key()),
            "exp":1, "iat":4_000_000_000u64, "nbf":4_000_000_000u64,
        }))
        .unwrap();
        let token = sign_compact(&key, &json!({"alg":"ES384"}), &claims, false).unwrap();
        let identity = Identity {
            private_key: key,
            token,
            domain: "self".into(),
        };
        let sdp = identity
            .sign_sdp(&super::tests::SDP.replace("00:11:22:33:44:55:66:77", "AA:BB"))
            .unwrap();
        assert!(verify_sdp(&sdp, PeerRole::Server, None).await.is_ok());
    }
}

#[cfg(test)]
mod optional_identity_tests {
    use super::*;

    struct Reject;
    impl TokenVerifier for Reject {
        fn verify<'a>(
            &'a self,
            _: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<(), IdentityError>> + Send + 'a>> {
            Box::pin(async { Err(invalid("rejected by configured verifier")) })
        }
    }

    #[tokio::test]
    async fn optional_identity_only_allows_genuinely_absent_native_attribute() {
        let identity = generate_server_at(
            SigningKey::from_slice(&[1; 48]).unwrap(),
            "self".into(),
            1000,
        )
        .unwrap();
        let signed = identity.sign_sdp(super::tests::SDP).unwrap();
        for prefix in ["a=identity:", "a= identity:", "a=\tidentity:"] {
            let sdp = signed.replace("a=identity:", prefix);
            assert!(
                verify_sdp_optional(&sdp, PeerRole::Server, None, true)
                    .await
                    .unwrap()
                    .is_some()
            );
            assert!(
                verify_sdp_optional(&sdp, PeerRole::Server, Some(&Reject), true)
                    .await
                    .is_err()
            );
        }
        assert!(
            verify_sdp_optional(super::tests::SDP, PeerRole::Server, Some(&Reject), true)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            verify_sdp_optional(super::tests::SDP, PeerRole::Server, None, false)
                .await
                .is_err()
        );
        let malformed = format!("{}a= identity\r\n", super::tests::SDP);
        assert!(
            verify_sdp_optional(&malformed, PeerRole::Server, None, true)
                .await
                .is_err()
        );
    }
}
