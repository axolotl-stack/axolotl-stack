//! Redstone: power, dust, torches, repeaters, comparators, observers and the
//! blocks power acts on, timed like BDS 1.26.51.
//!
//! Measured with the circuit sweeps in
//! `crates/unastar-data/data/measured/redstone/` (`tools/bds-sweep/circuits.py`);
//! this module's tests replay every circuit tick by tick. What BDS does, as
//! seen from outside:
//!
//! - The circuit is worked every other game tick (an *update*). A block
//!   changed in a tick is first seen by the update of a later tick, so a
//!   redstone block set at tick t lights dust at the first update at or
//!   after t + 1.
//! - An update first takes every dust's level from the components as they
//!   stand and shows it on the dust; then repeaters, comparators, torches and
//!   observers take their new output from what they read; then the blocks
//!   power acts on (lamps, doors, rails, ...) react to the new outputs. So a
//!   lamp behind a repeater and a dust lights in the update the repeater
//!   turns on, while the dust between them shows its level an update later,
//!   and each repeater, comparator, torch or observer in a chain adds an
//!   update.
//! - A repeater's output follows its input `delay` updates later (0 for the
//!   shortest setting) and stays on for at least `delay` + 1 updates; a
//!   powered repeater or comparator facing its side locks it.
//! - A lamp lights in the update it is powered and goes out two updates
//!   after it loses power; a copper bulb follows a tick after the update and
//!   toggles on each rising edge.
//! - An observer pulses for two ticks, starting at the first tick between
//!   updates at least two ticks after the change it saw (one when a block
//!   was set at the start of a tick).
//! - A torch that turns off for the eighth time within 60 ticks burns out
//!   and stays off until a block near it changes.
//! - A comparator reads a container's contents an update later than other
//!   changes.

pub(crate) mod kinds;
mod live;
#[cfg(test)]
mod piston_packets;
pub(crate) mod pistons;
pub(crate) mod plates;
pub(crate) mod power;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;

use bevy_ecs::prelude::Resource;
use glam::{IVec3, Vec3};
use zuri_nbt::tag;

use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::registry::block::StateValue;
use crate::server::game::block_update::support::Face;
use kinds::{Consumer, FACES, Kind, Table};
pub(crate) use live::{note_change, press_plates, tick_redstone};
use power::View;

/// Turn-offs within [`BURNOUT_TICKS`] that burn a torch out.
const BURNOUT_OFFS: usize = 8;
const BURNOUT_TICKS: u64 = 60;
/// Ticks a lamp stays lit after losing power.
const LAMP_OFF_TICKS: u64 = 4;
/// Ticks of an observer pulse.
const OBSERVER_PULSE: u64 = 2;

/// A world the circuit can read and change.
pub(crate) trait Circuit: BlockSource {
    /// Store `state` at `pos` as the circuit's own change, without block
    /// updates: BDS runs the block updates of a circuit change in the next
    /// tick ([`Redstone::take_written`]), so dust on a trapdoor the circuit
    /// opens pops a tick later.
    fn write(&mut self, pos: IVec3, state: u32);
    /// The comparator reading of the container at `pos` from its contents
    /// ([`container_reading`]).
    fn container_signal(&mut self, pos: IVec3) -> Option<u8>;
    /// A note block at `pos` was powered.
    fn note_block(&mut self, _pos: IVec3) {}
    /// Break the block at `pos` as a piston does: its drops fall and it
    /// becomes air (a circuit change, block updates a tick later).
    fn break_block(&mut self, pos: IVec3);
    /// Remove the block actor at `pos` and return it.
    fn take_actor(&mut self, _pos: IVec3) -> Option<tag::Compound> {
        None
    }
    /// Store `actor` at `pos` and send it to the players that see it.
    fn put_actor(&mut self, _pos: IVec3, _actor: tag::Compound) {}
    /// Play `sound` at `pos` for the players near it.
    fn sound(&mut self, _sound: &str, _pos: Vec3) {}
    /// Spawn a primed TNT at `pos` (its block was just written as air).
    fn prime_tnt(&mut self, _pos: IVec3) {}
}

