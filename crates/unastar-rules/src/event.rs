//! Actor event trees: parse every accepted spelling, write one canonical
//! form, and evaluate a tree into the ordered effects the entity layer applies.
//!
//! # Node shape
//!
//! A node may carry `filters`, leaf actions and at most one child list
//! (`sequence`, `randomize` or `first_valid`). Under `randomize` each child
//! has a `weight`.
//!
//! # Accepted spellings (parser fidelity)
//!
//! | Concern | Accepted | Canonical |
//! |---|---|---|
//! | `add` / `remove` | `{"component_groups": [..]}`; `{}`; any other key is ignored with a diagnostic (vanilla silverfish `become_angry` / `on_calm` write `"remove": {"minecraft:silverfish_calm": {}}`) | `{"component_groups": [..]}` |
//! | `trigger` | an event name, or `{"event", "target"?, "filters"?}`; target defaults to `self` | `{"event", "target"}` (+ `filters`) |
//! | `filters` | every filter spelling ([`crate::filter`]) | canonical filter |
//! | `set_property` | property → literal or Molang string (resolved against the property's type when the definition is decoded) | unchanged |
//! | Other actions | `queue_command`, `emit_vibration`, `play_sound`, `emit_particle`, `set_home_position`, `execute_event_on_home_block`, `reset_target`, `stop_movement`, `drop_item`, `run_command` | unchanged JSON |
//! | Unknown keys | inactive with a diagnostic; retained in `unknown_actions` | unchanged JSON |
//!
//! `randomize` children without `weight` are rejected (vanilla always writes
//! one; the BDS default is unconfirmed).
//!
//! # Evaluation
//!
//! A node whose filters fail does nothing. A passing node runs its own
//! actions (remove groups, then add groups, then properties and other actions
//! in key order), then its children: `sequence` runs every passing child,
//! `randomize` picks one passing child by weight, and `first_valid` runs the
//! first passing child. Triggers are collected for execution after the tree.
//! [`evaluate_tree`] also drives startup-bound actor trees, so their traversal
//! and the parsed representation cannot acquire different event semantics.

use crate::filter::{Filter, FilterContext, FilterRegistry, Subject};
use crate::{Diagnostics, ParseError};
use serde_json::{Map, Number, Value};

/// A parsed event node.
#[derive(Debug, Clone, PartialEq)]
pub struct EventNode {
    pub filters: Option<Filter>,
    /// Weight under a `randomize` parent.
    pub weight: Option<Number>,
    pub add: Option<Vec<String>>,
    pub remove: Option<Vec<String>>,
    pub trigger: Option<Trigger>,
    /// `set_property` entries in canonical key order, values unresolved.
    pub set_property: Option<Vec<(String, Value)>>,
    /// Other actions by key, in key order.
    pub actions: Vec<(EventAction, Value)>,
    /// Unknown actions retain their JSON, but invalidate the node, including
    /// its known sibling actions (BDS 1.26.51 crafted-pack probe).
    pub unknown_actions: Map<String, Value>,
    pub children: Option<Children>,
}

/// Child list of a node.
#[derive(Debug, Clone, PartialEq)]
pub enum Children {
    Sequence(Vec<EventNode>),
    Randomize(Vec<EventNode>),
    FirstValid(Vec<EventNode>),
}

/// `trigger`.
#[derive(Debug, Clone, PartialEq)]
pub struct Trigger {
    pub event: String,
    pub target: Subject,
    pub filters: Option<Filter>,
}

/// Actions other than group changes, triggers and properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventAction {
    QueueCommand,
    RunCommand,
    EmitVibration,
    PlaySound,
    EmitParticle,
    SetHomePosition,
    ExecuteEventOnHomeBlock,
    ResetTarget,
    StopMovement,
    DropItem,
}

