//! Redstone behaviour through a real `World`.

use glam::{DVec3, IVec3};
use jolyne::valentine::McpePacketData;

use super::super::tests::{FLOOR, Harness, state};
use super::{diode, power};
use crate::entity::components::{DroppedItem, PhysicsBody, Position};
use crate::registry::block::StateValue;

const Y: i32 = FLOOR + 1;

fn at(x: i32, z: i32) -> IVec3 {
    IVec3::new(x, Y, z)
}

/// A harness with the player far from every test circuit.
fn harness() -> Harness {
    let mut h = Harness::new();
    h.stand(DVec3::new(-10.5, Y as f64 + 1.62, -10.5), 0.0, 0.0);
    h
}

fn lever(h: &mut Harness, pos: IVec3, direction: &'static str) {
    h.put(
        pos,
        state(
            "minecraft:lever",
            &[("lever_direction", StateValue::Str(direction))],
        ),
    );
}

fn dust(h: &mut Harness, pos: IVec3) {
    h.put(pos, state("minecraft:redstone_wire", &[]));
}

fn put(h: &mut Harness, pos: IVec3, name: &str) {
    h.put(pos, state(name, &[]));
}

fn use_block(h: &mut Harness, pos: IVec3) -> bool {
    super::super::use_block(&mut h.world, h.player, pos)
}

fn signal(h: &Harness, pos: IVec3) -> i32 {
    match h.value(pos, super::SIGNAL) {
        Some(StateValue::Int(value)) => value,
        other => panic!("no signal at {pos}: {other:?}"),
    }
}

fn open(h: &Harness, pos: IVec3) -> bool {
    h.value(pos, "open_bit") == Some(StateValue::Byte(1))
}

/// `(sound, data)` of every LevelSoundEvent sent since the last call.
fn sounds(h: &mut Harness) -> Vec<(String, i32)> {
    h.packets()
        .into_iter()
        .filter_map(|packet| match packet {
            McpePacketData::LevelSoundEventPacket(sound) => {
                Some((sound.sound_event.to_string(), sound.data))
            }
            _ => None,
        })
        .collect()
}

