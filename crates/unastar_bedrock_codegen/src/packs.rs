//! Version-stack resolution of the vanilla BDS behaviour packs.
//!
//! BDS loads data for a world from a stack of layers, later layers overriding
//! earlier ones by identifier:
//!
//! 1. `definitions/<family>/*.json` (only `features` and `feature_rules` exist
//!    there; they are the oldest copies, every overlap is overridden by a pack),
//! 2. `behavior_packs/vanilla`, then every `behavior_packs/vanilla_<version>`
//!    in ascending version order up to the world's base game version.
//!
//! Experimental, chemistry, editor and library packs are not part of a default
//! world and are ignored. Inside one layer a family is read from
//! `__brarchive/<family>.brarchive`, nested `__brarchive/<family>/**.brarchive`
//! archives, and loose `<family>/**.json` files, in that order. The same
//! identifier twice in one layer is an error.
//!
//! The base game version is fixed to the BDS release unastar targets; the
//! source layer of every resolved document is kept so emitted code and reports
//! can cite it.

use crate::brarchive;
use crate::error::{Error, Result};
use crate::json;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Base game version unastar's Bedrock-style generator reproduces.
pub const BASE_GAME_VERSION: [u32; 3] = [1, 26, 51];

/// A pack data family, named by its path below the pack root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Family {
    Biomes,
    FeatureRules,
    Features,
    SpawnRules,
    /// `worldgen/structures`: jigsaw structure definitions.
    JigsawStructures,
    /// `worldgen/structure_sets`: placement of jigsaw structures.
    StructureSets,
    /// `worldgen/template_pools`: jigsaw template pools.
    TemplatePools,
    /// `worldgen/processors`: structure processor lists.
    Processors,
}

impl Family {
    /// Path of the family below a pack root (and below `__brarchive`).
    pub fn path(self) -> &'static str {
        match self {
            Self::Biomes => "biomes",
            Self::FeatureRules => "feature_rules",
            Self::Features => "features",
            Self::SpawnRules => "spawn_rules",
            Self::JigsawStructures => "worldgen/structures",
            Self::StructureSets => "worldgen/structure_sets",
            Self::TemplatePools => "worldgen/template_pools",
            Self::Processors => "worldgen/processors",
        }
    }

    /// Every family the generator ingests.
    pub const ALL: [Family; 8] = [
        Self::Biomes,
        Self::FeatureRules,
        Self::Features,
        Self::SpawnRules,
        Self::JigsawStructures,
        Self::StructureSets,
        Self::TemplatePools,
        Self::Processors,
    ];
}

/// One resolved document.
#[derive(Debug, Clone)]
pub struct Document {
    /// `description.identifier` of the document, e.g. `minecraft:coal_ore_feature`.
    pub identifier: String,
    /// Top-level schema key, e.g. `minecraft:ore_feature` or `minecraft:biome`.
    pub kind: String,
    /// `format_version` as written (empty when absent).
    pub format_version: String,
    /// Layer and entry the winning copy came from, e.g. `vanilla_1.18.0/features.brarchive:coal_ore_feature.json`.
    pub source: String,
    /// The schema object (`document[kind]`).
    pub body: Value,
}

/// A layer of the stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    /// `definitions` or the pack directory name.
    pub name: String,
    /// Directory the family paths are resolved against.
    pub root: PathBuf,
    /// `true` for the `definitions` layer, whose families live directly below `root`.
    pub is_definitions: bool,
}

/// The ordered layer stack of a BDS install.
#[derive(Debug, Clone)]
pub struct PackStack {
    pub layers: Vec<Layer>,
}

/// Parse `vanilla` / `vanilla_<a.b.c>` pack names into a sortable version.
/// `vanilla` itself is the base layer (version `[]`).
pub fn vanilla_pack_version(name: &str) -> Option<Vec<u32>> {
    if name == "vanilla" {
        return Some(Vec::new());
    }
    let version = name.strip_prefix("vanilla_")?;
    version.split('.').map(|part| part.parse().ok()).collect()
}

