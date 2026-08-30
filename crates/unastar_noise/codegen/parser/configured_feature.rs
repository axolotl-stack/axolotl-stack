use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConfiguredFeatureJson {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub config: Value,
}

pub fn parse_all(
    dir: &Path,
) -> Result<HashMap<String, ConfiguredFeatureJson>, Box<dyn std::error::Error>> {
    let mut configured_features = HashMap::new();

    for entry in walkdir::WalkDir::new(dir) {
        let entry = entry?;
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            let name = entry
                .path()
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .to_string();
            let content = std::fs::read_to_string(entry.path())?;
            let feature: ConfiguredFeatureJson = serde_json::from_str(&content)
                .map_err(|e| format!("Failed to parse configured feature {}: {}", name, e))?;
            configured_features.insert(name, feature);
        }
    }

    Ok(configured_features)
}