/// The circuit's state between ticks.
#[derive(Resource, Debug, Default)]
pub(crate) struct Redstone {
    /// The tick of the last step.
    now: u64,
    table: Option<Arc<Table>>,
    /// Positions of blocks that take part in a circuit (anything a
    /// comparator reads is found through its comparator).
    components: HashSet<IVec3>,
    /// Positions to look around for components not known yet: outside
    /// changes and newly found components. A circuit the world had before
    /// the server knew it (generated, loaded) is found from the first change
    /// next to it.
    frontier: Vec<IVec3>,
    /// The state before an outside change since the last step: hidden from
    /// the update of the tick it was made in.
    pending: HashMap<IVec3, u32>,
    /// Outside changes since the last step, with the tick they count from.
    changed: Vec<(IVec3, u64)>,
    comparators: HashMap<IVec3, u8>,
    /// Observer outputs as the circuit sees them (taken from the observer's
    /// state with the other outputs of an update).
    observers: HashMap<IVec3, bool>,
    observer_busy: HashSet<IVec3>,
    observer_on: BTreeMap<u64, Vec<IVec3>>,
    observer_off: BTreeMap<u64, Vec<IVec3>>,
    /// The tick of each repeater's pending output change.
    repeaters: HashMap<IVec3, u64>,
    /// Repeaters placed since the last update: BDS takes their first
    /// change an update later.
    fresh: HashSet<IVec3>,
    /// Positions the circuit changed in the last step, whose block updates
    /// run at the start of the next tick.
    written: Vec<IVec3>,
    /// Whether each block power acts on was powered at the last update.
    powered: HashMap<IVec3, bool>,
    lamps_off: BTreeMap<u64, Vec<IVec3>>,
    /// Copper bulbs due a tick after an update: (pos, powered, rising).
    bulbs: BTreeMap<u64, Vec<(IVec3, bool, bool)>>,
    torch_offs: HashMap<IVec3, VecDeque<u64>>,
    burnt: HashSet<IVec3>,
    /// Container readings: (two steps ago, last step).
    containers: HashMap<IVec3, (u8, u8)>,
    pistons: pistons::Pistons,
}

/// The update's view: outside changes of this tick hidden.
struct Seen<'a, W: Circuit + ?Sized> {
    world: &'a W,
    pending: &'a HashMap<IVec3, u32>,
    table: &'a Table,
    comparators: &'a HashMap<IVec3, u8>,
    observers: &'a HashMap<IVec3, bool>,
}

impl<W: Circuit + ?Sized> View for Seen<'_, W> {
    fn state(&self, pos: IVec3) -> Option<u32> {
        self.pending
            .get(&pos)
            .copied()
            .or_else(|| self.world.block_at(pos))
    }

    fn registry(&self) -> &BlockRegistry {
        self.world.registry()
    }

    fn table(&self) -> &Table {
        self.table
    }

    fn comparator(&self, pos: IVec3) -> u8 {
        self.comparators.get(&pos).copied().unwrap_or(0)
    }

    fn observer(&self, pos: IVec3) -> bool {
        self.observers.get(&pos).copied().unwrap_or(false)
    }
}

fn with(registry: &BlockRegistry, state: u32, property: &str, value: StateValue) -> u32 {
    registry
        .get_by_runtime_id(state)
        .and_then(|entry| entry.with_state_value(state, property, value))
        .unwrap_or(state)
}

fn with_byte(registry: &BlockRegistry, state: u32, property: &str, value: bool) -> u32 {
    with(registry, state, property, StateValue::Byte(u8::from(value)))
}

/// The same block state under another identifier (`powered_repeater` /
/// `unpowered_repeater`, lit and unlit torches and lamps).
fn renamed(registry: &BlockRegistry, state: u32, name: &str) -> Option<u32> {
    let entry = registry.get_by_runtime_id(state)?;
    let target = registry.get_by_name(name)?;
    let mut out = target.default_state_id;
    for property in entry.data.properties {
        if let Some(value) = entry.state_value(state, property.name) {
            out = target
                .with_state_value(out, property.name, value)
                .unwrap_or(out);
        }
    }
    Some(out)
}

