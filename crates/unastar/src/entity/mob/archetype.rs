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
use unastar_data::actor_definitions::ActorDefinition;
use unastar_rules::event::EventAction;
use unastar_rules::filter::{BuiltinTest, Filter, Subject};
use unastar_rules::molang::QueryRegistry;

use crate::entity::attributes::{AttributeId, AttributeLayout, AttributeValue};
use crate::entity::definition::{CompiledDefinition, ComponentKinds, ComponentKindsBuilder};
use crate::entity::events::{
    ActionBinding, CompiledEvents, EventBindings, EventHandle, ExternalEventBinding, FilterBinding,
};
use crate::entity::families::{FamilyRegistry, FamilyRegistryBuilder};
use crate::entity::properties::{ActorPropertyRegistry, PropertyLayout};
use crate::registry::entity::ActorDefinitionRegistry;

/// Every component kind a mob can use, frozen once for the process.
pub fn component_kinds(families: &Arc<FamilyRegistry>) -> Result<Arc<ComponentKinds>, String> {
    let mut builder = ComponentKindsBuilder::default();
    crate::entity::definition_components::register_component_kinds(&mut builder)?;
    crate::entity::flags::register_component_kinds(&mut builder)?;
    families.register_component_kind(&mut builder)?;
    super::config::register_component_kinds(&mut builder)?;
    Ok(builder.freeze())
}

/// Event filters the mob runtime can answer (see `super::events::MobFacts`).
/// Binding any other test fails the archetype instead of guessing a value.
fn supported_test(test: BuiltinTest, subject: Subject) -> bool {
    match test {
        BuiltinTest::HasBiomeTag
        | BuiltinTest::InLava
        | BuiltinTest::InWater
        | BuiltinTest::IsBaby
        | BuiltinTest::EnumProperty
        | BuiltinTest::BoolProperty
        | BuiltinTest::IntProperty
        | BuiltinTest::RiderCount => subject == Subject::Self_,
        BuiltinTest::IsFamily | BuiltinTest::HasEquipment => {
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
    fn filter(&mut self, filter: &Filter) -> Result<FilterBinding, String> {
        check_filter(filter)?;
        self.filters.push(filter.clone());
        Ok(FilterBinding((self.filters.len() - 1) as u32))
    }

    fn action(&mut self, action: EventAction, _: &Value) -> Result<ActionBinding, String> {
        Err(format!("event action {} is not implemented", action.key()))
    }

    fn external_event(
        &mut self,
        target: Subject,
        event: &str,
    ) -> Result<ExternalEventBinding, String> {
        Err(format!(
            "event {event} targeting {} is not implemented",
            target.name()
        ))
    }
}

/// Reject a filter containing any test the runtime cannot answer.
pub(super) fn check_filter(filter: &Filter) -> Result<(), String> {
    let mut failure = None;
    filter.for_each_test(&mut |test| {
        if failure.is_some() {
            return;
        }
        match test.builtin() {
            Some(builtin) if supported_test(builtin, test.subject) => {}
            _ => {
                failure = Some(format!(
                    "filter test {} on {} is not implemented",
                    test.name,
                    test.subject.name()
                ))
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
}

impl MobArchetype {
    fn compile(
        source: &Arc<ActorDefinition>,
        kinds: &Arc<ComponentKinds>,
        properties: &ActorPropertyRegistry,
    ) -> Result<Self, String> {
        let identifier = source.identifier();
        let definition = CompiledDefinition::compile(source, Arc::clone(kinds))?;
        let layout = match properties.get(identifier) {
            Some(layout) => Arc::clone(layout),
            None => {
                Arc::new(PropertyLayout::compile(&source.description).map_err(|e| e.to_string())?)
            }
        };
        let mut bindings = MobBindings::default();
        let events = CompiledEvents::compile(
            source,
            &definition,
            &layout,
            QueryRegistry::vanilla(),
            &mut bindings,
        )
        .map_err(|e| e.to_string())?;
        let (attributes, attribute_seed) = attribute_seed(identifier)?;
        let attributes = Arc::new(attributes);
        Ok(Self {
            identifier: Arc::from(identifier),
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
        })
    }
}

/// AddActor attribute order and bounds from the native summon of this
/// identifier. Current values are re-derived from the definition
/// (`minecraft:health`, `minecraft:movement`) after composition; the other
/// rows keep the observed value, which matches their constructor state
/// (absorption 0, luck 0, modifiers 1, bounciness 0).
fn attribute_seed(identifier: &str) -> Result<(AttributeLayout, Box<[AttributeValue]>), String> {
    let observed = unastar_data::actor_wire::get(identifier)
        .ok_or_else(|| format!("{identifier}: no native AddActor attribute observation"))?;
    let layout = AttributeLayout::new(
        observed
            .attributes
            .iter()
            .map(|row| Arc::<str>::from(row.name.as_str())),
    )
    .map_err(|e| e.to_string())?;
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
    unsupported: Arc<BTreeMap<Arc<str>, String>>,
    families: Arc<FamilyRegistry>,
}

impl MobRegistry {
    pub fn compile(
        definitions: &ActorDefinitionRegistry,
        properties: &ActorPropertyRegistry,
    ) -> Result<Self, String> {
        let mut family_builder = FamilyRegistryBuilder::default();
        for definition in definitions.iter() {
            family_builder.register_definition(definition)?;
        }
        let families = family_builder.freeze();
        let kinds = component_kinds(&families)?;
        let mut archetypes = BTreeMap::new();
        let mut unsupported = BTreeMap::new();
        for source in definitions.iter() {
            let id: Arc<str> = Arc::from(source.identifier());
            match MobArchetype::compile(source, &kinds, properties) {
                Ok(archetype) => {
                    archetypes.insert(id, Arc::new(archetype));
                }
                Err(reason) => {
                    unsupported.insert(id, reason);
                }
            }
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
    pub fn unsupported_reason(&self, identifier: &str) -> Option<&str> {
        self.unsupported.get(identifier).map(String::as_str)
    }

    pub fn families(&self) -> &Arc<FamilyRegistry> {
        &self.families
    }

    pub fn identifiers(&self) -> impl Iterator<Item = &str> {
        self.archetypes.keys().map(AsRef::as_ref)
    }
}
