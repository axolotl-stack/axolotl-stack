//! Sculk sensors, calibrated sculk sensors and sculk shriekers.
//!
//! Only placement and survival are modelled: a sensor is placed inactive
//! (`sculk_sensor_phase` 0) and a shrieker placed by a player is inactive
//! and can never summon a warden (`active` and `can_summon` 0, provisional:
//! Minecraft Wiki "Sculk Shrieker"). They need no support and hold no
//! items.
//!
//! Not modelled: vibrations (block changes, steps and other game events
//! within 8 blocks activating a sensor with power by distance), the active
//! and cooldown phases, the calibrated sensor's frequency filter from
//! redstone at its back, shrieking, darkness and warden summoning. A sensor
//! set to an active phase stays in it.

use bevy_ecs::prelude::*;
use glam::IVec3;

use super::super::{Place, with};
use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};

/// Sensor state property.
pub const PHASE: &str = "sculk_sensor_phase";
/// Shrieker state properties.
pub const ACTIVE: &str = "active";
pub const CAN_SUMMON: &str = "can_summon";

/// Sculk blocks have no use interaction.
pub fn is_usable(_entry: &BlockEntry, _state: u32) -> bool {
    false
}

/// Sculk blocks have no use interaction.
pub fn use_block(_world: &mut World, _player: Entity, _pos: IVec3, _state: u32) -> bool {
    false
}

/// Sensors are placed inactive; shriekers inactive and unable to summon.
pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    let block = place.block;
    let state = with(block, place.state, PHASE, StateValue::Int(0));
    let state = with(block, state, ACTIVE, StateValue::Byte(0));
    Some(with(block, state, CAN_SUMMON, StateValue::Byte(0)))
}

/// Sculk blocks need no support.
pub fn can_survive(
    _blocks: &impl BlockSource,
    _pos: IVec3,
    _state: u32,
    _entry: &BlockEntry,
) -> bool {
    true
}

/// Nothing happens when a sculk block is set (vibrations are not modelled).
pub fn placed(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Neighbour changes do not reach sculk blocks as vibrations (not
/// modelled).
pub fn neighbour_changed(_world: &mut World, _pos: IVec3, _state: u32, _from: IVec3) {}

/// Sculk blocks schedule no ticks (their phases are not modelled).
pub fn scheduled_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Random ticks do not reach sculk blocks.
pub fn random_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Redstone input (the calibrated sensor's frequency filter) is not
/// modelled.
pub fn power_changed(_world: &mut World, _pos: IVec3, _state: u32, _powered: bool) {}

/// Sculk blocks hold no items.
pub fn breaking(_world: &mut World, _pos: IVec3, _state: u32) {}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::super::super::tests::{FLOOR, state};
    use super::*;
    use crate::entity::systems::physics::tests::TestBlocks;
    use crate::registry::BlockRegistry;
    use crate::server::game::placement::{PlacementInput, resolve_placement};

    fn place(name: &str) -> u32 {
        let block = BlockRegistry::vanilla().get_by_name(name).expect(name);
        let input = PlacementInput {
            clicked: IVec3::new(0, FLOOR, 0),
            face: 1,
            click: DVec3::new(0.5, 1.0, 0.5),
            yaw: 0.0,
            pitch: 30.0,
        };
        let blocks = TestBlocks::floor("minecraft:stone", FLOOR);
        resolve_placement(&blocks, block, &input)
            .expect("placeable")
            .state
    }

    #[test]
    fn sensors_are_placed_inactive() {
        for name in [
            "minecraft:sculk_sensor",
            "minecraft:calibrated_sculk_sensor",
        ] {
            let entry = BlockRegistry::vanilla().get_by_name(name).unwrap();
            assert_eq!(
                entry.state_value(place(name), PHASE),
                Some(StateValue::Int(0)),
                "{name}"
            );
        }
    }

    #[test]
    fn players_place_shriekers_that_cannot_summon() {
        assert_eq!(
            place("minecraft:sculk_shrieker"),
            state(
                "minecraft:sculk_shrieker",
                &[
                    (ACTIVE, StateValue::Byte(0)),
                    (CAN_SUMMON, StateValue::Byte(0))
                ],
            )
        );
    }
}
