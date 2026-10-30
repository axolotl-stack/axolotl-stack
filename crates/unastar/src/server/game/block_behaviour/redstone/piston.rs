//! Pistons and sticky pistons (`piston`/`sticky_piston`,
//! `facing_direction`), their arm (`piston_arm_collision`/
//! `sticky_piston_arm_collision`) and the blocks they move
//! (`moving_block`).
//!
//! # Direction
//!
//! `facing_direction` 0 pushes down and 1 up; the horizontal values name
//! the side opposite the push (2 pushes south, 3 north, 4 east, 5 west),
//! the wire meaning the Bedrock client renders (provisional: public
//! Bedrock server implementations). A placed piston faces the player:
//! up when the pitch is steeper than 45° down, down when steeper than 45°
//! up, else towards the player horizontally. The arm carries the piston's
//! own `facing_direction` value.
//!
//! # Power
//!
//! A piston is powered by any of its six neighbours except the one in
//! front; there is no quasi-connectivity (power next to the block above
//! does nothing), as Bedrock does. A powered piston without its arm
//! extends; an unpowered one with its arm retracts. A move in progress
//! finishes first; power is read again when it ends.
//!
//! # Moving
//!
//! - Extending pushes the line of blocks in front, plus the blocks slime
//!   and honey blocks drag along (slime and honey do not stick to each
//!   other), at most 12 blocks, as BDS 1.26.51 limits it; more, or an
//!   immovable block in the way, and nothing happens. Blocks that break
//!   when pushed end the line and drop as items (plants, torches, redstone
//!   parts, doors, ...); liquids and fire are pushed away without drops.
//! - Retracting removes the arm; a sticky piston pulls the block in front
//!   of its arm (and what sticks to it) back one block, unless that block
//!   breaks when pushed, is push-only (glazed terracotta) or cannot move.
//! - Immovable: obsidian (all kinds), respawn anchors, reinforced
//!   deepslate, enchanting tables, ender chests, beacons, spawners,
//!   vaults, conduits, unbreakable blocks (bedrock, barriers, portals,
//!   command and structure blocks), beds, extended or moving pistons,
//!   arms and moving blocks, and every block that keeps actor data in its
//!   chunk (containers, signs with text, ...; BDS moves many of these with
//!   their data, which this server cannot carry yet). Retracted pistons
//!   move. The block lists are provisional (public Bedrock behaviour).
//! - A move takes 2 game ticks (the arm advances 0.5 per tick), as BDS
//!   1.26.51 does: when it starts, the moved blocks leave their cells and
//!   appear as `moving_block` in their destinations, and an extending
//!   piston's arm takes the cell in front; when it ends, the moved blocks
//!   land as themselves and a retracting piston's arm goes. The start
//!   plays LevelSoundEvent `piston.out` (extending) or `piston.in`
//!   (retracting), as BDS 1.26.51 does.
//!
//! # Block actors
//!
//! The client animates the arm and the moving blocks from block actors,
//! which this module keeps in the chunk and sends as `BlockActorData`:
//!
//! - the piston's `PistonArm` (`State` 0 retracted, 1 extending,
//!   2 extended, 3 retracting, with `NewState`, `Progress`,
//!   `LastProgress`, `Sticky`, `isMovable`, and the moved and broken
//!   cells as flat `x, y, z` Int lists `AttachedBlocks`/`BreakBlocks`),
//!   sent when a move starts and when it ends. A retracted piston at rest
//!   keeps no actor (the client's default is retracted);
//! - each moving block's `MovingBlock` (`movingBlock` and
//!   `movingBlockExtra` as `{name, states, version}`, `pistonPosX/Y/Z`,
//!   `expanding`, `isMovable`), removed when the block lands.
//!
//! The field layout and the block `version` are provisional (public
//! Bedrock server implementations).
//!
//! # Breaking
//!
//! An arm whose piston is gone disappears without drops; a piston whose
//! arm was broken while extended breaks with its drops (provisional:
//! public Bedrock behaviour). Moving blocks whose piston is gone, or that
//! outlived the server's record of their move, land at once on their next
//! neighbour update.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use glam::IVec3;
use jolyne::valentine::{BlockActorDataPacket, BlockPos, McpePacket};
use zuri_nbt::{NBTTag, tag};

use super::super::{
    BlockFamilies, Family, LEVEL_EVENT_PARTICLES_DESTROY_BLOCK, NEIGHBOURS, air, break_structure,
    centre, deferred, int, particles, schedule_tick, sound, with,
};
use super::{Kind, kind, observer, power};
use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::registry::block::{BlockEntry, StateValue};
use crate::server::game::blocks::set_block;
use crate::server::game::placement::Horizontal;
use crate::server::game::types::BlockRegistryResource;
use crate::world::block_entity::BlockEntity;
use crate::world::chunk::{MIN_Y, SUBCHUNK_COUNT};
use crate::world::ecs::{ChunkData, ChunkManager, ChunkStateFlags, world_to_chunk_coords};

/// Push direction state of pistons and arms (see the module docs).
pub const FACING: &str = "facing_direction";
/// Most blocks one move takes along.
pub const PUSH_LIMIT: usize = 12;
/// Game ticks one move lasts.
pub const MOVE_TICKS: u8 = 2;
/// LevelSoundEvent of an extending piston.
pub const SOUND_OUT: &str = "piston.out";
/// LevelSoundEvent of a retracting piston.
pub const SOUND_IN: &str = "piston.in";

const MOVING_BLOCK: &str = "minecraft:moving_block";
const ARM: &str = "minecraft:piston_arm_collision";
const STICKY_ARM: &str = "minecraft:sticky_piston_arm_collision";
const ARM_ACTOR: &str = "PistonArm";
const MOVING_ACTOR: &str = "MovingBlock";
/// Block state version written with moved blocks (1.26.51.0).
const BLOCK_VERSION: i32 = (1 << 24) | (26 << 16) | (51 << 8);

