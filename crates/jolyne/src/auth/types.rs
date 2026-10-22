use crate::auth::client_data::ClientData;
use crate::error::AuthError;
use md5::{Digest, Md5};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthenticationType {
    Full = 0,
    Guest = 1,
    SelfSigned = 2,
}

impl TryFrom<u32> for AuthenticationType {
    type Error = AuthError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(AuthenticationType::Full),
            1 => Ok(AuthenticationType::Guest),
            2 => Ok(AuthenticationType::SelfSigned),
            other => Err(AuthError::UnsupportedAuthType(other)),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AuthInfo {
    #[serde(rename = "AuthenticationType")]
    pub authentication_type: Option<u32>,
    #[serde(rename = "Token")]
    pub token: Option<String>,
    #[serde(rename = "Certificate")]
    pub certificate: Option<serde_json::Value>,
    // Some clients may still send legacy "chain" shape in root.
    pub chain: Option<serde_json::Value>,
}

/// Legacy chain `extraData` (gophertunnel `login.IdentityData` JSON shape).
#[derive(Debug, Deserialize, Clone)]
pub struct ExtraDataClaims {
    #[serde(rename = "XUID")]
    pub xuid: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "identity")]
    pub uuid: Option<String>,
}

/// Multiplayer token claims (gophertunnel `login.tokenClaims`).
#[derive(Debug, Deserialize)]
pub struct OpenIdClaims {
    /// Xbox Live user ID; empty for unauthenticated identities.
    #[serde(rename = "xid")]
    pub xuid: Option<String>,
    #[serde(rename = "xname")]
    pub xbox_username: Option<String>,
    /// Client public key bound to the token; signs client data and keys encryption.
    #[serde(rename = "cpk", alias = "identityPublicKey")]
    pub identity_public_key: Option<String>,
    /// PlayFab master player account ID.
    #[serde(rename = "mid")]
    pub playfab_id: Option<String>,
    /// PlayFab title ID (`20CA2` for retail).
    #[serde(rename = "tid")]
    pub title_id: Option<String>,
    /// Player UUID; only present when it cannot be derived from the XUID.
    #[serde(rename = "leguuid")]
    pub legacy_uuid: Option<String>,
    /// Pre-OpenID tokens nested identity under `extraData`.
    #[serde(rename = "extraData")]
    pub extra_data: Option<ExtraDataClaims>,
}

/// Claims of a legacy self-signed chain link.
#[derive(Debug, Deserialize)]
pub struct ChainClaims {
    #[serde(rename = "identityPublicKey")]
    pub identity_public_key: Option<String>,
    #[serde(rename = "extraData")]
    pub extra_data: Option<ExtraDataClaims>,
}

/// How an identity was established. Only the crate can produce
/// [`Trust::Authenticated`]: it requires a provider-verified token *and* proof
/// that the client holds the token's key (the encryption handshake, or a
/// transport that proved the key, see `Transport::proven_peer_key`).
/// [`ValidatedIdentity`] has no public constructor or mutable fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// Provider-signed multiplayer token verified against the trusted JWKS,
    /// and the client proved possession of its key.
    Authenticated,
    /// Provider-signed token verified, but key possession is not yet proven:
    /// a captured `Login` could be a replay. Account accessors stay `None`;
    /// [`ValidatedIdentity::claimed_xuid`] may be used to *refuse* a player
    /// (e.g. in an admission hook), never to grant anything.
    AwaitingKeyProof,
    /// Offline login, or an online login whose key was never proven (Bedrock
    /// encryption off and no transport proof): every claim is client-chosen.
    Unauthenticated,
}

/// Identity extracted from a login request.
///
/// Fields are private so an unauthenticated identity cannot be upgraded or
/// edited after verification. [`ValidatedIdentity::xuid`],
/// [`ValidatedIdentity::playfab_id`] and [`ValidatedIdentity::title_id`] are
/// `None` for offline identities; the raw offline XUID claim is only available
/// through [`ValidatedIdentity::claimed_xuid`] and must never be trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedIdentity {
    display_name: String,
    uuid: Uuid,
    identity_public_key: String,
    claimed_xuid: Option<String>,
    playfab_id: Option<String>,
    title_id: Option<String>,
    trust: Trust,
}

