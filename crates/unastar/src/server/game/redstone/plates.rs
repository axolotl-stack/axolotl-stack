//! Pressure plates pressed by actors, as BDS 1.26.51 presses them
//! (measured with `tools/bds-sweep/plates.py`,
//! `data/measured/redstone/plates_*.json`; replayed in `redstone/tests.rs`
//! from the measured actor positions).
//!
//! - Every tick, a plate an actor's box touches (the plate's area: the
//!   block less a sixteenth on each side, a quarter high) reads what is on
//!   it: wooden plates count every actor, stone and polished blackstone
//!   plates every actor but items, weighted plates every actor (a stack of
//!   items is one actor; light: 1 per actor up to 15, heavy: 1 per 10
//!   actors, rounded up). An unpressed plate presses at once; a pressed
//!   weighted plate follows its count at once while actors touch it.
//! - A pressed plate checks itself every 20 ticks (wooden and stone) or 10
//!   ticks (weighted) from when it was pressed, and releases when nothing
//!   touches it then: an actor leaving does not release it before.
//! - A pressed wooden or stone plate stores `redstone_signal` 1 and powers
//!   15; a weighted plate stores and powers its count.
//! - String between tripwire hooks is pressed the same way
//!   (`plates_tripwire*.json`): every actor counts, a pressed string checks
//!   itself every 10 ticks, and it stores `powered_bit`; its hooks follow
//!   (`super::tripwire`).

use std::collections::HashMap;

use glam::{DVec3, IVec3};

use crate::entity::systems::BlockSource;
use crate::registry::block::{BlockEntry, StateValue};
use crate::registry::collision::Aabb;

/// What a plate counts and how often a pressed plate checks itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlateKind {
    /// Wooden plates: every actor.
    Wooden,
    /// Stone and polished blackstone plates: every actor but items.
    Mobs,
    /// Light weighted: one per actor, up to 15.
    Light,
    /// Heavy weighted: one per ten actors, rounded up, up to 15.
    Heavy,
    /// Tripwire string: every actor; stores `powered_bit` and powers
    /// nothing itself (its hooks do).
    Tripwire,
}

/// How high string reaches: the box over string is the whole block this
/// high. BDS 1.26.51 pressed string under armour stands standing 0.125
/// above it and not 0.1875 (`plates_tripwire_more.json`); the value between
/// is not measured, 2.5/16 is chosen.
pub(crate) const TRIPWIRE_HEIGHT: f64 = 2.5 / 16.0;

impl PlateKind {
    /// The plate kind of block `id`, if it is a pressure plate.
    pub(crate) fn of(id: &str) -> Option<Self> {
        if id == super::tripwire::TRIPWIRE {
            return Some(Self::Tripwire);
        }
        let name = id.strip_suffix("_pressure_plate")?;
        Some(match name {
            "minecraft:light_weighted" => Self::Light,
            "minecraft:heavy_weighted" => Self::Heavy,
            "minecraft:stone" | "minecraft:polished_blackstone" => Self::Mobs,
            _ => Self::Wooden,
        })
    }

    /// Ticks between a pressed plate's checks.
    fn check_every(self) -> u64 {
        match self {
            Self::Wooden | Self::Mobs => 20,
            Self::Light | Self::Heavy | Self::Tripwire => 10,
        }
    }

    /// Whether this is a pressure plate (a power source), not string.
    pub(crate) fn is_plate(self) -> bool {
        self != Self::Tripwire
    }

    /// The signal a block of this kind stores in `state`.
    pub(crate) fn stored(self, entry: &BlockEntry, state: u32) -> u8 {
        match entry.state_value(state, self.property()) {
            Some(StateValue::Byte(on)) => u8::from(on != 0),
            Some(StateValue::Int(signal)) => u8::try_from(signal.clamp(0, 15)).unwrap_or(0),
            _ => 0,
        }
    }

    /// `state` storing `signal`.
    pub(crate) fn with_signal(self, entry: &BlockEntry, state: u32, signal: u8) -> Option<u32> {
        let value = match self {
            Self::Tripwire => StateValue::Byte(u8::from(signal > 0)),
            _ => StateValue::Int(i32::from(signal)),
        };
        entry.with_state_value(state, self.property(), value)
    }

    fn property(self) -> &'static str {
        match self {
            Self::Tripwire => "powered_bit",
            _ => "redstone_signal",
        }
    }

    fn weighted(self) -> bool {
        matches!(self, Self::Light | Self::Heavy)
    }

    /// The power of a plate that stores `signal`: 15 when a wooden or stone
    /// plate is pressed, the stored count for a weighted one.
    pub(crate) fn power(self, signal: u8) -> u8 {
        if self.weighted() || signal == 0 {
            signal
        } else {
            15
        }
    }

    /// The `redstone_signal` the plate stores with `actors` on it (items
    /// counted in `items`).
    pub(crate) fn signal(self, actors: usize, items: usize) -> u8 {
        let count = |n: usize| u8::try_from(n.min(15)).unwrap_or(15);
        match self {
            Self::Wooden | Self::Tripwire => u8::from(actors > 0),
            Self::Mobs => u8::from(actors > items),
            Self::Light => count(actors),
            Self::Heavy => count(actors.div_ceil(10)),
        }
    }
}

