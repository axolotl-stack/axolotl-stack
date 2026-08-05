//! Data-driven corrections for known errors in Mojang's published schemas.
//!
//! The files in `crates/valentine_gen/overrides` are deliberately applied to
//! parsed `serde_json::Value`s before the Mojang frontend builds the Valentine
//! IR.  This keeps protocol-specific corrections reviewable and makes them
//! survive regeneration of the generated Rust output.

use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub fn apply(
    documents: &mut HashMap<String, Value>,
    override_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if !override_dir.exists() {
        return Ok(());
    }

    let mut files = fs::read_dir(override_dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect::<Vec<_>>();
    files.sort();

    for path in files {
        let contents = fs::read_to_string(&path)?;
        let patch_file: Value = serde_json::from_str(&contents)
            .map_err(|error| format!("failed to parse override {}: {error}", path.display()))?;
        let source = patch_file
            .get("source")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("override {} is missing source", path.display()))?;
        if source.trim().is_empty() {
            return Err(format!("override {} has an empty source", path.display()).into());
        }
        let operations = patch_file
            .get("operations")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("override {} is missing operations", path.display()))?;
        for operation in operations {
            apply_operation(documents, operation)
                .map_err(|error| format!("{}: {error}", path.display()))?;
        }
    }

    Ok(())
}

fn apply_operation(
    documents: &mut HashMap<String, Value>,
    operation: &Value,
) -> Result<(), String> {
    let object = operation
        .as_object()
        .ok_or_else(|| "override operation must be an object".to_string())?;
    let op = object
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| "override operation is missing op".to_string())?;
    let why = object
        .get("why")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("override operation {op} is missing why"))?;
    if why.trim().is_empty() {
        return Err(format!("override operation {op} has an empty why"));
    }

    if op == "add_document" {
        let file = object
            .get("file")
            .and_then(Value::as_str)
            .ok_or_else(|| "add_document is missing file".to_string())?;
        let document = object
            .get("document")
            .ok_or_else(|| format!("add_document {file} is missing document"))?;
        // Keep a future upstream definition authoritative if Mojang adds it;
        // this operation only fills a documented hole in older snapshots.
        documents
            .entry(file.to_string())
            .or_insert_with(|| document.clone());
        return Ok(());
    }

    let serialization_option = object.get("option").and_then(Value::as_str);

    let mut changed = false;
    for (file_name, document) in documents.iter_mut() {
        let mut visit = |node: &mut Map<String, Value>| {
            let schema_matches = matches_schema(node, file_name, object);
            match op {
                "remove_required" | "add_required" => {
                    if !schema_matches {
                        return;
                    }
                    let Some(field) = object.get("field").and_then(Value::as_str) else {
                        return;
                    };
                    let required = node
                        .entry("required")
                        .or_insert_with(|| Value::Array(Vec::new()));
                    let Some(required) = required.as_array_mut() else {
                        return;
                    };
                    if op == "remove_required" {
                        let before = required.len();
                        required.retain(|value| value.as_str() != Some(field));
                        changed |= before != required.len();
                    } else if !required.iter().any(|value| value.as_str() == Some(field)) {
                        required.push(Value::String(field.to_string()));
                        changed = true;
                    }
                }
                "add_enum_values" => {
                    if !schema_matches {
                        return;
                    }
                    let Some(values) = object.get("values").and_then(Value::as_array) else {
                        return;
                    };
                    if let Some(enumeration) = node.get_mut("enum").and_then(Value::as_array_mut) {
                        for value in values {
                            if !enumeration.contains(value) {
                                enumeration.push(value.clone());
                                changed = true;
                            }
                        }
                    }
                }
                "patch_property" | "double_optional" => {
                    if !schema_matches {
                        return;
                    }
                    let Some(field) = object.get("field").and_then(Value::as_str) else {
                        return;
                    };
                    let Some(properties) =
                        node.get_mut("properties").and_then(Value::as_object_mut)
                    else {
                        return;
                    };
                    let Some(property) = properties.get_mut(field).and_then(Value::as_object_mut)
                    else {
                        return;
                    };
                    if op == "double_optional" {
                        let options = property
                            .entry("x-serialization-options")
                            .or_insert_with(|| Value::Array(Vec::new()));
                        let Some(options) = options.as_array_mut() else {
                            return;
                        };
                        let marker = Value::String("+double-optional".to_string());
                        if !options.contains(&marker) {
                            options.push(marker);
                            changed = true;
                        }
                    } else if let Some(patch) = object.get("patch").and_then(Value::as_object) {
                        for (key, value) in patch {
                            if property.get(key) != Some(value) {
                                property.insert(key.clone(), value.clone());
                                changed = true;
                            }
                        }
                    }
                }
                "add_serialization_option" => {
                    let selector = object.get("selector").and_then(Value::as_str);
                    if selector == Some("oneOf_with_control")
                        && node.get("oneOf").and_then(Value::as_array).is_some()
                        && node.get("x-control-value-type").is_some()
                    {
                        let Some(option) = serialization_option else {
                            return;
                        };
                        let options = node
                            .entry("x-serialization-options")
                            .or_insert_with(|| Value::Array(Vec::new()));
                        let Some(options) = options.as_array_mut() else {
                            return;
                        };
                        let value = Value::String(option.to_string());
                        if !options.contains(&value) {
                            options.push(value);
                            changed = true;
                        }
                    }
                }
                "replace_ref" => {
                    let file_selector = object
                        .get("schema")
                        .or_else(|| object.get("file"))
                        .and_then(Value::as_str);
                    if file_selector.is_some_and(|selector| {
                        selector != file_name && !file_name.ends_with(selector)
                    }) {
                        return;
                    }
                    let Some(from) = object.get("from").and_then(Value::as_str) else {
                        return;
                    };
                    let Some(to) = object.get("to").and_then(Value::as_str) else {
                        return;
                    };
                    if node.get("$ref").and_then(Value::as_str) == Some(from) {
                        node.insert("$ref".to_string(), Value::String(to.to_string()));
                        changed = true;
                    }
                }
                _ => {}
            }
        };
        visit_objects(document, &mut visit);
    }

    if op != "add_serialization_option" && !changed {
        // A correction can legitimately be a no-op for an older/newer schema
        // snapshot, so do not fail generation. The explicit `why` remains the
        // audit trail for that intentional tolerance.
    }

    Ok(())
}

