//! Block behaviour: placement states, survival, interaction, neighbour
//! updates and ticks, written once per behaviour family.
//!
//! Block identities, states and properties come from the registry (the
//! protocolgen BDS 1.26.51 block states and native tags); [`Family`] sorts
//! every block state into the behaviour family that reads them. What each
//! family does with its states is hand-written in one submodule per family
//! ([`doors`], [`attached`], [`crops`], [`plants`], [`toggles`],
//! [`connections`], [`beds`], [`signs`]).
//!
//! # One change path
//!
//! Every block change goes through [`super::blocks::set_block`]: it writes
//! the chunk, queues the change for the chunk's viewers (sent once, in the
//! session's block numbering, at the start of the next tick by
//! [`super::blocks::send_block_updates`]) and then calls [`block_changed`],
//! which queues a neighbour
//! update for each of the six neighbours (down, up, north, south, west,
//! east). Updates drain breadth-first from one queue: a change made while
//! handling an update queues further updates instead of recursing, so a
//! tower of attached blocks unwinds without deep stacks.
//!
//! A neighbour update first asks the family whether the block can still
//! stay where it is ([`can_survive`]). A block that cannot is destroyed
//! with its drops and destroy particles ([`destroy_block`]) one tick later,
//! from a scheduled tick that checks again (as BDS 1.26.51 does for
//! torches, crops, buttons, doors, cane, cacti); a bed half that lost its
//! partner goes at once without drops. Destroying a block updates its own
//! neighbours in turn, so a chain of attached blocks unwinds one tick per
//! link. Families whose states depend on their neighbours (fences, walls,
//! panes, stairs corners, fence gates next to walls, stems pointing at
//! their fruit, farmland covered by a block) then react.
//!
//! # Ticks
//!
//! [`schedule_tick`] queues a tick for one block after a delay (button
//! release); the tick only runs when the same block is still there.
//! Random ticks ([`tick_blocks`]): every chunk in a player's ticking
//! diamond (simulation distance) gets `(int)((top + 1) × 2.5) ×`
//! `randomTickSpeed` ([`random_tick_speed`]) random positions per game tick, `top` being its
//! highest sub-chunk with a non-air block; crops, stems, nether wart,
//! farmland, sugar cane and cacti grow or change there.
//!
//! [`tick_blocks`] also carries vibrations to sculk listeners
//! ([`utility::sculk`]) and counts down sleeping players ([`sleep`]).

pub mod actors;
pub mod attached;
pub mod beds;
pub mod connections;
pub mod crops;
pub mod doors;
pub mod fire;
pub mod gravity;
pub mod plants;
pub mod redstone;
pub mod signs;
pub mod sleep;
mod support;
pub mod toggles;
pub mod utility;

#[cfg(test)]
mod bds_parity;
#[cfg(test)]
pub(crate) mod tests;

use std::collections::VecDeque;

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::placement::PlacementInput;
use super::types::BlockRegistryResource;
use crate::ecs::resources::TickCounter;
use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::registry::block::{BlockEntry, StateValue};
use crate::world::chunk::{MIN_Y, SUBCHUNK_COUNT};
use crate::world::ecs::ChunkData;

pub use support::{SupportKind, provides_support};

/// `LevelEvent` id of block destroy particles (with the block's sound).
pub const LEVEL_EVENT_PARTICLES_DESTROY_BLOCK: i32 = 2001;
/// `LevelEvent` id of block destroy particles without a sound.
pub const LEVEL_EVENT_PARTICLES_DESTROY_BLOCK_NO_SOUND: i32 = 2021;

/// The six neighbour offsets in update order: down, up, north, south, west,
/// east (the protocol's face order).
pub const NEIGHBOURS: [IVec3; 6] = [
    IVec3::NEG_Y,
    IVec3::Y,
    IVec3::NEG_Z,
    IVec3::Z,
    IVec3::NEG_X,
    IVec3::X,
];

/// The four horizontal neighbour offsets: north, south, west, east.
pub const HORIZONTAL: [IVec3; 4] = [IVec3::NEG_Z, IVec3::Z, IVec3::NEG_X, IVec3::X];

/// Neighbour updates handled in one drain before the rest is dropped (a
/// guard against two blocks rewriting each other forever).
const MAX_UPDATES_PER_DRAIN: usize = 1 << 16;

/// Behaviour family of a block state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// No behaviour beyond the generic placement states.
    Plain,
    /// Two-block doors; iron doors open only by redstone.
    Door {
        iron: bool,
    },
    /// Trapdoors; iron trapdoors open only by redstone.
    Trapdoor {
        iron: bool,
    },
    FenceGate,
    /// Blocks with `torch_facing_direction` (torches of every kind).
    Torch,
    /// Buttons; `wooden` buttons stay pressed longer.
    Button {
        wooden: bool,
    },
    Lever,
    /// Two-block beds (`head_piece_bit`).
    Bed,
    StandingSign,
    WallSign,
    HangingSign,
    Ladder,
    Vine,
    /// Fences, walls, panes and bars: neighbour-derived connection states.
    Connected,
    /// Stairs with the neighbour-derived `minecraft:corner` state.
    Stairs,
    /// Farmland crops with `growth` 0..=7 (wheat, carrots, potatoes,
    /// beetroot).
    Crop,
    /// Melon and pumpkin stems.
    Stem,
    NetherWart,
    Farmland,
    SugarCane,
    Cactus,
    /// Single-block plants that need soil below (flowers, grass, ferns,
    /// saplings, mushrooms, dead bushes, ...).
    Plant,
    /// Two-block plants (`upper_block_bit`: tall grass, large ferns,
    /// sunflowers, ...).
    DoublePlant,
    /// Carpets and other thin blocks that need any non-air block below.
    Carpet,
    /// Water and lava. Their flow is not part of this module (an outside
    /// contributor owns liquids); lava's random tick may start fires
    /// ([`fire::lava_ignites`]).
    Liquid,
    /// Fire and soul fire ([`fire`]).
    Fire,
    /// Redstone components ([`redstone`]): dust, repeaters, comparators,
    /// pressure plates, lamps, note blocks, redstone blocks, observers and
    /// pistons. Redstone torches stay in [`Family::Torch`] for placement and
    /// support; [`redstone`] also runs their power logic.
    Redstone,
    /// Concrete powder: falls, and hardens into concrete next to water
    /// ([`gravity`]).
    ConcretePowder,
    /// Interactive utility blocks ([`utility`]): dispensers, rails, TNT,
    /// cauldrons, composters, beehives, campfires, lecterns, bells,
    /// jukeboxes, flower pots, cakes, portals, respawn anchors, sculk.
    Utility(utility::Utility),
}

