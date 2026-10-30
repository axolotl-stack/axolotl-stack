//! Rails, powered rails, detector rails and activator rails.
//!
//! As BDS 1.26.51 does:
//! - **Shapes.** `rail_direction` 0 north–south, 1 east–west, 2..5 sloped
//!   up towards east, west, north and south, 6..9 the curves south–east,
//!   south–west, north–west and north–east. A rail connects to the two
//!   cells its shape names (a slope's raised end one block up).
//! - **Placement.** A neighbour rail (in the cell beside, or one up or down
//!   from it) counts when it already connects back or has fewer than two
//!   connections whose rails connect back. Powered, detector and activator
//!   rails only run straight: east–west when a west or east neighbour
//!   counts, else north–south when a north or south one does. A plain rail
//!   with exactly two perpendicular neighbours curves between them;
//!   otherwise it starts from that straight result and the curves win in
//!   the order south–east, south–west, north–east, north–west (unpowered)
//!   or north–west, north–east, south–west, south–east (powered). A
//!   straight result slopes up towards a rail one block up at its end
//!   (south over north, west over east). Then every rail the new shape
//!   names that may connect turns to it, with the same rules from its own
//!   connections.
//! - **Support.** Placing needs a full top face below; staying needs the
//!   top below to hold an edge. A rail that loses it breaks with its drop a
//!   tick later.
//! - **Powered and activator rails** set `rail_data_bit` while powered:
//!   power reaching one directly, or through up to 8 connected rails of
//!   the same kind and line that are powered themselves.
//! - **Detector rails** are pressed (`rail_data_bit`) while a minecart's
//!   box meets the box `(x + 0.2, y, z + 0.2)..(x + 0.8, y + 0.8, z + 0.8)`,
//!   checked when a minecart enters ([`entity_inside`]) and every 20 ticks
//!   while pressed; incoming power does nothing to them.
//! - **Minecarts.** Using a minecart item on a rail ([`use_block`]) places
//!   the matching minecart actor, unmoving, with its feet 0.15 above the
//!   rail's cell at its centre.
//!
//! Not available yet: minecart actors cannot be spawned (their definitions
//! use components the entity runtime does not implement), so
//! [`place_minecart`] reports failure, a minecart item used on a rail does
//! nothing, a dispenser ejects minecart items instead, and no minecart moves
//! onto a detector rail ([`entity_inside`] has no caller yet).
//!
//! Provisional (Minecraft Wiki): a plain rail where three neighbour rails
//! meet re-picks its shape with the powered preference whenever power is
//! re-evaluated; a minecart on a sloped rail stands 0.5 higher; sloped
//! rails' support at their raised side is not checked.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use rand::Rng;

use super::super::{Place, SupportKind, bit, int, provides_support, schedule_tick, with};
use crate::entity::components::{GameMode, PhysicsBody, Position};
use crate::entity::mob::{MobKind, SpawnCause, spawn_mob};
use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};

/// The shape state.
pub const RAIL_DIRECTION: &str = "rail_direction";
/// Powered (powered and activator rails) or pressed (detector rails).
pub const RAIL_DATA: &str = "rail_data_bit";
/// Rails a powered rail's power runs along beyond the one powered directly.
pub const POWER_REACH: u32 = 8;
/// Ticks between a pressed detector rail's checks.
pub const DETECTOR_RECHECK: u64 = 20;
/// Height of a placed minecart's feet above its rail's cell.
pub const MINECART_LIFT: f64 = 0.15;

const NORTH_SOUTH: i32 = 0;
const EAST_WEST: i32 = 1;
const ASCENDING_EAST: i32 = 2;
const ASCENDING_WEST: i32 = 3;
const ASCENDING_NORTH: i32 = 4;
const ASCENDING_SOUTH: i32 = 5;
const SOUTH_EAST: i32 = 6;
const SOUTH_WEST: i32 = 7;
const NORTH_WEST: i32 = 8;
const NORTH_EAST: i32 = 9;

const NORTH: IVec3 = IVec3::NEG_Z;
const SOUTH: IVec3 = IVec3::Z;
const WEST: IVec3 = IVec3::NEG_X;
const EAST: IVec3 = IVec3::X;

/// Which rail a block is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RailKind {
    Plain,
    Powered,
    Detector,
    Activator,
}

