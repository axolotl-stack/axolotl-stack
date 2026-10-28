//! Pack-authored type families resolved to scoped numeric memberships at startup.
//! Family matching performs no string lookup or allocation in actor/filter loops.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use bevy_ecs::prelude::{Component, EntityWorldMut};
use unastar_data::actor_components::{ActorComponent, body, decode};
use unastar_data::actor_definitions::ActorDefinition;

use super::{ComponentError, ComponentKindsBuilder, DefinitionError};

/// Why a definition's families cannot be collected or resolved.
#[derive(Debug, thiserror::Error)]
pub enum FamilyError {
    /// A `minecraft:type_family` body does not decode (the generated
    /// decoder's report).
    #[error("{0}")]
    Decode(String),
    #[error("family {0} was not registered before freeze")]
    Unregistered(String),
}

impl ComponentError for FamilyError {}

/// Collect all built-in and addon family names before freezing runtime handles.
#[derive(Default)]
pub struct FamilyRegistryBuilder {
    names: BTreeSet<Arc<str>>,
}

impl FamilyRegistryBuilder {
    pub fn register(&mut self, name: &str) {
        self.names.insert(Arc::from(name));
    }

    pub fn register_definition(&mut self, source: &ActorDefinition) -> Result<(), FamilyError> {
        let component = ActorComponent::TypeFamily.name();
        if source.ignored_components.contains(component) {
            return Ok(());
        }
        // Validate the entire definition before changing the builder.
        let mut names = BTreeSet::new();
        for components in
            std::iter::once(&source.components).chain(source.component_groups.values())
        {
            if let Some(value) = components.get(component) {
                let families = decode::<body::TypeFamily>(value).map_err(FamilyError::Decode)?;
                names.extend(families.family.into_iter().flatten().map(Arc::from));
            }
        }
        self.names.extend(names);
        Ok(())
    }

    pub fn freeze(self) -> Arc<FamilyRegistry> {
        Arc::new(FamilyRegistry {
            names: self
                .names
                .into_iter()
                .enumerate()
                .map(|(index, name)| (name, index))
                .collect(),
        })
    }
}

pub struct FamilyRegistry {
    names: BTreeMap<Arc<str>, usize>,
}

/// A family slot belongs to exactly one frozen registry, including addon rows.
#[derive(Clone)]
pub struct FamilyId {
    owner: Arc<FamilyRegistry>,
    index: usize,
}

impl FamilyRegistry {
    /// Resolve filter/Molang operands at compilation, not while matching actors.
    pub fn resolve(self: &Arc<Self>, name: &str) -> Option<FamilyId> {
        Some(FamilyId {
            owner: Arc::clone(self),
            index: *self.names.get(name)?,
        })
    }

    pub fn register_component_kind(
        self: &Arc<Self>,
        builder: &mut ComponentKindsBuilder,
    ) -> Result<(), DefinitionError> {
        let registry = Arc::clone(self);
        builder.register(
            move |body: body::TypeFamily| {
                Ok(registry.membership(body.family.as_deref().unwrap_or_default())?)
            },
            |families: &ActorFamilies, entity| {
                entity.insert(families.clone());
            },
            |entity: &mut EntityWorldMut<'_>| {
                entity.remove::<ActorFamilies>();
            },
        )
    }

    /// Membership for family names that were all registered before freeze.
    fn membership(self: &Arc<Self>, names: &[String]) -> Result<ActorFamilies, FamilyError> {
        let mut words = vec![0; self.names.len().div_ceil(u64::BITS as usize)];
        for name in names {
            let index = self
                .names
                .get(name.as_str())
                .ok_or_else(|| FamilyError::Unregistered(name.clone()))?;
            words[index / u64::BITS as usize] |= 1u64 << (index % u64::BITS as usize);
        }
        Ok(ActorFamilies {
            registry: Arc::clone(self),
            words: words.into(),
        })
    }
}

/// Shared immutable membership of one effective component prototype. Definition
/// group replacement swaps the component; instances do not copy the bitset.
#[derive(Component, Clone)]
pub struct ActorFamilies {
    registry: Arc<FamilyRegistry>,
    words: Arc<[u64]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("family handle belongs to another frozen registry")]