/// `(position, instrument, pitch)` of every BlockEvent sent since the
/// last call.
fn notes(h: &mut Harness) -> Vec<(IVec3, i32, i32)> {
    h.packets()
        .into_iter()
        .filter_map(|packet| match packet {
            McpePacketData::BlockEventPacket(event) => Some((
                IVec3::new(
                    event.block_position.x,
                    event.block_position.y,
                    event.block_position.z,
                ),
                event.event_type,
                event.event_value,
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn lever_powers_a_dust_line_that_decays_to_a_lamp_fifteen_blocks_away() {
    let mut h = harness();
    lever(&mut h, at(0, 5), "up_east_west");
    for x in 1..=15 {
        dust(&mut h, at(x, 5));
    }
    put(&mut h, at(16, 5), "minecraft:redstone_lamp");
    assert!(use_block(&mut h, at(0, 5)));
    // Nothing moves in the lever's own tick; the whole line and the lamp
    // change together at the next circuit tick.
    assert_eq!(signal(&h, at(1, 5)), 0);
    h.tick(1);
    assert_eq!(signal(&h, at(1, 5)), 0);
    h.tick(1);
    for x in 1..=15 {
        assert_eq!(signal(&h, at(x, 5)), 16 - x, "dust at x = {x}");
    }
    assert_eq!(h.name(at(16, 5)), "minecraft:lit_redstone_lamp");

    // Off: the dust drops at the next circuit tick, the lamp four ticks
    // later.
    assert!(use_block(&mut h, at(0, 5)));
    h.tick(2);
    for x in 1..=15 {
        assert_eq!(signal(&h, at(x, 5)), 0, "dust at x = {x}");
    }
    h.tick(3);
    assert_eq!(h.name(at(16, 5)), "minecraft:lit_redstone_lamp");
    h.tick(1);
    assert_eq!(h.name(at(16, 5)), "minecraft:redstone_lamp");

    // A sixteenth dust block gets nothing, nor does a lamp behind it.
    let mut far = harness();
    lever(&mut far, at(0, 5), "up_east_west");
    for x in 1..=16 {
        dust(&mut far, at(x, 5));
    }
    put(&mut far, at(17, 5), "minecraft:redstone_lamp");
    use_block(&mut far, at(0, 5));
    far.tick(2);
    assert_eq!(signal(&far, at(15, 5)), 1);
    assert_eq!(signal(&far, at(16, 5)), 0);
    assert_eq!(far.name(at(17, 5)), "minecraft:redstone_lamp");
}

#[test]
fn dust_steps_up_onto_a_block_and_down_again_until_covered() {
    let mut h = harness();
    lever(&mut h, at(0, 5), "up_east_west");
    dust(&mut h, at(1, 5));
    put(&mut h, at(2, 5), "minecraft:stone");
    dust(&mut h, at(2, 5) + IVec3::Y);
    dust(&mut h, at(3, 5));
    use_block(&mut h, at(0, 5));
    h.tick(2);
    assert_eq!(signal(&h, at(1, 5)), 15);
    assert_eq!(signal(&h, at(2, 5) + IVec3::Y), 14);
    assert_eq!(signal(&h, at(3, 5)), 13);

    // A block on the lower dust cuts the step up.
    h.set(at(1, 5) + IVec3::Y, state("minecraft:stone", &[]));
    h.tick(2);
    assert_eq!(signal(&h, at(1, 5)), 15);
    assert_eq!(signal(&h, at(2, 5) + IVec3::Y), 0);
    assert_eq!(signal(&h, at(3, 5)), 0);
}

#[test]
fn lever_on_a_block_opens_an_iron_door_beside_that_block() {
    let mut h = harness();
    let block = at(1, 5);
    let lever_at = block - IVec3::Z;
    let door = at(2, 5);
    put(&mut h, block, "minecraft:stone");
    // On the north face of the block.
    lever(&mut h, lever_at, "north");
    let half = |upper: u8| {
        state(
            "minecraft:iron_door",
            &[
                ("upper_block_bit", StateValue::Byte(upper)),
                ("minecraft:cardinal_direction", StateValue::Str("east")),
            ],
        )
    };
    h.put(door, half(0));
    h.put(door + IVec3::Y, half(1));
    h.packets();

    use_block(&mut h, lever_at);
    h.tick(2);
    assert!(open(&h, door));
    assert!(sounds(&mut h).iter().any(|(name, _)| name == "door.open"));
    use_block(&mut h, lever_at);
    h.tick(2);
    assert!(!open(&h, door));
    assert!(sounds(&mut h).iter().any(|(name, _)| name == "door.close"));

    // Breaking the lever while on closes the door too.
    use_block(&mut h, lever_at);
    h.tick(2);
    assert!(open(&h, door));
    h.set(lever_at, state("minecraft:air", &[]));
    h.tick(2);
    assert!(!open(&h, door));
}

#[test]
fn stone_button_powers_for_twenty_ticks() {
    let mut h = harness();
    let button = at(3, 5);
    let lamp = at(4, 5);
    h.put(
        button,
        state(
            "minecraft:stone_button",
            &[("facing_direction", StateValue::Int(1))],
        ),
    );
    put(&mut h, lamp, "minecraft:redstone_lamp");
    use_block(&mut h, button);
    assert_eq!(super::power_at(&h.world, lamp), 15);
    // The block the button is on is strongly powered.
    assert_eq!(power::strong_into(&h.world, button - IVec3::Y, true), 15);
    h.tick(2);
    assert_eq!(h.name(lamp), "minecraft:lit_redstone_lamp");
    h.tick(17);
    assert_eq!(super::power_at(&h.world, lamp), 15);
    h.tick(1);
    assert_eq!(super::power_at(&h.world, lamp), 0);
    assert_eq!(power::strong_into(&h.world, button - IVec3::Y, true), 0);
    // Released at an even count of ticks after the press: the lamp sees it
    // at the next circuit tick and goes out four ticks after that.
    h.tick(5);
    assert_eq!(h.name(lamp), "minecraft:lit_redstone_lamp");
    h.tick(1);
    assert_eq!(h.name(lamp), "minecraft:redstone_lamp");
}

fn item_at(h: &mut Harness, pos: IVec3) -> bevy_ecs::entity::Entity {
    h.world
        .spawn((
            DroppedItem,
            Position(pos.as_dvec3() + DVec3::new(0.5, 0.0, 0.5)),
            PhysicsBody::ITEM,
        ))
        .id()
}

#[test]
fn wooden_plate_takes_an_item_and_releases_at_its_next_look() {
    let mut h = harness();
    let wooden = at(3, 5);
    let stone = at(6, 5);
    let lamp = at(4, 5);
    put(&mut h, wooden, "minecraft:wooden_pressure_plate");
    put(&mut h, stone, "minecraft:stone_pressure_plate");
    put(&mut h, lamp, "minecraft:redstone_lamp");
    let item = item_at(&mut h, wooden);
    item_at(&mut h, stone);
    h.packets();

    super::press_plates(&mut h.world);
    assert_eq!(signal(&h, wooden), 1, "a pressed wooden plate stores 1");
    assert_eq!(signal(&h, stone), 0, "items do not press stone plates");
    assert_eq!(super::power_at(&h.world, lamp), 15);
    h.tick(2);
    assert_eq!(h.name(lamp), "minecraft:lit_redstone_lamp");
    let on = h.state(wooden);
    let wire = crate::server::game::types::block_network_ids(&h.world).to_wire(on) as i32;
    assert_eq!(
        sounds(&mut h)
            .into_iter()
            .filter(|(name, _)| name.starts_with("pressure_plate"))
            .collect::<Vec<_>>(),
        vec![("pressure_plate.click_on".to_string(), wire)]
    );

    h.world.despawn(item);
    h.tick(17);
    assert_eq!(signal(&h, wooden), 1);
    h.tick(1);
    assert_eq!(signal(&h, wooden), 0, "released at the look 20 ticks in");
    assert!(
        sounds(&mut h)
            .iter()
            .any(|(name, _)| name == "pressure_plate.click_off")
    );
}

#[test]
fn weighted_plates_count_entities() {
    let mut h = harness();
    let light = at(3, 5);
    let heavy = at(6, 5);
    put(&mut h, light, "minecraft:light_weighted_pressure_plate");
    put(&mut h, heavy, "minecraft:heavy_weighted_pressure_plate");
    for _ in 0..3 {
        item_at(&mut h, light);
        item_at(&mut h, heavy);
    }
    super::press_plates(&mut h.world);
    assert_eq!(signal(&h, light), 3);
    assert_eq!(signal(&h, heavy), 1);
}

/// A redstone torch on a block a lever on the block's north face powers,
/// and a lamp beside the block above the torch.
fn torch_circuit(h: &mut Harness) -> (IVec3, IVec3, IVec3) {
    let block = at(3, 5);
    let torch = block + IVec3::Y;
    let lever_at = block - IVec3::Z;
    let lamp = torch + IVec3::new(1, 1, 0);
    put(h, block, "minecraft:stone");
    h.put(
        torch,
        state(
            "minecraft:redstone_torch",
            &[("torch_facing_direction", StateValue::Str("top"))],
        ),
    );
    lever(h, lever_at, "north");
    put(h, torch + IVec3::Y, "minecraft:stone");
    h.set(lamp, state("minecraft:redstone_lamp", &[]));
    h.tick(2);
    (torch, lever_at, lamp)
}

#[test]
fn redstone_torch_inverts_its_block_two_ticks_later() {
    let mut h = harness();
    let (torch, lever_at, lamp) = torch_circuit(&mut h);
    // The torch powers the block above it strongly, which lights the lamp.
    assert_eq!(h.name(lamp), "minecraft:lit_redstone_lamp");

    use_block(&mut h, lever_at);
    h.tick(1);
    assert_eq!(h.name(torch), "minecraft:redstone_torch");
    h.tick(1);
    assert_eq!(h.name(torch), "minecraft:unlit_redstone_torch");
    assert_eq!(
        h.value(torch, "torch_facing_direction"),
        Some(StateValue::Str("top"))
    );
    h.tick(4);
    assert_eq!(h.name(lamp), "minecraft:redstone_lamp");

    use_block(&mut h, lever_at);
    h.tick(1);
    assert_eq!(h.name(torch), "minecraft:unlit_redstone_torch");
    h.tick(1);
    assert_eq!(h.name(torch), "minecraft:redstone_torch");
    assert_eq!(h.name(lamp), "minecraft:lit_redstone_lamp");
}

#[test]
fn redstone_torch_switching_every_circuit_tick_burns_out_after_its_eighth_turn_off() {
    let mut h = harness();
    let (torch, lever_at, _) = torch_circuit(&mut h);
    h.packets();
    // Switching at every chance: off, on, off, ... each two ticks apart.
    for round in 1..=8 {
        use_block(&mut h, lever_at);
        h.tick(2);
        assert_eq!(h.name(torch), "minecraft:unlit_redstone_torch", "{round}");
        use_block(&mut h, lever_at);
        h.tick(2);
        if round < 8 {
            assert_eq!(h.name(torch), "minecraft:redstone_torch", "{round}");
        }
    }
    // After the eighth turn-off (the fifteenth switch in a row) it burns
    // out instead of lighting again.
    assert_eq!(h.name(torch), "minecraft:unlit_redstone_torch");
    assert!(
        sounds(&mut h).contains(&("fizz".to_string(), -1)),
        "burnout fizz"
    );
    // Updates within 32 ticks leave it out; the first one after lights it.
    h.tick(30);
    h.set(torch + IVec3::X, state("minecraft:stone", &[]));
    h.tick(2);
    assert_eq!(h.name(torch), "minecraft:unlit_redstone_torch");
    h.set(torch + IVec3::X, state("minecraft:air", &[]));
    h.tick(1);
    assert_eq!(h.name(torch), "minecraft:unlit_redstone_torch");
    h.tick(1);
    assert_eq!(h.name(torch), "minecraft:redstone_torch");
}

#[test]
fn redstone_torch_switching_with_pauses_never_burns_out() {
    let mut h = harness();
    let (torch, lever_at, _) = torch_circuit(&mut h);
    for round in 1..=12 {
        use_block(&mut h, lever_at);
        h.tick(3);
        use_block(&mut h, lever_at);
        h.tick(3);
        assert_eq!(h.name(torch), "minecraft:redstone_torch", "{round}");
    }
}

fn repeater(direction: &'static str, delay: i32) -> u32 {
    state(
        "minecraft:unpowered_repeater",
        &[
            (diode::DIRECTION, StateValue::Str(direction)),
            (diode::DELAY, StateValue::Int(delay)),
        ],
    )
}

#[test]
fn repeater_waits_two_ticks_per_delay_step() {
    for delay in 0..4 {
        let mut h = harness();
        lever(&mut h, at(3, 5), "up_east_west");
        h.put(at(4, 5), repeater("west", delay));
        put(&mut h, at(5, 5), "minecraft:redstone_lamp");
        use_block(&mut h, at(3, 5));
        let ticks = (delay as u64 + 1) * 2;
        h.tick(ticks - 1);
        assert_eq!(h.name(at(5, 5)), "minecraft:redstone_lamp", "delay {delay}");
        h.tick(1);
        assert_eq!(h.name(at(4, 5)), "minecraft:powered_repeater");
        assert_eq!(
            h.value(at(4, 5), diode::DELAY),
            Some(StateValue::Int(delay))
        );
        assert_eq!(
            h.name(at(5, 5)),
            "minecraft:lit_redstone_lamp",
            "delay {delay}"
        );
    }
}

#[test]
fn use_cycles_the_repeater_delay() {
    let mut h = harness();
    h.put(at(4, 5), repeater("west", 0));
    for delay in [1, 2, 3, 0] {
        assert!(use_block(&mut h, at(4, 5)));
        assert_eq!(
            h.value(at(4, 5), diode::DELAY),
            Some(StateValue::Int(delay))
        );
    }
}

#[test]
fn powered_side_repeater_locks_a_repeater() {
    let mut h = harness();
    lever(&mut h, at(3, 5), "up_east_west");
    h.put(at(4, 5), repeater("west", 0));
    put(&mut h, at(5, 5), "minecraft:redstone_lamp");
    // A repeater south of it pointing north, fed from the south.
    h.put(at(4, 6), repeater("south", 0));
    lever(&mut h, at(4, 7), "up_east_west");

    use_block(&mut h, at(4, 7));
    h.tick(2);
    assert_eq!(h.name(at(4, 6)), "minecraft:powered_repeater");
    use_block(&mut h, at(3, 5));
    h.tick(10);
    assert_eq!(h.name(at(4, 5)), "minecraft:unpowered_repeater");
    assert_eq!(h.name(at(5, 5)), "minecraft:redstone_lamp");

    // Unlocked, it follows its input.
    use_block(&mut h, at(4, 7));
    h.tick(2);
    assert_eq!(h.name(at(4, 6)), "minecraft:unpowered_repeater");
    h.tick(2);
    assert_eq!(h.name(at(4, 5)), "minecraft:powered_repeater");
    assert_eq!(h.name(at(5, 5)), "minecraft:lit_redstone_lamp");
}

fn comparator() -> u32 {
    state(
        "minecraft:unpowered_comparator",
        &[(diode::DIRECTION, StateValue::Str("west"))],
    )
}

#[test]
fn comparator_compares_and_subtracts_its_side() {
    let mut h = harness();
    let comparator_at = at(4, 5);
    let front = at(5, 5);
    put(&mut h, at(3, 5), "minecraft:redstone_block");
    h.put(comparator_at, comparator());
    dust(&mut h, front);
    // A side line from a lever: 15, 14, 13 at the comparator.
    lever(&mut h, at(4, 9), "up_east_west");
    for z in 6..=8 {
        dust(&mut h, at(4, z));
    }
    use_block(&mut h, at(4, 9));
    h.tick(2);
    assert_eq!(signal(&h, at(4, 6)), 13);
    // The block turns powered in the circuit tick that sees the change; the
    // output level follows one circuit tick later.
    assert_eq!(h.name(comparator_at), "minecraft:powered_comparator");
    assert_eq!(
        h.value(comparator_at, diode::LIT),
        Some(StateValue::Byte(1))
    );
    assert_eq!(signal(&h, front), 0);
    h.tick(2);
    assert_eq!(signal(&h, front), 15, "compare: the back is not weaker");

    h.packets();
    assert!(use_block(&mut h, comparator_at));
    assert_eq!(
        h.value(comparator_at, diode::SUBTRACT),
        Some(StateValue::Byte(1))
    );
    let mode_sounds = |h: &mut Harness| -> Vec<String> {
        sounds(h)
            .into_iter()
            .map(|(name, _)| name)
            .filter(|name| name.starts_with("power."))
            .collect()
    };
    assert_eq!(mode_sounds(&mut h), vec![diode::SOUND_SUBTRACT.to_string()]);
    h.tick(2);
    assert_eq!(signal(&h, front), 15);
    h.tick(2);
    assert_eq!(signal(&h, front), 2, "subtract: 15 - 13");
    assert!(use_block(&mut h, comparator_at));
    assert_eq!(mode_sounds(&mut h), vec![diode::SOUND_COMPARE.to_string()]);
    h.tick(4);
    assert_eq!(signal(&h, front), 15);
}

#[test]
fn comparator_in_compare_mode_gives_nothing_under_a_stronger_side() {
    let mut h = harness();
    let comparator_at = at(4, 5);
    let front = at(5, 5);
    lever(&mut h, at(1, 5), "up_east_west");
    dust(&mut h, at(2, 5));
    dust(&mut h, at(3, 5));
    h.put(comparator_at, comparator());
    dust(&mut h, front);
    // A side repeater pointing north into the comparator, fed from the
    // south.
    h.put(at(4, 6), repeater("south", 0));
    lever(&mut h, at(4, 7), "up_east_west");
    use_block(&mut h, at(1, 5));
    h.tick(2);
    assert_eq!(signal(&h, at(3, 5)), 14);
    h.tick(2);
    assert_eq!(signal(&h, front), 14);

    // A redstone block is no side input.
    h.set(at(4, 4), state("minecraft:redstone_block", &[]));
    h.tick(2);
    assert_eq!(signal(&h, front), 14);

    use_block(&mut h, at(4, 7));
    h.tick(2);
    assert_eq!(h.name(at(4, 6)), "minecraft:powered_repeater");
    // The comparator sees the repeater one circuit tick later, its output
    // dust one more.
    h.tick(2);
    assert_eq!(h.name(comparator_at), "minecraft:unpowered_comparator");
    assert_eq!(signal(&h, front), 14);
    h.tick(2);
    assert_eq!(signal(&h, front), 0);
}

#[test]
fn comparator_reads_container_fullness() {
    assert_eq!(diode::fullness(0.0, 27), 0);
    assert_eq!(diode::fullness(1.0 / 64.0, 27), 1);
    assert_eq!(diode::fullness(13.5, 27), 8);
    assert_eq!(diode::fullness(27.0, 27), 15);
    assert_eq!(diode::fullness(1.0, 5), 3);
}

#[test]
fn power_opens_and_closes_fence_gates_and_trapdoors() {
    let mut h = harness();
    let gate = at(4, 5);
    let trapdoor = at(2, 5);
    lever(&mut h, at(3, 5), "up_east_west");
    h.put(
        gate,
        state(
            "minecraft:fence_gate",
            &[(diode::DIRECTION, StateValue::Str("north"))],
        ),
    );
    put(&mut h, trapdoor, "minecraft:iron_trapdoor");
    h.packets();

    use_block(&mut h, at(3, 5));
    h.tick(2);
    assert!(open(&h, gate));
    assert_eq!(
        h.value(gate, diode::DIRECTION),
        Some(StateValue::Str("north"))
    );
    assert!(open(&h, trapdoor));
    let names: Vec<String> = sounds(&mut h).into_iter().map(|(name, _)| name).collect();
    assert!(names.contains(&"fence_gate.open".to_string()), "{names:?}");
    assert!(names.contains(&"trapdoor.open".to_string()), "{names:?}");

    use_block(&mut h, at(3, 5));
    h.tick(2);
    assert!(!open(&h, gate));
    assert!(!open(&h, trapdoor));

    // Only a change of power acts: a gate opened by hand stays open.
    assert!(use_block(&mut h, gate));
    assert!(open(&h, gate));
    h.set(gate + IVec3::Z, state("minecraft:stone", &[]));
    h.tick(2);
    assert!(open(&h, gate));
}

#[test]
fn note_block_plays_on_a_rising_edge_only() {
    let mut h = harness();
    let note = at(4, 5);
    lever(&mut h, at(3, 5), "up_east_west");
    put(&mut h, note, "minecraft:noteblock");
    h.packets();

    use_block(&mut h, at(3, 5));
    h.tick(2);
    // Stone below: bass drum (1), pitch 0.
    assert_eq!(notes(&mut h), vec![(note, 1, 0)]);
    h.set(note + IVec3::Z, state("minecraft:stone", &[]));
    h.tick(2);
    assert!(notes(&mut h).is_empty(), "no new edge");
    use_block(&mut h, at(3, 5));
    h.tick(2);
    assert!(notes(&mut h).is_empty(), "falling edge");
    use_block(&mut h, at(3, 5));
    h.tick(2);
    assert_eq!(notes(&mut h), vec![(note, 1, 0)]);

    // Wood below plays the bass; a block above silences it.
    let mut wood = harness();
    let note = at(4, 5);
    wood.put(note - IVec3::Y, state("minecraft:oak_planks", &[]));
    lever(&mut wood, at(3, 5), "up_east_west");
    put(&mut wood, note, "minecraft:noteblock");
    wood.packets();
    use_block(&mut wood, at(3, 5));
    wood.tick(2);
    assert_eq!(notes(&mut wood), vec![(note, 4, 0)]);
    use_block(&mut wood, at(3, 5));
    wood.tick(2);
    wood.set(note + IVec3::Y, state("minecraft:stone", &[]));
    use_block(&mut wood, at(3, 5));
    wood.tick(2);
    assert!(notes(&mut wood).is_empty());
}

#[test]
fn note_block_placed_in_power_stays_silent() {
    let mut h = harness();
    lever(&mut h, at(3, 5), "up_east_west");
    use_block(&mut h, at(3, 5));
    h.tick(2);
    h.packets();
    h.set(at(4, 5), state("minecraft:noteblock", &[]));
    h.tick(2);
    assert!(notes(&mut h).is_empty());
}

#[test]
fn door_placed_in_power_stays_closed_until_the_power_changes() {
    let mut h = harness();
    let door = at(4, 5);
    lever(&mut h, at(3, 5), "up_east_west");
    use_block(&mut h, at(3, 5));
    h.tick(2);
    let half = |upper: u8| {
        state(
            "minecraft:wooden_door",
            &[
                ("upper_block_bit", StateValue::Byte(upper)),
                ("minecraft:cardinal_direction", StateValue::Str("east")),
            ],
        )
    };
    super::super::deferred(&mut h.world, |world| {
        crate::server::game::blocks::set_block(world, door, half(0), false);
        crate::server::game::blocks::set_block(world, door + IVec3::Y, half(1), false);
    });
    h.tick(2);
    assert!(!open(&h, door));
    use_block(&mut h, at(3, 5));
    h.tick(2);
    assert!(!open(&h, door));
    use_block(&mut h, at(3, 5));
    h.tick(2);
    assert!(open(&h, door));
}

#[test]
fn redstone_items_place_dust_repeaters_and_comparators() {
    for (item, block) in [
        ("minecraft:redstone", "minecraft:redstone_wire"),
        ("minecraft:repeater", "minecraft:unpowered_repeater"),
        ("minecraft:comparator", "minecraft:unpowered_comparator"),
    ] {
        let mut h = Harness::new();
        h.stand(DVec3::new(0.5, Y as f64 + 1.62, 3.5), 180.0, 30.0);
        h.hold(item, 4);
        h.click(at(0, 0) - IVec3::Y, 1, DVec3::new(0.5, 1.0, 0.5));
        assert_eq!(h.name(at(0, 0)), block, "{item}");
    }
}