impl RailKind {
    pub fn of(entry: &BlockEntry) -> Option<Self> {
        Some(match entry.string_id.as_str() {
            "minecraft:rail" => Self::Plain,
            "minecraft:golden_rail" => Self::Powered,
            "minecraft:detector_rail" => Self::Detector,
            "minecraft:activator_rail" => Self::Activator,
            _ => return None,
        })
    }

    /// Powered, detector and activator rails never curve.
    fn straight_only(self) -> bool {
        self != Self::Plain
    }
}

/// The rail at `pos`: its kind and shape.
fn rail(blocks: &impl BlockSource, pos: IVec3) -> Option<(RailKind, i32)> {
    let state = blocks.block_at(pos)?;
    let entry = blocks.registry().get_by_runtime_id(state)?;
    Some((RailKind::of(entry)?, int(entry, state, RAIL_DIRECTION)?))
}

/// The rail meant by a connection to `pos`: there, else one up, else one
/// down.
fn rail_near(blocks: &impl BlockSource, pos: IVec3) -> Option<(IVec3, RailKind, i32)> {
    [pos, pos + IVec3::Y, pos - IVec3::Y]
        .into_iter()
        .find_map(|at| rail(blocks, at).map(|(kind, shape)| (at, kind, shape)))
}

/// The two cells the shape `shape` at `pos` connects.
fn ends(pos: IVec3, shape: i32) -> [IVec3; 2] {
    let up = IVec3::Y;
    match shape {
        EAST_WEST => [pos + WEST, pos + EAST],
        ASCENDING_EAST => [pos + WEST, pos + EAST + up],
        ASCENDING_WEST => [pos + WEST + up, pos + EAST],
        ASCENDING_NORTH => [pos + NORTH + up, pos + SOUTH],
        ASCENDING_SOUTH => [pos + NORTH, pos + SOUTH + up],
        SOUTH_EAST => [pos + EAST, pos + SOUTH],
        SOUTH_WEST => [pos + WEST, pos + SOUTH],
        NORTH_WEST => [pos + WEST, pos + NORTH],
        NORTH_EAST => [pos + EAST, pos + NORTH],
        _ => [pos + NORTH, pos + SOUTH],
    }
}

fn same_column(a: IVec3, b: IVec3) -> bool {
    a.x == b.x && a.z == b.z
}

/// The connections of the rail `shape` at `pos` whose rails connect back.
fn firm_ends(blocks: &impl BlockSource, pos: IVec3, shape: i32) -> Vec<IVec3> {
    ends(pos, shape)
        .into_iter()
        .filter(|end| {
            rail_near(blocks, *end).is_some_and(|(at, _, other)| {
                ends(at, other).iter().any(|back| same_column(*back, pos))
            })
        })
        .collect()
}

/// Whether the rail meant by `pos` may connect to a rail at `to`: it
/// already does, or it has fewer than two firm connections.
fn may_connect(blocks: &impl BlockSource, pos: IVec3, to: IVec3) -> bool {
    let Some((at, _, shape)) = rail_near(blocks, pos) else {
        return false;
    };
    let firm = firm_ends(blocks, at, shape);
    firm.iter().any(|end| same_column(*end, to)) || firm.len() != 2
}

fn is_rail(blocks: &impl BlockSource, pos: IVec3) -> bool {
    rail(blocks, pos).is_some()
}

/// A straight shape slopes up towards a rail one block up at its end
/// (south over north, west over east).
fn sloped(blocks: &impl BlockSource, pos: IVec3, shape: i32) -> i32 {
    let up = IVec3::Y;
    let mut shape = shape;
    if shape == NORTH_SOUTH {
        if is_rail(blocks, pos + NORTH + up) {
            shape = ASCENDING_NORTH;
        }
        if is_rail(blocks, pos + SOUTH + up) {
            shape = ASCENDING_SOUTH;
        }
    } else if shape == EAST_WEST {
        if is_rail(blocks, pos + EAST + up) {
            shape = ASCENDING_EAST;
        }
        if is_rail(blocks, pos + WEST + up) {
            shape = ASCENDING_WEST;
        }
    }
    shape
}

