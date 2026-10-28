//! Validate exact native generator inputs; unsupported recipes fail closed.

use serde_json::{Value, json};

use super::java_canonical_inputs::{DataProfile, Inputs, fingerprint, vanilla_biomes};
fn member<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    if value["type"] != "Compound" {
        return None;
    }
    value["value"].as_array()?.iter().find_map(|pair| {
        (pair[0].as_str() == Some(name)).then_some(&pair[1])
    })
}

fn typed_strings(value: Value) -> Value {
    match value {
        Value::String(value) => json!({"type":"Str", "value":value}),
        Value::Object(values) => {
            let mut entries: Vec<_> = values.into_iter()
                .map(|(name, value)| json!([name, typed_strings(value)])).collect();
            entries.sort_unstable_by(|a, b| a[0].as_str().cmp(&b[0].as_str()));
            json!({"type":"Compound", "value":entries})
        }
        _ => unreachable!("vanilla dimension identity contains only strings and compounds"),
    }
}

pub(crate) fn vanilla_dimension(dimension: &str) -> Result<Value, String> {
    let (dimension_type, settings, biome_source) = match dimension {
        "minecraft:overworld" => ("minecraft:overworld", "minecraft:overworld",
            json!({"type":"minecraft:multi_noise", "preset":"minecraft:overworld"})),
        "minecraft:the_nether" => ("minecraft:the_nether", "minecraft:nether",
            json!({"type":"minecraft:multi_noise", "preset":"minecraft:nether"})),
        "minecraft:the_end" => ("minecraft:the_end", "minecraft:end", json!({"type":"minecraft:the_end"})),
        other => return Err(format!("unsupported native dimension {other}")),
    };
    Ok(typed_strings(json!({"type":dimension_type, "generator":{
        "type":"minecraft:noise", "settings":settings, "biome_source":biome_source
    }})))
}

impl Inputs {
    pub fn validate_metadata(&self, seed: i64, structures: bool, dimension: &str) -> Result<(), String> {
        let settings = &self.worldgen_settings;
        if member(settings, "seed") != Some(&json!({"type":"Long", "value":seed}))
            || member(settings, "generate_features") != Some(&json!({"type":"Byte", "value":i8::from(structures)})) {
            return Err("captured native seed/structure options disagree with the header".into());
        }
        if member(settings, "dimensions").and_then(|dimensions| member(dimensions, dimension)).is_none() {
            return Err("captured native settings omit the requested dimension".into());
        }
        Ok(())
    }

    /// Whether biome features are enabled. Unsupported native inputs are errors,
    /// not a guessed vanilla fallback. Structure enablement remains a separate input.
    pub fn validate_generation(&self, seed: i64, structures: bool, dimension: &str, datapacks: &[String]) -> Result<bool, String> {
        self.validate_metadata(seed, structures, dimension)?;
        let settings = &self.worldgen_settings;
        if self.enabled_features != ["minecraft:vanilla"] {
            return Err("unsupported native enabled feature flags".into());
        }
        if member(settings, "bonus_chest") != Some(&json!({"type":"Byte", "value":0})) {
            return Err("native world requests an unimplemented bonus chest".into());
        }
        let dimensions = member(settings, "dimensions").ok_or("missing native dimension settings")?;
        if member(dimensions, dimension) != Some(&vanilla_dimension(dimension)?) {
            return Err(format!("native {dimension} generator is not the pinned vanilla preset"));
        }
        match &self.data_profile {
            DataProfile::Vanilla if datapacks == ["vanilla"] => Ok(true),
            DataProfile::EmptyBiomeFeatures { vanilla_biomes_sha256, .. }
                if datapacks.len() == 2 && datapacks[0] == "vanilla"
                    && vanilla_biomes_sha256 == &fingerprint(&vanilla_biomes()?) => Ok(false),
            _ => Err("unsupported native datapack contents; no equivalent Rust input recipe".into()),
        }
    }
}
