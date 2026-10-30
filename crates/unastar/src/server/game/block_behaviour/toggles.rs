//! Trapdoors, fence gates, buttons and levers: blocks a player's `use`
//! switches, as BDS 1.26.51 does.
//!
//! - **Trapdoors** (`direction` 0..=3, `open_bit`, `upside_down_bit`) face
//!   away from the placing player: `direction` is 3, 0, 2, 1 for a player
//!   looking south, west, north, east, so the open panel lies against the
//!   far side of the cell (the clicked block when the player looks at a
//!   side face). A click on the bottom face places the trapdoor in the top
//!   half, a click on the top face in the bottom half, and a click on a
//!   side face in the half of the face that was clicked. Use flips
//!   `open_bit`; iron trapdoors ignore hands (redstone only).
//! - **Fence gates** (`minecraft:cardinal_direction`, `open_bit`,
//!   `in_wall_bit`) face the way the placing player looks (provisional:
//!   Minecraft Wiki; the rotation of the placement direction was not
//!   measured). `in_wall_bit` is set while a wall stands on either side
//!   along the gate (west/east of a gate facing north or south,
//!   north/south of one facing west or east) and is recomputed one tick
//!   after a neighbour update. Opening by a player who looks the opposite way of
//!   the gate turns the gate to face the player's look, so it swings away
//!   from them; closing keeps the direction.
//! - **Buttons** (`button_pressed_bit`) stay pressed for 30 ticks (wooden)
//!   or 20 ticks (every other button), then release on a scheduled tick.
//!   Using a pressed button consumes the click and changes nothing.
//!   Arrows do not press or hold wooden buttons (projectiles in blocks are
//!   not modelled).
//! - **Levers** (`open_bit`) flip on every use.
//!
//! Every switch plays its LevelSoundEvent at the block centre carrying the
//! new state's wire id (`trapdoor.open`/`trapdoor.close`,
//! `fence_gate.open`/`fence_gate.close`, `button.click_on` when a button
//! or lever turns on, `button.click_off` when it turns off).
//!
//! Power ([`super::redstone`]): a lever that is on and a pressed button
//! power the block they are attached to strongly and their other
//! neighbours weakly with 15. Trapdoors (iron ones too) and fence gates
//! open when they become powered and close when they lose power, with
//! the same sounds as a use; a fence gate keeps its direction
//! ([`power_trapdoor`], [`power_fence_gate`]).

use bevy_ecs::prelude::*;
use glam::IVec3;

use super::{Place, bit, with};
use crate::entity::components::Rotation;
use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::placement::Horizontal;

/// Switch state properties.
pub const OPEN: &str = "open_bit";
pub const TRAPDOOR_DIRECTION: &str = "direction";
pub const UPSIDE_DOWN: &str = "upside_down_bit";
pub const GATE_DIRECTION: &str = "minecraft:cardinal_direction";
pub const IN_WALL: &str = "in_wall_bit";
pub const PRESSED: &str = "button_pressed_bit";

/// Ticks a wooden button stays pressed.
pub const WOODEN_BUTTON_TICKS: u64 = 30;
/// Ticks every other button stays pressed.
pub const STONE_BUTTON_TICKS: u64 = 20;

/// Placement state of a trapdoor (see the module docs).
pub fn trapdoor_placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    let block = place.block;
    let looking = Horizontal::from_yaw(place.input.yaw);
    // The trapdoor faces away from the player: looking south gives 3, west
    // 0, north 2, east 1, which is the stairs numbering of the opposite.
    let direction = looking.opposite().weirdo();
    let upside_down = match place.input.face {
        0 => true,
        1 => false,
        _ => place.input.click.y > 0.5,
    };
    let mut state = with(
        block,
        place.state,
        TRAPDOOR_DIRECTION,
        StateValue::Int(direction),
    );
    state = with(
        block,
        state,
        UPSIDE_DOWN,
        StateValue::Byte(u8::from(upside_down)),
    );
    Some(with(block, state, OPEN, StateValue::Byte(0)))
}

/// Placement state of a fence gate (see the module docs).
pub fn fence_gate_placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    let block = place.block;
    let facing = Horizontal::from_yaw(place.input.yaw);
    let mut state = with(
        block,
        place.state,
        GATE_DIRECTION,
        StateValue::Str(facing.cardinal()),
    );
    state = with(block, state, OPEN, StateValue::Byte(0));
    let in_wall = in_wall(place.blocks, place.position, facing);
    Some(with(
        block,
        state,
        IN_WALL,
        StateValue::Byte(u8::from(in_wall)),
    ))
}