impl EventAction {
    pub const ALL: [EventAction; 10] = [
        Self::QueueCommand,
        Self::RunCommand,
        Self::EmitVibration,
        Self::PlaySound,
        Self::EmitParticle,
        Self::SetHomePosition,
        Self::ExecuteEventOnHomeBlock,
        Self::ResetTarget,
        Self::StopMovement,
        Self::DropItem,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::QueueCommand => "queue_command",
            Self::RunCommand => "run_command",
            Self::EmitVibration => "emit_vibration",
            Self::PlaySound => "play_sound",
            Self::EmitParticle => "emit_particle",
            Self::SetHomePosition => "set_home_position",
            Self::ExecuteEventOnHomeBlock => "execute_event_on_home_block",
            Self::ResetTarget => "reset_target",
            Self::StopMovement => "stop_movement",
            Self::DropItem => "drop_item",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.key() == key)
    }
}

impl EventNode {
    /// Parse a node (an event body).
    pub fn parse(
        value: &Value,
        registry: &FilterRegistry,
        path: &str,
        diags: &mut Diagnostics,
    ) -> Result<Self, ParseError> {
        let Value::Object(map) = value else {
            return Err(ParseError::new(
                path,
                format!("event must be an object, got {value}"),
            ));
        };
        let mut node = EventNode {
            filters: None,
            weight: None,
            add: None,
            remove: None,
            trigger: None,
            set_property: None,
            actions: Vec::new(),
            unknown_actions: Map::new(),
            children: None,
        };
        for (key, member) in map {
            let at = format!("{path}/{key}");
            match key.as_str() {
                "filters" => node.filters = Some(Filter::parse(member, registry, &at, diags)?),
                "weight" => match member {
                    Value::Number(n) => node.weight = Some(n.clone()),
                    other => {
                        return Err(ParseError::new(
                            at,
                            format!("weight must be a number, got {other}"),
                        ));
                    }
                },
                "add" => node.add = Some(parse_groups(member, &at, diags)?),
                "remove" => node.remove = Some(parse_groups(member, &at, diags)?),
                "trigger" => node.trigger = Some(Trigger::parse(member, registry, &at, diags)?),
                "set_property" => match member {
                    Value::Object(props) => {
                        node.set_property =
                            Some(props.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    }
                    other => {
                        return Err(ParseError::new(
                            at,
                            format!("set_property must be an object, got {other}"),
                        ));
                    }
                },
                "sequence" | "randomize" | "first_valid" => {
                    if node.children.is_some() {
                        return Err(ParseError::new(
                            at,
                            "a node has at most one of sequence/randomize/first_valid",
                        ));
                    }
                    let Value::Array(items) = member else {
                        return Err(ParseError::new(at, format!("{key} must be an array")));
                    };
                    let children = items
                        .iter()
                        .enumerate()
                        .map(|(i, item)| {
                            EventNode::parse(item, registry, &format!("{at}/{i}"), diags)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    node.children = Some(match key.as_str() {
                        "sequence" => Children::Sequence(children),
                        "randomize" => {
                            if let Some(i) = children.iter().position(|c| c.weight.is_none()) {
                                return Err(ParseError::new(
                                    format!("{at}/{i}"),
                                    "randomize entry without weight",
                                ));
                            }
                            Children::Randomize(children)
                        }
                        _ => Children::FirstValid(children),
                    });
                }
                other => match EventAction::from_key(other) {
                    Some(action) => node.actions.push((action, member.clone())),
                    None => {
                        diags.push(at, "unknown event key ignored");
                        node.unknown_actions
                            .insert(other.to_owned(), member.clone());
                    }
                },
            }
        }
        Ok(node)
    }

    /// The canonical JSON form.
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        if let Some(filters) = &self.filters {
            map.insert("filters".into(), filters.to_json());
        }
        if let Some(weight) = &self.weight {
            map.insert("weight".into(), Value::Number(weight.clone()));
        }
        let groups = |names: &[String]| {
            let mut m = Map::new();
            m.insert(
                "component_groups".into(),
                Value::Array(names.iter().cloned().map(Value::String).collect()),
            );
            Value::Object(m)
        };
        if let Some(add) = &self.add {
            map.insert("add".into(), groups(add));
        }
        if let Some(remove) = &self.remove {
            map.insert("remove".into(), groups(remove));
        }
        if let Some(trigger) = &self.trigger {
            map.insert("trigger".into(), trigger.to_json());
        }
        if let Some(props) = &self.set_property {
            map.insert(
                "set_property".into(),
                Value::Object(props.iter().cloned().collect()),
            );
        }
        for (action, value) in &self.actions {
            map.insert(action.key().into(), value.clone());
        }
        map.extend(self.unknown_actions.clone());
        if let Some(children) = &self.children {
            let (key, items) = match children {
                Children::Sequence(items) => ("sequence", items),
                Children::Randomize(items) => ("randomize", items),
                Children::FirstValid(items) => ("first_valid", items),
            };
            map.insert(
                key.into(),
                Value::Array(items.iter().map(EventNode::to_json).collect()),
            );
        }
        Value::Object(map)
    }

    /// Visit this node and its descendants.
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a EventNode)) {
        f(self);
        if let Some(
            Children::Sequence(items) | Children::Randomize(items) | Children::FirstValid(items),
        ) = &self.children
        {
            items.iter().for_each(|c| c.walk(f));
        }
    }