/// The shape a rail of `kind` at `pos` takes when placed (or re-picked),
/// `current` when nothing decides.
fn place_shape(
    blocks: &impl BlockSource,
    pos: IVec3,
    kind: RailKind,
    current: i32,
    powered: bool,
) -> i32 {
    let [n, s, w, e] = [NORTH, SOUTH, WEST, EAST].map(|side| may_connect(blocks, pos + side, pos));
    let straight = if w || e {
        EAST_WEST
    } else if n || s {
        NORTH_SOUTH
    } else {
        current
    };
    let shape = if kind.straight_only() {
        straight
    } else {
        let only = [
            (s && e && !n && !w, SOUTH_EAST),
            (s && w && !n && !e, SOUTH_WEST),
            (n && w && !s && !e, NORTH_WEST),
            (n && e && !s && !w, NORTH_EAST),
        ];
        let preference = if powered {
            [
                (n && w, NORTH_WEST),
                (n && e, NORTH_EAST),
                (s && w, SOUTH_WEST),
                (s && e, SOUTH_EAST),
            ]
        } else {
            [
                (s && e, SOUTH_EAST),
                (s && w, SOUTH_WEST),
                (n && e, NORTH_EAST),
                (n && w, NORTH_WEST),
            ]
        };
        only.into_iter()
            .chain(preference)
            .find_map(|(holds, curve)| holds.then_some(curve))
            .unwrap_or(straight)
    };
    if matches!(shape, NORTH_SOUTH | EAST_WEST) {
        sloped(blocks, pos, shape)
    } else {
        shape
    }
}

/// The shape the rail of `kind` at `pos` (shape `current`) takes when it
/// turns to a new rail at `to`.
fn turn_shape(
    blocks: &impl BlockSource,
    pos: IVec3,
    kind: RailKind,
    current: i32,
    to: IVec3,
) -> i32 {
    let mut joined = firm_ends(blocks, pos, current);
    joined.push(to);
    let has = |side: IVec3| joined.iter().any(|end| same_column(*end, pos + side));
    let [n, s, w, e] = [NORTH, SOUTH, WEST, EAST].map(has);
    let mut shape = None;
    if n || s {
        shape = Some(NORTH_SOUTH);
    }
    if w || e {
        shape = Some(EAST_WEST);
    }
    if !kind.straight_only() {
        for (holds, curve) in [
            (s && e && !n && !w, SOUTH_EAST),
            (s && w && !n && !e, SOUTH_WEST),
            (n && w && !s && !e, NORTH_WEST),
            (n && e && !s && !w, NORTH_EAST),
        ] {
            if holds {
                shape = Some(curve);
            }
        }
    }
    match shape {
        Some(straight @ (NORTH_SOUTH | EAST_WEST)) => sloped(blocks, pos, straight),
        Some(curve) => curve,
        None => current,
    }
}

/// Every rail the rail at `pos` (shape `shape`) names that may connect to
/// it turns to it.
fn join_neighbours(world: &mut World, pos: IVec3, shape: i32) {
    for end in ends(pos, shape) {
        let Some((at, kind, current)) = rail_near(world, end) else {
            continue;
        };
        if !may_connect(world, at, pos) {
            continue;
        }
        let turned = turn_shape(world, at, kind, current, pos);
        if turned != current {
            set_shape(world, at, turned);
        }
    }
}

/// Write `shape` into the rail at `pos`.
fn set_shape(world: &mut World, pos: IVec3, shape: i32) {
    let Some(state) = world.block_at(pos) else {
        return;
    };
    let Some(entry) = world.registry().get_by_runtime_id(state) else {
        return;
    };
    let new = with(entry, state, RAIL_DIRECTION, StateValue::Int(shape));
    if new != state {
        crate::server::game::blocks::set_block(world, pos, new, false);
    }
}

/// Set `rail_data_bit` of the rail at `pos`; returns whether it changed.
fn set_data(world: &mut World, pos: IVec3, on: bool) -> bool {
    let Some(state) = world.block_at(pos) else {
        return false;
    };
    let Some(entry) = world.registry().get_by_runtime_id(state) else {
        return false;
    };
    if bit(entry, state, RAIL_DATA) == on {
        return false;
    }
    let new = with(entry, state, RAIL_DATA, StateValue::Byte(u8::from(on)));
    crate::server::game::blocks::set_block(world, pos, new, false);
    true
}

/// Whether the powered or activator rail at `pos` has power: directly, or
/// along its line through powered rails of its kind.
fn rail_powered(world: &World, pos: IVec3, kind: RailKind, shape: i32) -> bool {
    super::super::redstone::power_at(world, pos) > 0
        || powered_along(world, pos, kind, shape, true, 0)
        || powered_along(world, pos, kind, shape, false, 0)
}

