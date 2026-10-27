//! Actor-data state using the generated protocol payloads directly.
//! Numeric updates retain sorted keys and coalesced dirty fields; constructor
//! defaults and metadata-key meanings remain the caller's explicit input.

use super::components::*;
use bevy_ecs::prelude::Component;
use jolyne::valentine::bedrock::codec::BedrockString;
use jolyne::valentine::types::{
    DataItemBytePayload, DataItemEntry, DataItemEntryPayload, DataItemFloatPayload,
    DataItemInt64Payload, DataItemIntPayload, DataItemShortPayload, DataItemStringPayload,
    EnumsDataItemType, SynchedActorDataCopyableDataList,
};
use jolyne::valentine::{actor_data_i_ds as actor_data_ids, actor_flags};

/// Per-actor wire values. No hand-maintained duplicate of the protocol union.
#[derive(Component, Debug, Default)]
pub struct EntityMetadata {
    data: Vec<DataItemEntry>,
    dirty: Vec<u32>,
    revision: u64,
    clean_revision: u64,
}

impl EntityMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reserve all fields a compiled definition can use before ticking.
    pub fn with_capacity(fields: usize) -> Self {
        Self {
            data: Vec::with_capacity(fields),
            dirty: Vec::with_capacity(fields),
            revision: 0,
            clean_revision: 0,
        }
    }

    /// Explicit wire state, not a claim that a sampled instance is a default.
    /// Repeated keys keep their last supplied value.
    pub fn from_entries(entries: impl IntoIterator<Item = DataItemEntry>) -> Self {
        let entries = entries.into_iter();
        let mut metadata = Self::with_capacity(entries.size_hint().0);
        for entry in entries {
            metadata.set_payload(entry.id, entry.payload);
        }
        metadata
    }

    pub fn get(&self, key: u32) -> Option<&DataItemEntryPayload> {
        self.data
            .binary_search_by_key(&key, |entry| entry.id)
            .ok()
            .map(|index| &self.data[index].payload)
    }

    /// Update a generated payload. Existing-field updates perform no allocation
    /// or name lookup. New fields use the definition's reserved capacity.
    pub fn set_payload(&mut self, key: u32, payload: DataItemEntryPayload) -> bool {
        match self.data.binary_search_by_key(&key, |entry| entry.id) {
            Ok(index) => {
                if wire_value_eq(&self.data[index].payload, &payload) {
                    return false;
                }
                self.data[index].payload = payload;
            }
            Err(index) => self.data.insert(index, DataItemEntry { id: key, payload }),
        }
        if let Err(index) = self.dirty.binary_search(&key) {
            self.dirty.insert(index, key);
        }
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.clean_revision
    }

    /// Local bookkeeping only, not an acknowledgment of viewer delivery.
    pub fn mark_clean(&mut self) {
        self.clean_revision = self.revision;
        self.dirty.clear();
    }

    pub fn snapshot(&self) -> SynchedActorDataCopyableDataList {
        SynchedActorDataCopyableDataList {
            data: self.data.clone(),
        }
    }

    /// Refresh a full snapshot while retaining the caller's buffer.
    pub fn copy_into(&self, output: &mut SynchedActorDataCopyableDataList) {
        output.data.clone_from(&self.data);
    }

    /// Coalesce repeated writes to one final payload per dirty key. Calling this
    /// does not clear dirty state; the replication owner decides when to do so.
    pub fn copy_changes(&self, output: &mut SynchedActorDataCopyableDataList) {
        output.data.clear();
        output.data.extend(self.dirty.iter().map(|key| {
            let index = self
                .data
                .binary_search_by_key(key, |entry| entry.id)
                .expect("dirty key is present");
            self.data[index].clone()
        }));
    }

    /// Build the independently configured physics flags. Neither gravity nor
    /// collision implies the native breathing or lingering flags.
    pub fn from_physics(has_gravity: bool, has_collision: bool, on_fire: Option<&OnFire>) -> Self {
        let mut meta = Self::with_capacity(8);
        let mut entity_flags = 0;
        if has_gravity {
            entity_flags |= 1u64 << actor_flags::HAS_GRAVITY;
        }
        if has_collision {
            entity_flags |= 1u64 << actor_flags::HAS_COLLISION;
        }

        if let Some(fire) = on_fire
            && fire.is_on_fire()
        {
            entity_flags |= 1u64 << actor_flags::ONFIRE;
        }

        meta.set_long(actor_data_ids::RESERVED_0, entity_flags as i64);
        meta
    }

    /// Add player state flags to metadata.
    pub fn with_player_state(mut self, state: &PlayerState) -> Self {
        let player_flags = (1u64 << actor_flags::SNEAKING)
            | (1u64 << actor_flags::SPRINTING)
            | (1u64 << actor_flags::SWIMMING)
            | (1u64 << actor_flags::GLIDING)
            | (1u64 << actor_flags::CANFLY);
        let mut entity_flags =
            self.get_long(actor_data_ids::RESERVED_0).unwrap_or(0) as u64 & !player_flags;

        if state.sneaking {
            entity_flags |= 1u64 << actor_flags::SNEAKING;
        }
        if state.sprinting {
            entity_flags |= 1u64 << actor_flags::SPRINTING;
        }
        if state.swimming {
            entity_flags |= 1u64 << actor_flags::SWIMMING;
        }
        if state.gliding {
            entity_flags |= 1u64 << actor_flags::GLIDING;
        }
        if state.flying {
            entity_flags |= 1u64 << actor_flags::CANFLY;
        }

        self.set_long(actor_data_ids::RESERVED_0, entity_flags as i64);
        self
    }

    /// Add air supply only. Living health is synchronized through attributes;
    /// actor-data key 1 is structural integrity.
    pub fn with_air_supply(mut self, air_supply: &AirSupply) -> Self {
        self.set_short(actor_data_ids::AIR_SUPPLY, air_supply.current_ticks as i16);
        self.set_short(actor_data_ids::AIR_SUPPLY_MAX, air_supply.max_ticks as i16);
        self
    }

    /// Add nametag.
    pub fn with_nametag(mut self, name: &str) -> Self {
        if !name.is_empty() {
            self.set_string(actor_data_ids::NAME, name.to_string());
            let mut flags = self.get_long(actor_data_ids::RESERVED_0).unwrap_or(0) as u64;
            flags |= (1u64 << actor_flags::CAN_SHOW_NAME) | (1u64 << actor_flags::ALWAYS_SHOW_NAME);
            self.set_long(actor_data_ids::RESERVED_0, flags as i64);
        }
        self
    }

    pub fn set_byte(&mut self, key: u32, value: i8) -> bool {
        self.set_payload(
            key,
            DataItemEntryPayload::DataItemBytePayload(DataItemBytePayload {
                type_: EnumsDataItemType::Byte,
                value,
            }),
        )
    }

    pub fn set_short(&mut self, key: u32, value: i16) -> bool {
        self.set_payload(
            key,
            DataItemEntryPayload::DataItemShortPayload(DataItemShortPayload {
                type_: EnumsDataItemType::Short,
                value,
            }),
        )
    }

    pub fn set_int(&mut self, key: u32, value: i32) -> bool {
        self.set_payload(
            key,
            DataItemEntryPayload::DataItemIntPayload(DataItemIntPayload {
                type_: EnumsDataItemType::Int,
                value,
            }),
        )
    }

    pub fn set_float(&mut self, key: u32, value: f32) -> bool {
        self.set_payload(
            key,
            DataItemEntryPayload::DataItemFloatPayload(DataItemFloatPayload {
                type_: EnumsDataItemType::Float,
                value,
            }),
        )
    }

    pub fn set_string(&mut self, key: u32, value: impl Into<BedrockString>) -> bool {
        self.set_payload(
            key,
            DataItemEntryPayload::DataItemStringPayload(DataItemStringPayload {
                type_: EnumsDataItemType::String,
                value: value.into(),
            }),
        )
    }

    pub fn set_long(&mut self, key: u32, value: i64) -> bool {
        self.set_payload(
            key,
            DataItemEntryPayload::DataItemInt64Payload(DataItemInt64Payload {
                type_: EnumsDataItemType::Int64,
                value,
            }),
        )
    }

    fn get_long(&self, key: u32) -> Option<i64> {
        match self.get(key) {
            Some(DataItemEntryPayload::DataItemInt64Payload(row)) => Some(row.value),
            _ => None,
        }
    }

    /// Consume the state without copying its packet entries.
    pub fn into_data(self) -> SynchedActorDataCopyableDataList {
        SynchedActorDataCopyableDataList { data: self.data }
    }

    pub fn iter(&self) -> impl Iterator<Item = &DataItemEntry> {
        self.data.iter()
    }
}

