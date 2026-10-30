//! Test support: every block id a packet carries must be a wire id of the
//! configured numbering. Internally a block state is its canonical palette
//! ordinal; in hashed mode (the default, as BDS 1.26.51 advertises) an
//! ordinal that leaks onto the wire names no block, or the wrong one, and the
//! client draws air.

use jolyne::valentine::McpePacketData;
use jolyne::valentine::actor_data_i_ds as actor_data_ids;
use jolyne::valentine::types::DataItemEntryPayload;

use crate::entity::mob::constants::{
    LEVEL_EVENT_PARTICLES_DESTROY_BLOCK, LEVEL_EVENT_PARTICLES_DESTROY_BLOCK_NO_SOUND,
};
use crate::registry::block::BlockNetworkIds;

/// `LevelEvent` punch-block particles (the block's id in the data).
const LEVEL_EVENT_PARTICLE_PUNCH_BLOCK: i32 = 2014;

/// `LevelSoundEvent` names whose data is the block the sound belongs to.
const BLOCK_SOUNDS: &[&str] = &[
    "hit",
    "place",
    "break.block",
    "fall",
    "step",
    "item.use.on",
    "chest.open",
    "chest.closed",
    "barrel.open",
    "barrel.close",
];

/// Panics when a block id in `packets` is not a `ids` wire id; returns how
/// many block ids were checked, so a caller can tell the check was not empty.
pub(crate) fn assert_block_ids_on_wire(ids: BlockNetworkIds, packets: &[McpePacketData]) -> usize {
    let mut checked = 0;
    let mut check = |what: String, wire: u32| {
        assert!(
            ids.from_wire(wire).is_some(),
            "{what} carries {wire} (0x{wire:08x}), which is not a {ids:?} block id"
        );
        checked += 1;
    };
    for packet in packets {
        match packet {
            McpePacketData::UpdateBlockPacket(update) => {
                check("UpdateBlock".into(), update.block_runtime_id);
            }
            McpePacketData::LevelEventPacket(event)
                if [
                    LEVEL_EVENT_PARTICLES_DESTROY_BLOCK,
                    LEVEL_EVENT_PARTICLES_DESTROY_BLOCK_NO_SOUND,
                    LEVEL_EVENT_PARTICLE_PUNCH_BLOCK,
                ]
                .contains(&event.event_id) =>
            {
                check(format!("LevelEvent {}", event.event_id), event.data as u32);
            }
            McpePacketData::LevelSoundEventPacket(sound)
                if BLOCK_SOUNDS.contains(&&*sound.sound_event) =>
            {
                check(
                    format!("LevelSoundEvent {}", sound.sound_event),
                    sound.data as u32,
                );
            }
            McpePacketData::AddActorPacket(actor)
                if &*actor.actor_type == "minecraft:falling_block" =>
            {
                for entry in &actor.actor_data.data {
                    if entry.id == actor_data_ids::VARIANT
                        && let DataItemEntryPayload::DataItemIntPayload(value) = &entry.payload
                    {
                        check("falling_block VARIANT".into(), value.value as u32);
                    }
                }
            }
            _ => {}
        }
    }
    checked
}
