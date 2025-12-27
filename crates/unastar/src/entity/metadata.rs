//! Entity metadata for network serialization.

use super::components::*;
use std::collections::HashMap;

/// Protocol metadata keys (from Bedrock protocol).
pub mod keys {
    pub const FLAGS: u32 = 0;
    pub const HEALTH: u32 = 1;
    pub const VARIANT: u32 = 2;
    pub const COLOR: u32 = 3;
    pub const NAMETAG: u32 = 4;
    pub const OWNER: u32 = 5;
    pub const TARGET: u32 = 6;
    pub const AIR_SUPPLY: u32 = 7;
    pub const EFFECT_COLOR: u32 = 8;
    pub const EFFECT_AMBIENCE: u32 = 9;
    pub const HURT_TICKS: u32 = 11;
    pub const HURT_DIRECTION: u32 = 12;
    pub const ROW_TIME_LEFT: u32 = 13;
    pub const ROW_TIME_RIGHT: u32 = 14;
    pub const FUSE_TIME: u32 = 15;
    pub const PADDING: u32 = 16;
    pub const DISPLAY_TILE_RUNTIME_ID: u32 = 17;
    pub const DISPLAY_OFFSET: u32 = 18;
    pub const CUSTOM_DISPLAY: u32 = 19;
    pub const SWELL: u32 = 20;
    pub const OLD_SWELL: u32 = 21;
    pub const SWELL_DIRECTION: u32 = 22;
    pub const CHARGE_AMOUNT: u32 = 23;
    pub const ENDER_CRYSTAL_TIME_OFFSET: u32 = 24;
    pub const ALWAYS_SHOW_NAMETAG: u32 = 25;
    pub const SCALE: u32 = 56;
    pub const WIDTH: u32 = 54;
    pub const HEIGHT: u32 = 55;
    pub const MAX_AIR_SUPPLY: u32 = 59;
    pub const MARK_VARIANT: u32 = 60;
    pub const CONTAINER_TYPE: u32 = 61;
    pub const CONTAINER_SIZE: u32 = 62;
    pub const CONTAINER_STRENGTH: u32 = 63;
    pub const BOUNDING_BOX_WIDTH: u32 = 68;
    pub const BOUNDING_BOX_HEIGHT: u32 = 69;
}