/// Whether power reaches the rail at `pos` from one direction of its line,
/// `depth` rails away from where the search started.
fn powered_along(
    world: &World,
    pos: IVec3,
    kind: RailKind,
    shape: i32,
    forward: bool,
    depth: u32,
) -> bool {
    if depth >= POWER_REACH {
        return false;
    }
    let up = IVec3::Y;
    let (next, line, may_drop) = match shape {
        NORTH_SOUTH if forward => (pos + SOUTH, NORTH_SOUTH, true),
        NORTH_SOUTH => (pos + NORTH, NORTH_SOUTH, true),
        EAST_WEST if forward => (pos + WEST, EAST_WEST, true),
        EAST_WEST => (pos + EAST, EAST_WEST, true),
        ASCENDING_EAST if forward => (pos + WEST, EAST_WEST, true),
        ASCENDING_EAST => (pos + EAST + up, EAST_WEST, false),
        ASCENDING_WEST if forward => (pos + WEST + up, EAST_WEST, false),
        ASCENDING_WEST => (pos + EAST, EAST_WEST, true),
        ASCENDING_NORTH if forward => (pos + SOUTH, NORTH_SOUTH, true),
        ASCENDING_NORTH => (pos + NORTH + up, NORTH_SOUTH, false),
        ASCENDING_SOUTH if forward => (pos + SOUTH + up, NORTH_SOUTH, false),
        ASCENDING_SOUTH => (pos + NORTH, NORTH_SOUTH, true),
        _ => return false,
    };
    passes_power(world, next, kind, line, forward, depth)
        || (may_drop && passes_power(world, next - up, kind, line, forward, depth))
}

/// Whether the rail at `pos` is a powered rail of `kind` on the line `line`
/// that carries power from further along.
fn passes_power(
    world: &World,
    pos: IVec3,
    kind: RailKind,
    line: i32,
    forward: bool,
    depth: u32,
) -> bool {
    let Some(state) = world.block_at(pos) else {
        return false;
    };
    let Some(entry) = world.registry().get_by_runtime_id(state) else {
        return false;
    };
    if RailKind::of(entry) != Some(kind) {
        return false;
    }
    let Some(shape) = int(entry, state, RAIL_DIRECTION) else {
        return false;
    };
    let crosses = match line {
        EAST_WEST => matches!(shape, NORTH_SOUTH | ASCENDING_NORTH | ASCENDING_SOUTH),
        _ => matches!(shape, EAST_WEST | ASCENDING_EAST | ASCENDING_WEST),
    };
    if crosses || !bit(entry, state, RAIL_DATA) {
        return false;
    }
    super::super::redstone::power_at(world, pos) > 0
        || powered_along(world, pos, kind, shape, forward, depth + 1)
}

/// Re-read the power of the powered or activator rail at `pos`; when it
/// changed, the rails along its line re-read theirs.
fn update_power(world: &mut World, pos: IVec3) {
    let Some((kind, shape)) = rail(world, pos) else {
        return;
    };
    if !matches!(kind, RailKind::Powered | RailKind::Activator) {
        return;
    }
    let powered = rail_powered(world, pos, kind, shape);
    if !set_data(world, pos, powered) {
        return;
    }
    for end in ends(pos, shape) {
        for at in [end, end + IVec3::Y, end - IVec3::Y] {
            if rail(world, at).is_some_and(|(other, _)| other == kind) {
                update_power(world, at);
            }
        }
    }
}

/// Whether an actor box `width` wide and `height` tall with its feet at
/// `feet` meets the detecting box of the detector rail at `pos`.
fn meets_detector(pos: IVec3, feet: DVec3, width: f64, height: f64) -> bool {
    let min = pos.as_dvec3() + DVec3::new(0.2, 0.0, 0.2);
    let max = pos.as_dvec3() + DVec3::new(0.8, 0.8, 0.8);
    let half = width / 2.0;
    feet.x - half < max.x
        && feet.x + half > min.x
        && feet.y < max.y
        && feet.y + height > min.y
        && feet.z - half < max.z
        && feet.z + half > min.z
}

/// Whether a minecart's box meets the detecting box of the detector rail at
/// `pos`.
fn minecart_on(world: &mut World, pos: IVec3) -> bool {
    let mut carts = world.query::<(&MobKind, &Position, Option<&PhysicsBody>)>();
    carts.iter(world).any(|(kind, position, body)| {
        let (width, height) = body.map_or((0.98, 0.7), |b| (b.width, b.height));
        kind.0.identifier.ends_with("minecart") && meets_detector(pos, position.0, width, height)
    })
}

