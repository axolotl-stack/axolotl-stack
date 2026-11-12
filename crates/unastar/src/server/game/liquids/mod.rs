//! Liquids: water and lava flow, sources, mixing and the liquid layer, as
//! BDS 1.26.51 does them, measured tick by tick (`tools/bds-sweep/liquids.py`,
//! evidence in `crates/unastar-data/data/measured/liquids/`; this module's
//! tests replay it).
//!
//! - A liquid is still until something wakes it: a block placed or removed
//!   beside it (any block change), a bucket placing it, or its own spread. A
//!   liquid set by a command stays still (BDS leaves `/setblock` water
//!   standing until a neighbour changes).
//! - A woken liquid ticks [`WATER_DELAY`] (water) or [`LAVA_DELAY`] (lava;
//!   [`NETHER_LAVA_DELAY`] in the nether) game ticks later. A tick
//!   recomputes a flowing liquid's depth from its neighbours, makes a source
//!   between two water sources standing on a solid block or a source, then
//!   flows down if it can, else spreads sideways toward the nearest drop
//!   within [`WATER_SLOPE`] + 1 (water) or [`LAVA_SLOPE`] + 1 (lava) blocks,
//!   or every way when no drop is that near. Depth grows by 1 per block
//!   (lava in the overworld: 2) up to 7; falling liquid has depth 8 plus the
//!   depth it fell from. There is no random part (lava recedes on time too).
//! - BDS stores and sends flowing liquid as `minecraft:water` /
//!   `minecraft:lava` with `liquid_depth`, never as `flowing_*`.
//! - Lava touching water (above or beside) turns to obsidian (a source) or
//!   cobblestone; lava flowing down into water turns the water into stone;
//!   lava over soul soil beside blue ice turns to basalt.
//! - Blocks in the way: per block the liquid breaks it, fills its liquid
//!   layer (water) or stays out (`unastar_data::liquids`). A block that
//!   holds water keeps the water it is set into in its liquid layer, and that
//!   water spreads out of it (not through a stair's full side).
//! - Ticks are kept per chunk. A tick due in the same game tick as another
//!   runs in the order it was scheduled. A liquid tick scheduled into
//!   another chunk lands a tick early when that chunk has not ticked yet in
//!   the current game tick; BDS ticks chunks in an order that changes from
//!   tick to tick, so the engine draws a random chunk order every tick.
//! - Changes are sent once per tick, batched per subchunk: one change as
//!   `UpdateBlock`, several as `UpdateSubChunkBlocks` ([`live`]).

use std::collections::{BTreeMap, HashMap};

use bevy_ecs::prelude::Resource;
use glam::IVec3;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use unastar_data::liquids::{Holds, LiquidBlock, Reaction};

use super::block_update::shape::{CORNERABLE_STAIRS, stair_facing};
use super::block_update::{self, NEIGHBOURS, UpdateTarget};
use crate::registry::BlockRegistry;
use crate::registry::block::{LiquidKind, StateValue};
use crate::registry::collision::Side;

pub(crate) mod live;
#[cfg(test)]
mod tests;

pub(crate) use live::{note_change, place_source, take_source, tick_liquids};

/// Game ticks from a water wake to its tick.
pub(crate) const WATER_DELAY: u64 = 5;
/// Game ticks from a lava wake to its tick in the overworld.
pub(crate) const LAVA_DELAY: u64 = 30;
/// Game ticks from a lava wake to its tick in the nether.
pub(crate) const NETHER_LAVA_DELAY: u64 = 10;
/// How far past the first block water looks for a drop (it turns toward a
/// drop up to 5 blocks away).
pub(crate) const WATER_SLOPE: i32 = 4;
/// How far past the first block lava looks for a drop (3 blocks away, in the
/// overworld and the nether alike).
pub(crate) const LAVA_SLOPE: i32 = 2;

