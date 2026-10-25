//! Build script: compiles the Bedrock-style generator's data ahead of time.
//!
//! Only active with the `bedrock-worldgen` feature. It reads the vanilla packs
//! of a local BDS 1.26.51 install (Minecraft EULA: never committed) named by
//! `UNASTAR_BDS_DIR`, plus protocolgen's native block data, and writes
//! `OUT_DIR/bedrock_worldgen.rs` (see `unastar_bedrock_codegen`).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_FEATURE_BEDROCK_WORLDGEN").is_none() {
        return;
    }
    bedrock::generate();
}

#[cfg(feature = "bedrock-worldgen")]
mod bedrock {
    use std::path::PathBuf;

    pub fn generate() {
        println!("cargo:rerun-if-env-changed=UNASTAR_BDS_DIR");
        let Some(bds_dir) = std::env::var_os("UNASTAR_BDS_DIR").map(PathBuf::from) else {
            panic!(
                "the `bedrock-worldgen` feature needs UNASTAR_BDS_DIR: set it to a BDS 1.26.51 \
                 server directory (the one containing `bedrock_server`, `definitions/feature_rules`, \
                 `definitions/features` and `behavior_packs/vanilla*`). The packs are read at build \
                 time and never committed."
            );
        };
        let manifest =
            PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
        let native_dir =
            manifest.join("../valentine_gen/protocolgen/data/source/inputs/1.26.51/native");
        if !native_dir.join("canonical_block_states.nbt").is_file() {
            panic!(
                "protocolgen native data is missing at {}: run `git submodule update --init \
                 crates/valentine_gen/protocolgen`",
                native_dir.display()
            );
        }
        let model = unastar_bedrock_codegen::model::Model::load(&bds_dir, &native_dir)
            .unwrap_or_else(|e| {
                panic!(
                    "the `bedrock-worldgen` feature could not read UNASTAR_BDS_DIR={}: {e}",
                    bds_dir.display()
                )
            });
        let (source, _report) = unastar_bedrock_codegen::emit::generate(&model)
            .unwrap_or_else(|e| panic!("compiling BDS worldgen data: {e}"));
        let out =
            PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("bedrock_worldgen.rs");
        std::fs::write(&out, source).unwrap_or_else(|e| panic!("writing {}: {e}", out.display()));
        for input in &model.inputs {
            println!("cargo:rerun-if-changed={}", input.display());
        }
    }
}

#[cfg(not(feature = "bedrock-worldgen"))]
mod bedrock {
    pub fn generate() {
        unreachable!("CARGO_FEATURE_BEDROCK_WORLDGEN is only set with the feature enabled")
    }
}