impl Family {
    /// Classify `entry` (every state of a block shares its family).
    pub fn of(entry: &BlockEntry) -> Self {
        let id = entry.string_id.as_str();
        let state = entry.default_state_id;
        let has = |property: &str| entry.state_value(state, property).is_some();
        if matches!(
            id,
            "minecraft:water"
                | "minecraft:flowing_water"
                | "minecraft:lava"
                | "minecraft:flowing_lava"
        ) {
            return Self::Liquid;
        }
        if matches!(id, "minecraft:fire" | "minecraft:soul_fire") {
            return Self::Fire;
        }
        if let Some(kind) = utility::Utility::of(entry) {
            return Self::Utility(kind);
        }
        if id.ends_with("_concrete_powder") {
            return Self::ConcretePowder;
        }
        if redstone::is_component(entry) {
            return Self::Redstone;
        }
        if has("door_hinge_bit") {
            return Self::Door {
                iron: id == "minecraft:iron_door",
            };
        }
        if has("torch_facing_direction") {
            return Self::Torch;
        }
        if has("button_pressed_bit") {
            return Self::Button {
                wooden: entry.has_tag("wood") || !id.contains("stone"),
            };
        }
        if has("lever_direction") {
            return Self::Lever;
        }
        if has("head_piece_bit") {
            return Self::Bed;
        }
        if has("in_wall_bit") {
            return Self::FenceGate;
        }
        if entry.has_tag("trapdoors") {
            return Self::Trapdoor {
                iron: id == "minecraft:iron_trapdoor",
            };
        }
        if entry.has_tag("minecraft:cornerable_stairs") || has("minecraft:corner") {
            return Self::Stairs;
        }
        if has("wall_post_bit") || has("minecraft:connection_north") {
            return Self::Connected;
        }
        if entry.has_tag("text_sign") {
            return if has("hanging") {
                Self::HangingSign
            } else if has("ground_sign_direction") {
                Self::StandingSign
            } else {
                Self::WallSign
            };
        }
        match id {
            "minecraft:ladder" => return Self::Ladder,
            "minecraft:vine" => return Self::Vine,
            "minecraft:farmland" => return Self::Farmland,
            "minecraft:reeds" => return Self::SugarCane,
            "minecraft:cactus" => return Self::Cactus,
            "minecraft:nether_wart" => return Self::NetherWart,
            "minecraft:melon_stem" | "minecraft:pumpkin_stem" => return Self::Stem,
            _ => {}
        }
        if entry.has_tag("minecraft:crop") && id != "minecraft:sweet_berry_bush" && has("growth") {
            return Self::Crop;
        }
        if plants::is_double_plant(entry) {
            return Self::DoublePlant;
        }
        if plants::is_plant(entry) {
            return Self::Plant;
        }
        if id.ends_with("_carpet") || id == "minecraft:moss_carpet" {
            return Self::Carpet;
        }
        Self::Plain
    }

    /// Whether random ticks reach this family.
    pub fn random_ticks(self) -> bool {
        matches!(
            self,
            Self::Crop
                | Self::Stem
                | Self::NetherWart
                | Self::Farmland
                | Self::SugarCane
                | Self::Cactus
                | Self::Liquid
                | Self::Fire
        )
    }
}

/// [`Family`] of every block state, built once from the block registry.
#[derive(Resource, Debug)]
pub struct BlockFamilies {
    by_state: Vec<Family>,
}

impl BlockFamilies {
    pub fn new(registry: &BlockRegistry) -> Self {
        let mut by_state = Vec::new();
        for entry in registry.iter() {
            let family = Family::of(entry);
            let end = entry.max_state_id as usize + 1;
            if by_state.len() < end {
                by_state.resize(end, Family::Plain);
            }
            for slot in &mut by_state[entry.min_state_id as usize..end] {
                *slot = family;
            }
        }
        Self { by_state }
    }

    pub fn get(&self, state: u32) -> Family {
        self.by_state
            .get(state as usize)
            .copied()
            .unwrap_or(Family::Plain)
    }
}

/// The family of `state`, from the world's family table (built on first
/// use from the registry resource).
pub fn family(world: &mut World, state: u32) -> Family {
    if !world.contains_resource::<BlockFamilies>() {
        let families = BlockFamilies::new(&world.resource::<BlockRegistryResource>().0);
        world.insert_resource(families);
    }
    world.resource::<BlockFamilies>().get(state)
}

/// Family of `state` without a cached table (placement checks that only
/// hold a [`BlockSource`]).
pub fn family_of(registry: &BlockRegistry, state: u32) -> Family {
    registry
        .get_by_runtime_id(state)
        .map_or(Family::Plain, Family::of)
}