/// The four horizontal directions in the order a liquid spreads: -x, +x,
/// -z, +z.
const SIDES: [IVec3; 4] = [IVec3::NEG_X, IVec3::X, IVec3::NEG_Z, IVec3::Z];

/// Which storage of a block holds a liquid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layer {
    /// The block itself is the liquid.
    Block,
    /// The liquid sits in the block's liquid layer (a waterlogged block).
    Liquid,
}

/// A liquid at a position: kind, `liquid_depth` and where it is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Fluid {
    pub kind: LiquidKind,
    pub depth: u8,
    pub layer: Layer,
}

/// A block store the liquid engine can change: the block layer through
/// [`UpdateTarget`], plus the liquid layer and the effects of a liquid
/// breaking a block.
pub(crate) trait LiquidTarget: UpdateTarget {
    /// The liquid-layer block at `pos` (air when empty), `None` when not loaded.
    fn layer_at(&self, pos: IVec3) -> Option<u32>;
    /// Store `state` in the liquid layer at `pos`.
    fn write_layer(&mut self, pos: IVec3, state: u32);
    /// A liquid flowing in breaks the block `state` at `pos` (water drops what
    /// it drops; lava burns it). The block becomes air.
    fn wash(&mut self, pos: IVec3, state: u32, kind: LiquidKind);
}

/// The liquid states this engine writes, resolved once.
#[derive(Debug, Clone)]
struct States {
    air: u32,
    water: [u32; 16],
    lava: [u32; 16],
    stone: u32,
    cobblestone: u32,
    obsidian: u32,
    basalt: u32,
    /// Block (not state) ids basalt forms between.
    soul_soil: u32,
    blue_ice: u32,
}

impl States {
    fn new(registry: &BlockRegistry) -> Self {
        let id = |name: &str| {
            registry
                .default_state_id_by_name(name)
                .unwrap_or_else(|| panic!("{name} in the block registry"))
        };
        let depths = |name: &str| {
            let entry = registry.get_by_name(name).expect(name);
            std::array::from_fn(|depth| {
                entry
                    .with_state_value(
                        entry.default_state_id,
                        "liquid_depth",
                        StateValue::Int(depth as i32),
                    )
                    .expect("liquid_depth 0..15")
            })
        };
        let block = |name: &str| {
            registry
                .get_by_name(name)
                .unwrap_or_else(|| panic!("{name} in the block registry"))
        };
        let basalt = block("minecraft:basalt");
        Self {
            air: id("minecraft:air"),
            water: depths("minecraft:water"),
            lava: depths("minecraft:lava"),
            stone: id("minecraft:stone"),
            cobblestone: id("minecraft:cobblestone"),
            obsidian: id("minecraft:obsidian"),
            basalt: basalt
                .with_state_value(basalt.default_state_id, "pillar_axis", StateValue::Str("y"))
                .unwrap_or(basalt.default_state_id),
            soul_soil: block("minecraft:soul_soil").id,
            blue_ice: block("minecraft:blue_ice").id,
        }
    }

    fn fluid(&self, kind: LiquidKind, depth: u8) -> u32 {
        match kind {
            LiquidKind::Water => self.water[usize::from(depth & 15)],
            LiquidKind::Lava => self.lava[usize::from(depth & 15)],
        }
    }
}

/// What a liquid of some kind meets at a position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Meets {
    Air,
    Fluid(Fluid),
    /// The liquid breaks the block and takes its place.
    Breaks(u32),
    /// Water enters the block's (empty) liquid layer.
    Fills,
    /// The liquid stays out.
    Blocked,
}

fn chunk_of(pos: IVec3) -> (i32, i32) {
    (pos.x >> 4, pos.z >> 4)
}

