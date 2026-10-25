//! The committed BDS 1.26.51 capture fixtures (`captures/1.26.51`) are the
//! packets the manifests describe, and Valentine decodes and re-encodes each
//! of them byte for byte.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use valentine::bedrock::version::v1_26_51::{McpePacketArgs, McpePacketData};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("captures/1.26.51")
}

fn manifest(session: &str) -> HashMap<String, serde_json::Value> {
    let text = std::fs::read_to_string(root().join(session).join("manifest.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(manifest["protocol_version"], 2193);
    assert_eq!(manifest["server_version"], "1.26.51");
    manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| (entry["path"].as_str().unwrap().to_owned(), entry.clone()))
        .collect()
}

fn framed(bytes: &[u8]) -> bytes::Bytes {
    let mut out = Vec::with_capacity(bytes.len() + 5);
    let mut length = bytes.len() as u32;
    while length >= 0x80 {
        out.push(length as u8 | 0x80);
        length >>= 7;
    }
    out.push(length as u8);
    out.extend_from_slice(bytes);
    out.into()
}

fn bins(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            bins(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "bin") {
            out.push(path);
        }
    }
}

#[test]
fn committed_fixtures_match_manifests_and_round_trip() {
    let manifests: HashMap<&str, _> = ["member", "operator"]
        .into_iter()
        .map(|session| (session, manifest(session)))
        .collect();
    let mut files = Vec::new();
    bins(&root(), &mut files);
    assert!(files.len() > 150, "only {} fixtures", files.len());
    for path in files {
        let relative = path
            .strip_prefix(root())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let (session, inner) = relative.split_once('/').unwrap();
        let entry = &manifests[session][inner];
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            entry["sha256"],
            hex::encode(Sha256::digest(&bytes)),
            "{relative} differs from its manifest entry"
        );

        let (header, data) =
            McpePacketData::decode_inner_bytes(&mut framed(&bytes), McpePacketArgs)
                .unwrap_or_else(|error| panic!("{relative}: {error:?}"));
        let debug = format!("{data:?}");
        let name = debug.split('(').next().unwrap();
        assert_eq!(Some(name), entry["packet"].as_str(), "{relative}");
        let mut encoded = bytes::BytesMut::new();
        data.encode_inner_bytes_mut(&mut encoded, header.from_subclient, header.to_subclient)
            .unwrap();
        assert_eq!(
            &encoded[..],
            &framed(&bytes)[..],
            "{relative} re-encodes differently"
        );
    }
}

#[test]
fn every_summonable_identifier_has_an_add_actor_snapshot() {
    let text = std::fs::read_to_string(root().join("operator/manifest.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&text).unwrap();
    let summons = manifest["summons"].as_array().unwrap();
    assert!(summons.len() > 100);
    for summon in summons {
        let identifier = summon["identifier"].as_str().unwrap();
        assert!(!summon["seq"].is_null(), "{identifier} has no AddActor");
        let name = identifier.trim_start_matches("minecraft:");
        assert!(
            root().join(format!("operator/summons/{name}.bin")).exists(),
            "{identifier} snapshot is not committed"
        );
    }
}
