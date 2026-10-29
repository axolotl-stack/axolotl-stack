//! World-generation rows for both editions.
//!
//! **Java 1.21.11** rows come from the committed data-generator tree
//! `crates/unastar_noise/worldgen_data` plus the biome tags in
//! `crates/unastar_noise/java_data/tags/worldgen/biome`. A dimension's biomes
//! are the `is_overworld` / `is_nether` / `is_end` tags; its features are the
//! placed features its biomes list, closed over placed features referenced from
//! configured features; structures are those whose biome set meets the
//! dimension; carvers come from the biomes; surface rules are the block-placing
//! leaves of the dimension's `noise_settings` rule tree, numbered in rule order
//! (the same order as the `RULE_LEAVES` coverage labels). Features placed only
//! by structure pools are `java/pool/feature` rows.
//!
//! **Bedrock 1.26.51** worldgen data ships only inside BDS (Minecraft EULA), so
//! it is read through `unastar_bedrock_codegen::model::Model` from the install
//! named by `UNASTAR_BDS_DIR`. The resulting row list (identifiers only) is
//! committed as [`BEDROCK_SNAPSHOT`] so the catalog can be checked without an
//! install; with the variable set, the snapshot is regenerated (`generate`) or
//! compared (`check`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context as _, bail};
use serde_json::Value;
use unastar_bedrock_codegen::ir::{Biome, Pass};
use unastar_bedrock_codegen::model::Model;

use crate::catalog::Row;

const JAVA_WORLDGEN: &str = "crates/unastar_noise/worldgen_data";
const JAVA_BIOME_TAGS: &str = "crates/unastar_noise/java_data/tags/worldgen/biome";
const NATIVE_DIR: &str = "crates/valentine_gen/protocolgen/data/source/inputs/1.26.51/native";
/// Committed Bedrock row list, `family<TAB>id<TAB>label` per line.
pub const BEDROCK_SNAPSHOT: &str = "docs/parity/source/bedrock-worldgen.tsv";
pub const BDS_DIR_VAR: &str = "UNASTAR_BDS_DIR";

const DIMENSIONS: [(&str, &str); 3] = [
    ("overworld", "is_overworld"),
    ("nether", "is_nether"),
    ("end", "is_end"),
];

pub fn java_rows(root: &Path) -> anyhow::Result<Vec<Row>> {
    let data = JavaData::load(root)?;
    let mut rows = Vec::new();
    let mut dimension_features = BTreeSet::new();
    for (dimension, tag) in DIMENSIONS {
        let biomes = data.tag(tag)?;
        let family = |kind: &str| format!("java/{dimension}/{kind}");
        let row = |kind: &str, name: &str| {
            Row::new(
                format!("java/{dimension}/{kind}/{}", short(name)),
                family(kind),
            )
        };

        let mut features = BTreeSet::new();
        let mut carvers = BTreeSet::new();
        for biome in &biomes {
            rows.push(row("biome", biome));
            let json = data.registry_entry("biome", biome)?;
            for step in json["features"].as_array().into_iter().flatten() {
                features.extend(
                    step.as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_owned),
                );
            }
            match &json["carvers"] {
                Value::String(carver) => {
                    carvers.insert(carver.clone());
                }
                Value::Array(list) => {
                    carvers.extend(list.iter().filter_map(Value::as_str).map(str::to_owned))
                }
                _ => {}
            }
        }
        let features = data.placed_closure(features)?;
        for feature in &features {
            rows.push(row("feature", feature));
        }
        dimension_features.extend(features);
        for carver in &carvers {
            rows.push(row("carver", carver));
        }
        for structure in data.names("structure")? {
            let json = data.registry_entry("structure", &structure)?;
            let structure_biomes = data.biome_holders(&json["biomes"])?;
            if structure_biomes.iter().any(|b| biomes.contains(b)) {
                rows.push(row("structure", &structure));
            }
        }
        let settings = data.registry_entry("noise_settings", dimension)?;
        let mut leaves = Vec::new();
        surface_leaves(&settings["surface_rule"], &mut leaves)?;
        for (index, result) in leaves.iter().enumerate() {
            rows.push(Row::new(
                format!("java/{dimension}/surface_rule/{index}-{}", short(result)),
                family("surface_rule"),
            ));
        }
    }

    let pool_features = data.placed_closure(data.pool_features()?)?;
    for feature in pool_features.difference(&dimension_features) {
        rows.push(Row::new(
            format!("java/pool/feature/{}", short(feature)),
            "java/pool/feature",
        ));
    }
    Ok(rows)
}