impl PackStack {
    /// Build the stack for a BDS server directory (the one holding
    /// `bedrock_server(.exe)`, `definitions/` and `behavior_packs/`).
    pub fn open(server_dir: &Path) -> Result<Self> {
        let definitions = server_dir.join("definitions");
        let packs_dir = server_dir.join("behavior_packs");
        for dir in [&definitions, &packs_dir] {
            if !dir.is_dir() {
                return Err(Error::data(
                    server_dir.display().to_string(),
                    format!("not a BDS server directory: {} is missing", dir.display()),
                ));
            }
        }
        let mut packs = Vec::new();
        for entry in std::fs::read_dir(&packs_dir).map_err(|e| Error::io(&packs_dir, e))? {
            let entry = entry.map_err(|e| Error::io(&packs_dir, e))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(version) = vanilla_pack_version(&name)
                && version.as_slice() <= BASE_GAME_VERSION.as_slice()
                && entry.path().is_dir()
            {
                packs.push((version, name, entry.path()));
            }
        }
        if !packs.iter().any(|(_, name, _)| name == "vanilla") {
            return Err(Error::data(
                packs_dir.display().to_string(),
                "no `vanilla` behaviour pack",
            ));
        }
        packs.sort();
        let mut layers = vec![Layer {
            name: "definitions".to_owned(),
            root: definitions,
            is_definitions: true,
        }];
        layers.extend(packs.into_iter().map(|(_, name, root)| Layer {
            name,
            root,
            is_definitions: false,
        }));
        Ok(Self { layers })
    }

    /// Every file this stack reads for `family`, for `rerun-if-changed`.
    pub fn inputs(&self, family: Family) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for layer in &self.layers {
            for (path, _) in layer_sources(layer, family) {
                out.push(path);
            }
        }
        out
    }

    /// Resolve `family` over the stack: the last layer defining an identifier wins.
    pub fn resolve(&self, family: Family) -> Result<BTreeMap<String, Document>> {
        let mut resolved = BTreeMap::new();
        for layer in &self.layers {
            let mut in_layer: BTreeMap<String, String> = BTreeMap::new();
            for (path, source) in layer_sources(layer, family) {
                for (entry_name, bytes) in source.read(&path)? {
                    let origin = format!("{}/{}:{}", layer.name, family.path(), entry_name);
                    if !entry_name.ends_with(".json") || bytes.is_empty() {
                        continue;
                    }
                    let doc = parse_document(&origin, &bytes)?;
                    if let Some(previous) = in_layer.insert(doc.identifier.clone(), origin.clone()) {
                        return Err(Error::data(
                            origin,
                            format!("{} already defined in the same layer by {previous}", doc.identifier),
                        ));
                    }
                    resolved.insert(doc.identifier.clone(), doc);
                }
            }
        }
        Ok(resolved)
    }
}

/// Where a layer stores a family.
enum SourceKind {
    Archive,
    LooseJson,
}

impl SourceKind {
    fn read(&self, path: &Path) -> Result<Vec<(String, Vec<u8>)>> {
        match self {
            Self::Archive => Ok(brarchive::read_file(path)?
                .into_iter()
                .map(|e| (e.name, e.data))
                .collect()),
            Self::LooseJson => {
                let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
                let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                Ok(vec![(name, bytes)])
            }
        }
    }
}

fn layer_sources(layer: &Layer, family: Family) -> Vec<(PathBuf, SourceKind)> {
    let mut out = Vec::new();
    if !layer.is_definitions {
        let archive_root = layer.root.join("__brarchive");
        let archive = archive_root.join(format!("{}.brarchive", family.path()));
        if archive.is_file() {
            out.push((archive, SourceKind::Archive));
        }
        let mut nested = Vec::new();
        collect_files(&archive_root.join(family.path()), "brarchive", &mut nested);
        out.extend(nested.into_iter().map(|p| (p, SourceKind::Archive)));
    }
    let mut loose = Vec::new();
    collect_files(&layer.root.join(family.path()), "json", &mut loose);
    out.extend(loose.into_iter().map(|p| (p, SourceKind::LooseJson)));
    out
}

/// Recursively collect files with `extension` below `dir`, sorted for determinism.
fn collect_files(dir: &Path, extension: &str, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_files(&path, extension, out);
        } else if path.extension().is_some_and(|ext| ext == extension) {
            out.push(path);
        }
    }
}

