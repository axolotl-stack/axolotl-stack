//! Merge layers: defaults → vanilla → overrides

use crate::ingest::Overrides;
use crate::ir::*;
use std::collections::HashMap;
use tracing::debug;

/// Merge layers: defaults → vanilla → overrides
pub fn merge_layers(
    mut vanilla: HashMap<String, EntityDef>,
    overrides: &Overrides,
) -> HashMap<String, EntityDef> {
    for (name, entity) in vanilla.iter_mut() {
        debug!("Merging entity: {}", name);

        // Layer 1: Apply defaults to missing components
        apply_defaults(entity, &overrides.defaults.components);

        // Layer 2: Apply entity-specific overrides
        if let Some(entity_override) = overrides.entities.get(name) {
            apply_entity_override(entity, entity_override, name);
        }
    }

    vanilla
}

fn apply_defaults(entity: &mut EntityDef, defaults: &HashMap<String, serde_json::Value>) {
    for (comp_name, default_value) in defaults {
        // Only apply if entity doesn't have this component
        if !entity.components.contains_key(comp_name) {
            entity.components.insert(
                comp_name.clone(),
                ComponentValue::Data(default_value.clone()),
            );
            entity
                .attribution
                .component_sources
                .insert(comp_name.clone(), Source::Defaults);
        }
    }
}

fn apply_entity_override(
    entity: &mut EntityDef,
    override_data: &crate::ingest::EntityOverride,
    entity_name: &str,
) {
    for (comp_name, override_value) in &override_data.components {
        // Check what exists first before modifying
        let needs_merge = matches!(
            entity.components.get(comp_name),
            Some(ComponentValue::Data(_))
        );

        if needs_merge {
            if let Some(ComponentValue::Data(existing)) = entity.components.get_mut(comp_name) {
                merge_json_objects(existing, override_value);
            }
        } else {
            // Replace marker or insert new
            entity.components.insert(
                comp_name.clone(),
                ComponentValue::Data(override_value.clone()),
            );
        }

        entity.attribution.component_sources.insert(
            comp_name.clone(),
            Source::Override(format!("{}.kdl", entity_name)),
        );
    }
}

/// Deep merge two JSON objects
fn merge_json_objects(base: &mut serde_json::Value, overlay: &serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(base_obj), serde_json::Value::Object(overlay_obj)) => {
            for (key, value) in overlay_obj {
                match base_obj.get_mut(key) {
                    Some(base_value) if base_value.is_object() && value.is_object() => {
                        merge_json_objects(base_value, value);
                    }
                    _ => {
                        base_obj.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (base, overlay) => {
            *base = overlay.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_json_objects() {
        let mut base = serde_json::json!({
            "value": 10,
            "nested": { "a": 1 }
        });
        let overlay = serde_json::json!({
            "value": 20,
            "nested": { "b": 2 },
            "new_field": true
        });

        merge_json_objects(&mut base, &overlay);

        assert_eq!(base["value"], 20);
        assert_eq!(base["nested"]["a"], 1);
        assert_eq!(base["nested"]["b"], 2);
        assert_eq!(base["new_field"], true);
    }
}