/// The direction a fence gate state faces.
fn gate_direction(entry: &BlockEntry, state: u32) -> Option<Horizontal> {
    match entry.state_value(state, GATE_DIRECTION)? {
        StateValue::Str(value) => Horizontal::from_cardinal(value),
        _ => None,
    }
}

/// Whether a wall stands beside a gate at `pos` facing `facing`, on either
/// side along the gate.
fn in_wall(blocks: &impl BlockSource, pos: IVec3, facing: Horizontal) -> bool {
    let side = facing.clockwise().offset();
    is_wall(blocks, pos + side) || is_wall(blocks, pos - side)
}

fn is_wall(blocks: &impl BlockSource, pos: IVec3) -> bool {
    blocks.block_at(pos).is_some_and(|state| {
        blocks
            .registry()
            .get_by_runtime_id(state)
            .is_some_and(|entry| entry.state_value(state, "wall_post_bit").is_some())
    })
}

/// The scheduled tick after a neighbour of the fence gate at `pos` changed:
/// recompute `in_wall_bit`.
pub fn fence_gate_neighbour_changed(world: &mut World, pos: IVec3, state: u32) {
    let next = {
        let Some(entry) = world.registry().get_by_runtime_id(state) else {
            return;
        };
        let Some(facing) = gate_direction(entry, state) else {
            return;
        };
        let in_wall = in_wall(&*world, pos, facing);
        with(entry, state, IN_WALL, StateValue::Byte(u8::from(in_wall)))
    };
    if next != state {
        crate::server::game::blocks::set_block(world, pos, next, false);
    }
}

/// Open or close the trapdoor at `pos`.
pub fn toggle_trapdoor(world: &mut World, pos: IVec3, state: u32) -> bool {
    let Some((next, open)) = flipped(world, state, OPEN) else {
        return false;
    };
    let sound = if open {
        "trapdoor.open"
    } else {
        "trapdoor.close"
    };
    switch(world, pos, next, sound);
    true
}

/// Open or close the fence gate at `pos`; `player` opening it from the
/// side it faces turns it to swing away from them.
pub fn toggle_fence_gate(world: &mut World, player: Entity, pos: IVec3, state: u32) -> bool {
    let looking = world
        .get::<Rotation>(player)
        .map(|rotation| Horizontal::from_yaw(rotation.yaw));
    let (next, open) = {
        let Some(entry) = world.registry().get_by_runtime_id(state) else {
            return false;
        };
        let open = !bit(entry, state, OPEN);
        let mut next = with(entry, state, OPEN, StateValue::Byte(u8::from(open)));
        if open
            && let Some(looking) = looking
            && gate_direction(entry, state) == Some(looking.opposite())
        {
            next = with(
                entry,
                next,
                GATE_DIRECTION,
                StateValue::Str(looking.cardinal()),
            );
        }
        (next, open)
    };
    let sound = if open {
        "fence_gate.open"
    } else {
        "fence_gate.close"
    };
    switch(world, pos, next, sound);
    true
}

/// Open (`open`) or close the trapdoor at `pos` by power; nothing when it
/// already is.
pub fn power_trapdoor(world: &mut World, pos: IVec3, state: u32, open: bool) {
    let is_open = world
        .registry()
        .get_by_runtime_id(state)
        .is_some_and(|entry| bit(entry, state, OPEN));
    if is_open != open {
        toggle_trapdoor(world, pos, state);
    }
}

/// Open (`open`) or close the fence gate at `pos` by power, keeping its
/// direction; nothing when it already is.
pub fn power_fence_gate(world: &mut World, pos: IVec3, state: u32, open: bool) {
    let Some((next, now_open)) = flipped(world, state, OPEN) else {
        return;
    };
    if now_open != open {
        return;
    }
    let sound = if open {
        "fence_gate.open"
    } else {
        "fence_gate.close"
    };
    switch(world, pos, next, sound);
}

/// Press the button at `pos` and schedule its release. A pressed button
/// consumes the click without change.
pub fn press_button(world: &mut World, pos: IVec3, state: u32, wooden: bool) -> bool {
    let Some((next, pressed)) = flipped(world, state, PRESSED) else {
        return false;
    };
    if !pressed {
        return true;
    }
    switch(world, pos, next, "button.click_on");
    let ticks = if wooden {
        WOODEN_BUTTON_TICKS
    } else {
        STONE_BUTTON_TICKS
    };
    super::schedule_tick(world, pos, ticks);
    true
}