/// The game rule `name` (BDS's default when the world has no rules).
fn game_rule(world: &World, name: &str) -> Option<unastar_data::game_rules::GameRuleValue> {
    world
        .get_resource::<crate::world::rules::GameRules>()
        .and_then(|rules| rules.value(name))
        .map(|(_, value)| value)
}

/// The `randomTickSpeed` game rule: random ticks per chunk budget unit
/// (BDS's default is 1).
pub fn random_tick_speed(world: &World) -> u32 {
    match game_rule(world, "randomTickSpeed") {
        Some(unastar_data::game_rules::GameRuleValue::Int(speed)) => speed.max(0) as u32,
        _ => 1,
    }
}

/// The `doFireTick` game rule: whether fire spreads, ages and burns out
/// (on by default).
pub fn fire_ticks(world: &World) -> bool {
    !matches!(
        game_rule(world, "doFireTick"),
        Some(unastar_data::game_rules::GameRuleValue::Bool(false))
    )
}

/// Random source of block ticks and growth (seedable in tests).
#[derive(Resource, Debug)]
pub struct BlockRandom(pub StdRng);

impl Default for BlockRandom {
    fn default() -> Self {
        Self(StdRng::from_entropy())
    }
}

impl BlockRandom {
    pub fn seeded(seed: u64) -> Self {
        Self(StdRng::seed_from_u64(seed))
    }
}

/// A random draw from the world's [`BlockRandom`].
pub fn random(world: &mut World) -> impl std::ops::DerefMut<Target = StdRng> + '_ {
    world
        .get_resource_or_init::<BlockRandom>()
        .map_unchanged(|random| &mut random.0)
}

/// Block ticks scheduled for later: `(due tick, position, block id)`.
#[derive(Resource, Debug, Default)]
pub struct ScheduledTicks(Vec<(u64, IVec3, u32)>);

/// Neighbour updates waiting to run, and whether a drain is in progress.
#[derive(Resource, Debug, Default)]
struct NeighbourUpdates {
    /// `(block to update, changed neighbour)`.
    queue: VecDeque<(IVec3, IVec3)>,
    draining: bool,
}

/// Schedule a tick for the block at `pos` in `delay` ticks. It runs only if
/// the same block (any state of it) is still there then; one pending tick
/// per block and position.
pub fn schedule_tick(world: &mut World, pos: IVec3, delay: u64) {
    let Some(block) = world
        .block_at(pos)
        .and_then(|state| world.registry().get_by_runtime_id(state))
        .map(|entry| entry.id)
    else {
        return;
    };
    let due = world.get_resource::<TickCounter>().map_or(0, |t| t.current) + delay.max(1);
    let mut ticks = world.get_resource_or_init::<ScheduledTicks>();
    if !ticks.0.iter().any(|(_, p, b)| *p == pos && *b == block) {
        ticks.0.push((due, pos, block));
    }
}

/// Called by [`super::blocks::set_block`] after the block at `pos`
/// changed: update its six neighbours (see the module docs).
pub fn block_changed(world: &mut World, pos: IVec3) {
    let draining = {
        let mut updates = world.get_resource_or_init::<NeighbourUpdates>();
        // The changed block itself first (`from == pos`: placement reactions
        // such as fire scheduling its first tick), then its six neighbours.
        updates.queue.push_back((pos, pos));
        updates
            .queue
            .extend(NEIGHBOURS.iter().map(|offset| (pos + *offset, pos)));
        updates.draining
    };
    if !draining {
        drain(world);
    }
}

/// Handle queued neighbour updates until none are left.
fn drain(world: &mut World) {
    world.resource_mut::<NeighbourUpdates>().draining = true;
    let mut handled = 0;
    loop {
        let next = world.resource_mut::<NeighbourUpdates>().queue.pop_front();
        let Some((next, from)) = next else {
            break;
        };
        handled += 1;
        if handled > MAX_UPDATES_PER_DRAIN {
            tracing::warn!(?next, "neighbour update limit reached; dropping the rest");
            world.resource_mut::<NeighbourUpdates>().queue.clear();
            break;
        }
        neighbour_changed(world, next, from);
    }
    world.resource_mut::<NeighbourUpdates>().draining = false;
}

/// One neighbour update of the block at `pos`, caused by a change at
/// `from`.
fn neighbour_changed(world: &mut World, pos: IVec3, from: IVec3) {
    let Some(state) = world.block_at(pos) else {
        return;
    };
    let family = family(world, state);
    if from == pos {
        placed(world, pos, state, family);
        return;
    }
    // A solid block passes changes on to the redstone around it (a lever
    // broken on one side of a block unpowers the dust on the other side).
    if family != Family::Redstone && !redstone::is_consumer(family) {
        redstone::relay(world, pos, state, from);
    }
    if family == Family::Plain {
        return;
    }
    if !can_survive(world, pos, state) {
        if family == Family::Bed {
            // A bed half without its partner goes at once, without drops.
            particles(
                world,
                pos,
                state,
                LEVEL_EVENT_PARTICLES_DESTROY_BLOCK_NO_SOUND,
            );
            let air = air(&world.resource::<BlockRegistryResource>().0);
            super::blocks::set_block(world, pos, air, false);
        } else if family == Family::Fire {
            // Fire that cannot stay goes out at once, without particles.
            let air = air(&world.resource::<BlockRegistryResource>().0);
            super::blocks::set_block(world, pos, air, false);
        } else {
            // Everything else breaks with its drops on a tick one later.
            schedule_tick(world, pos, 1);
        }
        return;
    }
    match family {
        // Walls and fence gates rewrite their neighbour-derived states on a
        // tick one later; fences, panes, bars and stairs at once.
        Family::FenceGate => schedule_tick(world, pos, 1),
        Family::Connected if is_wall(world, state) => schedule_tick(world, pos, 1),
        Family::Connected | Family::Stairs => connections::neighbour_changed(world, pos, state),
        Family::Farmland => crops::farmland_neighbour_changed(world, pos, state),
        Family::Stem => detach_stem(world, pos, state, from),
        Family::Vine => attached::vine_neighbour_changed(world, pos, state),
        Family::Fire => fire::neighbour_changed(world, pos, state),
        Family::ConcretePowder => gravity::harden_concrete_powder(world, pos, state),
        Family::Utility(kind) => utility::neighbour_changed(world, kind, pos, state, from),
        _ => {}
    }
    // Redstone components and the blocks power acts on (doors, trapdoors,
    // fence gates, redstone torches, lamps, note blocks).
    if family == Family::Redstone || redstone::is_consumer(family) {
        redstone::neighbour_changed(world, pos, from);
    }
}

