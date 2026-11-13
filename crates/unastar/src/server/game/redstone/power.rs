//! Who powers what: what each component gives each neighbour, the power of
//! blocks (strong from components, weak from dust), the level of every dust,
//! the sides a dust points at, and the signal a block reads from a neighbour.
//!
//! All of it is read through a [`View`]: the blocks as the circuit sees them
//! during an update (changes made earlier in the same tick stay hidden, see
//! [`super::Redstone`]) plus the outputs only the circuit knows (comparator
//! strengths, observer pulses).

use std::collections::{HashMap, HashSet};

use glam::IVec3;

use super::kinds::{Analog, Facts, Kind, SIDES, Table};
use crate::server::game::block_update::support::Face;

/// The blocks and outputs an update reads.
pub(crate) trait View {
    fn state(&self, pos: IVec3) -> Option<u32>;
    fn registry(&self) -> &crate::registry::BlockRegistry;
    fn table(&self) -> &Table;
    /// The output strength of the comparator at `pos`.
    fn comparator(&self, pos: IVec3) -> u8;
    /// Whether the observer at `pos` powers the circuit.
    fn observer(&self, pos: IVec3) -> bool;

    fn facts(&self, pos: IVec3) -> Facts {
        self.state(pos).map_or_else(
            || self.table().get(u32::MAX),
            |state| self.table().get(state),
        )
    }

    fn kind(&self, pos: IVec3) -> Kind {
        self.facts(pos).kind
    }
}

/// What the component at `pos` gives the neighbour in direction `toward`:
/// its strength, and whether that strongly powers the neighbour (a
/// conductor strongly powered passes it on to dust beside it).
pub(crate) fn emits(view: &impl View, pos: IVec3, toward: Face) -> (u8, bool) {
    match view.kind(pos) {
        Kind::RedstoneBlock => (15, false),
        Kind::Switch { attached, on } | Kind::Hook { attached, on } => {
            if on {
                (15, toward == attached)
            } else {
                (0, false)
            }
        }
        Kind::Plate { signal } => (signal, signal > 0 && toward == Face::Down),
        Kind::Daylight { signal } => (signal, false),
        Kind::Torch { attached, lit } => {
            if lit && toward != attached {
                (15, toward == Face::Up)
            } else {
                (0, false)
            }
        }
        Kind::Repeater { input, powered, .. } => {
            if powered && toward == input.opposite() {
                (15, true)
            } else {
                (0, false)
            }
        }
        Kind::Comparator { input, .. } => {
            if toward == input.opposite() {
                let strength = view.comparator(pos);
                (strength, strength > 0)
            } else {
                (0, false)
            }
        }
        Kind::Observer { facing, .. } => {
            if toward == facing.opposite() && view.observer(pos) {
                (15, true)
            } else {
                (0, false)
            }
        }
        Kind::None | Kind::Dust { .. } | Kind::Consumer(_) => (0, false),
    }
}

/// Strong power of the block at `pos`: from components that strongly power
/// it (a repeater or comparator facing it, a torch under it, a lever, button
/// or hook on it, a plate on it, an observer's back).
pub(crate) fn strong(view: &impl View, pos: IVec3) -> u8 {
    super::kinds::FACES
        .iter()
        .map(|face| {
            let (strength, strong) = emits(view, pos + face.offset(), face.opposite());
            if strong { strength } else { 0 }
        })
        .max()
        .unwrap_or(0)
}

/// The dust levels of one update, and the sides each dust points at.
#[derive(Debug, Default)]
pub(crate) struct Dust {
    pub levels: HashMap<IVec3, u8>,
    pointing: HashMap<IVec3, [bool; 4]>,
}

fn side_index(face: Face) -> usize {
    match face {
        Face::North => 0,
        Face::South => 1,
        Face::West => 2,
        Face::East => 3,
        _ => usize::MAX,
    }
}

impl Dust {
    pub(crate) fn level(&self, pos: IVec3) -> u8 {
        self.levels.get(&pos).copied().unwrap_or(0)
    }

