//! Frozen component factories and transactional, numeric-ID composition batches.
//! Components are identified by the generated [`ActorComponent`] enum and
//! their bodies decoded into the generated typed structs while compiling
//! definitions, never while applying.

use std::collections::BTreeMap;
use std::sync::Arc;

use bevy_ecs::prelude::*;
use serde_json::Value;
use unastar_data::actor_components::{ActorComponent, ComponentBody, decode};
use unastar_data::actor_definitions::ActorDefinition;

use super::error::{ComponentCompileError, CompositionError, DefinitionError};
use super::stack::{ComponentBinding, CompositionChange, DefinitionComposition, DefinitionStack};

type Remove = fn(&mut EntityWorldMut<'_>);
type Compile = dyn Fn(&Value) -> Result<Box<dyn Prototype>, ComponentCompileError> + Send + Sync;

trait Prototype: Send + Sync {
    fn apply(&self, entity: &mut EntityWorldMut<'_>);
}

struct TypedPrototype<T> {
    value: T,
    apply: fn(&T, &mut EntityWorldMut<'_>),
}

impl<T: Send + Sync> Prototype for TypedPrototype<T> {
    fn apply(&self, entity: &mut EntityWorldMut<'_>) {
        (self.apply)(&self.value, entity);
    }
}

struct Kind {
    compile: Box<Compile>,
    remove: Remove,
}

#[derive(Default)]
pub struct ComponentKindsBuilder {
    names: BTreeMap<ActorComponent, usize>,
    kinds: Vec<Kind>,
}

impl ComponentKindsBuilder {
    /// A kind owns its typed runtime state and removal semantics. Register
    /// vanilla/addon implementations before freezing the registry (D5).
    /// The component is the body type's [`ComponentBody::COMPONENT`]; the
    /// authored JSON is decoded into `B` (unknown fields are rejected), then
    /// `compile` turns it into the runtime value `T`.
    pub fn register<B: ComponentBody, T: Send + Sync + 'static>(
        &mut self,
        compile: impl Fn(B) -> Result<T, ComponentCompileError> + Send + Sync + 'static,
        apply: fn(&T, &mut EntityWorldMut<'_>),
        remove: Remove,
    ) -> Result<(), DefinitionError> {
        let component = B::COMPONENT;
        if self.names.contains_key(&component) {
            return Err(DefinitionError::DuplicateKind(component.name()));
        }
        self.names.insert(component, self.kinds.len());
        self.kinds.push(Kind {
            compile: Box::new(move |body| {
                let value = compile(decode::<B>(body).map_err(ComponentCompileError::Decode)?)?;
                Ok(Box::new(TypedPrototype { value, apply }))
            }),
            remove,
        });
        Ok(())
    }

    pub fn freeze(self) -> Arc<ComponentKinds> {
        Arc::new(ComponentKinds {
            names: self.names,
            kinds: self.kinds,
        })
    }
}

pub struct ComponentKinds {
    names: BTreeMap<ActorComponent, usize>,
    kinds: Vec<Kind>,
}

#[derive(Clone)]
pub struct ComponentKindId {
    owner: Arc<ComponentKinds>,
    index: usize,
}

impl ComponentKinds {
    pub fn resolve(self: &Arc<Self>, component: ActorComponent) -> Option<ComponentKindId> {
        Some(ComponentKindId {
            owner: Arc::clone(self),
            index: *self.names.get(&component)?,
        })
    }
}

pub struct CompiledDefinition {
    identifier: Arc<str>,
    kinds: Arc<ComponentKinds>,
    composition: DefinitionComposition,
    groups: Arc<BTreeMap<Arc<str>, usize>>,
    /// Group names by group index.
    group_names: Box<[Arc<str>]>,
    prototypes: Vec<Box<dyn Prototype>>,
}

impl CompiledDefinition {
    pub fn compile(
        source: &ActorDefinition,
        kinds: Arc<ComponentKinds>,
    ) -> Result<Arc<Self>, DefinitionError> {
        let mut prototypes = Vec::new();
        let mut compile_set = |components: &BTreeMap<String, Value>| {
            let mut bindings = Vec::with_capacity(components.len());
            for (name, body) in components {
                if source.ignored_components.contains(name) {
                    continue;
                }
                let kind = ActorComponent::from_name(name)
                    .and_then(|component| kinds.names.get(&component).copied())
                    .ok_or_else(|| DefinitionError::UnknownComponent {
                        identifier: source.identifier().to_owned(),
                        component: name.clone(),
                    })?;
                let prototype = (kinds.kinds[kind].compile)(body).map_err(|cause| {
                    DefinitionError::Component {
                        identifier: source.identifier().to_owned(),
                        component: name.clone(),
                        source: cause,
                    }
                })?;
                bindings.push(ComponentBinding {
                    kind,
                    prototype: prototypes.len(),
                });
                prototypes.push(prototype);
            }
            Ok::<_, DefinitionError>(bindings)
        };
        let base = compile_set(&source.components)?;
        let mut groups = BTreeMap::new();
        let mut group_names = Vec::with_capacity(source.component_groups.len());
        let mut group_bindings = Vec::with_capacity(source.component_groups.len());
        for (name, components) in &source.component_groups {
            let name: Arc<str> = Arc::from(name.as_str());
            groups.insert(Arc::clone(&name), group_bindings.len());
            group_names.push(name);
            group_bindings.push(compile_set(components)?);
        }
        let composition = DefinitionComposition::new(kinds.kinds.len(), base, group_bindings);
        Ok(Arc::new(Self {
            identifier: Arc::from(source.identifier()),
            kinds,
            composition,
            groups: Arc::new(groups),
            group_names: group_names.into_boxed_slice(),
            prototypes,
        }))
    }

    pub fn identifier(&self) -> &str {
        &self.identifier
    }

    /// Resolve event/command names once, before queuing numeric group changes.
    pub fn group(&self, name: &str) -> Option<ComponentGroupId> {
        Some(ComponentGroupId {
            owner: Arc::clone(&self.groups),
            index: *self.groups.get(name)?,
        })
    }
}

#[derive(Component)]
pub struct DefinitionState {
    definition: Arc<CompiledDefinition>,
    stack: DefinitionStack,
    changes: Vec<CompositionChange>,
    queued: bool,
    /// Every group added or removed since attachment, once each, in order of
    /// its latest change (`true` = added).
    group_history: Vec<(usize, bool)>,
}

impl DefinitionState {
    /// Numeric membership query for compiled filters/Molang. Handles from
    /// another frozen kind registry cannot accidentally address this stack.
    pub fn has_component(&self, kind: &ComponentKindId) -> Result<bool, CompositionError> {
        if !Arc::ptr_eq(&kind.owner, &self.definition.kinds) {
            return Err(CompositionError::ForeignKind);
        }
        Ok(self.stack.component(kind.index).is_some())
    }

    /// Whether the current composition includes `component` (the
    /// `has_component` filter test).
    pub fn has_actor_component(&self, component: ActorComponent) -> bool {
        self.definition
            .kinds
            .names
            .get(&component)
            .is_some_and(|&kind| self.stack.component(kind).is_some())
    }

    /// The component groups added (`true`) or removed (`false`) since the
    /// definition was attached, each once, ordered by its latest change.
    ///
    /// Groups are destructive patches, so a kind's final selection is set by
    /// the last change touching it; replaying this list over the base
    /// selection with [`queue_group_change`] reproduces the current
    /// composition. This is the `definitions` list actors persist.
    pub fn group_history(&self) -> impl Iterator<Item = (&str, bool)> + '_ {
        self.group_history
            .iter()
            .map(|&(group, added)| (&*self.definition.group_names[group], added))
    }
}

#[derive(Clone)]
pub struct ComponentGroupId {
    owner: Arc<BTreeMap<Arc<str>, usize>>,
    index: usize,
}

#[derive(Resource, Default)]
pub(crate) struct DefinitionApplyQueue(Vec<Entity>);

#[derive(Clone)]
pub enum GroupChange {
    Add(ComponentGroupId),
    Remove(ComponentGroupId),
}

/// Attach the base selection. The exclusive apply phase materializes it before
/// physics/AI/network systems run, not through intermediate group states.
pub fn attach_definition(
    world: &mut World,
    entity: Entity,
    definition: Arc<CompiledDefinition>,
) -> Result<(), CompositionError> {
    let kind_count = definition.kinds.kinds.len();
    let stack = DefinitionStack::new(&definition.composition);
    let mut target = world.get_entity_mut(entity)?;
    if target.contains::<DefinitionState>() {
        return Err(CompositionError::AlreadyAttached);
    }
    target.insert(DefinitionState {
        definition,
        stack,
        changes: Vec::with_capacity(kind_count),
        queued: true,
        group_history: Vec::new(),
    });
    world.init_resource::<DefinitionApplyQueue>();
    world.resource_mut::<DefinitionApplyQueue>().0.push(entity);
    Ok(())
}

pub fn queue_group_change(
    world: &mut World,
    entity: Entity,
    change: GroupChange,
) -> Result<(), CompositionError> {
    let mut state = world
        .get_mut::<DefinitionState>(entity)
        .ok_or(CompositionError::NoStack)?;
    let state = &mut *state;
    let group = match &change {
        GroupChange::Add(group) | GroupChange::Remove(group) => group,
    };
    if !Arc::ptr_eq(&group.owner, &state.definition.groups) {
        return Err(CompositionError::ForeignGroup);
    }
    let (index, added) = match change {
        GroupChange::Add(group) => {
            state
                .stack
                .add_group(&state.definition.composition, group.index);
            (group.index, true)
        }
        GroupChange::Remove(group) => {
            state
                .stack
                .remove_group(&state.definition.composition, group.index);
            (group.index, false)
        }
    };
    state.group_history.retain(|&(group, _)| group != index);
    state.group_history.push((index, added));
    let enqueue = !state.queued;
    state.queued = true;
    if enqueue {
        world.resource_mut::<DefinitionApplyQueue>().0.push(entity);
    }
    Ok(())
}

/// All removals precede all insertions for an actor. The scratch storage is
/// returned to its component for reuse. Changes queued by component hooks wait
/// for the next apply barrier instead of being recursively materialized.
pub fn apply_pending_definitions(world: &mut World) {
    let Some(mut queue) = world.get_resource_mut::<DefinitionApplyQueue>() else {
        return;
    };
    let mut pending = std::mem::take(&mut queue.0);
    for entity in pending.drain(..) {
        let Some(mut state) = world.get_mut::<DefinitionState>(entity) else {
            continue;
        };
        state.queued = false;
        let state = &mut *state;
        state.stack.collect_changes(&mut state.changes);
        let definition = Arc::clone(&state.definition);
        let changes = std::mem::take(&mut state.changes);
        {
            let mut target = world.entity_mut(entity);
            for change in &changes {
                if change.previous.is_some() {
                    (definition.kinds.kinds[change.kind].remove)(&mut target);
                }
            }
            for change in &changes {
                if let Some(prototype) = change.next {
                    definition.prototypes[prototype].apply(&mut target);
                }
            }
        }
        if let Some(mut state) = world.get_mut::<DefinitionState>(entity) {
            state.changes = changes;
        }
    }
    let mut queue = world.resource_mut::<DefinitionApplyQueue>();
    pending.append(&mut queue.0);
    queue.0 = pending;
}

#[cfg(test)]
mod tests;
