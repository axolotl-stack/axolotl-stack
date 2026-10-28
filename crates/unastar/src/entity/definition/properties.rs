//! Actor property schemas and state compiled from pack descriptions.
//! Assignment targets resolve to scoped slots at startup; applying an assignment
//! does not parse JSON. Molang context bindings are supplied by the caller.

use std::collections::BTreeMap;
use std::sync::Arc;

use bevy_ecs::prelude::{Component, Resource};
use bytes::Bytes;
use jolyne::valentine::SyncActorPropertyPacket;
use jolyne::valentine::bedrock::codec::Nbt;
use jolyne::valentine::types::{
    PropertySyncData, PropertySyncDataPropertySyncFloatEntry, PropertySyncDataPropertySyncIntEntry,
};
use serde_json::Value;
use unastar_data::actor_definitions::{Description, PropertyKind};
use unastar_packs::nbt::{Tag, encode_network_fields};
use unastar_rules::molang::eval::{self, MolangContext};
use unastar_rules::molang::{self, Expr, QueryContext, QueryRegistry};

use crate::registry::entity::ActorDefinitionRegistry;

#[derive(Debug, thiserror::Error)]
pub enum PropertyError {
    #[error("actor property {property}: {reason}")]
    Definition {
        property: Arc<str>,
        reason: PropertyReason,
    },
    #[error(transparent)]
    Evaluation(#[from] eval::EvalError),
}

/// What is wrong with one property, its value or its handle.
#[derive(Debug, thiserror::Error)]
pub enum PropertyReason {
    #[error("missing default")]
    MissingDefault,
    #[error("no measured protocol2193 descriptor code for {0}")]
    NoDescriptorCode(&'static str),
    /// The NBT encoder's report on the sync descriptor.
    #[error("{0}")]
    Descriptor(String),
    #[error("not declared")]
    NotDeclared,
    #[error("slot belongs to another property layout")]
    ForeignSlot,
    #[error("assignment belongs to another property layout")]
    ForeignAssignment,
    #[error("unknown enum value {0:?}")]
    UnknownEnumValue(String),
    #[error("{0} does not match the declared kind")]
    KindMismatch(String),
    #[error("integer range does not fit an ordered wire Int32 range")]
    IntRange,
    #[error("float range is not ordered and finite")]
    FloatRange,
    #[error("enum needs a nonempty Int32-indexed value list")]
    EnumValues,
    #[error("enum value is not a literal name")]
    EnumNotLiteral,
    /// The Molang parser's or binder's report.
    #[error("{0}")]
    Molang(String),
    #[error("invalid numeric value")]
    InvalidNumber,
    #[error("expected a number, boolean or Molang expression")]
    UnexpectedValue,
    #[error("non-finite numeric value")]
    NonFinite,
    #[error("enum does not accept numeric expressions")]
    EnumNumeric,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PropertyValue {
    Bool(bool),
    Int(i32),
    Float(f32),
    Enum(i32),
}

/// A property value with enum values by label: how actors persist them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NamedPropertyValue<'a> {
    Bool(bool),
    Int(i32),
    Float(f32),
    Enum(&'a str),
}

#[derive(Debug, Clone)]
enum Source {
    Literal(PropertyValue),
    Expression(Expr),
}

#[derive(Debug)]
struct Slot {
    name: Arc<str>,
    kind: PropertyKind,
    initial: Source,
    sync_index: Option<u32>,
}

/// A frozen layout shared by instances of an actor definition. Server-only
/// properties retain storage but have no client-sync slot.
#[derive(Debug)]
pub struct PropertyLayout {
    slots: Box<[Slot]>,
}

/// An opaque slot in exactly one frozen property layout.
#[derive(Debug, Clone)]
pub struct PropertySlot {
    owner: Arc<PropertyLayout>,
    index: usize,
}

#[derive(Debug, Clone)]
pub struct PropertyAssignment {
    property: PropertySlot,
    source: Source,
}

impl PropertyLayout {
    pub fn compile(description: &Description) -> Result<Self, PropertyError> {
        let mut slots = Vec::new();
        let mut int_index = 0;
        let mut float_index = 0;
        if let Some(properties) = description.active_properties() {
            slots.reserve(properties.len());
            for (name, definition) in properties {
                let name: Arc<str> = Arc::from(name.as_str());
                validate_kind(&name, &definition.kind)?;
                let initial = if let Some(expression) = &definition.default_expression {
                    Source::Expression(expression.clone())
                } else {
                    let value = definition
                        .default
                        .as_ref()
                        .ok_or_else(|| error(&name, PropertyReason::MissingDefault))?;
                    compile_source(
                        &name,
                        &definition.kind,
                        value,
                        QueryContext::PropertyDefault,
                        QueryRegistry::vanilla(),
                    )?
                };
                let sync_index = definition.client_sync.then(|| {
                    let next = if matches!(definition.kind, PropertyKind::Float { .. }) {
                        &mut float_index
                    } else {
                        &mut int_index
                    };
                    let index = *next;
                    *next += 1;
                    index
                });
                slots.push(Slot {
                    name,
                    kind: definition.kind.clone(),
                    initial,
                    sync_index,
                });
            }
        }
        Ok(Self {
            slots: slots.into_boxed_slice(),
        })
    }

    /// Build the frozen join descriptor once. Actors without client-synced
    /// properties send no descriptor; values travel separately in actor data.
    pub fn sync_packet(
        &self,
        identifier: &str,
    ) -> Result<Option<SyncActorPropertyPacket>, PropertyError> {
        let mut properties = Vec::new();
        for slot in self.slots.iter().filter(|slot| slot.sync_index.is_some()) {
            let mut fields =
                BTreeMap::from([("name".to_owned(), Tag::String(slot.name.to_string()))]);
            let type_name = match &slot.kind {
                PropertyKind::Int { min, max } => {
                    fields.insert("min".to_owned(), Tag::Int(*min as i32));
                    fields.insert("max".to_owned(), Tag::Int(*max as i32));
                    "int"
                }
                PropertyKind::Float { min, max } => {
                    fields.insert("min".to_owned(), Tag::Float(*min as f32));
                    fields.insert("max".to_owned(), Tag::Float(*max as f32));
                    "float"
                }
                PropertyKind::Bool => "bool",
                PropertyKind::Enum { values } => {
                    fields.insert(
                        "enum".to_owned(),
                        Tag::List(values.iter().cloned().map(Tag::String).collect()),
                    );
                    "enum"
                }
            };
            let type_id = *unastar_data::actor_wire::vanilla()
                .property_types
                .get(type_name)
                .ok_or_else(|| error(&slot.name, PropertyReason::NoDescriptorCode(type_name)))?;
            fields.insert("type".to_owned(), Tag::Int(type_id));
            properties.push(Tag::Compound(fields));
        }
        if properties.is_empty() {
            return Ok(None);
        }
        let fields = BTreeMap::from([
            ("properties".to_owned(), Tag::List(properties)),
            ("type".to_owned(), Tag::String(identifier.to_owned())),
        ]);
        let mut bytes = vec![10, 0];
        bytes.extend(encode_network_fields(&fields).map_err(|cause| {
            error(
                &Arc::from(identifier),
                PropertyReason::Descriptor(cause.to_string()),
            )
        })?);
        bytes.push(0);
        Ok(Some(SyncActorPropertyPacket {
            property_data: Nbt(Bytes::from(bytes)),
        }))
    }

    /// Resolve only when compiling a definition/event or handling a command.
    pub fn slot(self: &Arc<Self>, name: &str) -> Option<PropertySlot> {
        let index = self
            .slots
            .binary_search_by(|slot| slot.name.as_ref().cmp(name))
            .ok()?;
        Some(PropertySlot {
            owner: Arc::clone(self),
            index,
        })
    }

    pub fn compile_assignment(
        self: &Arc<Self>,
        name: &str,
        value: &Value,
        queries: &QueryRegistry,
    ) -> Result<PropertyAssignment, PropertyError> {
        let index = self
            .slot(name)
            .ok_or_else(|| error(&Arc::from(name), PropertyReason::NotDeclared))?;
        let slot = &self.slots[index.index];
        Ok(PropertyAssignment {
            property: index,
            source: compile_source(
                &slot.name,
                &slot.kind,
                value,
                QueryContext::Runtime,
                queries,
            )?,
        })
    }

    pub fn enum_value(&self, slot: &PropertySlot, value: PropertyValue) -> Option<&str> {
        if !std::ptr::eq(Arc::as_ptr(&slot.owner), self) {
            return None;
        }
        match (&self.slots[slot.index].kind, value) {
            (PropertyKind::Enum { values }, PropertyValue::Enum(index)) => {
                values.get(usize::try_from(index).ok()?).map(String::as_str)
            }
            _ => None,
        }
    }
}

/// Frozen schemas and cached join descriptors. Built-in packet order comes
/// from the retained native sequence; addon-only rows follow identifier order.
#[derive(Debug, Resource)]
pub struct ActorPropertyRegistry {
    layouts: BTreeMap<Arc<str>, Arc<PropertyLayout>>,
    packets: Vec<SyncActorPropertyPacket>,
}

impl ActorPropertyRegistry {
    pub fn compile(definitions: &ActorDefinitionRegistry) -> Result<Self, PropertyError> {
        let mut layouts = BTreeMap::new();
        let mut descriptors = BTreeMap::new();
        for definition in definitions.iter() {
            let layout = Arc::new(PropertyLayout::compile(&definition.description)?);
            if layout.slots.is_empty() {
                continue;
            }
            if let Some(packet) = layout.sync_packet(definition.identifier())? {
                descriptors.insert(definition.identifier().to_owned(), packet);
            }
            layouts.insert(Arc::from(definition.identifier()), layout);
        }
        let mut packets = Vec::with_capacity(descriptors.len());
        for identifier in &unastar_data::actor_wire::vanilla().property_descriptor_order {
            if let Some(packet) = descriptors.remove(identifier) {
                packets.push(packet);
            }
        }
        packets.extend(descriptors.into_values());
        Ok(Self { layouts, packets })
    }

    /// Cold lookup for actor/command/event compilation, never a tick lookup.
    pub fn get(&self, identifier: &str) -> Option<&Arc<PropertyLayout>> {
        self.layouts.get(identifier)
    }

    pub fn packets(&self) -> &[SyncActorPropertyPacket] {
        &self.packets
    }
}

#[derive(Component, Debug)]
pub struct ActorProperties {
    layout: Arc<PropertyLayout>,
    values: Box<[PropertyValue]>,
    revision: u64,
    sync_revision: u64,
}

impl ActorProperties {
    pub fn new(
        layout: Arc<PropertyLayout>,
        context: &mut dyn MolangContext,
    ) -> Result<Self, PropertyError> {
        let mut values = Vec::with_capacity(layout.slots.len());
        for slot in &layout.slots {
            values.push(evaluate(&slot.name, &slot.kind, &slot.initial, context)?);
        }
        Ok(Self {
            layout,
            values: values.into_boxed_slice(),
            revision: 0,
            sync_revision: 0,
        })
    }

    pub fn value(&self, slot: &PropertySlot) -> Result<PropertyValue, PropertyError> {
        if !Arc::ptr_eq(&slot.owner, &self.layout) {
            return Err(error(
                &slot.owner.slots[slot.index].name,
                PropertyReason::ForeignSlot,
            ));
        }
        Ok(self.values[slot.index])
    }

    pub fn layout(&self) -> &Arc<PropertyLayout> {
        &self.layout
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn sync_revision(&self) -> u64 {
        self.sync_revision
    }

    pub fn apply(
        &mut self,
        assignment: &PropertyAssignment,
        context: &mut dyn MolangContext,
    ) -> Result<bool, PropertyError> {
        if !Arc::ptr_eq(&assignment.property.owner, &self.layout) {
            return Err(error(
                &assignment.property.owner.slots[assignment.property.index].name,
                PropertyReason::ForeignAssignment,
            ));
        }
        let index = assignment.property.index;
        let slot = &self.layout.slots[index];
        let value = evaluate(&slot.name, &slot.kind, &assignment.source, context)?;
        if value == self.values[index] {
            return Ok(false);
        }
        self.values[index] = value;
        self.revision = self.revision.wrapping_add(1);
        if slot.sync_index.is_some() {
            self.sync_revision = self.sync_revision.wrapping_add(1);
        }
        Ok(true)
    }

    /// Fill a full AddActor/SetActorData property snapshot, retaining buffers.
    pub fn copy_synced(&self, output: &mut PropertySyncData) {
        output.int_entries_list.clear();
        output.float_entries_list.clear();
        for (slot, value) in self.layout.slots.iter().zip(self.values.iter()) {
            let Some(property_index) = slot.sync_index else {
                continue;
            };
            match *value {
                PropertyValue::Float(data) => {
                    output
                        .float_entries_list
                        .push(PropertySyncDataPropertySyncFloatEntry {
                            property_index,
                            data,
                        })
                }
                value => {
                    let data = match value {
                        PropertyValue::Bool(value) => i32::from(value),
                        PropertyValue::Int(value) | PropertyValue::Enum(value) => value,
                        PropertyValue::Float(_) => unreachable!(),
                    };
                    output
                        .int_entries_list
                        .push(PropertySyncDataPropertySyncIntEntry {
                            property_index,
                            data,
                        });
                }
            }
        }
    }

    /// Every property by name with enum values by label, in layout order.
    pub fn named_values(&self) -> impl Iterator<Item = (&str, NamedPropertyValue<'_>)> + '_ {
        self.layout
            .slots
            .iter()
            .zip(self.values.iter())
            .map(|(slot, value)| {
                let named = match (*value, &slot.kind) {
                    (PropertyValue::Bool(value), _) => NamedPropertyValue::Bool(value),
                    (PropertyValue::Int(value), _) => NamedPropertyValue::Int(value),
                    (PropertyValue::Float(value), _) => NamedPropertyValue::Float(value),
                    (PropertyValue::Enum(index), PropertyKind::Enum { values }) => {
                        NamedPropertyValue::Enum(&values[index as usize])
                    }
                    (PropertyValue::Enum(_), _) => {
                        unreachable!("enum values only live in enum slots")
                    }
                };
                (&*slot.name, named)
            })
    }

    /// Set a property by name from a persisted value. Numbers clamp into the
    /// declared range; an undeclared name, a value of another kind or an
    /// unknown enum label is an error. Returns whether the value changed.
    pub fn set_named(
        &mut self,
        name: &str,
        value: NamedPropertyValue<'_>,
    ) -> Result<bool, PropertyError> {
        let index = self
            .layout
            .slots
            .binary_search_by(|slot| slot.name.as_ref().cmp(name))
            .map_err(|_| error(&Arc::from(name), PropertyReason::NotDeclared))?;
        let slot = &self.layout.slots[index];
        let value = match (&slot.kind, value) {
            (PropertyKind::Bool, NamedPropertyValue::Bool(value)) => PropertyValue::Bool(value),
            (PropertyKind::Int { .. }, NamedPropertyValue::Int(value)) => {
                coerce(&slot.name, &slot.kind, f64::from(value))?
            }
            (PropertyKind::Float { .. }, NamedPropertyValue::Float(value)) => {
                coerce(&slot.name, &slot.kind, f64::from(value))?
            }
            (PropertyKind::Enum { values }, NamedPropertyValue::Enum(label)) => {
                let index = values
                    .iter()
                    .position(|value| value == label)
                    .ok_or_else(|| {
                        error(
                            &slot.name,
                            PropertyReason::UnknownEnumValue(label.to_owned()),
                        )
                    })?;
                PropertyValue::Enum(index as i32)
            }
            (_, value) => {
                return Err(error(
                    &slot.name,
                    PropertyReason::KindMismatch(format!("{value:?}")),
                ));
            }
        };
        if value == self.values[index] {
            return Ok(false);
        }
        self.values[index] = value;
        self.revision = self.revision.wrapping_add(1);
        if slot.sync_index.is_some() {
            self.sync_revision = self.sync_revision.wrapping_add(1);
        }
        Ok(true)
    }
}

fn error(name: &Arc<str>, reason: PropertyReason) -> PropertyError {
    PropertyError::Definition {
        property: Arc::clone(name),
        reason,
    }
}

fn validate_kind(name: &Arc<str>, kind: &PropertyKind) -> Result<(), PropertyError> {
    match kind {
        PropertyKind::Int { min, max }
            if min > max || i32::try_from(*min).is_err() || i32::try_from(*max).is_err() =>
        {
            Err(error(name, PropertyReason::IntRange))
        }
        PropertyKind::Float { min, max }
            if min > max || !(*min as f32).is_finite() || !(*max as f32).is_finite() =>
        {
            Err(error(name, PropertyReason::FloatRange))
        }
        PropertyKind::Enum { values }
            if values.is_empty() || i32::try_from(values.len()).is_err() =>
        {
            Err(error(name, PropertyReason::EnumValues))
        }
        _ => Ok(()),
    }
}

fn compile_source(
    name: &Arc<str>,
    kind: &PropertyKind,
    value: &Value,
    context: QueryContext,
    queries: &QueryRegistry,
) -> Result<Source, PropertyError> {
    if let PropertyKind::Enum { values } = kind {
        let label = value
            .as_str()
            .ok_or_else(|| error(name, PropertyReason::EnumNotLiteral))?;
        let index = values
            .iter()
            .position(|value| value == label)
            .ok_or_else(|| error(name, PropertyReason::UnknownEnumValue(label.to_owned())))?;
        return Ok(Source::Literal(PropertyValue::Enum(index as i32)));
    }
    if let Value::String(source) = value {
        let mut expression =
            molang::parse(source).map_err(|e| error(name, PropertyReason::Molang(e)))?;
        expression
            .bind(queries, context)
            .map_err(|e| error(name, PropertyReason::Molang(e)))?;
        return Ok(Source::Expression(expression));
    }
    let number = match value {
        Value::Bool(value) => f64::from(u8::from(*value)),
        Value::Number(value) => value
            .as_f64()
            .ok_or_else(|| error(name, PropertyReason::InvalidNumber))?,
        _ => {
            return Err(error(name, PropertyReason::UnexpectedValue));
        }
    };
    Ok(Source::Literal(coerce(name, kind, number)?))
}

fn evaluate(
    name: &Arc<str>,
    kind: &PropertyKind,
    source: &Source,
    context: &mut dyn MolangContext,
) -> Result<PropertyValue, PropertyError> {
    match source {
        Source::Literal(value) => Ok(*value),
        Source::Expression(expression) => coerce(
            name,
            kind,
            f64::from(eval::evaluate_num(expression, context)?),
        ),
    }
}

fn coerce(
    name: &Arc<str>,
    kind: &PropertyKind,
    value: f64,
) -> Result<PropertyValue, PropertyError> {
    if !value.is_finite() {
        return Err(error(name, PropertyReason::NonFinite));
    }
    Ok(match kind {
        PropertyKind::Bool => PropertyValue::Bool(value != 0.0),
        PropertyKind::Int { min, max } => {
            PropertyValue::Int(value.trunc().clamp(*min as f64, *max as f64) as i32)
        }
        PropertyKind::Float { min, max } => PropertyValue::Float(value.clamp(*min, *max) as f32),
        PropertyKind::Enum { .. } => {
            return Err(error(name, PropertyReason::EnumNumeric));
        }
    })
}

#[cfg(test)]
mod tests;
