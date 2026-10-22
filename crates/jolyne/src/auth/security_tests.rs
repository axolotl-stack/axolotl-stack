use super::*;
use crate::auth::client_data::tests::valid as valid_client_data;
use crate::auth::openid::{KeySetSource, OpenIdProvider, OpenIdVerifier, RefreshPolicy};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use futures::future::BoxFuture;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use p384::elliptic_curve::sec1::ToEncodedPoint;
use p384::{
    SecretKey,
    pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding},
};
use serde_json::{Value, json};

pub(super) fn key(n: u8) -> SecretKey {
    SecretKey::from_slice(&[n; 48]).unwrap()
}
pub(super) fn public(key: &SecretKey) -> String {
    STANDARD.encode(key.public_key().to_public_key_der().unwrap().as_bytes())
}
pub(super) fn sign_with(key: &SecretKey, kid: Option<&str>, claims: &Value) -> String {
    let mut header = Header::new(Algorithm::ES384);
    header.kid = kid.map(Into::into);
    // The exploit used raw SEC1 header keys; valid identity claims use SPKI.
    header.x5u = Some(STANDARD.encode(key.public_key().to_encoded_point(false).as_bytes()));
    let pem = key.to_pkcs8_pem(LineEnding::LF).unwrap();
    encode(
        &header,
        claims,
        &EncodingKey::from_ec_pem(pem.as_bytes()).unwrap(),
    )
    .unwrap()
}
pub(super) fn sign(key: &SecretKey, claims: &Value) -> String {
    sign_with(key, None, claims)
}
pub(super) fn claims(key: &SecretKey) -> Value {
    let now = jsonwebtoken::get_current_timestamp();
    json!({"cpk":public(key),"xid":"2535428123456789","xname":"Player","mid":"ABCDEF0123456789","tid":"20CA2","ipt":"PlayFab","iss":"https://trusted.test/","aud":"api://auth-minecraft-services/multiplayer","exp":now+3600,"nbf":now-60})
}
/// Valid client data bound to `key`.
pub(super) fn client_data(key: &SecretKey) -> String {
    let mut data = serde_json::to_value(valid_client_data()).unwrap();
    data["IdentityPublicKey"] = public(key).into();
    sign(key, &data)
}
pub(super) fn jwks(keys: &[(&SecretKey, Option<&str>)]) -> String {
    let keys: Vec<Value> = keys
        .iter()
        .map(|(key, kid)| {
            let point = key.public_key().to_encoded_point(false);
            let mut jwk = json!({"kty":"EC","crv":"P-384","alg":"ES384","use":"sig","x":URL_SAFE_NO_PAD.encode(point.x().unwrap()),"y":URL_SAFE_NO_PAD.encode(point.y().unwrap())});
            if let Some(kid) = kid {
                jwk["kid"] = (*kid).into();
            }
            jwk
        })
        .collect();
    json!({ "keys": keys }).to_string()
}
pub(super) fn verifier_for(keys: &[(&SecretKey, Option<&str>)]) -> OpenIdVerifier {
    OpenIdVerifier::from_jwks("https://trusted.test/", &[Algorithm::ES384], &jwks(keys)).unwrap()
}
fn verifier(key: &SecretKey) -> OpenIdVerifier {
    verifier_for(&[(key, None)])
}

/// Fixed trusted key set; never refreshes to anything else.
struct StaticSource(SecretKey);
impl KeySetSource for StaticSource {
    fn fetch(&self) -> BoxFuture<'_, Result<OpenIdVerifier, AuthError>> {
        Box::pin(async move { Ok(verifier(&self.0)) })
    }
}
fn provider(key: &SecretKey) -> OpenIdProvider {
    OpenIdProvider::new(StaticSource(key.clone()), RefreshPolicy::default())
}

#[test]
fn self_signed_header_key_is_not_online_trust() {
    let attacker = key(1);
    let token = sign(&attacker, &claims(&attacker));
    assert!(
        verifier(&key(2))
            .validate(&token, &client_data(&attacker))
            .is_err()
    );
}

