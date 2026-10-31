//! Sculk sensors, calibrated sculk sensors and sculk shriekers, and the
//! vibrations they hear.
//!
//! - **Placement**: a sensor is placed inactive (`sculk_sensor_phase` 0); a
//!   shrieker placed by a player is inactive and can never summon a warden
//!   (`active` and `can_summon` 0, provisional: Minecraft Wiki "Sculk
//!   Shrieker"). They need no support and hold no items.
//! - **Game events** ([`game_event`]): block changes post a [`GameEvent`]
//!   at a point with an optional source entity and the block involved:
//!   placing (frequency 13) and breaking (12) by a player, opening (10) and
//!   closing (9) doors, trapdoors and fence gates, levers and buttons
//!   turning on (10) and off (9), a lectern's book being taken (block
//!   change, 11). Placing and breaking wool or a wool carpet is silent.
//! - **Listeners**: sensors hear 8 blocks, calibrated sensors 16 and
//!   shriekers 8 (squared integer block distance at most the range
//!   squared); the vibration's distance is `⌊√(dx² + dy² + dz²)⌋` between
//!   the event's block and the listener. Listeners are known from their
//!   placement and from a one-off scan of every sub-chunk around the first
//!   event near it.
//! - **Hearing**: a sensor hears only while inactive and never a block
//!   placed or broken in its own cell. A calibrated sensor
//!   (`minecraft:cardinal_direction` is its front) reads the redstone power
//!   coming from the block behind it: with a non-zero input it hears only
//!   that frequency. A shrieker hears only a sensor's tendrils clicking
//!   (frequency 0, posted at the sensor's centre when it activates and
//!   carrying the original source) caused by a player, and only while not
//!   `active`. Six rays from the event block's centre (each moved 0.001
//!   along one axis) to the listener's centre are traced; a vibration is
//!   blocked when every one of them crosses wool.
//! - **Choosing and travelling**: a listener takes vibrations only during
//!   the first tick it hears one, keeping the nearest (the higher frequency
//!   at the same distance). The next tick it sends the vibration on its way
//!   (`LevelEventGeneric` [`LEVEL_EVENT_VIBRATION`] with the `origin` and
//!   `target` points and `timeToLive` = distance / 20) and ignores
//!   everything until it arrives, `max(1, distance)` ticks after the
//!   event.
//! - **Sensors**: an arriving vibration turns an inactive sensor active
//!   (phase 1) for 30 ticks (calibrated: 10) with `power.on.sculk_sensor`
//!   at its corner, then cooldown (phase 2) for 10 ticks and inactive
//!   again with `power.off.sculk_sensor` (no sounds when waterlogged).
//!   While active it gives weak power `max(1, 15 − ⌊distance / range ×
//!   15⌋)` to every neighbour (a calibrated sensor not behind it)
//!   ([`power`]) and a comparator reads the vibration's frequency
//!   ([`comparator_signal`]), 0 otherwise.
//! - **Shriekers**: an arriving tendrils click turns the shrieker `active`
//!   for 90 ticks with `LevelEventGeneric` [`LEVEL_EVENT_SHRIEK`]
//!   (`originX`, `originY`, `originZ`: the shrieker) and
//!   `block.sculk_shrieker.shriek`.
//!
//! Not modelled: steps, landings, projectiles, item drops, mob deaths and
//! the other entity game events (nothing posts them), the sneaking and
//! dampening-block-under-the-source filters those use, catalyst blooming,
//! amethyst resonance next to a sensor, strong power from a sensor,
//! darkness, and warden warnings and summoning.

use std::collections::{BTreeMap, HashMap, HashSet};

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use unastar_packs::nbt::Tag;

use super::super::{Place, bit, centre, generic_event, int, schedule_tick, with};
use crate::ecs::resources::TickCounter;
use crate::entity::components::PlayerSession;
use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::blocks::set_block;
use crate::server::game::placement::Horizontal;
use crate::server::game::types::BlockRegistryResource;
use crate::world::ecs::{ChunkData, ChunkManager};

/// Sensor state property.
pub const PHASE: &str = "sculk_sensor_phase";
/// Shrieker state properties.
pub const ACTIVE: &str = "active";
pub const CAN_SUMMON: &str = "can_summon";
/// The calibrated sensor's front.
const FACING: &str = "minecraft:cardinal_direction";