    /// Evaluate the tree. `random` returns a uniform number in `[0, 1)`.
    pub fn evaluate<'a>(
        &'a self,
        ctx: &dyn FilterContext,
        random: &mut dyn FnMut() -> f32,
    ) -> Outcome<'a> {
        let mut out = Outcome::default();
        let result = evaluate_tree(
            self,
            ctx,
            random,
            &mut |node| {
                node.emit(&mut out);
                Ok(())
            },
            &mut Vec::new(),
        );
        match result {
            Ok(()) => {}
            Err(never) => match never {},
        }
        out
    }

    fn emit<'a>(&'a self, out: &mut Outcome<'a>) {
        if let Some(remove) = &self.remove {
            out.effects
                .extend(remove.iter().map(|g| Effect::RemoveGroup(g)));
        }
        if let Some(add) = &self.add {
            out.effects.extend(add.iter().map(|g| Effect::AddGroup(g)));
        }
        if let Some(props) = &self.set_property {
            out.effects
                .extend(props.iter().map(|(k, v)| Effect::SetProperty(k, v)));
        }
        out.effects
            .extend(self.actions.iter().map(|(a, v)| Effect::Action(*a, v)));
        if let Some(trigger) = &self.trigger {
            out.triggers.push(trigger);
        }
    }
}