#[test]
fn trusted_provider_binds_client_data_to_cpk() {
    let provider = key(2);
    let identity = key(1);
    let token = sign(&provider, &claims(&identity));
    let login = verifier(&provider)
        .validate(&token, &client_data(&identity))
        .unwrap();
    assert!(login.identity.is_authenticated());
    assert_eq!(login.identity.xuid(), Some("2535428123456789"));
    assert_eq!(login.identity.identity_public_key(), public(&identity));
    assert_eq!(login.client_data.device_os, 1);
    // Client data signed by another key, or echoing another key, is rejected.
    let forged = client_data(&provider);
    assert!(verifier(&provider).validate(&token, &forged).is_err());
    let mut mismatch = serde_json::to_value(valid_client_data()).unwrap();
    mismatch["IdentityPublicKey"] = public(&provider).into();
    let mismatch = sign(&identity, &mismatch);
    assert!(verifier(&provider).validate(&token, &mismatch).is_err());
}

#[test]
fn trusted_signatures_do_not_override_claim_validation() {
    let provider = key(2);
    let identity = key(1);
    let client = client_data(&identity);
    for (name, value) in [
        ("iss", json!("https://evil.test/")),
        ("aud", json!("wrong")),
        ("exp", json!(1)),
        ("nbf", json!(jsonwebtoken::get_current_timestamp() + 3600)),
    ] {
        let mut payload = claims(&identity);
        payload[name] = value;
        let error = verifier(&provider)
            .validate(&sign(&provider, &payload), &client)
            .unwrap_err();
        match name {
            "iss" => assert!(matches!(error, AuthError::InvalidIssuer(_))),
            "aud" => assert!(matches!(error, AuthError::InvalidAudience(_))),
            _ => assert_eq!(error, AuthError::TemporalValidation),
        }
    }
    for name in ["iss", "aud", "exp", "nbf", "cpk"] {
        let mut payload = claims(&identity);
        payload.as_object_mut().unwrap().remove(name);
        assert!(
            verifier(&provider)
                .validate(&sign(&provider, &payload), &client)
                .is_err(),
            "missing {name}"
        );
    }
}

#[tokio::test]
async fn online_identity_carries_playfab_and_title_ids() {
    let provider_key = key(2);
    let identity = key(1);
    let auth = json!({"AuthenticationType":0,"Token":sign(&provider_key, &claims(&identity))});
    let login = authenticate_login_with(
        &provider(&provider_key),
        &auth.to_string(),
        &client_data(&identity),
        true,
        false,
    )
    .await
    .unwrap();
    assert!(login.identity.is_authenticated());
    assert_eq!(login.identity.playfab_id(), Some("ABCDEF0123456789"));
    assert_eq!(login.identity.title_id(), Some("20CA2"));
    assert_eq!(login.identity.display_name(), "Player");
}

#[tokio::test]
async fn online_token_without_xuid_is_not_authenticated() {
    let provider_key = key(2);
    let identity = key(1);
    let mut payload = claims(&identity);
    payload["xid"] = "".into();
    let auth = json!({"AuthenticationType":0,"Token":sign(&provider_key, &payload)});
    let err = authenticate_login_with(
        &provider(&provider_key),
        &auth.to_string(),
        &client_data(&identity),
        true,
        false,
    )
    .await
    .unwrap_err();
    assert_eq!(err, AuthError::NotAuthenticated);
}

#[tokio::test]
async fn signed_but_invalid_client_data_is_rejected() {
    let provider_key = key(2);
    let identity = key(1);
    let mut data = serde_json::to_value(valid_client_data()).unwrap();
    data["DeviceOS"] = 0.into();
    let auth = json!({"AuthenticationType":0,"Token":sign(&provider_key, &claims(&identity))});
    let err = authenticate_login_with(
        &provider(&provider_key),
        &auth.to_string(),
        &sign(&identity, &data),
        true,
        false,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, AuthError::InvalidClientData(_)));
}

#[tokio::test]
async fn offline_token_is_accepted_but_never_authenticated() {
    let identity = key(1);
    // An offline client claims somebody's XUID and PlayFab IDs.
    let token = sign(&identity, &claims(&identity));
    let auth = json!({"AuthenticationType":2,"Token":token});
    let login = authenticate_login(&auth.to_string(), &client_data(&identity), false, true)
        .await
        .unwrap();
    assert!(!login.identity.is_authenticated());
    assert_eq!(login.identity.trust(), Trust::Unauthenticated);
    assert_eq!(login.identity.xuid(), None);
    assert_eq!(login.identity.playfab_id(), None);
    assert_eq!(login.identity.title_id(), None);
    assert_eq!(login.identity.claimed_xuid(), Some("2535428123456789"));
    assert_eq!(login.identity.identity_public_key(), public(&identity));
}