/// Split a pack document into identifier, schema key and body.
pub fn parse_document(origin: &str, bytes: &[u8]) -> Result<Document> {
    let value = json::parse(origin, bytes)?;
    let Value::Object(map) = value else {
        return Err(Error::data(origin, "document is not an object"));
    };
    let format_version = match map.get("format_version") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    };
    let mut schema = map.into_iter().filter(|(k, _)| k != "format_version");
    let (Some((kind, body)), None) = (schema.next(), schema.next()) else {
        return Err(Error::data(
            origin,
            "expected exactly one schema key besides format_version",
        ));
    };
    let identifier = body
        .pointer("/description/identifier")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::data(origin, format!("{kind} has no description.identifier")))?
        .to_owned();
    Ok(Document {
        identifier,
        kind,
        format_version,
        source: origin.to_owned(),
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brarchive::{Entry, encode};

    fn feature(id: &str, count: u32) -> Vec<u8> {
        format!(
            "// header comment\n{{\"format_version\":\"1.13.0\",\"minecraft:ore_feature\":{{\"description\":{{\"identifier\":\"{id}\"}},\"count\":{count}}}}}"
        )
        .into_bytes()
    }

    fn write_archive(dir: &Path, family: &str, entries: &[(&str, Vec<u8>)]) {
        let path = dir.join("__brarchive").join(format!("{family}.brarchive"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let entries: Vec<Entry> = entries
            .iter()
            .map(|(n, d)| Entry {
                name: n.to_string(),
                data: d.clone(),
            })
            .collect();
        std::fs::write(path, encode(&entries)).unwrap();
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("unastar_bedrock_codegen_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("definitions/features")).unwrap();
        std::fs::create_dir_all(dir.join("behavior_packs/vanilla")).unwrap();
        dir
    }

    #[test]
    fn pack_versions_sort_numerically_and_vanilla_is_base() {
        let mut names = ["vanilla_1.21.100", "vanilla_1.21.20", "vanilla", "vanilla_1.9"];
        names.sort_by_key(|n| vanilla_pack_version(n).unwrap());
        assert_eq!(names, ["vanilla", "vanilla_1.9", "vanilla_1.21.20", "vanilla_1.21.100"]);
        assert_eq!(vanilla_pack_version("experimental_vanilla_shapes"), None);
        assert_eq!(vanilla_pack_version("vanilla_beta"), None);
    }

    #[test]
    fn later_layers_override_and_newer_than_base_version_is_ignored() {
        let dir = scratch("override");
        std::fs::write(dir.join("definitions/features/coal.json"), feature("minecraft:coal", 1)).unwrap();
        std::fs::write(dir.join("definitions/features/iron.json"), feature("minecraft:iron", 2)).unwrap();
        for (pack, count) in [("vanilla_1.18.0", 10), ("vanilla_1.17.0", 5), ("vanilla_9.0.0", 99)] {
            let pack = dir.join("behavior_packs").join(pack);
            write_archive(&pack, "features", &[("coal.json", feature("minecraft:coal", count))]);
        }
        let stack = PackStack::open(&dir).unwrap();
        let names: Vec<_> = stack.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["definitions", "vanilla", "vanilla_1.17.0", "vanilla_1.18.0"]);
        let docs = stack.resolve(Family::Features).unwrap();
        assert_eq!(docs["minecraft:coal"].body["count"], 10);
        assert_eq!(docs["minecraft:coal"].kind, "minecraft:ore_feature");
        assert!(docs["minecraft:coal"].source.starts_with("vanilla_1.18.0/"));
        assert_eq!(docs["minecraft:iron"].body["count"], 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn duplicate_identifier_in_one_layer_is_an_error() {
        let dir = scratch("dup");
        write_archive(
            &dir.join("behavior_packs/vanilla"),
            "features",
            &[("a.json", feature("minecraft:x", 1)), ("b.json", feature("minecraft:x", 2))],
        );
        let err = PackStack::open(&dir).unwrap().resolve(Family::Features).unwrap_err();
        assert!(err.to_string().contains("same layer"), "{err}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_layout_names_the_missing_directory() {
        let dir = std::env::temp_dir().join(format!("unastar_bedrock_codegen_none_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let err = PackStack::open(&dir).unwrap_err().to_string();
        assert!(err.contains("definitions"), "{err}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