/// What an update decided for one producer.
enum Change {
    Torch(IVec3, bool),
    Repeater(IVec3, bool),
    Comparator(IVec3, u8),
    Observer(IVec3, bool),
}

impl Redstone {
    /// A circuit whose next step is tick `now + 1`.
    #[cfg(test)]
    pub(crate) fn starting_at(now: u64) -> Self {
        Self {
            now,
            ..Self::default()
        }
    }

    fn update_tick(tick: u64) -> bool {
        tick.is_multiple_of(2)
    }

    fn table(&mut self, registry: &BlockRegistry) -> Arc<Table> {
        static VANILLA: std::sync::LazyLock<Arc<Table>> =
            std::sync::LazyLock::new(|| Arc::new(Table::build(BlockRegistry::vanilla())));
        self.table
            .get_or_insert_with(|| {
                if registry.len() == BlockRegistry::vanilla().len() {
                    VANILLA.clone()
                } else {
                    Arc::new(Table::build(registry))
                }
            })
            .clone()
    }

    /// A block changed outside the circuit (a player, a command, a block
    /// update): `old` stays what the update of this tick sees.
    pub(crate) fn outside_change(&mut self, pos: IVec3, old: u32) {
        self.pending.entry(pos).or_insert(old);
        self.changed.push((pos, self.now));
    }