#[tokio::test]
async fn offline_type_zero_token_is_also_unauthenticated() {
    let identity = key(1);
    let auth = json!({"AuthenticationType":0,"Token":sign(&identity, &claims(&identity))});
    let login = authenticate_login(&auth.to_string(), &client_data(&identity), false, false)
        .await
        .unwrap();
    assert!(!login.identity.is_authenticated());
    assert_eq!(login.identity.xuid(), None);
}

#[tokio::test]
async fn legacy_self_signed_chain_is_offline_only() {
    let attacker = key(1);
    let now = jsonwebtoken::get_current_timestamp();
    let link = sign(
        &attacker,
        &json!({"identityPublicKey":public(&attacker),"extraData":{"XUID":"123","displayName":"Forged","identity":"e401efb1-4838-328e-9d95-f0821e340749"},"exp":now+3600,"nbf":now-60}),
    );
    let auth = json!({"AuthenticationType":2,"Certificate":json!({"chain":[link]}).to_string()})
        .to_string();
    let client = client_data(&attacker);
    assert_eq!(
        authenticate_login_with(&provider(&key(2)), &auth, &client, true, true)
            .await
            .unwrap_err(),
        AuthError::LegacyAuthDisabled
    );
    let login = authenticate_login(&auth, &client, false, true)
        .await
        .unwrap();
    assert!(!login.identity.is_authenticated());
    assert_eq!(login.identity.xuid(), None);
}

/// Client output must be accepted by the server path it targets.
#[cfg(feature = "client")]
mod client_server {
    use super::*;
    use crate::auth::client::{LoginCredentials, MultiplayerTokenSource, build_login};
    use std::sync::Arc;

    /// Test "PlayFab": signs a multiplayer token for whatever key it is given.
    struct TestIssuer(SecretKey);
    impl MultiplayerTokenSource for TestIssuer {
        fn multiplayer_token<'a>(
            &'a self,
            public_key_der: &'a [u8],
        ) -> BoxFuture<'a, Result<String, AuthError>> {
            let mut payload = claims(&key(9));
            payload["cpk"] = STANDARD.encode(public_key_der).into();
            let token = sign(&self.0, &payload);
            Box::pin(async move { Ok(token) })
        }
    }

    #[tokio::test]
    async fn online_client_login_is_accepted_by_online_server() {
        let provider_key = key(2);
        let connection_key = key(5);
        let credentials = LoginCredentials::Online(Arc::new(TestIssuer(provider_key.clone())));
        let login = build_login(&connection_key, "Player", uuid::Uuid::nil(), &credentials)
            .await
            .unwrap();
        let request = crate::auth::client::encode_login_request(
            &login.auth_info_json,
            &login.client_data_jwt,
        );
        let (auth, client) = decode_login_request(&request).unwrap();
        let accepted =
            authenticate_login_with(&provider(&provider_key), &auth, &client, true, false)
                .await
                .unwrap();
        assert!(accepted.identity.is_authenticated());
        assert_eq!(
            accepted.identity.identity_public_key(),
            public(&connection_key)
        );
    }

    #[tokio::test]
    async fn offline_client_login_is_rejected_online_and_accepted_offline() {
        let connection_key = key(5);
        let login = build_login(
            &connection_key,
            "Player",
            uuid::Uuid::from_u128(5),
            &LoginCredentials::Offline,
        )
        .await
        .unwrap();
        assert_eq!(
            authenticate_login_with(
                &provider(&key(2)),
                &login.auth_info_json,
                &login.client_data_jwt,
                true,
                true
            )
            .await
            .unwrap_err(),
            AuthError::LegacyAuthDisabled
        );
        let accepted =
            authenticate_login(&login.auth_info_json, &login.client_data_jwt, false, true)
                .await
                .unwrap();
        assert!(!accepted.identity.is_authenticated());
        assert_eq!(accepted.identity.uuid(), uuid::Uuid::from_u128(5));
    }
}

