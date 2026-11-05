//! Vanilla behaviour-pack data (bedrock-samples submodule) through the loaders,
//! and the D9 check against a BDS install when `UNASTAR_BDS_DIR` is set.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use unastar_packs::actor::{self, ActorParser, Loaded};
use unastar_packs::json;
use unastar_packs::packs::PackStack;
use unastar_rules::Diagnostics;
use unastar_rules::filter::{
    BuiltinTest, Fact, Filter, FilterContext, FilterRegistry, FilterTest, Subject,
};
use unastar_rules::molang::{self, QueryContext, QueryRegistry};

fn samples_pack() -> PathBuf {
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../unastar-data/data/vanilla_bp/behavior_pack");
    assert!(
        dir.join("entities").is_dir(),
        "bedrock-samples submodule missing: run `git submodule update --init crates/unastar-data/data/vanilla_bp`"
    );
    dir
}

fn parser() -> &'static ActorParser<'static> {
    static PARSER: std::sync::LazyLock<ActorParser<'static>> = std::sync::LazyLock::new(|| {
        let schema_dir = samples_pack()
            .parent()
            .unwrap()
            .join("metadata/json_schemas/server/entity");
        let files = actor::component_schema_files(&schema_dir).unwrap();
        let mut parser = ActorParser::from_schemas(
            FilterRegistry::vanilla(),
            files.iter().map(|path| {
                json::parse(&path.display().to_string(), &std::fs::read(path).unwrap())
            }),
        )
        .unwrap();
        let version = parser.schema_version();
        let document = schema_dir
            .join(format!("{}.{}.{}", version[0], version[1], version[2]))
            .join("ActorDocument.json");
        parser
            .register_document_schema(
                json::parse("actor document schema", &std::fs::read(document).unwrap()).unwrap(),
            )
            .unwrap();
        let legacy = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../unastar-data/data/upstream/mojang/entity_forms/entity_component_definitions.form.json");
        parser
            .register_form(
                json::parse("legacy component form", &std::fs::read(legacy).unwrap()).unwrap(),
            )
            .unwrap();
        parser
    });
    &PARSER
}

#[test]
fn canonical_entity_member_is_not_rejected_as_an_unknown_extension() {
    let mut document = serde_json::json!({
        "format_version": "1.26.50",
        "minecraft:entity": {"description": {"identifier": "test:actor"}, "upgrades": {}}
    });
    let Loaded::Definition { definition, .. } =
        actor::load_value("document.json", &document, parser()).unwrap()
    else {
        panic!("schema-declared actor skipped")
    };
    assert!(definition.entity_other.contains_key("upgrades"));
    document["minecraft:entity"]["test:unregistered"] = serde_json::json!({});
    assert!(actor::load_value("unknown.json", &document, parser()).is_err());
}

#[test]
fn crafted_actor_acceptance_matches_the_recorded_bds_matrix() {
    let inputs: BTreeMap<String, Value> =
        serde_json::from_str(include_str!("fixtures/bds-1.26.51-fidelity-inputs.json")).unwrap();
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/bds-1.26.51-fidelity.json")).unwrap();
    let observations = reference["observations"].as_object().unwrap();
    assert_eq!(
        inputs.keys().collect::<BTreeSet<_>>(),
        observations.keys().collect::<BTreeSet<_>>()
    );
    for (name, document) in inputs {
        let registered = observations[&name]["summon_succeeded"].as_bool().unwrap();
        match actor::load_value(&name, &document, parser()) {
            Ok(Loaded::Definition { definition, .. }) => {
                assert!(registered, "{name}: BDS rejected this definition");
                let normal = definition.to_json();
                let Loaded::Definition {
                    definition: again, ..
                } = actor::load_value(&name, &normal, parser()).unwrap()
                else {
                    panic!("{name}: normalised definition skipped")
                };
                assert_eq!(again, definition, "{name}: normalisation loses data");
            }
            Ok(Loaded::Skipped { reason }) => panic!("{name}: unexpected skip: {reason}"),
            Err(error) => assert!(!registered, "{name}: BDS accepted this definition: {error}"),
        }
    }
}