fn short(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

/// Block-placing leaves of a Java surface rule tree, in rule order.
fn surface_leaves(rule: &Value, leaves: &mut Vec<String>) -> anyhow::Result<()> {
    match rule["type"].as_str() {
        Some("minecraft:sequence") => {
            for child in rule["sequence"].as_array().into_iter().flatten() {
                surface_leaves(child, leaves)?;
            }
        }
        Some("minecraft:condition") => surface_leaves(&rule["then_run"], leaves)?,
        Some("minecraft:block") => {
            let name = rule["result_state"]["Name"]
                .as_str()
                .context("surface rule block without result_state.Name")?;
            leaves.push(name.to_owned());
        }
        Some("minecraft:bandlands") => leaves.push("bandlands".to_owned()),
        other => bail!("unknown surface rule type {other:?}"),
    }
    Ok(())
}

struct JavaData<'a> {
    worldgen: std::path::PathBuf,
    tags: std::path::PathBuf,
    _root: &'a Path,
}

impl<'a> JavaData<'a> {
    fn load(root: &'a Path) -> anyhow::Result<Self> {
        let data = JavaData {
            worldgen: root.join(JAVA_WORLDGEN),
            tags: root.join(JAVA_BIOME_TAGS),
            _root: root,
        };
        anyhow::ensure!(
            data.worldgen.is_dir(),
            "missing {}",
            data.worldgen.display()
        );
        Ok(data)
    }

    fn read(path: &Path) -> anyhow::Result<Value> {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_slice(&bytes).with_context(|| format!("decoding {}", path.display()))
    }

    fn registry_entry(&self, registry: &str, name: &str) -> anyhow::Result<Value> {
        Self::read(
            &self
                .worldgen
                .join(registry)
                .join(format!("{}.json", short(name))),
        )
    }

    fn exists(&self, registry: &str, name: &str) -> bool {
        name.starts_with("minecraft:")
            && self
                .worldgen
                .join(registry)
                .join(format!("{}.json", short(name)))
                .is_file()
    }

