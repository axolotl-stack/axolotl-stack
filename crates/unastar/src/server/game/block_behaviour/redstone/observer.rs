//! Observers (`observer`, `minecraft:facing_direction`, `powered_bit`).
//!
//! The face looks along `minecraft:facing_direction` at the observed cell;
//! the output is the opposite side.
//!
//! - A change of the observed cell (any block or state change there, as
//!   the neighbour update from that cell reports it) schedules a tick 2
//!   game ticks later, one more when that is not a circuit tick
//!   ([`super::circuit`]), as BDS 1.26.51 does. Changes of the other five
//!   neighbours, and power changes that leave the observed block's state
//!   alone, are ignored. While a tick is pending (the delay and the whole
//!   pulse) further changes add nothing.
//! - That tick turns the observer on (`powered_bit`) and schedules the
//!   turn-off 2 game ticks later (again on a circuit tick): a pulse of 15
//!   lasting one circuit evaluation, as BDS 1.26.51 does. Both run as
//!   part of their circuit tick's evaluation, so dust behind follows in
//!   the same tick.
//! - While on it gives 15, strong and weak, only into the block behind it
//!   (the output side), so a conductor there powers its own neighbours.
//! - Placement: the face looks where the player looks (down when the
//!   pitch is steeper than 45° down, up when steeper than 45° up, else the
//!   horizontal look direction), so the output faces the player
//!   (provisional: public Bedrock behaviour).
//! - An observer moved by a piston pulses as if its observed cell changed
//!   (provisional: public Bedrock behaviour).

use bevy_ecs::prelude::*;
use glam::IVec3;

use super::super::{bit, schedule_tick, with};
use super::{circuit, power};
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::placement::Horizontal;

/// Direction the face looks.
pub const FACING: &str = "minecraft:facing_direction";
/// Whether the observer is giving its pulse.
pub const POWERED: &str = "powered_bit";
/// Game ticks from a watched change to the pulse.
pub const DELAY: u64 = 2;
/// Game ticks the pulse lasts.
pub const PULSE: u64 = 2;

/// The offset from the observer to the cell it watches.
pub fn face(entry: &BlockEntry, state: u32) -> Option<IVec3> {
    let StateValue::Str(value) = entry.state_value(state, FACING)? else {
        return None;
    };
    Some(match value {
        "down" => IVec3::NEG_Y,
        "up" => IVec3::Y,
        other => Horizontal::from_cardinal(other)?.offset(),
    })
}

/// The offset from the observer to the block its output goes into.
pub fn output(entry: &BlockEntry, state: u32) -> Option<IVec3> {
    face(entry, state).map(|face| -face)
}

/// Whether the observer state is giving its pulse.
pub fn powered(entry: &BlockEntry, state: u32) -> bool {
    bit(entry, state, POWERED)
}

/// The placed state: unpowered, the face looking where the player looks
/// (`yaw`/`pitch` of the placing player).
pub fn placement(entry: &BlockEntry, state: u32, yaw: f32, pitch: f32) -> u32 {
    let facing = if pitch > 45.0 {
        "down"
    } else if pitch < -45.0 {
        "up"
    } else {
        Horizontal::from_yaw(yaw).cardinal()
    };
    let state = with(entry, state, FACING, StateValue::Str(facing));
    with(entry, state, POWERED, StateValue::Byte(0))
}

/// A neighbour update of the observer at `pos` from `from`: a change of
/// the observed cell schedules the pulse.
pub fn changed(world: &mut World, pos: IVec3, from: IVec3) {
    let Some((entry, state)) = power::at(world, pos) else {
        return;
    };
    if face(entry, state).is_some_and(|face| pos + face == from) {
        schedule_aligned(world, pos, DELAY);
    }
}

/// The observer was moved to `pos` by a piston: it pulses.
pub fn moved(world: &mut World, pos: IVec3) {
    schedule_aligned(world, pos, DELAY);
}

/// Schedule the observer's tick `delay` game ticks ahead, pushed back to a
/// circuit tick.
fn schedule_aligned(world: &mut World, pos: IVec3, delay: u64) {
    let now = circuit::now(world);
    schedule_tick(world, pos, circuit::aligned_delay(now, delay));
}

/// A due tick: an observer that is off turns on and schedules the end of
/// its pulse; one that is on turns off.
pub fn tick(world: &mut World, pos: IVec3, state: u32) {
    let Some((entry, _)) = power::at(world, pos) else {
        return;
    };
    let on = !powered(entry, state);
    let next = with(entry, state, POWERED, StateValue::Byte(u8::from(on)));
    circuit::as_evaluation(world, |world| {
        crate::server::game::blocks::set_block(world, pos, next, false);
    });
    if on {
        schedule_aligned(world, pos, PULSE);
    }
}

