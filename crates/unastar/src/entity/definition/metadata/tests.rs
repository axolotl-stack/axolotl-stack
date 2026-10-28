use super::*;
use jolyne::valentine::AddActorPacket;
use valentine::bedrock::codec::BedrockCodec;

#[test]
fn changes_coalesce_without_mutating_an_earlier_snapshot() {
    let mut metadata = EntityMetadata::with_capacity(3);
    metadata.set_int(9, 10);
    metadata.set_long(u32::MAX, -1);
    metadata.set_string(2, "before");
    let earlier = metadata.snapshot();
    metadata.mark_clean();
    assert!(!metadata.is_dirty());

    assert!(metadata.set_int(9, 11));
    assert!(metadata.set_int(9, 12));
    assert!(!metadata.set_int(9, 12));
    assert!(metadata.set_string(2, "after"));
    let revision = metadata.revision();
    let mut changes = earlier.clone();
    metadata.copy_changes(&mut changes);
    assert_eq!(
        changes
            .data
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        [2, 9]
    );
    assert_eq!(
        changes.data[0].payload,
        DataItemEntryPayload::DataItemStringPayload(DataItemStringPayload {
            type_: EnumsDataItemType::String,
            value: "after".into(),
        })
    );
    assert_eq!(
        changes.data[1].payload,
        DataItemEntryPayload::DataItemIntPayload(DataItemIntPayload {
            type_: EnumsDataItemType::Int,
            value: 12,
        })
    );
    assert_ne!(earlier.data[0].payload, changes.data[0].payload);
    assert_eq!(metadata.get_long(u32::MAX), Some(-1));
    assert!(
        metadata.is_dirty(),
        "copying is not a delivery acknowledgment"
    );
    metadata.mark_clean();
    metadata.copy_changes(&mut changes);
    assert!(changes.data.is_empty());
    assert_eq!(metadata.revision(), revision);
}

#[test]
fn repeated_input_keys_keep_the_last_value_and_one_wire_entry() {
    let mut source = EntityMetadata::new();
    source.set_int(4, 1);
    let mut entries = source.into_data().data;
    source = EntityMetadata::new();
    source.set_int(4, 2);
    entries.extend(source.into_data().data);
    let metadata = EntityMetadata::from_entries(entries);
    assert_eq!(
        metadata.into_data().data,
        vec![DataItemEntry {
            id: 4,
            payload: DataItemEntryPayload::DataItemIntPayload(DataItemIntPayload {
                type_: EnumsDataItemType::Int,
                value: 2,
            }),
        }]
    );
}

