//! Per-identifier mob archetypes compiled once at startup.
//!
//! An archetype bundles everything a spawn needs: the compiled component
//! definition, its bound event table, the property layout, the AddActor
//! attribute layout and the filter binding table. A definition becomes an
//! archetype only when every component it uses has a runtime kind and every
//! event filter/action it uses is implemented; anything else is reported as
//! unsupported with the first reason, and `/summon` refuses it.

use std::collections::BTreeMap;
use std::sync::Arc;

use bevy_ecs::prelude::Resource;
use serde_json::Value;
use thiserror::Error;
use unastar_data::actor_components::ActorComponent;
use unastar_data::actor_definitions::ActorDefinition;
use unastar_data::actor_identifiers::ActorType;
use unastar_rules::event::EventAction;
use unastar_rules::filter::{BuiltinTest, Filter, Subject};
use unastar_rules::molang::QueryRegistry;

use super::native::NativeTraits;
use crate::entity::definition::attributes::{
    AttributeError, AttributeId, AttributeLayout, AttributeValue,
};
use crate::entity::definition::events::{
    ActionBinding, CompiledEvents, EventBindings, EventCompileError, EventHandle,
    ExternalEventBinding, FilterBinding,
};
use crate::entity::definition::families::{FamilyError, FamilyRegistry, FamilyRegistryBuilder};
use crate::entity::definition::properties::{ActorPropertyRegistry, PropertyError, PropertyLayout};
use crate::entity::definition::{
    CompiledDefinition, ComponentKinds, ComponentKindsBuilder, DefinitionError,
};
use crate::entity::projectile::{self, ProjectileError};
use crate::registry::ItemRegistry;
use crate::registry::entity::ActorDefinitionRegistry;

/// A filter test the mob runtime cannot answer (see [`check_filter`]).
#[derive(Debug, Error)]
#[error("filter test {test} on {subject} is not implemented")]
pub struct UnsupportedFilterTest {
    pub test: String,
    pub subject: &'static str,
}

