//! Native inputs shared by the capture producer and canonical comparator.
//! Filesystem inspection and runtime recipe validation live in their consuming
//! examples, so neither binary carries unused policy implementations.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inputs {
    pub worldgen_settings: Value,
    pub enabled_features: Vec<String>,
    pub data_profile: DataProfile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DataProfile {
    Vanilla,
    EmptyBiomeFeatures {
        vanilla_biomes_sha256: String,
        overrides_sha256: String,
    },
    Unresolved {
        enabled_pack_sha256: BTreeMap<String, String>,
    },
}

pub(super) fn files(root: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    fn visit(
        root: &Path,
        dir: &Path,
        output: &mut BTreeMap<String, Vec<u8>>,
    ) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() {
                visit(root, &path, output)?;
            } else if kind.is_file() {
                let name = path
                    .strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_str()
                    .ok_or("non-UTF8 datapack path")?
                    .replace('\\', "/");
                output.insert(
                    name,
                    std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?,
                );
            } else {
                return Err(format!(
                    "{}: non-regular native input cannot be fingerprinted",
                    path.display()
                ));
            }
        }
        Ok(())
    }
    let mut output = BTreeMap::new();
    visit(root, root, &mut output)?;
    Ok(output)
}

pub(super) fn fingerprint(files: &BTreeMap<String, Vec<u8>>) -> String {
    let mut digest = Sha256::new();
    for (name, bytes) in files {
        digest.update((name.len() as u64).to_be_bytes());
        digest.update(name.as_bytes());
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    format!("{:x}", digest.finalize())
}

pub(super) fn vanilla_biomes() -> Result<BTreeMap<String, Vec<u8>>, String> {
    files(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../unastar_noise/worldgen_data/biome"
    )))
}