/// `PistonArm` `State` values.
const RETRACTED: u8 = 0;
const EXTENDING: u8 = 1;
const EXTENDED: u8 = 2;
const RETRACTING: u8 = 3;

/// Blocks no piston moves (besides unbreakable ones).
const IMMOVABLE: [&str; 12] = [
    "minecraft:obsidian",
    "minecraft:crying_obsidian",
    "minecraft:glowingobsidian",
    "minecraft:respawn_anchor",
    "minecraft:reinforced_deepslate",
    "minecraft:enchanting_table",
    "minecraft:ender_chest",
    "minecraft:beacon",
    "minecraft:mob_spawner",
    "minecraft:trial_spawner",
    "minecraft:vault",
    "minecraft:conduit",
];

/// A move in progress.
#[derive(Debug, Clone)]
struct Motion {
    extending: bool,
    /// Game ticks run so far.
    ticks: u8,
    /// Destination and state of every moved block.
    moved: Vec<(IVec3, u32)>,
}

/// Moves in progress, by piston position.
#[derive(Resource, Debug, Default)]
pub struct PistonMotions(HashMap<IVec3, Motion>);

/// How a block takes a push.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reaction {
    /// Nothing there: a line of pushed blocks ends.
    Empty,
    Move,
    /// Moves when pushed, never when pulled or dragged.
    PushOnly,
    /// Breaks when pushed and ends the line.
    Break,
    /// Cannot move.
    Block,
}

/// The push direction a piston or arm state names.
pub fn direction(entry: &BlockEntry, state: u32) -> Option<IVec3> {
    Some(match int(entry, state, FACING)? {
        0 => IVec3::NEG_Y,
        1 => IVec3::Y,
        2 => IVec3::Z,
        3 => IVec3::NEG_Z,
        4 => IVec3::X,
        5 => IVec3::NEG_X,
        _ => return None,
    })
}

/// The placed state: facing the placing player (`yaw`/`pitch`).
pub fn placement(entry: &BlockEntry, state: u32, yaw: f32, pitch: f32) -> u32 {
    let facing = if pitch > 45.0 {
        1
    } else if pitch < -45.0 {
        0
    } else {
        // The horizontal value names the side away from the push, so the
        // look direction itself makes the piston push towards the player.
        Horizontal::from_yaw(yaw).facing()
    };
    with(entry, state, FACING, StateValue::Int(facing))
}

fn in_height(y: i32) -> bool {
    (MIN_Y..MIN_Y + i32::from(SUBCHUNK_COUNT) * 16).contains(&y)
}

fn family(world: &World, entry: &BlockEntry, state: u32) -> Family {
    world
        .get_resource::<BlockFamilies>()
        .map_or_else(|| Family::of(entry), |families| families.get(state))
}

fn moving(world: &World, pos: IVec3) -> bool {
    world
        .get_resource::<PistonMotions>()
        .is_some_and(|motions| motions.0.contains_key(&pos))
}

/// Whether the piston `state` at `pos` has its arm in front.
fn extended(world: &World, pos: IVec3, entry: &BlockEntry, state: u32, sticky: bool) -> bool {
    let Some(dir) = direction(entry, state) else {
        return false;
    };
    power::at(world, pos + dir).is_some_and(|(arm, arm_state)| {
        kind(arm) == Some(Kind::PistonHead { sticky })
            && int(arm, arm_state, FACING) == int(entry, state, FACING)
    })
}

/// How the block at `pos` takes a push (see the module docs).
fn reaction(world: &World, pos: IVec3) -> Reaction {
    if !in_height(pos.y) {
        return Reaction::Block;
    }
    let Some((entry, state)) = power::at(world, pos) else {
        return Reaction::Block;
    };
    match kind(entry) {
        Some(Kind::Piston { sticky }) => {
            return if moving(world, pos) || extended(world, pos, entry, state, sticky) {
                Reaction::Block
            } else {
                Reaction::Move
            };
        }
        Some(Kind::PistonHead { .. } | Kind::MovingBlock) => return Reaction::Block,
        Some(
            Kind::Dust
            | Kind::Repeater { .. }
            | Kind::Comparator { .. }
            | Kind::Plate(_)
            | Kind::Torch { .. }
            | Kind::Lever
            | Kind::Button,
        ) => return Reaction::Break,
        Some(Kind::Lamp { .. } | Kind::RedstoneBlock | Kind::Observer) => return Reaction::Move,
        Some(Kind::NoteBlock) | None => {}
    }
    let id = entry.string_id.as_str();
    let material = entry.data.material;
    // Air and light blocks.
    if material == 0 {
        return Reaction::Empty;
    }
    if IMMOVABLE.contains(&id) || entry.hardness < 0.0 || has_actor(world, pos) {
        return Reaction::Block;
    }
    match material {
        // Water, lava, fire, plants.
        5 | 6 | 10 | 8 => return Reaction::Break,
        // Decorations (torches, candles, skulls, flower pots, snow layers,
        // ...), except carpets, rails and end rods, which move.
        24 if !(id.ends_with("_carpet") || id.ends_with("rail") || id == "minecraft:end_rod") => {
            return Reaction::Break;
        }
        _ => {}
    }
    match family(world, entry, state) {
        Family::Bed => return Reaction::Block,
        Family::Door { .. }
        | Family::Torch
        | Family::Button { .. }
        | Family::Lever
        | Family::StandingSign
        | Family::WallSign
        | Family::HangingSign
        | Family::Ladder
        | Family::Vine
        | Family::Crop
        | Family::Stem
        | Family::NetherWart
        | Family::SugarCane
        | Family::Cactus
        | Family::Plant
        | Family::DoublePlant
        | Family::Fire
        | Family::Liquid => return Reaction::Break,
        _ => {}
    }
    if matches!(id, "minecraft:web" | "minecraft:cocoa") {
        return Reaction::Break;
    }
    if id.ends_with("_glazed_terracotta") {
        return Reaction::PushOnly;
    }
    Reaction::Move
}