    /// Find the components connected to the frontier (every block within
    /// one step, diagonals included, of a known component or a change).
    fn discover(&mut self, world: &(impl Circuit + ?Sized), table: &Table) {
        /// Positions one step may add (a runaway search stops here).
        const BUDGET: usize = 65_536;
        let mut budget = BUDGET;
        while let Some(pos) = self.frontier.pop() {
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        let at = pos + IVec3::new(dx, dy, dz);
                        if self.components.contains(&at) {
                            continue;
                        }
                        let Some(state) = world.block_at(at) else {
                            continue;
                        };
                        if table.get(state).kind == Kind::None {
                            continue;
                        }
                        self.components.insert(at);
                        if budget == 0 {
                            continue;
                        }
                        budget -= 1;
                        self.frontier.push(at);
                    }
                }
            }
        }
    }

    /// Track a changed block's part in circuits and wake the observers
    /// watching it; `from` is the tick the change counts from.
    fn track(&mut self, world: &(impl Circuit + ?Sized), table: &Table, pos: IVec3, from: u64) {
        let kind = world
            .block_at(pos)
            .map_or(Kind::None, |state| table.get(state).kind);
        if kind == Kind::None {
            self.components.remove(&pos);
        } else if self.components.insert(pos) {
            self.frontier.push(pos);
        }
        if !matches!(kind, Kind::Comparator { .. }) {
            self.comparators.remove(&pos);
        }
        if !matches!(kind, Kind::Observer { .. }) {
            self.observers.remove(&pos);
        }
        if !matches!(kind, Kind::Repeater { .. }) {
            self.repeaters.remove(&pos);
            self.fresh.remove(&pos);
        }
        if !matches!(kind, Kind::Consumer(_)) {
            self.powered.remove(&pos);
        }
        if !matches!(kind, Kind::Consumer(Consumer::Piston { .. })) {
            self.pistons.removed(pos);
        }
        for face in FACES {
            let watcher = pos - face.offset();
            if let Some(state) = world.block_at(watcher)
                && let Kind::Observer { facing, .. } = table.get(state).kind
                && facing == face
            {
                self.wake_observer(watcher, from);
            }
        }
    }

    fn wake_observer(&mut self, pos: IVec3, from: u64) {
        if !self.observer_busy.insert(pos) {
            return;
        }
        let mut at = from + 2;
        if Self::update_tick(at) {
            at += 1;
        }
        self.observer_on.entry(at).or_default().push(pos);
    }

    /// Recover burnt-out torches next to an outside change or next to the
    /// block they hang on.
    fn unburn_near(&mut self, world: &(impl Circuit + ?Sized), table: &Table, pos: IVec3) {
        if self.burnt.is_empty() {
            return;
        }
        let near: Vec<IVec3> = self
            .burnt
            .iter()
            .copied()
            .filter(|torch| {
                let attached =
                    world
                        .block_at(*torch)
                        .and_then(|state| match table.get(state).kind {
                            Kind::Torch { attached, .. } => Some(*torch + attached.offset()),
                            _ => None,
                        });
                (*torch - pos).abs().element_sum() <= 1
                    || attached.is_some_and(|block| (block - pos).abs().element_sum() <= 1)
            })
            .collect();
        for torch in near {
            self.burnt.remove(&torch);
            self.torch_offs.remove(&torch);
        }
    }

    /// The circuit's own change of `pos` to `state`.
    fn write(
        &mut self,
        world: &mut (impl Circuit + ?Sized),
        table: &Table,
        pos: IVec3,
        state: u32,
    ) {
        if world.block_at(pos) == Some(state) {
            return;
        }
        world.write(pos, state);
        self.written.push(pos);
        self.track(world, table, pos, self.now);
    }

    /// The positions the circuit changed in its last step; the caller runs
    /// their block updates (as outside changes) before the next step.
    pub(crate) fn take_written(&mut self) -> Vec<IVec3> {
        let mut written = std::mem::take(&mut self.written);
        written.dedup();
        written
    }

    /// One game tick.
    pub(crate) fn step(&mut self, registry: &BlockRegistry, world: &mut (impl Circuit + ?Sized)) {
        let table = self.table(registry);
        self.now += 1;
        let now = self.now;
        let changed = std::mem::take(&mut self.changed);
        for (pos, from) in &changed {
            self.frontier.push(*pos);
            self.track(world, &table, *pos, *from);
            let is_repeater = |state: Option<u32>| {
                state.is_some_and(|state| matches!(table.get(state).kind, Kind::Repeater { .. }))
            };
            if is_repeater(world.block_at(*pos)) && !is_repeater(self.pending.get(pos).copied()) {
                self.fresh.insert(*pos);
            }
        }
        self.discover(world, &table);
        self.run_due(registry, world, &table, now, false);
        if Self::update_tick(now) {
            self.update(registry, world, &table);
        }
        // A burnt-out torch takes the change near it like any other: from the
        // next update on.
        for (pos, _) in &changed {
            self.unburn_near(world, &table, *pos);
        }
        self.pending.clear();
        self.run_due(registry, world, &table, now, true);
        self.read_containers(world, &table);
    }

    /// Work due at `now`: lamps going out and copper bulbs before the
    /// update, observer pulses after it.
    fn run_due(
        &mut self,
        registry: &BlockRegistry,
        world: &mut (impl Circuit + ?Sized),
        table: &Table,
        now: u64,
        after: bool,
    ) {
        if !after {
            self.run_pistons(registry, world, table, now);
            for pos in self.lamps_off.remove(&now).unwrap_or_default() {
                if self.powered.get(&pos) == Some(&false)
                    && let Some(state) = world.block_at(pos)
                    && table.get(state).kind == Kind::Consumer(Consumer::Lamp { lit: true })
                    && let Some(unlit) = renamed(registry, state, "minecraft:redstone_lamp")
                {
                    self.write(world, table, pos, unlit);
                }
            }
            for (pos, powered, rising) in self.bulbs.remove(&now).unwrap_or_default() {
                let Some(state) = world.block_at(pos) else {
                    continue;
                };
                let Kind::Consumer(Consumer::Bulb { lit, .. }) = table.get(state).kind else {
                    continue;
                };
                let mut next = with_byte(registry, state, "powered_bit", powered);
                if rising {
                    next = with_byte(registry, next, "lit", !lit);
                }
                self.write(world, table, pos, next);
            }
            return;
        }
        for pos in self.observer_on.remove(&now).unwrap_or_default() {
            match world
                .block_at(pos)
                .map(|state| (state, table.get(state).kind))
            {
                Some((state, Kind::Observer { .. })) => {
                    let on = with_byte(registry, state, "powered_bit", true);
                    self.write(world, table, pos, on);
                    self.observer_off
                        .entry(now + OBSERVER_PULSE)
                        .or_default()
                        .push(pos);
                }
                _ => {
                    self.observer_busy.remove(&pos);
                }
            }
        }
        for pos in self.observer_off.remove(&now).unwrap_or_default() {
            self.observer_busy.remove(&pos);
            if let Some(state) = world.block_at(pos)
                && matches!(table.get(state).kind, Kind::Observer { .. })
            {
                let off = with_byte(registry, state, "powered_bit", false);
                self.write(world, table, pos, off);
            }
        }
    }

    /// Piston moves ending and starting at `now`: their writes are circuit
    /// changes (block updates a tick later).
    fn run_pistons(
        &mut self,
        registry: &BlockRegistry,
        world: &mut (impl Circuit + ?Sized),
        table: &Table,
        now: u64,
    ) {
        let mut written = self.pistons.end_due(registry, world, table, now);
        written.extend(self.pistons.start_due(registry, world, table, now));
        for pos in written {
            self.written.push(pos);
            self.track(world, table, pos, now);
            // An observer a piston set down pulses at once.
            if let Some(state) = world.block_at(pos)
                && let Kind::Observer { powered: true, .. } = table.get(state).kind
                && self.observer_busy.insert(pos)
            {
                self.observer_off
                    .entry(now + OBSERVER_PULSE)
                    .or_default()
                    .push(pos);
            }
        }
    }

    /// Remember what each comparator's container reads, so the next updates
    /// see it a step late.
    fn read_containers(&mut self, world: &mut (impl Circuit + ?Sized), table: &Table) {
        let mut seen = HashMap::new();
        for pos in &self.components {
            let Some(state) = world.block_at(*pos) else {
                continue;
            };
            let Kind::Comparator { input, .. } = table.get(state).kind else {
                continue;
            };
            let behind = *pos + input.offset();
            for at in [behind, behind + input.offset()] {
                if let Some(state) = world.block_at(at)
                    && table.get(state).analog == kinds::Analog::Container
                {
                    let now = world.container_signal(at).unwrap_or(0);
                    let last = self.containers.get(&at).map_or(now, |(_, last)| *last);
                    seen.insert(at, (last, now));
                }
            }
        }
        self.containers = seen;
    }

    fn update(
        &mut self,
        registry: &BlockRegistry,
        world: &mut (impl Circuit + ?Sized),
        table: &Table,
    ) {
        let now = self.now;
        let positions: Vec<IVec3> = self.components.iter().copied().collect();
        let containers = self.containers.clone();
        let container = |pos: IVec3| containers.get(&pos).map(|(old, _)| *old);

        // Dust levels and producer inputs, from the outputs as they stand.
        let (levels, changes) = {
            let view = Seen {
                world: &*world,
                pending: &self.pending,
                table,
                comparators: &self.comparators,
                observers: &self.observers,
            };
            let dust = power::dust(&view, positions.iter().copied());
            let mut changes = Vec::new();
            for &pos in &positions {
                match view.kind(pos) {
                    Kind::Torch { attached, lit } => {
                        let block = pos + attached.offset();
                        let facts = view.facts(block);
                        let powered = if facts.conductor {
                            power::strong(&view, block).max(dust.weak(block)) > 0
                        } else {
                            power::emits(&view, block, attached.opposite()).0 > 0
                        };
                        let want = !powered && !self.burnt.contains(&pos);
                        if want != lit {
                            changes.push(Change::Torch(pos, want));
                        }
                    }
                    Kind::Repeater {
                        input,
                        delay,
                        powered,
                    } => {
                        // A repeater placed since the last update waits one.
                        if self.fresh.remove(&pos) {
                            continue;
                        }
                        let locked = beside(input).into_iter().any(|side| {
                            let neighbour = pos + side.offset();
                            matches!(
                                view.kind(neighbour),
                                Kind::Repeater { .. } | Kind::Comparator { .. }
                            ) && power::emits(&view, neighbour, side.opposite()).0 > 0
                        });
                        if locked {
                            continue;
                        }
                        let on = power::signal_from(&view, &dust, pos, input) > 0;
                        let due = self.repeaters.get(&pos).copied();
                        let fire = match due {
                            Some(at) => at <= now,
                            None if on != powered => {
                                if delay == 0 {
                                    true
                                } else {
                                    self.repeaters.insert(pos, now + 2 * u64::from(delay));
                                    false
                                }
                            }
                            None => false,
                        };
                        if fire {
                            self.repeaters.remove(&pos);
                            if powered && !on {
                                changes.push(Change::Repeater(pos, false));
                            } else if !powered {
                                changes.push(Change::Repeater(pos, true));
                                if !on {
                                    self.repeaters.insert(pos, now + 2 * (u64::from(delay) + 1));
                                }
                            }
                        }
                    }
                    Kind::Comparator {
                        input, subtract, ..
                    } => {
                        let rear = power::rear_signal(&view, &dust, pos, input, &container);
                        let side = beside(input)
                            .into_iter()
                            .map(|from| power::side_signal(&view, &dust, pos, from))
                            .max()
                            .unwrap_or(0);
                        let out = if subtract {
                            rear.saturating_sub(side)
                        } else if side > rear {
                            0
                        } else {
                            rear
                        };
                        // BDS rewrites a comparator only when its output
                        // changes: one set powered with nothing to read
                        // keeps its powered look.
                        if out != view.comparator(pos) {
                            changes.push(Change::Comparator(pos, out));
                        }
                    }
                    Kind::Observer { powered, .. } if powered != view.observer(pos) => {
                        changes.push(Change::Observer(pos, powered));
                    }
                    _ => {}
                }
            }
            (dust.levels, changes)
        };

        // Dust shows the levels the update started from.
        for (pos, level) in levels {
            if let Some(state) = world.block_at(pos)
                && let Kind::Dust { signal } = table.get(state).kind
                && signal != level
                && !self.pending.contains_key(&pos)
            {
                let next = with(
                    registry,
                    state,
                    "redstone_signal",
                    StateValue::Int(i32::from(level)),
                );
                self.write(world, table, pos, next);
            }
        }

        // New outputs.
        for change in changes {
            match change {
                Change::Torch(pos, lit) => {
                    if !lit {
                        let offs = self.torch_offs.entry(pos).or_default();
                        offs.push_back(now);
                        while offs.front().is_some_and(|t| *t + BURNOUT_TICKS <= now) {
                            offs.pop_front();
                        }
                        if offs.len() >= BURNOUT_OFFS {
                            self.burnt.insert(pos);
                        }
                    }
                    let name = if lit {
                        "minecraft:redstone_torch"
                    } else {
                        "minecraft:unlit_redstone_torch"
                    };
                    if let Some(state) = world.block_at(pos)
                        && let Some(next) = renamed(registry, state, name)
                    {
                        self.write(world, table, pos, next);
                    }
                }
                Change::Repeater(pos, powered) => {
                    let name = if powered {
                        "minecraft:powered_repeater"
                    } else {
                        "minecraft:unpowered_repeater"
                    };
                    if let Some(state) = world.block_at(pos)
                        && let Some(next) = renamed(registry, state, name)
                    {
                        self.write(world, table, pos, next);
                    }
                }
                Change::Comparator(pos, out) => {
                    self.comparators.insert(pos, out);
                    let name = if out > 0 {
                        "minecraft:powered_comparator"
                    } else {
                        "minecraft:unpowered_comparator"
                    };
                    if let Some(state) = world.block_at(pos)
                        && let Some(next) = renamed(registry, state, name)
                    {
                        let next = with_byte(registry, next, "output_lit_bit", out > 0);
                        self.write(world, table, pos, next);
                    }
                }
                Change::Observer(pos, on) => {
                    self.observers.insert(pos, on);
                }
            }
        }

        // The blocks power acts on, with the new outputs.
        let positions: Vec<IVec3> = self.components.iter().copied().collect();
        let acts = {
            let view = Seen {
                world: &*world,
                pending: &self.pending,
                table,
                comparators: &self.comparators,
                observers: &self.observers,
            };
            let dust = power::dust(&view, positions.iter().copied());
            let mut acts = Vec::new();
            let mut rails = Vec::new();
            for &pos in &positions {
                let Kind::Consumer(consumer) = view.kind(pos) else {
                    continue;
                };
                let powered = match consumer {
                    Consumer::Door { upper: true, .. } => continue,
                    Consumer::Door { upper: false, .. } => {
                        power::power(&view, &dust, pos) > 0
                            || (matches!(
                                view.kind(pos + IVec3::Y),
                                Kind::Consumer(Consumer::Door { upper: true, .. })
                            ) && power::power(&view, &dust, pos + IVec3::Y) > 0)
                    }
                    Consumer::Rail { .. } => {
                        rails.push((pos, power::power(&view, &dust, pos) > 0));
                        continue;
                    }
                    Consumer::Piston { facing, .. } => {
                        kinds::FACES
                            .iter()
                            .filter(|face| **face != facing)
                            .map(|face| power::signal_from(&view, &dust, pos, *face))
                            .max()
                            .unwrap_or(0)
                            > 0
                    }
                    _ => power::power(&view, &dust, pos) > 0,
                };
                acts.push((pos, consumer, powered));
            }
            for (pos, powered) in rail_power(&view, &rails) {
                if let Kind::Consumer(consumer) = view.kind(pos) {
                    acts.push((pos, consumer, powered));
                }
            }
            acts
        };
        for (pos, consumer, powered) in acts {
            let was = self.powered.insert(pos, powered).unwrap_or(false);
            let Some(state) = world.block_at(pos) else {
                continue;
            };
            match consumer {
                Consumer::Lamp { lit } => {
                    if powered && !lit {
                        if let Some(next) = renamed(registry, state, "minecraft:lit_redstone_lamp")
                        {
                            self.write(world, table, pos, next);
                        }
                    } else if !powered && lit && (was || !self.lamp_scheduled(pos)) {
                        self.lamps_off
                            .entry(now + LAMP_OFF_TICKS)
                            .or_default()
                            .push(pos);
                    }
                }
                Consumer::Door { open, .. } => {
                    if powered != was && powered != open {
                        let next = with_byte(registry, state, "open_bit", powered);
                        self.write(world, table, pos, next);
                    }
                }
                Consumer::Opens { open } => {
                    if powered != was && powered != open {
                        let next = with_byte(registry, state, "open_bit", powered);
                        self.write(world, table, pos, next);
                    }
                }
                Consumer::Bulb { .. } => {
                    if powered != was {
                        self.bulbs
                            .entry(now + 1)
                            .or_default()
                            .push((pos, powered, powered));
                    }
                }
                Consumer::Hopper { locked } => {
                    if powered != locked {
                        let next = with_byte(registry, state, "toggle_bit", powered);
                        self.write(world, table, pos, next);
                    }
                }
                Consumer::Triggered { triggered } => {
                    if powered != triggered {
                        let next = with_byte(registry, state, "triggered_bit", powered);
                        self.write(world, table, pos, next);
                    }
                }
                Consumer::Rail {
                    powered: was_on, ..
                } => {
                    if powered != was_on {
                        let next = with_byte(registry, state, "rail_data_bit", powered);
                        self.write(world, table, pos, next);
                    }
                }
                Consumer::NoteBlock => {
                    if powered && !was {
                        world.note_block(pos);
                    }
                }
                // Power reaching TNT primes it in the update (BDS 1.26.51,
                // `explosions/tnt_prime.json`).
                Consumer::Tnt => {
                    if powered && !was {
                        let air = registry
                            .get_by_name("minecraft:air")
                            .map_or(0, |entry| entry.default_state_id);
                        self.write(world, table, pos, air);
                        world.prime_tnt(pos);
                    }
                }
                Consumer::Piston { facing, .. } => {
                    self.pistons
                        .powered(&*world, table, pos, facing, powered, now);
                }
            }
        }
    }

    fn lamp_scheduled(&self, pos: IVec3) -> bool {
        self.lamps_off.values().any(|list| list.contains(&pos))
    }
}