    /// Identifiers of every top-level entry of a registry, sorted.
    fn names(&self, registry: &str) -> anyhow::Result<Vec<String>> {
        let dir = self.worldgen.join(registry);
        let mut names = Vec::new();
        for entry in
            std::fs::read_dir(&dir).with_context(|| format!("listing {}", dir.display()))?
        {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "json") {
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .context("non-UTF-8 file name")?;
                names.push(format!("minecraft:{stem}"));
            }
        }
        names.sort();
        Ok(names)
    }

    /// Biomes in a biome tag, nested tags resolved.
    fn tag(&self, tag: &str) -> anyhow::Result<BTreeSet<String>> {
        let json = Self::read(&self.tags.join(format!("{tag}.json")))?;
        let mut biomes = BTreeSet::new();
        for value in json["values"].as_array().into_iter().flatten() {
            let value = match value {
                Value::String(s) => s.as_str(),
                Value::Object(o) => o
                    .get("id")
                    .and_then(Value::as_str)
                    .context("tag entry without id")?,
                _ => bail!("biome tag {tag}: unexpected entry {value}"),
            };
            match value.strip_prefix('#') {
                Some(nested) => biomes.extend(self.tag(short(nested))?),
                None => {
                    biomes.insert(value.to_owned());
                }
            }
        }
        Ok(biomes)
    }

    /// A structure's `biomes` holder set: a tag, one biome or a list.
    fn biome_holders(&self, holders: &Value) -> anyhow::Result<BTreeSet<String>> {
        Ok(match holders {
            Value::String(s) => match s.strip_prefix('#') {
                Some(tag) => self.tag(short(tag))?,
                None => BTreeSet::from([s.clone()]),
            },
            Value::Array(list) => list
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            other => bail!("unexpected biome holder set {other}"),
        })
    }

    /// Placed features reachable from `start`: through the configured feature
    /// of each placed feature and any placed or configured feature identifier
    /// named inside a configuration.
    fn placed_closure(&self, start: BTreeSet<String>) -> anyhow::Result<BTreeSet<String>> {
        let mut placed = BTreeSet::new();
        let mut configured_seen = BTreeSet::new();
        let mut stack: Vec<(bool, String)> = start.into_iter().map(|p| (true, p)).collect();
        while let Some((is_placed, name)) = stack.pop() {
            if is_placed {
                if !placed.insert(name.clone()) {
                    continue;
                }
                let json = self.registry_entry("placed_feature", &name)?;
                self.collect_refs(&json["feature"], &mut stack);
            } else {
                if !configured_seen.insert(name.clone()) {
                    continue;
                }
                let json = self.registry_entry("configured_feature", &name)?;
                self.collect_refs(&json, &mut stack);
            }
        }
        Ok(placed)
    }

    fn collect_refs(&self, value: &Value, out: &mut Vec<(bool, String)>) {
        match value {
            Value::String(s) => {
                if self.exists("placed_feature", s) {
                    out.push((true, s.clone()));
                } else if self.exists("configured_feature", s) {
                    out.push((false, s.clone()));
                }
            }
            Value::Array(list) => list.iter().for_each(|v| self.collect_refs(v, out)),
            Value::Object(map) => map.values().for_each(|v| self.collect_refs(v, out)),
            _ => {}
        }
    }

    /// Placed features named by `feature_pool_element`s in template pools.
    fn pool_features(&self) -> anyhow::Result<BTreeSet<String>> {
        let mut features = BTreeSet::new();
        let mut dirs = vec![self.worldgen.join("template_pool")];
        while let Some(dir) = dirs.pop() {
            for entry in
                std::fs::read_dir(&dir).with_context(|| format!("listing {}", dir.display()))?
            {
                let path = entry?.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|e| e == "json") {
                    find_pool_features(&Self::read(&path)?, &mut features);
                }
            }
        }
        Ok(features)
    }
}

fn find_pool_features(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if map.get("element_type").and_then(Value::as_str)
                == Some("minecraft:feature_pool_element")
                && let Some(feature) = map.get("feature").and_then(Value::as_str)
            {
                out.insert(feature.to_owned());
            }
            map.values().for_each(|v| find_pool_features(v, out));
        }
        Value::Array(list) => list.iter().for_each(|v| find_pool_features(v, out)),
        _ => {}
    }
}

