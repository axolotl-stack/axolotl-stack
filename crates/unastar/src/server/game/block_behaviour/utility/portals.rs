//! End portal frames and nether portals.
//!
//! # End portal frames
//!
//! A frame (`end_portal_eye_bit`, `minecraft:cardinal_direction`) is placed
//! facing the player (the generic placement state).
//!
//! - **Using** an ender eye on a frame without an eye sets
//!   `end_portal_eye_bit`; any other use, or a frame that already holds an
//!   eye, does nothing (the click goes on to item use and placement). Outside
//!   creative the eye is used up. Then:
//!   1. when the frame completes a ring, the 3×3 inside the ring at the
//!      frames' height becomes `end_portal` and viewers hear
//!      `block.end_portal.spawn` at the centre cell + (0.5, 1.0, 0.5);
//!   2. four smoke particles rise at the frame's top, at
//!      `x + 0.5 + nextFloat × 0.4 − 0.2`, `y + 1`,
//!      `z + 0.5 + nextFloat × 0.4 − 0.2`;
//!   3. viewers hear `block.end_portal_frame.fill` at the frame + (0.5, 1.0,
//!      0.5), whether or not a portal formed.
//! - **A ring** is the 12 cells two blocks from a 3×3 centre on each side,
//!   corners excluded: every one an `end_portal_frame` holding an eye and
//!   facing the centre. The inside is not checked; the eye's frame is on the
//!   ring the inserted frame faces into.
//! - **Breaking** a frame of a complete ring (creative only: frames cannot
//!   be broken in survival) turns the ring's `end_portal` cells back to air.
//!
//! Provisional: the sound and particle data (−1 for the sounds, 0 for the
//! smoke `LevelEvent` 16389, legacy particle 5).
//!
//! # Nether portals
//!
//! - **Lighting**: fire placed in the overworld or the nether
//!   ([`try_light`], from fire's placement; flint and steel and fire charges
//!   light portals by placing fire) evaluates a frame for axis x, then z,
//!   and fills the first that is valid and holds no portal block yet with
//!   `portal` blocks of that `portal_axis`, all at once.
//! - **A frame** ([`evaluate`]): "empty" cells are air, fire or portal.
//!   From the cell, go down while the cell below is empty (at most 21 cells,
//!   not below the world). Along the frame width (axis x: left is east,
//!   right is west; axis z: left is north, right is south), the distance to
//!   an edge steps up to 22 cells while the cell is empty with obsidian
//!   under it, and counts only when it ends at obsidian. The bottom-left
//!   cell is one short of the left edge; the width (to the right edge) must
//!   be 2..=21. Rows then count up while every cell of the row is empty
//!   with obsidian left of the first and right of the last column (at most
//!   21); the row above must be obsidian across the width; the height must
//!   be 3..=21. Corners are never checked.
//! - **Survival**: a portal block that was just set re-checks its frame one
//!   tick later; a neighbour change re-checks at once (one tick later while
//!   any chunk within 16 blocks is not loaded). The portal stays when its
//!   frame (for its own axis; unknown counts as x) is valid and filled with
//!   portal blocks; otherwise every portal block connected to it in its
//!   plane becomes air together, without drops or particles. Breaking a
//!   frame block or a portal block therefore removes the whole portal.
//! - **Not modelled**: travelling through a portal (an entity inside a
//!   portal block stays where it is), portal records for linking, zombified
//!   piglins spawning from portal blocks.

use std::collections::VecDeque;

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use rand::Rng;

use super::super::{Place, air, bit, deferred, name_at, random, schedule_tick, with};
use crate::entity::components::GameMode;
use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::blocks::set_block;
use crate::server::game::item_use::{held, set_slot};
use crate::server::game::placement::Horizontal;
use crate::server::game::types::BlockRegistryResource;

pub const FRAME: &str = "minecraft:end_portal_frame";
pub const END_PORTAL: &str = "minecraft:end_portal";
pub const PORTAL: &str = "minecraft:portal";
const OBSIDIAN: &str = "minecraft:obsidian";
const ENDER_EYE: &str = "minecraft:ender_eye";

/// Frame state properties.
pub const EYE: &str = "end_portal_eye_bit";
pub const FACING: &str = "minecraft:cardinal_direction";
/// Portal state property.
pub const AXIS: &str = "portal_axis";

/// Smoke particles over a frame that took an eye.
const EYE_SMOKE_COUNT: usize = 4;
/// `LevelEvent` of a legacy particle (added to the particle id).
const LEVEL_EVENT_LEGACY_PARTICLE: i32 = 0x4000;
/// Legacy particle id of smoke.
const PARTICLE_SMOKE: i32 = 5;