/// What a comparator reads from a container of `slots` slots holding stacks
/// of (count, maximum stack size): 0 when empty, else 1 + 14 x the average
/// fullness of the slots, rounded down.
pub(crate) fn container_reading(slots: usize, stacks: impl Iterator<Item = (u32, u32)>) -> u8 {
    let mut fullness = 0.0f64;
    let mut any = false;
    for (count, max) in stacks {
        if count == 0 {
            continue;
        }
        any = true;
        fullness += f64::from(count) / f64::from(max.max(1));
    }
    if !any || slots == 0 {
        return 0;
    }
    (1.0 + fullness / slots as f64 * 14.0).floor().min(15.0) as u8
}

/// Powered and activator rails: a rail with power, and the rails of the same
/// kind joined to it along the track, up to 8 rails away.
fn rail_power(view: &impl View, rails: &[(IVec3, bool)]) -> Vec<(IVec3, bool)> {
    let kind_of = |pos: IVec3| match view.kind(pos) {
        Kind::Consumer(Consumer::Rail { activator, .. }) => Some(activator),
        _ => None,
    };
    let mut out: HashMap<IVec3, bool> = rails.iter().map(|(pos, _)| (*pos, false)).collect();
    let mut queue: VecDeque<(IVec3, u8)> = rails
        .iter()
        .filter(|(_, powered)| *powered)
        .map(|(pos, _)| (*pos, 0))
        .collect();
    while let Some((pos, distance)) = queue.pop_front() {
        if out.get(&pos) == Some(&true) {
            continue;
        }
        out.insert(pos, true);
        if distance >= 8 {
            continue;
        }
        let Some(kind) = kind_of(pos) else {
            continue;
        };
        for next in rail_neighbours(view, pos) {
            if kind_of(next) == Some(kind) && out.get(&next) == Some(&false) {
                queue.push_back((next, distance + 1));
            }
        }
    }
    out.into_iter().collect()
}

