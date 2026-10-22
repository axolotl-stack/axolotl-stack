use client_diff::{Masks, Status, compare, load};
use std::path::Path;

#[test]
fn inserted_packet_does_not_shift_later_alignment() {
    let a = load(Path::new("tests/fixtures/latency-a.jsonl"), "c2s").unwrap();
    let b = load(Path::new("tests/fixtures/latency-b.jsonl"), "c2s").unwrap();
    let rows = compare(&a, &b, &Masks::default()).unwrap();
    assert_eq!(
        rows.iter().map(|r| r.status).collect::<Vec<_>>(),
        vec![Status::MaskedEqual, Status::Missing, Status::Identical]
    );
    assert_eq!((rows[1].left, rows[1].right), (None, Some(1)));
}

#[test]
fn disabling_timestamp_mask_exposes_wire_difference() {
    let a = load(Path::new("tests/fixtures/latency-a.jsonl"), "c2s").unwrap();
    let b = load(Path::new("tests/fixtures/latency-b.jsonl"), "c2s").unwrap();
    let masks = Masks {
        timestamps: false,
        ..Masks::default()
    };
    assert_eq!(
        compare(&a, &b, &masks).unwrap()[0].status,
        Status::Different
    );
}

#[test]
fn unknown_packets_stay_raw_and_headers_are_not_masked() {
    let mut a = load(Path::new("tests/fixtures/latency-a.jsonl"), "c2s").unwrap();
    let mut b = a.clone();
    a[0].id = 1023;
    a[0].hex = "ff0701".into();
    b[0].id = 1023;
    b[0].hex = "ff0702".into();
    assert_eq!(
        compare(&a, &b, &Masks::default()).unwrap()[0].status,
        Status::Different
    );
}

#[test]
fn known_but_truncated_packet_compares_raw() {
    let mut a = load(Path::new("tests/fixtures/latency-a.jsonl"), "c2s").unwrap();
    let mut b = a.clone();
    a[0].hex = "7301".into();
    b[0].hex = "7302".into();
    assert_eq!(
        compare(&a, &b, &Masks::default()).unwrap()[0].status,
        Status::Different
    );
}

#[test]
fn float_tolerance_masks_only_nearby_coordinates() {
    use valentine::bedrock::version::v1_26_51::{McpePacketData, MovePlayerPacket};
    fn record(x: f32, on_ground: bool) -> client_diff::Record {
        let mut packet = MovePlayerPacket::default();
        packet.position.x = x;
        packet.on_ground = on_ground;
        let mut encoded = Vec::new();
        McpePacketData::MovePlayerPacket(packet)
            .encode_inner(&mut encoded, 0, 0)
            .unwrap();
        // The small default MovePlayer body has a one-byte batch length prefix.
        assert!(encoded[0] < 128);
        client_diff::Record {
            seq: 0,
            t_ms: 0.,
            dir: "c2s".into(),
            id: 19,
            name: "MovePlayerPacket".into(),
            hex: hex::encode(&encoded[1..]),
        }
    }
    let a = [record(1., false)];
    assert_eq!(
        compare(&a, &[record(1.00005, false)], &Masks::default()).unwrap()[0].status,
        Status::MaskedEqual
    );
    assert_eq!(
        compare(&a, &[record(1.001, false)], &Masks::default()).unwrap()[0].status,
        Status::Different
    );
    assert_eq!(
        compare(&a, &[record(1.00005, true)], &Masks::default()).unwrap()[0].status,
        Status::Different
    );
}

#[test]
fn modern_login_masks_embedded_certificate_keys_but_preserves_client_claims() {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use valentine::bedrock::version::v1_26_51::{LoginPacket, McpePacket};
    fn record(key: &str, name: &str) -> client_diff::Record {
        let jwt = |claims: serde_json::Value| {
            format!(
                "{}.{}.{}",
                URL_SAFE_NO_PAD.encode(br#"{"alg":"ES384"}"#),
                URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap()),
                URL_SAFE_NO_PAD.encode(key)
            )
        };
        let chain = jwt(serde_json::json!({"identityPublicKey":key,"displayName":"ClientDiff"}));
        let auth = serde_json::json!({"Certificate":serde_json::json!({"chain":[chain]}).to_string(),"AuthenticationType":2,"Token":jwt(serde_json::json!({"cpk":key,"xname":"ClientDiff"}))}).to_string();
        let client = jwt(serde_json::json!({"ThirdPartyName":name,"DeviceId":key}));
        let mut request = Vec::new();
        for part in [auth, client] {
            request.extend_from_slice(&(part.len() as u32).to_le_bytes());
            request.extend_from_slice(part.as_bytes());
        }
        let mut bytes = Vec::new();
        McpePacket::from(LoginPacket {
            client_network_version: 2193,
            connection_request: request.into(),
        })
        .data
        .encode_inner(&mut bytes, 0, 0)
        .unwrap();
        let prefix = bytes.iter().position(|b| b & 128 == 0).unwrap() + 1;
        client_diff::Record {
            seq: 0,
            t_ms: 0.,
            dir: "c2s".into(),
            id: 1,
            name: "Login".into(),
            hex: hex::encode(&bytes[prefix..]),
        }
    }
    let a = [record("key-a", "ClientDiff")];
    assert_eq!(
        compare(&a, &[record("key-b", "ClientDiff")], &Masks::default()).unwrap()[0].status,
        Status::MaskedEqual
    );
    assert_eq!(
        compare(&a, &[record("key-b", "Other")], &Masks::default()).unwrap()[0].status,
        Status::Different
    );
}

#[test]
fn noncanonical_wire_values_are_not_erased_by_decode_encode() {
    let mut a = load(Path::new("tests/fixtures/latency-a.jsonl"), "c2s").unwrap();
    let mut b = a.clone();
    a[0].hex = "73010000000000000001".into();
    b[0].hex = "73010000000000000002".into();
    assert_eq!(
        compare(&a, &b, &Masks::default()).unwrap()[0].status,
        Status::Different
    );
    b[0].hex = "f300010000000000000001".into();
    assert_eq!(
        compare(&a, &b, &Masks::default()).unwrap()[0].status,
        Status::Different
    );
}