/// Nether portal frame bounds (inner size).
pub const MIN_WIDTH: i32 = 2;
pub const MAX_WIDTH: i32 = 21;
pub const MIN_HEIGHT: i32 = 3;
pub const MAX_HEIGHT: i32 = 21;
/// Steps taken looking for a frame edge.
const EDGE_SEARCH: i32 = 22;
/// Cells a frame evaluation walks down from its start.
const MAX_DESCENT: i32 = 21;
/// Chunks within this many blocks must be loaded for a portal to re-check
/// its frame.
const LOADED_RADIUS: i32 = 16;

/// Whether a player's `use` on `state` is a block interaction: a frame
/// without an eye takes one.
pub fn is_usable(entry: &BlockEntry, state: u32) -> bool {
    entry.string_id == FRAME && !bit(entry, state, EYE)
}

/// A player's `use` (with whatever they hold). Returns whether the block
/// reacted (then nothing is placed).
pub fn use_block(world: &mut World, player: Entity, pos: IVec3, state: u32) -> bool {
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return false;
    };
    if entry.string_id != FRAME || bit(entry, state, EYE) {
        return false;
    }
    let Some((slot, stack)) = held(world, player) else {
        return false;
    };
    if stack.item_id() != ENDER_EYE {
        return false;
    }
    set_block(
        world,
        pos,
        with(entry, state, EYE, StateValue::Byte(1)),
        false,
    );
    if let Some(centre) = end_portal_centre(world, pos) {
        fill_end_portal(world, centre);
        level_sound(
            world,
            centre.as_dvec3() + DVec3::new(0.5, 1.0, 0.5),
            "block.end_portal.spawn",
        );
    }
    if world.get::<GameMode>(player) != Some(&GameMode::Creative) {
        set_slot(world, player, slot, stack.grow(-1));
    }
    for _ in 0..EYE_SMOKE_COUNT {
        let (dx, dz) = {
            let mut rng = random(world);
            (rng.r#gen::<f32>(), rng.r#gen::<f32>())
        };
        let at = DVec3::new(
            f64::from(pos.x) + 0.5 + f64::from(dx * 0.4 - 0.2),
            f64::from(pos.y) + 1.0,
            f64::from(pos.z) + 0.5 + f64::from(dz * 0.4 - 0.2),
        );
        crate::entity::mob::net::sound::level_event(
            world,
            at,
            LEVEL_EVENT_LEGACY_PARTICLE | PARTICLE_SMOKE,
            0,
        );
    }
    level_sound(
        world,
        pos.as_dvec3() + DVec3::new(0.5, 1.0, 0.5),
        "block.end_portal_frame.fill",
    );
    true
}

/// Placement keeps the generic state (frames face the player).
pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    Some(place.state)
}

/// Frames and portal blocks need no support; a portal's frame is checked
/// by its neighbour updates, which remove the whole portal at once.
pub fn can_survive(
    _blocks: &impl BlockSource,
    _pos: IVec3,
    _state: u32,
    _entry: &BlockEntry,
) -> bool {
    true
}

/// A portal block re-checks its frame one tick after it is set.
pub fn placed(world: &mut World, pos: IVec3, _state: u32) {
    if name_at(world, pos) == Some(PORTAL) {
        schedule_tick(world, pos, 1);
    }
}

/// A portal block re-checks its frame.
pub fn neighbour_changed(world: &mut World, pos: IVec3, state: u32, _from: IVec3) {
    check_portal(world, pos, state);
}

/// A portal block re-checks its frame.
pub fn scheduled_tick(world: &mut World, pos: IVec3, state: u32) {
    check_portal(world, pos, state);
}

/// Random ticks do not reach frames or portals (portal mob spawning is not
/// modelled).
pub fn random_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Redstone does not act on frames or portals.
pub fn power_changed(_world: &mut World, _pos: IVec3, _state: u32, _powered: bool) {}

/// Breaking a frame of a complete ring closes its end portal.
pub fn breaking(world: &mut World, pos: IVec3, _state: u32) {
    let Some(centre) = end_portal_centre(world, pos) else {
        return;
    };
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let air = air(&registry);
    deferred(world, |world| {
        for cell in inside(centre) {
            if name_at(world, cell) == Some(END_PORTAL) {
                set_block(world, cell, air, false);
            }
        }
    });
}

fn level_sound(world: &mut World, at: DVec3, name: &str) {
    crate::server::game::blocks::queue_level_sound(world, None, at, name, -1);
}

/// The direction a frame faces.
fn frame_facing(entry: &BlockEntry, state: u32) -> Option<Horizontal> {
    match entry.state_value(state, FACING)? {
        StateValue::Str(value) => Horizontal::from_cardinal(value),
        _ => None,
    }
}

/// The 3×3 inside a ring around `centre`.
fn inside(centre: IVec3) -> impl Iterator<Item = IVec3> {
    (-1..=1).flat_map(move |dx| (-1..=1).map(move |dz| centre + IVec3::new(dx, 0, dz)))
}

