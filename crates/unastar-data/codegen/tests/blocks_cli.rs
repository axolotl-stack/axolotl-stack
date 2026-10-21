use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn blocks_only_cli_generates_static_consumer_from_artifact() {
    let temp = TempDir::new("unastar-data-codegen-blocks");
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    std::fs::create_dir_all(&input).expect("create input dir");
    std::fs::create_dir_all(&output).expect("create output dir");
    write_blocks_artifact(&input, 15);

    let status = Command::new(env!("CARGO_BIN_EXE_unastar-data-codegen"))
        .arg("--blocks-only")
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .arg("--log")
        .arg("warn")
        .status()
        .expect("run unastar-data-codegen --blocks-only");

    assert!(status.success());

    let consumer = output.join("consumer.rs");
    std::fs::write(
        &consumer,
        r#"
        mod blocks;
        fn main() {
            assert_eq!(blocks::get("minecraft:stone").unwrap().default_state_id, 0);
            assert_eq!(blocks::by_runtime_id(1).unwrap().identifier, "minecraft:stone");
            assert_eq!(blocks::by_runtime_id(2).unwrap().identifier, "minecraft:air");
            assert!(blocks::by_runtime_id(3).is_none());
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
fn blocks_only_cli_rejects_invalid_light_values() {
    let temp = TempDir::new("unastar-data-codegen-blocks-invalid");
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    std::fs::create_dir_all(&input).expect("create input dir");
    std::fs::create_dir_all(&output).expect("create output dir");
    write_blocks_artifact(&input, 16);

    let output = Command::new(env!("CARGO_BIN_EXE_unastar-data-codegen"))
        .arg("--blocks-only")
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .arg("--log")
        .arg("warn")
        .output()
        .expect("run unastar-data-codegen --blocks-only");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("block minecraft:stone has light values outside 0..=15"),
        "stderr should explain invalid block light range, got: {stderr}"
    );
}

fn write_blocks_artifact(input: &Path, filter_light: u8) {
    std::fs::write(
        input.join("blocks.kdl"),
        format!(
            r#"block "minecraft:stone" id=1 name="Stone" hardness=1.5 resistance=6.0 is_transparent=#false emit_light=0 filter_light={filter_light} min_state_id=0 max_state_id=1 default_state_id=0 state_id_count=2
block "minecraft:air" id=0 name="Air" hardness=0.0 resistance=0.0 is_transparent=#true emit_light=0 filter_light=0 min_state_id=2 max_state_id=2 default_state_id=2 state_id_count=1
"#
        ),
    )
    .expect("write blocks.kdl");
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