/// Slime and honey drag their neighbours along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sticky {
    Slime,
    Honey,
}

fn sticky(world: &World, pos: IVec3) -> Option<Sticky> {
    match super::super::name_at(world, pos)? {
        "minecraft:slime" => Some(Sticky::Slime),
        "minecraft:honey_block" => Some(Sticky::Honey),
        _ => None,
    }
}

/// The blocks one move takes: the line in front (or behind the arm when
/// pulling), what slime and honey drag along, and what breaks.
struct Structure<'w> {
    world: &'w World,
    piston: IVec3,
    /// The direction the blocks move.
    dir: IVec3,
    /// The piston's push direction.
    facing: IVec3,
    /// The arm's cell while pulling: free for the pulled blocks.
    arm: Option<IVec3>,
    moved: Vec<IVec3>,
    broken: Vec<IVec3>,
}

impl<'w> Structure<'w> {
    fn new(world: &'w World, piston: IVec3, dir: IVec3, facing: IVec3, arm: Option<IVec3>) -> Self {
        Self {
            world,
            piston,
            dir,
            facing,
            arm,
            moved: Vec::new(),
            broken: Vec::new(),
        }
    }

    fn reaction(&self, pos: IVec3) -> Reaction {
        if self.arm == Some(pos) {
            Reaction::Empty
        } else {
            reaction(self.world, pos)
        }
    }

    /// Whether the block at `pos` may move along this move when taken in
    /// direction `along` (push-only blocks only when that is the push).
    fn movable(&self, pos: IVec3, breaks: bool, along: IVec3) -> bool {
        match self.reaction(pos) {
            Reaction::Empty | Reaction::Move => true,
            Reaction::PushOnly => self.dir == along,
            Reaction::Break => breaks,
            Reaction::Block => false,
        }
    }

    fn sticks(&self, a: IVec3, b: IVec3) -> bool {
        match (sticky(self.world, a), sticky(self.world, b)) {
            (Some(x), Some(y)) => x == y,
            (x, y) => x.is_some() || y.is_some(),
        }
    }

