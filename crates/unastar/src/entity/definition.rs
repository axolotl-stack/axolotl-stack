//! Frozen component factories and transactional, numeric-ID composition batches.
//! JSON/name resolution happens while compiling definitions, never while applying.

use std::collections::BTreeMap;
use std::sync::Arc;

use bevy_ecs::prelude::*;
use serde_json::Value;
use unastar_data::actor_definitions::ActorDefinition;

use super::definition_stack::{
    ComponentBinding, CompositionChange, DefinitionComposition, DefinitionStack,
};

type Remove = fn(&mut EntityWorldMut<'_>);
type Compile = dyn Fn(&Value) -> Result<Box<dyn Prototype>, String> + Send + Sync;

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
    names: BTreeMap<Arc<str>, usize>,
    kinds: Vec<Kind>,
}

impl ComponentKindsBuilder {
    /// A kind owns its typed runtime state and removal semantics. Register
    /// vanilla/addon implementations before freezing the registry (D5).
    pub fn register<T: Send + Sync + 'static>(
        &mut self,
        name: &str,
        compile: impl Fn(&Value) -> Result<T, String> + Send + Sync + 'static,
        apply: fn(&T, &mut EntityWorldMut<'_>),
        remove: Remove,
    ) -> Result<(), String> {
        if self.names.contains_key(name) {
            return Err(format!("duplicate actor component kind {name}"));
        }
        self.names.insert(Arc::from(name), self.kinds.len());
        self.kinds.push(Kind {
            compile: Box::new(move |body| {
                let value = compile(body)?;
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
    names: BTreeMap<Arc<str>, usize>,
    kinds: Vec<Kind>,
}

#[derive(Clone)]
pub struct ComponentKindId {
    owner: Arc<ComponentKinds>,
    index: usize,
}

impl ComponentKinds {
    pub fn resolve(self: &Arc<Self>, name: &str) -> Option<ComponentKindId> {
        Some(ComponentKindId {
            owner: Arc::clone(self),
            index: *self.names.get(name)?,
        })
    }
}

pub struct CompiledDefinition {
    identifier: Arc<str>,
    kinds: Arc<ComponentKinds>,
    composition: DefinitionComposition,
    groups: Arc<BTreeMap<Arc<str>, usize>>,
    prototypes: Vec<Box<dyn Prototype>>,
}

impl CompiledDefinition {
    pub fn compile(
        source: &ActorDefinition,
        kinds: Arc<ComponentKinds>,
    ) -> Result<Arc<Self>, String> {
        let mut prototypes = Vec::new();
        let mut compile_set = |components: &BTreeMap<String, Value>| {
            let mut bindings = Vec::with_capacity(components.len());
            for (name, body) in components {
                if source.ignored_components.contains(name) {
                    continue;
                }
                let kind = *kinds.names.get(name.as_str()).ok_or_else(|| {
                    format!(
                        "{}: no runtime implementation for component {name}",
                        source.identifier()
                    )
                })?;
                let prototype = (kinds.kinds[kind].compile)(body)
                    .map_err(|cause| format!("{} / {name}: {cause}", source.identifier()))?;
                bindings.push(ComponentBinding {
                    kind,
                    prototype: prototypes.len(),
                });
                prototypes.push(prototype);
            }
            Ok::<_, String>(bindings)
        };
        let base = compile_set(&source.components)?;
        let mut groups = BTreeMap::new();
        let mut group_bindings = Vec::with_capacity(source.component_groups.len());
        for (name, components) in &source.component_groups {
            groups.insert(Arc::from(name.as_str()), group_bindings.len());
            group_bindings.push(compile_set(components)?);
        }
        let composition = DefinitionComposition::new(kinds.kinds.len(), base, group_bindings);
        Ok(Arc::new(Self {
            identifier: Arc::from(source.identifier()),
            kinds,
            composition,
            groups: Arc::new(groups),
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
}

impl DefinitionState {
    /// Numeric membership query for compiled filters/Molang. Handles from
    /// another frozen kind registry cannot accidentally address this stack.
    pub fn has_component(&self, kind: &ComponentKindId) -> Result<bool, String> {
        if !Arc::ptr_eq(&kind.owner, &self.definition.kinds) {
            return Err("component kind belongs to another registry".into());
        }
        Ok(self.stack.component(kind.index).is_some())
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
) -> Result<(), String> {
    let kind_count = definition.kinds.kinds.len();
    let stack = DefinitionStack::new(&definition.composition);
    let mut target = world
        .get_entity_mut(entity)
        .map_err(|cause| cause.to_string())?;
    if target.contains::<DefinitionState>() {
        return Err("actor already has a definition stack".into());
    }
    target.insert(DefinitionState {
        definition,
        stack,
        changes: Vec::with_capacity(kind_count),
        queued: true,
    });
    world.init_resource::<DefinitionApplyQueue>();
    world.resource_mut::<DefinitionApplyQueue>().0.push(entity);
    Ok(())
}

pub fn queue_group_change(
    world: &mut World,
    entity: Entity,
    change: GroupChange,
) -> Result<(), String> {
    let mut state = world
        .get_mut::<DefinitionState>(entity)
        .ok_or_else(|| "actor has no definition stack".to_owned())?;
    let state = &mut *state;
    let group = match &change {
        GroupChange::Add(group) | GroupChange::Remove(group) => group,
    };
    if !Arc::ptr_eq(&group.owner, &state.definition.groups) {
        return Err("component group belongs to another definition".into());
    }
    match change {
        GroupChange::Add(group) => state
            .stack
            .add_group(&state.definition.composition, group.index),
        GroupChange::Remove(group) => state
            .stack
            .remove_group(&state.definition.composition, group.index),
    }
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
mod tests {
    use super::*;
    use serde_json::json;
    use unastar_packs::actor::{Loaded, load_value};

    #[derive(Component)]
    struct Baby;

    fn source(components: Value, groups: Value) -> ActorDefinition {
        let document = json!({
            "format_version": "1.26.50",
            "minecraft:entity": {
                "description": {"identifier": "test:composition", "is_summonable": true},
                "components": components, "component_groups": groups
            }
        });
        let Loaded::Definition { definition, .. } = load_value(
            "composition control",
            &document,
            unastar_data::actor_definitions::parser(),
        )
        .unwrap() else {
            panic!("control definition skipped")
        };
        *definition
    }

    fn kinds() -> Arc<ComponentKinds> {
        let mut kinds = ComponentKindsBuilder::default();
        kinds
            .register(
                "minecraft:is_baby",
                |_| Ok(()),
                |_, entity| {
                    entity.insert(Baby);
                },
                |entity| {
                    entity.remove::<Baby>();
                },
            )
            .unwrap();
        kinds.freeze()
    }

    #[test]
    fn native_destructive_group_removal_reaches_actual_ecs_state() {
        let kinds = kinds();
        let baby = kinds.resolve("minecraft:is_baby").unwrap();
        let definition = CompiledDefinition::compile(
            &source(
                json!({"minecraft:is_baby": {}}),
                json!({"override": {"minecraft:is_baby": {}}}),
            ),
            kinds,
        )
        .unwrap();
        let group = definition.group("override").unwrap();
        let mut world = World::new();
        let actor = world.spawn_empty().id();
        attach_definition(&mut world, actor, definition).unwrap();
        apply_pending_definitions(&mut world);
        assert!(world.get::<Baby>(actor).is_some());
        queue_group_change(&mut world, actor, GroupChange::Add(group.clone())).unwrap();
        apply_pending_definitions(&mut world);
        queue_group_change(&mut world, actor, GroupChange::Remove(group)).unwrap();
        apply_pending_definitions(&mut world);
        assert!(world.get::<Baby>(actor).is_none());
        assert!(
            !world
                .get::<DefinitionState>(actor)
                .unwrap()
                .has_component(&baby)
                .unwrap()
        );
    }

    #[test]
    fn transient_group_is_not_materialized_and_foreign_handles_do_not_mutate() {
        let kinds = kinds();
        let definition_source = source(json!({}), json!({"baby": {"minecraft:is_baby": {}}}));
        let first = CompiledDefinition::compile(&definition_source, Arc::clone(&kinds)).unwrap();
        let second = CompiledDefinition::compile(&definition_source, kinds).unwrap();
        let group = first.group("baby").unwrap();
        let foreign = second.group("baby").unwrap();
        let mut world = World::new();
        #[derive(Resource, Default)]
        struct SpawnNotifications(usize);
        world.init_resource::<SpawnNotifications>();
        world
            .register_component_hooks::<Baby>()
            .on_add(|mut world, _| {
                world.resource_mut::<SpawnNotifications>().0 += 1;
            });
        let actor = world.spawn_empty().id();
        attach_definition(&mut world, actor, first).unwrap();
        queue_group_change(&mut world, actor, GroupChange::Add(group.clone())).unwrap();
        queue_group_change(&mut world, actor, GroupChange::Remove(group.clone())).unwrap();
        assert!(queue_group_change(&mut world, actor, GroupChange::Add(foreign)).is_err());
        apply_pending_definitions(&mut world);
        assert!(world.get::<Baby>(actor).is_none());
        assert_eq!(world.resource::<SpawnNotifications>().0, 0);
        let foreign_kind = self::kinds().resolve("minecraft:is_baby").unwrap();
        assert!(
            world
                .get::<DefinitionState>(actor)
                .unwrap()
                .has_component(&foreign_kind)
                .is_err()
        );
        queue_group_change(&mut world, actor, GroupChange::Add(group)).unwrap();
        apply_pending_definitions(&mut world);
        assert!(world.get::<Baby>(actor).is_some());
        assert_eq!(world.resource::<SpawnNotifications>().0, 1);
    }

    #[test]
    fn group_change_queued_by_component_hook_waits_for_next_barrier() {
        #[derive(Resource)]
        struct RemoveSelf(ComponentGroupId);
        let mut kinds = ComponentKindsBuilder::default();
        kinds
            .register(
                "minecraft:is_baby",
                |_| Ok(()),
                |_, entity| {
                    entity.insert(Baby);
                    let actor = entity.id();
                    entity.world_scope(|world| {
                        let group = world.resource::<RemoveSelf>().0.clone();
                        queue_group_change(world, actor, GroupChange::Remove(group)).unwrap();
                    });
                },
                |entity| {
                    entity.remove::<Baby>();
                },
            )
            .unwrap();
        let kinds = kinds.freeze();
        let baby = kinds.resolve("minecraft:is_baby").unwrap();
        let definition = CompiledDefinition::compile(
            &source(json!({}), json!({"baby": {"minecraft:is_baby": {}}})),
            kinds,
        )
        .unwrap();
        let group = definition.group("baby").unwrap();
        let mut world = World::new();
        world.insert_resource(RemoveSelf(group.clone()));
        let actor = world.spawn_empty().id();
        attach_definition(&mut world, actor, definition).unwrap();
        queue_group_change(&mut world, actor, GroupChange::Add(group)).unwrap();
        apply_pending_definitions(&mut world);
        assert!(world.get::<Baby>(actor).is_some());
        assert!(
            !world
                .get::<DefinitionState>(actor)
                .unwrap()
                .has_component(&baby)
                .unwrap()
        );
        apply_pending_definitions(&mut world);
        assert!(world.get::<Baby>(actor).is_none());
    }

    #[derive(Component)]
    struct A(i64);
    #[derive(Component)]
    struct B(i64);

    #[test]
    fn every_previous_component_is_removed_before_replacement_callbacks() {
        let mut kinds = ComponentKindsBuilder::default();
        let parse = |body: &Value| {
            body["value"]
                .as_i64()
                .ok_or_else(|| "integer value required".into())
        };
        kinds
            .register(
                "minecraft:scale",
                parse,
                |value, entity| {
                    entity.insert(A(*value));
                },
                |entity| {
                    entity.remove::<A>();
                },
            )
            .unwrap();
        kinds
            .register(
                "minecraft:health",
                parse,
                |value, entity| {
                    if *value == 2 {
                        assert!(!entity.contains::<A>());
                    }
                    entity.insert(B(*value));
                },
                |entity| {
                    entity.remove::<B>();
                },
            )
            .unwrap();
        let definition = CompiledDefinition::compile(&source(
            json!({"minecraft:scale": {"value": 1}, "minecraft:health": {"value": 1}}),
            json!({"replacement": {"minecraft:scale": {"value": 2}, "minecraft:health": {"value": 2}}}),
        ), kinds.freeze()).unwrap();
        let group = definition.group("replacement").unwrap();
        let mut world = World::new();
        let actor = world.spawn_empty().id();
        attach_definition(&mut world, actor, definition).unwrap();
        apply_pending_definitions(&mut world);
        assert_eq!(world.get::<A>(actor).unwrap().0, 1);
        assert_eq!(world.get::<B>(actor).unwrap().0, 1);
        queue_group_change(&mut world, actor, GroupChange::Add(group)).unwrap();
        apply_pending_definitions(&mut world);
        assert_eq!(world.get::<A>(actor).unwrap().0, 2);
        assert_eq!(world.get::<B>(actor).unwrap().0, 2);
    }
}