pub struct ForeignFamilyHandle;

impl ActorFamilies {
    pub fn contains(&self, family: &FamilyId) -> Result<bool, ForeignFamilyHandle> {
        if !Arc::ptr_eq(&self.registry, &family.owner) {
            return Err(ForeignFamilyHandle);
        }
        Ok(self.words[family.index / u64::BITS as usize]
            & (1u64 << (family.index % u64::BITS as usize))
            != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::prelude::World;
    use serde_json::json;
    use unastar_packs::actor::{Loaded, load_value};

    use crate::entity::definition::{
        CompiledDefinition, GroupChange, apply_pending_definitions, attach_definition,
        queue_group_change,
    };

    fn source() -> ActorDefinition {
        let Loaded::Definition { definition, .. } = load_value(
            "family transition control",
            &json!({
                "format_version": "1.26.50",
                "minecraft:entity": {
                    "description": {"identifier": "test:families"},
                    "components": {"minecraft:type_family": {"family": ["base", "shared"]}},
                    "component_groups": {
                        "replacement": {"minecraft:type_family": {"family": ["addon", "shared", "addon"]}}
                    }
                }
            }),
            unastar_data::actor_definitions::parser(),
        )
        .unwrap() else {
            panic!("family control was skipped")
        };
        *definition
    }

    #[test]
    fn group_replacement_and_removal_do_not_union_or_restore_memberships() {
        let source = source();
        let mut builder = FamilyRegistryBuilder::default();
        builder.register_definition(&source).unwrap();
        let families = builder.freeze();
        let base = families.resolve("base").unwrap();
        let addon = families.resolve("addon").unwrap();
        let shared = families.resolve("shared").unwrap();
        let mut kinds = ComponentKindsBuilder::default();
        families.register_component_kind(&mut kinds).unwrap();
        let definition = CompiledDefinition::compile(&source, kinds.freeze()).unwrap();
        let group = definition.group("replacement").unwrap();
        let mut world = World::new();
        let actor = world.spawn_empty().id();
        attach_definition(&mut world, actor, definition).unwrap();
        apply_pending_definitions(&mut world);
        let membership = world.get::<ActorFamilies>(actor).unwrap();
        assert_eq!(membership.contains(&base), Ok(true));
        assert_eq!(membership.contains(&shared), Ok(true));
        assert_eq!(membership.contains(&addon), Ok(false));

        queue_group_change(&mut world, actor, GroupChange::Add(group.clone())).unwrap();
        // The definition barrier, not queueing, replaces the runtime membership.
        assert_eq!(
            world.get::<ActorFamilies>(actor).unwrap().contains(&base),
            Ok(true)
        );
        apply_pending_definitions(&mut world);
        let membership = world.get::<ActorFamilies>(actor).unwrap();
        assert_eq!(membership.contains(&base), Ok(false));
        assert_eq!(membership.contains(&addon), Ok(true));
        assert_eq!(membership.contains(&shared), Ok(true));
        queue_group_change(&mut world, actor, GroupChange::Remove(group)).unwrap();
        apply_pending_definitions(&mut world);
        assert!(world.get::<ActorFamilies>(actor).is_none());
    }

    #[test]
    fn equal_numeric_slots_from_other_freezes_are_rejected() {
        let mut builder = FamilyRegistryBuilder::default();
        builder.register("same");
        let first = builder.freeze();
        let membership = first.membership(&["same".to_owned()]).unwrap();
        let mut builder = FamilyRegistryBuilder::default();
        builder.register("same");
        let second = builder.freeze();
        assert_eq!(
            membership.contains(&second.resolve("same").unwrap()),
            Err(ForeignFamilyHandle)
        );
        assert_eq!(
            membership.contains(&first.resolve("same").unwrap()),
            Ok(true)
        );
    }

    #[test]
    fn late_names_and_non_string_members_are_not_silently_lost() {
        let mut builder = FamilyRegistryBuilder::default();
        builder.register("known");
        let families = builder.freeze();
        assert!(
            families
                .membership(&["known".to_owned(), "late".to_owned()])
                .is_err()
        );
        assert!(decode::<body::TypeFamily>(&json!({"family": ["known", false]})).is_err());
    }
}