/// The centre of the complete ring the frame at `pos` belongs to: the ring
/// lies on the side the frame faces, with the frame on one of the three
/// cells of that side.
fn end_portal_centre(blocks: &impl BlockSource, pos: IVec3) -> Option<IVec3> {
    let state = blocks.block_at(pos)?;
    let entry = blocks.registry().get_by_runtime_id(state)?;
    if entry.string_id != FRAME {
        return None;
    }
    let facing = frame_facing(entry, state)?;
    let along = facing.clockwise().offset();
    (-1..=1)
        .map(|k| pos + facing.offset() * 2 + along * k)
        .find(|centre| ring_complete(blocks, *centre))
}

/// Whether the 12 ring cells around `centre` all hold frames with eyes
/// facing the centre.
fn ring_complete(blocks: &impl BlockSource, centre: IVec3) -> bool {
    let registry = blocks.registry();
    [
        Horizontal::South,
        Horizontal::West,
        Horizontal::North,
        Horizontal::East,
    ]
    .into_iter()
    .all(|side| {
        let along = side.clockwise().offset();
        (-1..=1).all(|k| {
            let cell = centre + side.offset() * 2 + along * k;
            blocks
                .block_at(cell)
                .and_then(|state| Some((registry.get_by_runtime_id(state)?, state)))
                .is_some_and(|(entry, state)| {
                    entry.string_id == FRAME
                        && bit(entry, state, EYE)
                        && frame_facing(entry, state) == Some(side.opposite())
                })
        })
    })
}

/// Fill the 3×3 inside the ring around `centre` with end portal blocks.
fn fill_end_portal(world: &mut World, centre: IVec3) {
    let Some(portal) = world
        .resource::<BlockRegistryResource>()
        .0
        .default_state_id_by_name(END_PORTAL)
    else {
        return;
    };
    deferred(world, |world| {
        for cell in inside(centre) {
            set_block(world, cell, portal, false);
        }
    });
}

/// The plane a nether portal spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Along x (the frame width runs east–west).
    X,
    /// Along z (the frame width runs north–south).
    Z,
}

impl Axis {
    /// `portal_axis` value.
    pub fn name(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Z => "z",
        }
    }

    /// The axis of a portal state (unknown counts as x).
    fn of(entry: &BlockEntry, state: u32) -> Self {
        match entry.state_value(state, AXIS) {
            Some(StateValue::Str("z")) => Self::Z,
            _ => Self::X,
        }
    }

    /// The left and right steps along the frame width.
    fn sides(self) -> (IVec3, IVec3) {
        match self {
            Self::X => (IVec3::X, IVec3::NEG_X),
            Self::Z => (IVec3::NEG_Z, IVec3::Z),
        }
    }
}

/// A valid nether portal frame's inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    pub axis: Axis,
    /// The lowest cell of the inside's first (leftmost) column.
    pub bottom_left: IVec3,
    pub width: i32,
    pub height: i32,
    /// Portal blocks found inside.
    pub portals: i32,
}

impl Shape {
    /// Every cell of the inside.
    fn cells(&self) -> impl Iterator<Item = IVec3> + '_ {
        let right = self.axis.sides().1;
        (0..self.width).flat_map(move |i| {
            (0..self.height).map(move |j| self.bottom_left + right * i + IVec3::Y * j)
        })
    }
}

fn is_empty(name: Option<&str>) -> bool {
    matches!(name, Some("minecraft:air" | "minecraft:fire" | PORTAL))
}

fn is_obsidian(blocks: &impl BlockSource, pos: IVec3) -> bool {
    name_at(blocks, pos) == Some(OBSIDIAN)
}

/// Steps from `from` towards `step` to the frame edge (0 when none).
fn distance_to_edge(blocks: &impl BlockSource, from: IVec3, step: IVec3) -> i32 {
    let mut i = 0;
    while i < EDGE_SEARCH {
        let cell = from + step * i;
        if !is_empty(name_at(blocks, cell)) || !is_obsidian(blocks, cell - IVec3::Y) {
            break;
        }
        i += 1;
    }
    if is_obsidian(blocks, from + step * i) {
        i
    } else {
        0
    }
}