/// The block at `pos` was just set to `state` (placed, grown, toggled):
/// placement reactions. Survival is not checked here; placement already
/// refused blocks that could not stay.
fn placed(world: &mut World, pos: IVec3, state: u32, family: Family) {
    match family {
        Family::Fire => fire::placed(world, pos, state),
        Family::ConcretePowder => gravity::harden_concrete_powder(world, pos, state),
        Family::Utility(kind) => utility::placed(world, kind, pos, state),
        _ => {}
    }
    if family == Family::Redstone || redstone::is_consumer(family) {
        redstone::neighbour_changed(world, pos, pos);
    }
}

/// Whether `state` is lava (still or flowing).
fn is_lava(world: &World, state: u32) -> bool {
    world
        .resource::<BlockRegistryResource>()
        .0
        .liquid_state(state)
        .is_some_and(|liquid| liquid.kind == crate::registry::block::LiquidKind::Lava)
}

/// Whether `state` is a wall (it has `wall_post_bit`).
fn is_wall(world: &World, state: u32) -> bool {
    world
        .resource::<BlockRegistryResource>()
        .0
        .get_by_runtime_id(state)
        .is_some_and(|entry| entry.state_value(state, "wall_post_bit").is_some())
}

/// A stem pointing at its fruit (`facing_direction` 2..5) lets go (back to
/// 0) whenever the block in that direction changes, whatever it became.
fn detach_stem(world: &mut World, pos: IVec3, state: u32, from: IVec3) {
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return;
    };
    let facing = int(entry, state, "facing_direction").unwrap_or(0);
    let points_at = super::placement::Horizontal::from_facing(facing).map(|h| pos + h.offset());
    if points_at == Some(from) {
        let detached = with(entry, state, "facing_direction", StateValue::Int(0));
        super::blocks::set_block(world, pos, detached, false);
    }
}

/// Whether the block `state` can stay at `pos` (support, soil, the other
/// half of a two-block structure). Blocks without a survival rule always
/// can.
pub fn can_survive(blocks: &impl BlockSource, pos: IVec3, state: u32) -> bool {
    let registry = blocks.registry();
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return true;
    };
    match Family::of(entry) {
        Family::Door { .. } => doors::can_survive(blocks, pos, state, entry),
        Family::Bed => beds::can_survive(blocks, pos, state, entry),
        Family::Torch => attached::torch_can_survive(blocks, pos, state, entry),
        Family::Ladder => attached::ladder_can_survive(blocks, pos, state, entry),
        Family::Button { .. } | Family::Lever => {
            attached::switch_can_survive(blocks, pos, state, entry)
        }
        Family::Vine => attached::vine_can_survive(blocks, pos, state, entry),
        Family::StandingSign | Family::WallSign | Family::HangingSign => {
            signs::can_survive(blocks, pos, state, entry)
        }
        Family::Crop | Family::Stem => crops::crop_can_survive(blocks, pos, state, entry),
        Family::NetherWart => crops::nether_wart_can_survive(blocks, pos),
        Family::SugarCane => plants::sugar_cane_can_survive(blocks, pos),
        Family::Cactus => plants::cactus_can_survive(blocks, pos),
        Family::Plant => plants::plant_can_survive(blocks, pos, entry),
        Family::DoublePlant => plants::double_plant_can_survive(blocks, pos, state, entry),
        Family::Carpet => plants::carpet_can_survive(blocks, pos),
        Family::Fire => fire::can_survive(blocks, pos, state, entry),
        Family::Redstone => redstone::can_survive(blocks, pos, state, entry),
        Family::Utility(kind) => utility::can_survive(kind, blocks, pos, state, entry),
        Family::Plain
        | Family::Trapdoor { .. }
        | Family::FenceGate
        | Family::Connected
        | Family::Stairs
        | Family::Farmland
        | Family::Liquid
        | Family::ConcretePowder => true,
    }
}

/// What a click places: the state of `block` placed at `position` by
/// `input`, starting from the generic placement state `state` (pillar
/// axis, slab half, stairs direction, facing). `None` refuses the
/// placement.
pub fn placement_state(
    blocks: &impl BlockSource,
    block: &BlockEntry,
    state: u32,
    input: &PlacementInput,
    position: IVec3,
) -> Option<u32> {
    let place = Place {
        blocks,
        block,
        state,
        input,
        position,
    };
    let state = match Family::of(block) {
        Family::Door { .. } => doors::placement(&place)?,
        Family::Bed => beds::placement(&place)?,
        Family::Torch => attached::torch_placement(&place)?,
        Family::Ladder => attached::ladder_placement(&place)?,
        Family::Button { .. } | Family::Lever => attached::switch_placement(&place)?,
        Family::Vine => attached::vine_placement(&place)?,
        Family::StandingSign | Family::WallSign | Family::HangingSign => signs::placement(&place)?,
        Family::Trapdoor { .. } => toggles::trapdoor_placement(&place)?,
        Family::FenceGate => toggles::fence_gate_placement(&place)?,
        Family::Connected | Family::Stairs => connections::placement(&place)?,
        Family::Redstone => redstone::placement(&place)?,
        Family::Utility(kind) => utility::placement(kind, &place)?,
        _ => state,
    };
    let mut placed = vec![(position, state)];
    placed.extend(parts(blocks, position, state));
    let view = WithBlocks::new(blocks, placed.clone());
    placed
        .iter()
        .all(|(pos, part)| can_survive(&view, *pos, *part))
        .then_some(state)
}

