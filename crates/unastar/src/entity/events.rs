//! Startup-bound actor events. Only lookup/compilation accepts names or JSON;
//! evaluation emits borrowed, typed operands using the shared rules traversal.
//!
//! The caller owns filter/action bindings and the corresponding runtime context.
//! In particular, binding a filter must bind its entire parsed tree (including
//! subjects, operators, domains and values), or return an error. Non-self
//! subjects are not silently redirected to self. Other actions are emitted for
//! real caller implementations, never acknowledged as successful no-ops.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;
use unastar_data::actor_definitions::ActorDefinition;
use unastar_rules::event::{
    Children, EventAction, EventNode, EventTree, TreeChildren, evaluate_tree,
};
use unastar_rules::filter::{Filter, Subject};
use unastar_rules::molang::QueryRegistry;

use super::definition::{CompiledDefinition, ComponentGroupId};
use super::properties::{PropertyAssignment, PropertyLayout};

/// Numeric operands in the caller's frozen binding tables, not JSON payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FilterBinding(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActionBinding(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExternalEventBinding(pub u32);

/// Startup integration with caller-owned implementations. No method has a
/// permissive default: an unsupported filter, subject, action or external event
/// must return an error. External event binding must account for the explicit
/// target subject; it must not resolve the name in this actor's event table.
pub trait EventBindings {
    fn filter(&mut self, filter: &Filter) -> Result<FilterBinding, String>;
    fn action(&mut self, action: EventAction, value: &Value) -> Result<ActionBinding, String>;
    fn external_event(
        &mut self,
        target: Subject,
        event: &str,
    ) -> Result<ExternalEventBinding, String>;
}

/// Runtime counterpart of the same frozen binding tables. Filters are pure
/// and can be queried more than once by first_valid/randomize, just as in the
/// parsed evaluator. Missing subjects follow the shared filter semantics;
/// unsupported implementations are errors, not invented true/false answers.
pub trait EventContext {
    fn filter(&self, binding: FilterBinding) -> Result<bool, String>;
}

#[derive(Debug, thiserror::Error)]
#[error("actor event {path}: {reason}")]
pub struct EventCompileError {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EventEvaluationError {
    #[error("event handle belongs to another compiled event table")]
    ForeignHandle,
    #[error("event filter binding {binding:?}: {reason}")]
    Filter {
        binding: FilterBinding,
        reason: String,
    },
}

#[derive(Debug)]
struct EventScope;

/// A numeric event index scoped to exactly one frozen table. Even two tables
/// compiled from the same source do not accept each other's handles.
#[derive(Debug, Clone)]
pub struct EventHandle {
    owner: Arc<EventScope>,
    index: usize,
}

impl PartialEq for EventHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.owner, &other.owner) && self.index == other.index
    }
}
impl Eq for EventHandle {}

#[derive(Debug, Clone)]
pub enum TriggerEvent {
    Internal(EventHandle),
    External(ExternalEventBinding),
}

/// Collected without evaluating its filter, exactly like the parsed rules
/// outcome. The caller supplies the context when explicitly checking `passes`;
/// the native timing of that check relative to ECS apply remains unmeasured.
/// Tree evaluation never recursively dispatches a trigger.
#[derive(Debug, Clone)]
pub struct DeferredTrigger {
    pub target: Subject,
    pub event: TriggerEvent,
    pub filter: Option<FilterBinding>,
}

impl DeferredTrigger {
    pub fn passes<C: EventContext + ?Sized>(
        &self,
        context: &C,
    ) -> Result<bool, EventEvaluationError> {
        passes(self.filter, context)
    }
}

/// Ordered effects ready for the parent's ECS apply integration. The borrowed
/// operands are already scoped to their definition/property layouts; there is
/// no runtime group/property/event identifier lookup or JSON interpretation.
#[derive(Clone, Copy)]
pub enum EventEffect<'a> {
    RemoveGroup(&'a ComponentGroupId),
    AddGroup(&'a ComponentGroupId),
    SetProperty(&'a PropertyAssignment),
    Action {
        kind: EventAction,
        binding: ActionBinding,
    },
}

#[derive(Default)]
pub struct EventOutcome<'a> {
    pub effects: Vec<EventEffect<'a>>,
    pub triggers: Vec<&'a DeferredTrigger>,
}

impl EventOutcome<'_> {
    pub fn clear(&mut self) {
        self.effects.clear();
        self.triggers.clear();
    }
}

