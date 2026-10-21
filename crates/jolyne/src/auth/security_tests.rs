use super::*;
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use p384::elliptic_curve::sec1::ToEncodedPoint;
use p384::{
    SecretKey,
    pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding},
};
use serde_json::{Value, json};

fn key(n: u8) -> SecretKey {
    SecretKey::from_slice(&[n; 48]).unwrap()
}
fn public(key: &SecretKey) -> String {
    STANDARD.encode(key.public_key().to_public_key_der().unwrap().as_bytes())
}
fn sign(key: &SecretKey, claims: &Value) -> String {
    let mut header = Header::new(Algorithm::ES384);
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
fn claims(key: &SecretKey) -> Value {
    let now = jsonwebtoken::get_current_timestamp();
    json!({"cpk":public(key),"xid":"123","xname":"Player","iss":"https://trusted.test/","aud":"api://auth-minecraft-services/multiplayer","exp":now+3600,"nbf":now-60})
}
fn verifier(key: &SecretKey) -> openid::OpenIdVerifier {
    let point = key.public_key().to_encoded_point(false);
    let jwks = json!({"keys":[{"kty":"EC","crv":"P-384","alg":"ES384","use":"sig","x":URL_SAFE_NO_PAD.encode(point.x().unwrap()),"y":URL_SAFE_NO_PAD.encode(point.y().unwrap())}]});
    openid::OpenIdVerifier::from_jwks(
        "https://trusted.test/",
        &[Algorithm::ES384],
        &jwks.to_string(),
    )
    .unwrap()
}
#[test]
fn self_signed_header_key_is_not_online_trust() {
    let attacker = key(1);
    let token = sign(&attacker, &claims(&attacker));
    let client = sign(&attacker, &json!({"IdentityPublicKey":public(&attacker)}));
    assert!(verifier(&key(2)).validate(&token, &client).is_err());
}
#[test]
fn trusted_provider_binds_client_data_to_cpk() {
    let provider = key(2);
    let identity = key(1);
    let token = sign(&provider, &claims(&identity));
    let client = sign(&identity, &json!({"IdentityPublicKey":public(&identity)}));
    let validated = verifier(&provider).validate(&token, &client).unwrap();
    assert!(validated.authenticated);
    assert_eq!(validated.xuid.as_deref(), Some("123"));
    assert_eq!(validated.identity_public_key, public(&identity));
    let forged = sign(&provider, &json!({"IdentityPublicKey":public(&identity)}));
    assert!(verifier(&provider).validate(&token, &forged).is_err());
    let mismatch = sign(&identity, &json!({"IdentityPublicKey":public(&provider)}));
    assert!(verifier(&provider).validate(&token, &mismatch).is_err());
}
#[test]
fn trusted_signatures_do_not_override_claim_validation() {
    let provider = key(2);
    let identity = key(1);
    let client = sign(&identity, &json!({}));
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
            "iss" => assert!(matches!(
                error,
                JolyneError::Auth(AuthError::InvalidIssuer(_))
            )),
            "aud" => assert!(matches!(
                error,
                JolyneError::Auth(AuthError::InvalidAudience(_))
            )),
            _ => assert!(matches!(
                error,
                JolyneError::Auth(AuthError::TemporalValidation)
            )),
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
async fn offline_token_is_accepted_but_never_authenticated() {
    let identity = key(1);
    let token = sign(&identity, &claims(&identity));
    let client = sign(&identity, &json!({"IdentityPublicKey":public(&identity)}));
    let auth = json!({"AuthenticationType":2,"Token":token});
    let validated = authenticate_login(&auth.to_string(), &client, false, true)
        .await
        .unwrap();
    assert!(!validated.authenticated);
    assert_eq!(validated.identity_public_key, public(&identity));
}
#[test]
fn legacy_self_signed_chain_cannot_claim_mojang_trust() {
    let attacker = key(1);
    let now = jsonwebtoken::get_current_timestamp();
    let token = sign(
        &attacker,
        &json!({"identityPublicKey":public(&attacker),"extraData":{"XUID":"123","displayName":"Forged","identity":"uuid"},"exp":now+3600,"nbf":now-60}),
    );
    assert!(validate_chain(vec![token.clone()], true).is_err());
    assert!(!validate_chain(vec![token], false).unwrap().authenticated);
}