#[cfg(test)]
mod tests {
    use glam::{DVec3, IVec3};

    use super::super::super::tests::{FLOOR, Harness, state};
    use super::{FACING, POWERED};
    use crate::registry::block::StateValue;

    const Y: i32 = FLOOR + 1;

    fn at(x: i32, z: i32) -> IVec3 {
        IVec3::new(x, Y, z)
    }

    fn harness() -> Harness {
        let mut h = Harness::new();
        h.stand(DVec3::new(-10.5, Y as f64 + 1.62, -10.5), 0.0, 0.0);
        h
    }

    fn observer(facing: &'static str) -> u32 {
        state("minecraft:observer", &[(FACING, StateValue::Str(facing))])
    }

    fn on(h: &Harness, pos: IVec3) -> bool {
        h.value(pos, POWERED) == Some(StateValue::Byte(1))
    }

    fn signal(h: &Harness, pos: IVec3) -> Option<StateValue> {
        h.value(pos, super::super::SIGNAL)
    }

    /// An observer watching east, dust behind it (west).
    fn setup() -> Harness {
        let mut h = harness();
        h.put(at(0, 0), observer("east"));
        h.put(at(-1, 0), state("minecraft:redstone_wire", &[]));
        h
    }

    #[test]
    fn a_change_in_front_gives_a_two_tick_pulse_after_two_ticks() {
        let mut h = setup();
        h.set(at(1, 0), state("minecraft:stone", &[]));
        h.tick(1);
        assert!(!on(&h, at(0, 0)));
        assert_eq!(signal(&h, at(-1, 0)), Some(StateValue::Int(0)));
        h.tick(1);
        assert!(on(&h, at(0, 0)));
        assert_eq!(signal(&h, at(-1, 0)), Some(StateValue::Int(15)));
        h.tick(1);
        assert!(on(&h, at(0, 0)));
        h.tick(1);
        assert!(!on(&h, at(0, 0)));
        assert_eq!(signal(&h, at(-1, 0)), Some(StateValue::Int(0)));
        h.tick(10);
        assert!(!on(&h, at(0, 0)), "one change, one pulse");
    }

    #[test]
    fn a_state_change_in_front_counts_too() {
        let mut h = setup();
        h.put(
            at(1, 0),
            state(
                "minecraft:lever",
                &[("lever_direction", StateValue::Str("up_east_west"))],
            ),
        );
        let lever = h.state(at(1, 0));
        let entry = crate::registry::BlockRegistry::vanilla()
            .get_by_runtime_id(lever)
            .unwrap();
        let open = super::super::super::with(entry, lever, "open_bit", StateValue::Byte(1));
        h.set(at(1, 0), open);
        h.tick(2);
        assert!(on(&h, at(0, 0)));
    }

    #[test]
    fn changes_beside_above_below_and_behind_are_ignored() {
        let mut h = harness();
        h.put(at(0, 0), observer("east"));
        for cell in [at(0, 1), at(0, -1), at(0, 0) + IVec3::Y, at(-1, 0)] {
            h.set(cell, state("minecraft:stone", &[]));
            h.tick(6);
            assert!(!on(&h, at(0, 0)), "change at {cell}");
        }
    }

    #[test]
    fn the_pulse_strongly_powers_the_block_behind() {
        let mut h = harness();
        h.put(at(0, 0), observer("east"));
        h.put(at(-1, 0), state("minecraft:stone", &[]));
        h.put(at(-2, 0), state("minecraft:redstone_lamp", &[]));
        h.set(at(1, 0), state("minecraft:stone", &[]));
        h.tick(2);
        assert_eq!(h.name(at(-2, 0)), "minecraft:lit_redstone_lamp");
    }

    #[test]
    fn placement_faces_where_the_player_looks() {
        let mut h = Harness::new();
        h.hold("minecraft:observer", 1);
        // Looking east (yaw 270) at the floor block's top face.
        h.stand(DVec3::new(0.5, Y as f64 + 1.62, 0.5), 270.0, 10.0);
        let placed = h.click(IVec3::new(2, FLOOR, 0), 1, DVec3::new(0.5, 1.0, 0.5));
        assert_eq!(placed.len(), 1, "{placed:?}");
        let pos = placed[0].0;
        assert_eq!(h.value(pos, FACING), Some(StateValue::Str("east")));
        assert!(!on(&h, pos));
    }
}