struct BoundNode {
    enabled: bool,
    filter: Option<FilterBinding>,
    weight: f64,
    effects: Box<[EventEffectOwned]>,
    trigger: Option<DeferredTrigger>,
    children: Option<BoundChildren>,
}

enum EventEffectOwned {
    RemoveGroup(ComponentGroupId),
    AddGroup(ComponentGroupId),
    SetProperty(PropertyAssignment),
    Action {
        kind: EventAction,
        binding: ActionBinding,
    },
}

enum BoundChildren {
    Sequence(Box<[BoundNode]>),
    Randomize(Box<[BoundNode]>),
    FirstValid(Box<[BoundNode]>),
}

fn passes<C: EventContext + ?Sized>(
    filter: Option<FilterBinding>,
    context: &C,
) -> Result<bool, EventEvaluationError> {
    match filter {
        None => Ok(true),
        Some(binding) => context
            .filter(binding)
            .map_err(|reason| EventEvaluationError::Filter { binding, reason }),
    }
}

impl<C: EventContext + ?Sized> EventTree<C> for BoundNode {
    type Error = EventEvaluationError;

    fn passes(&self, context: &C) -> Result<bool, Self::Error> {
        if !self.enabled {
            return Ok(false);
        }
        passes(self.filter, context)
    }

    fn weight(&self) -> f64 {
        self.weight
    }

    fn children(&self) -> Option<TreeChildren<'_, Self>> {
        self.children.as_ref().map(|children| match children {
            BoundChildren::Sequence(items) => TreeChildren::Sequence(items),
            BoundChildren::Randomize(items) => TreeChildren::Randomize(items),
            BoundChildren::FirstValid(items) => TreeChildren::FirstValid(items),
        })
    }
}

pub struct CompiledEvents {
    owner: Arc<EventScope>,
    names: BTreeMap<Arc<str>, usize>,
    events: Box<[BoundNode]>,
}

impl CompiledEvents {
    pub fn compile(
        source: &ActorDefinition,
        definition: &CompiledDefinition,
        properties: &Arc<PropertyLayout>,
        queries: &QueryRegistry,
        bindings: &mut impl EventBindings,
    ) -> Result<Self, EventCompileError> {
        if source.identifier() != definition.identifier() {
            return Err(EventCompileError {
                path: source.identifier().to_owned(),
                reason: "component definition belongs to another actor identifier".into(),
            });
        }
        let owner = Arc::new(EventScope);
        let names = source
            .events
            .keys()
            .enumerate()
            .map(|(index, name)| (Arc::from(name.as_str()), index))
            .collect::<BTreeMap<_, _>>();
        let mut compiler = Compiler {
            owner: &owner,
            names: &names,
            definition,
            properties,
            queries,
            bindings,
        };
        let events = source
            .events
            .iter()
            .map(|(name, node)| {
                compiler.node(node, &format!("{}/events/{name}", source.identifier()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            owner,
            names,
            events: events.into_boxed_slice(),
        })
    }

    /// Resolve once during startup (or command ingress), never in event actions.
    pub fn event(&self, name: &str) -> Option<EventHandle> {
        Some(EventHandle {
            owner: Arc::clone(&self.owner),
            index: *self.names.get(name)?,
        })
    }

    /// Append one event's effects and deferred triggers, reusing both output
    /// and scratch capacities. Foreign handles fail before touching the output,
    /// scratch, context or RNG. A runtime filter error rolls back this call's
    /// appended output; RNG already used by earlier nodes is not rewound.
    pub fn evaluate<'a, C: EventContext + ?Sized>(
        &'a self,
        event: &EventHandle,
        context: &C,
        random: &mut dyn FnMut() -> f32,
        output: &mut EventOutcome<'a>,
        scratch: &mut Vec<usize>,
    ) -> Result<(), EventEvaluationError> {
        if !Arc::ptr_eq(&self.owner, &event.owner) {
            return Err(EventEvaluationError::ForeignHandle);
        }
        let effects_start = output.effects.len();
        let triggers_start = output.triggers.len();
        let result = evaluate_tree(
            &self.events[event.index],
            context,
            random,
            &mut |node| {
                output
                    .effects
                    .extend(node.effects.iter().map(|effect| match effect {
                        EventEffectOwned::RemoveGroup(group) => EventEffect::RemoveGroup(group),
                        EventEffectOwned::AddGroup(group) => EventEffect::AddGroup(group),
                        EventEffectOwned::SetProperty(assignment) => {
                            EventEffect::SetProperty(assignment)
                        }
                        EventEffectOwned::Action { kind, binding } => EventEffect::Action {
                            kind: *kind,
                            binding: *binding,
                        },
                    }));
                if let Some(trigger) = &node.trigger {
                    output.triggers.push(trigger);
                }
                Ok(())
            },
            scratch,
        );
        if result.is_err() {
            output.effects.truncate(effects_start);
            output.triggers.truncate(triggers_start);
        }
        result
    }
}

struct Compiler<'a, B> {
    owner: &'a Arc<EventScope>,
    names: &'a BTreeMap<Arc<str>, usize>,
    definition: &'a CompiledDefinition,
    properties: &'a Arc<PropertyLayout>,
    queries: &'a QueryRegistry,
    bindings: &'a mut B,
}

