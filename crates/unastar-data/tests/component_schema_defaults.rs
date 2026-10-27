#[path = "../build/component_defaults.rs"]
mod component_defaults;

use serde_json::json;

#[test]
fn numeric_schema_versions_and_referenced_defaults_preserve_false_and_zero() {
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join("old.json");
    let new = root.path().join("new.json");
    std::fs::write(
        &old,
        serde_json::to_vec(&json!({
            "x-format-version": "1.21.90",
            "properties": {"minecraft:test": {"properties": {
                "enabled": {"default": true}, "count": {"default": 99}
            }}}
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        &new,
        serde_json::to_vec(&json!({
            "x-format-version": "1.21.130",
            "properties": {"minecraft:test": {"$ref": "./Base%20Settings.json#/$defs/Shape"}}
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.path().join("Base Settings.json"),
        serde_json::to_vec(&json!({
            "$defs": {
                "Shape": {"properties": {
                    "enabled": {"$ref": "#/$defs/Enabled", "default": false},
                    "count": {"$ref": "#/$defs/Count"},
                    "zero": {"default": 0},
                    "no_default": {"type": "number"}
                }},
                "Enabled": {"type": "boolean", "default": true},
                "Count": {"type": "integer", "default": 7}
            }
        }))
        .unwrap(),
    )
    .unwrap();
    component_defaults::write(&[new, old], root.path());
    let output: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.path().join("component_schema_defaults.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        output["minecraft:test"],
        json!({"enabled": false, "count": 7, "zero": 0})
    );
}

#[test]
fn cyclic_schema_reference_is_an_error_not_an_invented_default() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("cycle.json");
    std::fs::write(
        &input,
        serde_json::to_vec(&json!({
            "x-format-version": "1.26.50",
            "properties": {"minecraft:test": {"$ref": "#/$defs/A"}},
            "$defs": {"A": {"$ref": "#/$defs/B"}, "B": {"$ref": "#/$defs/A"}}
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(std::panic::catch_unwind(|| component_defaults::write(&[input], root.path())).is_err());
}