/// The rails a rail's track runs on to: both ends of its direction, one block
/// up the way it rises.
fn rail_neighbours(view: &impl View, pos: IVec3) -> Vec<IVec3> {
    let direction = view
        .state(pos)
        .and_then(|state| {
            view.registry()
                .get_by_runtime_id(state)
                .and_then(|entry| entry.state_value(state, "rail_direction"))
        })
        .and_then(|value| match value {
            StateValue::Int(value) => Some(value),
            _ => None,
        })
        .unwrap_or(0);
    let (a, b, rise) = match direction {
        1 => (IVec3::NEG_X, IVec3::X, None),
        2 => (IVec3::NEG_X, IVec3::X, Some(IVec3::X)),
        3 => (IVec3::NEG_X, IVec3::X, Some(IVec3::NEG_X)),
        4 => (IVec3::NEG_Z, IVec3::Z, Some(IVec3::NEG_Z)),
        5 => (IVec3::NEG_Z, IVec3::Z, Some(IVec3::Z)),
        _ => (IVec3::NEG_Z, IVec3::Z, None),
    };
    let mut out = Vec::with_capacity(4);
    for end in [a, b] {
        let up = rise == Some(end);
        let next = pos + end + if up { IVec3::Y } else { IVec3::ZERO };
        out.push(next);
        // A rail the track goes down to.
        out.push(pos + end - IVec3::Y);
    }
    out
}

/// The two horizontal faces beside a horizontal `face` (a diode's sides).
fn beside(face: Face) -> [Face; 2] {
    match face {
        Face::North | Face::South => [Face::West, Face::East],
        _ => [Face::North, Face::South],
    }
}
