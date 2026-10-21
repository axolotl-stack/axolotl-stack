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
    let original: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fixture).unwrap()).unwrap();
    assert_eq!(data, original);

    let _ = std::fs::remove_file(output);
}

#[test]
fn fixture_cli_rejects_missing_required_sections_without_writing() {
    for section in ["creative", "biomes", "entities"] {
        let mut data: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/minimal-extracted-data.json")).unwrap();
        data.as_object_mut().unwrap().remove(section);
        let base = std::env::temp_dir().join(format!(
            "bds-extractor-missing-{section}-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        let fixture = base.with_extension("input.json");
        let output = base.with_extension("output.json");
        std::fs::write(&fixture, serde_json::to_vec(&data).unwrap()).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_bds-extractor"))
            .arg("--fixture")
            .arg(&fixture)
            .arg("--output")
            .arg(&output)
            .output()
            .unwrap();
        assert!(!result.status.success(), "accepted missing {section}");
        assert!(
            String::from_utf8_lossy(&result.stderr)
                .contains(&format!("missing required section: {section}")),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!output.exists());
        std::fs::remove_file(fixture).unwrap();
    }
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
