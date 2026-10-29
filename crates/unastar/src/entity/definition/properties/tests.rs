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
fn native_integer_snapshots_exclude_interleaved_server_only_properties() {
    let definitions = ActorDefinitionRegistry::vanilla();
    for (identifier, bytes, observed_enum) in [
        (
            "minecraft:wolf",
            include_bytes!(
                "../../../../../bds-extractor/captures/1.26.51/operator/summons/wolf.bin"
            )
            .as_slice(),
            Some(("minecraft:sound_variant", "cute")),
        ),
        (
            "minecraft:copper_golem",
            include_bytes!(
                "../../../../../bds-extractor/captures/1.26.51/operator/summons/copper_golem.bin"
            )
            .as_slice(),
            None,
        ),
    ] {
        let mut input = bytes;
        let _header = VarUInt::decode(&mut input, ()).unwrap();
        let native = jolyne::valentine::AddActorPacket::decode(&mut input, ()).unwrap();
        assert!(input.is_empty());
        let definition = definitions.get(identifier).unwrap();
        let layout = Arc::new(PropertyLayout::compile(&definition.description).unwrap());
        let mut state = ActorProperties::new(Arc::clone(&layout), &mut LiteralOnly).unwrap();
        // Reproduce the sampled enum, not a claim about the spawn default.
        if let Some((name, value)) = observed_enum {
            let assignment = layout
                .compile_assignment(name, &json!(value), QueryRegistry::vanilla())
                .unwrap();
            state.apply(&assignment, &mut LiteralOnly).unwrap();
        }
        for (name, _) in definition
            .description
            .active_properties()
            .unwrap()
            .iter()
            .filter(|(_, property)| !property.client_sync)
        {
            let assignment = layout
                .compile_assignment(name, &json!(true), QueryRegistry::vanilla())
                .unwrap();
            state.apply(&assignment, &mut LiteralOnly).unwrap();
        }
        let mut actual = PropertySyncData::default();
        state.copy_synced(&mut actual);
        assert_eq!(actual, native.synched_properties, "{identifier}");
    }
}

#[test]
fn vanilla_descriptors_equal_all_retained_native_property_schemas() {
    let registry = ActorPropertyRegistry::compile(&ActorDefinitionRegistry::vanilla()).unwrap();
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../bds-extractor/captures/1.26.51/member");
    let sequence: Vec<Value> =
        serde_json::from_slice(&std::fs::read(root.join("join-sequence.json")).unwrap()).unwrap();
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