/// Why an archetype's events cannot bind a filter, action or external event.
#[derive(Debug, Error)]
pub enum BindingError {
    #[error(transparent)]
    Filter(#[from] UnsupportedFilterTest),
    #[error("event action {0} is not implemented")]
    Action(&'static str),
    #[error("event {event} targeting {target} is not implemented")]
    ExternalEvent { event: String, target: &'static str },
}

/// Why a definition cannot become a spawnable archetype.
#[derive(Debug, Error)]
pub enum ArchetypeError {
    #[error("{0} is not a native actor type")]
    NotNativeActor(String),
    #[error(transparent)]
    Definition(#[from] DefinitionError),
    #[error("a projectile component in a component group is not implemented")]
    ProjectileInGroup,
    #[error(transparent)]
    Properties(#[from] PropertyError),
    #[error(transparent)]
    Events(#[from] EventCompileError<BindingError>),
    #[error("{0}: no native AddActor attribute observation")]
    NoAttributeObservation(String),
    #[error(transparent)]
    Attributes(#[from] AttributeError),
    /// What the projectile spawns on hit is not spawnable.
    #[error(transparent)]
    Projectile(#[from] ProjectileError),
}

/// Why the mob registry cannot be built at all (as opposed to one
/// definition being unsupported, which [`MobRegistry`] records).
#[derive(Debug, Error)]
pub enum MobRegistryError {
    #[error(transparent)]
    Families(#[from] FamilyError),
    #[error(transparent)]
    Kinds(#[from] DefinitionError),
}

/// Every component kind a mob can use, frozen once for the process.
pub fn component_kinds(
    families: &Arc<FamilyRegistry>,
    items: &Arc<ItemRegistry>,
) -> Result<Arc<ComponentKinds>, DefinitionError> {
    let mut builder = ComponentKindsBuilder::default();
    crate::entity::definition::components::register_component_kinds(&mut builder)?;
    crate::entity::definition::flags::register_component_kinds(&mut builder)?;
    families.register_component_kind(&mut builder)?;
    super::components::register_component_kinds(&mut builder, items)?;
    projectile::register_component_kinds(&mut builder, items)?;
    Ok(builder.freeze())
}

/// Event filters the mob runtime can answer (see `super::events::MobFacts`).
/// Binding any other test fails the archetype instead of guessing a value.
fn supported_test(test: BuiltinTest, subject: Subject) -> bool {
    match test {
        BuiltinTest::HasBiomeTag
        | BuiltinTest::HasComponent
        | BuiltinTest::InLava
        | BuiltinTest::IsBaby
        | BuiltinTest::IsDifficulty
        | BuiltinTest::BoolProperty
        | BuiltinTest::IntProperty
        | BuiltinTest::RiderCount => subject == Subject::Self_,
        BuiltinTest::IsSneakHeld => subject == Subject::Other,
        BuiltinTest::IsFamily
        | BuiltinTest::IsRiding
        | BuiltinTest::HasEquipment
        | BuiltinTest::EnumProperty
        | BuiltinTest::InContactWithWater
        | BuiltinTest::InWater
        | BuiltinTest::IsUnderwater
        | BuiltinTest::IsVehicleFamily => {
            matches!(subject, Subject::Self_ | Subject::Other)
        }
        _ => false,
    }
}

/// The frozen filter table an archetype's events and components reference.
#[derive(Default)]
struct MobBindings {
    filters: Vec<Filter>,
}

impl EventBindings for MobBindings {
    type Error = BindingError;

    fn filter(&mut self, filter: &Filter) -> Result<FilterBinding, BindingError> {
        check_filter(filter)?;
        self.filters.push(filter.clone());
        Ok(FilterBinding((self.filters.len() - 1) as u32))
    }

    fn action(&mut self, action: EventAction, _: &Value) -> Result<ActionBinding, BindingError> {
        Err(BindingError::Action(action.key()))
    }

    fn external_event(
        &mut self,
        target: Subject,
        event: &str,
    ) -> Result<ExternalEventBinding, BindingError> {
        Err(BindingError::ExternalEvent {
            event: event.to_owned(),
            target: target.name(),
        })
    }
}

/// Reject a filter containing any test the runtime cannot answer.
pub(super) fn check_filter(filter: &Filter) -> Result<(), UnsupportedFilterTest> {
    let mut failure = None;
    filter.for_each_test(&mut |test| {
        if failure.is_some() {
            return;
        }
        match test.builtin() {
            Some(builtin) if supported_test(builtin, test.subject) => {}
            _ => {
                failure = Some(UnsupportedFilterTest {
                    test: test.name.clone(),
                    subject: test.subject.name(),
                })
            }
        }
    });
    failure.map_or(Ok(()), Err)
}

/// The well-known lifecycle events, resolved once per archetype.
pub struct LifecycleEvents {
    /// Fired for summoned, spawn-egg and naturally spawned mobs.
    pub spawned: Option<EventHandle>,
    /// Fired for babies produced by breeding.
    pub born: Option<EventHandle>,
}

pub struct MobArchetype {
    pub identifier: Arc<str>,
    /// The native actor class: `runtime_identifier` when the definition
    /// sets one, else the identifier.
    pub actor_type: ActorType,
    /// Native class behaviour of [`Self::actor_type`].
    pub native: NativeTraits,
    pub source: Arc<ActorDefinition>,
    pub definition: Arc<CompiledDefinition>,
    pub events: CompiledEvents,
    pub lifecycle: LifecycleEvents,
    pub properties: Arc<PropertyLayout>,
    pub attributes: Arc<AttributeLayout>,
    /// Initial wire triples in layout order (see [`attribute_seed`]).
    pub attribute_seed: Box<[AttributeValue]>,
    pub health_attribute: Option<AttributeId>,
    pub movement_attribute: Option<AttributeId>,
    /// Indexed by [`FilterBinding`].
    pub filters: Box<[Filter]>,
    /// The definition predates the 1.26.20 friction change; BDS marks such
    /// actors with `USES_LEGACY_FRICTION` (observed on the native cow summon,
    /// whose 1.26.10 definition does not declare the component).
    pub legacy_friction: bool,
    /// The definition's base components include `minecraft:projectile`: the
    /// actor flies as a projectile instead of living physics and AI.
    pub projectile: bool,
}

impl MobArchetype {
    fn compile(
        source: &Arc<ActorDefinition>,
        kinds: &Arc<ComponentKinds>,
        properties: &ActorPropertyRegistry,
    ) -> Result<Self, ArchetypeError> {
        let identifier = source.identifier();
        let runtime_identifier = source
            .description
            .runtime_identifier
            .as_deref()
            .unwrap_or(identifier);
        let actor_type = ActorType::from_name(runtime_identifier)
            .ok_or_else(|| ArchetypeError::NotNativeActor(runtime_identifier.to_owned()))?;
        let definition = CompiledDefinition::compile(source, Arc::clone(kinds))?;
        let projectile_name = ActorComponent::Projectile.name();
        let projectile = source.components.contains_key(projectile_name);
        if !projectile
            && source
                .component_groups
                .values()
                .any(|group| group.contains_key(projectile_name))
        {
            return Err(ArchetypeError::ProjectileInGroup);
        }
        let layout = match properties.get(identifier) {
            Some(layout) => Arc::clone(layout),
            None => Arc::new(PropertyLayout::compile(&source.description)?),
        };
        let mut bindings = MobBindings::default();
        let events = CompiledEvents::compile(
            source,
            &definition,
            &layout,
            QueryRegistry::vanilla(),
            &mut bindings,
        )?;
        let (attributes, attribute_seed) = attribute_seed(identifier)?;
        let attributes = Arc::new(attributes);
        Ok(Self {
            identifier: Arc::from(identifier),
            actor_type,
            native: NativeTraits::of(actor_type),
            source: Arc::clone(source),
            lifecycle: LifecycleEvents {
                spawned: events.event("minecraft:entity_spawned"),
                born: events.event("minecraft:entity_born"),
            },
            definition,
            events,
            properties: layout,
            health_attribute: attributes.resolve("minecraft:health"),
            movement_attribute: attributes.resolve("minecraft:movement"),
            attributes,
            attribute_seed,
            filters: bindings.filters.into_boxed_slice(),
            legacy_friction: format_before(&source.format_version, [1, 26, 20]),
            projectile,
        })
    }
}

/// AddActor attribute order and bounds from the native summon of this
/// identifier. Current values are re-derived from the definition
/// (`minecraft:health`, `minecraft:movement`) after composition; the other
/// rows keep the observed value, which matches their constructor state
/// (absorption 0, luck 0, modifiers 1, bounciness 0).
fn attribute_seed(
    identifier: &str,
) -> Result<(AttributeLayout, Box<[AttributeValue]>), ArchetypeError> {
    let observed = unastar_data::actor_wire::get(identifier)
        .ok_or_else(|| ArchetypeError::NoAttributeObservation(identifier.to_owned()))?;
    let layout = AttributeLayout::new(
        observed
            .attributes
            .iter()
            .map(|row| Arc::<str>::from(row.name.as_str())),
    )?;
    let values = observed
        .attributes
        .iter()
        .map(|row| AttributeValue {
            min: row.min,
            current: row.observed_current,
            max: row.max,
        })
        .collect();
    Ok((layout, values))
}

fn format_before(version: &str, limit: [u32; 3]) -> bool {
    let mut parts = version.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let parsed = [
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    ];
    parsed < limit
}

/// All compiled mob archetypes plus the reasons other definitions are not
/// spawnable yet.
#[derive(Resource, Clone)]
pub struct MobRegistry {
    archetypes: Arc<BTreeMap<Arc<str>, Arc<MobArchetype>>>,
    unsupported: Arc<BTreeMap<Arc<str>, Arc<ArchetypeError>>>,
    families: Arc<FamilyRegistry>,
}

impl MobRegistry {
    pub fn compile(
        definitions: &ActorDefinitionRegistry,
        properties: &ActorPropertyRegistry,
        items: &Arc<ItemRegistry>,
    ) -> Result<Self, MobRegistryError> {
        let mut family_builder = FamilyRegistryBuilder::default();
        for definition in definitions.iter() {
            family_builder.register_definition(definition)?;
        }
        let families = family_builder.freeze();
        let kinds = component_kinds(&families, items)?;
        let mut archetypes = BTreeMap::new();
        let mut unsupported = BTreeMap::new();
        for source in definitions.iter() {
            let id: Arc<str> = Arc::from(source.identifier());
            match MobArchetype::compile(source, &kinds, properties) {
                Ok(archetype) => {
                    archetypes.insert(id, Arc::new(archetype));
                }
                Err(reason) => {
                    unsupported.insert(id, Arc::new(reason));
                }
            }
        }
        // What a projectile spawns on hit must itself be spawnable; checked
        // once every archetype is compiled.
        let broken: Vec<(Arc<str>, ArchetypeError)> = archetypes
            .iter()
            .filter(|(_, archetype)| archetype.projectile)
            .filter_map(|(id, archetype)| {
                projectile::check_spawns(archetype, items, |name| {
                    archetypes.get(name).map(Arc::clone)
                })
                .err()
                .map(|error| (Arc::clone(id), error.into()))
            })
            .collect();
        for (id, reason) in broken {
            archetypes.remove(&id);
            unsupported.insert(id, Arc::new(reason));
        }
        Ok(Self {
            archetypes: Arc::new(archetypes),
            unsupported: Arc::new(unsupported),
            families,
        })
    }

    pub fn get(&self, identifier: &str) -> Option<&Arc<MobArchetype>> {
        self.archetypes.get(identifier)
    }

    /// Why a known definition cannot be spawned, if it cannot.
    pub fn unsupported(&self, identifier: &str) -> Option<&Arc<ArchetypeError>> {
        self.unsupported.get(identifier)
    }

    /// [`Self::unsupported`], rendered for messages.
    pub fn unsupported_reason(&self, identifier: &str) -> Option<String> {
        self.unsupported(identifier).map(ToString::to_string)
    }

    pub fn families(&self) -> &Arc<FamilyRegistry> {
        &self.families
    }

    pub fn identifiers(&self) -> impl Iterator<Item = &str> {
        self.archetypes.keys().map(AsRef::as_ref)
    }
}