/// Sensor phases.
const INACTIVE: i32 = 0;
const ACTIVE_PHASE: i32 = 1;
const COOLDOWN: i32 = 2;

const SENSOR_ACTIVE_TICKS: u64 = 30;
const CALIBRATED_ACTIVE_TICKS: u64 = 10;
const COOLDOWN_TICKS: u64 = 10;
const SHRIEK_TICKS: u64 = 90;

/// `LevelEventGeneric` id of a vibration on its way to a listener.
pub const LEVEL_EVENT_VIBRATION: i32 = 2027;
/// `LevelEventGeneric` id of a shrieker shrieking.
pub const LEVEL_EVENT_SHRIEK: i32 = 2035;

/// The farthest any listener hears.
const MAX_RANGE: i32 = 16;

/// What happened at a game event's origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameEvent {
    BlockPlace,
    BlockDestroy,
    BlockOpen,
    BlockClose,
    BlockActivate,
    BlockDeactivate,
    BlockChange,
    /// A sensor's tendrils clicking as it activates.
    TendrilsClicking,
}

impl GameEvent {
    /// The vibration's frequency (a comparator's reading of a sensor).
    pub fn frequency(self) -> u8 {
        match self {
            Self::BlockPlace => 13,
            Self::BlockDestroy => 12,
            Self::BlockOpen | Self::BlockActivate => 10,
            Self::BlockClose | Self::BlockDeactivate => 9,
            Self::BlockChange => 11,
            Self::TendrilsClicking => 0,
        }
    }

    /// Whether sensors hear it.
    fn vibrates(self) -> bool {
        self != Self::TendrilsClicking
    }

    /// Whether it is silent when its block dampens vibrations.
    fn dampened_by_block(self) -> bool {
        matches!(self, Self::BlockPlace | Self::BlockDestroy)
    }
}

/// Which listener a block is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Sensor,
    Calibrated,
    Shrieker,
}

impl Kind {
    fn of(entry: &BlockEntry) -> Option<Self> {
        Some(match entry.string_id.as_str() {
            "minecraft:sculk_sensor" => Self::Sensor,
            "minecraft:calibrated_sculk_sensor" => Self::Calibrated,
            "minecraft:sculk_shrieker" => Self::Shrieker,
            _ => return None,
        })
    }

    fn range(self) -> i32 {
        match self {
            Self::Calibrated => 16,
            Self::Sensor | Self::Shrieker => 8,
        }
    }
}

/// A vibration a listener heard.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Vibration {
    event: GameEvent,
    origin: DVec3,
    distance: i32,
    source: Option<Entity>,
}

/// One listener's vibration and what it last did.
#[derive(Debug, Clone, Copy, Default)]
struct Listener {
    /// The best vibration of the tick it started hearing (tick, vibration).
    candidate: Option<(u64, Vibration)>,
    /// The vibration on its way and the tick it arrives.
    travelling: Option<(u64, Vibration)>,
    /// Power and frequency of the vibration that last activated a sensor.
    power: u8,
    frequency: u8,
}

/// The listeners the world knows and the sub-chunks scanned for them.
#[derive(Resource, Debug, Default)]
pub struct Vibrations {
    listeners: HashMap<IVec3, Listener>,
    scanned: HashSet<IVec3>,
}

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
    let state = with(block, place.state, PHASE, StateValue::Int(INACTIVE));
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

/// A sculk block set at `pos` listens from now on.
pub fn placed(world: &mut World, pos: IVec3, _state: u32) {
    world
        .get_resource_or_init::<Vibrations>()
        .listeners
        .entry(pos)
        .or_default();
}

/// Neighbour changes do not reach sculk blocks (only game events do).
pub fn neighbour_changed(_world: &mut World, _pos: IVec3, _state: u32, _from: IVec3) {}

/// The end of a sensor's active or cooldown phase, or of a shriek.
pub fn scheduled_tick(world: &mut World, pos: IVec3, state: u32) {
    let Some(entry) = world.registry().get_by_runtime_id(state).cloned() else {
        return;
    };
    match Kind::of(&entry) {
        Some(Kind::Sensor | Kind::Calibrated) => match int(&entry, state, PHASE) {
            Some(ACTIVE_PHASE) => {
                let next = with(&entry, state, PHASE, StateValue::Int(COOLDOWN));
                set_block(world, pos, next, false);
                schedule_tick(world, pos, COOLDOWN_TICKS);
            }
            Some(COOLDOWN) => {
                let next = with(&entry, state, PHASE, StateValue::Int(INACTIVE));
                set_block(world, pos, next, false);
                corner_sound(world, pos, "power.off.sculk_sensor");
            }
            _ => {}
        },
        Some(Kind::Shrieker) if bit(&entry, state, ACTIVE) => {
            set_block(
                world,
                pos,
                with(&entry, state, ACTIVE, StateValue::Byte(0)),
                false,
            );
        }
        _ => {}
    }
}