/// Protocol entity data flags.
pub mod flags {
    pub const ON_FIRE: u64 = 1 << 0;
    pub const SNEAKING: u64 = 1 << 1;
    pub const RIDING: u64 = 1 << 2;
    pub const SPRINTING: u64 = 1 << 3;
    pub const USING_ITEM: u64 = 1 << 4;
    pub const INVISIBLE: u64 = 1 << 5;
    pub const TEMPTED: u64 = 1 << 6;
    pub const IN_LOVE: u64 = 1 << 7;
    pub const SADDLED: u64 = 1 << 8;
    pub const POWERED: u64 = 1 << 9;
    pub const IGNITED: u64 = 1 << 10;
    pub const BABY: u64 = 1 << 11;
    pub const CONVERTING: u64 = 1 << 12;
    pub const CRITICAL: u64 = 1 << 13;
    pub const SHOW_NAMETAG: u64 = 1 << 14;
    pub const ALWAYS_SHOW_NAMETAG: u64 = 1 << 15;
    pub const IMMOBILE: u64 = 1 << 16;
    pub const SILENT: u64 = 1 << 17;
    pub const WALL_CLIMBING: u64 = 1 << 18;
    pub const CLIMB: u64 = 1 << 19;
    pub const SWIM: u64 = 1 << 20;
    pub const FLY: u64 = 1 << 21;
    pub const WALKER: u64 = 1 << 22;
    pub const RESTING: u64 = 1 << 23;
    pub const SITTING: u64 = 1 << 24;
    pub const ANGRY: u64 = 1 << 25;
    pub const INTERESTED: u64 = 1 << 26;
    pub const CHARGED: u64 = 1 << 27;
    pub const TAMED: u64 = 1 << 28;
    pub const ORPHANED: u64 = 1 << 29;
    pub const LEASHED: u64 = 1 << 30;
    pub const SHEARED: u64 = 1 << 31;
    pub const GLIDING: u64 = 1 << 32;
    pub const ELDER: u64 = 1 << 33;
    pub const MOVING: u64 = 1 << 34;
    pub const BREATHING: u64 = 1 << 35;
    pub const CHESTED: u64 = 1 << 36;
    pub const STACKABLE: u64 = 1 << 37;
    pub const SHOW_BOTTOM: u64 = 1 << 38;
    pub const STANDING: u64 = 1 << 39;
    pub const SHAKING: u64 = 1 << 40;
    pub const IDLING: u64 = 1 << 41;
    pub const CASTING: u64 = 1 << 42;
    pub const CHARGING: u64 = 1 << 43;
    pub const WASD_CONTROLLED: u64 = 1 << 44;
    pub const CAN_POWER_JUMP: u64 = 1 << 45;
    pub const CAN_DASH: u64 = 1 << 46;
    pub const HAS_GRAVITY: u64 = 1 << 47;
    pub const HAS_COLLISION: u64 = 1 << 48;
    pub const AFFECTED_BY_GRAVITY: u64 = 1 << 49;
    pub const FIRE_IMMUNE: u64 = 1 << 50;
    pub const DANCING: u64 = 1 << 51;
    pub const ENCHANTED: u64 = 1 << 52;
    pub const RETURN_TRIDENT: u64 = 1 << 53;
    pub const CONTAINER_PRIVATE: u64 = 1 << 54;
    pub const TRANSFORMING: u64 = 1 << 55;
    pub const DAMAGE_NEARBY_MOBS: u64 = 1 << 56;
    pub const SWIMMING: u64 = 1 << 57;
    pub const BRIBED: u64 = 1 << 58;
    pub const PREGNANT: u64 = 1 << 59;
    pub const LAYING_EGG: u64 = 1 << 60;
    pub const RIDER_CAN_PICK: u64 = 1 << 61;
    pub const TRANSITION_SITTING: u64 = 1 << 62;
    pub const EATING: u64 = 1 << 63;
}

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

    /// Build base metadata from transform components.
    pub fn from_base(on_fire: Option<&OnFire>) -> Self {
        let mut meta = Self::new();
        let mut entity_flags: u64 = flags::HAS_GRAVITY | flags::HAS_COLLISION | flags::BREATHING;

        if let Some(fire) = on_fire {
            if fire.is_on_fire() {
                entity_flags |= flags::ON_FIRE;
            }
        }

        meta.set_long(keys::FLAGS, entity_flags as i64);
        meta
    }

    /// Add player state flags to metadata.
    pub fn with_player_state(mut self, state: &PlayerState) -> Self {
        let mut entity_flags = self.get_long(keys::FLAGS).unwrap_or(0) as u64;

        if state.sneaking {
            entity_flags |= flags::SNEAKING;
        }
        if state.sprinting {
            entity_flags |= flags::SPRINTING;
        }
        if state.swimming {
            entity_flags |= flags::SWIMMING;
        }
        if state.gliding {
            entity_flags |= flags::GLIDING;
        }
        if state.flying {
            entity_flags |= flags::FLY;
        }

        self.set_long(keys::FLAGS, entity_flags as i64);
        self
    }

    /// Add living entity data.
    pub fn with_living(mut self, health: &Health, air_supply: &AirSupply) -> Self {
        self.set_int(keys::HEALTH, health.current as i32);
        self.set_short(keys::AIR_SUPPLY, air_supply.current_ticks as i16);
        self.set_short(keys::MAX_AIR_SUPPLY, air_supply.max_ticks as i16);
        self
    }

    /// Add nametag.
    pub fn with_nametag(mut self, name: &str) -> Self {
        if !name.is_empty() {
            self.set_string(keys::NAMETAG, name.to_string());
            let mut flags = self.get_long(keys::FLAGS).unwrap_or(0) as u64;
            flags |= flags::SHOW_NAMETAG | flags::ALWAYS_SHOW_NAMETAG;
            self.set_long(keys::FLAGS, flags as i64);
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