/// The liquid engine: pending ticks per chunk and the game tick counter.
#[derive(Resource)]
pub(crate) struct Liquids {
    now: u64,
    /// Per chunk, due tick to positions in the order they were scheduled.
    queue: HashMap<(i32, i32), BTreeMap<u64, Vec<IVec3>>>,
    /// Positions with a pending tick, and when it is due.
    pending: HashMap<IVec3, u64>,
    /// While a game tick runs: each chunk's place in this tick's random
    /// chunk order (drawn when first needed) and the chunk being ticked.
    ranks: HashMap<(i32, i32), u32>,
    ticking: Option<(i32, i32)>,
    rng: StdRng,
    /// Lava ticks [`NETHER_LAVA_DELAY`] apart and spreads 1 per block.
    pub nether: bool,
    states: Option<States>,
}

impl Default for Liquids {
    fn default() -> Self {
        Self::seeded(0x6c69_7175_6964)
    }
}

impl Liquids {
    /// An engine whose chunk order draws come from `seed`.
    pub(crate) fn seeded(seed: u64) -> Self {
        Self {
            now: 0,
            queue: HashMap::new(),
            pending: HashMap::new(),
            ranks: HashMap::new(),
            ticking: None,
            rng: StdRng::seed_from_u64(seed),
            nether: false,
            states: None,
        }
    }

    fn states(&mut self, registry: &BlockRegistry) -> States {
        self.states
            .get_or_insert_with(|| States::new(registry))
            .clone()
    }

    fn delay(&self, kind: LiquidKind) -> u64 {
        match kind {
            LiquidKind::Water => WATER_DELAY,
            LiquidKind::Lava if self.nether => NETHER_LAVA_DELAY,
            LiquidKind::Lava => LAVA_DELAY,
        }
    }

