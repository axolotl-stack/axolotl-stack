//! Vanilla behaviour-pack data (bedrock-samples submodule) through the loaders,
//! and the D9 check against a BDS install when `UNASTAR_BDS_DIR` is set.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use unastar_packs::actor::{self, Loaded};
use unastar_packs::json;
use unastar_packs::packs::PackStack;
use unastar_rules::Diagnostics;
use unastar_rules::filter::{Filter, FilterRegistry};
use unastar_rules::molang::{self, QueryRegistry};

fn samples_pack() -> PathBuf {
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../unastar-data/data/vanilla_bp/behavior_pack");
    assert!(
        dir.join("entities").is_dir(),
        "bedrock-samples submodule missing: run `git submodule update --init crates/unastar-data/data/vanilla_bp`"
    );
    dir
}

fn samples() -> actor::Resolved {
    actor::resolve_stack(
        &PackStack::single_pack("bedrock-samples", samples_pack()),
        FilterRegistry::vanilla(),
    )
    .expect("every vanilla entity file loads")
}

#[test]
fn every_vanilla_entity_loads_and_normalises_to_a_fixed_point() {
    let resolved = samples();
    assert_eq!(resolved.definitions.len(), 128);
    assert!(resolved.skipped.is_empty(), "{:?}", resolved.skipped);
    let mut diagnostics = BTreeMap::new();
    for (id, def) in &resolved.definitions {
        if !def.diagnostics.is_empty() {
            diagnostics.insert(
                id.clone(),
                def.diagnostics
                    .iter()
                    .map(|d| d.path.clone())
                    .collect::<Vec<_>>(),
            );
        }
        let normal = def.definition.to_json();
        let bytes = serde_json::to_vec(&normal).unwrap();
        match actor::load(id, &bytes, FilterRegistry::vanilla()).unwrap() {
            Loaded::Definition {
                definition,
                diagnostics,
            } => {
                assert!(
                    diagnostics.is_empty(),
                    "{id}: normalised form has diagnostics {diagnostics:?}"
                );
                assert_eq!(
                    *definition, def.definition,
                    "{id}: normalised form does not round-trip"
                );
                assert_eq!(definition.to_json(), normal);
            }
            Loaded::Skipped { reason } => panic!("{id}: {reason}"),
        }
    }
    // The only tolerated malformed vanilla data: silverfish `become_angry` /
    // `on_calm` remove a group without the `component_groups` key.
    let expected: BTreeMap<String, Vec<String>> = BTreeMap::from([(
        "minecraft:silverfish".to_owned(),
        vec![
            "/events/minecraft:become_angry/remove/minecraft:silverfish_calm".to_owned(),
            "/events/minecraft:on_calm/remove/minecraft:silverfish_angry".to_owned(),
        ],
    )]);
    assert_eq!(diagnostics, expected);
}

#[test]
fn compact_filters_are_normalised_and_legacy_pushable_upgraded() {
    let resolved = samples();
    let blaze = resolved.definitions["minecraft:blaze"]
        .definition
        .to_json()
        .to_string();
    for compact in ["\"AND\"", "\"OR\"", "\"subject\":1", "\"operator\":0"] {
        assert!(!blaze.contains(compact), "blaze still has {compact}");
    }
    let mut pushable = 0;
    for def in resolved.definitions.values() {
        let d = &def.definition;
        let sets = std::iter::once(&d.components).chain(d.component_groups.values());
        for set in sets {
            assert!(
                !set.contains_key("minecraft:pushable"),
                "{}",
                d.identifier()
            );
        }
        if d.components.contains_key("minecraft:pushable_by_entity")
            || d.components.contains_key("minecraft:pushable_by_block")
        {
            pushable += 1;
        }
    }
    assert!(pushable >= 100, "{pushable}");
    let tnt = &resolved.definitions["minecraft:tnt"].definition.components;
    assert!(
        !tnt.contains_key("minecraft:pushable_by_entity"),
        "tnt has is_pushable: false"
    );
    assert!(tnt.contains_key("minecraft:pushable_by_block"));
}

/// Every non-empty JSON file of a pack family directory, comments stripped.
fn family_documents(family: &str) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    let mut stack = vec![samples_pack().join(family)];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "json") {
                let origin = path.display().to_string();
                let bytes = std::fs::read(&path).unwrap();
                out.push((origin.clone(), json::parse(&origin, &bytes).unwrap()));
            }
        }
    }
    out
}

