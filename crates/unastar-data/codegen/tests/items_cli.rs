use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn items_only_cli_generates_static_consumer_from_artifact() {
    let temp = TempDir::new("unastar-data-codegen-items");
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    std::fs::create_dir_all(&input).expect("create input dir");
    std::fs::create_dir_all(&output).expect("create output dir");
    write_items_artifact(&input, 285, 286);

    let status = Command::new(env!("CARGO_BIN_EXE_unastar-data-codegen"))
        .arg("--items-only")
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .arg("--log")
        .arg("warn")
        .status()
        .expect("run unastar-data-codegen --items-only");

    assert!(status.success());

    let generated =
        std::fs::read_to_string(output.join("items.rs")).expect("generated items.rs should exist");
    assert!(generated.contains("pub static ALL_ITEMS: [ItemData; 3usize]"));
    assert!(generated.contains("identifier: \"minecraft:apple\""));
    assert!(generated.contains("network_id: 285i32"));
    assert!(generated.contains("component_based: true"));
    assert!(generated.contains("identifier: \"minecraft:honey_bottle\""));
    assert!(generated.contains("max_stack_size: 16u8"));
    assert!(generated.contains("max_stack_size_source: \"vanilla_behavior_pack\""));
    assert!(generated.contains("identifier: \"minecraft:stick\""));
    assert!(generated.contains("version: 2i32"));

    for unexpected in [
        "bds_packets.rs",
        "biomes.rs",
        "blocks.rs",
        "creative.rs",
        "entities",
    ] {
        assert!(
            !output.join(unexpected).exists(),
            "--items-only should not generate {unexpected}"
        );
    }
}

#[test]
fn items_only_cli_rejects_duplicate_network_ids() {
    let temp = TempDir::new("unastar-data-codegen-items-invalid");
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    std::fs::create_dir_all(&input).expect("create input dir");
    std::fs::create_dir_all(&output).expect("create output dir");
    write_items_artifact(&input, 285, 285);

    let output = Command::new(env!("CARGO_BIN_EXE_unastar-data-codegen"))
        .arg("--items-only")
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .arg("--log")
        .arg("warn")
        .output()
        .expect("run unastar-data-codegen --items-only");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("duplicate item network_id"),
        "stderr should explain duplicate item network ID, got: {stderr}"
    );
}

fn write_items_artifact(input: &Path, apple_id: i32, stick_id: i32) {
    std::fs::write(
        input.join("items.kdl"),
        format!(
            r#"item "minecraft:apple" runtime_id={apple_id} component_based=#true version=1
item "minecraft:honey_bottle" runtime_id=737 component_based=#false version=2 max_stack_size=16 max_stack_size_source="vanilla_behavior_pack"
item "minecraft:stick" runtime_id={stick_id} component_based=#false version=2
"#
        ),
    )
    .expect("write items.kdl");
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