impl ValidatedIdentity {
    /// Builds an identity after applying gophertunnel `IdentityData.Validate`.
    ///
    /// The UUID falls back like gophertunnel `tokenClaims.identityData`: the
    /// XUID-derived v3 UUID, or a random one when neither claim exists.
    pub(crate) fn new(
        claims: IdentityClaims,
        identity_public_key: String,
        trust: Trust,
    ) -> Result<Self, AuthError> {
        let IdentityClaims {
            xuid,
            display_name,
            uuid,
            playfab_id,
            title_id,
        } = claims;
        let xuid = xuid.filter(|xuid| !xuid.is_empty());
        let token_verified = trust != Trust::Unauthenticated;
        if token_verified && xuid.is_none() {
            return Err(AuthError::NotAuthenticated);
        }
        if identity_public_key.is_empty() {
            return Err(AuthError::MissingIdentityKey);
        }
        if xuid
            .as_deref()
            .is_some_and(|xuid| xuid.parse::<i64>().is_err())
        {
            return Err(AuthError::InvalidIdentityData("XUID is not an int64"));
        }
        let uuid = match uuid.filter(|uuid| !uuid.is_empty()) {
            Some(uuid) => Uuid::parse_str(&uuid)
                .ok()
                .filter(|uuid| !uuid.is_nil())
                .ok_or(AuthError::InvalidIdentityData("identity is not a UUID"))?,
            None => match &xuid {
                Some(xuid) => uuid_from_xuid(xuid),
                None => Uuid::new_v4(),
            },
        };
        let display_name = display_name.unwrap_or_default();
        validate_display_name(&display_name, xuid.is_some())?;
        Ok(Self {
            display_name,
            uuid,
            identity_public_key,
            claimed_xuid: xuid,
            playfab_id: playfab_id.filter(|id| token_verified && !id.is_empty()),
            title_id: title_id.filter(|id| token_verified && !id.is_empty()),
            trust,
        })
    }

    /// Records that the client proved it holds the identity key: a
    /// provider-verified identity becomes [`Trust::Authenticated`].
    pub(crate) fn confirm_key_possession(&mut self) {
        if self.trust == Trust::AwaitingKeyProof {
            self.trust = Trust::Authenticated;
        }
    }

    /// Key possession can no longer be proven (encryption off, no transport
    /// proof): the identity is treated as offline.
    pub(crate) fn forfeit_key_proof(&mut self) {
        if self.trust == Trust::AwaitingKeyProof {
            self.trust = Trust::Unauthenticated;
            self.playfab_id = None;
            self.title_id = None;
        }
    }

    /// Whether this identity's key is the given transport-proven key.
    pub(crate) fn key_matches(&self, key: &p384::PublicKey) -> bool {
        crate::proto::handshake::parse_public_key(&self.identity_public_key)
            .is_ok_and(|own| own == *key)
    }

    /// Whether a trusted provider verified this identity and the client
    /// proved it holds the identity key.
    pub fn is_authenticated(&self) -> bool {
        self.trust == Trust::Authenticated
    }

    /// How this identity was established.
    pub fn trust(&self) -> Trust {
        self.trust
    }

    /// Display name (Xbox gamertag when authenticated, client-chosen otherwise).
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Player UUID.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Base64 SPKI P-384 key that signed the client data; pass it to the
    /// encryption handshake.
    pub fn identity_public_key(&self) -> &str {
        &self.identity_public_key
    }

    /// Xbox Live user ID, only for authenticated identities.
    pub fn xuid(&self) -> Option<&str> {
        self.claimed_xuid
            .as_deref()
            .filter(|_| self.is_authenticated())
    }

    /// The XUID claim as sent. Untrusted unless [`Self::is_authenticated`].
    pub fn claimed_xuid(&self) -> Option<&str> {
        self.claimed_xuid.as_deref()
    }

    /// PlayFab master player account ID (`mid`), only when authenticated.
    pub fn playfab_id(&self) -> Option<&str> {
        self.playfab_id
            .as_deref()
            .filter(|_| self.is_authenticated())
    }

    /// PlayFab title ID (`tid`, `20CA2` for retail), only when authenticated.
    pub fn title_id(&self) -> Option<&str> {
        self.title_id.as_deref().filter(|_| self.is_authenticated())
    }
}

/// Identity claims common to multiplayer tokens and legacy chains.
#[derive(Debug, Default)]
pub(crate) struct IdentityClaims {
    pub xuid: Option<String>,
    pub display_name: Option<String>,
    pub uuid: Option<String>,
    pub playfab_id: Option<String>,
    pub title_id: Option<String>,
}

impl From<OpenIdClaims> for IdentityClaims {
    fn from(claims: OpenIdClaims) -> Self {
        let extra = claims.extra_data;
        Self {
            xuid: claims
                .xuid
                .or_else(|| extra.as_ref().and_then(|e| e.xuid.clone())),
            display_name: claims
                .xbox_username
                .or_else(|| extra.as_ref().and_then(|e| e.display_name.clone())),
            uuid: claims
                .legacy_uuid
                .or_else(|| extra.as_ref().and_then(|e| e.uuid.clone())),
            playfab_id: claims.playfab_id,
            title_id: claims.title_id,
        }
    }
}

impl From<ExtraDataClaims> for IdentityClaims {
    fn from(extra: ExtraDataClaims) -> Self {
        Self {
            xuid: extra.xuid,
            display_name: extra.display_name,
            uuid: extra.uuid,
            playfab_id: None,
            title_id: None,
        }
    }
}

