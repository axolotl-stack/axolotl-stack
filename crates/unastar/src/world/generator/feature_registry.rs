use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RuntimeConfiguredFeatureDefinition {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub config: Value,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RuntimePlacedFeatureDefinition {
    pub feature: Value,
    #[serde(default)]
    pub placement: Vec<Value>,
}

#[derive(Debug, Default)]
pub struct FeatureRuntimeRegistry {
    configured_features: HashMap<String, RuntimeConfiguredFeatureDefinition>,
    placed_features: HashMap<String, RuntimePlacedFeatureDefinition>,
}

impl FeatureRuntimeRegistry {
    pub fn global() -> &'static Self {
        &FEATURE_RUNTIME_REGISTRY
    }

    pub fn configured_feature(&self, name: &str) -> Option<&RuntimeConfiguredFeatureDefinition> {
        self.configured_features.get(normalize_registry_name(name))
    }

    pub fn placed_feature(&self, name: &str) -> Option<&RuntimePlacedFeatureDefinition> {
        self.placed_features.get(normalize_registry_name(name))
    }
}

static FEATURE_RUNTIME_REGISTRY: LazyLock<FeatureRuntimeRegistry> = LazyLock::new(|| {
    let mut registry = FeatureRuntimeRegistry::default();

    for feature_name in unastar_noise::CONFIGURED_FEATURE_NAMES {
        let raw = unastar_noise::configured_feature_json(feature_name)
            .unwrap_or_else(|| panic!("missing configured_feature registry entry: {feature_name}"));
        let parsed: RuntimeConfiguredFeatureDefinition =
            serde_json::from_str(raw).unwrap_or_else(|err| {
                panic!("invalid configured_feature JSON for {feature_name}: {err}")
            });
        registry
            .configured_features
            .insert(normalize_registry_name(feature_name).to_string(), parsed);
    }

    for feature_name in unastar_noise::PLACED_FEATURE_NAMES {
        let raw = unastar_noise::placed_feature_json(feature_name)
            .unwrap_or_else(|| panic!("missing placed_feature registry entry: {feature_name}"));
        let parsed: RuntimePlacedFeatureDefinition = serde_json::from_str(raw)
            .unwrap_or_else(|err| panic!("invalid placed_feature JSON for {feature_name}: {err}"));
        registry
            .placed_features
            .insert(normalize_registry_name(feature_name).to_string(), parsed);
    }

    registry
});

fn normalize_registry_name(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_runtime_feature_assets() {
        let registry = FeatureRuntimeRegistry::global();

        let oak = registry
            .configured_feature("oak")
            .expect("missing configured oak feature");
        assert_eq!(oak.kind, "minecraft:tree");

        let trees_plains = registry
            .placed_feature("trees_plains")
            .expect("missing placed trees_plains feature");
        assert!(!trees_plains.placement.is_empty());
    }
}