    /// Whether the dust at `pos` powers its neighbour in direction `toward`:
    /// the block under it, and the sides it points at.
    pub(crate) fn powers(&self, pos: IVec3, toward: Face) -> bool {
        match toward {
            Face::Down => true,
            Face::Up => false,
            side => self
                .pointing
                .get(&pos)
                .is_some_and(|sides| sides[side_index(side)]),
        }
    }

    /// Weak power of the block at `pos`: the dust on it and the dust beside
    /// it pointing at it.
    pub(crate) fn weak(&self, pos: IVec3) -> u8 {
        let above = pos + IVec3::Y;
        let mut best = if self.levels.contains_key(&above) {
            self.level(above)
        } else {
            0
        };
        for side in SIDES {
            let dust = pos + side.offset();
            if self.levels.contains_key(&dust) && self.powers(dust, side.opposite()) {
                best = best.max(self.level(dust));
            }
        }
        best
    }
}

/// Whether dust can carry power from `from` to the dust at `to`, a side or
/// a diagonal step away (BDS 1.26.51, `redstone_dust*.json`): a block above
/// the lower dust that cuts dust stops both ways; climbing needs a block
/// that takes dust up on top, coming down one that passes it down.
fn carries(view: &impl View, from: IVec3, to: IVec3) -> bool {
    let dy = to.y - from.y;
    match dy {
        0 => true,
        1 => {
            // Up: `to` stands on the block beside `from`.
            let side = IVec3::new(to.x, from.y, to.z);
            !view.facts(from + IVec3::Y).cuts && view.facts(side).climb_up
        }
        -1 => {
            // Down: the block beside `from`, above `to`, must not cut.
            let side = IVec3::new(to.x, from.y, to.z);
            !view.facts(side).cuts && view.facts(from - IVec3::Y).climb_down
        }
        _ => false,
    }
}

/// The dust next to `pos` that can exchange power with it, and the side.
fn dust_links(dust: &HashSet<IVec3>, pos: IVec3) -> Vec<(Face, IVec3)> {
    let mut out = Vec::with_capacity(4);
    for side in SIDES {
        let beside = pos + side.offset();
        for candidate in [beside, beside + IVec3::Y, beside - IVec3::Y] {
            if dust.contains(&candidate) {
                out.push((side, candidate));
            }
        }
    }
    out
}

/// Every dust's level, from the components around each dust and from the
/// dust it is connected to (one less per step).
pub(crate) fn dust(view: &impl View, positions: impl Iterator<Item = IVec3>) -> Dust {
    let dust: HashSet<IVec3> = positions.filter(|pos| view.kind(*pos).is_dust()).collect();
    let mut levels: HashMap<IVec3, u8> = HashMap::with_capacity(dust.len());
    let mut buckets: Vec<Vec<IVec3>> = vec![Vec::new(); 16];
    for &pos in &dust {
        let mut base = 0u8;
        for face in super::kinds::FACES {
            let neighbour = pos + face.offset();
            if dust.contains(&neighbour) {
                continue;
            }
            let (strength, _) = emits(view, neighbour, face.opposite());
            base = base.max(strength);
            if view.facts(neighbour).conductor {
                base = base.max(strong(view, neighbour));
            }
        }
        levels.insert(pos, base);
        if base > 1 {
            buckets[usize::from(base)].push(pos);
        }
    }
    for level in (2..=15u8).rev() {
        let mut index = 0;
        while index < buckets[usize::from(level)].len() {
            let pos = buckets[usize::from(level)][index];
            index += 1;
            if levels.get(&pos).copied() != Some(level) {
                continue;
            }
            for (_, next) in dust_links(&dust, pos) {
                if !carries(view, pos, next) {
                    continue;
                }
                let current = levels.get(&next).copied().unwrap_or(0);
                if current < level - 1 {
                    levels.insert(next, level - 1);
                    buckets[usize::from(level - 1)].push(next);
                }
            }
        }
    }
    let mut pointing = HashMap::with_capacity(dust.len());
    for &pos in &dust {
        pointing.insert(pos, points(view, &dust, pos));
    }
    Dust { levels, pointing }
}