/// Bedrock rows from a BDS install.
pub fn bedrock_rows_from_install(root: &Path, bds_dir: &Path) -> anyhow::Result<Vec<Row>> {
    let model = Model::load(bds_dir, &root.join(NATIVE_DIR))
        .map_err(|e| anyhow::anyhow!("loading BDS worldgen from {}: {e}", bds_dir.display()))?;
    let dimension_of = |biome: &Biome| -> &'static str {
        if biome.tags.contains("nether") {
            "nether"
        } else if biome.tags.contains("the_end") {
            "end"
        } else if unastar_bedrock_codegen::model::OVERWORLD_TAGS
            .iter()
            .any(|t| biome.tags.contains(*t))
        {
            "overworld"
        } else {
            "unmatched"
        }
    };
    let mut rows = Vec::new();
    let mut dimension_biomes: BTreeMap<&str, Vec<&Biome>> = BTreeMap::new();
    for biome in &model.biomes {
        let dimension = dimension_of(biome);
        dimension_biomes.entry(dimension).or_default().push(biome);
        rows.push(Row::new(
            format!("bedrock/{dimension}/biome/{}", short(&biome.identifier)),
            format!("bedrock/{dimension}/biome"),
        ));
        if let Some(surface) = &biome.surface {
            rows.push(
                Row::new(
                    format!("bedrock/{dimension}/surface/{}", short(&biome.identifier)),
                    format!("bedrock/{dimension}/surface"),
                )
                .with_label(format!("{} surface builder", builder_kind(surface))),
            );
        }
    }

    // A rule belongs to the first dimension with a biome it can run in.
    for rule in model.rules.values().filter(|r| !r.is_disabled()) {
        let dimension = ["overworld", "nether", "end"]
            .into_iter()
            .find(|dimension| {
                dimension_biomes.get(dimension).is_some_and(|biomes| {
                    biomes.iter().any(|b| {
                        rule.biome_filter
                            .as_ref()
                            .is_none_or(|f| f.matches(&b.tags))
                    })
                })
            });
        // Our biome-filter evaluation (shared with the generator) finds no
        // biome this rule can run in; kept visible rather than dropped.
        let dimension = dimension.unwrap_or("unmatched");
        let kind = if rule.pass == Pass::Pregeneration && rule.places_feature.contains("carver") {
            "carver"
        } else {
            "feature_rule"
        };
        rows.push(
            Row::new(
                format!("bedrock/{dimension}/{kind}/{}", short(&rule.identifier)),
                format!("bedrock/{dimension}/{kind}"),
            )
            .with_label(format!("places {}", rule.places_feature)),
        );
    }

    let reachable = model
        .reachable_features(unastar_bedrock_codegen::builtin::BUILTIN_FEATURES)
        .map_err(|e| anyhow::anyhow!("Bedrock feature reachability: {e}"))?;
    let all_features: BTreeSet<&String> = model.features.keys().chain(reachable.iter()).collect();
    for feature in all_features {
        let kind = if model.features.contains_key(feature) {
            "feature"
        } else {
            "builtin_feature"
        };
        rows.push(Row::new(
            format!("bedrock/{kind}/{}", short(feature)),
            format!("bedrock/{kind}"),
        ));
    }
    for structure in &model.structures.structures {
        rows.push(Row::new(
            format!("bedrock/structure/{}", short(&structure.identifier)),
            "bedrock/structure",
        ));
    }
    for set in &model.structures.sets {
        rows.push(Row::new(
            format!("bedrock/structure_set/{}", short(&set.identifier)),
            "bedrock/structure_set",
        ));
    }
    Ok(rows)
}

/// The surface builder's kind, from its variant name.
fn builder_kind(surface: &unastar_bedrock_codegen::ir::SurfaceBuilder) -> String {
    let debug = format!("{surface:?}");
    debug
        .split(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// Bedrock rows: from the install when `UNASTAR_BDS_DIR` is set, otherwise
/// from the committed snapshot. Returns the rows and whether they came from
/// an install.
pub fn bedrock_rows(root: &Path) -> anyhow::Result<(Vec<Row>, bool)> {
    match std::env::var_os(BDS_DIR_VAR) {
        Some(dir) => Ok((bedrock_rows_from_install(root, Path::new(&dir))?, true)),
        None => Ok((read_snapshot(root)?, false)),
    }
}

pub fn render_snapshot(rows: &[Row]) -> String {
    let mut sorted: Vec<&Row> = rows.iter().collect();
    sorted.sort_by(|a, b| a.family.cmp(&b.family).then_with(|| a.id.cmp(&b.id)));
    let mut out = String::from(
        "# Bedrock 1.26.51 worldgen rows (family, id, label), generated by `parity generate`\n\
         # from a BDS install (UNASTAR_BDS_DIR). Identifiers only; do not edit by hand.\n",
    );
    for row in sorted {
        out.push_str(&format!(
            "{}\t{}\t{}\n",
            row.family,
            row.id,
            row.label.as_deref().unwrap_or("")
        ));
    }
    out
}

fn read_snapshot(root: &Path) -> anyhow::Result<Vec<Row>> {
    let path = root.join(BEDROCK_SNAPSHOT);
    let text = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "reading {} (run `parity generate` with {BDS_DIR_VAR} set to create it)",
            path.display()
        )
    })?;
    let mut rows = Vec::new();
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let mut fields = line.split('\t');
        let (Some(family), Some(id), label) = (fields.next(), fields.next(), fields.next()) else {
            bail!("{BEDROCK_SNAPSHOT}: malformed line `{line}`");
        };
        let mut row = Row::new(id, family);
        if let Some(label) = label.filter(|l| !l.is_empty()) {
            row = row.with_label(label);
        }
        rows.push(row);
    }
    Ok(rows)
}