impl<B: EventBindings> Compiler<'_, B> {
    fn node(&mut self, source: &EventNode, path: &str) -> Result<BoundNode, EventCompileError> {
        let error = |reason| EventCompileError {
            path: path.to_owned(),
            reason,
        };
        let weight = source
            .weight
            .as_ref()
            .and_then(serde_json::Number::as_f64)
            .unwrap_or(0.0)
            .max(0.0);
        // Native-tested invalidation suppresses the entire subtree, including
        // known sibling actions. Do not require bindings for unreachable JSON.
        if !source.unknown_actions.is_empty() {
            return Ok(BoundNode {
                enabled: false,
                filter: None,
                weight,
                effects: Box::new([]),
                trigger: None,
                children: None,
            });
        }
        let filter = source
            .filters
            .as_ref()
            .map(|filter| self.bindings.filter(filter).map_err(&error))
            .transpose()?;
        let mut effects = Vec::new();
        for name in source.remove.iter().flatten() {
            let group = self.definition.group(name).ok_or_else(|| {
                error(format!(
                    "remove references undeclared component group {name}"
                ))
            })?;
            effects.push(EventEffectOwned::RemoveGroup(group));
        }
        for name in source.add.iter().flatten() {
            let group = self.definition.group(name).ok_or_else(|| {
                error(format!("add references undeclared component group {name}"))
            })?;
            effects.push(EventEffectOwned::AddGroup(group));
        }
        for (name, value) in source.set_property.iter().flatten() {
            let assignment = self
                .properties
                .compile_assignment(name, value, self.queries)
                .map_err(|cause| error(cause.to_string()))?;
            effects.push(EventEffectOwned::SetProperty(assignment));
        }
        for (kind, value) in &source.actions {
            let binding = self
                .bindings
                .action(*kind, value)
                .map_err(|cause| error(format!("{}: {cause}", kind.key())))?;
            effects.push(EventEffectOwned::Action {
                kind: *kind,
                binding,
            });
        }
        let trigger = source
            .trigger
            .as_ref()
            .map(|trigger| {
                let event = if trigger.target == Subject::Self_ {
                    let index = *self.names.get(trigger.event.as_str()).ok_or_else(|| {
                        error(format!(
                            "trigger references undeclared self event {}",
                            trigger.event
                        ))
                    })?;
                    TriggerEvent::Internal(EventHandle {
                        owner: Arc::clone(self.owner),
                        index,
                    })
                } else {
                    TriggerEvent::External(
                        self.bindings
                            .external_event(trigger.target, &trigger.event)
                            .map_err(&error)?,
                    )
                };
                let filter = trigger
                    .filters
                    .as_ref()
                    .map(|filter| self.bindings.filter(filter).map_err(&error))
                    .transpose()?;
                Ok::<_, EventCompileError>(DeferredTrigger {
                    target: trigger.target,
                    event,
                    filter,
                })
            })
            .transpose()?;
        let children = source
            .children
            .as_ref()
            .map(|children| {
                let (kind, items) = match children {
                    Children::Sequence(items) => ("sequence", items),
                    Children::Randomize(items) => ("randomize", items),
                    Children::FirstValid(items) => ("first_valid", items),
                };
                let nodes = items
                    .iter()
                    .enumerate()
                    .map(|(index, node)| self.node(node, &format!("{path}/{kind}/{index}")))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice();
                Ok::<_, EventCompileError>(match children {
                    Children::Sequence(_) => BoundChildren::Sequence(nodes),
                    Children::Randomize(_) => BoundChildren::Randomize(nodes),
                    Children::FirstValid(_) => BoundChildren::FirstValid(nodes),
                })
            })
            .transpose()?;
        Ok(BoundNode {
            enabled: true,
            filter,
            weight,
            effects: effects.into_boxed_slice(),
            trigger,
            children,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::prelude::{Component, Entity, World};
    use serde_json::json;
    use unastar_packs::actor::{Loaded, load_value};
    use unastar_rules::filter::{BuiltinTest, Operator};
    use unastar_rules::molang::QueryId;
    use unastar_rules::molang::eval::{MolangContext, Value as MolangValue};

    use crate::entity::definition::{
        ComponentKindsBuilder, GroupChange, apply_pending_definitions, attach_definition,
        queue_group_change,
    };
    use crate::entity::properties::{ActorProperties, PropertyValue};

    // This test binder implements only the explicitly supported leaf operands.
    // It retains no Filter/JSON at runtime and rejects everything else.
    struct Bindings;

    impl EventBindings for Bindings {
        fn filter(&mut self, filter: &Filter) -> Result<FilterBinding, String> {
            let Filter::Test(test) = filter else {
                return Err("control does not implement composite filters".into());
            };
            if test.operator != Operator::Eq || test.domain.is_some() {
                return Err("control does not implement this operator/domain".into());
            }
            match (test.builtin(), test.subject) {
                (Some(BuiltinTest::HasBiomeTag), Subject::Self_) => match test.value.as_str() {
                    Some("spawns_warm_variant_farm_animals") => Ok(FilterBinding(0)),
                    Some("spawns_cold_variant_farm_animals") => Ok(FilterBinding(1)),
                    _ => Err("unbound biome tag".into()),
                },
                (Some(BuiltinTest::IsBaby), subject)
                    if test.value == unastar_rules::filter::FilterValue::Bool(true) =>
                {
                    match subject {
                        Subject::Self_ => Ok(FilterBinding(2)),
                        Subject::Other => Ok(FilterBinding(3)),
                        _ => Err("unbound baby subject".into()),
                    }
                }
                _ => Err("unsupported control filter".into()),
            }
        }

        fn action(&mut self, kind: EventAction, value: &Value) -> Result<ActionBinding, String> {
            if kind == EventAction::PlaySound && value == &json!({"sound": "test:sound"}) {
                Ok(ActionBinding(7))
            } else {
                Err("no control action implementation".into())
            }
        }

        fn external_event(
            &mut self,
            target: Subject,
            event: &str,
        ) -> Result<ExternalEventBinding, String> {
            if target == Subject::Other && event == "test:other" {
                Ok(ExternalEventBinding(9))
            } else {
                Err("no control external event implementation".into())
            }
        }
    }

    #[derive(Default)]
    struct Context {
        warm: bool,
        cold: bool,
        baby: bool,
        other_baby: Option<bool>,
    }

    impl EventContext for Context {
        fn filter(&self, binding: FilterBinding) -> Result<bool, String> {
            match binding.0 {
                0 => Ok(self.warm),
                1 => Ok(self.cold),
                2 => Ok(self.baby),
                // An absent subject fails the test; it is not evaluated on self.
                3 => Ok(self.other_baby.unwrap_or(false)),
                _ => Err("foreign control filter binding".into()),
            }
        }
    }

    struct LiteralOnly;
    impl MolangContext for LiteralOnly {
        fn query(&mut self, _: QueryId, _: &[MolangValue]) -> Option<MolangValue> {
            panic!("literal property queried context")
        }
        fn variable(&self, _: &str) -> Option<MolangValue> {
            panic!("literal property read variable")
        }
        fn set_variable(&mut self, _: &str, _: MolangValue) {
            panic!("literal property wrote variable")
        }
        fn context(&self, _: &str) -> Option<MolangValue> {
            panic!("literal property read context")
        }
        fn this(&self) -> MolangValue {
            panic!("literal property read this")
        }
        fn enter(&mut self, _: u64) -> bool {
            panic!("literal property switched actor")
        }
        fn leave(&mut self) {
            panic!("literal property left actor")
        }
        fn random(&mut self) -> f32 {
            panic!("literal property requested randomness")
        }
    }

    #[derive(Component)]
    struct Baby;
    #[derive(Component)]
    struct Scale(f64);

    fn compile(
        source: &ActorDefinition,
    ) -> (Arc<CompiledDefinition>, Arc<PropertyLayout>, CompiledEvents) {
        let mut kinds = ComponentKindsBuilder::default();
        kinds
            .register(
                |_: unastar_data::actor_components::body::IsBaby| Ok(()),
                |_, entity| {
                    entity.insert(Baby);
                },
                |entity| {
                    entity.remove::<Baby>();
                },
            )
            .unwrap();
        kinds
            .register(
                |body: unastar_data::actor_components::body::Scale| Ok(f64::from(body.value)),
                |value, entity| {
                    entity.insert(Scale(*value));
                },
                |entity| {
                    entity.remove::<Scale>();
                },
            )
            .unwrap();
        let definition = CompiledDefinition::compile(source, kinds.freeze()).unwrap();
        let properties = Arc::new(PropertyLayout::compile(&source.description).unwrap());
        let events = CompiledEvents::compile(
            source,
            &definition,
            &properties,
            QueryRegistry::vanilla(),
            &mut Bindings,
        )
        .unwrap();
        (definition, properties, events)
    }

    fn vanilla_projection(identifier: &str) -> ActorDefinition {
        let mut source = (**unastar_data::actor_definitions::get(identifier).unwrap()).clone();
        // Observe actual vanilla event trees through two real typed component
        // implementations, not pretend implementations of every mob component.
        let supported = |name: &String, _: &mut Value| {
            matches!(name.as_str(), "minecraft:is_baby" | "minecraft:scale")
        };
        source.components.retain(supported);
        for components in source.component_groups.values_mut() {
            components.retain(supported);
        }
        source
    }

    fn control(events: Value) -> ActorDefinition {
        let document = json!({
            "format_version": "1.26.50",
            "minecraft:entity": {
                "description": {
                    "identifier": "test:events", "is_summonable": true,
                    "properties": {"test:number": {
                        "type": "int", "range": [0, 20], "default": 0, "client_sync": true
                    }}
                },
                "components": {"minecraft:scale": {"value": 1}},
                "component_groups": {
                    "small": {"minecraft:scale": {"value": 0.5}, "minecraft:is_baby": {}},
                    "large": {"minecraft:scale": {"value": 2}}
                },
                "events": events
            }
        });
        let Loaded::Definition { definition, .. } = load_value(
            "bound event control",
            &document,
            unastar_data::actor_definitions::parser(),
        )
        .unwrap() else {
            panic!("bound event control skipped")
        };
        *definition
    }

    fn apply(
        outcome: &EventOutcome<'_>,
        world: &mut World,
        actor: Entity,
        properties: &mut ActorProperties,
    ) {
        for effect in &outcome.effects {
            match effect {
                EventEffect::RemoveGroup(group) => {
                    queue_group_change(world, actor, GroupChange::Remove((*group).clone()))
                        .unwrap();
                }
                EventEffect::AddGroup(group) => {
                    queue_group_change(world, actor, GroupChange::Add((*group).clone())).unwrap();
                }
                EventEffect::SetProperty(assignment) => {
                    properties.apply(assignment, &mut LiteralOnly).unwrap();
                }
                EventEffect::Action { .. } => {
                    panic!("control did not apply an action implementation")
                }
            }
        }
        apply_pending_definitions(world);
    }

    fn evaluate<'a>(
        events: &'a CompiledEvents,
        name: &str,
        context: &Context,
        roll: f32,
    ) -> EventOutcome<'a> {
        let mut outcome = EventOutcome::default();
        events
            .evaluate(
                &events.event(name).unwrap(),
                context,
                &mut || roll,
                &mut outcome,
                &mut Vec::new(),
            )
            .unwrap();
        outcome
    }

    #[test]
    fn actual_cow_and_chicken_baby_spawn_growth_and_sound_reach_ecs_properties() {
        for (identifier, special_sound) in
            [("minecraft:cow", "moody"), ("minecraft:chicken", "picky")]
        {
            let source = vanilla_projection(identifier);
            let (definition, layout, events) = compile(&source);
            let climate = layout.slot("minecraft:climate_variant").unwrap();
            let sound = layout.slot("minecraft:sound_variant").unwrap();
            let mut properties =
                ActorProperties::new(Arc::clone(&layout), &mut LiteralOnly).unwrap();
            let mut world = World::new();
            let actor = world.spawn_empty().id();
            attach_definition(&mut world, actor, definition).unwrap();
            apply_pending_definitions(&mut world);

            // Both tags deliberately pass: actual first_valid must choose warm.
            let spawned = evaluate(
                &events,
                "minecraft:entity_spawned",
                &Context {
                    warm: true,
                    cold: true,
                    ..Context::default()
                },
                0.96,
            );
            assert!(matches!(
                spawned.effects.as_slice(),
                [EventEffect::AddGroup(_), EventEffect::SetProperty(_),]
            ));
            assert!(spawned.triggers.is_empty());
            apply(&spawned, &mut world, actor, &mut properties);
            assert!(world.get::<Baby>(actor).is_some());
            assert_eq!(world.get::<Scale>(actor).unwrap().0, 0.5);
            assert_eq!(
                layout.enum_value(&climate, properties.value(&climate).unwrap()),
                Some("warm")
            );

            let grown = evaluate(
                &events,
                "minecraft:ageable_grow_up",
                &Context::default(),
                0.0,
            );
            assert!(matches!(
                grown.effects.as_slice(),
                [EventEffect::RemoveGroup(_), EventEffect::AddGroup(_),]
            ));
            apply(&grown, &mut world, actor, &mut properties);
            assert!(world.get::<Baby>(actor).is_none());
            assert!(world.get::<Scale>(actor).is_none());
            assert_eq!(
                layout.enum_value(&climate, properties.value(&climate).unwrap()),
                Some("warm")
            );

            let born = evaluate(&events, "minecraft:entity_born", &Context::default(), 0.0);
            apply(&born, &mut world, actor, &mut properties);
            assert!(world.get::<Baby>(actor).is_some());
            assert_eq!(born.triggers.len(), 1);
            assert_eq!(born.triggers[0].target, Subject::Self_);
            assert!(born.triggers[0].passes(&Context::default()).unwrap());
            let TriggerEvent::Internal(event) = &born.triggers[0].event else {
                panic!("vanilla self trigger compiled as external")
            };
            assert_eq!(
                event,
                &events.event("minecraft:randomize_sound_variant").unwrap()
            );
            assert_eq!(
                layout.enum_value(&sound, properties.value(&sound).unwrap()),
                Some("default")
            );
            let mut sound_outcome = EventOutcome::default();
            events
                .evaluate(
                    event,
                    &Context::default(),
                    &mut || 0.75,
                    &mut sound_outcome,
                    &mut Vec::new(),
                )
                .unwrap();
            apply(&sound_outcome, &mut world, actor, &mut properties);
            assert_eq!(
                layout.enum_value(&sound, properties.value(&sound).unwrap()),
                Some(special_sound)
            );
        }
    }

    #[test]
    fn actual_adult_spawn_defers_both_trigger_generations_and_applies_cold_property_first() {
        for identifier in ["minecraft:cow", "minecraft:chicken"] {
            let (definition, layout, events) = compile(&vanilla_projection(identifier));
            let climate = layout.slot("minecraft:climate_variant").unwrap();
            let mut properties =
                ActorProperties::new(Arc::clone(&layout), &mut LiteralOnly).unwrap();
            let mut world = World::new();
            let actor = world.spawn_empty().id();
            attach_definition(&mut world, actor, definition).unwrap();
            let spawned = evaluate(
                &events,
                "minecraft:entity_spawned",
                &Context {
                    cold: true,
                    ..Context::default()
                },
                0.0,
            );
            assert!(matches!(
                spawned.effects.as_slice(),
                [EventEffect::SetProperty(_)]
            ));
            assert_eq!(spawned.triggers.len(), 1);
            apply(&spawned, &mut world, actor, &mut properties);
            assert_eq!(
                layout.enum_value(&climate, properties.value(&climate).unwrap()),
                Some("cold")
            );
            assert!(world.get::<Baby>(actor).is_none());
            let TriggerEvent::Internal(adult) = &spawned.triggers[0].event else {
                panic!("not internal")
            };
            let mut grown = EventOutcome::default();
            events
                .evaluate(
                    adult,
                    &Context::default(),
                    &mut || panic!("adult leaf uses no RNG"),
                    &mut grown,
                    &mut Vec::new(),
                )
                .unwrap();
            assert!(matches!(
                grown.effects.as_slice(),
                [EventEffect::AddGroup(_)]
            ));
            assert_eq!(grown.triggers.len(), 1);
            let TriggerEvent::Internal(sound) = &grown.triggers[0].event else {
                panic!("not internal")
            };
            assert_eq!(
                sound,
                &events.event("minecraft:randomize_sound_variant").unwrap()
            );
        }
    }

    #[test]
    fn ordered_group_property_and_action_effects_preserve_shadow_and_destructive_removal() {
        let source = control(json!({
            "shadow": {
                "remove": {"component_groups": ["large"]},
                "add": {"component_groups": ["small"]},
                "set_property": {"test:number": 4},
                "play_sound": {"sound": "test:sound"},
                "trigger": "after",
                "sequence": [{
                    "add": {"component_groups": ["large"]},
                    "set_property": {"test:number": 9}
                }]
            },
            "remove": {"remove": {"component_groups": ["large"]}},
            "after": {"set_property": {"test:number": 12}}
        }));
        let (definition, layout, events) = compile(&source);
        let slot = layout.slot("test:number").unwrap();
        let mut properties = ActorProperties::new(layout, &mut LiteralOnly).unwrap();
        let mut world = World::new();
        let actor = world.spawn_empty().id();
        attach_definition(&mut world, actor, definition).unwrap();
        apply_pending_definitions(&mut world);
        assert_eq!(world.get::<Scale>(actor).unwrap().0, 1.0);
        let outcome = evaluate(&events, "shadow", &Context::default(), 0.0);
        assert!(matches!(
            outcome.effects.as_slice(),
            [
                EventEffect::RemoveGroup(_),
                EventEffect::AddGroup(_),
                EventEffect::SetProperty(_),
                EventEffect::Action {
                    kind: EventAction::PlaySound,
                    binding: ActionBinding(7)
                },
                EventEffect::AddGroup(_),
                EventEffect::SetProperty(_),
            ]
        ));
        let mut played_sound_at = None;
        for effect in &outcome.effects {
            match effect {
                EventEffect::Action {
                    kind: EventAction::PlaySound,
                    binding: ActionBinding(7),
                } => {
                    played_sound_at = Some(properties.value(&slot).unwrap());
                }
                EventEffect::Action { .. } => panic!("unbound action"),
                _ => {
                    let one = EventOutcome {
                        effects: vec![*effect],
                        triggers: Vec::new(),
                    };
                    apply(&one, &mut world, actor, &mut properties);
                }
            }
        }
        assert_eq!(played_sound_at, Some(PropertyValue::Int(4)));
        assert_eq!(properties.value(&slot).unwrap(), PropertyValue::Int(9));
        assert_eq!(world.get::<Scale>(actor).unwrap().0, 2.0);
        assert!(world.get::<Baby>(actor).is_some());
        assert_eq!(outcome.triggers.len(), 1);
        // Removing the winning source destroys that component; it does not
        // revive the small group's scale or the actor's base scale.
        apply(
            &evaluate(&events, "remove", &Context::default(), 0.0),
            &mut world,
            actor,
            &mut properties,
        );
        assert!(world.get::<Scale>(actor).is_none());
        assert!(world.get::<Baby>(actor).is_some());
        assert_eq!(properties.value(&slot).unwrap(), PropertyValue::Int(9));
    }

    #[test]
    fn foreign_handles_fail_before_context_rng_output_or_scratch_mutation() {
        let source = control(json!({
            "seed": {"set_property": {"test:number": 3}, "trigger": "random"},
            "random": {"randomize": [
                {"weight": 1, "set_property": {"test:number": 1}},
                {"weight": 1, "set_property": {"test:number": 2}}
            ]}
        }));
        let (_, _, first) = compile(&source);
        let (_, _, second) = compile(&source);
        let foreign = second.event("random").unwrap();
        let mut outcome = evaluate(&first, "seed", &Context::default(), 0.0);
        let mut scratch = vec![31, 41];
        struct Never;
        impl EventContext for Never {
            fn filter(&self, _: FilterBinding) -> Result<bool, String> {
                panic!("foreign handle queried context")
            }
        }
        assert_eq!(
            first.evaluate(
                &foreign,
                &Never,
                &mut || panic!("foreign handle consumed RNG"),
                &mut outcome,
                &mut scratch
            ),
            Err(EventEvaluationError::ForeignHandle),
        );
        assert_eq!(outcome.effects.len(), 1);
        assert_eq!(outcome.triggers.len(), 1);
        assert_eq!(scratch, vec![31, 41]);
        assert_ne!(first.event("random").unwrap(), foreign);
        first
            .evaluate(
                &first.event("random").unwrap(),
                &Context::default(),
                &mut || 0.75,
                &mut outcome,
                &mut scratch,
            )
            .unwrap();
        assert_eq!(outcome.effects.len(), 2);
        assert_eq!(scratch, vec![31, 41]);
    }

    #[test]
    fn external_subjects_and_trigger_filters_remain_explicit_and_unevaluated() {
        let source = control(json!({
            "root": {
                "add": {"component_groups": ["small"]},
                "trigger": {"event": "test:other", "target": "other",
                    "filters": {"test": "is_baby", "subject": "self", "value": true}},
                "sequence": [{
                    "filters": {"test": "is_baby", "subject": "other", "value": true},
                    "set_property": {"test:number": 6}
                }]
            }
        }));
        let (_, _, events) = compile(&source);
        let output = evaluate(&events, "root", &Context::default(), 0.0);
        assert!(matches!(
            output.effects.as_slice(),
            [EventEffect::AddGroup(_)]
        ));
        assert_eq!(output.triggers.len(), 1);
        let trigger = output.triggers[0];
        assert_eq!(trigger.target, Subject::Other);
        assert!(matches!(
            &trigger.event,
            TriggerEvent::External(ExternalEventBinding(9))
        ));
        assert!(!trigger.passes(&Context::default()).unwrap());
        assert!(
            trigger
                .passes(&Context {
                    baby: true,
                    ..Context::default()
                })
                .unwrap()
        );
        let with_other = evaluate(
            &events,
            "root",
            &Context {
                other_baby: Some(true),
                ..Context::default()
            },
            0.0,
        );
        assert!(matches!(
            with_other.effects.as_slice(),
            [EventEffect::AddGroup(_), EventEffect::SetProperty(_),]
        ));
    }

    #[test]
    fn unknown_action_invalidates_known_siblings_and_descendants_without_binding_them() {
        let source = control(json!({
            "invalid": {
                "addon:unknown": {},
                "add": {"component_groups": ["not_declared"]},
                "set_property": {"not_declared": true},
                "run_command": "not supported",
                "trigger": "not_declared",
                "randomize": [{"weight": 1, "run_command": "also not supported"}]
            }
        }));
        let (_, _, events) = compile(&source);
        let mut output = EventOutcome::default();
        events
            .evaluate(
                &events.event("invalid").unwrap(),
                &Context::default(),
                &mut || panic!("invalid node consumed RNG"),
                &mut output,
                &mut Vec::new(),
            )
            .unwrap();
        assert!(output.effects.is_empty());
        assert!(output.triggers.is_empty());
    }

    #[test]
    fn missing_names_and_unsupported_recognized_bindings_fail_compilation() {
        for event in [
            json!({"add": {"component_groups": ["missing"]}}),
            json!({"remove": {"component_groups": ["missing"]}}),
            json!({"trigger": "missing"}),
            json!({"set_property": {"missing": true}}),
            json!({"run_command": {"command": "say unsupported"}}),
            json!({"filters": {"test": "is_on_ground", "value": true}}),
            json!({"trigger": {"event": "missing", "target": "other"}}),
        ] {
            let source = control(json!({"root": event}));
            let mut composition_source = source.clone();
            composition_source.events.clear();
            let (definition, properties, _) = compile(&composition_source);
            assert!(
                CompiledEvents::compile(
                    &source,
                    &definition,
                    &properties,
                    QueryRegistry::vanilla(),
                    &mut Bindings,
                )
                .is_err()
            );
        }
    }

    #[test]
    fn runtime_binding_failure_restores_appended_output_and_scratch() {
        let source = control(json!({
            "seed": {"set_property": {"test:number": 3}},
            "root": {
                "set_property": {"test:number": 8},
                "trigger": "seed",
                "randomize": [
                    {"weight": 1, "set_property": {"test:number": 1}},
                    {"weight": 1, "filters": {"test": "is_baby", "value": true}}
                ]
            }
        }));
        let (_, _, events) = compile(&source);
        struct MissingContext;
        impl EventContext for MissingContext {
            fn filter(&self, _: FilterBinding) -> Result<bool, String> {
                Err("binding table unavailable".into())
            }
        }
        let mut outcome = evaluate(&events, "seed", &Context::default(), 0.0);
        let mut scratch = vec![123];
        let result = events.evaluate(
            &events.event("root").unwrap(),
            &MissingContext,
            &mut || panic!("filter failure preceded RNG"),
            &mut outcome,
            &mut scratch,
        );
        assert!(matches!(
            result,
            Err(EventEvaluationError::Filter {
                binding: FilterBinding(2),
                ..
            })
        ));
        assert_eq!(outcome.effects.len(), 1);
        assert!(outcome.triggers.is_empty());
        assert_eq!(scratch, vec![123]);
    }
}