/// The sides a dust points at: the sides it connects to (dust, a component
/// that takes dust, or dust a diagonal step away), both ends of a line with
/// one connection, and every side of a dot.
fn points(view: &impl View, dust: &HashSet<IVec3>, pos: IVec3) -> [bool; 4] {
    let mut connected = [false; 4];
    for side in SIDES {
        let beside = pos + side.offset();
        let up = beside + IVec3::Y;
        let down = beside - IVec3::Y;
        let link = dust.contains(&beside)
            || view.kind(beside).connects_dust(side)
            || (dust.contains(&up) && (carries(view, pos, up) || carries(view, up, pos)))
            || (dust.contains(&down) && (carries(view, pos, down) || carries(view, down, pos)));
        connected[side_index(side)] = link;
    }
    let count = connected.iter().filter(|c| **c).count();
    match count {
        0 => [true; 4],
        1 => {
            let side = SIDES
                .into_iter()
                .find(|side| connected[side_index(*side)])
                .expect("one connection");
            let mut out = [false; 4];
            out[side_index(side)] = true;
            out[side_index(side.opposite())] = true;
            out
        }
        _ => connected,
    }
}

/// The signal the neighbour of `pos` in direction `from` gives `pos`: a
/// component's output toward it, a dust that powers it, or a conductor's
/// strong or weak power.
pub(crate) fn signal_from(view: &impl View, dust: &Dust, pos: IVec3, from: Face) -> u8 {
    let neighbour = pos + from.offset();
    let toward = from.opposite();
    let facts = view.facts(neighbour);
    match facts.kind {
        Kind::Dust { .. } => {
            if dust.powers(neighbour, toward) {
                dust.level(neighbour)
            } else {
                0
            }
        }
        Kind::None | Kind::Consumer(_) => {
            if facts.conductor {
                strong(view, neighbour).max(dust.weak(neighbour))
            } else {
                0
            }
        }
        _ => {
            let (strength, _) = emits(view, neighbour, toward);
            if facts.conductor {
                strength
                    .max(strong(view, neighbour))
                    .max(dust.weak(neighbour))
            } else {
                strength
            }
        }
    }
}

/// The strongest signal any neighbour gives `pos`.
pub(crate) fn power(view: &impl View, dust: &Dust, pos: IVec3) -> u8 {
    super::kinds::FACES
        .iter()
        .map(|face| signal_from(view, dust, pos, *face))
        .max()
        .unwrap_or(0)
}

/// A comparator's side input from `from`: dust pointing at it, or a repeater
/// or comparator facing it (BDS 1.26.51 ignores a redstone block beside it).
pub(crate) fn side_signal(view: &impl View, dust: &Dust, pos: IVec3, from: Face) -> u8 {
    let neighbour = pos + from.offset();
    match view.kind(neighbour) {
        Kind::Dust { .. } => signal_from(view, dust, pos, from),
        Kind::Repeater { .. } | Kind::Comparator { .. } => {
            emits(view, neighbour, from.opposite()).0
        }
        _ => 0,
    }
}

/// What a comparator at `pos` reads behind it, with `container` giving a
/// container's reading: the block behind's own reading if it has one, else
/// the signal from behind, and through a conductor (with less than 15) the
/// reading of the block behind that.
pub(crate) fn rear_signal(
    view: &impl View,
    dust: &Dust,
    pos: IVec3,
    input: Face,
    container: &impl Fn(IVec3) -> Option<u8>,
) -> u8 {
    let behind = pos + input.offset();
    let reading = |at: IVec3| match view.facts(at).analog {
        Analog::None => None,
        Analog::State(value) => Some(value),
        Analog::Container => Some(container(at).unwrap_or(0)),
    };
    if let Some(value) = reading(behind) {
        return value;
    }
    let signal = signal_from(view, dust, pos, input);
    if signal < 15
        && view.facts(behind).conductor
        && let Some(value) = reading(behind + input.offset())
    {
        return value;
    }
    signal
}
