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
    Definition { property: Arc<str>, reason: String },
    #[error(transparent)]
    Evaluation(#[from] eval::EvalError),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PropertyValue {
    Bool(bool),
    Int(i32),
    Float(f32),
    Enum(i32),
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
                        .ok_or_else(|| error(&name, "missing default"))?;
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
                .ok_or_else(|| {
                    error(
                        &slot.name,
                        format!("no measured protocol2193 descriptor code for {type_name}"),
                    )
                })?;
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
        bytes.extend(
            encode_network_fields(&fields)
                .map_err(|cause| error(&Arc::from(identifier), cause.to_string()))?,
        );
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
            .ok_or_else(|| error(&Arc::from(name), "not declared"))?;
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
                "slot belongs to another property layout",
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
                "assignment belongs to another property layout",
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
}

fn error(name: &Arc<str>, reason: impl Into<String>) -> PropertyError {
    PropertyError::Definition {
        property: Arc::clone(name),
        reason: reason.into(),
    }
}

fn validate_kind(name: &Arc<str>, kind: &PropertyKind) -> Result<(), PropertyError> {
    match kind {
        PropertyKind::Int { min, max }
            if min > max || i32::try_from(*min).is_err() || i32::try_from(*max).is_err() =>
        {
            Err(error(
                name,
                "integer range does not fit an ordered wire Int32 range",
            ))
        }
        PropertyKind::Float { min, max }
            if min > max || !(*min as f32).is_finite() || !(*max as f32).is_finite() =>
        {
            Err(error(name, "float range is not ordered and finite"))
        }
        PropertyKind::Enum { values }
            if values.is_empty() || i32::try_from(values.len()).is_err() =>
        {
            Err(error(
                name,
                "enum needs a nonempty Int32-indexed value list",
            ))
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
            .ok_or_else(|| error(name, "enum value is not a literal name"))?;
        let index = values
            .iter()
            .position(|value| value == label)
            .ok_or_else(|| error(name, format!("unknown enum value {label:?}")))?;
        return Ok(Source::Literal(PropertyValue::Enum(index as i32)));
    }
    if let Value::String(source) = value {
        let mut expression = molang::parse(source).map_err(|e| error(name, e.to_string()))?;
        expression
            .bind(queries, context)
            .map_err(|e| error(name, e))?;
        return Ok(Source::Expression(expression));
    }
    let number = match value {
        Value::Bool(value) => f64::from(u8::from(*value)),
        Value::Number(value) => value
            .as_f64()
            .ok_or_else(|| error(name, "invalid numeric value"))?,
        _ => {
            return Err(error(
                name,
                "expected a number, boolean or Molang expression",
            ));
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
        return Err(error(name, "non-finite numeric value"));
    }
    Ok(match kind {
        PropertyKind::Bool => PropertyValue::Bool(value != 0.0),
        PropertyKind::Int { min, max } => {
            PropertyValue::Int(value.trunc().clamp(*min as f64, *max as f64) as i32)
        }
        PropertyKind::Float { min, max } => PropertyValue::Float(value.clamp(*min, *max) as f32),
        PropertyKind::Enum { .. } => {
            return Err(error(name, "enum does not accept numeric expressions"));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use jolyne::valentine::bedrock::codec::{BedrockCodec, VarUInt};
    use serde_json::json;
    use unastar_packs::actor::{Loaded, load_value};
    use unastar_packs::nbt::{Encoding, read_compound};
    use unastar_rules::molang::QueryId;
    use unastar_rules::molang::eval::Value as MolangValue;

    struct LiteralOnly;
    impl MolangContext for LiteralOnly {
        fn query(&mut self, _: QueryId, _: &[MolangValue]) -> Option<MolangValue> {
            panic!("literal control queried context")
        }
        fn variable(&self, _: &str) -> Option<MolangValue> {
            panic!("literal control read variable")
        }
        fn set_variable(&mut self, _: &str, _: MolangValue) {
            panic!("literal control wrote variable")
        }
        fn context(&self, _: &str) -> Option<MolangValue> {
            panic!("literal control read context")
        }
        fn this(&self) -> MolangValue {
            panic!("literal control read this")
        }
        fn enter(&mut self, _: u64) -> bool {
            panic!("literal control switched actor")
        }
        fn leave(&mut self) {
            panic!("literal control left actor")
        }
        fn random(&mut self) -> f32 {
            panic!("literal control requested randomness")
        }
    }

    fn layout(properties: Value) -> Arc<PropertyLayout> {
        let document = json!({
            "format_version": "1.26.50",
            "minecraft:entity": {"description": {
                "identifier": "test:properties", "is_summonable": true, "properties": properties
            }}
        });
        let Loaded::Definition { definition, .. } = load_value(
            "property control",
            &document,
            unastar_data::actor_definitions::parser(),
        )
        .unwrap() else {
            panic!("property control skipped")
        };
        Arc::new(PropertyLayout::compile(&definition.description).unwrap())
    }

    #[test]
    fn foreign_assignment_and_slot_cannot_change_another_layouts_value_type() {
        let first = layout(json!({"addon:value": {
            "type": "int", "range": [-10, 10], "default": 0, "client_sync": true
        }}));
        let second = layout(json!({"addon:value": {
            "type": "bool", "default": false, "client_sync": true
        }}));
        let assignment = first
            .compile_assignment("addon:value", &json!(7), QueryRegistry::vanilla())
            .unwrap();
        let mut state = ActorProperties::new(Arc::clone(&second), &mut LiteralOnly).unwrap();
        assert!(state.apply(&assignment, &mut LiteralOnly).is_err());
        assert!(state.value(&first.slot("addon:value").unwrap()).is_err());
        assert_eq!(
            state.value(&second.slot("addon:value").unwrap()).unwrap(),
            PropertyValue::Bool(false)
        );
        assert_eq!(state.revision(), 0);
        assert_eq!(state.sync_revision(), 0);
    }

    #[test]
    fn server_only_changes_do_not_dirty_the_client_property_snapshot() {
        let layout = layout(json!({
            "addon:hidden": {"type": "bool", "default": false},
            "addon:visible": {"type": "int", "range": [-10, 10], "default": 0, "client_sync": true}
        }));
        let mut state = ActorProperties::new(Arc::clone(&layout), &mut LiteralOnly).unwrap();
        let hidden = layout
            .compile_assignment("addon:hidden", &json!(true), QueryRegistry::vanilla())
            .unwrap();
        state.apply(&hidden, &mut LiteralOnly).unwrap();
        assert_eq!(state.revision(), 1);
        assert_eq!(state.sync_revision(), 0);
        let mut snapshot = PropertySyncData::default();
        state.copy_synced(&mut snapshot);
        assert_eq!(snapshot.int_entries_list.len(), 1);
        assert_eq!(snapshot.int_entries_list[0].data, 0);
        let visible = layout
            .compile_assignment("addon:visible", &json!(7), QueryRegistry::vanilla())
            .unwrap();
        state.apply(&visible, &mut LiteralOnly).unwrap();
        assert_eq!(state.sync_revision(), 1);
        state.copy_synced(&mut snapshot);
        assert_eq!(snapshot.int_entries_list[0].data, 7);
        assert!(!state.apply(&visible, &mut LiteralOnly).unwrap());
        assert_eq!(state.sync_revision(), 1);
    }

    #[test]
    fn vanilla_descriptors_equal_all_retained_native_property_schemas() {
        let registry = ActorPropertyRegistry::compile(&ActorDefinitionRegistry::vanilla()).unwrap();
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../bds-extractor/captures/1.26.51/member");
        let sequence: Vec<Value> =
            serde_json::from_slice(&std::fs::read(root.join("join-sequence.json")).unwrap())
                .unwrap();
        let expected: Vec<_> = sequence
            .iter()
            .filter(|row| row["packet"] == "SyncActorPropertyPacket")
            .map(|row| {
                let seq = row["seq"].as_u64().unwrap();
                let bytes =
                    std::fs::read(root.join(format!("join/{seq:05}-SyncActorPropertyPacket.bin")))
                        .unwrap();
                let mut input = bytes.as_slice();
                let _header = VarUInt::decode(&mut input, ()).unwrap();
                let packet = SyncActorPropertyPacket::decode(&mut input, ()).unwrap();
                assert!(input.is_empty());
                read_compound(
                    "native property schema",
                    &packet.property_data.0,
                    Encoding::NetworkLittleEndian,
                )
                .unwrap()
            })
            .collect();
        let actual: Vec<_> = registry
            .packets()
            .iter()
            .map(|packet| {
                read_compound(
                    "compiled property schema",
                    &packet.property_data.0,
                    Encoding::NetworkLittleEndian,
                )
                .unwrap()
            })
            .collect();
        assert_eq!(actual, expected);
    }
}
