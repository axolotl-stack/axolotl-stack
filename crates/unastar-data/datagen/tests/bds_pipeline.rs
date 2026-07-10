use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn bds_only_cli_writes_packet_artifacts_and_manifest_provenance() {
    let temp = TempDir::new("unastar-data-gen-bds-pipeline");
    let capture = temp.path().join("capture.json");
    let output = temp.path().join("out");
    write_fixture_capture(&capture);

    let status = Command::new(env!("CARGO_BIN_EXE_unastar-data-gen"))
        .arg("--bds-only")
        .arg("--bds-extraction")
        .arg(&capture)
        .arg("--output")
        .arg(&output)
        .arg("--log")
        .arg("warn")
        .status()
        .expect("run unastar-data-gen --bds-only");

    assert!(status.success());

    let biome_packets =
        std::fs::read_to_string(output.join("biome_packets.kdl")).expect("biome_packets.kdl");
    let entity_identifiers = std::fs::read_to_string(output.join("entity_identifiers.kdl"))
        .expect("entity_identifiers.kdl");
    let manifest = std::fs::read_to_string(output.join("manifest.kdl")).expect("manifest.kdl");

    assert!(biome_packets.contains("biome_packet minecraft:plains"));
    assert!(entity_identifiers.contains("entity_identifiers source=bds_extractor"));
    assert!(manifest.contains("source \"bds_extractor_capture\""));
    assert!(manifest.contains("artifact \"biome_packets\""));
    assert!(manifest.contains("artifact \"entity_identifiers\""));
}

fn write_fixture_capture(path: &Path) {
    let json = r#"{
        "metadata": {
            "extraction_date": "fixture",
            "game_version": "1.26.30",
            "engine": "BDS"
        },
        "biomes": {
            "definitions": [
                {
                    "name_index": 0,
                    "biome_id": 1,
                    "temperature": 0.8,
                    "downfall": 0.4
                }
            ],
            "string_list": ["minecraft:plains"]
        },
        "entities": {
            "identifiers_nbt_base64": "CgA="
        }
    }"#;
    std::fs::write(path, json).expect("write fixture capture");
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "{}-{}-{}",
            prefix,
            std::process::id(),
            unique_suffix()
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
