//! Preserve published schema defaults as build-generated data, not Rust guesses.
//!
//! Also owns the shared schema plumbing (version selection, `$ref`
//! resolution) used by the typed component body generator.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

pub type Schemas = BTreeMap<PathBuf, Arc<Value>>;

/// The highest stable registry version listing each component name, with that
/// registry's entry: either a `$ref` to the body schema or a bare
/// `"Dynamic value"` description (no published field schema).
pub fn select(files: &[PathBuf], schemas: &mut Schemas) -> BTreeMap<String, (PathBuf, Value)> {
    let mut selected = BTreeMap::<String, ([u32; 3], PathBuf, Value)>::new();
    for path in files {
        let schema = load(path, schemas);
        let version = version(schema["x-format-version"].as_str().expect("schema version"));
        for (name, body) in schema["properties"]
            .as_object()
            .expect("component schema properties")
        {
            let replace = selected.get(name).is_none_or(|(old, _, _)| version > *old);
            if replace {
                selected.insert(name.clone(), (version, path.clone(), body.clone()));
            }
        }
    }
    selected
        .into_iter()
        .map(|(name, (_, path, body))| (name, (path, body)))
        .collect()
}

pub fn write(files: &[PathBuf], out: &Path) {
    let mut schemas = Schemas::new();
    let mut defaults = BTreeMap::<String, BTreeMap<String, Value>>::new();
    for (name, (path, body)) in select(files, &mut schemas) {
        let resolved = resolve(&path, body, &mut schemas);
        let mut fields = BTreeMap::new();
        if let Some(properties) = resolved.schema.get("properties").and_then(Value::as_object) {
            for (field, body) in properties {
                if let Some(value) = body.get("default") {
                    fields.insert(field.clone(), value.clone());
                } else if body.get("$ref").is_some() {
                    let field_schema = resolve(&resolved.path, body.clone(), &mut schemas).schema;
                    if let Some(value) = field_schema.get("default") {
                        fields.insert(field.clone(), value.clone());
                    }
                }
            }
        }
        defaults.insert(name, fields);
    }
    std::fs::write(
        out.join("component_schema_defaults.json"),
        serde_json::to_vec(&defaults).expect("component schema defaults serialization"),
    )
    .expect("writing component schema defaults");
}

pub fn version(source: &str) -> [u32; 3] {
    let mut parts = source.split('.');
    let result = std::array::from_fn(|_| {
        parts
            .next()
            .expect("three schema version parts")
            .parse()
            .expect("numeric schema version")
    });
    assert!(parts.next().is_none(), "three schema version parts");
    result
}

pub fn load(path: &Path, schemas: &mut Schemas) -> Arc<Value> {
    let path = path
        .canonicalize()
        .unwrap_or_else(|cause| panic!("{}: {cause}", path.display()));
    if let Some(schema) = schemas.get(&path) {
        return Arc::clone(schema);
    }
    println!("cargo:rerun-if-changed={}", path.display());
    let bytes = std::fs::read(&path).unwrap_or_else(|cause| panic!("{}: {cause}", path.display()));
    let schema = Arc::new(
        unastar_packs::json::parse(&path.display().to_string(), &bytes)
            .unwrap_or_else(|cause| panic!("{}: {cause}", path.display())),
    );
    schemas.insert(path, Arc::clone(&schema));
    schema
}

/// A schema node with every `$ref` hop followed.
pub struct Resolved {
    /// File holding the final definition; nested references resolve from here.
    pub path: PathBuf,
    /// JSON pointer of the final definition inside `path` (`""` for the whole
    /// file, or when the input node was not a reference).
    pub pointer: String,
    /// The final definition with every hop's inline annotations
    /// (`default`, `description`, ...) merged over it.
    pub schema: Value,
}

pub fn resolve(path: &Path, body: Value, schemas: &mut Schemas) -> Resolved {
    resolve_from(path, String::new(), body, schemas, &mut Vec::new())
}

fn resolve_from(
    path: &Path,
    pointer: String,
    mut body: Value,
    schemas: &mut Schemas,
    visiting: &mut Vec<(PathBuf, String)>,
) -> Resolved {
    let Some(reference) = body.get("$ref").and_then(Value::as_str) else {
        return Resolved {
            path: path.to_owned(),
            pointer,
            schema: body,
        };
    };
    let (file, pointer) = reference.split_once('#').unwrap_or((reference, ""));
    assert!(
        !file.contains("://"),
        "component schema must use local references"
    );
    let target = if file.is_empty() {
        path.to_owned()
    } else {
        path.parent().expect("schema parent").join(decode_uri(file))
    };
    let target = target
        .canonicalize()
        .unwrap_or_else(|cause| panic!("{}: {cause}", target.display()));
    let key = (target.clone(), pointer.to_owned());
    assert!(
        !visiting.contains(&key),
        "cyclic component schema reference {}#{pointer}",
        target.display()
    );
    visiting.push(key);
    let document = load(&target, schemas);
    let referenced = if pointer.is_empty() {
        document.as_ref()
    } else {
        document.pointer(&decode_uri(pointer)).unwrap_or_else(|| {
            panic!(
                "missing component schema reference {}#{pointer}",
                target.display()
            )
        })
    };
    let mut resolved = resolve_from(
        &target,
        pointer.to_owned(),
        referenced.clone(),
        schemas,
        visiting,
    );
    visiting.pop();
    // Inline schema annotations take precedence over the referenced definition.
    let overrides = body.as_object_mut().expect("schema reference object");
    overrides.remove("$ref");
    if !overrides.is_empty() {
        resolved
            .schema
            .as_object_mut()
            .expect("referenced schema object")
            .append(overrides);
    }
    resolved
}

fn decode_uri(source: &str) -> String {
    let mut result = Vec::with_capacity(source.len());
    let mut bytes = source.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = bytes.next().expect("complete schema URI escape");
            let low = bytes.next().expect("complete schema URI escape");
            let digit =
                |byte: u8| (byte as char).to_digit(16).expect("hex schema URI escape") as u8;
            result.push(digit(high) * 16 + digit(low));
        } else {
            result.push(byte);
        }
    }
    String::from_utf8(result).expect("UTF-8 schema URI")
}