/// Check the detector rail at `pos`: pressed while a minecart is on it
/// (checked again every [`DETECTOR_RECHECK`] ticks), released when none is.
fn check_pressed(world: &mut World, pos: IVec3) {
    let found = minecart_on(world, pos);
    set_data(world, pos, found);
    if found {
        schedule_tick(world, pos, DETECTOR_RECHECK);
    }
}

/// A minecart is inside the cell of the detector rail at `pos` (called by
/// minecart movement): an unpressed detector rail checks for it.
pub fn entity_inside(world: &mut World, pos: IVec3) {
    let Some(state) = world.block_at(pos) else {
        return;
    };
    let Some(entry) = world.registry().get_by_runtime_id(state) else {
        return;
    };
    if RailKind::of(entry) == Some(RailKind::Detector) && !bit(entry, state, RAIL_DATA) {
        check_pressed(world, pos);
    }
}

/// The minecart actor a minecart item places.
pub fn minecart_actor(item: &str) -> Option<&'static str> {
    Some(match item {
        "minecraft:minecart" => "minecraft:minecart",
        "minecraft:chest_minecart" => "minecraft:chest_minecart",
        "minecraft:hopper_minecart" => "minecraft:hopper_minecart",
        "minecraft:tnt_minecart" => "minecraft:tnt_minecart",
        "minecraft:command_block_minecart" => "minecraft:command_block_minecart",
        _ => return None,
    })
}

/// Place the minecart actor `actor` on the rail at `pos` (a minecart item
/// used on it, or a dispenser in front of it). Returns whether it spawned.
pub fn place_minecart(world: &mut World, pos: IVec3, actor: &str) -> bool {
    let Some((_, shape)) = rail(world, pos) else {
        return false;
    };
    let lift = if (ASCENDING_EAST..=ASCENDING_SOUTH).contains(&shape) {
        0.5
    } else {
        0.0
    };
    let feet = DVec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + MINECART_LIFT + lift,
        f64::from(pos.z) + 0.5,
    );
    let yaw = super::super::random(world).gen_range(0.0f32..360.0);
    match spawn_mob(world, actor, feet, yaw, SpawnCause::Spawned) {
        Ok(_) => true,
        Err(error) => {
            tracing::warn!(%actor, %error, "minecart could not spawn");
            false
        }
    }
}

/// Rails react to no `use` of their own; a minecart item reaches
/// [`use_block`] through the item path.
pub fn is_usable(_entry: &BlockEntry, _state: u32) -> bool {
    false
}

/// A player's use holding a minecart item places that minecart on the
/// rail; one item is used up outside creative. Anything else is not
/// handled.
pub fn use_block(world: &mut World, player: Entity, pos: IVec3, _state: u32) -> bool {
    use crate::server::game::item_use::{held, set_slot};
    let Some((slot, stack)) = held(world, player) else {
        return false;
    };
    let Some(actor) = minecart_actor(stack.item_id()) else {
        return false;
    };
    if !place_minecart(world, pos, actor) {
        return false;
    }
    if world.get::<GameMode>(player) != Some(&GameMode::Creative) {
        set_slot(world, player, slot, stack.grow(-1));
    }
    true
}

/// A rail needs a full top face below; its shape comes from the rails
/// around it.
pub fn placement(place: &Place<'_, impl BlockSource>) -> Option<u32> {
    let below = place.position - IVec3::Y;
    if !provides_support(place.blocks, below, IVec3::Y, SupportKind::Full) {
        return None;
    }
    let kind = RailKind::of(place.block)?;
    let current = int(place.block, place.state, RAIL_DIRECTION).unwrap_or(NORTH_SOUTH);
    let shape = place_shape(place.blocks, place.position, kind, current, false);
    Some(with(
        place.block,
        place.state,
        RAIL_DIRECTION,
        StateValue::Int(shape),
    ))
}

/// A rail stays while the top below holds an edge.
pub fn can_survive(
    blocks: &impl BlockSource,
    pos: IVec3,
    _state: u32,
    _entry: &BlockEntry,
) -> bool {
    provides_support(blocks, pos - IVec3::Y, IVec3::Y, SupportKind::Edge)
}