#[test]
fn floating_wire_changes_distinguish_signed_zero_and_nan_payloads() {
    let mut metadata = EntityMetadata::new();
    let nan = f32::from_bits(0x7fc0_0001);
    assert!(metadata.set_float(1, nan));
    metadata.mark_clean();
    assert!(!metadata.set_float(1, nan));
    assert!(!metadata.is_dirty());
    assert!(metadata.set_float(1, f32::from_bits(0x7fc0_0002)));
    assert!(metadata.set_float(1, 0.0));
    assert!(metadata.set_float(1, -0.0));
    let Some(DataItemEntryPayload::DataItemFloatPayload(scalar)) = metadata.get(1) else {
        panic!("missing float payload");
    };
    assert_eq!(scalar.value.to_bits(), (-0.0_f32).to_bits());

    let vector = |z| {
        DataItemEntryPayload::DataItemVec3Payload(jolyne::valentine::types::DataItemVec3Payload {
            type_: EnumsDataItemType::Vec3,
            value: jolyne::valentine::types::Vec3 { x: nan, y: 0.0, z },
        })
    };
    assert!(metadata.set_payload(2, vector(0.0)));
    metadata.mark_clean();
    assert!(!metadata.set_payload(2, vector(0.0)));
    assert!(!metadata.is_dirty());
    assert!(metadata.set_payload(2, vector(-0.0)));
    let Some(DataItemEntryPayload::DataItemVec3Payload(vector)) = metadata.get(2) else {
        panic!("missing vector payload");
    };
    assert_eq!(vector.value.x.to_bits(), nan.to_bits());
    assert_eq!(vector.value.z.to_bits(), (-0.0_f32).to_bits());
}
#[test]
fn leaving_player_states_clears_owned_flags_without_clearing_physics() {
    let active = PlayerState {
        sneaking: true,
        sprinting: true,
        swimming: true,
        gliding: true,
        flying: true,
    };
    let metadata = EntityMetadata::from_physics(true, true, None)
        .with_player_state(&active)
        .with_player_state(&PlayerState::default());
    let actual = metadata.get_long(actor_data_ids::RESERVED_0).unwrap() as u64;
    let player = (1u64 << actor_flags::SNEAKING)
        | (1u64 << actor_flags::SPRINTING)
        | (1u64 << actor_flags::SWIMMING)
        | (1u64 << actor_flags::GLIDING)
        | (1u64 << actor_flags::CANFLY);
    let physics = (1u64 << actor_flags::HAS_COLLISION) | (1u64 << actor_flags::HAS_GRAVITY);
    assert_eq!(actual & player, 0);
    assert_eq!(actual & physics, physics);
}

#[test]
fn living_metadata_matches_native_cow_control() {
    assert_living_metadata_control(
        "minecraft:cow",
        include_bytes!("../../../../../bds-extractor/captures/1.26.51/operator/summons/cow.bin"),
    );
}

#[test]
fn living_metadata_matches_native_chicken_control() {
    assert_living_metadata_control(
        "minecraft:chicken",
        include_bytes!(
            "../../../../../bds-extractor/captures/1.26.51/operator/summons/chicken.bin"
        ),
    );
}

fn assert_living_metadata_control(identifier: &str, bytes: &[u8]) {
    assert_eq!(bytes[0], 0x0d);
    let mut input = &bytes[1..];
    let native = AddActorPacket::decode(&mut input, ()).unwrap();
    assert!(input.is_empty());
    let definition = unastar_data::actor_definitions::get(identifier).unwrap();
    let collision = &definition.components["minecraft:collision_box"];
    let body = PhysicsBody::living(
        collision["width"].as_f64().unwrap(),
        collision["height"].as_f64().unwrap(),
    );
    let mut metadata =
        EntityMetadata::from_physics(true, true, None).with_air_supply(&AirSupply::default());
    metadata.set_float(actor_data_ids::RESERVED_038, 1.0);
    metadata.set_float(actor_data_ids::RESERVED_053, body.width as f32);
    metadata.set_float(actor_data_ids::RESERVED_054, body.height as f32);
    let actual = metadata;
    for id in [7, 38, 42, 53, 54] {
        let expected = &native
            .actor_data
            .data
            .iter()
            .find(|row| row.id == id)
            .unwrap()
            .payload;
        assert_eq!(actual.get(id), Some(expected), "native metadata {id}");
    }
    assert!(
        actual.get(1).is_none(),
        "living health is an attribute, not actor-data structural integrity"
    );
    let Some(DataItemEntryPayload::DataItemInt64Payload(actual_flags)) = actual.get(0) else {
        panic!("missing actor flags");
    };
    let DataItemEntryPayload::DataItemInt64Payload(expected_flags) = &native
        .actor_data
        .data
        .iter()
        .find(|row| row.id == 0)
        .unwrap()
        .payload
    else {
        panic!("native actor flags are not Int64");
    };
    let physics_mask = (1u64 << 35) | (1u64 << 47) | (1u64 << 48) | (1u64 << 49);
    assert_eq!(
        actual_flags.value as u64 & physics_mask,
        expected_flags.value as u64 & physics_mask,
        "dry living physics/breathing flags"
    );
}
