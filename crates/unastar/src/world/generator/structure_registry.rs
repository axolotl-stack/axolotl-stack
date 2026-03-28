use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RuntimeStructureSelection {
    pub structure: String,
    pub weight: i32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RuntimeStructureSet {
    pub placement: Value,
    pub structures: Vec<RuntimeStructureSelection>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RuntimeStructureDefinition {
    #[serde(rename = "type")]
    pub kind: String,
    pub biomes: Value,
    #[serde(default)]
    pub spawn_overrides: Value,
    pub step: String,
    #[serde(default)]
    pub terrain_adaptation: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RuntimeTemplatePool {
    pub fallback: String,
    pub elements: Vec<Value>,
}

#[derive(Debug, Default)]
pub struct StructureRuntimeRegistry {
    structures: HashMap<String, RuntimeStructureDefinition>,
    structure_sets: HashMap<String, RuntimeStructureSet>,
    template_pools: HashMap<String, RuntimeTemplatePool>,
    processor_lists: HashMap<String, Value>,
    structure_templates: HashMap<String, &'static [u8]>,
}

impl StructureRuntimeRegistry {
    pub fn global() -> &'static Self {
        &STRUCTURE_RUNTIME_REGISTRY
    }

    pub fn structure(&self, name: &str) -> Option<&RuntimeStructureDefinition> {
        self.structures.get(normalize_registry_name(name))
    }

    pub fn structure_set(&self, name: &str) -> Option<&RuntimeStructureSet> {
        self.structure_sets.get(normalize_registry_name(name))
    }

    pub fn template_pool(&self, name: &str) -> Option<&RuntimeTemplatePool> {
        self.template_pools.get(normalize_registry_name(name))
    }

    pub fn processor_list(&self, name: &str) -> Option<&Value> {
        self.processor_lists.get(normalize_registry_name(name))
    }

    pub fn structure_template(&self, name: &str) -> Option<&'static [u8]> {
        self.structure_templates
            .get(normalize_registry_name(name))
            .copied()
    }
}

static STRUCTURE_RUNTIME_REGISTRY: LazyLock<StructureRuntimeRegistry> = LazyLock::new(|| {
    let mut registry = StructureRuntimeRegistry::default();

    for structure_name in unastar_noise::STRUCTURE_NAMES {
        let raw = unastar_noise::structure_json(structure_name)
            .unwrap_or_else(|| panic!("missing structure registry entry: {structure_name}"));
        let parsed: RuntimeStructureDefinition = serde_json::from_str(raw)
            .unwrap_or_else(|err| panic!("invalid structure JSON for {structure_name}: {err}"));
        registry
            .structures
            .insert(normalize_registry_name(structure_name).to_string(), parsed);
    }

    for structure_set_name in unastar_noise::STRUCTURE_SET_NAMES {
        let raw = unastar_noise::structure_set_json(structure_set_name).unwrap_or_else(|| {
            panic!("missing structure_set registry entry: {structure_set_name}")
        });
        let parsed: RuntimeStructureSet = serde_json::from_str(raw).unwrap_or_else(|err| {
            panic!("invalid structure_set JSON for {structure_set_name}: {err}")
        });
        registry.structure_sets.insert(
            normalize_registry_name(structure_set_name).to_string(),
            parsed,
        );
    }

    for template_pool_name in unastar_noise::TEMPLATE_POOL_NAMES {
        let raw = unastar_noise::template_pool_json(template_pool_name).unwrap_or_else(|| {
            panic!("missing template_pool registry entry: {template_pool_name}")
        });
        let parsed: RuntimeTemplatePool = serde_json::from_str(raw).unwrap_or_else(|err| {
            panic!("invalid template_pool JSON for {template_pool_name}: {err}")
        });
        registry.template_pools.insert(
            normalize_registry_name(template_pool_name).to_string(),
            parsed,
        );
    }

    for processor_list_name in unastar_noise::PROCESSOR_LIST_NAMES {
        let raw = unastar_noise::processor_list_json(processor_list_name).unwrap_or_else(|| {
            panic!("missing processor_list registry entry: {processor_list_name}")
        });
        let parsed: Value = serde_json::from_str(raw).unwrap_or_else(|err| {
            panic!("invalid processor_list JSON for {processor_list_name}: {err}")
        });
        registry.processor_lists.insert(
            normalize_registry_name(processor_list_name).to_string(),
            parsed,
        );
    }

    for template_name in unastar_noise::STRUCTURE_TEMPLATE_NAMES {
        let bytes = unastar_noise::structure_template_nbt(template_name)
            .unwrap_or_else(|| panic!("missing structure template asset: {template_name}"));
        registry
            .structure_templates
            .insert(normalize_registry_name(template_name).to_string(), bytes);
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
    fn exposes_runtime_structure_assets() {
        let registry = StructureRuntimeRegistry::global();

        let village = registry
            .structure("village_plains")
            .expect("missing village_plains");
        assert_eq!(village.kind, "minecraft:jigsaw");
        assert_eq!(village.step, "surface_structures");

        let villages = registry
            .structure_set("villages")
            .expect("missing villages");
        assert_eq!(villages.structures.len(), 5);

        let town_centers = registry
            .template_pool("village/plains/town_centers")
            .expect("missing village/plains/town_centers");
        assert!(!town_centers.elements.is_empty());

        let watchtower = registry
            .structure_template("pillager_outpost/watchtower")
            .expect("missing pillager_outpost/watchtower");
        assert!(!watchtower.is_empty());
    }
}