/// The rails the shape names that may connect turn to it.
pub fn placed(world: &mut World, pos: IVec3, state: u32) {
    let Some(entry) = world.registry().get_by_runtime_id(state) else {
        return;
    };
    let Some(shape) = int(entry, state, RAIL_DIRECTION) else {
        return;
    };
    join_neighbours(world, pos, shape);
}

/// Support is checked by the change path; power arrives through
/// [`power_changed`].
pub fn neighbour_changed(_world: &mut World, _pos: IVec3, _state: u32, _from: IVec3) {}

/// A pressed detector rail checks for minecarts again.
pub fn scheduled_tick(world: &mut World, pos: IVec3, state: u32) {
    let Some(entry) = world.registry().get_by_runtime_id(state) else {
        return;
    };
    if RailKind::of(entry) == Some(RailKind::Detector) && bit(entry, state, RAIL_DATA) {
        check_pressed(world, pos);
    }
}

/// Rails have no random ticks.
pub fn random_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Powered and activator rails take their power; a plain rail where three
/// rails meet re-picks its shape for the power; detector rails ignore
/// power.
pub fn power_changed(world: &mut World, pos: IVec3, state: u32, powered: bool) {
    let Some(entry) = world.registry().get_by_runtime_id(state) else {
        return;
    };
    let Some(kind) = RailKind::of(entry) else {
        return;
    };
    match kind {
        RailKind::Powered | RailKind::Activator => update_power(world, pos),
        RailKind::Detector => {}
        RailKind::Plain => {
            let current = int(entry, state, RAIL_DIRECTION).unwrap_or(NORTH_SOUTH);
            let meeting = [NORTH, SOUTH, WEST, EAST]
                .into_iter()
                .filter(|side| rail_near(world, pos + *side).is_some())
                .count();
            if meeting == 3 {
                let shape = place_shape(world, pos, kind, current, powered);
                if shape != current {
                    set_shape(world, pos, shape);
                    join_neighbours(world, pos, shape);
                }
            }
        }
    }
}

/// Rails hold no items.
pub fn breaking(_world: &mut World, _pos: IVec3, _state: u32) {}

#[cfg(test)]
mod tests {
    use glam::{DVec3, IVec3};

    use super::super::super::tests::{FLOOR, Harness, state};
    use super::super::tnt::tests::Server;
    use super::*;

    const Y: i32 = FLOOR + 1;

    fn shape(h: &Harness, pos: IVec3) -> Option<StateValue> {
        h.value(pos, RAIL_DIRECTION)
    }

    fn place(h: &mut Harness, item: &str, pos: IVec3) {
        h.hold(item, 1);
        let placed = h.click(pos - IVec3::Y, 1, DVec3::new(0.5, 1.0, 0.5));
        assert_eq!(placed.len(), 1, "{item} placed at {pos}");
    }

    /// The player stands within reach of the rails at the origin.
    fn harness() -> Harness {
        let mut h = Harness::new();
        h.stand(DVec3::new(2.5, 65.62, 2.5), 0.0, 30.0);
        h
    }

    #[test]
    fn a_rail_between_two_rails_at_a_corner_curves_and_they_turn_to_it() {
        let mut h = harness();
        let corner = IVec3::new(0, Y, 0);
        let south = corner + SOUTH;
        let east = corner + EAST;
        place(&mut h, "minecraft:rail", south);
        place(&mut h, "minecraft:rail", east);
        assert_eq!(shape(&h, east), Some(StateValue::Int(NORTH_SOUTH)));
        place(&mut h, "minecraft:rail", corner);
        assert_eq!(shape(&h, corner), Some(StateValue::Int(SOUTH_EAST)));
        assert_eq!(shape(&h, east), Some(StateValue::Int(EAST_WEST)));
        assert_eq!(shape(&h, south), Some(StateValue::Int(NORTH_SOUTH)));
    }

    #[test]
    fn powered_rails_stay_straight_at_a_corner() {
        let mut h = harness();
        let corner = IVec3::new(0, Y, 0);
        place(&mut h, "minecraft:rail", corner + SOUTH);
        place(&mut h, "minecraft:rail", corner + EAST);
        place(&mut h, "minecraft:golden_rail", corner);
        assert_eq!(shape(&h, corner), Some(StateValue::Int(EAST_WEST)));
    }

