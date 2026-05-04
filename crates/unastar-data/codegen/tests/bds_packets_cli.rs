use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn bds_packets_only_cli_generates_packet_consumer_from_artifacts() {
    let temp = TempDir::new("unastar-data-codegen-bds-packets");
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    std::fs::create_dir_all(&input).expect("create input dir");
    std::fs::create_dir_all(&output).expect("create output dir");
    write_bds_artifacts(&input);

    let status = Command::new(env!("CARGO_BIN_EXE_unastar-data-codegen"))
        .arg("--bds-packets-only")
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .arg("--log")
        .arg("warn")
        .status()
        .expect("run unastar-data-codegen --bds-packets-only");

    assert!(status.success());

    let generated =
        std::fs::read_to_string(output.join("bds_packets.rs")).expect("generated bds_packets.rs");
    assert!(generated.contains("BIOME_PACKET_DEFINITIONS: [BdsBiomePacketDefinition; 1usize]"));
    assert!(generated.contains("identifier: \"minecraft:plains\""));
    assert!(generated.contains("biome_id: 1u16"));
    assert!(generated.contains("ENTITY_IDENTIFIERS: Option<BdsEntityIdentifiers> = Some"));
    assert!(generated.contains("nbt_base64: \"CgA=\""));

    for unexpected in [
        "biomes.rs",
        "blocks.rs",
        "creative.rs",
        "entities",
        "items.rs",
    ] {
        assert!(
            !output.join(unexpected).exists(),
            "--bds-packets-only should not generate {unexpected}"
        );
    }
}

fn write_bds_artifacts(input: &Path) {
    std::fs::write(
        input.join("biome_packets.kdl"),
        r#"biome_packet "minecraft:plains" source="bds_extractor" biome_id=1 name_index=0 temperature=0.8 downfall=0.4"#,
    )
    .expect("write biome_packets.kdl");
    std::fs::write(
        input.join("entity_identifiers.kdl"),
        r#"entity_identifiers source="bds_extractor" nbt_base64="CgA=""#,
    )
    .expect("write entity_identifiers.kdl");
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