/// The nether portal frame of `axis` around `pos`, if it is valid.
pub fn evaluate(blocks: &impl BlockSource, pos: IVec3, axis: Axis) -> Option<Shape> {
    let mut start = pos;
    while start.y > pos.y - MAX_DESCENT
        && start.y > crate::world::chunk::MIN_Y
        && is_empty(name_at(blocks, start - IVec3::Y))
    {
        start -= IVec3::Y;
    }
    let (left, right) = axis.sides();
    let to_left = distance_to_edge(blocks, start, left) - 1;
    if to_left < 0 {
        return None;
    }
    let bottom_left = start + left * to_left;
    let width = distance_to_edge(blocks, bottom_left, right);
    if !(MIN_WIDTH..=MAX_WIDTH).contains(&width) {
        return None;
    }
    let mut portals = 0;
    let mut height = 0;
    'rows: while height < MAX_HEIGHT {
        for i in 0..width {
            let cell = bottom_left + right * i + IVec3::Y * height;
            let name = name_at(blocks, cell);
            if !is_empty(name) {
                break 'rows;
            }
            if name == Some(PORTAL) {
                portals += 1;
            }
            if (i == 0 && !is_obsidian(blocks, cell + left))
                || (i == width - 1 && !is_obsidian(blocks, cell + right))
            {
                break 'rows;
            }
        }
        height += 1;
    }
    let top_closed =
        (0..width).all(|i| is_obsidian(blocks, bottom_left + right * i + IVec3::Y * height));
    (top_closed && (MIN_HEIGHT..=MAX_HEIGHT).contains(&height)).then_some(Shape {
        axis,
        bottom_left,
        width,
        height,
        portals,
    })
}

/// Fire was placed at `pos`: light the nether portal frame around it (axis
/// x first, then z) when it is valid and empty of portal blocks, in the
/// overworld and the nether. Returns whether a portal formed (the fire is
/// then gone).
pub fn try_light(world: &mut World, pos: IVec3) -> bool {
    if super::respawn_anchor::dimension(world) == super::respawn_anchor::END {
        return false;
    }
    let Some(shape) = [Axis::X, Axis::Z]
        .into_iter()
        .filter_map(|axis| evaluate(world, pos, axis))
        .find(|shape| shape.portals == 0)
    else {
        return false;
    };
    let portal = {
        let registry = &world.resource::<BlockRegistryResource>().0;
        let Some(entry) = registry.get_by_name(PORTAL) else {
            return false;
        };
        with(
            entry,
            entry.default_state_id,
            AXIS,
            StateValue::Str(shape.axis.name()),
        )
    };
    deferred(world, |world| {
        for cell in shape.cells() {
            set_block(world, cell, portal, false);
        }
    });
    true
}

/// Whether every chunk within [`LOADED_RADIUS`] of `pos` is loaded.
fn surroundings_loaded(world: &World, pos: IVec3) -> bool {
    [-LOADED_RADIUS, LOADED_RADIUS].into_iter().all(|dx| {
        [-LOADED_RADIUS, LOADED_RADIUS]
            .into_iter()
            .all(|dz| world.block_at(pos + IVec3::new(dx, 0, dz)).is_some())
    })
}

/// Keep the portal block at `pos` only while its frame is valid and full of
/// portal blocks; otherwise remove the whole portal.
fn check_portal(world: &mut World, pos: IVec3, state: u32) {
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return;
    };
    if entry.string_id != PORTAL {
        return;
    }
    if !surroundings_loaded(world, pos) {
        schedule_tick(world, pos, 1);
        return;
    }
    let intact = evaluate(world, pos, Axis::of(entry, state))
        .is_some_and(|shape| shape.portals == shape.width * shape.height);
    if !intact {
        remove_portal(world, pos, Axis::of(entry, state));
    }
}