/// An actor that may press a plate: its box, and whether it is an item.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Presser {
    pub aabb: Aabb,
    pub item: bool,
}

/// The area of the `kind` plate (or string) at `pos` actors press.
fn area(kind: PlateKind, pos: IVec3) -> Aabb {
    let min = pos.as_dvec3();
    if kind == PlateKind::Tripwire {
        return Aabb::new(min, min + DVec3::new(1.0, TRIPWIRE_HEIGHT, 1.0));
    }
    Aabb::new(
        min + DVec3::new(1.0 / 16.0, 0.0, 1.0 / 16.0),
        min + DVec3::new(15.0 / 16.0, 0.25, 15.0 / 16.0),
    )
}

fn touches(a: &Aabb, b: &Aabb) -> bool {
    a.min.x < b.max.x
        && a.max.x > b.min.x
        && a.min.y < b.max.y
        && a.max.y > b.min.y
        && a.min.z < b.max.z
        && a.max.z > b.min.z
}

/// The `redstone_signal` of a `kind` plate at `pos` with `pressers` around.
pub(crate) fn reading(kind: PlateKind, pos: IVec3, pressers: &[Presser]) -> u8 {
    let area = area(kind, pos);
    let on: Vec<&Presser> = pressers
        .iter()
        .filter(|presser| touches(&presser.aabb, &area))
        .collect();
    kind.signal(on.len(), on.iter().filter(|presser| presser.item).count())
}

/// The pressed plates and when each checks itself next.
#[derive(bevy_ecs::prelude::Resource, Debug, Default)]
pub(crate) struct Plates {
    checks: HashMap<IVec3, u64>,
}

impl Plates {
    /// One tick: the new `redstone_signal` of every plate whose signal
    /// changes. `plates` lists the plates (position, kind, stored signal)
    /// that actors touch or that are pressed; `pressers` are the actors.
    pub(crate) fn tick(
        &mut self,
        now: u64,
        plates: &[(IVec3, PlateKind, u8)],
        pressers: &[Presser],
    ) -> Vec<(IVec3, u8)> {
        let mut out = Vec::new();
        for &(pos, kind, stored) in plates {
            let reading = reading(kind, pos, pressers);
            let due = self.checks.get(&pos).is_some_and(|at| *at <= now);
            if stored == 0 && reading > 0 {
                self.checks.insert(pos, now + kind.check_every());
                out.push((pos, reading));
            } else if stored > 0 && due {
                if reading > 0 {
                    self.checks.insert(pos, now + kind.check_every());
                } else {
                    self.checks.remove(&pos);
                }
                if reading != stored {
                    out.push((pos, reading));
                }
            } else if stored > 0 && kind.weighted() && reading > 0 && reading != stored {
                out.push((pos, reading));
            } else if stored > 0 && !self.checks.contains_key(&pos) {
                // A plate found pressed (loaded or set so) checks itself.
                self.checks.insert(pos, now + kind.check_every());
            }
        }
        out
    }

    /// The plates that are waiting for a check.
    pub(crate) fn pressed(&self) -> impl Iterator<Item = IVec3> + '_ {
        self.checks.keys().copied()
    }

    /// Forget a plate that is gone.
    pub(crate) fn removed(&mut self, pos: IVec3) {
        self.checks.remove(&pos);
    }
}

/// One tick of the plates and string at `candidates` in `world` under
/// `pressers`: every changed plate or string, then the tripwire lines of
/// changed string (`super::tripwire`), is handed to `write` (which stores it
/// as a change with its block updates) in that order.
pub(crate) fn press<W: BlockSource>(
    world: &mut W,
    plates: &mut Plates,
    now: u64,
    candidates: impl IntoIterator<Item = IVec3>,
    pressers: &[Presser],
    mut write: impl FnMut(&mut W, IVec3, u32),
) {
    let mut listed = Vec::new();
    for pos in candidates {
        let found = world.block_at(pos).and_then(|state| {
            let entry = world.registry().get_by_runtime_id(state)?;
            let kind = PlateKind::of(&entry.string_id)?;
            Some((pos, kind, kind.stored(entry, state)))
        });
        match found {
            Some(plate) => listed.push(plate),
            None => plates.removed(pos),
        }
    }
    if listed.is_empty() {
        return;
    }
    let mut strings = Vec::new();
    for (pos, signal) in plates.tick(now, &listed, pressers) {
        let Some((_, kind, _)) = listed.iter().find(|(at, ..)| *at == pos) else {
            continue;
        };
        let Some(next) = world.block_at(pos).and_then(|state| {
            let entry = world.registry().get_by_runtime_id(state)?;
            kind.with_signal(entry, state, signal)
        }) else {
            continue;
        };
        write(world, pos, next);
        if !kind.is_plate() {
            strings.push(pos);
        }
    }
    // The lines of pressed or released string follow in the same tick.
    for (pos, next) in super::tripwire::update_lines(&*world, &strings) {
        write(world, pos, next);
    }
}
