//! Fingerprint native datapacks before declaring an equivalent input recipe.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};

use super::java_canonical_inputs::{DataProfile, files, fingerprint, vanilla_biomes};
pub fn inspect_profile(world: &Path, enabled: &[String]) -> Result<DataProfile, String> {
    if enabled == ["vanilla"] {
        return Ok(DataProfile::Vanilla);
    }
    let mut packs = BTreeMap::new();
    let mut fingerprints = BTreeMap::new();
    for name in enabled.iter().filter(|name| name.as_str() != "vanilla") {
        let Some(relative) = name.strip_prefix("file/") else {
            return Err(format!(
                "cannot fingerprint non-file enabled datapack {name}"
            ));
        };
        let path = world.join("datapacks").join(relative);
        let bytes = if path.is_dir() {
            files(&path)?
        } else {
            BTreeMap::from([(
                String::new(),
                std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?,
            )])
        };
        fingerprints.insert(name.clone(), fingerprint(&bytes));
        packs.insert(name.clone(), bytes);
    }
    if enabled.len() == 2
        && enabled[0] == "vanilla"
        && let Some(overrides) = packs.get(&enabled[1])
    {
        let vanilla = vanilla_biomes()?;
        let data_files: BTreeMap<_, _> = overrides
            .iter()
            .filter(|(name, _)| name.starts_with("data/"))
            .collect();
        let mut matches = data_files.len() == vanilla.len();
        for (name, bytes) in &vanilla {
            let key = format!("data/minecraft/worldgen/biome/{name}");
            let Some(actual) = overrides.get(&key) else {
                matches = false;
                break;
            };
            let mut expected: Value =
                serde_json::from_slice(bytes).map_err(|e| format!("pinned biome {name}: {e}"))?;
            expected
                .as_object_mut()
                .ok_or("pinned biome must be an object")?
                .insert("features".into(), json!([]));
            let actual: Value = serde_json::from_slice(actual)
                .map_err(|e| format!("datapack biome {name}: {e}"))?;
            if expected != actual {
                matches = false;
                break;
            }
        }
        if matches {
            return Ok(DataProfile::EmptyBiomeFeatures {
                vanilla_biomes_sha256: fingerprint(&vanilla),
                overrides_sha256: fingerprints[&enabled[1]].clone(),
            });
        }
    }
    Ok(DataProfile::Unresolved {
        enabled_pack_sha256: fingerprints,
    })
}
