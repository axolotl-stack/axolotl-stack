use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct NoiseParams {
    #[serde(rename = "firstOctave")]
    pub first_octave: i32,
    pub amplitudes: Vec<f64>,
}

pub fn parse_all(dir: &Path) -> Result<HashMap<String, NoiseParams>, Box<dyn std::error::Error>> {
    let mut noises = HashMap::new();

    for entry in walkdir::WalkDir::new(dir) {
        let entry = entry?;
        if entry.path().extension().is_some_and(|e| e == "json") {
            let name = entry.path().file_stem().unwrap().to_string_lossy();
            let content = std::fs::read_to_string(entry.path())?;
            let params: NoiseParams = serde_json::from_str(&content)?;
            noises.insert(format!("minecraft:{}", name), params);
        }
    }

    Ok(noises)
}