/// Flip the lever at `pos`.
pub fn toggle_lever(world: &mut World, pos: IVec3, state: u32) -> bool {
    let Some((next, on)) = flipped(world, state, OPEN) else {
        return false;
    };
    let sound = if on {
        "button.click_on"
    } else {
        "button.click_off"
    };
    switch(world, pos, next, sound);
    true
}

/// The scheduled release of the pressed button at `pos`.
pub fn release_button(world: &mut World, pos: IVec3, state: u32) {
    let Some((next, pressed)) = flipped(world, state, PRESSED) else {
        return;
    };
    if pressed {
        return;
    }
    switch(world, pos, next, "button.click_off");
}

/// `state` with the bit `property` inverted, and the new value.
fn flipped(world: &World, state: u32, property: &str) -> Option<(u32, bool)> {
    let entry = world.registry().get_by_runtime_id(state)?;
    let on = !bit(entry, state, property);
    Some((
        with(entry, state, property, StateValue::Byte(u8::from(on))),
        on,
    ))
}

/// Change the block at `pos` to `next` and play `sound` at its centre for
/// the viewers, carrying the new state's wire id.
fn switch(world: &mut World, pos: IVec3, next: u32, sound: &str) {
    crate::server::game::blocks::set_block(world, pos, next, false);
    let wire = crate::server::game::types::block_network_ids(world).to_wire(next);
    crate::entity::mob::net::sound::play_level_sound(
        world,
        None,
        super::centre(pos),
        sound,
        wire as i32,
    );
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use jolyne::valentine::McpePacketData;

    use super::super::tests::{FLOOR, Harness, state};
    use super::*;
    use crate::entity::systems::physics::tests::TestBlocks;
    use crate::registry::BlockRegistry;
    use crate::server::game::placement::PlacementInput;

    const AT: IVec3 = IVec3::new(0, FLOOR + 1, 0);

    fn value(state: u32, property: &str) -> Option<StateValue> {
        BlockRegistry::vanilla()
            .get_by_runtime_id(state)?
            .state_value(state, property)
    }

    /// The state a click places `name` at `position` with.
    fn placed(blocks: &TestBlocks, name: &str, input: PlacementInput, position: IVec3) -> u32 {
        let entry = BlockRegistry::vanilla().get_by_name(name).expect(name);
        super::super::placement_state(blocks, entry, entry.default_state_id, &input, position)
            .expect("placement accepted")
    }

    /// A click on `face` of the block next to the origin cell, at height
    /// `click_y` within the face, looking with `yaw`.
    fn click(face: u8, click_y: f64, yaw: f32) -> PlacementInput {
        let normal = crate::server::game::placement::face_offset(face).unwrap();
        PlacementInput {
            clicked: -normal,
            face,
            click: DVec3::new(0.5, click_y, 0.5),
            yaw,
            pitch: 0.0,
        }
    }

    /// The `(sound, data)` of every LevelSoundEvent the player received.
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

    fn wire(h: &Harness, state: u32) -> i32 {
        crate::server::game::types::block_network_ids(&h.world).to_wire(state) as i32
    }

    fn use_block(h: &mut Harness, pos: IVec3) -> bool {
        super::super::use_block(&mut h.world, h.player, pos)
    }

    #[test]
    fn trapdoor_faces_away_from_the_player_and_takes_the_clicked_half() {
        let blocks = TestBlocks::default();
        let cases = [
            // (face, click y, yaw, direction, upside down)
            (5, 0.25, 90.0, 0, 0),
            (5, 0.75, 90.0, 0, 1),
            (4, 0.75, 270.0, 1, 1),
            (3, 0.25, 180.0, 2, 0),
            (2, 0.25, 0.0, 3, 0),
            (1, 0.9, 180.0, 2, 0),
            (0, 0.1, 270.0, 1, 1),
        ];
        for (face, y, yaw, direction, upside_down) in cases {
            let trapdoor = placed(
                &blocks,
                "minecraft:trapdoor",
                click(face, y, yaw),
                IVec3::ZERO,
            );
            assert_eq!(
                value(trapdoor, TRAPDOOR_DIRECTION),
                Some(StateValue::Int(direction)),
                "face {face} yaw {yaw}"
            );
            assert_eq!(
                value(trapdoor, UPSIDE_DOWN),
                Some(StateValue::Byte(upside_down)),
                "face {face} click y {y}"
            );
            assert_eq!(value(trapdoor, OPEN), Some(StateValue::Byte(0)));
        }
    }

    #[test]
    fn open_trapdoor_lies_against_the_clicked_block() {
        let blocks = TestBlocks::default();
        // Each side face, clicked while looking into it.
        for (face, yaw) in [(2, 0.0), (3, 180.0), (4, 270.0), (5, 90.0)] {
            let trapdoor = placed(
                &blocks,
                "minecraft:trapdoor",
                click(face, 0.25, yaw),
                IVec3::ZERO,
            );
            let entry = BlockRegistry::vanilla()
                .get_by_name("minecraft:trapdoor")
                .unwrap();
            let open = with(entry, trapdoor, OPEN, StateValue::Byte(1));
            let boxes: Vec<_> = crate::registry::collision::block_collision_aabbs(open).collect();
            assert_eq!(boxes.len(), 1, "face {face}");
            let panel = boxes[0];
            // The clicked block lies at -normal: the panel spans the 0.1825
            // slab of the cell on that side.
            let normal = crate::server::game::placement::face_offset(face)
                .unwrap()
                .as_dvec3();
            let axis = if normal.x != 0.0 { 0 } else { 2 };
            let (min, max) = (panel.min[axis], panel.max[axis]);
            if normal[axis] > 0.0 {
                assert!(min == 0.0 && max < 0.2, "face {face}: {panel:?}");
            } else {
                assert!(min > 0.8 && max == 1.0, "face {face}: {panel:?}");
            }
            assert_eq!((panel.min.y, panel.max.y), (0.0, 1.0), "face {face}");
        }
    }

    #[test]
    fn using_a_trapdoor_opens_and_closes_it_with_its_sound() {
        let mut h = Harness::new();
        h.put(AT, state("minecraft:trapdoor", &[]));
        h.packets();

        assert!(use_block(&mut h, AT));
        assert_eq!(h.value(AT, OPEN), Some(StateValue::Byte(1)));
        let open = h.state(AT);
        assert_eq!(
            sounds(&mut h),
            vec![("trapdoor.open".into(), wire(&h, open))]
        );

        assert!(use_block(&mut h, AT));
        assert_eq!(h.value(AT, OPEN), Some(StateValue::Byte(0)));
        let closed = h.state(AT);
        assert_eq!(
            sounds(&mut h),
            vec![("trapdoor.close".into(), wire(&h, closed))]
        );
    }

    #[test]
    fn fence_gate_faces_the_look_and_sits_in_walls_beside_it() {
        let mut blocks = TestBlocks::default();
        blocks.set(IVec3::X, "minecraft:cobblestone_wall");
        let north = placed(
            &blocks,
            "minecraft:fence_gate",
            click(1, 0.5, 180.0),
            IVec3::ZERO,
        );
        assert_eq!(value(north, GATE_DIRECTION), Some(StateValue::Str("north")));
        assert_eq!(value(north, IN_WALL), Some(StateValue::Byte(1)));
        // Facing east the gate spans north–south; the wall to the east is
        // in front of it, not beside it.
        let east = placed(
            &blocks,
            "minecraft:fence_gate",
            click(1, 0.5, 270.0),
            IVec3::ZERO,
        );
        assert_eq!(value(east, GATE_DIRECTION), Some(StateValue::Str("east")));
        assert_eq!(value(east, IN_WALL), Some(StateValue::Byte(0)));
    }

    #[test]
    fn fence_gate_follows_walls_placed_and_removed_beside_it() {
        let mut h = Harness::new();
        h.put(
            AT,
            state(
                "minecraft:fence_gate",
                &[(GATE_DIRECTION, StateValue::Str("north"))],
            ),
        );
        h.set(AT - IVec3::X, state("minecraft:cobblestone_wall", &[]));
        // The gate follows one tick after the neighbour update.
        assert_eq!(h.value(AT, IN_WALL), Some(StateValue::Byte(0)));
        h.tick(1);
        assert_eq!(h.value(AT, IN_WALL), Some(StateValue::Byte(1)));
        h.set(AT - IVec3::X, state("minecraft:air", &[]));
        h.tick(1);
        assert_eq!(h.value(AT, IN_WALL), Some(StateValue::Byte(0)));
        // A wall in front of the gate is not beside it.
        h.set(AT + IVec3::Z, state("minecraft:cobblestone_wall", &[]));
        h.tick(1);
        assert_eq!(h.value(AT, IN_WALL), Some(StateValue::Byte(0)));
    }

    #[test]
    fn fence_gate_swings_away_from_a_player_facing_it() {
        let mut h = Harness::new();
        let gate = |direction| {
            state(
                "minecraft:fence_gate",
                &[(GATE_DIRECTION, StateValue::Str(direction))],
            )
        };
        let eye = DVec3::new(0.5, 66.0, 3.5);
        // Looking north at a gate facing south: it turns north and opens.
        h.put(AT, gate("south"));
        h.stand(eye, 180.0, 0.0);
        h.packets();
        assert!(use_block(&mut h, AT));
        assert_eq!(h.value(AT, GATE_DIRECTION), Some(StateValue::Str("north")));
        assert_eq!(h.value(AT, OPEN), Some(StateValue::Byte(1)));
        let open = h.state(AT);
        assert_eq!(
            sounds(&mut h),
            vec![("fence_gate.open".into(), wire(&h, open))]
        );
        // Closing keeps the direction, whichever way the player looks.
        h.stand(eye, 0.0, 0.0);
        assert!(use_block(&mut h, AT));
        assert_eq!(h.value(AT, GATE_DIRECTION), Some(StateValue::Str("north")));
        assert_eq!(h.value(AT, OPEN), Some(StateValue::Byte(0)));
        let closed = h.state(AT);
        assert_eq!(
            sounds(&mut h),
            vec![("fence_gate.close".into(), wire(&h, closed))]
        );
        // Looking along or across the gate keeps its direction.
        for yaw in [180.0, 90.0] {
            h.put(AT, gate("north"));
            h.stand(eye, yaw, 0.0);
            assert!(use_block(&mut h, AT));
            assert_eq!(h.value(AT, GATE_DIRECTION), Some(StateValue::Str("north")));
            assert_eq!(h.value(AT, OPEN), Some(StateValue::Byte(1)));
        }
    }

    /// Press the button `name` lying on the floor and check it releases
    /// after exactly `ticks`.
    fn button_releases_after(name: &str, ticks: u64) {
        let mut h = Harness::new();
        h.put(AT, state(name, &[("facing_direction", StateValue::Int(1))]));
        h.packets();
        assert!(use_block(&mut h, AT));
        assert_eq!(h.value(AT, PRESSED), Some(StateValue::Byte(1)));
        let pressed = h.state(AT);
        assert_eq!(
            sounds(&mut h),
            vec![("button.click_on".into(), wire(&h, pressed))]
        );
        // Using it again while pressed is consumed without a change.
        assert!(use_block(&mut h, AT));
        assert_eq!(h.state(AT), pressed);
        assert!(sounds(&mut h).is_empty());

        h.tick(ticks - 1);
        assert_eq!(h.value(AT, PRESSED), Some(StateValue::Byte(1)), "{name}");
        h.tick(1);
        assert_eq!(h.value(AT, PRESSED), Some(StateValue::Byte(0)), "{name}");
        let released = h.state(AT);
        assert_eq!(
            sounds(&mut h),
            vec![("button.click_off".into(), wire(&h, released))]
        );
    }

    #[test]
    fn wooden_button_releases_after_30_ticks() {
        button_releases_after("minecraft:wooden_button", 30);
    }

    #[test]
    fn stone_button_releases_after_20_ticks() {
        button_releases_after("minecraft:stone_button", 20);
    }

    #[test]
    fn lever_flips_with_a_click_on_and_off() {
        let mut h = Harness::new();
        h.put(
            AT,
            state(
                "minecraft:lever",
                &[("lever_direction", StateValue::Str("up_east_west"))],
            ),
        );
        h.packets();
        assert!(use_block(&mut h, AT));
        assert_eq!(h.value(AT, OPEN), Some(StateValue::Byte(1)));
        let on = h.state(AT);
        assert_eq!(
            sounds(&mut h),
            vec![("button.click_on".into(), wire(&h, on))]
        );
        assert!(use_block(&mut h, AT));
        assert_eq!(h.value(AT, OPEN), Some(StateValue::Byte(0)));
        let off = h.state(AT);
        assert_eq!(
            sounds(&mut h),
            vec![("button.click_off".into(), wire(&h, off))]
        );
    }
}