/// Random ticks do not reach sculk blocks.
pub fn random_tick(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Redstone power does not switch sculk blocks; a calibrated sensor reads
/// its input when a vibration reaches it.
pub fn power_changed(_world: &mut World, _pos: IVec3, _state: u32, _powered: bool) {}

/// Sculk blocks hold no items.
pub fn breaking(_world: &mut World, _pos: IVec3, _state: u32) {}

/// Weak power the sculk block `state` at `pos` sends into its neighbour at
/// `pos + dir` (see the module docs).
pub fn power(world: &World, pos: IVec3, dir: IVec3, entry: &BlockEntry, state: u32) -> u8 {
    let Some(kind) = Kind::of(entry) else {
        return 0;
    };
    if kind == Kind::Shrieker || int(entry, state, PHASE) != Some(ACTIVE_PHASE) {
        return 0;
    }
    if kind == Kind::Calibrated && back(entry, state) == Some(dir) {
        return 0;
    }
    listener(world, pos).map_or(0, |listener| listener.power)
}

/// What a comparator reads from the sculk block at `pos`: an active
/// sensor's last frequency, 0 for a sensor otherwise, nothing for a
/// shrieker.
pub fn comparator_signal(world: &World, pos: IVec3) -> Option<u8> {
    let (kind, entry, state) = listener_at(world, pos)?;
    if kind == Kind::Shrieker {
        return None;
    }
    Some(if int(entry, state, PHASE) == Some(ACTIVE_PHASE) {
        listener(world, pos).map_or(0, |listener| listener.frequency)
    } else {
        0
    })
}

fn listener(world: &World, pos: IVec3) -> Option<&Listener> {
    world.get_resource::<Vibrations>()?.listeners.get(&pos)
}

/// The listener block at `pos`, its entry and state.
fn listener_at(world: &World, pos: IVec3) -> Option<(Kind, &BlockEntry, u32)> {
    let state = world.block_at(pos)?;
    let entry = world.registry().get_by_runtime_id(state)?;
    Some((Kind::of(entry)?, entry, state))
}

/// The offset of the block behind a calibrated sensor.
fn back(entry: &BlockEntry, state: u32) -> Option<IVec3> {
    match entry.state_value(state, FACING)? {
        StateValue::Str(facing) => Some(Horizontal::from_cardinal(facing)?.opposite().offset()),
        _ => None,
    }
}

/// Play `name` at the corner of the sensor at `pos` unless it is
/// waterlogged.
fn corner_sound(world: &World, pos: IVec3, name: &str) {
    if !super::super::gravity::holds_water(world, pos) {
        crate::entity::mob::net::sound::play_level_sound(world, None, pos.as_dvec3(), name, -1);
    }
}

/// Wool and wool carpets dampen vibrations of their own placing and
/// breaking.
fn dampens(world: &World, state: u32) -> bool {
    world
        .registry()
        .get_by_runtime_id(state)
        .is_some_and(|entry| {
            let name = entry.string_id.as_str();
            name.ends_with("_wool") || (name.ends_with("_carpet") && !name.ends_with("moss_carpet"))
        })
}

/// Wool stops vibrations.
fn occludes(world: &World, pos: IVec3) -> bool {
    super::super::name_at(world, pos).is_some_and(|name| name.ends_with("_wool"))
}

/// A game event `event` at `origin`, caused by `source`, involving the
/// block `block` (placed or broken): every listener in range that hears it
/// may take it (see the module docs).
pub fn game_event(
    world: &mut World,
    origin: DVec3,
    event: GameEvent,
    source: Option<Entity>,
    block: Option<u32>,
) {
    if event.dampened_by_block() && block.is_some_and(|state| dampens(world, state)) {
        return;
    }
    let at = origin.floor().as_ivec3();
    scan_around(world, at);
    let now = world.get_resource::<TickCounter>().map_or(0, |t| t.current);
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let near: Vec<IVec3> = world
        .get_resource_or_init::<Vibrations>()
        .listeners
        .keys()
        .filter(|pos| (**pos - at).abs().max_element() <= MAX_RANGE)
        .copied()
        .collect();
    for pos in near {
        let Some(state) = world.block_at(pos) else {
            continue;
        };
        let Some((kind, entry)) = registry
            .get_by_runtime_id(state)
            .and_then(|entry| Some((Kind::of(entry)?, entry)))
        else {
            world.resource_mut::<Vibrations>().listeners.remove(&pos);
            continue;
        };
        let distance_squared = (pos - at).length_squared();
        if distance_squared > kind.range() * kind.range() {
            continue;
        }
        let busy = world
            .resource::<Vibrations>()
            .listeners
            .get(&pos)
            .is_some_and(|listener| {
                listener.travelling.is_some()
                    || listener.candidate.is_some_and(|(heard, _)| heard != now)
            });
        if busy || !hears(world, kind, entry, state, pos, at, event, source) {
            continue;
        }
        if occluded(world, origin, centre(pos)) {
            continue;
        }
        let heard = Vibration {
            event,
            origin,
            distance: f64::from(distance_squared).sqrt().floor() as i32,
            source,
        };
        let mut vibrations = world.resource_mut::<Vibrations>();
        let listener = vibrations.listeners.entry(pos).or_default();
        let better = listener.candidate.is_none_or(|(_, best)| {
            heard.distance < best.distance
                || (heard.distance == best.distance
                    && heard.event.frequency() > best.event.frequency())
        });
        if better {
            listener.candidate = Some((now, heard));
        }
    }
}

/// Whether the listener `kind` at `pos` hears `event` from the block `at`.
#[allow(clippy::too_many_arguments)]
fn hears(
    world: &World,
    kind: Kind,
    entry: &BlockEntry,
    state: u32,
    pos: IVec3,
    at: IVec3,
    event: GameEvent,
    source: Option<Entity>,
) -> bool {
    match kind {
        Kind::Sensor | Kind::Calibrated => {
            if !event.vibrates() || int(entry, state, PHASE) != Some(INACTIVE) {
                return false;
            }
            if at == pos && matches!(event, GameEvent::BlockPlace | GameEvent::BlockDestroy) {
                return false;
            }
            if kind == Kind::Calibrated
                && let Some(back) = back(entry, state)
            {
                let input = super::super::redstone::power::signal(world, pos + back, -back, true);
                if input != 0 && input != event.frequency() {
                    return false;
                }
            }
            true
        }
        Kind::Shrieker => {
            event == GameEvent::TendrilsClicking
                && !bit(entry, state, ACTIVE)
                && source.is_some_and(|source| world.get::<PlayerSession>(source).is_some())
        }
    }
}

/// Whether wool blocks every one of the six rays from the centre of the
/// block at `from` to `to`.
fn occluded(world: &World, from: DVec3, to: DVec3) -> bool {
    let start = from.floor() + DVec3::splat(0.5);
    [
        DVec3::X,
        DVec3::NEG_X,
        DVec3::Y,
        DVec3::NEG_Y,
        DVec3::Z,
        DVec3::NEG_Z,
    ]
    .into_iter()
    .all(|axis| crosses_wool(world, start + axis * 0.001, to))
}

/// Whether the segment from `from` to `to` passes through wool (every
/// block it touches, both ends included).
fn crosses_wool(world: &World, from: DVec3, to: DVec3) -> bool {
    let delta = to - from;
    let mut cell = from.floor().as_ivec3();
    let end = to.floor().as_ivec3();
    let step = delta.signum().as_ivec3();
    let axis = |d: f64, p: f64, c: i32| -> (f64, f64) {
        if d == 0.0 {
            return (f64::INFINITY, f64::INFINITY);
        }
        let boundary = if d > 0.0 {
            f64::from(c + 1)
        } else {
            f64::from(c)
        };
        ((boundary - p) / d, 1.0 / d.abs())
    };
    let (mut next_x, step_x) = axis(delta.x, from.x, cell.x);
    let (mut next_y, step_y) = axis(delta.y, from.y, cell.y);
    let (mut next_z, step_z) = axis(delta.z, from.z, cell.z);
    loop {
        if occludes(world, cell) {
            return true;
        }
        if cell == end {
            return false;
        }
        let next = next_x.min(next_y).min(next_z);
        if next > 1.0 {
            return false;
        }
        if next == next_x {
            cell.x += step.x;
            next_x += step_x;
        } else if next == next_y {
            cell.y += step.y;
            next_y += step_y;
        } else {
            cell.z += step.z;
            next_z += step_z;
        }
    }
}

/// Find the listeners of every sub-chunk around `at` (within the farthest
/// range) not scanned yet; sub-chunks of unloaded chunks wait.
fn scan_around(world: &mut World, at: IVec3) {
    let low = (at - IVec3::splat(MAX_RANGE)) >> 4_i32;
    let high = (at + IVec3::splat(MAX_RANGE)) >> 4_i32;
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let states: Vec<(u32, u32)> = [
        "minecraft:sculk_sensor",
        "minecraft:calibrated_sculk_sensor",
        "minecraft:sculk_shrieker",
    ]
    .into_iter()
    .filter_map(|name| registry.get_by_name(name))
    .map(|entry| (entry.min_state_id, entry.max_state_id))
    .collect();
    let is_listener = |state: u32| {
        states
            .iter()
            .any(|(min, max)| (*min..=*max).contains(&state))
    };
    let mut scanned = Vec::new();
    let mut found = Vec::new();
    {
        let known = world.get_resource::<Vibrations>();
        let Some(chunks) = world.get_resource::<ChunkManager>() else {
            return;
        };
        for sx in low.x..=high.x {
            for sz in low.z..=high.z {
                let Some(data) = chunks
                    .get_by_coords(sx, sz)
                    .and_then(|chunk| world.get::<ChunkData>(chunk))
                else {
                    continue;
                };
                for sy in low.y..=high.y {
                    let sub = IVec3::new(sx, sy, sz);
                    if known.is_some_and(|known| known.scanned.contains(&sub)) {
                        continue;
                    }
                    scanned.push(sub);
                    if data.inner.is_subchunk_empty(sy) {
                        continue;
                    }
                    for y in 0..16u8 {
                        for z in 0..16u8 {
                            for x in 0..16u8 {
                                let world_y = sy * 16 + i32::from(y);
                                if is_listener(data.inner.get_block(x, world_y as i16, z)) {
                                    found.push(IVec3::new(
                                        sx * 16 + i32::from(x),
                                        world_y,
                                        sz * 16 + i32::from(z),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let mut vibrations = world.get_resource_or_init::<Vibrations>();
    vibrations.scanned.extend(scanned);
    for pos in found {
        vibrations.listeners.entry(pos).or_default();
    }
}

/// Once per game tick: listeners send the vibration they chose on its way
/// and the vibrations due arrive.
pub fn tick(world: &mut World) {
    let now = world.get_resource::<TickCounter>().map_or(0, |t| t.current);
    let Some(vibrations) = world.get_resource::<Vibrations>() else {
        return;
    };
    let busy: Vec<IVec3> = vibrations
        .listeners
        .iter()
        .filter(|(_, listener)| listener.candidate.is_some() || listener.travelling.is_some())
        .map(|(pos, _)| *pos)
        .collect();
    for pos in busy {
        let chosen = {
            let mut vibrations = world.resource_mut::<Vibrations>();
            let Some(listener) = vibrations.listeners.get_mut(&pos) else {
                continue;
            };
            match listener.candidate {
                Some((heard, vibration)) if heard < now => {
                    listener.candidate = None;
                    let due = heard + vibration.distance.max(1) as u64;
                    listener.travelling = Some((due, vibration));
                    Some(vibration)
                }
                _ => None,
            }
        };
        if let Some(vibration) = chosen {
            send_vibration(world, vibration, centre(pos));
        }
        let arrived = {
            let mut vibrations = world.resource_mut::<Vibrations>();
            let Some(listener) = vibrations.listeners.get_mut(&pos) else {
                continue;
            };
            match listener.travelling {
                Some((due, vibration)) if due <= now => {
                    listener.travelling = None;
                    Some(vibration)
                }
                _ => None,
            }
        };
        if let Some(vibration) = arrived {
            receive(world, pos, vibration);
        }
    }
}

/// Show the vibration travelling from its origin to `target`.
fn send_vibration(world: &World, vibration: Vibration, target: DVec3) {
    let point = |at: DVec3| {
        Tag::Compound(BTreeMap::from([
            ("type".to_owned(), Tag::String("vec3".to_owned())),
            ("x".to_owned(), Tag::Float(at.x as f32)),
            ("y".to_owned(), Tag::Float(at.y as f32)),
            ("z".to_owned(), Tag::Float(at.z as f32)),
        ]))
    };
    let fields = BTreeMap::from([
        ("origin".to_owned(), point(vibration.origin)),
        ("target".to_owned(), point(target)),
        (
            "timeToLive".to_owned(),
            Tag::Float(vibration.distance as f32 / 20.0),
        ),
    ]);
    crate::entity::mob::net::sound::send_to_viewers(
        world,
        vibration.origin,
        &generic_event(LEVEL_EVENT_VIBRATION, &fields),
    );
}

/// `vibration` reached the listener at `pos`.
fn receive(world: &mut World, pos: IVec3, vibration: Vibration) {
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let Some(state) = world.block_at(pos) else {
        return;
    };
    let Some((kind, entry)) = registry
        .get_by_runtime_id(state)
        .and_then(|entry| Some((Kind::of(entry)?, entry)))
    else {
        return;
    };
    match kind {
        Kind::Sensor | Kind::Calibrated if int(entry, state, PHASE) == Some(INACTIVE) => {
            let range = kind.range();
            let fraction = (vibration.distance as f32 / range as f32 * 15.0).floor() as i32;
            if let Some(listener) = world.resource_mut::<Vibrations>().listeners.get_mut(&pos) {
                listener.power = (15 - fraction).clamp(1, 15) as u8;
                listener.frequency = vibration.event.frequency();
            }
            let next = with(entry, state, PHASE, StateValue::Int(ACTIVE_PHASE));
            set_block(world, pos, next, false);
            schedule_tick(
                world,
                pos,
                if kind == Kind::Calibrated {
                    CALIBRATED_ACTIVE_TICKS
                } else {
                    SENSOR_ACTIVE_TICKS
                },
            );
            corner_sound(world, pos, "power.on.sculk_sensor");
            game_event(
                world,
                centre(pos),
                GameEvent::TendrilsClicking,
                vibration.source,
                None,
            );
        }
        Kind::Shrieker if !bit(entry, state, ACTIVE) => {
            set_block(
                world,
                pos,
                with(entry, state, ACTIVE, StateValue::Byte(1)),
                false,
            );
            schedule_tick(world, pos, SHRIEK_TICKS);
            let fields = BTreeMap::from([
                ("originX".to_owned(), Tag::Int(pos.x)),
                ("originY".to_owned(), Tag::Int(pos.y)),
                ("originZ".to_owned(), Tag::Int(pos.z)),
            ]);
            crate::entity::mob::net::sound::send_to_viewers(
                world,
                centre(pos),
                &generic_event(LEVEL_EVENT_SHRIEK, &fields),
            );
            crate::entity::mob::net::sound::play_level_sound(
                world,
                None,
                pos.as_dvec3(),
                "block.sculk_shrieker.shriek",
                -1,
            );
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use jolyne::valentine::McpePacketData;

    use super::super::super::redstone::{diode, power};
    use super::super::super::tests::{FLOOR, Harness, state};
    use super::*;
    use crate::entity::systems::physics::tests::TestBlocks;
    use crate::registry::BlockRegistry;
    use crate::server::game::placement::{PlacementInput, resolve_placement};

    /// The listener, five blocks east of the player's cell.
    const SENSOR: IVec3 = IVec3::new(5, FLOOR + 1, 0);
    /// Where the player places stone: four blocks west of [`SENSOR`].
    const PLACED: IVec3 = IVec3::new(1, FLOOR + 1, 0);
    const TOP: DVec3 = DVec3::new(0.5, 1.0, 0.5);

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

    fn sensor() -> u32 {
        state("minecraft:sculk_sensor", &[(PHASE, StateValue::Int(0))])
    }

    fn phase(h: &Harness) -> Option<StateValue> {
        h.value(SENSOR, PHASE)
    }

    /// The player places stone on top of the block `on`.
    fn place_stone_on(h: &mut Harness, on: IVec3) {
        h.hold("minecraft:stone", 1);
        let placed = h.click(on, 1, TOP);
        assert_eq!(placed.len(), 1, "stone placed on {on}");
    }

    /// Weak power the sensor gives the cell east of it.
    fn east_power(h: &Harness) -> u8 {
        power::received(&h.world, SENSOR + IVec3::X, true)
    }

    fn generic_events(packets: &[McpePacketData], id: i32) -> usize {
        packets
            .iter()
            .filter(|packet| {
                matches!(packet, McpePacketData::LevelEventGenericPacket(event) if event.event_id == id)
            })
            .count()
    }

    fn sounds(packets: &[McpePacketData]) -> Vec<String> {
        packets
            .iter()
            .filter_map(|packet| match packet {
                McpePacketData::LevelSoundEventPacket(sound) => Some(sound.sound_event.to_string()),
                _ => None,
            })
            .collect()
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

    #[test]
    fn a_block_placed_four_blocks_away_powers_a_sensor_after_its_travel_time() {
        let mut h = Harness::new();
        h.set(SENSOR, sensor());
        place_stone_on(&mut h, PLACED - IVec3::Y);
        h.packets();
        h.tick(1);
        assert_eq!(
            generic_events(&h.packets(), LEVEL_EVENT_VIBRATION),
            1,
            "the vibration sets off the tick after the placement"
        );
        h.tick(2);
        assert_eq!(phase(&h), Some(StateValue::Int(0)));
        h.tick(1);
        assert_eq!(
            phase(&h),
            Some(StateValue::Int(1)),
            "four ticks for four blocks"
        );
        assert!(sounds(&h.packets()).contains(&"power.on.sculk_sensor".to_string()));
        // max(1, 15 − ⌊4 / 8 × 15⌋) = 8; a comparator reads the placement's
        // frequency.
        assert_eq!(east_power(&h), 8);
        assert_eq!(diode::container_level(&h.world, SENSOR), Some(13));
        h.tick(29);
        assert_eq!(phase(&h), Some(StateValue::Int(1)));
        h.tick(1);
        assert_eq!(phase(&h), Some(StateValue::Int(2)), "30 active ticks");
        assert_eq!(east_power(&h), 0);
        assert_eq!(diode::container_level(&h.world, SENSOR), Some(0));
        // A placement during the cooldown is not heard.
        place_stone_on(&mut h, PLACED);
        h.packets();
        h.tick(9);
        assert_eq!(phase(&h), Some(StateValue::Int(2)));
        h.tick(1);
        assert_eq!(phase(&h), Some(StateValue::Int(0)), "10 cooldown ticks");
        assert!(sounds(&h.packets()).contains(&"power.off.sculk_sensor".to_string()));
        h.tick(10);
        assert_eq!(phase(&h), Some(StateValue::Int(0)));
    }

    #[test]
    fn the_nearest_vibration_of_the_first_tick_wins() {
        let mut h = Harness::new();
        h.set(SENSOR, sensor());
        let now = h.world.resource::<TickCounter>().current;
        // A lever turning off three blocks away, then a block changing
        // seven away in the same tick.
        game_event(
            &mut h.world,
            centre(SENSOR - IVec3::X * 3),
            GameEvent::BlockDeactivate,
            None,
            None,
        );
        game_event(
            &mut h.world,
            centre(SENSOR - IVec3::X * 7),
            GameEvent::BlockChange,
            None,
            None,
        );
        assert_eq!(
            listener(&h.world, SENSOR)
                .unwrap()
                .candidate
                .map(|(at, v)| (at, v.event)),
            Some((now, GameEvent::BlockDeactivate))
        );
        h.tick(3);
        assert_eq!(phase(&h), Some(StateValue::Int(1)));
        assert_eq!(diode::container_level(&h.world, SENSOR), Some(9));
        // 15 − ⌊3 / 8 × 15⌋ = 10.
        assert_eq!(east_power(&h), 10);
    }

    #[test]
    fn wool_between_blocks_the_vibration() {
        let mut h = Harness::new();
        h.set(SENSOR, sensor());
        h.put(SENSOR - IVec3::X * 2, state("minecraft:white_wool", &[]));
        place_stone_on(&mut h, PLACED - IVec3::Y);
        h.tick(10);
        assert_eq!(phase(&h), Some(StateValue::Int(0)));
    }

    #[test]
    fn placing_wool_is_silent() {
        let mut h = Harness::new();
        h.set(SENSOR, sensor());
        h.hold("minecraft:white_wool", 1);
        h.click(PLACED - IVec3::Y, 1, TOP);
        assert_eq!(h.name(PLACED), "minecraft:white_wool");
        h.tick(10);
        assert_eq!(phase(&h), Some(StateValue::Int(0)));
    }

    #[test]
    fn a_calibrated_sensor_hears_only_the_frequency_behind_it() {
        let mut h = Harness::new();
        // Facing south: the input is the block to the north.
        let calibrated = state(
            "minecraft:calibrated_sculk_sensor",
            &[
                (PHASE, StateValue::Int(0)),
                (FACING, StateValue::Str("south")),
            ],
        );
        h.set(SENSOR, calibrated);
        let behind = SENSOR - IVec3::Z;
        h.put(behind, state("minecraft:redstone_block", &[]));
        place_stone_on(&mut h, PLACED - IVec3::Y);
        h.tick(10);
        assert_eq!(phase(&h), Some(StateValue::Int(0)), "15 is not 13");
        h.put(
            behind,
            state(
                "minecraft:redstone_wire",
                &[("redstone_signal", StateValue::Int(13))],
            ),
        );
        place_stone_on(&mut h, PLACED);
        // (1, 65, 0) to (5, 64, 0): ⌊√17⌋ = 4.
        h.tick(3);
        assert_eq!(phase(&h), Some(StateValue::Int(0)));
        h.tick(1);
        assert_eq!(phase(&h), Some(StateValue::Int(1)));
        // 15 − ⌊4 / 16 × 15⌋ = 12, on every side but the back.
        assert_eq!(east_power(&h), 12);
        assert_eq!(
            power::emitted(&h.world, SENSOR, -IVec3::Z, false, true),
            0,
            "nothing into the input"
        );
        assert_eq!(diode::container_level(&h.world, SENSOR), Some(13));
        h.tick(9);
        assert_eq!(phase(&h), Some(StateValue::Int(1)));
        h.tick(1);
        assert_eq!(phase(&h), Some(StateValue::Int(2)), "10 active ticks");
    }

    #[test]
    fn a_shrieker_above_a_sensor_shrieks_for_a_player() {
        let mut h = Harness::new();
        h.set(SENSOR, sensor());
        let shrieker = SENSOR + IVec3::Y;
        let quiet = state(
            "minecraft:sculk_shrieker",
            &[
                (ACTIVE, StateValue::Byte(0)),
                (CAN_SUMMON, StateValue::Byte(0)),
            ],
        );
        h.set(shrieker, quiet);
        // A vibration without a source sets the sensor off but not the
        // shrieker.
        game_event(
            &mut h.world,
            centre(PLACED),
            GameEvent::BlockChange,
            None,
            None,
        );
        h.tick(5);
        assert_eq!(phase(&h), Some(StateValue::Int(1)));
        assert_eq!(h.value(shrieker, ACTIVE), Some(StateValue::Byte(0)));
        h.tick(40);
        assert_eq!(phase(&h), Some(StateValue::Int(0)));

        place_stone_on(&mut h, PLACED - IVec3::Y);
        h.packets();
        h.tick(4);
        assert_eq!(phase(&h), Some(StateValue::Int(1)));
        assert_eq!(h.value(shrieker, ACTIVE), Some(StateValue::Byte(0)));
        h.tick(1);
        assert_eq!(
            h.value(shrieker, ACTIVE),
            Some(StateValue::Byte(1)),
            "one tick from the sensor"
        );
        let packets = h.packets();
        assert_eq!(generic_events(&packets, LEVEL_EVENT_SHRIEK), 1);
        assert!(sounds(&packets).contains(&"block.sculk_shrieker.shriek".to_string()));
        h.tick(89);
        assert_eq!(h.value(shrieker, ACTIVE), Some(StateValue::Byte(1)));
        h.tick(1);
        assert_eq!(
            h.value(shrieker, ACTIVE),
            Some(StateValue::Byte(0)),
            "90 ticks"
        );
    }

    #[test]
    fn listeners_already_in_the_world_are_found() {
        let mut h = Harness::new();
        // Written without the change path: only the scan can find it.
        h.put(SENSOR, sensor());
        place_stone_on(&mut h, PLACED - IVec3::Y);
        h.tick(4);
        assert_eq!(phase(&h), Some(StateValue::Int(1)));
    }
}
