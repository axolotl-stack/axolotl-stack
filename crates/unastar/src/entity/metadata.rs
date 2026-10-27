//! Entity metadata for network serialization.

use super::components::*;
use jolyne::valentine::{actor_data_i_ds as actor_data_ids, actor_flags};
use std::collections::HashMap;

/// Metadata value types.
#[derive(Debug, Clone)]
pub enum MetadataValue {
    Byte(u8),
    Short(i16),
    Int(i32),
    Float(f32),
    String(String),
    CompoundTag(Vec<u8>),
    BlockPos(i32, i32, i32),
    Long(i64),
    Vec3(f32, f32, f32),
}

/// Builder for entity metadata.
#[derive(Debug, Default)]
pub struct EntityMetadata {
    data: HashMap<u32, MetadataValue>,
}

impl EntityMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build the independently configured physics flags. Neither gravity nor
    /// collision implies the native breathing or lingering flags.
    pub fn from_physics(has_gravity: bool, has_collision: bool, on_fire: Option<&OnFire>) -> Self {
        let mut meta = Self::new();
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
        let mut entity_flags = self.get_long(actor_data_ids::RESERVED_0).unwrap_or(0) as u64;

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

    // Setters for various types
    pub fn set_byte(&mut self, key: u32, value: u8) {
        self.data.insert(key, MetadataValue::Byte(value));
    }

    pub fn set_short(&mut self, key: u32, value: i16) {
        self.data.insert(key, MetadataValue::Short(value));
    }

    pub fn set_int(&mut self, key: u32, value: i32) {
        self.data.insert(key, MetadataValue::Int(value));
    }

    pub fn set_float(&mut self, key: u32, value: f32) {
        self.data.insert(key, MetadataValue::Float(value));
    }

    pub fn set_string(&mut self, key: u32, value: String) {
        self.data.insert(key, MetadataValue::String(value));
    }

    pub fn set_long(&mut self, key: u32, value: i64) {
        self.data.insert(key, MetadataValue::Long(value));
    }

    fn get_long(&self, key: u32) -> Option<i64> {
        match self.data.get(&key) {
            Some(MetadataValue::Long(v)) => Some(*v),
            _ => None,
        }
    }

    /// Get the raw data map.
    pub fn into_map(self) -> HashMap<u32, MetadataValue> {
        self.data
    }

    /// Get iterator over entries.
    pub fn iter(&self) -> impl Iterator<Item = (&u32, &MetadataValue)> {
        self.data.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jolyne::valentine::AddActorPacket;
    use jolyne::valentine::types::DataItemEntryPayload;
    use valentine::bedrock::codec::BedrockCodec;

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
        let actual = metadata.into_map();
        for id in [7, 38, 42, 53, 54] {
            let expected = &native
                .actor_data
                .data
                .iter()
                .find(|row| row.id == id)
                .unwrap()
                .payload;
            match (actual.get(&id), expected) {
                (
                    Some(MetadataValue::Short(value)),
                    DataItemEntryPayload::DataItemShortPayload(row),
                ) => {
                    assert_eq!(*value, row.value, "native metadata {id}");
                }
                (
                    Some(MetadataValue::Float(value)),
                    DataItemEntryPayload::DataItemFloatPayload(row),
                ) => {
                    assert_eq!(*value, row.value, "native metadata {id}");
                }
                (actual, expected) => panic!("native metadata {id}: {actual:?} != {expected:?}"),
            }
        }
        assert!(
            !actual.contains_key(&1),
            "living health is an attribute, not actor-data structural integrity"
        );
        let Some(MetadataValue::Long(actual_flags)) = actual.get(&0) else {
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
            *actual_flags as u64 & physics_mask,
            expected_flags.value as u64 & physics_mask,
            "dry living physics/breathing flags"
        );
    }
}
