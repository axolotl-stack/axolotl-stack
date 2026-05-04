use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn entities_only_cli_generates_entity_tree_from_artifact() {
    let temp = TempDir::new("unastar-data-codegen-entities");
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    std::fs::create_dir_all(&input).expect("create input dir");
    std::fs::create_dir_all(&output).expect("create output dir");
    write_entities_artifact(&input, "runtime_id 12");

    let status = Command::new(env!("CARGO_BIN_EXE_unastar-data-codegen"))
        .arg("--entities-only")
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .arg("--log")
        .arg("warn")
        .status()
        .expect("run unastar-data-codegen --entities-only");

    assert!(status.success());

    let entities_mod = std::fs::read_to_string(output.join("entities/mod.rs"))
        .expect("generated entities/mod.rs should exist");
    assert!(entities_mod.contains("pub mod components"));
    assert!(entities_mod.contains("pub mod definitions"));

    let definitions_mod = std::fs::read_to_string(output.join("entities/definitions/mod.rs"))
        .expect("generated definitions/mod.rs should exist");
    assert!(definitions_mod.contains("pub mod pig"));
    assert!(definitions_mod.contains("identifier: \"minecraft:pig\""));
    assert!(definitions_mod.contains("spawn_category: Some(\"animal\")"));
    assert!(definitions_mod.contains("is_spawnable: true"));
    assert!(definitions_mod.contains("is_summonable: true"));
    assert!(definitions_mod.contains("runtime_id: Some(12u32)"));

    assert!(output.join("entities/components/mod.rs").exists());
    let pig_definition = std::fs::read_to_string(output.join("entities/definitions/pig.rs"))
        .expect("generated pig entity definition should exist");
    assert!(pig_definition.contains("pub const IDENTIFIER: &'static str = \"minecraft:pig\""));
    assert!(pig_definition.contains("pub const RUNTIME_ID: u32 = 12u32"));

    for unexpected in [
        "bds_packets.rs",
        "biomes.rs",
        "blocks.rs",
        "creative.rs",
        "items.rs",
    ] {
        assert!(
            !output.join(unexpected).exists(),
            "--entities-only should not generate {unexpected}"
        );
    }
}

#[test]
fn entities_only_cli_rejects_negative_runtime_ids() {
    let temp = TempDir::new("unastar-data-codegen-entities-invalid");
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    std::fs::create_dir_all(&input).expect("create input dir");
    std::fs::create_dir_all(&output).expect("create output dir");
    write_entities_artifact(&input, "runtime_id -1");

    let output = Command::new(env!("CARGO_BIN_EXE_unastar-data-codegen"))
        .arg("--entities-only")
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .arg("--log")
        .arg("warn")
        .output()
        .expect("run unastar-data-codegen --entities-only");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("entity minecraft:pig runtime_id -1 is out of u32 range"),
        "stderr should explain invalid entity runtime ID, got: {stderr}"
    );
}

fn write_entities_artifact(input: &Path, runtime_line: &str) {
    std::fs::write(
        input.join("entities.kdl"),
        format!(
            r#"entity "minecraft:pig" {{
    spawn_category "animal"
    is_spawnable #true
    is_summonable #true
    {runtime_line}
}}
"#
        ),
    )
    .expect("write entities.kdl");
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