/// Deterministic xorshift for fuzz-style inputs.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn mutate(rng: &mut Rng, input: &[u8]) -> Vec<u8> {
    let mut out = input.to_vec();
    for _ in 0..=rng.below(4) {
        match rng.below(4) {
            0 if !out.is_empty() => {
                let i = rng.below(out.len());
                out[i] = rng.next() as u8;
            }
            1 if !out.is_empty() => {
                let i = rng.below(out.len());
                out.truncate(i);
            }
            2 => {
                let i = rng.below(out.len() + 1);
                out.insert(i, rng.next() as u8);
            }
            _ if out.len() >= 4 => {
                // Overwrite a length prefix with an extreme value.
                let at = if rng.below(2) == 0 {
                    0
                } else {
                    rng.below(out.len() - 3)
                };
                let value = [i32::MIN, -1, 0, i32::MAX, out.len() as i32][rng.below(5)];
                out[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
            _ => {}
        }
    }
    out
}

#[test]
fn malformed_envelopes_never_panic() {
    let valid = {
        let mut request = Vec::new();
        let auth = br#"{"AuthenticationType":0,"Token":"a.b.c"}"#;
        request.extend_from_slice(&(auth.len() as i32).to_le_bytes());
        request.extend_from_slice(auth);
        request.extend_from_slice(&5i32.to_le_bytes());
        request.extend_from_slice(b"x.y.z");
        request
    };
    assert!(decode_login_request(&valid).is_ok());
    for (bytes, name) in [
        (&[][..], "empty"),
        (&[1, 0, 0][..], "short prefix"),
        (&[0xff, 0xff, 0xff, 0xff][..], "negative length"),
        (&[0xff, 0xff, 0xff, 0x7f, 0][..], "huge length"),
        (&[0, 0, 0, 0, 0, 0, 0, 0, 0][..], "trailing byte"),
        (
            &[0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff][..],
            "negative client length",
        ),
        (&[1, 0, 0, 0, 0xff, 0, 0, 0, 0][..], "invalid utf-8"),
    ] {
        assert!(decode_login_request(bytes).is_err(), "{name}");
    }
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..20_000 {
        let _ = decode_login_request(&mutate(&mut rng, &valid));
    }
}

#[tokio::test]
async fn malformed_tokens_and_client_data_never_panic() {
    let provider_key = key(2);
    let identity = key(1);
    let provider = provider(&provider_key);
    let token = sign(&provider_key, &claims(&identity));
    let client = client_data(&identity);
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let mut inputs: Vec<(String, String)> = vec![
        (String::new(), String::new()),
        ("...".into(), "...".into()),
        ("a.b".into(), client.clone()),
        (format!("{token}.extra"), client.clone()),
        (token.clone(), "eyJhbGciOiJub25lIn0.e30.".into()),
        (
            "eyJhbGciOiJFUzM4NCJ9.bm90IGpzb24.c2ln".into(),
            client.clone(),
        ),
        ("eyJhbGciOiJFUzM4NCJ9.W10.c2ln".into(), client.clone()),
    ];
    for _ in 0..400 {
        let token = String::from_utf8_lossy(&mutate(&mut rng, token.as_bytes())).into_owned();
        let client = String::from_utf8_lossy(&mutate(&mut rng, client.as_bytes())).into_owned();
        inputs.push((token, client));
    }
    for (token, client) in &inputs {
        for auth_type in [0, 1, 2] {
            let auth = json!({"AuthenticationType":auth_type,"Token":token}).to_string();
            for (online, legacy) in [(true, false), (false, true), (false, false)] {
                let _ = authenticate_login_with(&provider, &auth, client, online, legacy).await;
            }
        }
        let chain =
            json!({"AuthenticationType":2,"Certificate":json!({"chain":[token]}).to_string()});
        let _ = authenticate_login_with(&provider, &chain.to_string(), client, false, true).await;
    }
    for auth in [
        "",
        "null",
        "[]",
        r#"{"AuthenticationType":-1}"#,
        r#"{"AuthenticationType":"0"}"#,
        r#"{"AuthenticationType":2,"Certificate":7}"#,
        r#"{"AuthenticationType":2,"Certificate":"{\"chain\":[1,2]}"}"#,
        r#"{"AuthenticationType":2,"chain":[""]}"#,
        r#"{"AuthenticationType":2,"Certificate":{"chain":[]}}"#,
    ] {
        assert!(
            authenticate_login_with(&provider, auth, &client, false, true)
                .await
                .is_err(),
            "{auth}"
        );
    }
}