/// Child selection shared by parsed and startup-bound event trees.
pub enum TreeChildren<'a, T> {
    Sequence(&'a [T]),
    Randomize(&'a [T]),
    FirstValid(&'a [T]),
}

/// The traversal only needs a passing predicate, weight and children. Leaf
/// payloads remain in their original representation and are emitted by the
/// caller; bound trees need not retain JSON, identifiers or parsed filters.
pub trait EventTree<C: ?Sized>: Sized {
    type Error;

    fn passes(&self, context: &C) -> Result<bool, Self::Error>;
    fn weight(&self) -> f64;
    fn children(&self) -> Option<TreeChildren<'_, Self>>;
}

impl<'ctx> EventTree<dyn FilterContext + 'ctx> for EventNode {
    type Error = std::convert::Infallible;

    fn passes(&self, context: &(dyn FilterContext + 'ctx)) -> Result<bool, Self::Error> {
        Ok(self.unknown_actions.is_empty()
            && self
                .filters
                .as_ref()
                .is_none_or(|filter| filter.evaluate(context)))
    }

    fn weight(&self) -> f64 {
        self.weight
            .as_ref()
            .and_then(Number::as_f64)
            .unwrap_or(0.0)
            .max(0.0)
    }

    fn children(&self) -> Option<TreeChildren<'_, Self>> {
        self.children.as_ref().map(|children| match children {
            Children::Sequence(items) => TreeChildren::Sequence(items),
            Children::Randomize(items) => TreeChildren::Randomize(items),
            Children::FirstValid(items) => TreeChildren::FirstValid(items),
        })
    }
}

/// Run one tree, emitting passing nodes before their descendants. Filters
/// must be pure. `random` supplies a uniform value in `[0, 1)`. Scratch stores
/// passing random-child indices and keeps its original length, even on error;
/// its capacity can be reused across actors. Triggers belong in the sink's
/// deferred queue, not recursively dispatched by the sink.
pub fn evaluate_tree<'a, C: ?Sized, T: EventTree<C>>(
    node: &'a T,
    context: &C,
    random: &mut dyn FnMut() -> f32,
    emit: &mut impl FnMut(&'a T) -> Result<(), T::Error>,
    scratch: &mut Vec<usize>,
) -> Result<(), T::Error> {
    if !node.passes(context)? {
        return Ok(());
    }
    emit(node)?;
    match node.children() {
        None => {}
        Some(TreeChildren::Sequence(items)) => {
            for child in items {
                evaluate_tree(child, context, random, emit, scratch)?;
            }
        }
        Some(TreeChildren::FirstValid(items)) => {
            for child in items {
                if child.passes(context)? {
                    evaluate_tree(child, context, random, emit, scratch)?;
                    break;
                }
            }
        }
        Some(TreeChildren::Randomize(items)) => {
            let start = scratch.len();
            let mut total = 0.0;
            for (index, child) in items.iter().enumerate() {
                let passes = match child.passes(context) {
                    Ok(passes) => passes,
                    Err(error) => {
                        scratch.truncate(start);
                        return Err(error);
                    }
                };
                if passes {
                    total += child.weight();
                    scratch.push(index);
                }
            }
            let mut chosen = None;
            if total > 0.0 {
                let mut roll = f64::from(random()) * total;
                for &index in &scratch[start..] {
                    chosen = Some(index);
                    roll -= items[index].weight();
                    if roll < 0.0 {
                        break;
                    }
                }
            }
            scratch.truncate(start);
            if let Some(index) = chosen {
                evaluate_tree(&items[index], context, random, emit, scratch)?;
            }
        }
    }
    Ok(())
}

fn parse_groups(
    value: &Value,
    path: &str,
    diags: &mut Diagnostics,
) -> Result<Vec<String>, ParseError> {
    let Value::Object(map) = value else {
        return Err(ParseError::new(
            path,
            format!("expected {{\"component_groups\": [...]}}, got {value}"),
        ));
    };
    let mut groups = Vec::new();
    for (key, member) in map {
        if key != "component_groups" {
            diags.push(
                format!("{path}/{key}"),
                "unknown key ignored (no group is added or removed)",
            );
            continue;
        }
        let Value::Array(items) = member else {
            return Err(ParseError::new(
                format!("{path}/{key}"),
                "component_groups must be an array",
            ));
        };
        for (i, item) in items.iter().enumerate() {
            match item {
                Value::String(s) => groups.push(s.clone()),
                other => {
                    return Err(ParseError::new(
                        format!("{path}/{key}/{i}"),
                        format!("group name must be a string, got {other}"),
                    ));
                }
            }
        }
    }
    Ok(groups)
}

impl Trigger {
    fn parse(
        value: &Value,
        registry: &FilterRegistry,
        path: &str,
        diags: &mut Diagnostics,
    ) -> Result<Self, ParseError> {
        match value {
            Value::String(event) => Ok(Self {
                event: event.clone(),
                target: Subject::Self_,
                filters: None,
            }),
            Value::Object(map) => {
                let event = map
                    .get("event")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ParseError::new(path, "trigger needs an event name"))?
                    .to_owned();
                let target = match map.get("target") {
                    None => Subject::Self_,
                    Some(Value::String(s)) => Subject::from_name(s).ok_or_else(|| {
                        ParseError::new(format!("{path}/target"), format!("unknown target {s:?}"))
                    })?,
                    Some(other) => {
                        return Err(ParseError::new(
                            format!("{path}/target"),
                            format!("target must be a string, got {other}"),
                        ));
                    }
                };
                let filters = match map.get("filters") {
                    Some(f) => Some(Filter::parse(
                        f,
                        registry,
                        &format!("{path}/filters"),
                        diags,
                    )?),
                    None => None,
                };
                for key in map.keys() {
                    if !matches!(key.as_str(), "event" | "target" | "filters") {
                        diags.push(format!("{path}/{key}"), "unknown trigger key ignored");
                    }
                }
                Ok(Self {
                    event,
                    target,
                    filters,
                })
            }
            other => Err(ParseError::new(
                path,
                format!("trigger must be a string or object, got {other}"),
            )),
        }
    }

    fn to_json(&self) -> Value {
        let mut map = Map::new();
        map.insert("event".into(), Value::String(self.event.clone()));
        map.insert("target".into(), Value::String(self.target.name().into()));
        if let Some(filters) = &self.filters {
            map.insert("filters".into(), filters.to_json());
        }
        Value::Object(map)
    }
}

/// One effect of an evaluated event, in execution order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Effect<'a> {
    RemoveGroup(&'a str),
    AddGroup(&'a str),
    /// Property name and its unresolved value (literal or Molang).
    SetProperty(&'a str, &'a Value),
    Action(EventAction, &'a Value),
}

/// Result of evaluating an event tree.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outcome<'a> {
    pub effects: Vec<Effect<'a>>,
    /// Triggers to run after `effects` are applied.
    pub triggers: Vec<&'a Trigger>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{BuiltinTest, Fact, FilterTest};
    use serde_json::json;

    fn parse(v: Value) -> (EventNode, Vec<crate::Diagnostic>) {
        let mut diags = Diagnostics::default();
        let node = EventNode::parse(&v, FilterRegistry::vanilla(), "e", &mut diags).unwrap();
        (node, diags.items)
    }

    #[test]
    fn silverfish_remove_without_component_groups_is_tolerated() {
        let (node, diags) = parse(
            json!({"remove": {"minecraft:silverfish_calm": {}}, "add": {"component_groups": ["minecraft:silverfish_angry"]}}),
        );
        assert_eq!(node.remove.as_deref(), Some(&[][..]));
        assert_eq!(
            node.add.as_deref(),
            Some(&["minecraft:silverfish_angry".to_owned()][..])
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].path, "e/remove/minecraft:silverfish_calm");
    }

    #[test]
    fn trigger_spellings_normalise() {
        let (a, _) = parse(json!({"trigger": "minecraft:grow"}));
        let (b, _) = parse(json!({"trigger": {"event": "minecraft:grow", "target": "self"}}));
        assert_eq!(a.to_json(), b.to_json());
        assert_eq!(
            a.to_json()["trigger"],
            json!({"event": "minecraft:grow", "target": "self"})
        );
        let (round, _) = parse(a.to_json());
        assert_eq!(round, a);
    }

    #[test]
    fn rejects_structural_errors() {
        let mut diags = Diagnostics::default();
        for bad in [
            json!({"randomize": [{"add": {"component_groups": ["a"]}}]}),
            json!({"sequence": [], "first_valid": []}),
            json!({"add": {"component_groups": "a"}}),
            json!({"trigger": {"target": "self"}}),
        ] {
            assert!(
                EventNode::parse(&bad, FilterRegistry::vanilla(), "e", &mut diags).is_err(),
                "{bad}"
            );
        }
    }

    struct Families(&'static [&'static str]);
    impl FilterContext for Families {
        fn fact(&self, _: Subject, test: &FilterTest) -> Option<Fact> {
            match test.builtin()? {
                BuiltinTest::IsFamily => {
                    Some(Fact::Matches(self.0.contains(&test.value.as_str()?)))
                }
                _ => None,
            }
        }
    }

    #[test]
    fn evaluation_order_and_node_kinds() {
        let (node, _) = parse(json!({
            "remove": {"component_groups": ["r"]},
            "add": {"component_groups": ["a"]},
            "trigger": "t",
            "sequence": [
                {"filters": {"test": "is_family", "value": "zombie"}, "add": {"component_groups": ["z"]}},
                {"filters": {"test": "is_family", "value": "cow"}, "add": {"component_groups": ["c"]}},
                {"first_valid": [
                    {"filters": {"test": "is_family", "value": "cow"}, "add": {"component_groups": ["fv_cow"]}},
                    {"add": {"component_groups": ["fv_any"]}, "play_sound": {"sound": "x"}},
                    {"add": {"component_groups": ["fv_never"]}}
                ]}
            ]
        }));
        let out = node.evaluate(&Families(&["zombie"]), &mut || 0.0);
        let sound = json!({"sound": "x"});
        assert_eq!(
            out.effects,
            vec![
                Effect::RemoveGroup("r"),
                Effect::AddGroup("a"),
                Effect::AddGroup("z"),
                Effect::AddGroup("fv_any"),
                Effect::Action(EventAction::PlaySound, &sound),
            ]
        );
        assert_eq!(out.triggers.len(), 1);
    }

    #[test]
    fn randomize_picks_by_weight_among_passing_children() {
        let (node, _) = parse(json!({"randomize": [
            {"weight": 95, "add": {"component_groups": ["adult"]}},
            {"weight": 5, "add": {"component_groups": ["baby"]}},
            {"weight": 1000, "filters": {"test": "is_family", "value": "cow"}, "add": {"component_groups": ["never"]}}
        ]}));
        let pick = |roll: f32| node.evaluate(&Families(&[]), &mut || roll).effects;
        assert_eq!(pick(0.0), vec![Effect::AddGroup("adult")]);
        assert_eq!(pick(0.94), vec![Effect::AddGroup("adult")]);
        assert_eq!(pick(0.96), vec![Effect::AddGroup("baby")]);
        assert_eq!(pick(0.999_999), vec![Effect::AddGroup("baby")]);
    }

    #[test]
    fn failing_filters_skip_the_node() {
        let (node, _) = parse(
            json!({"filters": {"test": "is_family", "value": "cow"}, "add": {"component_groups": ["a"]}, "trigger": "t"}),
        );
        let out = node.evaluate(&Families(&["zombie"]), &mut || 0.0);
        assert!(out.effects.is_empty() && out.triggers.is_empty());
    }
}

#[cfg(test)]
mod preservation_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unknown_actions_survive_normalisation_without_executing() {
        let input = json!({
            "addon:action": {"nested": [1, {"x": 2}]},
            "trigger": "known",
            "set_property": {"addon:p": true},
            "queue_command": {"command": "scoreboard players set probe fidelity 1"}
        });
        let mut diagnostics = Diagnostics::default();
        let event =
            EventNode::parse(&input, FilterRegistry::vanilla(), "event", &mut diagnostics).unwrap();
        assert!(
            !diagnostics.items.is_empty(),
            "unknown action must be diagnosed"
        );
        assert_eq!(event.to_json()["addon:action"], input["addon:action"]);
        struct EmptyContext;
        impl FilterContext for EmptyContext {
            fn fact(
                &self,
                _: Subject,
                _: &crate::filter::FilterTest,
            ) -> Option<crate::filter::Fact> {
                None
            }
        }
        let result = event.evaluate(&EmptyContext, &mut || panic!("leaf uses no randomness"));
        assert!(result.effects.is_empty());
        assert!(result.triggers.is_empty());
    }
}