    #[test]
    fn a_rail_slopes_up_to_a_rail_one_block_higher() {
        let mut h = harness();
        let at = IVec3::new(0, Y, 0);
        h.put(at + EAST, state("minecraft:stone", &[]));
        place(&mut h, "minecraft:rail", at + EAST + IVec3::Y);
        place(&mut h, "minecraft:rail", at);
        assert_eq!(shape(&h, at), Some(StateValue::Int(ASCENDING_EAST)));
    }

    #[test]
    fn a_rail_needs_a_full_top_below_and_breaks_without_one() {
        let mut h = harness();
        let at = IVec3::new(0, Y, 0);
        place(&mut h, "minecraft:rail", at);
        h.set(at - IVec3::Y, state("minecraft:air", &[]));
        h.tick(2);
        assert_eq!(h.name(at), "minecraft:air");
        assert_eq!(h.drops(), [("minecraft:rail".to_owned(), 1)]);
    }

    #[test]
    fn powered_rail_power_runs_eight_rails_beyond_the_powered_one() {
        let mut h = harness();
        let rail = state(
            "minecraft:golden_rail",
            &[(RAIL_DIRECTION, StateValue::Int(EAST_WEST))],
        );
        for x in 1..=10 {
            h.put(IVec3::new(x, Y, 0), rail);
        }
        let lever = IVec3::new(0, Y, 0);
        h.set(
            lever,
            state(
                "minecraft:lever",
                &[("lever_direction", StateValue::Str("up_north_south"))],
            ),
        );
        let player = h.player;
        assert!(super::super::super::use_block(&mut h.world, player, lever));
        h.tick(4);
        let powered = |h: &Harness| {
            (1..=10)
                .map(|x| h.value(IVec3::new(x, Y, 0), RAIL_DATA) == Some(StateValue::Byte(1)))
                .collect::<Vec<_>>()
        };
        let mut expected = vec![true; 9];
        expected.push(false);
        assert_eq!(powered(&h), expected, "the powered rail and 8 beyond");
        assert!(super::super::super::use_block(&mut h.world, player, lever));
        h.tick(4);
        assert_eq!(powered(&h), vec![false; 10]);
    }

    #[test]
    fn a_placed_minecart_would_meet_the_detecting_box() {
        let at = IVec3::new(2, Y, 2);
        let (width, height) = (0.98, 0.7);
        let placed = at.as_dvec3() + DVec3::new(0.5, MINECART_LIFT, 0.5);
        assert!(meets_detector(at, placed, width, height));
        assert!(
            !meets_detector(at, placed + DVec3::X * 1.2, width, height),
            "on the next rail"
        );
        assert!(
            !meets_detector(at, placed + DVec3::Y * 0.7, width, height),
            "above the box"
        );
    }

    #[test]
    fn a_pressed_detector_rail_without_a_minecart_releases_at_its_recheck() {
        let mut s = Server::new();
        let at = IVec3::new(2, Y, 2);
        s.set(at, state("minecraft:detector_rail", &[]));
        entity_inside(s.world(), at);
        assert_eq!(
            s.value(at, RAIL_DATA),
            Some(StateValue::Byte(0)),
            "nothing on it"
        );
        s.set(
            at,
            state(
                "minecraft:detector_rail",
                &[(RAIL_DATA, StateValue::Byte(1))],
            ),
        );
        schedule_tick(s.world(), at, DETECTOR_RECHECK);
        s.tick(DETECTOR_RECHECK - 1);
        assert_eq!(s.value(at, RAIL_DATA), Some(StateValue::Byte(1)));
        s.tick(2);
        assert_eq!(s.value(at, RAIL_DATA), Some(StateValue::Byte(0)));
    }

    /// Minecart actors cannot be spawned yet: placing one reports failure
    /// and leaves nothing behind.
    #[test]
    fn minecarts_cannot_be_placed_yet() {
        let mut s = Server::new();
        let at = IVec3::new(2, Y, 2);
        s.set(at, state("minecraft:detector_rail", &[]));
        assert!(
            s.world()
                .resource::<crate::entity::mob::MobRegistry>()
                .unsupported("minecraft:minecart")
                .is_some()
        );
        assert!(!place_minecart(s.world(), at, "minecraft:minecart"));
        assert!(s.actors("minecraft:minecart").is_empty());
        assert_eq!(s.value(at, RAIL_DATA), Some(StateValue::Byte(0)));
    }
}