    /// Collect the blocks from `start`; `false` when the move cannot
    /// happen.
    fn resolve(&mut self, start: IVec3, extending: bool) -> bool {
        match self.reaction(start) {
            Reaction::Empty => return true,
            Reaction::Break if extending => {
                self.broken.push(start);
                return true;
            }
            Reaction::Break | Reaction::Block => return false,
            Reaction::PushOnly if self.dir != self.facing => return false,
            Reaction::PushOnly | Reaction::Move => {}
        }
        if !self.add_line(start, self.dir) {
            return false;
        }
        let mut i = 0;
        while i < self.moved.len() {
            let pos = self.moved[i];
            if sticky(self.world, pos).is_some() && !self.add_branches(pos) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// Add the block at `origin` (reached in direction `along`), the
    /// sticky chain behind it and the line it pushes.
    fn add_line(&mut self, origin: IVec3, along: IVec3) -> bool {
        if self.reaction(origin) == Reaction::Empty
            || !self.movable(origin, false, along)
            || origin == self.piston
            || self.moved.contains(&origin)
        {
            return true;
        }
        let mut count = 1;
        if count + self.moved.len() > PUSH_LIMIT {
            return false;
        }
        // Blocks stuck behind `origin` come along.
        let mut current = origin;
        while sticky(self.world, current).is_some() {
            let behind = origin - self.dir * count as i32;
            if self.reaction(behind) == Reaction::Empty
                || !self.sticks(current, behind)
                || !self.movable(behind, false, -self.dir)
                || behind == self.piston
            {
                break;
            }
            current = behind;
            count += 1;
            if count + self.moved.len() > PUSH_LIMIT {
                return false;
            }
        }
        for back in (0..count).rev() {
            self.moved.push(origin - self.dir * back as i32);
        }
        let mut step = 1;
        loop {
            let ahead = origin + self.dir * step;
            if let Some(index) = self.moved.iter().position(|pos| *pos == ahead) {
                self.reorder(count, index);
                for i in 0..=index + count {
                    let pos = self.moved[i];
                    if sticky(self.world, pos).is_some() && !self.add_branches(pos) {
                        return false;
                    }
                }
                return true;
            }
            let reaction = self.reaction(ahead);
            if reaction == Reaction::Empty {
                return true;
            }
            if !self.movable(ahead, true, self.dir) || ahead == self.piston {
                return false;
            }
            if reaction == Reaction::Break {
                self.broken.push(ahead);
                return true;
            }
            if self.moved.len() >= PUSH_LIMIT {
                return false;
            }
            self.moved.push(ahead);
            count += 1;
            step += 1;
        }
    }

    /// A line ran into blocks already taken: keep the list ordered from
    /// the far end of each line.
    fn reorder(&mut self, count: usize, index: usize) {
        let tail = self.moved.split_off(self.moved.len() - count);
        let middle = self.moved.split_off(index);
        self.moved.extend(tail);
        self.moved.extend(middle);
    }

    /// The neighbours a sticky block at `from` drags along, off the move's
    /// axis.
    fn add_branches(&mut self, from: IVec3) -> bool {
        for side in NEIGHBOURS {
            if side.abs() == self.dir.abs() {
                continue;
            }
            let pos = from + side;
            if self.sticks(pos, from) && !self.add_line(pos, side) {
                return false;
            }
        }
        true
    }
}

/// Whether the piston at `pos`, pushing `dir`, receives power.
fn powered(world: &World, pos: IVec3, dir: IVec3) -> bool {
    NEIGHBOURS
        .iter()
        .any(|offset| *offset != dir && power::signal(world, pos + *offset, -*offset, true) > 0)
}

/// A neighbour update of the piston at `pos` from `from` (`from == pos`:
/// the piston was just set).
pub fn changed(world: &mut World, pos: IVec3, from: IVec3) {
    let Some((entry, state)) = power::at(world, pos) else {
        return;
    };
    let Some(Kind::Piston { sticky }) = kind(entry) else {
        return;
    };
    let Some(dir) = direction(entry, state) else {
        return;
    };
    if from == pos {
        // A piston placed (or moved) here starts fresh: a move left behind
        // by a piston that was here ends, and stale actor data goes.
        if moving(world, pos) {
            finish(world, pos);
        }
        drop_actor(world, pos, ARM_ACTOR);
    } else if from == pos + dir
        && !moving(world, pos)
        && !extended(world, pos, entry, state, sticky)
        && arm_state(world, pos) == Some(EXTENDED)
    {
        // The arm was broken off: the piston breaks too.
        drop_actor(world, pos, ARM_ACTOR);
        super::super::destroy_block(world, pos, true);
        return;
    }
    update(world, pos);
}

/// The piston at `pos` reads its power: extend or retract when that
/// disagrees with its arm.
pub fn update(world: &mut World, pos: IVec3) {
    if moving(world, pos) {
        return;
    }
    let Some((entry, state)) = power::at(world, pos) else {
        return;
    };
    let Some(Kind::Piston { sticky }) = kind(entry) else {
        return;
    };
    let Some(dir) = direction(entry, state) else {
        return;
    };
    let arm = extended(world, pos, entry, state, sticky);
    match (powered(world, pos, dir), arm) {
        (true, false) => extend(world, pos, sticky, dir),
        (false, true) => retract(world, pos, sticky, dir),
        _ => {}
    }
}

fn extend(world: &mut World, pos: IVec3, sticky: bool, dir: IVec3) {
    let mut structure = Structure::new(world, pos, dir, dir, None);
    if !structure.resolve(pos + dir, true) {
        return;
    }
    let (moved, broken) = (structure.moved, structure.broken);
    start(world, pos, sticky, dir, true, &moved, &broken);
}

fn retract(world: &mut World, pos: IVec3, sticky: bool, dir: IVec3) {
    let mut moved = Vec::new();
    let mut broken = Vec::new();
    if sticky {
        let mut structure = Structure::new(world, pos, -dir, dir, Some(pos + dir));
        if structure.resolve(pos + dir * 2, false) {
            moved = structure.moved;
            broken = structure.broken;
        }
    }
    start(world, pos, sticky, dir, false, &moved, &broken);
}

/// Start a move of the piston at `pos`: `moved` blocks go one block along
/// (`dir` extending, back when retracting), `broken` blocks break.
fn start(
    world: &mut World,
    pos: IVec3,
    sticky: bool,
    dir: IVec3,
    extending: bool,
    moved: &[IVec3],
    broken: &[IVec3],
) {
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let Some(moving_state) = registry.default_state_id_by_name(MOVING_BLOCK) else {
        return;
    };
    let Some(piston_state) = world.block_at(pos) else {
        return;
    };
    let air = air(&registry);
    let step = if extending { dir } else { -dir };
    let landing: Vec<(IVec3, u32)> = moved
        .iter()
        .filter_map(|origin| Some((*origin + step, world.block_at(*origin)?)))
        .collect();
    world.get_resource_or_init::<PistonMotions>().0.insert(
        pos,
        Motion {
            extending,
            ticks: 0,
            moved: landing.clone(),
        },
    );
    let (arm, progress) = if extending {
        (EXTENDING, 0.0)
    } else {
        (RETRACTING, 1.0)
    };
    store_actor(
        world,
        pos,
        arm_actor(pos, sticky, arm, progress, moved, broken),
    );
    let arm_state = extending
        .then(|| {
            let entry = registry.get_by_name(if sticky { STICKY_ARM } else { ARM })?;
            let facing = registry
                .get_by_runtime_id(piston_state)
                .and_then(|piston| int(piston, piston_state, FACING))?;
            Some(with(
                entry,
                entry.default_state_id,
                FACING,
                StateValue::Int(facing),
            ))
        })
        .flatten();
    deferred(world, |world| {
        for cell in broken.iter().rev() {
            break_block(world, *cell);
        }
        for (cell, block) in &landing {
            set_block(world, *cell, moving_state, false);
            if let Some(entry) = registry.get_by_runtime_id(*block) {
                store_actor(
                    world,
                    *cell,
                    moving_actor(*cell, pos, extending, entry, *block),
                );
            }
        }
        for origin in moved {
            let taken = landing.iter().any(|(cell, _)| cell == origin)
                || (arm_state.is_some() && *origin == pos + dir);
            if !taken {
                set_block(world, *origin, air, false);
            }
        }
        if let Some(arm_state) = arm_state {
            set_block(world, pos + dir, arm_state, false);
        }
    });
    sound(
        world,
        pos,
        if extending { SOUND_OUT } else { SOUND_IN },
        piston_state,
    );
    schedule_tick(world, pos, 1);
}

/// Break a block in a piston's way: liquids and fire go without drops,
/// everything else drops as if broken (a door or double plant as a whole).
fn break_block(world: &mut World, pos: IVec3) {
    let Some((entry, state)) = power::at(world, pos) else {
        return;
    };
    let air = air(world.registry());
    if matches!(entry.data.material, 5 | 6 | 10) {
        set_block(world, pos, air, false);
        return;
    }
    let drops = break_structure(world, pos, state);
    particles(world, pos, state, LEVEL_EVENT_PARTICLES_DESTROY_BLOCK);
    set_block(world, pos, air, false);
    crate::server::game::block_drops::drop_block_items(world, pos, drops, None);
}

/// A due tick of the piston at `pos`: a move advances (and ends after
/// [`MOVE_TICKS`]); without one the piston reads its power.
pub fn tick(world: &mut World, pos: IVec3) {
    let done = world
        .get_resource_mut::<PistonMotions>()
        .and_then(|mut motions| {
            let motion = motions.0.get_mut(&pos)?;
            motion.ticks += 1;
            Some(motion.ticks >= MOVE_TICKS)
        });
    match done {
        None => update(world, pos),
        Some(true) => finish(world, pos),
        Some(false) => schedule_tick(world, pos, 1),
    }
}

/// End the move of the piston at `pos`: the moved blocks land, a
/// retracting arm goes, and the piston reads its power again.
fn finish(world: &mut World, pos: IVec3) {
    let Some(motion) = world
        .get_resource_mut::<PistonMotions>()
        .and_then(|mut motions| motions.0.remove(&pos))
    else {
        return;
    };
    let registry = world.resource::<BlockRegistryResource>().0.clone();
    let piston = power::at(world, pos).and_then(|(entry, state)| match kind(entry) {
        Some(Kind::Piston { sticky }) => Some((sticky, direction(entry, state)?)),
        _ => None,
    });
    match piston {
        Some((sticky, _)) if motion.extending => {
            store_actor(world, pos, arm_actor(pos, sticky, EXTENDED, 1.0, &[], &[]));
        }
        Some((sticky, _)) => {
            send_actor(world, pos, arm_actor(pos, sticky, RETRACTED, 0.0, &[], &[]));
            drop_actor(world, pos, ARM_ACTOR);
        }
        None => drop_actor(world, pos, ARM_ACTOR),
    }
    let moving_state = registry.default_state_id_by_name(MOVING_BLOCK);
    let air = air(&registry);
    deferred(world, |world| {
        for (cell, block) in &motion.moved {
            drop_actor(world, *cell, MOVING_ACTOR);
            if moving_state.is_some() && world.block_at(*cell) == moving_state {
                set_block(world, *cell, *block, false);
            }
        }
        if let Some((sticky, dir)) = piston
            && !motion.extending
            && power::at(world, pos + dir)
                .is_some_and(|(arm, _)| kind(arm) == Some(Kind::PistonHead { sticky }))
        {
            set_block(world, pos + dir, air, false);
        }
    });
    for (cell, block) in &motion.moved {
        let observer = registry
            .get_by_runtime_id(*block)
            .is_some_and(|entry| kind(entry) == Some(Kind::Observer));
        if observer {
            observer::moved(world, *cell);
        }
    }
    if piston.is_some() {
        update(world, pos);
    }
}

/// A neighbour update of the arm at `pos`: an arm whose piston is gone
/// disappears without drops.
pub fn arm_changed(world: &mut World, pos: IVec3) {
    let Some((entry, state)) = power::at(world, pos) else {
        return;
    };
    let Some(Kind::PistonHead { sticky }) = kind(entry) else {
        return;
    };
    let Some(dir) = direction(entry, state) else {
        return;
    };
    let base = pos - dir;
    let facing = int(entry, state, FACING);
    let attached = power::at(world, base).is_some_and(|(piston, piston_state)| {
        kind(piston) == Some(Kind::Piston { sticky }) && int(piston, piston_state, FACING) == facing
    });
    if attached {
        return;
    }
    let air = air(world.registry());
    set_block(world, pos, air, false);
    if moving(world, base) {
        finish(world, base);
    } else {
        drop_actor(world, base, ARM_ACTOR);
    }
}

/// A neighbour update of the moving block at `pos`: it waits for its
/// piston, or lands at once when the piston is gone or its move is not
/// known any more.
pub fn moving_changed(world: &mut World, pos: IVec3) {
    let owner = world.get_resource::<PistonMotions>().and_then(|motions| {
        motions
            .0
            .iter()
            .find(|(_, motion)| motion.moved.iter().any(|(cell, _)| *cell == pos))
            .map(|(piston, _)| *piston)
    });
    match owner {
        Some(piston) => {
            let present = power::at(world, piston)
                .is_some_and(|(entry, _)| matches!(kind(entry), Some(Kind::Piston { .. })));
            if !present {
                finish(world, piston);
            }
        }
        None => {
            let registry = world.resource::<BlockRegistryResource>().0.clone();
            let landed = actor(world, pos)
                .filter(|compound| actor_id(compound) == Some(MOVING_ACTOR))
                .and_then(|compound| match compound.0.get("movingBlock") {
                    Some(NBTTag::Compound(block)) => block_from_nbt(&registry, block),
                    _ => None,
                })
                .unwrap_or_else(|| air(&registry));
            drop_actor(world, pos, MOVING_ACTOR);
            set_block(world, pos, landed, false);
        }
    }
}

// ── Block actors ─────────────────────────────────────────────────────────

fn int_tag(value: i32) -> NBTTag {
    NBTTag::Int(tag::Int(value))
}

fn byte_tag(value: u8) -> NBTTag {
    NBTTag::Byte(tag::Byte(value))
}

fn string_tag(value: &str) -> NBTTag {
    NBTTag::String(tag::String(value.to_owned()))
}

fn cells_tag(cells: &[IVec3]) -> NBTTag {
    NBTTag::List(tag::List(
        cells
            .iter()
            .flat_map(|cell| [cell.x, cell.y, cell.z])
            .map(int_tag)
            .collect(),
    ))
}

fn actor_header(id: &str, pos: IVec3) -> HashMap<String, NBTTag> {
    HashMap::from([
        ("id".to_owned(), string_tag(id)),
        ("x".to_owned(), int_tag(pos.x)),
        ("y".to_owned(), int_tag(pos.y)),
        ("z".to_owned(), int_tag(pos.z)),
    ])
}

/// The `PistonArm` actor of the piston at `pos` in arm state `arm`.
fn arm_actor(
    pos: IVec3,
    sticky: bool,
    arm: u8,
    progress: f32,
    attached: &[IVec3],
    broken: &[IVec3],
) -> tag::Compound {
    let mut out = actor_header(ARM_ACTOR, pos);
    let float = |value: f32| NBTTag::Float(tag::Float(value));
    out.insert("isMovable".into(), byte_tag(u8::from(arm == RETRACTED)));
    out.insert("Progress".into(), float(progress));
    out.insert("LastProgress".into(), float(progress));
    out.insert("State".into(), byte_tag(arm));
    out.insert("NewState".into(), byte_tag(arm));
    out.insert("Sticky".into(), byte_tag(u8::from(sticky)));
    out.insert("AttachedBlocks".into(), cells_tag(attached));
    out.insert("BreakBlocks".into(), cells_tag(broken));
    tag::Compound(out)
}

/// `{name, states, version}` of the block state `state`.
fn block_nbt(entry: &BlockEntry, state: u32) -> NBTTag {
    let states = entry
        .data
        .properties
        .iter()
        .filter_map(|property| {
            let value = match entry.state_value(state, property.name)? {
                StateValue::Byte(value) => byte_tag(value),
                StateValue::Int(value) => int_tag(value),
                StateValue::Str(value) => string_tag(value),
            };
            Some((property.name.to_owned(), value))
        })
        .collect();
    NBTTag::Compound(tag::Compound(HashMap::from([
        ("name".to_owned(), string_tag(&entry.string_id)),
        ("states".to_owned(), NBTTag::Compound(tag::Compound(states))),
        ("version".to_owned(), int_tag(BLOCK_VERSION)),
    ])))
}

/// The block state a `{name, states}` compound names.
fn block_from_nbt(registry: &BlockRegistry, block: &tag::Compound) -> Option<u32> {
    let NBTTag::String(name) = block.0.get("name")? else {
        return None;
    };
    let entry = registry.get_by_name(&name.0)?;
    let mut state = entry.default_state_id;
    if let Some(NBTTag::Compound(states)) = block.0.get("states") {
        for property in entry.data.properties {
            let value = match states.0.get(property.name) {
                Some(NBTTag::Byte(value)) => StateValue::Byte(value.0),
                Some(NBTTag::Int(value)) => StateValue::Int(value.0),
                Some(NBTTag::String(value)) => *property
                    .values
                    .iter()
                    .find(|known| matches!(known, StateValue::Str(s) if *s == value.0))?,
                _ => continue,
            };
            state = with(entry, state, property.name, value);
        }
    }
    Some(state)
}

/// The `MovingBlock` actor of a block in state `state` moving into `pos`
/// for the piston at `piston`.
fn moving_actor(
    pos: IVec3,
    piston: IVec3,
    extending: bool,
    entry: &BlockEntry,
    state: u32,
) -> tag::Compound {
    let mut out = actor_header(MOVING_ACTOR, pos);
    out.insert("isMovable".into(), byte_tag(1));
    out.insert("expanding".into(), byte_tag(u8::from(extending)));
    out.insert("pistonPosX".into(), int_tag(piston.x));
    out.insert("pistonPosY".into(), int_tag(piston.y));
    out.insert("pistonPosZ".into(), int_tag(piston.z));
    out.insert("movingBlock".into(), block_nbt(entry, state));
    out.insert(
        "movingBlockExtra".into(),
        NBTTag::Compound(tag::Compound(HashMap::from([
            ("name".to_owned(), string_tag("minecraft:air")),
            (
                "states".to_owned(),
                NBTTag::Compound(tag::Compound(HashMap::new())),
            ),
            ("version".to_owned(), int_tag(BLOCK_VERSION)),
        ]))),
    );
    tag::Compound(out)
}

fn chunk(world: &World, pos: IVec3) -> Option<Entity> {
    let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
    world.get_resource::<ChunkManager>()?.get_by_coords(cx, cz)
}

fn has_actor(world: &World, pos: IVec3) -> bool {
    chunk(world, pos)
        .and_then(|chunk| world.get::<ChunkData>(chunk))
        .is_some_and(|data| {
            data.inner
                .block_entities()
                .iter()
                .any(|actor| actor.position() == pos.to_array())
        })
}

/// The actor saved at `pos`.
fn actor(world: &World, pos: IVec3) -> Option<tag::Compound> {
    let data = world.get::<ChunkData>(chunk(world, pos)?)?;
    data.inner
        .block_entities()
        .iter()
        .find(|actor| actor.position() == pos.to_array())?
        .bedrock_compound()?
        .ok()
}

fn actor_id(compound: &tag::Compound) -> Option<&str> {
    match compound.0.get("id")? {
        NBTTag::String(id) => Some(id.0.as_str()),
        _ => None,
    }
}

/// `State` of the `PistonArm` actor saved at `pos`.
fn arm_state(world: &World, pos: IVec3) -> Option<u8> {
    let compound = actor(world, pos)?;
    if actor_id(&compound) != Some(ARM_ACTOR) {
        return None;
    }
    match compound.0.get("State")? {
        NBTTag::Byte(state) => Some(state.0),
        _ => None,
    }
}

/// Send `compound` to the viewers of `pos` as `BlockActorData`.
fn send_actor(world: &World, pos: IVec3, compound: tag::Compound) -> Option<BlockEntity> {
    let actor = match BlockEntity::from_bedrock_compound(compound) {
        Ok(actor) => actor,
        Err(error) => {
            tracing::warn!(?pos, %error, "piston actor failed to encode");
            return None;
        }
    };
    let packet = McpePacket::from(BlockActorDataPacket {
        block_position: BlockPos {
            x: pos.x,
            y: pos.y,
            z: pos.z,
        },
        actor_data_tags: actor.network_nbt().clone(),
    });
    crate::entity::mob::net::sound::send_to_viewers(world, centre(pos), &packet);
    Some(actor)
}

/// Send `compound` and keep it as the chunk's actor at `pos`.
fn store_actor(world: &mut World, pos: IVec3, compound: tag::Compound) {
    let Some(actor) = send_actor(world, pos, compound) else {
        return;
    };
    let Some(chunk) = chunk(world, pos) else {
        return;
    };
    if let Some(mut data) = world.get_mut::<ChunkData>(chunk) {
        data.inner.set_block_entity(actor);
    }
    if let Some(mut flags) = world.get_mut::<ChunkStateFlags>(chunk) {
        flags.mark_dirty();
    }
}

/// Remove the chunk's actor at `pos` if it is an `id` actor.
fn drop_actor(world: &mut World, pos: IVec3, id: &str) {
    if actor(world, pos).as_ref().and_then(actor_id) != Some(id) {
        return;
    }
    let Some(chunk) = chunk(world, pos) else {
        return;
    };
    if let Some(mut data) = world.get_mut::<ChunkData>(chunk) {
        data.inner.remove_block_entity(pos.x, pos.y, pos.z);
    }
    if let Some(mut flags) = world.get_mut::<ChunkStateFlags>(chunk) {
        flags.mark_dirty();
    }
}

#[cfg(test)]
mod tests {
    use glam::{DVec3, IVec3};
    use jolyne::valentine::McpePacketData;

    use super::super::super::tests::{FLOOR, Harness, state};
    use super::{ARM, FACING, MOVING_BLOCK, SOUND_IN, SOUND_OUT, STICKY_ARM};
    use crate::registry::block::StateValue;

    const Y: i32 = FLOOR + 1;
    const STONE: &str = "minecraft:stone";
    const AIR: &str = "minecraft:air";

    fn at(x: i32, z: i32) -> IVec3 {
        IVec3::new(x, Y, z)
    }

    fn harness() -> Harness {
        let mut h = Harness::new();
        h.stand(DVec3::new(-10.5, Y as f64 + 1.62, -10.5), 0.0, 0.0);
        h
    }

    /// A piston at `pos` pushing east (`facing_direction` 4).
    fn piston(h: &mut Harness, pos: IVec3, sticky: bool) {
        let name = if sticky {
            "minecraft:sticky_piston"
        } else {
            "minecraft:piston"
        };
        h.put(pos, state(name, &[(FACING, StateValue::Int(4))]));
    }

    fn stones(h: &mut Harness, z: i32, xs: std::ops::RangeInclusive<i32>) {
        for x in xs {
            h.put(at(x, z), state(STONE, &[]));
        }
    }

    /// Power the piston at `pos` from behind (west).
    fn power(h: &mut Harness, pos: IVec3) {
        h.set(pos - IVec3::X, state("minecraft:redstone_block", &[]));
    }

    fn unpower(h: &mut Harness, pos: IVec3) {
        h.set(pos - IVec3::X, state(AIR, &[]));
    }

    fn sounds(h: &mut Harness) -> Vec<String> {
        h.packets()
            .into_iter()
            .filter_map(|packet| match packet {
                McpePacketData::LevelSoundEventPacket(sound) => Some(sound.sound_event.to_string()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn extending_pushes_a_line_of_three_and_places_the_arm_after_two_ticks() {
        let mut h = harness();
        piston(&mut h, at(0, 0), false);
        stones(&mut h, 0, 1..=3);
        h.packets();
        power(&mut h, at(0, 0));
        assert!(sounds(&mut h).contains(&SOUND_OUT.to_string()));
        // The move starts at once: the arm takes the cell in front, the
        // stones travel as moving blocks.
        assert_eq!(h.name(at(1, 0)), ARM);
        assert_eq!(h.value(at(1, 0), FACING), Some(StateValue::Int(4)));
        for x in 2..=4 {
            assert_eq!(h.name(at(x, 0)), MOVING_BLOCK, "x {x}");
        }
        h.tick(1);
        assert_eq!(h.name(at(4, 0)), MOVING_BLOCK);
        h.tick(1);
        for x in 2..=4 {
            assert_eq!(h.name(at(x, 0)), STONE, "x {x}");
        }
        assert_eq!(h.name(at(1, 0)), ARM);
        assert_eq!(h.name(at(5, 0)), AIR);
        assert_eq!(h.name(at(0, 0)), "minecraft:piston");
        h.tick(10);
        assert_eq!(h.name(at(1, 0)), ARM, "stays out while powered");
    }

    #[test]
    fn the_arm_actor_is_sent_when_the_move_starts_and_ends() {
        let mut h = harness();
        piston(&mut h, at(0, 0), false);
        h.packets();
        power(&mut h, at(0, 0));
        h.tick(2);
        let arm_updates = h
            .packets()
            .into_iter()
            .filter(|packet| {
                matches!(packet, McpePacketData::BlockActorDataPacket(data)
                    if data.block_position.x == 0 && data.block_position.y == Y)
            })
            .count();
        assert_eq!(arm_updates, 2);
        assert_eq!(super::arm_state(&h.world, at(0, 0)), Some(super::EXTENDED));
    }

    #[test]
    fn twelve_blocks_move_and_thirteen_refuse() {
        let mut h = harness();
        piston(&mut h, at(0, 0), false);
        stones(&mut h, 0, 1..=12);
        power(&mut h, at(0, 0));
        h.tick(2);
        assert_eq!(h.name(at(1, 0)), ARM);
        assert_eq!(h.name(at(13, 0)), STONE);

        piston(&mut h, at(0, 2), false);
        stones(&mut h, 2, 1..=13);
        h.packets();
        power(&mut h, at(0, 2));
        assert!(!sounds(&mut h).contains(&SOUND_OUT.to_string()));
        h.tick(2);
        assert_eq!(h.name(at(1, 2)), STONE);
        assert_eq!(h.name(at(14, 2)), AIR);
        for x in 1..=13 {
            assert_eq!(h.name(at(x, 2)), STONE, "x {x}");
        }
    }

    #[test]
    fn a_sticky_piston_pulls_its_block_back() {
        let mut h = harness();
        piston(&mut h, at(0, 0), true);
        stones(&mut h, 0, 1..=1);
        power(&mut h, at(0, 0));
        h.tick(2);
        assert_eq!(h.name(at(1, 0)), STICKY_ARM);
        assert_eq!(h.name(at(2, 0)), STONE);
        h.packets();
        unpower(&mut h, at(0, 0));
        assert!(sounds(&mut h).contains(&SOUND_IN.to_string()));
        assert_eq!(h.name(at(1, 0)), MOVING_BLOCK);
        assert_eq!(h.name(at(2, 0)), AIR);
        h.tick(2);
        assert_eq!(h.name(at(1, 0)), STONE);
        assert_eq!(h.name(at(2, 0)), AIR);
        assert_eq!(super::arm_state(&h.world, at(0, 0)), None);
    }

    #[test]
    fn a_plain_piston_leaves_its_block_when_retracting() {
        let mut h = harness();
        piston(&mut h, at(0, 0), false);
        stones(&mut h, 0, 1..=1);
        power(&mut h, at(0, 0));
        h.tick(2);
        unpower(&mut h, at(0, 0));
        h.tick(2);
        assert_eq!(h.name(at(1, 0)), AIR);
        assert_eq!(h.name(at(2, 0)), STONE);
        assert_eq!(h.name(at(0, 0)), "minecraft:piston");
    }

    #[test]
    fn plants_and_torches_in_the_way_break_with_drops() {
        let mut h = harness();
        piston(&mut h, at(0, 0), false);
        stones(&mut h, 0, 1..=1);
        h.put(IVec3::new(2, FLOOR, 0), state("minecraft:dirt", &[]));
        h.put(at(2, 0), state("minecraft:poppy", &[]));
        power(&mut h, at(0, 0));
        // The poppy goes as the move starts; the stone lands in its cell.
        assert_eq!(h.drops(), vec![("minecraft:poppy".to_string(), 1)]);
        h.tick(2);
        assert_eq!(h.name(at(2, 0)), STONE);

        piston(&mut h, at(0, 2), false);
        h.put(
            at(1, 2),
            state(
                "minecraft:torch",
                &[("torch_facing_direction", StateValue::Str("top"))],
            ),
        );
        power(&mut h, at(0, 2));
        h.tick(2);
        assert_eq!(h.name(at(1, 2)), ARM);
        assert!(h.drops().contains(&("minecraft:torch".to_string(), 1)));
    }

    #[test]
    fn obsidian_refuses_to_move() {
        let mut h = harness();
        piston(&mut h, at(0, 0), false);
        stones(&mut h, 0, 1..=1);
        h.put(at(2, 0), state("minecraft:obsidian", &[]));
        power(&mut h, at(0, 0));
        h.tick(2);
        assert_eq!(h.name(at(1, 0)), STONE);
        assert_eq!(h.name(at(2, 0)), "minecraft:obsidian");

        piston(&mut h, at(0, 2), false);
        h.put(at(1, 2), state("minecraft:obsidian", &[]));
        power(&mut h, at(0, 2));
        h.tick(2);
        assert_eq!(h.name(at(1, 2)), "minecraft:obsidian");
    }

    #[test]
    fn power_from_the_front_or_above_the_piston_does_nothing() {
        let mut h = harness();
        piston(&mut h, at(0, 0), false);
        // In front of the piston.
        h.set(at(1, 0), state("minecraft:redstone_block", &[]));
        h.tick(2);
        assert_eq!(h.name(at(1, 0)), "minecraft:redstone_block");
        // Next to the cell above the piston (no quasi-connectivity).
        h.set(at(-1, 0) + IVec3::Y, state("minecraft:redstone_block", &[]));
        h.tick(2);
        assert_eq!(h.name(at(1, 0)), "minecraft:redstone_block");
    }

    #[test]
    fn breaking_the_piston_removes_its_arm() {
        let mut h = harness();
        piston(&mut h, at(0, 0), false);
        power(&mut h, at(0, 0));
        h.tick(2);
        assert_eq!(h.name(at(1, 0)), ARM);
        h.set(at(0, 0), state(AIR, &[]));
        assert_eq!(h.name(at(1, 0)), AIR);
        assert_eq!(super::arm_state(&h.world, at(0, 0)), None);
    }

    #[test]
    fn placement_pushes_towards_the_player() {
        let mut h = Harness::new();
        h.hold("minecraft:piston", 1);
        // Looking north (yaw 180): the piston pushes south, back at the
        // player, which the wire names 2.
        h.stand(DVec3::new(0.5, Y as f64 + 1.62, 0.5), 180.0, 10.0);
        let placed = h.click(IVec3::new(0, FLOOR, -2), 1, DVec3::new(0.5, 1.0, 0.5));
        assert_eq!(placed.len(), 1, "{placed:?}");
        assert_eq!(h.value(placed[0].0, FACING), Some(StateValue::Int(2)));
    }

    #[test]
    fn an_observer_watching_a_pushed_block_pulses() {
        let mut h = harness();
        piston(&mut h, at(0, 0), false);
        stones(&mut h, 0, 1..=1);
        // Watching the cell the stone is pushed into, from the south.
        h.put(
            at(2, 1),
            state(
                "minecraft:observer",
                &[(super::super::observer::FACING, StateValue::Str("north"))],
            ),
        );
        power(&mut h, at(0, 0));
        h.tick(2);
        assert_eq!(
            h.value(at(2, 1), super::super::observer::POWERED),
            Some(StateValue::Byte(1))
        );
    }
}
