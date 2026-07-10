use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn biomes_only_cli_generates_static_consumer_from_artifact() {
    let temp = TempDir::new("unastar-data-codegen-biomes");
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    std::fs::create_dir_all(&input).expect("create input dir");
    std::fs::create_dir_all(&output).expect("create output dir");
    write_biomes_artifact(&input, "vanilla_behavior_pack");

    let status = Command::new(env!("CARGO_BIN_EXE_unastar-data-codegen"))
        .arg("--biomes-only")
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .arg("--log")
        .arg("warn")
        .status()
        .expect("run unastar-data-codegen --biomes-only");

    assert!(status.success());

    let generated = std::fs::read_to_string(output.join("biomes.rs"))
        .expect("generated biomes.rs should exist");
    assert!(generated.contains("pub const ALL_BIOMES: [BiomeData; 1usize]"));
    assert!(generated.contains("identifier: \"minecraft:plains\""));
    assert!(generated.contains("source: \"vanilla_behavior_pack\""));
    assert!(generated.contains("temperature: 0.8f32"));
    assert!(generated.contains("downfall: 0.4f32"));
    assert!(generated.contains("&[\"overworld\", \"plains\"]"));

    for unexpected in [
        "bds_packets.rs",
        "blocks.rs",
        "creative.rs",
        "entities",
        "items.rs",
    ] {
        assert!(
            !output.join(unexpected).exists(),
            "--biomes-only should not generate {unexpected}"
        );
    }
}

#[test]
fn biomes_only_cli_rejects_non_behavior_pack_source() {
    let temp = TempDir::new("unastar-data-codegen-biomes-invalid");
    let input = temp.path().join("input");
    let output = temp.path().join("output");
    std::fs::create_dir_all(&input).expect("create input dir");
    std::fs::create_dir_all(&output).expect("create output dir");
    write_biomes_artifact(&input, "valentine_bedrock_1_26_30");

    let output = Command::new(env!("CARGO_BIN_EXE_unastar-data-codegen"))
        .arg("--biomes-only")
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .arg("--log")
        .arg("warn")
        .output()
        .expect("run unastar-data-codegen --biomes-only");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("must be sourced from vanilla_behavior_pack"),
        "stderr should explain invalid biome source, got: {stderr}"
    );
}

fn write_biomes_artifact(input: &Path, source: &str) {
    std::fs::write(
        input.join("biomes.kdl"),
        format!(
            r#"biome "minecraft:plains" format_version="1.21.110" source="{source}" source_file="plains.biome.json" {{
    component "minecraft:climate" {{
        temperature 0.8
        downfall 0.4
    }}
    component "minecraft:tags" {{
        tags {{
            tag "plains"
            tag "overworld"
        }}
    }}
}}
"#
        ),
    )
    .expect("write biomes.kdl");
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
