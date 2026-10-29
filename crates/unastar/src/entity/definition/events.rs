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

use super::properties::{PropertyAssignment, PropertyError, PropertyLayout};
use super::{CompiledDefinition, ComponentGroupId};

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
    /// Why a filter, action or external event cannot be bound.
    type Error: std::error::Error + Send + Sync + 'static;

    fn filter(&mut self, filter: &Filter) -> Result<FilterBinding, Self::Error>;
    fn action(&mut self, action: EventAction, value: &Value) -> Result<ActionBinding, Self::Error>;
    fn external_event(
        &mut self,
        target: Subject,
        event: &str,
    ) -> Result<ExternalEventBinding, Self::Error>;
}

/// Runtime counterpart of the same frozen binding tables. Filters are pure
/// and can be queried more than once by first_valid/randomize, just as in the
/// parsed evaluator. Missing subjects follow the shared filter semantics;
/// unsupported implementations are errors, not invented true/false answers.
pub trait EventContext {
    /// Why a bound filter cannot be answered.
    type Error: std::error::Error + Send + Sync + 'static;

    fn filter(&self, binding: FilterBinding) -> Result<bool, Self::Error>;
}

/// Why an actor's events cannot be bound; `path` names the failing node
/// (`<identifier>/events/<event>[/<children kind>/<index>...]`).
#[derive(Debug, thiserror::Error)]
#[error("actor event {path}: {reason}")]
pub struct EventCompileError<E: std::error::Error + 'static> {
    pub path: String,
    #[source]
    pub reason: EventCompileReason<E>,
}

/// What is wrong with one event node. `E` is the caller's binding error.
#[derive(Debug, thiserror::Error)]
pub enum EventCompileReason<E: std::error::Error + 'static> {
    #[error("component definition belongs to another actor identifier")]
    ForeignDefinition,
    #[error("remove references undeclared component group {0}")]
    UndeclaredRemoveGroup(String),
    #[error("add references undeclared component group {0}")]
    UndeclaredAddGroup(String),
    #[error("trigger references undeclared self event {0}")]
    UndeclaredEvent(String),
    #[error(transparent)]
    Property(#[from] PropertyError),
    /// The caller cannot bind a filter or an external event.
    #[error(transparent)]
    Binding(E),
    /// The caller cannot bind an action.
    #[error("{action}: {source}")]
    Action {
        action: &'static str,
        #[source]
        source: E,
    },
}

/// Why evaluating an event failed. `E` is the context's filter error.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EventEvaluationError<E: std::error::Error + 'static> {
    #[error("event handle belongs to another compiled event table")]
    ForeignHandle,
    #[error("event filter binding {binding:?}: {source}")]
    Filter {
        binding: FilterBinding,
        #[source]
        source: E,
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
    ) -> Result<bool, EventEvaluationError<C::Error>> {
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
) -> Result<bool, EventEvaluationError<C::Error>> {
    match filter {
        None => Ok(true),
        Some(binding) => context
            .filter(binding)
            .map_err(|source| EventEvaluationError::Filter { binding, source }),
    }
}

impl<C: EventContext + ?Sized> EventTree<C> for BoundNode {
    type Error = EventEvaluationError<C::Error>;

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
    pub fn compile<B: EventBindings>(
        source: &ActorDefinition,
        definition: &CompiledDefinition,
        properties: &Arc<PropertyLayout>,
        queries: &QueryRegistry,
        bindings: &mut B,
    ) -> Result<Self, EventCompileError<B::Error>> {
        if source.identifier() != definition.identifier() {
            return Err(EventCompileError {
                path: source.identifier().to_owned(),
                reason: EventCompileReason::ForeignDefinition,
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
    ) -> Result<(), EventEvaluationError<C::Error>> {
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
    fn node(
        &mut self,
        source: &EventNode,
        path: &str,
    ) -> Result<BoundNode, EventCompileError<B::Error>> {
        let error = |reason| EventCompileError {
            path: path.to_owned(),
            reason,
        };
        let binding = |source| error(EventCompileReason::Binding(source));
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
            .map(|filter| self.bindings.filter(filter).map_err(binding))
            .transpose()?;
        let mut effects = Vec::new();
        for name in source.remove.iter().flatten() {
            let group = self
                .definition
                .group(name)
                .ok_or_else(|| error(EventCompileReason::UndeclaredRemoveGroup(name.clone())))?;
            effects.push(EventEffectOwned::RemoveGroup(group));
        }
        for name in source.add.iter().flatten() {
            let group = self
                .definition
                .group(name)
                .ok_or_else(|| error(EventCompileReason::UndeclaredAddGroup(name.clone())))?;
            effects.push(EventEffectOwned::AddGroup(group));
        }
        for (name, value) in source.set_property.iter().flatten() {
            let assignment = self
                .properties
                .compile_assignment(name, value, self.queries)
                .map_err(|cause| error(cause.into()))?;
            effects.push(EventEffectOwned::SetProperty(assignment));
        }
        for (kind, value) in &source.actions {
            let binding = self.bindings.action(*kind, value).map_err(|source| {
                error(EventCompileReason::Action {
                    action: kind.key(),
                    source,
                })
            })?;
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
                        error(EventCompileReason::UndeclaredEvent(trigger.event.clone()))
                    })?;
                    TriggerEvent::Internal(EventHandle {
                        owner: Arc::clone(self.owner),
                        index,
                    })
                } else {
                    TriggerEvent::External(
                        self.bindings
                            .external_event(trigger.target, &trigger.event)
                            .map_err(binding)?,
                    )
                };
                let filter = trigger
                    .filters
                    .as_ref()
                    .map(|filter| self.bindings.filter(filter).map_err(binding))
                    .transpose()?;
                Ok::<_, EventCompileError<B::Error>>(DeferredTrigger {
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
                Ok::<_, EventCompileError<B::Error>>(match children {
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
mod tests;