fn matches_schema(
    node: &Map<String, Value>,
    file_name: &str,
    operation: &Map<String, Value>,
) -> bool {
    let file_selector = operation
        .get("schema")
        .or_else(|| operation.get("file"))
        .and_then(Value::as_str);
    if let Some(selector) = file_selector {
        if selector != file_name && !file_name.ends_with(selector) {
            return false;
        }
    }

    let title_selector = operation
        .get("schema_title")
        .or_else(|| operation.get("title"))
        .and_then(Value::as_str);
    if let Some(selector) = title_selector {
        if node.get("title").and_then(Value::as_str) != Some(selector) {
            return false;
        }
    }

    file_selector.is_some() || title_selector.is_some()
}

fn visit_objects<F>(value: &mut Value, visit: &mut F)
where
    F: FnMut(&mut Map<String, Value>),
{
    match value {
        Value::Object(object) => {
            visit(object);
            for child in object.values_mut() {
                visit_objects(child, visit);
            }
        }
        Value::Array(array) => {
            for child in array {
                visit_objects(child, visit);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::apply;
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;

    #[test]
    fn applies_required_enum_double_optional_and_global_rules() {
        let directory =
            std::env::temp_dir().join(format!("valentine-gen-overrides-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("create override fixture");
        fs::write(
            directory.join("fixture.json"),
            serde_json::to_vec_pretty(&json!({
                "source": "https://example.invalid/fixture",
                "operations": [
                    {"op":"remove_required", "schema":"Example.json", "field":"Optional", "why":"test requiredness correction"},
                    {"op":"add_enum_values", "schema_title":"ExampleEnum", "values":["Legacy"], "why":"test enum correction"},
                    {"op":"double_optional", "schema":"Example.json", "field":"Optional", "why":"test double presence correction"},
                    {"op":"add_serialization_option", "selector":"oneOf_with_control", "option":"Compression", "why":"test discriminator correction"},
                    {"op":"add_document", "file":"Added.json", "document":{"title":"Added", "type":"string"}, "why":"test document correction"},
                    {"op":"replace_ref", "from":"#/definitions/legacy", "to":"Added.json", "why":"test reference correction"}
                ]
            }))
            .expect("serialize override"),
        )
        .expect("write override");

        let mut documents = HashMap::from([(
            "Example.json".to_string(),
            json!({
                "title":"Example",
                "type":"object",
                "required":["Optional"],
                "properties": {
                    "Optional": {"type":"integer"}
                },
                "definitions": {
                    "enum": {"title":"ExampleEnum", "enum":["Current"]}
                },
                "variant": {"oneOf":[], "x-control-value-type":"uint32"},
                "ref": {"$ref":"#/definitions/legacy"}
            }),
        )]);
        apply(&mut documents, &directory).expect("apply overrides");

        let document = &documents["Example.json"];
        assert_eq!(document["required"], json!([]));
        assert_eq!(
            document["properties"]["Optional"]["x-serialization-options"],
            json!(["+double-optional"])
        );
        assert_eq!(
            document["definitions"]["enum"]["enum"],
            json!(["Current", "Legacy"])
        );
        assert_eq!(
            document["variant"]["x-serialization-options"],
            json!(["Compression"])
        );
        assert_eq!(document["ref"]["$ref"], "Added.json");
        assert_eq!(documents["Added.json"]["title"], "Added");

        let _ = fs::remove_dir_all(directory);
    }
}
