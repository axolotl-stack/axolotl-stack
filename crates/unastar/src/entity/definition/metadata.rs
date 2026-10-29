//! Actor-data state using the generated protocol payloads directly.
//! Numeric updates retain sorted keys and coalesced dirty fields; constructor
//! defaults and metadata-key meanings remain the caller's explicit input.

use crate::entity::components::*;
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

    /// Actor data a living mob starts with before its composition is
    /// derived. Every row is the value BDS 1.26.51 sends in `AddActor` for a
    /// freshly summoned adult cow and chicken (identical for both, retained
    /// captures `operator/summons/{cow,chicken}.bin`); rows that depend on the
    /// definition (flags, scale, box, air) are overwritten by
    /// `entity::mob::derive_mob_state`. Keys are listed in wire order.
    pub fn living_base() -> Self {
        use actor_data_ids as id;
        use jolyne::valentine::bedrock::codec::Nbt;
        use jolyne::valentine::types::{DataItemCompoundTagPayload, DataItemVec3Payload, Vec3};
        let mut m = Self::with_capacity(72);
        m.set_long(id::RESERVED_0, 0);
        m.set_int(id::STRUCTURAL_INTEGRITY, 1);
        m.set_int(id::VARIANT, 0);
        m.set_byte(id::COLOR_INDEX, 0);
        m.set_string(id::NAME, "");
        m.set_long(id::OWNER, -1);
        m.set_long(id::TARGET, 0);
        m.set_short(id::AIR_SUPPLY, 300);
        m.set_byte(id::RESERVED_010, 0);
        m.set_byte(id::CHARGE_AMOUNT, 0);
        m.set_long(id::LEASH_HOLDER, 0);
        m.set_float(id::RESERVED_038, 1.0);
        m.set_byte(id::HAS_NPC, 0);
        m.set_short(id::AIR_SUPPLY_MAX, 300);
        m.set_int(id::MARK_VARIANT, 0);
        m.set_byte(id::CONTAINER_TYPE, 0);
        m.set_int(id::CONTAINER_SIZE, 0);
        m.set_int(id::CONTAINER_STRENGTH_MODIFIER, 0);
        m.set_float(id::RESERVED_053, 0.0);
        m.set_float(id::RESERVED_054, 0.0);
        m.set_int(id::FUSE_TIME, -1);
        m.set_payload(
            id::RESERVED_056,
            DataItemEntryPayload::DataItemVec3Payload(DataItemVec3Payload {
                type_: EnumsDataItemType::Vec3,
                value: Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            }),
        );
        m.set_byte(id::SEAT_LOCK_PASSENGER_ROTATION, 0);
        m.set_float(id::SEAT_LOCK_PASSENGER_ROTATION_DEGREES, 0.0);
        m.set_byte(id::SEAT_ROTATION_OFFSET, 0);
        m.set_float(id::SEAT_ROTATION_OFFSET_DEGREES, 0.0);
        m.set_byte(id::HAS_COMMAND_BLOCK, 0);
        m.set_string(id::COMMAND_NAME, "");
        m.set_string(id::LAST_COMMAND_OUTPUT, "");
        m.set_byte(id::TRACK_COMMAND_OUTPUT, 1);
        m.set_byte(id::RESERVED_074, 0);
        m.set_int(id::STRENGTH, 0);
        m.set_int(id::STRENGTH_MAX, 0);
        m.set_int(id::DATA_SPELL_CASTING_COLOR, 0);
        m.set_int(id::DATA_LIFETIME_TICKS, -1);
        m.set_byte(id::NAMETAG_ALWAYS_SHOW, 0);
        m.set_byte(id::COLOR_2_INDEX, 0);
        m.set_int(id::EATING_COUNTER, 0);
        m.set_long(id::RESERVED_092, 0);
        m.set_int(id::TRADE_TIER, 0);
        m.set_int(id::MAX_TRADE_TIER, 0);
        m.set_int(id::TRADE_EXPERIENCE, 0);
        m.set_int(id::SKIN_ID, 0);
        m.set_int(id::COMMAND_BLOCK_TICK_DELAY, 3);
        m.set_byte(id::COMMAND_BLOCK_EXECUTE_ON_FIRST_TICK, 1);
        m.set_float(id::AMBIENT_SOUND_INTERVAL, 6.0);
        m.set_float(id::AMBIENT_SOUND_INTERVAL_RANGE, 16.0);
        m.set_string(id::AMBIENT_SOUND_EVENT_NAME, "ambient");
        m.set_float(id::FALL_DAMAGE_MULTIPLIER, 1.0);
        m.set_int(id::LOW_TIER_CURED_TRADE_DISCOUNT, 0);
        m.set_int(id::HIGH_TIER_CURED_TRADE_DISCOUNT, 0);
        m.set_int(id::NEARBY_CURED_TRADE_DISCOUNT, 0);
        m.set_int(id::NEARBY_CURED_DISCOUNT_TIME_STAMP, 0);
        m.set_payload(
            id::HITBOX,
            DataItemEntryPayload::DataItemCompoundTagPayload(DataItemCompoundTagPayload {
                type_: EnumsDataItemType::Compoundtag,
                value: Nbt(bytes::Bytes::from_static(b"\n\0\0")),
            }),
        );
        m.set_byte(id::IS_BUOYANT, 0);
        m.set_float(id::FREEZING_EFFECT_STRENGTH, 0.0);
        m.set_float(id::MOVEMENT_SOUND_DISTANCE_OFFSET, 1.0);
        m.set_int(id::HEARTBEAT_INTERVAL_TICKS, 20);
        m.set_int(id::HEARTBEAT_SOUND_EVENT, 614);
        m.set_long(id::VISIBLE_MOB_EFFECTS, 0);
        m.set_string(id::FILTERED_NAME, "");
        m.set_float(id::SEAT_THIRD_PERSON_CAMERA_RADIUS, 0.0);
        m.set_float(id::SEAT_CAMERA_RELAX_DISTANCE_SMOOTHING, 0.0);
        m.set_int(id::AIM_ASSIST_PRIORITY_PRESET_ID, -1);
        m.set_int(id::AIM_ASSIST_PRIORITY_CATEGORY_ID, -1);
        m.set_int(id::AIM_ASSIST_PRIORITY_ACTOR_ID, -1);
        m.set_long(id::RESERVED_139, 0);
        m.set_float(id::NAMEPLATE_RENDER_DISTANCE_MAX, 64.0);
        m.mark_clean();
        m
    }

    /// Actor data a non-mob actor (experience orb, projectile) starts with:
    /// the [`Self::living_base`] rows without the target, eating, mob-effect
    /// and one reserved row, with the name tag shown byte `0xff` and an
    /// ambient sound interval of 8 s. Matches the native summons of the orb
    /// and the egg (`operator/summons/{xp_orb,egg}.bin`) apart from their
    /// own rows (flags, box, orb value, buoyancy).
    pub fn actor_base() -> Self {
        use actor_data_ids as id;
        let mut m = Self::from_entries(Self::living_base().data.into_iter().filter(|entry| {
            ![
                id::TARGET,
                id::RESERVED_010,
                id::EATING_COUNTER,
                id::VISIBLE_MOB_EFFECTS,
            ]
            .contains(&entry.id)
        }));
        m.set_byte(id::NAMETAG_ALWAYS_SHOW, -1);
        m.set_float(id::AMBIENT_SOUND_INTERVAL, 8.0);
        m.mark_clean();
        m
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
mod tests;
