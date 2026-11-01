//! Configured archive offers exercise the real pack-transfer state machines.

use super::*;
use std::io::Write;
use std::time::Instant;

use jolyne::config::ConnectionOptions;
use jolyne::proto::conn::{Connection, Datagram, Framing};
use jolyne::proto::packs::{
    ClientPackExchange, ClientPackPolicy, DownloadedPack, PackProgress, ServerPackExchange,
};
use jolyne::valentine::McpePacket;
use serde_json::{Value, json};

/// Independent fixture identities are not any installed or external pack's identity.
pub(crate) fn document(id: u128, version: Value) -> Value {
    json!({
        "format_version": 2,
        "header": {
            "name": "Interface example", "description": "Generated test content",
            "uuid": Uuid::from_u128(id).to_string(), "version": version,
            "min_engine_version": [1, 26, 0]
        },
        "modules": [{"type": "resources", "uuid": Uuid::from_u128(id + 1).to_string(),
                     "version": [1, 0, 0]}]
    })
}

/// Build original minimal archives without copying game files or textures.
pub(crate) fn archive(manifest: &[u8], entry_name: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file(entry_name, options).unwrap();
    zip.write_all(manifest).unwrap();
    zip.start_file("ui/example.json", options).unwrap();
    zip.write_all(br#"{"namespace":"example"}"#).unwrap();
    zip.finish().unwrap().into_inner()
}

/// Create one real configured archive in an isolated local directory.
pub(crate) fn configured(bytes: &[u8]) -> (tempfile::TempDir, ResourcePackConfig) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interface.mcpack");
    std::fs::write(&path, bytes).unwrap();
    (
        directory,
        ResourcePackConfig {
            paths: vec![path],
            ..ResourcePackConfig::default()
        },
    )
}

/// Decode actual sealed resource-pack protocol batches without a transport shim.
fn packets(from: &mut Connection, to: &mut Connection) -> Vec<McpePacket> {
    while let Some(transmit) = from.poll_transmit() {
        to.handle_datagram(Datagram::Contiguous(transmit.payload))
            .unwrap();
    }
    let mut packets = Vec::new();
    while let Some(packet) = to.next_packet().unwrap() {
        packets.push(packet.decode().unwrap());
    }
    packets
}

/// Both real protocol peers must finish and verify archive hashes before success.
fn download(offers: Arc<ServerPacks>) -> Vec<DownloadedPack> {
    let mut server_wire = Connection::new(Framing::RakNet, ConnectionOptions::default());
    let mut client_wire = Connection::new(Framing::RakNet, ConnectionOptions::default());
    let mut now = Instant::now();
    let mut server = ServerPackExchange::start(offers, &mut server_wire, now).unwrap();
    let mut client = ClientPackExchange::new(ClientPackPolicy::default());
    for _ in 0..256 {
        for packet in packets(&mut server_wire, &mut client_wire) {
            client.on_packet(&packet, &mut client_wire, now).unwrap();
        }
        for packet in packets(&mut client_wire, &mut server_wire) {
            if server.on_packet(&packet, &mut server_wire, now).unwrap() == PackProgress::Done {
                return client.into_downloaded().0;
            }
        }
        if let Some(next) = server.timer() {
            now = next;
            server.on_timer(&mut server_wire, now).unwrap();
        }
    }
    panic!("configured pack exchange did not complete");
}

#[test]
fn archive_identity_bytes_and_hash_reach_the_actual_client_in_declared_order() {
    let a = archive(
        &document(10, json!([1, 2, 3])).to_string().into_bytes(),
        "manifest.json",
    );
    let b = archive(
        &document(20, json!("4.5.6")).to_string().into_bytes(),
        "manifest.json",
    );
    let (directory, mut config) = configured(&a);
    let second = directory.path().join("second.mcpack");
    std::fs::write(&second, &b).unwrap();
    config.paths.push(second);
    let loaded = load(&config, true).unwrap();
    assert!(loaded.required);
    assert_eq!(loaded.packs[0].uuid, Uuid::from_u128(10));
    assert_eq!(loaded.packs[0].version, "1.2.3");
    assert_eq!(loaded.packs[1].uuid, Uuid::from_u128(20));
    let expected_hashes: Vec<_> = loaded.packs.iter().map(|pack| *pack.sha256()).collect();
    let received = download(loaded);
    assert_eq!(received.len(), 2);
    assert_eq!(received[0].content.as_ref(), a);
    assert_eq!(received[1].content.as_ref(), b);
    assert_eq!(received[0].sha256, expected_hashes[0]);
    assert_eq!(received[1].sha256, expected_hashes[1]);
}

#[test]
fn loaded_offer_is_immutable_when_the_configured_file_changes_or_disappears() {
    let bytes = archive(
        &document(10, json!([1, 0, 0])).to_string().into_bytes(),
        "manifest.json",
    );
    let (_directory, config) = configured(&bytes);
    let loaded = load(&config, false).unwrap();
    std::fs::write(&config.paths[0], b"changed after startup").unwrap();
    assert!(load(&config, false).is_err());
    std::fs::remove_file(&config.paths[0]).unwrap();
    assert_eq!(download(loaded)[0].content.as_ref(), bytes);
    assert!(load(&config, false).is_err());
}