/// The other blocks a structure placed as `state` at `position` takes (the
/// upper door half, the bed head).
pub fn parts(blocks: &impl BlockSource, position: IVec3, state: u32) -> Vec<(IVec3, u32)> {
    let Some(entry) = blocks.registry().get_by_runtime_id(state) else {
        return Vec::new();
    };
    match Family::of(entry) {
        Family::Door { .. } => doors::parts(blocks, entry, position, state),
        Family::Bed => beds::parts(entry, position, state),
        Family::DoublePlant => plants::double_plant_parts(entry, position, state),
        _ => Vec::new(),
    }
}

/// The block an item places, from the item's data where it has any:
///
/// - `minecraft:seed` places its `crop_result` block;
/// - `minecraft:block_placer` places its `block`;
/// - a sign item (`<wood>_sign`) places that wood's standing sign (oak's is
///   `standing_sign`, dark oak's `darkoak_standing_sign`; the wall variant
///   is chosen by [`signs::placement`]); `sugar_cane` places `reeds`,
///   `redstone` places `redstone_wire`, `repeater` and `comparator` place
///   their unpowered blocks;
/// - every other item places the block of the same name, if there is one.
pub fn item_block<'a>(
    registry: &'a BlockRegistry,
    item: &crate::registry::item::ItemEntry,
) -> Option<&'a BlockEntry> {
    let components = item.components();
    let named = |name: &str| {
        if name.contains(':') {
            registry.get_by_name(name)
        } else {
            registry.get_by_name(&format!("minecraft:{name}"))
        }
    };
    if let Some(seed) = &components.seed {
        return named(&seed.crop_result);
    }
    if let Some(placer) = &components.block_placer {
        return named(&placer.block);
    }
    let id = item.identifier();
    let placed = match id {
        "minecraft:sugar_cane" => Some("minecraft:reeds"),
        "minecraft:redstone" => Some("minecraft:redstone_wire"),
        "minecraft:repeater" => Some("minecraft:unpowered_repeater"),
        "minecraft:comparator" => Some("minecraft:unpowered_comparator"),
        _ => None,
    };
    if let Some(block) = placed {
        return registry.get_by_name(block);
    }
    if let Some(wood) = id.strip_suffix("_sign").filter(|wood| {
        !wood.ends_with("_hanging") && !wood.ends_with("_standing") && !wood.ends_with("_wall")
    }) {
        let wood = wood.trim_start_matches("minecraft:");
        let found = if wood == "oak" {
            registry.get_by_name("minecraft:standing_sign")
        } else {
            registry
                .get_by_name(&format!("minecraft:{wood}_standing_sign"))
                .or_else(|| {
                    registry.get_by_name(&format!(
                        "minecraft:{}_standing_sign",
                        wood.replace('_', "")
                    ))
                })
        };
        if found.is_some() {
            return found;
        }
    }
    registry.get_by_name(id)
}

/// Whether a click with a seed item on the block `clicked` (face `face`)
/// may plant: the clicked block must be one of the seed's `plant_at` blocks
/// (farmland when none are listed) and the face its `plant_at_face` (the
/// top when unset). Seeds with `plant_at_any_solid_surface` are not
/// planted by this rule.
pub fn seed_plants_on(
    seed: &unastar_data::item_components::Seed,
    clicked: &BlockEntry,
    face: u8,
) -> bool {
    if seed.plant_at_any_solid_surface {
        return false;
    }
    let wanted_face = match seed.plant_at_face.as_deref() {
        None | Some("UP") => 1,
        Some("DOWN") => 0,
        Some(_) => return false,
    };
    let name = clicked.string_id.trim_start_matches("minecraft:");
    let on = if seed.plant_at.is_empty() {
        name == "farmland"
    } else {
        seed.plant_at
            .iter()
            .any(|block| block.trim_start_matches("minecraft:") == name)
    };
    on && face == wanted_face
}

/// Whether a click holding the item that places `held` may place `placed`:
/// the same block, or the wall variant of a standing sign
/// (`<wood>_standing_sign` → `<wood>_wall_sign`).
pub fn same_placeable(held: &BlockEntry, placed: &BlockEntry) -> bool {
    held.string_id == placed.string_id
        || (held.string_id.ends_with("standing_sign")
            && placed.string_id == held.string_id.replace("standing_sign", "wall_sign"))
}

/// A player's `use` on the block at `pos` (doors, trapdoors, fence gates,
/// buttons, levers). Returns whether the block reacted (then the click
/// places nothing).
pub fn use_block(world: &mut World, player: Entity, pos: IVec3) -> bool {
    let Some(state) = world.block_at(pos) else {
        return false;
    };
    match family(world, state) {
        Family::Door { iron: false } => doors::toggle(world, pos, state, Some(player)),
        Family::Trapdoor { iron: false } => {
            toggles::toggle_trapdoor(world, pos, state, Some(player))
        }
        Family::FenceGate => toggles::toggle_fence_gate(world, player, pos, state),
        Family::Button { wooden } => toggles::press_button(world, pos, state, wooden, Some(player)),
        Family::Lever => toggles::toggle_lever(world, pos, state, Some(player)),
        Family::Redstone => redstone::use_block(world, player, pos, state),
        Family::Utility(kind) => utility::use_block(world, kind, player, pos, state),
        Family::Bed => beds::use_bed(world, player, pos, state),
        _ => false,
    }
}