    fn rank(&mut self, chunk: (i32, i32)) -> u32 {
        let rng = &mut self.rng;
        *self.ranks.entry(chunk).or_insert_with(|| rng.r#gen())
    }

    /// Schedule a tick of the liquid at `pos` `delay` ticks from now (one
    /// tick sooner when it lands in a chunk that ticks later in the running
    /// game tick than the chunk scheduling it). A position with a pending
    /// tick keeps it.
    fn schedule(&mut self, pos: IVec3, delay: u64) {
        if self.pending.contains_key(&pos) {
            return;
        }
        let chunk = chunk_of(pos);
        let mut due = self.now + delay;
        if let Some(from) = self.ticking
            && from != chunk
        {
            let (from_rank, to_rank) = (self.rank(from), self.rank(chunk));
            if to_rank > from_rank {
                due -= 1;
            }
        }
        self.pending.insert(pos, due);
        self.queue
            .entry(chunk)
            .or_default()
            .entry(due)
            .or_default()
            .push(pos);
    }

    /// The liquid at `pos`: the block itself, or water in its liquid layer.
    fn fluid<T: LiquidTarget + ?Sized>(target: &T, pos: IVec3) -> Option<Fluid> {
        let registry = target.registry();
        let state = target.block_at(pos)?;
        if let Some(liquid) = registry.liquid_state(state) {
            return Some(Fluid {
                kind: liquid.kind,
                depth: liquid.depth,
                layer: Layer::Block,
            });
        }
        let layer = target.layer_at(pos)?;
        registry.liquid_state(layer).map(|liquid| Fluid {
            kind: liquid.kind,
            depth: liquid.depth,
            layer: Layer::Liquid,
        })
    }

    /// What a liquid of `kind` meets at `pos`.
    fn meets<T: LiquidTarget + ?Sized>(target: &T, pos: IVec3, kind: LiquidKind) -> Meets {
        let Some(state) = target.block_at(pos) else {
            return Meets::Blocked;
        };
        if let Some(fluid) = Self::fluid(target, pos) {
            return Meets::Fluid(fluid);
        }
        if state == *crate::world::chunk::blocks::AIR {
            return Meets::Air;
        }
        match reaction(target.registry(), state, kind) {
            Some(Reaction::Breaks(_)) => Meets::Breaks(state),
            Some(Reaction::Fills) if kind == LiquidKind::Water => Meets::Fills,
            _ => Meets::Blocked,
        }
    }

    /// Whether `pos` stops a liquid of `kind`: a block it neither breaks nor
    /// fills (a liquid or air does not).
    fn blocks_flow<T: LiquidTarget + ?Sized>(target: &T, pos: IVec3, kind: LiquidKind) -> bool {
        matches!(Self::meets(target, pos, kind), Meets::Blocked)
    }

    /// Whether a liquid of `kind` can flow into `pos`.
    fn can_enter<T: LiquidTarget + ?Sized>(target: &T, pos: IVec3, kind: LiquidKind) -> bool {
        matches!(
            Self::meets(target, pos, kind),
            Meets::Air | Meets::Breaks(_) | Meets::Fills
        )
    }

    fn is_source<T: LiquidTarget + ?Sized>(target: &T, pos: IVec3, kind: LiquidKind) -> bool {
        Self::fluid(target, pos).is_some_and(|f| f.kind == kind && f.depth == 0)
    }

    /// The sides a liquid in a block's liquid layer may leave through:
    /// every side, except a stair's full side.
    fn exits<T: LiquidTarget + ?Sized>(target: &T, pos: IVec3, fluid: Fluid) -> [bool; 4] {
        if fluid.layer == Layer::Block {
            return [true; 4];
        }
        let registry = target.registry();
        let Some((entry, state)) = target
            .block_at(pos)
            .and_then(|state| Some((registry.get_by_runtime_id(state)?, state)))
        else {
            return [true; 4];
        };
        // Stairs were measured in their first palette state (facing east:
        // the full side is +x); the other facings by symmetry.
        if entry.has_tag(CORNERABLE_STAIRS)
            && let Some((facing, _)) = stair_facing(entry, state)
        {
            let mut out = [true; 4];
            out[side_index(facing)] = false;
            return out;
        }
        match facts(registry, state).map(|facts| facts.holds) {
            Some(Holds::Sides(sides)) => sides,
            _ => [true; 4],
        }
    }

    /// Store `fluid` (or nothing) at `pos` in the layer it belongs to.
    fn put<T: LiquidTarget + ?Sized>(
        &mut self,
        target: &mut T,
        pos: IVec3,
        kind: LiquidKind,
        depth: Option<u8>,
        layer: Layer,
    ) {
        let states = self.states(target.registry());
        let state = depth.map_or(states.air, |depth| states.fluid(kind, depth));
        match layer {
            Layer::Block => target.write(pos, state),
            Layer::Liquid => target.write_layer(pos, state),
        }
    }

    /// A block next to `pos` changed (or `pos` itself was rewritten): wake
    /// the liquid there, hardening lava that now touches water.
    pub(crate) fn neighbour_changed<T: LiquidTarget + ?Sized>(
        &mut self,
        target: &mut T,
        pos: IVec3,
    ) {
        let Some(fluid) = Self::fluid(target, pos) else {
            return;
        };
        if fluid.kind == LiquidKind::Lava && self.harden(target, pos, fluid) {
            return;
        }
        let delay = self.delay(fluid.kind);
        self.schedule(pos, delay);
    }

    /// A liquid was placed at `pos` (by its own spread or a bucket): lava
    /// touching water hardens at once, else it ticks after its delay.
    pub(crate) fn placed<T: LiquidTarget + ?Sized>(&mut self, target: &mut T, pos: IVec3) {
        self.neighbour_changed(target, pos);
    }

    /// The block at `pos` changed: wake the liquids beside it.
    pub(crate) fn changed<T: LiquidTarget + ?Sized>(&mut self, target: &mut T, pos: IVec3) {
        for offset in NEIGHBOURS {
            self.neighbour_changed(target, pos + offset);
        }
    }

    /// After the engine rewrote `pos`: wake the liquids beside it and run
    /// the block updates of the change for the other blocks around it.
    fn rewrote<T: LiquidTarget + ?Sized>(&mut self, target: &mut T, pos: IVec3) {
        self.changed(target, pos);
        let mut notify = Notify {
            target,
            liquids: self,
        };
        block_update::changed(&mut notify, pos, false);
    }

    /// Lava at `pos` touching water above or beside becomes obsidian (a
    /// source) or cobblestone; over soul soil beside blue ice, basalt.
    fn harden<T: LiquidTarget + ?Sized>(
        &mut self,
        target: &mut T,
        pos: IVec3,
        fluid: Fluid,
    ) -> bool {
        let states = self.states(target.registry());
        let water = |target: &T, at: IVec3| {
            Self::fluid(target, at).is_some_and(|f| f.kind == LiquidKind::Water)
        };
        let next = if [IVec3::Y, IVec3::NEG_X, IVec3::X, IVec3::NEG_Z, IVec3::Z]
            .iter()
            .any(|offset| water(target, pos + *offset))
        {
            Some(if fluid.depth == 0 {
                states.obsidian
            } else {
                states.cobblestone
            })
        } else {
            let block = |target: &T, at: IVec3| {
                target
                    .block_at(at)
                    .and_then(|state| target.registry().get_by_runtime_id(state))
                    .map(|entry| entry.id)
            };
            (block(target, pos + IVec3::NEG_Y) == Some(states.soul_soil)
                && SIDES
                    .iter()
                    .any(|offset| block(target, pos + *offset) == Some(states.blue_ice)))
            .then_some(states.basalt)
        };
        let Some(next) = next else {
            return false;
        };
        target.write(pos, next);
        self.rewrote(target, pos);
        true
    }

    /// The cheapest way to a drop from `pos` (Beta-era flow cost): the
    /// number of steps past the first block, or 1000 when none is within
    /// `slope`.
    fn flow_cost<T: LiquidTarget + ?Sized>(
        target: &T,
        pos: IVec3,
        steps: i32,
        came: usize,
        kind: LiquidKind,
        slope: i32,
    ) -> i32 {
        let mut cost = 1000;
        for (i, offset) in SIDES.iter().enumerate() {
            if i == came ^ 1 {
                continue;
            }
            let next = pos + *offset;
            if Self::blocks_flow(target, next, kind) || Self::is_source(target, next, kind) {
                continue;
            }
            if !Self::blocks_flow(target, next + IVec3::NEG_Y, kind) {
                return steps;
            }
            if steps < slope {
                cost = cost.min(Self::flow_cost(target, next, steps + 1, i, kind, slope));
            }
        }
        cost
    }

    /// The sides a liquid at `pos` spreads to: those with the cheapest way
    /// to a drop.
    fn directions<T: LiquidTarget + ?Sized>(target: &T, pos: IVec3, kind: LiquidKind) -> [bool; 4] {
        let slope = match kind {
            LiquidKind::Water => WATER_SLOPE,
            LiquidKind::Lava => LAVA_SLOPE,
        };
        let costs: [i32; 4] = std::array::from_fn(|i| {
            let next = pos + SIDES[i];
            if Self::blocks_flow(target, next, kind) || Self::is_source(target, next, kind) {
                return 1000;
            }
            if !Self::blocks_flow(target, next + IVec3::NEG_Y, kind) {
                0
            } else {
                Self::flow_cost(target, next, 1, i, kind, slope)
            }
        });
        let low = *costs.iter().min().unwrap_or(&1000);
        costs.map(|cost| cost == low)
    }

    /// A liquid of `kind` at `depth` flows into `pos` (if it can): it takes
    /// the place of air or a block it breaks, or fills a liquid layer.
    fn flow_into<T: LiquidTarget + ?Sized>(
        &mut self,
        target: &mut T,
        pos: IVec3,
        kind: LiquidKind,
        depth: u8,
    ) {
        let layer = match Self::meets(target, pos, kind) {
            Meets::Air => Layer::Block,
            Meets::Breaks(state) => {
                target.wash(pos, state, kind);
                Layer::Block
            }
            Meets::Fills => Layer::Liquid,
            Meets::Fluid(_) | Meets::Blocked => return,
        };
        self.put(target, pos, kind, Some(depth), layer);
        self.placed(target, pos);
        self.rewrote(target, pos);
    }

    /// One liquid tick at `pos`.
    fn tick<T: LiquidTarget + ?Sized>(&mut self, target: &mut T, pos: IVec3) {
        let Some(fluid) = Self::fluid(target, pos) else {
            return;
        };
        let kind = fluid.kind;
        let step: i32 = match kind {
            LiquidKind::Lava if !self.nether => 2,
            _ => 1,
        };
        let mut level = i32::from(fluid.depth);
        if level > 0 {
            let mut low = -1;
            let mut sources = 0;
            for offset in SIDES {
                let Some(next) = Self::fluid(target, pos + offset) else {
                    continue;
                };
                if next.kind != kind {
                    continue;
                }
                let mut depth = i32::from(next.depth);
                if depth == 0 {
                    sources += 1;
                }
                if depth >= 8 {
                    depth = 0;
                }
                if low < 0 || depth < low {
                    low = depth;
                }
            }
            let mut new = low + step;
            if new >= 8 || low < 0 {
                new = -1;
            }
            if let Some(above) = Self::fluid(target, pos + IVec3::Y)
                && above.kind == kind
            {
                let above = i32::from(above.depth);
                new = if above >= 8 { above } else { above + 8 };
            }
            if sources >= 2 && kind == LiquidKind::Water {
                let below = pos + IVec3::NEG_Y;
                if Self::blocks_flow(target, below, kind) || Self::is_source(target, below, kind) {
                    new = 0;
                }
            }
            if new != level {
                level = new;
                let depth = u8::try_from(new).ok();
                self.put(target, pos, kind, depth, fluid.layer);
                if depth.is_some() {
                    let delay = self.delay(kind);
                    self.schedule(pos, delay);
                }
                self.rewrote(target, pos);
            }
        }
        if level < 0 {
            return;
        }
        let below = pos + IVec3::NEG_Y;
        if kind == LiquidKind::Lava
            && Self::fluid(target, below).is_some_and(|f| f.kind == LiquidKind::Water)
        {
            let states = self.states(target.registry());
            target.write(below, states.stone);
            self.rewrote(target, below);
            return;
        }
        if Self::can_enter(target, below, kind) {
            let depth = if level >= 8 { level } else { level + 8 };
            self.flow_into(target, below, kind, depth as u8);
        } else if level == 0 || Self::blocks_flow(target, below, kind) {
            let next = if level >= 8 { 1 } else { level + step };
            if next >= 8 {
                return;
            }
            let ways = Self::directions(target, pos, kind);
            let exits = Self::exits(target, pos, fluid);
            for (i, offset) in SIDES.iter().enumerate() {
                if ways[i] && exits[i] {
                    self.flow_into(target, pos + *offset, kind, next as u8);
                }
            }
        }
    }

    /// Run one game tick: every chunk with ticks due, in a random order, its
    /// due ticks in the order they were scheduled.
    pub(crate) fn step<T: LiquidTarget + ?Sized>(&mut self, target: &mut T) {
        self.now += 1;
        self.ranks.clear();
        let mut chunks: Vec<(i32, i32)> = self
            .queue
            .iter()
            .filter(|(_, ticks)| ticks.keys().next().is_some_and(|due| *due <= self.now))
            .map(|(chunk, _)| *chunk)
            .collect();
        chunks.sort_unstable();
        let mut ranked: Vec<(u32, (i32, i32))> =
            chunks.into_iter().map(|c| (self.rank(c), c)).collect();
        ranked.sort_unstable();
        for (_, chunk) in ranked {
            self.ticking = Some(chunk);
            while let Some(pos) = self.pop_due(chunk) {
                self.pending.remove(&pos);
                self.tick(target, pos);
            }
        }
        self.ticking = None;
        self.queue.retain(|_, ticks| !ticks.is_empty());
    }

    fn pop_due(&mut self, chunk: (i32, i32)) -> Option<IVec3> {
        let ticks = self.queue.get_mut(&chunk)?;
        let mut entry = ticks.first_entry()?;
        if *entry.key() > self.now {
            return None;
        }
        let pos = entry.get_mut().remove(0);
        if entry.get().is_empty() {
            entry.remove();
        }
        Some(pos)
    }
}

/// A target whose writes and pops (from block updates the engine runs)
/// wake the liquids beside them.
struct Notify<'a, T: ?Sized> {
    target: &'a mut T,
    liquids: &'a mut Liquids,
}

