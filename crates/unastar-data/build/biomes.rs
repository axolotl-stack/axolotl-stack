//! Native identities and measurements enriched with pack components. Native
//! entries without a pack row stay native-only; no gameplay defaults are added.

use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use unastar_packs::packs::{Family, PackStack};

pub fn write(pack: &Path, native: &Path, out: &Path) {
    fn native_json(path: &Path) -> Value {
        println!("cargo:rerun-if-changed={}", path.display());
        let data = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        serde_json::from_slice(&data).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }
    let ids: BTreeMap<String, u32> =
        serde_json::from_value(native_json(&native.join("biome_id_map.json")))
            .expect("native biome ids");
    let mut definitions: BTreeMap<String, Value> =
        serde_json::from_value(native_json(&native.join("biome_definitions.json")))
            .expect("native biome definitions");
    assert!(!ids.is_empty(), "native biome ids are empty");
    let mut seen = BTreeSet::new();
    for id in ids.values() {
        assert!(seen.insert(*id), "duplicate native biome id {id}");
    }
    let stack = PackStack::single_pack("bedrock-samples", pack);
    println!("cargo:rerun-if-changed={}", pack.join("biomes").display());
    for input in stack.inputs(Family::Biomes) {
        println!("cargo:rerun-if-changed={}", input.display());
    }
    let mut gameplay = stack
        .resolve(Family::Biomes)
        .unwrap_or_else(|e| panic!("biomes: {e}"));
    for id in gameplay.keys() {
        assert!(
            ids.contains_key(id),
            "pack biome {id} is missing a native id"
        );
    }
    let mut rows = Vec::with_capacity(ids.len());
    for (identifier, id) in ids {
        let definition = definitions
            .remove(&identifier)
            .unwrap_or_else(|| panic!("{identifier}: missing native definition"));
        for field in [
            "temperature",
            "downfall",
            "redSporeDensity",
            "blueSporeDensity",
            "ashDensity",
            "whiteAshDensity",
            "snowAccumulation",
            "foliageSnow",
            "minSnowLevel",
            "maxSnowLevel",
            "depth",
            "scale",
            "waterTransparency",
        ] {
            assert!(
                definition[field].as_f64().is_some(),
                "{identifier}: missing native numeric {field}"
            );
        }
        assert!(
            definition["rain"].as_bool().is_some(),
            "{identifier}: missing native rain"
        );
        assert!(
            definition["mapWaterColor"].as_str().is_some(),
            "{identifier}: missing native mapWaterColor"
        );
        let doc = gameplay.remove(&identifier);
        let format_version = doc.as_ref().map_or("", |d| d.format_version.as_str());
        let components = doc
            .as_ref()
            .and_then(|d| d.body.get("components"))
            .cloned()
            .unwrap_or_else(|| json!({}));
        assert!(
            components.is_object(),
            "{identifier}: biome components must be an object"
        );
        let source_file = doc.as_ref().map_or("", |d| {
            d.source.split_once(':').map_or("", |(_, file)| file)
        });
        rows.push(json!({
            "id": id,
            "identifier": identifier,
            "native_definition": definition,
            "format_version": format_version,
            "source_file": source_file,
            "components": components,
        }));
    }
    assert!(
        definitions.is_empty(),
        "native biome definitions missing ids: {:?}",
        definitions.keys()
    );
    std::fs::write(
        out.join("biomes.json"),
        serde_json::to_vec(&rows).expect("biomes serialize"),
    )
    .expect("writing embedded biomes");
}