fn wire_value_eq(left: &DataItemEntryPayload, right: &DataItemEntryPayload) -> bool {
    match (left, right) {
        (
            DataItemEntryPayload::DataItemFloatPayload(left),
            DataItemEntryPayload::DataItemFloatPayload(right),
        ) => left.type_ == right.type_ && left.value.to_bits() == right.value.to_bits(),
        (
            DataItemEntryPayload::DataItemVec3Payload(left),
            DataItemEntryPayload::DataItemVec3Payload(right),
        ) => {
            left.type_ == right.type_
                && left.value.x.to_bits() == right.value.x.to_bits()
                && left.value.y.to_bits() == right.value.y.to_bits()
                && left.value.z.to_bits() == right.value.z.to_bits()
        }
        _ => left == right,
    }
}

#[cfg(test)]
mod tests {
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
            DataItemEntryPayload::DataItemVec3Payload(
                jolyne::valentine::types::DataItemVec3Payload {
                    type_: EnumsDataItemType::Vec3,
                    value: jolyne::valentine::types::Vec3 { x: nan, y: 0.0, z },
                },
            )
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
            include_bytes!("../../../bds-extractor/captures/1.26.51/operator/summons/cow.bin"),
        );
    }

    #[test]
    fn living_metadata_matches_native_chicken_control() {
        assert_living_metadata_control(
            "minecraft:chicken",
            include_bytes!("../../../bds-extractor/captures/1.26.51/operator/summons/chicken.bin"),
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
}
