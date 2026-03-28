use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlacedFeatureJson {
    pub feature: Value,
    #[serde(default)]
    pub placement: Vec<Value>,
}

pub fn parse_all(
    dir: &Path,
) -> Result<HashMap<String, PlacedFeatureJson>, Box<dyn std::error::Error>> {
    let mut placed_features = HashMap::new();

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
            let feature: PlacedFeatureJson = serde_json::from_str(&content)
                .map_err(|e| format!("Failed to parse placed feature {}: {}", name, e))?;
            placed_features.insert(name, feature);
        }
    }

    Ok(placed_features)
}
