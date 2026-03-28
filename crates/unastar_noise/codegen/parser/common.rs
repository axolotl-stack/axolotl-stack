use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct RawJsonAsset {
    pub relative_path: String,
    #[allow(dead_code)]
    pub value: Value,
}

#[derive(Debug, Clone)]
pub struct BinaryAsset {
    pub relative_path: String,
}

pub fn parse_raw_json_registry(
    dir: &Path,
) -> Result<HashMap<String, RawJsonAsset>, Box<dyn std::error::Error>> {
    let mut assets = HashMap::new();

    if !dir.exists() {
        return Ok(assets);
    }

    for entry in walkdir::WalkDir::new(dir).into_iter().flatten() {
        if !entry.path().extension().is_some_and(|ext| ext == "json") {
            continue;
        }

        let relative = entry.path().strip_prefix(dir)?;
        let name = registry_name(relative);
        let relative_path = normalize_path(relative);
        let content = std::fs::read_to_string(entry.path())?;
        let value: Value = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse JSON asset {}: {}", name, e))?;

        assets.insert(
            name,
            RawJsonAsset {
                relative_path,
                value,
            },
        );
    }

    Ok(assets)
}

pub fn collect_binary_assets(
    dir: &Path,
    extension: &str,
) -> Result<HashMap<String, BinaryAsset>, Box<dyn std::error::Error>> {
    let mut assets = HashMap::new();

    if !dir.exists() {
        return Ok(assets);
    }

    for entry in walkdir::WalkDir::new(dir).into_iter().flatten() {
        if !entry
            .path()
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
        {
            continue;
        }

        let relative = entry.path().strip_prefix(dir)?;
        let name = registry_name(relative);
        let relative_path = normalize_path(relative);

        assets.insert(name, BinaryAsset { relative_path });
    }

    Ok(assets)
}

fn registry_name(relative: &Path) -> String {
    normalize_path(&relative.with_extension(""))
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