/// Whether `use` on `state` is a block interaction (the client then does
/// not expect a placement even with a block in hand).
pub fn is_usable(registry: &BlockRegistry, state: u32) -> bool {
    match family_of(registry, state) {
        Family::Door { iron: false }
        | Family::Trapdoor { iron: false }
        | Family::FenceGate
        | Family::Button { .. }
        | Family::Lever
        | Family::Bed => true,
        Family::Redstone => registry
            .get_by_runtime_id(state)
            .is_some_and(redstone::is_usable),
        Family::Utility(kind) => registry
            .get_by_runtime_id(state)
            .is_some_and(|entry| utility::is_usable(kind, entry, state)),
        _ => false,
    }
}

/// A player is breaking the block `state` at `pos` (before anything is
/// removed): blocks holding items drop them.
pub fn breaking(world: &mut World, pos: IVec3, state: u32) {
    if let Family::Utility(kind) = family(world, state) {
        utility::breaking(world, kind, pos, state);
    }
}

/// A player is breaking the block `state` at `pos`: remove the other parts
/// of its structure (door half, bed half, upper plant half) without drops
/// and return the state whose drops the break yields (the lower door half
/// carries the door item whichever half was hit).
pub fn break_structure(world: &mut World, pos: IVec3, state: u32) -> u32 {
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return state;
    };
    let (partners, drop_state) = match Family::of(entry) {
        Family::Door { .. } => doors::structure(entry, pos, state),
        Family::Bed => beds::structure(entry, pos, state),
        Family::DoublePlant => plants::double_plant_structure(entry, pos, state),
        _ => return state,
    };
    for partner in partners {
        let same = world
            .block_at(partner)
            .is_some_and(|other| (entry.min_state_id..=entry.max_state_id).contains(&other));
        if same {
            if let Some(partner_state) = world.block_at(partner) {
                particles(
                    world,
                    partner,
                    partner_state,
                    LEVEL_EVENT_PARTICLES_DESTROY_BLOCK,
                );
            }
            super::blocks::set_block(world, partner, air(&registry), false);
        }
    }
    drop_state
}

/// Destroy the block at `pos`: air, destroy particles and sound for its
/// viewers and, with `drops`, its block drops (no tool).
pub fn destroy_block(world: &mut World, pos: IVec3, drops: bool) {
    let Some(state) = world.block_at(pos) else {
        return;
    };
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let air = air(&registry);
    if state == air {
        return;
    }
    particles(world, pos, state, LEVEL_EVENT_PARTICLES_DESTROY_BLOCK);
    super::blocks::set_block(world, pos, air, false);
    if drops {
        super::block_drops::drop_block_items(world, pos, state, None);
    }
}

/// Play the `LevelSoundEvent` `name` at the centre of `pos` for its
/// viewers, carrying the wire id of `state`; it goes out with the next
/// tick's block updates ([`super::blocks::queue_block_sound`]).
pub fn sound(world: &mut World, pos: IVec3, name: &str, state: u32) {
    let wire = super::types::block_network_ids(world).to_wire(state);
    let packet = jolyne::valentine::McpePacket::from(jolyne::valentine::LevelSoundEventPacket {
        sound_event: name.into(),
        position: jolyne::valentine::types::Vec3 {
            x: pos.x as f32 + 0.5,
            y: pos.y as f32 + 0.5,
            z: pos.z as f32 + 0.5,
        },
        data: wire as i32,
        actor_identifier: "".into(),
        is_baby: false,
        is_global: false,
        actor_unique_id: 0,
        fire_at_position: None,
    });
    super::blocks::queue_block_sound(world, centre(pos), packet);
}

/// Send the viewers of `pos` the destroy particles `event` of `state`.
pub fn particles(world: &World, pos: IVec3, state: u32, event: i32) {
    let wire = super::types::block_network_ids(world).to_wire(state);
    crate::entity::mob::net::sound::level_event(world, centre(pos), event, wire as i32);
}

/// A `LevelEventGeneric` packet of `event_id` carrying `fields` as its
/// compound.
pub fn generic_event(
    event_id: i32,
    fields: &std::collections::BTreeMap<String, unastar_packs::nbt::Tag>,
) -> jolyne::valentine::McpePacket {
    let mut nbt =
        unastar_packs::nbt::encode_network_fields(fields).expect("number and string tags encode");
    // The compound's end tag.
    nbt.push(0);
    jolyne::valentine::McpePacket::from(jolyne::valentine::LevelEventGenericPacket {
        event_id,
        __ctd__: valentine::bedrock::codec::NbtCompoundBody(nbt.into()),
    })
}

/// The centre of the block at `pos`.
pub fn centre(pos: IVec3) -> DVec3 {
    pos.as_dvec3() + DVec3::splat(0.5)
}

/// The air state.
pub fn air(registry: &BlockRegistry) -> u32 {
    registry
        .default_state_id_by_name("minecraft:air")
        .unwrap_or(*crate::world::chunk::blocks::AIR)
}

/// `state` with `property` set, or `state` unchanged when the block has no
/// such property or value.
pub fn with(entry: &BlockEntry, state: u32, property: &str, value: StateValue) -> u32 {
    entry
        .with_state_value(state, property, value)
        .unwrap_or(state)
}

/// Integer state property (`Int` or `Byte`).
pub fn int(entry: &BlockEntry, state: u32, property: &str) -> Option<i32> {
    match entry.state_value(state, property)? {
        StateValue::Int(value) => Some(value),
        StateValue::Byte(value) => Some(i32::from(value)),
        StateValue::Str(_) => None,
    }
}

