use std::path::PathBuf;
use std::process::Command;

#[test]
fn fixture_cli_validates_and_writes_roundtrip_json() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture = manifest_dir.join("tests/fixtures/minimal-extracted-data.json");
    let output = std::env::temp_dir().join(format!(
        "bds-extractor-fixture-{}-{}.json",
        std::process::id(),
        unique_suffix()
    ));

    let status = Command::new(env!("CARGO_BIN_EXE_bds-extractor"))
        .arg("--fixture")
        .arg(&fixture)
        .arg("--output")
        .arg(&output)
        .arg("--log")
        .arg("warn")
        .status()
        .expect("failed to run bds-extractor fixture mode");

    assert!(status.success());

    let json = std::fs::read_to_string(&output).expect("fixture output should exist");
    let data: serde_json::Value = serde_json::from_str(&json).expect("fixture output is JSON");
    assert_eq!(data["metadata"]["game_version"].as_str(), Some("1.26.0"));
    assert_eq!(
        data["blocks"]["properties"][0]["name"].as_str(),
        Some("minecraft:stone")
    );

    let _ = std::fs::remove_file(output);
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