fn samples() -> actor::Resolved {
    actor::resolve_stack(
        &PackStack::single_pack("bedrock-samples", samples_pack()),
        parser(),
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
        match actor::load(id, &bytes, parser()).unwrap() {
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

fn molang_strings(
    value: &Value,
    context: QueryContext,
    out: &mut BTreeSet<(String, QueryContext)>,
) {
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
                out.insert((s.clone(), context));
            }
        }
        Value::Object(map) => map.values().for_each(|v| molang_strings(v, context, out)),
        Value::Array(items) => items.iter().for_each(|v| molang_strings(v, context, out)),
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
        for (_, mut doc) in family_documents(family) {
            if family == "entities"
                && let Some(Value::Object(properties)) =
                    doc.pointer_mut("/minecraft:entity/description/properties")
            {
                for property in properties.values_mut() {
                    if let Value::Object(fields) = property {
                        let expression_default = matches!(
                            fields.get("type").and_then(Value::as_str),
                            Some("bool" | "int" | "float")
                        );
                        if let Some(default) = fields.remove("default")
                            && expression_default
                        {
                            molang_strings(&default, QueryContext::PropertyDefault, &mut strings);
                        }
                    }
                }
            }
            molang_strings(&doc, QueryContext::Runtime, &mut strings);
        }
    }
    let mut unbound = BTreeSet::new();
    for (src, context) in &strings {
        let mut expr = molang::parse(src).unwrap_or_else(|e| panic!("{src}: {e}"));
        if let Err(e) = expr.bind(QueryRegistry::vanilla(), *context) {
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
    let bds = actor::resolve_stack(&stack, parser()).unwrap();
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

/// Facts about a freshly summoned fidelity probe for the one context the
/// Bevy-free layers can decide: `self` is the probe (its `type_family`, its
/// active components and its property defaults); every other subject is
/// absent.
struct ProbeContext<'a> {
    definition: &'a actor::ActorDefinition,
    properties: BTreeMap<String, Fact>,
}

impl<'a> ProbeContext<'a> {
    fn new(definition: &'a actor::ActorDefinition) -> Self {
        let mut properties = BTreeMap::new();
        for (name, property) in definition
            .description
            .active_properties()
            .into_iter()
            .flatten()
        {
            let value = match &property.default_expression {
                Some(expr) => {
                    let value = molang::eval::evaluate_num(expr, &mut DefaultContext).unwrap();
                    match property.kind {
                        actor::PropertyKind::Bool => Fact::Bool(value != 0.0),
                        _ => Fact::Number(f64::from(value)),
                    }
                }
                None => match property.default.as_ref() {
                    Some(Value::Bool(b)) => Fact::Bool(*b),
                    Some(Value::Number(n)) => Fact::Number(n.as_f64().unwrap()),
                    other => panic!("{name}: unexpected literal default {other:?}"),
                },
            };
            properties.insert(name.clone(), value);
        }
        Self {
            definition,
            properties,
        }
    }

    fn families(&self) -> Vec<&str> {
        self.definition.components["minecraft:type_family"]["family"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect()
    }
}

impl FilterContext for ProbeContext<'_> {
    fn fact(&self, subject: Subject, test: &FilterTest) -> Option<Fact> {
        if subject != Subject::Self_ {
            return None;
        }
        let value = test.value.as_str();
        match test.builtin()? {
            BuiltinTest::IsFamily => Some(Fact::Matches(self.families().contains(&value?))),
            BuiltinTest::HasComponent => Some(Fact::Matches(
                self.definition.components.contains_key(value?),
            )),
            BuiltinTest::BoolProperty | BuiltinTest::IntProperty => {
                self.properties.get(test.domain_str()?).copied()
            }
            _ => None,
        }
    }
}

/// Property defaults are evaluated with nothing bound beyond the expression
/// itself: unset variables and context values, and component-group queries
/// on a fresh actor, read as 0.
struct DefaultContext;

impl molang::eval::MolangContext for DefaultContext {
    fn query(
        &mut self,
        _: molang::QueryId,
        _: &[molang::eval::Value],
    ) -> Option<molang::eval::Value> {
        None
    }
    fn variable(&self, _: &str) -> Option<molang::eval::Value> {
        None
    }
    fn set_variable(&mut self, _: &str, _: molang::eval::Value) {}
    fn context(&self, _: &str) -> Option<molang::eval::Value> {
        None
    }
    fn this(&self) -> molang::eval::Value {
        molang::eval::Value::Num(0.0)
    }
    fn enter(&mut self, _: u64) -> bool {
        false
    }
    fn leave(&mut self) {}
    fn random(&mut self) -> f32 {
        0.0
    }
}

/// Whether the probe's environment sensor fires `fidelity:pass` and that
/// event runs: the trigger filter passes for a fresh probe and the event
/// exists with no unknown action (an unknown action invalidates the node).
fn sensor_fires(definition: &actor::ActorDefinition) -> bool {
    let Some(sensor) = definition.components.get("minecraft:environment_sensor") else {
        return false;
    };
    let trigger = &sensor["triggers"][0];
    assert_eq!(trigger["event"], "fidelity:pass");
    let filter = Filter::parse(
        &trigger["filters"],
        FilterRegistry::vanilla(),
        "filters",
        &mut Diagnostics::default(),
    )
    .unwrap();
    let Some(event) = definition.events.get("fidelity:pass") else {
        return false;
    };
    let context = ProbeContext::new(definition);
    event.unknown_actions.is_empty() && filter.evaluate(&context)
}

/// Probes whose filter names a subject other than `self`. In the BDS
/// environment-sensor context `baby`, `block`, `holder` and `item` resolve to
/// something for a summoned actor (the negated `is_family` test fails), while
/// `player`, `target`, `parent` and `damager` are absent. Which entity each
/// resolves to is the runtime's business (the server's filter context), so
/// these are left out of the self-only comparison below.
const RUNTIME_SUBJECT_PROBES: [&str; 4] = [
    "subject_numeric_5",
    "subject_numeric_6",
    "subject_numeric_8",
    "subject_numeric_9",
];

/// Probes with a spawn event change their component groups (or run commands)
/// before the sensor ticks; executing events is the server runtime's job
/// (`crates/unastar/src/entity/definition`), so the static check skips them.
const SPAWN_EVENT: &str = "minecraft:entity_spawned";

/// `filter_pass` of every crafted probe (did the environment sensor's filter
/// pass and its event run on the summoned actor), not only whether BDS
/// accepted the definition.
#[test]
fn crafted_actor_sensor_outcomes_match_the_recorded_bds_matrix() {
    let inputs: BTreeMap<String, Value> =
        serde_json::from_str(include_str!("fixtures/bds-1.26.51-fidelity-inputs.json")).unwrap();
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/bds-1.26.51-fidelity.json")).unwrap();
    let observations = reference["observations"].as_object().unwrap();
    let mut compared = 0;
    let mut spawn_event_probes = 0;
    let mut differing = BTreeSet::new();
    for (name, document) in &inputs {
        let observed = observations[name]["filter_pass"].as_bool().unwrap();
        let ours = match actor::load_value(name, document, parser()) {
            Ok(Loaded::Definition { definition, .. }) => {
                if definition.events.contains_key(SPAWN_EVENT) {
                    spawn_event_probes += 1;
                    continue;
                }
                sensor_fires(&definition)
            }
            _ => false,
        };
        if RUNTIME_SUBJECT_PROBES.contains(&name.as_str()) {
            if ours != observed {
                differing.insert(name.as_str());
            }
            continue;
        }
        assert_eq!(ours, observed, "{name}: filter_pass");
        compared += 1;
    }
    assert_eq!(spawn_event_probes, 3);
    assert_eq!(
        compared,
        inputs.len() - RUNTIME_SUBJECT_PROBES.len() - spawn_event_probes
    );
    // Keep the exclusion list honest: every excluded probe must still be one
    // the self-only context gets wrong.
    assert_eq!(
        differing,
        RUNTIME_SUBJECT_PROBES.into_iter().collect::<BTreeSet<_>>()
    );
}