#[test]
fn json_comments_bom_and_manifest_metadata_do_not_change_the_pack_identity() {
    let value = document(40, json!([2, 0, 1]));
    let text = format!(
        "\u{feff}/* generated fixture */\n{}\n// retained line comment",
        value
    );
    let (_directory, config) = configured(&archive(text.as_bytes(), "manifest.json"));
    let loaded = load(&config, false).unwrap();
    assert_eq!(loaded.packs[0].uuid, Uuid::from_u128(40));
    assert_eq!(loaded.packs[0].version, "2.0.1");
}

#[test]
fn invalid_second_archive_never_becomes_a_successful_partial_offer() {
    let bytes = archive(
        &document(10, json!([1, 0, 0])).to_string().into_bytes(),
        "manifest.json",
    );
    let (directory, mut config) = configured(&bytes);
    let second = directory.path().join("invalid.mcpack");
    std::fs::write(&second, b"not a ZIP archive").unwrap();
    config.paths.push(second);
    assert!(
        load(&config, true)
            .unwrap_err()
            .to_string()
            .contains("invalid ZIP")
    );
}

#[test]
fn identity_and_module_failures_are_explicit_before_an_offer_exists() {
    let original = document(10, json!([1, 0, 0]));
    let mut variants = Vec::new();
    let mut bad = original.clone();
    bad["header"]["uuid"] = json!(Uuid::nil().to_string());
    variants.push(bad);
    let mut bad = original.clone();
    bad["modules"][0]["uuid"] = bad["header"]["uuid"].clone();
    variants.push(bad);
    let mut bad = original.clone();
    bad["modules"] = json!([]);
    variants.push(bad);
    let mut bad = original.clone();
    bad["modules"][0]["type"] = json!("script");
    variants.push(bad);
    let mut bad = original.clone();
    bad["header"]["version"] = json!("not-a-version");
    variants.push(bad);
    let mut bad = original.clone();
    bad["format_version"] = json!(1);
    variants.push(bad);
    for bad in variants {
        let (_directory, config) =
            configured(&archive(bad.to_string().as_bytes(), "manifest.json"));
        assert!(load(&config, false).is_err());
    }
}

#[test]
fn duplicate_configured_uuid_is_not_advertised_as_two_versions() {
    let a = archive(
        &document(10, json!([1, 0, 0])).to_string().into_bytes(),
        "manifest.json",
    );
    let b = archive(
        &document(10, json!([2, 0, 0])).to_string().into_bytes(),
        "manifest.json",
    );
    let (directory, mut config) = configured(&a);
    let second = directory.path().join("second.mcpack");
    std::fs::write(&second, b).unwrap();
    config.paths.push(second);
    assert!(
        load(&config, true)
            .unwrap_err()
            .to_string()
            .contains("UUID")
    );
}

#[test]
fn aggregate_bytes_and_decompressed_manifest_limits_are_enforced_without_truncation() {
    let bytes = archive(
        &document(10, json!([1, 0, 0])).to_string().into_bytes(),
        "manifest.json",
    );
    let (_directory, mut config) = configured(&bytes);
    config.max_total_bytes = bytes.len() as u64 - 1;
    assert!(load(&config, false).is_err());
    config.max_total_bytes = bytes.len() as u64;
    assert!(load(&config, false).is_ok());
    config.max_manifest_bytes = 10;
    assert!(
        load(&config, false)
            .unwrap_err()
            .to_string()
            .contains("manifest exceeds")
    );
}

#[test]
fn root_manifest_is_required_and_unsafe_names_are_rejected() {
    let value = document(10, json!([1, 0, 0]));
    for name in [
        "nested/manifest.json",
        "../manifest.json",
        "/manifest.json",
        "..\\manifest.json",
    ] {
        let (_directory, config) = configured(&archive(value.to_string().as_bytes(), name));
        assert!(load(&config, false).is_err());
    }
}

#[test]
fn empty_configuration_preserves_an_empty_protocol_offer() {
    let loaded = load(&ResourcePackConfig::default(), false).unwrap();
    assert!(loaded.is_empty());
    assert!(download(loaded).is_empty());
}

#[test]
fn preview_manifest_uses_declared_text_versions_instead_of_rewriting_them() {
    let mut value = document(60, json!("3.0.0-preview.1"));
    value["format_version"] = json!(3);
    value["header"]["min_engine_version"] = json!("1.26.0");
    value["modules"][0]["version"] = json!("1.0.0");
    let (_directory, config) = configured(&archive(value.to_string().as_bytes(), "manifest.json"));
    assert_eq!(
        load(&config, false).unwrap().packs[0].version,
        "3.0.0-preview.1"
    );
    value["modules"][0]["version"] = json!([1, 0, 0]);
    let (_directory, config) = configured(&archive(value.to_string().as_bytes(), "manifest.json"));
    assert!(load(&config, false).is_err());
}

#[test]
fn declared_visual_capability_and_addon_metadata_are_preserved() {
    let mut value = document(80, json!([1, 0, 0]));
    value["capabilities"] = json!(["raytraced"]);
    value["metadata"] = json!({"product_type":"addon", "authors":["Example"]});
    let (_directory, config) = configured(&archive(value.to_string().as_bytes(), "manifest.json"));
    let loaded = load(&config, false).unwrap();
    assert!(loaded.packs[0].ray_tracing);
    assert!(loaded.packs[0].is_addon);
}