/// A login whose provider token (online) or claimed key (offline) signed the
/// client data. Check [`ValidatedIdentity::is_authenticated`] before trusting
/// account claims.
#[derive(Debug, Clone)]
pub struct AuthenticatedLogin {
    pub identity: ValidatedIdentity,
    pub client_data: ClientData,
}

/// gophertunnel `login.identityFromXUID`: MD5 v3 UUID of
/// `pocket-auth-1-xuid:<xuid>` (no namespace bytes).
fn uuid_from_xuid(xuid: &str) -> Uuid {
    let digest = Md5::new()
        .chain_update(b"pocket-auth-1-xuid:")
        .chain_update(xuid.as_bytes())
        .finalize();
    let mut bytes: [u8; 16] = digest.into();
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

/// gophertunnel `IdentityData.Validate` display-name rules. Lengths are UTF-8
/// bytes as in Go. The Go character-class regexes are unanchored (they only
/// require one matching character), so they are not reproduced.
fn validate_display_name(name: &str, has_xuid: bool) -> Result<(), AuthError> {
    let limit = if has_xuid { 15 } else { 16 };
    if name.is_empty() || name.len() > limit {
        return Err(AuthError::InvalidIdentityData(
            "displayName is empty or too long",
        ));
    }
    if name.starts_with(' ') || name.ends_with(' ') {
        return Err(AuthError::InvalidIdentityData(
            "displayName starts or ends with a space",
        ));
    }
    if name.as_bytes()[0].is_ascii_digit() {
        return Err(AuthError::InvalidIdentityData(
            "displayName starts with a number",
        ));
    }
    if name.contains("  ") {
        return Err(AuthError::InvalidIdentityData(
            "displayName contains consecutive spaces",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(xuid: &str, name: &str, uuid: Option<&str>) -> IdentityClaims {
        IdentityClaims {
            xuid: Some(xuid.into()),
            display_name: Some(name.into()),
            uuid: uuid.map(Into::into),
            playfab_id: Some("abc".into()),
            title_id: Some("20CA2".into()),
        }
    }

    #[test]
    fn uuid_is_derived_from_xuid_like_gophertunnel() {
        // gophertunnel identityFromXUID("2535428123456789").
        assert_eq!(
            uuid_from_xuid("2535428123456789").to_string(),
            "e401efb1-4838-328e-9d95-f0821e340749"
        );
        let identity = ValidatedIdentity::new(
            claims("2535428123456789", "Steve", None),
            "key".into(),
            Trust::Authenticated,
        )
        .unwrap();
        assert_eq!(identity.uuid(), uuid_from_xuid("2535428123456789"));
    }

    #[test]
    fn offline_identity_never_exposes_trusted_account_ids() {
        let identity = ValidatedIdentity::new(
            claims("2535428123456789", "Steve", None),
            "key".into(),
            Trust::Unauthenticated,
        )
        .unwrap();
        assert!(!identity.is_authenticated());
        assert_eq!(identity.xuid(), None);
        assert_eq!(identity.playfab_id(), None);
        assert_eq!(identity.title_id(), None);
        assert_eq!(identity.claimed_xuid(), Some("2535428123456789"));
    }

    #[test]
    fn authenticated_identity_requires_an_xuid() {
        let err = ValidatedIdentity::new(
            IdentityClaims {
                display_name: Some("Steve".into()),
                ..Default::default()
            },
            "key".into(),
            Trust::Authenticated,
        )
        .unwrap_err();
        assert_eq!(err, AuthError::NotAuthenticated);
    }

    #[test]
    fn identity_data_rules_match_gophertunnel() {
        let ok_uuid = Some("8dc5e4ba-75c0-37d3-a0a5-2bdfb2d1bd0d");
        for (xuid, name, uuid, valid) in [
            ("1", "Steve", ok_uuid, true),
            ("", "SixteenCharsName", ok_uuid, true),
            ("1", "SixteenCharsName", ok_uuid, false),
            ("1", "", ok_uuid, false),
            ("1", " Steve", ok_uuid, false),
            ("1", "Steve ", ok_uuid, false),
            ("1", "9Steve", ok_uuid, false),
            ("1", "St  eve", ok_uuid, false),
            ("not-a-number", "Steve", ok_uuid, false),
            ("1", "Steve", Some("not-a-uuid"), false),
            (
                "1",
                "Steve",
                Some("00000000-0000-0000-0000-000000000000"),
                false,
            ),
        ] {
            let result = ValidatedIdentity::new(
                claims(xuid, name, uuid),
                "key".into(),
                Trust::Unauthenticated,
            );
            assert_eq!(result.is_ok(), valid, "{xuid:?} {name:?} {uuid:?}");
        }
    }
}