/// Whether a byte state property is set.
pub fn bit(entry: &BlockEntry, state: u32, property: &str) -> bool {
    int(entry, state, property).is_some_and(|value| value != 0)
}

/// Name of the block at `pos` (`None` when unloaded).
pub fn name_at(blocks: &impl BlockSource, pos: IVec3) -> Option<&str> {
    let state = blocks.block_at(pos)?;
    blocks
        .registry()
        .get_by_runtime_id(state)
        .map(|entry| entry.string_id.as_str())
}

/// One placement decision's inputs.
pub struct Place<'a, B: BlockSource> {
    pub blocks: &'a B,
    pub block: &'a BlockEntry,
    /// The generic placement state.
    pub state: u32,
    pub input: &'a PlacementInput,
    /// The cell the block goes into.
    pub position: IVec3,
}

/// A [`BlockSource`] that sees extra blocks (the ones being placed).
pub struct WithBlocks<'a, B: BlockSource> {
    inner: &'a B,
    blocks: Vec<(IVec3, u32)>,
}

impl<'a, B: BlockSource> WithBlocks<'a, B> {
    pub fn new(inner: &'a B, blocks: Vec<(IVec3, u32)>) -> Self {
        Self { inner, blocks }
    }
}

impl<B: BlockSource> BlockSource for WithBlocks<'_, B> {
    fn block_at(&self, pos: IVec3) -> Option<u32> {
        self.blocks
            .iter()
            .find(|(p, _)| *p == pos)
            .map(|(_, state)| *state)
            .or_else(|| self.inner.block_at(pos))
    }

    fn registry(&self) -> &BlockRegistry {
        self.inner.registry()
    }
}

/// Run `change` with neighbour updates held back until it returns, so the
/// blocks of a structure placed or removed together never see each other
/// half-done (a door's lower half is not destroyed for lacking its upper
/// half while both are being placed).
pub fn deferred<T>(world: &mut World, change: impl FnOnce(&mut World) -> T) -> T {
    let owner = {
        let mut updates = world.get_resource_or_init::<NeighbourUpdates>();
        !std::mem::replace(&mut updates.draining, true)
    };
    let result = change(world);
    if owner {
        world.resource_mut::<NeighbourUpdates>().draining = false;
        drain(world);
    }
    result
}

/// Exclusive system: run due scheduled ticks, vibrations on their way to
/// sculk listeners, the redstone changes due on this circuit tick, random
/// ticks, then sleeping players.
pub fn tick_blocks(world: &mut World) {
    let now = world.get_resource::<TickCounter>().map_or(0, |t| t.current);
    let due: Vec<(IVec3, u32)> = match world.get_resource_mut::<ScheduledTicks>() {
        None => Vec::new(),
        Some(mut ticks) => {
            let mut due: Vec<_> = Vec::new();
            ticks.0.retain(|(at, pos, block)| {
                if *at <= now {
                    due.push((*at, *pos, *block));
                    false
                } else {
                    true
                }
            });
            due.sort_by_key(|(at, _, _)| *at);
            due.into_iter()
                .map(|(_, pos, block)| (pos, block))
                .collect()
        }
    };
    for (pos, block) in due {
        let Some(state) = world.block_at(pos) else {
            continue;
        };
        let same = world
            .registry()
            .get_by_runtime_id(state)
            .is_some_and(|entry| entry.id == block);
        if same {
            scheduled_tick(world, pos, state);
        }
    }
    // Vibrations travelling to sculk sensors and shriekers.
    utility::sculk::tick(world);
    // Redstone changes due on this circuit tick.
    redstone::circuit::tick(world);
    random_ticks(world);
    sleep::tick(world);
}

/// A due scheduled tick: a block that can no longer stay breaks with its
/// drops; a pressed button releases; farmland under a solid block turns to
/// dirt; walls and fence gates rewrite their neighbour-derived states.
fn scheduled_tick(world: &mut World, pos: IVec3, state: u32) {
    if !can_survive(world, pos, state) {
        if family(world, state) == Family::Fire {
            let air = air(&world.resource::<BlockRegistryResource>().0);
            super::blocks::set_block(world, pos, air, false);
        } else {
            destroy_block(world, pos, true);
        }
        return;
    }
    match family(world, state) {
        Family::Button { .. } => toggles::release_button(world, pos, state),
        Family::Farmland => crops::farmland_tick(world, pos, state),
        Family::FenceGate => toggles::fence_gate_neighbour_changed(world, pos, state),
        Family::Connected => connections::neighbour_changed(world, pos, state),
        Family::Fire => fire::scheduled_tick(world, pos, state),
        Family::Redstone | Family::Torch => redstone::scheduled_tick(world, pos, state),
        Family::Utility(kind) => utility::scheduled_tick(world, kind, pos, state),
        _ => {}
    }
}

/// The chunk offsets around a player that get random ticks: a diamond of
/// radius `n` (rows at `z = ±(n − i)` span `x ∈ [−(i + 1), i + 1]` for
/// `i < n`, the row `z = 0` spans `[−n, n]`; 57 chunks for `n = 4`).
fn ticking_offsets(n: i32) -> impl Iterator<Item = (i32, i32)> {
    let rows = (0..n).flat_map(move |i| {
        let z = n - i;
        (-(i + 1)..=i + 1).flat_map(move |x| [(x, z), (x, -z)])
    });
    rows.chain((-n..=n).map(|x| (x, 0)))
}