impl<T: LiquidTarget + ?Sized> crate::entity::systems::BlockSource for Notify<'_, T> {
    fn block_at(&self, pos: IVec3) -> Option<u32> {
        self.target.block_at(pos)
    }

    fn registry(&self) -> &BlockRegistry {
        self.target.registry()
    }
}

impl<T: LiquidTarget + ?Sized> UpdateTarget for Notify<'_, T> {
    fn write(&mut self, pos: IVec3, state: u32) {
        self.target.write(pos, state);
        self.liquids.changed(self.target, pos);
    }

    fn pop(&mut self, pos: IVec3, state: u32) {
        self.target.pop(pos, state);
        self.liquids.changed(self.target, pos);
    }
}

/// Which storage a block set at a position keeps for the water there: a
/// block that holds water keeps a water source in its liquid layer; air
/// set over a block holding water leaves that water as the block; anything
/// else clears the liquid layer. Returns `(block, liquid layer)`.
pub(crate) fn stored(
    registry: &BlockRegistry,
    old_block: u32,
    old_layer: u32,
    new_block: u32,
) -> (u32, u32) {
    let air = *crate::world::chunk::blocks::AIR;
    let source = |state: u32| {
        registry
            .liquid_state(state)
            .is_some_and(|l| l.kind == LiquidKind::Water && l.depth == 0)
    };
    let water = if registry.liquid_state(old_layer).is_some() {
        Some(old_layer)
    } else if source(old_block) {
        Some(old_block)
    } else {
        None
    };
    let Some(water) = water else {
        return (new_block, air);
    };
    if new_block == air && registry.liquid_state(old_layer).is_some() {
        return (old_layer, air);
    }
    if holds_water(registry, new_block) {
        return (new_block, water);
    }
    (new_block, air)
}

/// The measured liquid facts of the block `state` (`liquids/blocks.json`).
fn facts(registry: &BlockRegistry, state: u32) -> Option<&'static LiquidBlock> {
    let entry = registry.get_by_runtime_id(state)?;
    unastar_data::liquids::blocks().get(&entry.string_id)
}

/// What a liquid of `kind` spreading beside it does to the block `state`.
pub(crate) fn reaction(
    registry: &BlockRegistry,
    state: u32,
    kind: LiquidKind,
) -> Option<&'static Reaction> {
    facts(registry, state).map(|facts| match kind {
        LiquidKind::Water => &facts.water,
        LiquidKind::Lava => &facts.lava,
    })
}

/// Whether the block `state` keeps water set into it (`liquid_holders`), or
/// takes in spreading water (`liquid_blocks_water`).
pub(crate) fn holds_water(registry: &BlockRegistry, state: u32) -> bool {
    facts(registry, state)
        .is_some_and(|facts| facts.holds_water() || facts.water == Reaction::Fills)
}

/// Index of `side` in [`SIDES`].
fn side_index(side: Side) -> usize {
    match side {
        Side::West => 0,
        Side::East => 1,
        Side::North => 2,
        Side::South => 3,
    }
}