/// Turn every portal block connected to `pos` in its plane into air.
fn remove_portal(world: &mut World, pos: IVec3, axis: Axis) {
    let air = air(&world.resource::<BlockRegistryResource>().0);
    let (left, right) = axis.sides();
    let steps = [IVec3::Y, IVec3::NEG_Y, left, right];
    let limit = (MAX_WIDTH * MAX_HEIGHT) as usize;
    let mut cells = Vec::new();
    let mut queue = VecDeque::from([pos]);
    while let Some(cell) = queue.pop_front() {
        if cells.len() >= limit || cells.contains(&cell) || name_at(world, cell) != Some(PORTAL) {
            continue;
        }
        cells.push(cell);
        queue.extend(steps.iter().map(|step| cell + *step));
    }
    deferred(world, |world| {
        for cell in cells {
            set_block(world, cell, air, false);
        }
    });
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::super::super::tests::{FLOOR, Harness, state};
    use super::*;
    use crate::item::ItemStack;

    /// The ring's centre: frames at y = FLOOR + 1 around (8, _, 8).
    const CENTRE: IVec3 = IVec3::new(8, FLOOR + 1, 8);

    fn frame(facing: Horizontal, eye: bool) -> u32 {
        state(
            FRAME,
            &[
                (FACING, StateValue::Str(facing.cardinal())),
                (EYE, StateValue::Byte(u8::from(eye))),
            ],
        )
    }

    /// The 12 ring cells with the direction their frame must face.
    fn ring() -> Vec<(IVec3, Horizontal)> {
        [
            Horizontal::South,
            Horizontal::West,
            Horizontal::North,
            Horizontal::East,
        ]
        .into_iter()
        .flat_map(|side| {
            let along = side.clockwise().offset();
            (-1..=1).map(move |k| (CENTRE + side.offset() * 2 + along * k, side.opposite()))
        })
        .collect()
    }

    fn held_count(h: &Harness) -> u8 {
        h.world
            .get::<crate::entity::components::MainInventory>(h.player)
            .unwrap()
            .0
            .item(0)
            .map_or(0, |stack: &ItemStack| stack.count)
    }

    fn insert_eye(h: &mut Harness, at: IVec3) -> bool {
        let state = h.state(at);
        use_block(&mut h.world, h.player, at, state)
    }

    #[test]
    fn the_twelfth_eye_opens_the_end_portal() {
        let mut h = Harness::new();
        let ring = ring();
        let (last, last_facing) = ring[7];
        for (cell, facing) in &ring {
            h.put(*cell, frame(*facing, *cell != last));
        }
        h.hold(ENDER_EYE, 2);
        h.packets();
        assert!(insert_eye(&mut h, last));
        assert_eq!(h.state(last), frame(last_facing, true));
        for cell in inside(CENTRE) {
            assert_eq!(h.name(cell), END_PORTAL);
        }
        assert_eq!(held_count(&h), 1);
        let sounds: Vec<String> = h
            .packets()
            .into_iter()
            .filter_map(|packet| match packet {
                jolyne::valentine::McpePacketData::LevelSoundEventPacket(sound) => {
                    Some(sound.sound_event.to_string())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            sounds,
            ["block.end_portal.spawn", "block.end_portal_frame.fill"]
        );
    }

    #[test]
    fn eleven_eyes_do_not_open_the_portal() {
        let mut h = Harness::new();
        let ring = ring();
        for (cell, facing) in &ring[..11] {
            h.put(*cell, frame(*facing, false));
        }
        let (missing, facing) = ring[11];
        h.put(missing, frame(facing, false));
        h.hold(ENDER_EYE, 64);
        for (cell, _) in &ring[..11] {
            assert!(insert_eye(&mut h, *cell));
        }
        for cell in inside(CENTRE) {
            assert_eq!(h.name(cell), "minecraft:air");
        }
        assert_eq!(held_count(&h), 64 - 11);
        assert!(!insert_eye(&mut h, ring[0].0), "a frame takes one eye");
    }

    #[test]
    fn a_frame_facing_away_spoils_the_ring() {
        let mut h = Harness::new();
        let ring = ring();
        for (i, (cell, facing)) in ring.iter().enumerate() {
            let facing = if i == 4 { facing.opposite() } else { *facing };
            h.put(*cell, frame(facing, i != 0));
        }
        h.hold(ENDER_EYE, 1);
        assert!(insert_eye(&mut h, ring[0].0));
        assert_eq!(h.name(CENTRE), "minecraft:air");
    }

    #[test]
    fn only_ender_eyes_fill_frames_and_creative_keeps_them() {
        let mut h = Harness::new();
        let at = CENTRE;
        h.put(at, frame(Horizontal::North, false));
        h.hold("minecraft:stone", 1);
        assert!(!insert_eye(&mut h, at));
        h.hold(ENDER_EYE, 1);
        *h.world.get_mut::<GameMode>(h.player).unwrap() = GameMode::Creative;
        assert!(insert_eye(&mut h, at));
        assert_eq!(held_count(&h), 1);
    }

    #[test]
    fn breaking_a_ring_frame_closes_the_end_portal() {
        let mut h = Harness::new();
        let ring = ring();
        for (cell, facing) in &ring {
            h.put(*cell, frame(*facing, true));
        }
        for cell in inside(CENTRE) {
            h.put(cell, state(END_PORTAL, &[]));
        }
        let at = ring[3].0;
        let state = h.state(at);
        breaking(&mut h.world, at, state);
        for cell in inside(CENTRE) {
            assert_eq!(h.name(cell), "minecraft:air");
        }
    }

    /// Build an obsidian frame with a 2×3 inside whose bottom-left inner
    /// cell is `origin`, spanning `right` (corners included).
    fn obsidian_frame(h: &mut Harness, origin: IVec3, right: IVec3) {
        let obsidian = state(OBSIDIAN, &[]);
        for i in -1..=2 {
            for j in -1..=3 {
                if (0..2).contains(&i) && (0..3).contains(&j) {
                    continue;
                }
                h.put(origin + right * i + IVec3::Y * j, obsidian);
            }
        }
    }

    fn portal_cells(origin: IVec3, right: IVec3) -> Vec<IVec3> {
        (0..2)
            .flat_map(|i| (0..3).map(move |j| origin + right * i + IVec3::Y * j))
            .collect()
    }

    fn portal(axis: &'static str) -> u32 {
        state(PORTAL, &[(AXIS, StateValue::Str(axis))])
    }

    #[test]
    fn fire_inside_a_frame_fills_it_with_portal_blocks_of_its_axis() {
        for (right, axis) in [(IVec3::NEG_X, "x"), (IVec3::Z, "z")] {
            let mut h = Harness::new();
            let origin = IVec3::new(8, FLOOR + 1, 8);
            obsidian_frame(&mut h, origin, right);
            h.set(origin + IVec3::Y, state("minecraft:fire", &[]));
            for cell in portal_cells(origin, right) {
                assert_eq!(h.state(cell), portal(axis), "{axis} {cell}");
            }
            h.tick(2);
            for cell in portal_cells(origin, right) {
                assert_eq!(h.state(cell), portal(axis), "{axis} {cell} after a tick");
            }
        }
    }

    #[test]
    fn breaking_a_frame_block_removes_the_whole_portal() {
        let mut h = Harness::new();
        let origin = IVec3::new(8, FLOOR + 1, 8);
        obsidian_frame(&mut h, origin, IVec3::NEG_X);
        h.set(origin, state("minecraft:fire", &[]));
        assert_eq!(h.name(origin + IVec3::Y * 2), PORTAL);
        h.set(origin + IVec3::X + IVec3::Y, state("minecraft:air", &[]));
        for cell in portal_cells(origin, IVec3::NEG_X) {
            assert_eq!(h.name(cell), "minecraft:air", "{cell}");
        }
        assert!(h.drops().is_empty());
    }

    #[test]
    fn flint_and_steel_on_the_inside_lights_the_frame() {
        let mut h = Harness::new();
        let origin = IVec3::new(2, FLOOR + 1, 2);
        obsidian_frame(&mut h, origin, IVec3::Z);
        h.hold("minecraft:flint_and_steel", 1);
        h.stand(DVec3::new(3.5, FLOOR as f64 + 2.62, 3.5), 90.0, 30.0);
        h.click(origin - IVec3::Y, 1, DVec3::new(0.5, 1.0, 0.5));
        for cell in portal_cells(origin, IVec3::Z) {
            assert_eq!(h.state(cell), portal("z"), "{cell}");
        }
    }

    #[test]
    fn an_open_or_crowded_frame_does_not_light() {
        let mut h = Harness::new();
        let origin = IVec3::new(8, FLOOR + 1, 8);
        obsidian_frame(&mut h, origin, IVec3::NEG_X);
        // A missing side block leaves the frame open.
        h.put(origin + IVec3::X + IVec3::Y, state("minecraft:air", &[]));
        assert!(!try_light(&mut h.world, origin));
        // A block inside it fills it.
        h.put(origin + IVec3::X + IVec3::Y, state(OBSIDIAN, &[]));
        h.put(origin + IVec3::Y * 2, state("minecraft:stone", &[]));
        assert!(!try_light(&mut h.world, origin));
        assert_eq!(h.name(origin), "minecraft:air");
    }

    #[test]
    fn frames_measure_their_inside() {
        let mut h = Harness::new();
        let origin = IVec3::new(8, FLOOR + 1, 8);
        obsidian_frame(&mut h, origin, IVec3::Z);
        let shape = evaluate(&h.world, origin + IVec3::new(0, 2, 1), Axis::Z).unwrap();
        assert_eq!(
            shape,
            Shape {
                axis: Axis::Z,
                bottom_left: origin,
                width: 2,
                height: 3,
                portals: 0,
            }
        );
        assert_eq!(evaluate(&h.world, origin, Axis::X), None);
    }

    #[test]
    fn a_lone_portal_block_goes_one_tick_after_it_is_set() {
        let mut h = Harness::new();
        let at = IVec3::new(8, FLOOR + 1, 8);
        h.set(at, portal("x"));
        assert_eq!(h.name(at), PORTAL);
        h.tick(1);
        assert_eq!(h.name(at), "minecraft:air");
    }
}

/// Replays of the BDS 1.26.51 utility capture
/// (`testdata/bds_1_26_51_utility.json`, the bds-sweep `utility` job)
/// against unastar: the watched cells after each step, the step's sounds,
/// level events, chat messages and spawn positions. The capture's arena
/// floor is at y = -61, the harness floor at [`FLOOR`], so every position
/// is shifted by the difference. Every replayed step changed its cells in
/// one server tick in the capture (counted from the click landing); here
/// the same cells change before the click returns.
#[cfg(test)]
pub(super) mod capture {
    use glam::{DVec3, IVec3};
    use jolyne::valentine::McpePacketData;
    use serde_json::Value;

    use super::super::super::tests::{FLOOR, Harness};
    use crate::entity::systems::BlockSource;
    use crate::registry::block::StateValue;

    const FIXTURE: &str = include_str!("../testdata/bds_1_26_51_utility.json");
    /// The capture's floor height.
    const BDS_FLOOR: i32 = -61;

    pub fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture is JSON")
    }

    pub fn step<'a>(fixture: &'a Value, group: &str, name: &str) -> &'a Value {
        fixture[group]
            .as_array()
            .unwrap()
            .iter()
            .find(|step| step["step"] == name)
            .unwrap_or_else(|| panic!("{group}/{name} captured"))
    }

    fn number(value: &Value) -> f64 {
        value.as_f64().unwrap()
    }

    pub fn shift(pos: &Value) -> IVec3 {
        let at = |i: usize| pos[i].as_i64().unwrap() as i32;
        IVec3::new(at(0), at(1) - BDS_FLOOR + FLOOR, at(2))
    }

    pub fn shift_point(pos: &Value) -> DVec3 {
        DVec3::new(
            number(&pos[0]),
            number(&pos[1]) - f64::from(BDS_FLOOR) + f64::from(FLOOR),
            number(&pos[2]),
        )
    }

    /// `name[k=v,...]` in the capture's notation (keys sorted, bytes as
    /// booleans, strings quoted), or the bare name without states.
    pub fn notation(state: u32) -> String {
        let registry = crate::registry::BlockRegistry::vanilla();
        let entry = registry.get_by_runtime_id(state).unwrap();
        let mut states: Vec<String> = entry
            .data
            .properties
            .iter()
            .map(|property| {
                let value = match entry.state_value(state, property.name).unwrap() {
                    StateValue::Byte(b) => (b != 0).to_string(),
                    StateValue::Int(i) => i.to_string(),
                    StateValue::Str(s) => format!("\"{s}\""),
                };
                format!("{}={value}", property.name)
            })
            .collect();
        states.sort();
        if states.is_empty() {
            return entry.string_id.clone();
        }
        format!("{}[{}]", entry.string_id, states.join(","))
    }

    /// The state a capture notation names.
    pub fn parse(wanted: &str) -> u32 {
        let name = wanted.split('[').next().unwrap();
        let entry = crate::registry::BlockRegistry::vanilla()
            .get_by_name(name)
            .unwrap_or_else(|| panic!("{name} is a block"));
        (entry.min_state_id..=entry.max_state_id)
            .find(|state| notation(*state) == wanted)
            .unwrap_or_else(|| panic!("no state {wanted}"))
    }

    /// The position of a cell label: `x,y,z` or one of the step's named
    /// positions.
    pub fn cell_pos(step: &Value, label: &str) -> IVec3 {
        if let Some(pos) = step["positions"].get(label) {
            return shift(pos);
        }
        let parts: Vec<Value> = label
            .split(',')
            .map(|part| Value::from(part.parse::<i64>().unwrap()))
            .collect();
        shift(&Value::from(parts))
    }

    /// Write the step's `before` cells.
    pub fn put_before(h: &mut Harness, step: &Value) {
        for (label, cell) in step["before"].as_object().unwrap() {
            h.put(cell_pos(step, label), parse(cell.as_str().unwrap()));
        }
    }

    /// Compare the step's `after` cells (all, or only `labels`).
    pub fn assert_after(h: &Harness, step: &Value, labels: Option<&[&str]>) {
        let name = step["step"].as_str().unwrap();
        for (label, cell) in step["after"].as_object().unwrap() {
            if labels.is_some_and(|labels| !labels.contains(&label.as_str())) {
                continue;
            }
            let ours = notation(h.world.block_at(cell_pos(step, label)).unwrap());
            assert_eq!(ours, cell.as_str().unwrap(), "{name}: cell {label}");
        }
        let ticks: Vec<i64> = step["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|change| change["tick"].as_i64().unwrap())
            .collect();
        assert!(ticks.len() <= 1, "{name}: captured changes in one tick");
    }

    /// Hold `held` (or nothing), stand where the step's player stood and
    /// make its click. Returns every packet the player got.
    pub fn click(h: &mut Harness, step: &Value, held: Option<(&str, u8)>) -> Vec<McpePacketData> {
        match held {
            Some((item, count)) => h.hold(item, count),
            None => {
                h.world
                    .get_mut::<crate::entity::components::MainInventory>(h.player)
                    .unwrap()
                    .0
                    .set_item(0, crate::item::ItemStack::empty())
                    .unwrap();
            }
        }
        let action = &step["use"];
        let player = &action["player"];
        h.stand(
            shift_point(&player["feet"]) + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT,
            number(&player["yaw"]) as f32,
            number(&player["pitch"]) as f32,
        );
        let face = match action["face"].as_str().unwrap() {
            "down" => 0,
            "up" => 1,
            "north" => 2,
            "south" => 3,
            "west" => 4,
            _ => 5,
        };
        let at = &action["click"];
        h.packets();
        h.click(
            shift(&action["clicked"]),
            face,
            DVec3::new(number(&at[0]), number(&at[1]), number(&at[2])),
        );
        h.packets()
    }

    /// The selected stack's count (0 when empty).
    pub fn held_count(h: &Harness) -> u8 {
        h.world
            .get::<crate::entity::components::MainInventory>(h.player)
            .unwrap()
            .0
            .item(0)
            .map_or(0, |stack| stack.count)
    }

    fn rounded(point: DVec3) -> [i64; 3] {
        [point.x, point.y, point.z].map(|c| (c * 100.0).round() as i64)
    }

    /// Sounds as `(name, position, data)`.
    pub fn sounds(packets: &[McpePacketData]) -> Vec<(String, [i64; 3], i32)> {
        packets
            .iter()
            .filter_map(|packet| match packet {
                McpePacketData::LevelSoundEventPacket(sound) => Some((
                    sound.sound_event.to_string(),
                    rounded(DVec3::new(
                        f64::from(sound.position.x),
                        f64::from(sound.position.y),
                        f64::from(sound.position.z),
                    )),
                    sound.data,
                )),
                _ => None,
            })
            .collect()
    }

    pub fn captured_sounds(step: &Value) -> Vec<(String, [i64; 3], i32)> {
        step["sounds"]
            .as_array()
            .map(|sounds| {
                sounds
                    .iter()
                    .map(|sound| {
                        (
                            sound["sound"].as_str().unwrap().to_string(),
                            rounded(shift_point(&sound["position"])),
                            sound["data"].as_i64().unwrap() as i32,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Level events as `(id, position, data)`.
    pub fn level_events(packets: &[McpePacketData]) -> Vec<(i32, [i64; 3], i32)> {
        packets
            .iter()
            .filter_map(|packet| match packet {
                McpePacketData::LevelEventPacket(event) => Some((
                    event.event_id,
                    rounded(DVec3::new(
                        f64::from(event.position.x),
                        f64::from(event.position.y),
                        f64::from(event.position.z),
                    )),
                    event.data,
                )),
                _ => None,
            })
            .collect()
    }

    pub fn captured_level_events(step: &Value) -> Vec<(i32, [i64; 3], i32)> {
        step["level_events"]
            .as_array()
            .map(|events| {
                events
                    .iter()
                    .map(|event| {
                        (
                            event["event"].as_i64().unwrap() as i32,
                            rounded(shift_point(&event["position"])),
                            event["data"].as_i64().unwrap() as i32,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn the_last_eye_matches_the_bds_capture() {
        use super::*;
        let fixture = fixture();
        for name in ["inward_last_eye", "outward_last_eye"] {
            let step = step(&fixture, "end_portal", name);
            let mut h = Harness::new();
            put_before(&mut h, step);
            let last = cell_pos(step, "last_frame");
            let last_state = h.state(last);
            let registry = crate::registry::BlockRegistry::vanilla();
            let entry = registry.get_by_runtime_id(last_state).unwrap();
            let facing = frame_facing(entry, last_state).unwrap();
            // The ring around the watched 3×3, every frame turned the way
            // the last one is (into or out of the ring).
            let centre = shift(&serde_json::json!([2, -60, 2]));
            let inward = last + facing.offset() * 2 == centre;
            for side in [
                Horizontal::South,
                Horizontal::West,
                Horizontal::North,
                Horizontal::East,
            ] {
                let along = side.clockwise().offset();
                for k in -1..=1 {
                    let cell = centre + side.offset() * 2 + along * k;
                    if cell != last {
                        let towards = if inward { side.opposite() } else { side };
                        let frame = with(
                            entry,
                            entry.default_state_id,
                            FACING,
                            StateValue::Str(towards.cardinal()),
                        );
                        h.put(cell, with(entry, frame, EYE, StateValue::Byte(1)));
                    }
                }
            }
            let packets = click(&mut h, step, Some((ENDER_EYE, 2)));
            assert_after(&h, step, None);
            assert_eq!(sounds(&packets), captured_sounds(step), "{name}: sounds");
            assert_eq!(held_count(&h), 1, "{name}: held eyes");
        }
    }

    #[test]
    fn nether_portal_lighting_and_breaking_match_the_bds_capture() {
        use super::*;
        let fixture = fixture();
        let mut h = Harness::new();
        // The frame: inner x 1..=2, y -59..=-57 at z 0, corners included.
        let obsidian = super::super::super::tests::state(OBSIDIAN, &[]);
        for x in 0..=3 {
            for y in -60..=-56 {
                if !(1..=2).contains(&x) || y == -60 || y == -56 {
                    h.put(shift(&serde_json::json!([x, y, 0])), obsidian);
                }
            }
        }
        let lit = step(&fixture, "nether_portal", "flint_and_steel");
        put_before(&mut h, lit);
        let packets = click(&mut h, lit, Some(("minecraft:flint_and_steel", 1)));
        assert_after(&h, lit, None);
        assert_eq!(sounds(&packets), captured_sounds(lit), "lighting sounds");
        let removed = step(&fixture, "nether_portal", "frame_removed");
        assert_eq!(
            removed["commands"][0], "setblock 0 -58 0 air",
            "the replayed command"
        );
        h.set(
            cell_pos(removed, "removed_frame"),
            super::super::super::tests::state("minecraft:air", &[]),
        );
        assert_after(&h, removed, None);
    }
}
