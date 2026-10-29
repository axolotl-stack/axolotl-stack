use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn items_only_cli_writes_schema_and_embedded_rows() {
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

    // The schema is compiled by `unastar-data` itself; here the rows are
    // checked: typed native columns, omitted defaults, sorted identifiers.
    let rows: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(output.join("items.json")).expect("read items.json"),
    )
    .expect("items.json is JSON");
    assert_eq!(
        rows,
        serde_json::json!([
            {"identifier": "minecraft:apple", "network_id": 285, "component_based": true,
             "version": 1, "max_stack_size": 64, "tags": ["food"], "max_damage": 0},
            {"identifier": "minecraft:honey_bottle", "network_id": 737, "component_based": false,
             "version": 2, "max_stack_size": 16, "max_damage": 0},
            {"identifier": "minecraft:stick", "network_id": 286, "component_based": false,
             "version": 2, "max_stack_size": 64, "max_damage": 0}
        ])
    );
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
}

fn write_items_artifact(input: &Path, apple_id: i32, stick_id: i32) {
    std::fs::write(
        input.join("items.kdl"),
        format!(
            r#"item "minecraft:apple" runtime_id={apple_id} component_based=#true version=1 max_stack_size=64 max_stack_size_source="protocolgen_native" maxDamage=0 {{
    tag "food"
}}
item "minecraft:honey_bottle" runtime_id=737 component_based=#false version=2 max_stack_size=16 max_stack_size_source="protocolgen_native" maxDamage=0
item "minecraft:stick" runtime_id={stick_id} component_based=#false version=2 max_stack_size=64 max_stack_size_source="protocolgen_native" maxDamage=0
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