fn filters_by_shape(value: &Value, out: &mut Vec<Filter>) {
    match value {
        Value::Object(map) if map.get("test").is_some_and(Value::is_string) => {
            out.push(
                Filter::parse(
                    value,
                    FilterRegistry::vanilla(),
                    "",
                    &mut Diagnostics::default(),
                )
                .unwrap(),
            );
        }
        Value::Object(map) => map.values().for_each(|v| filters_by_shape(v, out)),
        Value::Array(items) => items.iter().for_each(|v| filters_by_shape(v, out)),
        _ => {}
    }
}

/// D10 coverage: a filter test used by vanilla data without an implementation
/// fails here (an unknown test name parses with a diagnostic and evaluates false).
#[test]
fn every_vanilla_filter_test_is_implemented() {
    let mut filters = Vec::new();
    for def in samples().definitions.values() {
        filters.extend(def.definition.filters(FilterRegistry::vanilla()));
    }
    for family in ["spawn_rules", "trading", "items", "blocks"] {
        for (_, doc) in family_documents(family) {
            filters_by_shape(&doc, &mut filters);
        }
    }
    let mut used = BTreeSet::new();
    let mut unknown = BTreeSet::new();
    for f in &filters {
        f.for_each_test(&mut |t| {
            used.insert(t.name.clone());
            if t.id.is_none() {
                unknown.insert(t.name.clone());
            }
        });
    }
    assert!(
        unknown.is_empty(),
        "unimplemented filter tests: {unknown:?}"
    );
    assert!(used.len() >= 87, "{} distinct tests", used.len());
}

fn molang_strings(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::String(s) => {
            let lower = s.to_ascii_lowercase();
            if [
                "query.",
                "q.",
                "math.",
                "variable.",
                "v.",
                "context.",
                "c.",
                "temp.",
                "t.",
            ]
            .iter()
            .any(|ns| {
                lower
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
                    .any(|w| w.starts_with(ns) && w.len() > ns.len())
            }) {
                out.insert(s.clone());
            }
        }
        Value::Object(map) => map.values().for_each(|v| molang_strings(v, out)),
        Value::Array(items) => items.iter().for_each(|v| molang_strings(v, out)),
        _ => {}
    }
}

/// D10 coverage: every Molang string in vanilla behaviour-pack data parses
/// and every query it uses has a runtime implementation.
#[test]
fn every_vanilla_molang_string_parses_and_binds() {
    let mut strings = BTreeSet::new();
    for family in [
        "entities",
        "items",
        "blocks",
        "biomes",
        "spawn_rules",
        "trading",
        "recipes",
        "loot_tables",
    ] {
        for (_, doc) in family_documents(family) {
            molang_strings(&doc, &mut strings);
        }
    }
    assert!(strings.len() >= 40, "{} Molang strings", strings.len());
    let mut unbound = BTreeSet::new();
    for src in &strings {
        let mut expr = molang::parse(src).unwrap_or_else(|e| panic!("{src}: {e}"));
        if let Err(e) = expr.bind(QueryRegistry::vanilla()) {
            unbound.insert(e);
        }
    }
    assert!(unbound.is_empty(), "{unbound:?}");
}

/// D9: bedrock-samples entities equal the BDS install's layered packs.
/// Skips (with a message) when `UNASTAR_BDS_DIR` is not set.
#[test]
fn bedrock_samples_match_the_bds_layer_stack() {
    let Some(server) = std::env::var_os("UNASTAR_BDS_DIR") else {
        eprintln!("skipping: set UNASTAR_BDS_DIR to a BDS 1.26.51 server directory");
        return;
    };
    let stack = PackStack::open(Path::new(&server)).unwrap();
    let bds = actor::resolve_stack(&stack, FilterRegistry::vanilla()).unwrap();
    let diff = actor::diff(&samples(), &bds);
    // `minecraft:agent` (Education Edition) ships only in the BDS packs.
    assert_eq!(diff.only_right, ["minecraft:agent"], "{diff:?}");
    assert!(
        diff.only_left.is_empty() && diff.differing.is_empty(),
        "{diff:?}"
    );
    let skipped: Vec<&str> = bds
        .skipped
        .iter()
        .map(|(origin, _)| origin.as_str())
        .collect();
    assert_eq!(
        skipped,
        [
            "vanilla_1.16.100/entities:npc.json",
            "vanilla_1.17.20/entities:npc.json"
        ]
    );
}
