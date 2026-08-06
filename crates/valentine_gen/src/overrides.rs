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
    let mut matched = false;
    let mut invalid: Option<String> = None;
    for (file_name, document) in documents.iter_mut() {
        let mut visit = |node: &mut Map<String, Value>| {
            let schema_matches = matches_schema(node, file_name, object);
            match op {
                "remove_required" | "add_required" => {
                    if !schema_matches {
                        return;
                    }
                    if node.get("properties").is_none() && node.get("required").is_none() {
                        return;
                    }
                    let Some(field) = object.get("field").and_then(Value::as_str) else {
                        invalid = Some(format!("{op} is missing field"));
                        return;
                    };
                    if !node
                        .get("properties")
                        .and_then(Value::as_object)
                        .is_some_and(|properties| properties.contains_key(field))
                    {
                        return;
                    }
                    matched = true;
                    let required = node
                        .entry("required")
                        .or_insert_with(|| Value::Array(Vec::new()));
                    let Some(required) = required.as_array_mut() else {
                        invalid = Some(format!("{op} target has a non-array required value"));
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
                    if node.get("enum").and_then(Value::as_array).is_none() {
                        return;
                    }
                    matched = true;
                    let Some(values) = object.get("values").and_then(Value::as_array) else {
                        invalid = Some("add_enum_values is missing values".to_string());
                        return;
                    };
                    let Some(enumeration) = node.get_mut("enum").and_then(Value::as_array_mut)
                    else {
                        invalid = Some("add_enum_values target has no enum array".to_string());
                        return;
                    };
                    for value in values {
                        if !enumeration.contains(value) {
                            enumeration.push(value.clone());
                            changed = true;
                        }
                    }
                }
                "set_enum_values" => {
                    if !schema_matches {
                        return;
                    }
                    let Some(enumeration) = node.get("enum").and_then(Value::as_array) else {
                        return;
                    };
                    let Some(values) = object.get("values").and_then(Value::as_array) else {
                        invalid = Some("set_enum_values is missing values".to_string());
                        return;
                    };
                    if values.len() != enumeration.len()
                        || values.iter().any(|value| value_as_i64(value).is_none())
                    {
                        invalid =
                            Some("set_enum_values length or values do not match enum".to_string());
                        return;
                    }
                    matched = true;
                    // A more specific correction (for example Animate's
                    // omitted legacy value) is authoritative when it ran
                    // earlier in the lexically ordered override set.
                    if node.get("x-enum-values").is_none() {
                        node.insert("x-enum-values".to_string(), Value::Array(values.clone()));
                        changed = true;
                    }
                }
                "patch_schema" => {
                    if !schema_matches {
                        return;
                    }
                    let Some(patch) = object.get("patch").and_then(Value::as_object) else {
                        invalid = Some("patch_schema is missing patch".to_string());
                        return;
                    };
                    matched = true;
                    for (key, value) in patch {
                        if node.get(key) != Some(value) {
                            node.insert(key.clone(), value.clone());
                            changed = true;
                        }
                    }
                }
                "replace_schema" => {
                    if !schema_matches {
                        return;
                    }
                    let Some(replacement) = object.get("replacement").and_then(Value::as_object)
                    else {
                        invalid = Some("replace_schema is missing replacement".to_string());
                        return;
                    };
                    matched = true;
                    if node != replacement {
                        *node = replacement.clone();
                        changed = true;
                    }
                }
                "patch_property" | "double_optional" | "select_one_of_branch" => {
                    if !schema_matches {
                        return;
                    }
                    if op == "select_one_of_branch" {
                        let Some(index) = object.get("index").and_then(Value::as_u64) else {
                            invalid =
                                Some("select_one_of_branch is missing a numeric index".to_string());
                            return;
                        };
                        let Some(branch) = node
                            .get("oneOf")
                            .and_then(Value::as_array)
                            .and_then(|branches| branches.get(index as usize))
                            .and_then(Value::as_object)
                        else {
                            invalid = Some(format!(
                                "select_one_of_branch index {index} is not present in oneOf"
                            ));
                            return;
                        };
                        matched = true;
                        *node = branch.clone();
                        changed = true;
                        return;
                    }
                    if node.get("properties").is_none() {
                        return;
                    }
                    let Some(field) = object.get("field").and_then(Value::as_str) else {
                        invalid = Some(format!("{op} is missing field"));
                        return;
                    };
                    let Some(properties) =
                        node.get_mut("properties").and_then(Value::as_object_mut)
                    else {
                        invalid = Some(format!("{op} target has no properties object"));
                        return;
                    };
                    let Some(property) = properties.get_mut(field).and_then(Value::as_object_mut)
                    else {
                        return;
                    };
                    matched = true;
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
                        && node.get("x-control-value-type").and_then(Value::as_str)
                            != Some("boolean")
                    {
                        matched = true;
                        let Some(option) = serialization_option else {
                            invalid =
                                Some("add_serialization_option is missing option".to_string());
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
                        matched = true;
                        node.insert("$ref".to_string(), Value::String(to.to_string()));
                        changed = true;
                    }
                }
                _ => invalid = Some(format!("unsupported override operation {op}")),
            }
        };
        visit_objects(document, &mut visit);
    }

    if let Some(invalid) = invalid {
        return Err(invalid);
    }
    if !matched {
        let schema = object
            .get("schema")
            .or_else(|| object.get("schema_title"))
            .or_else(|| object.get("file"))
            .and_then(Value::as_str)
            .unwrap_or("<any schema>");
        let field = object
            .get("field")
            .and_then(Value::as_str)
            .unwrap_or("<any field>");
        let reference = object
            .get("from")
            .and_then(Value::as_str)
            .map(|from| format!(", reference {from}"))
            .unwrap_or_default();
        return Err(format!(
            "override operation {op} did not match any schema node \
             (schema {schema}, field {field}{reference})"
        ));
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

    if let Some(expected) = operation.get("match_enum").and_then(Value::as_array) {
        if node.get("enum").and_then(Value::as_array) != Some(expected) {
            return false;
        }
    }

    file_selector.is_some() || title_selector.is_some()
}

fn value_as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
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
                    {"op":"patch_schema", "schema_title":"Example", "patch":{"x-fixture":"patched"}, "why":"test schema correction"},
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
        assert_eq!(document["x-fixture"], "patched");
        assert_eq!(documents["Added.json"]["title"], "Added");

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn rejects_unmatched_corrections() {
        let directory = std::env::temp_dir().join(format!(
            "valentine-gen-overrides-unmatched-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("create unmatched override fixture");
        fs::write(
            directory.join("fixture.json"),
            serde_json::to_vec(&json!({
                "source": "https://example.invalid/fixture",
                "operations": [{
                    "op": "double_optional",
                    "schema_title": "MissingSchema",
                    "field": "MissingField",
                    "why": "test unmatched correction"
                }]
            }))
            .expect("serialize unmatched override"),
        )
        .expect("write unmatched override");

        let mut documents = HashMap::from([(
            "Example.json".to_string(),
            json!({"title":"Example", "type":"object", "properties": {}}),
        )]);
        let error = apply(&mut documents, &directory).expect_err("unmatched correction fails");
        assert!(error.to_string().contains("did not match"));
        assert!(error.to_string().contains("MissingSchema"));
        assert!(error.to_string().contains("MissingField"));
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn rejects_missing_field_in_file_scoped_property_patch() {
        let directory = std::env::temp_dir().join(format!(
            "valentine-gen-overrides-missing-field-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("create missing-field override fixture");
        fs::write(
            directory.join("fixture.json"),
            serde_json::to_vec(&json!({
                "source": "https://example.invalid/fixture",
                "operations": [{
                    "op": "patch_property",
                    "schema": "Example.json",
                    "field": "MissingField",
                    "patch": {"type": "string"},
                    "why": "test file-scoped field diagnostics"
                }]
            }))
            .expect("serialize missing-field override"),
        )
        .expect("write missing-field override");

        let mut documents = HashMap::from([(
            "Example.json".to_string(),
            json!({"title":"Example", "type":"object", "properties": {}}),
        )]);
        let error = apply(&mut documents, &directory).expect_err("missing field fails closed");
        let message = error.to_string();
        assert!(message.contains("Example.json"));
        assert!(message.contains("MissingField"));
        let _ = fs::remove_dir_all(directory);
    }
}
