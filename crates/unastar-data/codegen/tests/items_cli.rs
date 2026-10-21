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

    let consumer = output.join("consumer.rs");
    std::fs::write(
        &consumer,
        r#"
        mod items;
        fn main() {
            let apple = items::get("minecraft:apple").unwrap();
            assert_eq!(apple.network_id, 285);
            assert!(apple.component_based);
            assert_eq!(apple.tags, &["food"]);
            assert_eq!(apple.native_properties_json, "{\"maxDamage\":0}");
            let honey = items::get("minecraft:honey_bottle").unwrap();
            assert_eq!(honey.max_stack_size, 16);
            assert_eq!(honey.version, 2);
            assert!(items::get("minecraft:missing").is_none());
        }
    "#,
    )
    .unwrap();
    let binary = output.join(format!("consumer{}", std::env::consts::EXE_SUFFIX));
    assert!(
        Command::new("rustc")
            .arg("--edition=2024")
            .arg(&consumer)
            .arg("-o")
            .arg(&binary)
            .status()
            .unwrap()
            .success()
    );
    assert!(Command::new(binary).status().unwrap().success());
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
            r#"item "minecraft:apple" runtime_id={apple_id} component_based=#true version=1 max_stack_size=64 max_stack_size_source="protocolgen_native" maxDamage=0 {{
    tag "food"
}}
item "minecraft:honey_bottle" runtime_id=737 component_based=#false version=2 max_stack_size=16 max_stack_size_source="protocolgen_native"
item "minecraft:stick" runtime_id={stick_id} component_based=#false version=2 max_stack_size=64 max_stack_size_source="protocolgen_native"
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