/// Random ticks: every chunk in a player's ticking diamond (simulation
/// distance) gets, once per tick, `(int)((top + 1) × 2.5) × speed` random
/// positions, `top` being the index of its highest sub-chunk with a
/// non-air block (counted from the bottom); a position's height is drawn
/// over all sub-chunks up to that one. Each position's block runs its
/// random tick when its family has one.
fn random_ticks(world: &mut World) {
    let speed = random_tick_speed(world);
    if speed == 0 {
        return;
    }
    let distance = world
        .get_resource::<crate::world::ecs::ChunkLoadConfig>()
        .map_or(4, |config| config.simulation_distance);
    let players: Vec<IVec3> = world
        .query_filtered::<&crate::entity::components::Position, With<crate::entity::components::PlayerSession>>()
        .iter(world)
        .filter(|position| position.0.is_finite())
        .map(|position| position.0.floor().as_ivec3())
        .collect();
    if players.is_empty() {
        return;
    }
    if !world.contains_resource::<BlockFamilies>() {
        let families = BlockFamilies::new(&world.resource::<BlockRegistryResource>().0);
        world.insert_resource(families);
    }
    let mut chunks: Vec<(i32, i32)> = Vec::new();
    for player in players {
        let (cx, cz) = (player.x >> 4, player.z >> 4);
        for (dx, dz) in ticking_offsets(distance) {
            let chunk = (cx + dx, cz + dz);
            if !chunks.contains(&chunk) {
                chunks.push(chunk);
            }
        }
    }
    let mut picks: Vec<IVec3> = Vec::new();
    let mut rng = world.remove_resource::<BlockRandom>().unwrap_or_default();
    {
        let families = world.resource::<BlockFamilies>();
        let manager = world.resource::<crate::world::ecs::ChunkManager>();
        for (cx, cz) in chunks {
            let Some(data) = manager
                .get_by_coords(cx, cz)
                .and_then(|entity| world.get::<ChunkData>(entity))
            else {
                continue;
            };
            let Some(top) = (0..i32::from(SUBCHUNK_COUNT))
                .rev()
                .find(|index| !data.inner.is_subchunk_empty((MIN_Y >> 4) + index))
            else {
                continue;
            };
            let count = ((top + 1) as f32 * 2.5) as u32 * speed;
            let height = (top + 1) * 16;
            for _ in 0..count {
                let (x, y, z) = (
                    rng.0.gen_range(0..16),
                    rng.0.gen_range(0..height),
                    rng.0.gen_range(0..16),
                );
                let y = MIN_Y + y;
                let state = data.inner.get_block(x as u8, y as i16, z as u8);
                if families.get(state).random_ticks() {
                    picks.push(IVec3::new(cx * 16 + x, y, cz * 16 + z));
                }
            }
        }
    }
    world.insert_resource(rng);
    for pos in picks {
        let Some(state) = world.block_at(pos) else {
            continue;
        };
        random_tick(world, pos, state);
    }
}

/// One random tick of the block `state` at `pos`.
pub fn random_tick(world: &mut World, pos: IVec3, state: u32) {
    match family(world, state) {
        Family::Crop => crops::crop_random_tick(world, pos, state),
        Family::Stem => crops::stem_random_tick(world, pos, state),
        Family::NetherWart => crops::nether_wart_random_tick(world, pos, state),
        Family::Farmland => crops::farmland_random_tick(world, pos, state),
        Family::SugarCane => plants::sugar_cane_random_tick(world, pos, state),
        Family::Cactus => plants::cactus_random_tick(world, pos, state),
        Family::Liquid if is_lava(world, state) => fire::lava_ignites(world, pos),
        Family::Utility(kind) => utility::random_tick(world, kind, pos, state),
        Family::Fire => fire::random_tick(world, pos, state),
        _ => {}
    }
}

/// Bone meal on the block at `pos`: grow it if it is a crop, stem or nether
/// wart that can still grow. Returns whether it grew (the bone meal is then
/// used).
pub fn fertilize(world: &mut World, pos: IVec3) -> bool {
    let Some(state) = world.block_at(pos) else {
        return false;
    };
    match family(world, state) {
        Family::Crop | Family::Stem => crops::fertilize(world, pos, state),
        Family::SugarCane => plants::sugar_cane_fertilize(world, pos, state),
        _ => false,
    }
}

/// Exclusive system, before the landings are resolved: players and mobs
/// that landed on farmland this tick may trample it
/// ([`crops::trample`]). Mobs need a bounding box of at least 0.512 cubic
/// blocks; players need a game mode that may break blocks. The landing
/// block is the one 0.2 below the feet, as for fall damage.
pub fn trample_farmland(world: &mut World) {
    use crate::entity::components::{FallDistance, GameMode, PhysicsBody, Position};
    use crate::entity::mob::MobKind;
    use crate::entity::mob::state::Dead;
    use crate::entity::player_life::environment::PlayerFall;

    const MIN_VOLUME: f64 = 0.512;
    let probe = crate::entity::mob::constants::FALL_LANDING_PROBE;
    let below = |feet: DVec3| {
        IVec3::new(
            feet.x.floor() as i32,
            (feet.y - probe).floor() as i32,
            feet.z.floor() as i32,
        )
    };
    let mut landings: Vec<(IVec3, f64, bool)> = world
        .query::<(&Position, &PlayerFall, &GameMode)>()
        .iter(world)
        .filter(|(_, fall, mode)| fall.landing.is_some() && mode.can_break_blocks())
        .map(|(position, fall, _)| {
            let feet = position.0 - DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
            (below(feet), fall.landing.unwrap_or(0.0), true)
        })
        .collect();
    landings.extend(
        world
            .query_filtered::<(&Position, &FallDistance, &PhysicsBody), (With<MobKind>, Without<Dead>)>()
            .iter(world)
            .filter(|(_, fall, body)| {
                fall.landed.is_some() && body.width * body.height * body.width >= MIN_VOLUME
            })
            .map(|(position, fall, _)| (below(position.0), fall.landed.unwrap_or(0.0), false)),
    );
    for (pos, distance, is_player) in landings {
        crops::trample(world, pos, distance as f32, is_player);
    }
}
