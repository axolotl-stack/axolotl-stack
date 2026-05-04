use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn creative_only_cli_generates_static_consumer_from_artifact() {
    let temp = TempDir::new("unastar-data-codegen-creative");
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    std::fs::create_dir_all(&input).expect("create input dir");
    std::fs::create_dir_all(&output).expect("create output dir");
    write_creative_artifact(&input, "CgA=", "CgA=");

    let status = Command::new(env!("CARGO_BIN_EXE_unastar-data-codegen"))
        .arg("--creative-only")
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .arg("--log")
        .arg("warn")
        .status()
        .expect("run unastar-data-codegen --creative-only");

    assert!(status.success());

    let generated = std::fs::read_to_string(output.join("creative.rs"))
        .expect("generated creative.rs should exist");
    assert!(generated.contains("pub const SOURCE: &str = \"pmmp_bedrock_data\""));
    assert!(generated.contains("pub static CREATIVE_INVENTORY: CreativeInventoryData"));
    assert!(generated.contains("group_name: \"itemGroup.name.arrow\""));
    assert!(generated.contains("group_icon: Some(CreativeEntryData"));
    assert!(generated.contains("name: \"minecraft:arrow\""));
    assert!(generated.contains("block_states: Some(\"CgA=\")"));
    assert!(generated.contains("damage: 6i16"));
    assert!(generated.contains("nbt: Some(\"CgA=\")"));
}

#[test]
fn creative_only_cli_rejects_invalid_artifact_payloads() {
    let temp = TempDir::new("unastar-data-codegen-creative-invalid");
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    std::fs::create_dir_all(&input).expect("create input dir");
    std::fs::create_dir_all(&output).expect("create output dir");
    write_creative_artifact(&input, "not base64!!", "CgA=");

    let output = Command::new(env!("CARGO_BIN_EXE_unastar-data-codegen"))
        .arg("--creative-only")
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .arg("--log")
        .arg("warn")
        .output()
        .expect("run unastar-data-codegen --creative-only");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not valid base64"),
        "stderr should explain invalid base64, got: {stderr}"
    );
}

fn write_creative_artifact(input: &Path, block_states: &str, nbt: &str) {
    std::fs::write(
        input.join("creative.kdl"),
        format!(
            r#"creative_tab construction source_file="creative_construction.json" {{
    group "itemGroup.name.arrow" index=0 {{
        icon "minecraft:arrow"
        item "minecraft:arrow" meta=6 block_states="{block_states}" nbt="{nbt}"
    }}
}}
creative_tab nature source_file="creative_nature.json" {{
    group "itemGroup.name.nature" index=0 {{
        item "minecraft:stone"
    }}
}}
creative_tab equipment source_file="creative_equipment.json" {{
    group "itemGroup.name.equipment" index=0 {{
        item "minecraft:apple"
    }}
}}
creative_tab items source_file="creative_items.json" {{
    group "itemGroup.name.items" index=0 {{
        item "minecraft:stick"
    }}
}}
"#
        ),
    )
    .expect("write creative.kdl");
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
