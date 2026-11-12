//! Pistons and sticky pistons, timed like BDS 1.26.51.
//!
//! Measured with the piston sweeps in `crates/unastar-data/data/measured/pistons/`
//! (`tools/bds-sweep/pistons.py`); `pistons/tests.rs` replays them tick by
//! tick. What BDS does, as seen from outside:
//!
//! - A piston is a block power acts on: it reads its power in the update
//!   (from every side but its face; no power through the block above it) and
//!   moves a tick later. It only reads its power while it is not moving: a
//!   move takes two ticks, so a short pulse still runs the whole extension,
//!   and the earliest retraction comes two updates after the extension.
//! - Extending: the head (`piston_arm_collision`) appears in front, every
//!   pushed block becomes a `moving_block` at its destination (its old place
//!   air), blocks that break are removed with their drops; two ticks later
//!   the moving blocks land. Retracting: the head goes; a sticky piston pulls
//!   the block in front of the head (and what sticks to it) the same way.
//! - At most 12 blocks move; slime and honey drag the blocks beside them
//!   (but not each other, and nothing that only pushes); a block that holds
//!   (obsidian, an extended piston, a moving block, ...) stops the push.
//! - Which blocks move, break or hold is measured per block
//!   (`unastar_data::pistons`).

use std::collections::{BTreeMap, HashMap};

use glam::{IVec3, Vec3};
use unastar_data::pistons::Movement;
use zuri_nbt::{NBTTag, tag};

use super::Circuit;
use super::kinds::{Consumer, FACES, Facts, Glue, Kind, Table, piston_facing_value};
use crate::registry::BlockRegistry;
use crate::registry::block::StateValue;
use crate::server::game::block_actors;
use crate::server::game::block_update::support::Face;

/// The most blocks one piston moves.
pub(crate) const PUSH_LIMIT: usize = 12;
/// Ticks from the start of a move to the moved blocks landing.
pub(crate) const MOVE_TICKS: u64 = 2;

/// The piston actor's `State`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArmState {
    Retracted = 0,
    Extending = 1,
    Extended = 2,
    Retracting = 3,
}

#[derive(Debug, Clone, Copy)]
struct Arm {
    /// Extended or extending.
    extended: bool,
    /// The tick its last move ends; it reads its power again after it.
    busy_until: u64,
}

/// A block in motion, landing at its destination.
#[derive(Debug, Clone)]
struct Moving {
    state: u32,
    actor: Option<tag::Compound>,
    piston: IVec3,
    /// The order the move took it in (BDS lands blocks in that order).
    order: usize,
}

/// The pistons of a circuit between ticks.
#[derive(Debug, Default)]
pub(crate) struct Pistons {
    arms: HashMap<IVec3, Arm>,
    /// Moves decided at an update, made a tick later: (piston, extend).
    due: BTreeMap<u64, Vec<(IVec3, bool)>>,
    /// Moves ending: (piston, extended).
    ends: BTreeMap<u64, Vec<(IVec3, bool)>>,
    /// Blocks in motion by destination.
    moving: HashMap<IVec3, Moving>,
}

/// The blocks one move takes: `moved` in the order BDS moves them, and
/// `broken` (removed as the move starts).
#[derive(Debug, Default)]
pub(crate) struct Structure {
    pub moved: Vec<IVec3>,
    pub broken: Vec<IVec3>,
}

/// Finds what a piston at `piston` takes along when it moves blocks from
/// `start` in direction `dir` (`facing` is the piston's own facing;
/// `ignore` reads as air: the head a retracting piston removes first).
struct Resolver<'a, W: Circuit + ?Sized> {
    world: &'a W,
    table: &'a Table,
    air: u32,
    piston: IVec3,
    dir: IVec3,
    facing: IVec3,
    ignore: Option<IVec3>,
    moved: Vec<IVec3>,
    broken: Vec<IVec3>,
}

impl<W: Circuit + ?Sized> Resolver<'_, W> {
    fn state(&self, pos: IVec3) -> Option<u32> {
        if Some(pos) == self.ignore {
            return Some(self.air);
        }
        self.world.block_at(pos)
    }

    fn facts(&self, pos: IVec3) -> Option<Facts> {
        self.state(pos).map(|state| self.table.get(state))
    }

    fn air(&self, pos: IVec3) -> bool {
        self.state(pos) == Some(self.air)
    }

    /// How the block at `pos` moves: its measured class, except that an
    /// extended piston holds.
    fn movement(&self, pos: IVec3) -> Movement {
        let Some(facts) = self.facts(pos) else {
            return Movement::Immovable;
        };
        if let Kind::Consumer(Consumer::Piston { facing, .. }) = facts.kind
            && self
                .facts(pos + facing.offset())
                .is_some_and(|front| front.head)
        {
            return Movement::Immovable;
        }
        facts.movement
    }

    /// Whether the block at `pos` can move in this move, reached along
    /// `along` (the line direction, or a slime block's side); breaking
    /// blocks count only where `destroy`.
    fn pushable(&self, pos: IVec3, along: IVec3, destroy: bool) -> bool {
        match self.movement(pos) {
            Movement::Immovable => false,
            Movement::Breaks => destroy,
            // Moves only away from the piston, reached along the line.
            Movement::PushOnly => self.dir == self.facing && along == self.dir,
            Movement::PushPull => true,
        }
    }

    fn glue(&self, pos: IVec3) -> Option<Glue> {
        self.facts(pos).and_then(|facts| facts.glue)
    }

    fn resolve(mut self, start: IVec3) -> Option<Structure> {
        if self.air(start) {
            return Some(Structure::default());
        }
        if !self.pushable(start, self.dir, false) {
            if self.dir == self.facing && self.movement(start) == Movement::Breaks {
                self.broken.push(start);
                return Some(self.finish());
            }
            return None;
        }
        if !self.line(start, self.dir) {
            return None;
        }
        let mut i = 0;
        while i < self.moved.len() {
            let pos = self.moved[i];
            if self.glue(pos).is_some() && !self.branches(pos) {
                return None;
            }
            i += 1;
        }
        Some(self.finish())
    }

    fn finish(self) -> Structure {
        let dir = self.dir;
        let mut moved: Vec<(usize, IVec3)> = self.moved.into_iter().enumerate().collect();
        // Farthest along the move first, then in the order found.
        moved.sort_by_key(|(index, pos)| (-pos.dot(dir), *index));
        Structure {
            moved: moved.into_iter().map(|(_, pos)| pos).collect(),
            broken: self.broken,
        }
    }

    /// The glued blocks behind `origin` (away from the move) that come
    /// along with it, counting `origin`; `None` past the push limit.
    fn stuck_behind(&self, origin: IVec3) -> Option<usize> {
        let mut count = 1;
        let mut glue = self.glue(origin);
        while glue.is_some() {
            let back = origin - self.dir * count as i32;
            let next = self.glue(back);
            if self.air(back)
                || !Glue::sticks_to(glue, next)
                || !self.pushable(back, -self.dir, false)
                || back == self.piston
            {
                break;
            }
            count += 1;
            if count + self.moved.len() > PUSH_LIMIT {
                return None;
            }
            glue = next;
        }
        Some(count)
    }

    /// Add the line through `origin`: the blocks stuck behind it, it, and
    /// what it pushes in front.
    fn line(&mut self, origin: IVec3, along: IVec3) -> bool {
        if self.air(origin)
            || !self.pushable(origin, along, false)
            || origin == self.piston
            || self.moved.contains(&origin)
        {
            return true;
        }
        if 1 + self.moved.len() > PUSH_LIMIT {
            return false;
        }
        let Some(count) = self.stuck_behind(origin) else {
            return false;
        };
        for i in (0..count).rev() {
            self.moved.push(origin - self.dir * i as i32);
        }
        let mut added = count;
        let mut j = 1;
        loop {
            let front = origin + self.dir * j;
            if let Some(index) = self.moved.iter().position(|pos| *pos == front) {
                self.reorder(added, index);
                return (0..=index + added).all(|k| {
                    k >= self.moved.len()
                        || self.glue(self.moved[k]).is_none()
                        || self.branches(self.moved[k])
                });
            }
            if self.air(front) {
                return true;
            }
            if !self.pushable(front, self.dir, true) || front == self.piston {
                return false;
            }
            if self.movement(front) == Movement::Breaks {
                self.broken.push(front);
                return true;
            }
            if self.moved.len() >= PUSH_LIMIT {
                return false;
            }
            self.moved.push(front);
            added += 1;
            j += 1;
        }
    }

    /// Move the `added` blocks just found in front of the line they ran
    /// into at `index`, so the list stays in push order.
    fn reorder(&mut self, added: usize, index: usize) {
        let len = self.moved.len();
        let found = self.moved.split_off(len - added);
        let rest = self.moved.split_off(index);
        self.moved.extend(found);
        self.moved.extend(rest);
    }

    /// The blocks stuck to the sides of the slime or honey block at `pos`.
    fn branches(&mut self, pos: IVec3) -> bool {
        let glue = self.glue(pos);
        FACES.iter().all(|face| {
            let offset = face.offset();
            if offset == self.dir || offset == -self.dir {
                return true;
            }
            let neighbour = pos + offset;
            !Glue::sticks_to(self.glue(neighbour), glue) || self.line(neighbour, offset)
        })
    }
}

/// What a piston at `piston` facing `facing` moves when it extends (`true`)
/// or a sticky one pulls when it retracts.
pub(crate) fn structure(
    world: &(impl Circuit + ?Sized),
    table: &Table,
    air: u32,
    piston: IVec3,
    facing: Face,
    extend: bool,
) -> Option<Structure> {
    let f = facing.offset();
    let resolver = Resolver {
        world,
        table,
        air,
        piston,
        dir: if extend { f } else { -f },
        facing: f,
        ignore: (!extend).then_some(piston + f),
        moved: Vec::new(),
        broken: Vec::new(),
    };
    resolver.resolve(if extend { piston + f } else { piston + f * 2 })
}

fn state_with(
    registry: &BlockRegistry,
    name: &str,
    property: &str,
    value: StateValue,
) -> Option<u32> {
    let entry = registry.get_by_name(name)?;
    entry.with_state_value(entry.default_state_id, property, value)
}

/// The default state of `name`, a block every registry the circuit runs on
/// has (air, the moving block).
pub(crate) fn default_state(registry: &BlockRegistry, name: &str) -> u32 {
    registry
        .get_by_name(name)
        .map(|entry| entry.default_state_id)
        .unwrap_or_else(|| panic!("the block registry has no {name}"))
}

fn ints(positions: &[IVec3]) -> NBTTag {
    NBTTag::List(tag::List(
        positions
            .iter()
            .flat_map(|pos| [pos.x, pos.y, pos.z])
            .map(block_actors::int)
            .collect(),
    ))
}

fn float(value: f32) -> NBTTag {
    NBTTag::Float(tag::Float(value))
}

/// The piston's `PistonArm` actor as BDS sends it.
fn arm_actor(
    pos: IVec3,
    sticky: bool,
    state: ArmState,
    progress: f32,
    attached: &[IVec3],
    broken: &[IVec3],
) -> tag::Compound {
    let mut compound = block_actors::header("PistonArm", pos);
    compound.insert("AttachedBlocks".into(), ints(attached));
    compound.insert("BlockEntityVersion".into(), block_actors::int(0));
    compound.insert("BreakBlocks".into(), ints(broken));
    compound.insert("LastProgress".into(), float(progress));
    compound.insert("NewState".into(), block_actors::byte(state as u8));
    compound.insert("Progress".into(), float(progress));
    compound.insert("State".into(), block_actors::byte(state as u8));
    compound.insert("Sticky".into(), block_actors::byte(u8::from(sticky)));
    compound.insert(
        "isMovable".into(),
        block_actors::byte(u8::from(state == ArmState::Retracted)),
    );
    tag::Compound(compound)
}

/// The `MovingBlock` actor of a block in motion.
fn moving_actor(
    registry: &BlockRegistry,
    pos: IVec3,
    piston: IVec3,
    state: u32,
    expanding: bool,
    actor: Option<&tag::Compound>,
) -> tag::Compound {
    let mut compound = block_actors::header("MovingBlock", pos);
    compound.insert("BlockEntityVersion".into(), block_actors::int(0));
    compound.insert("expanding".into(), block_actors::byte(u8::from(expanding)));
    if let Some(entry) = registry.get_by_runtime_id(state) {
        compound.insert("movingBlock".into(), block_actors::block_tag(entry, state));
    }
    if let Some(air) = registry.get_by_name("minecraft:air") {
        compound.insert(
            "movingBlockExtra".into(),
            block_actors::block_tag(air, air.default_state_id),
        );
    }
    if let Some(actor) = actor {
        compound.insert("movingEntity".into(), NBTTag::Compound(actor.clone()));
    }
    compound.insert("pistonPosX".into(), block_actors::int(piston.x));
    compound.insert("pistonPosY".into(), block_actors::int(piston.y));
    compound.insert("pistonPosZ".into(), block_actors::int(piston.z));
    tag::Compound(compound)
}

/// `actor` moved to `pos`.
fn moved_actor(mut actor: tag::Compound, pos: IVec3) -> tag::Compound {
    actor.insert("x".into(), block_actors::int(pos.x));
    actor.insert("y".into(), block_actors::int(pos.y));
    actor.insert("z".into(), block_actors::int(pos.z));
    actor
}

fn corner(pos: IVec3) -> Vec3 {
    pos.as_vec3()
}

impl Pistons {
    fn arm(
        &mut self,
        world: &(impl Circuit + ?Sized),
        table: &Table,
        pos: IVec3,
        facing: Face,
    ) -> Arm {
        *self.arms.entry(pos).or_insert_with(|| {
            let extended = world
                .block_at(pos + facing.offset())
                .is_some_and(|front| table.get(front).head);
            Arm {
                extended,
                busy_until: 0,
            }
        })
    }

    /// The update read the piston at `pos` as `powered`: a piston that is
    /// not moving starts a move a tick later when its power and its arm
    /// disagree.
    pub(crate) fn powered(
        &mut self,
        world: &(impl Circuit + ?Sized),
        table: &Table,
        pos: IVec3,
        facing: Face,
        powered: bool,
        now: u64,
    ) {
        let arm = self.arm(world, table, pos, facing);
        if now <= arm.busy_until || arm.extended == powered {
            return;
        }
        if self
            .due
            .get(&(now + 1))
            .is_some_and(|due| due.iter().any(|(at, _)| *at == pos))
        {
            return;
        }
        self.due.entry(now + 1).or_default().push((pos, powered));
    }

    /// Forget a piston that is no longer there.
    pub(crate) fn removed(&mut self, pos: IVec3) {
        self.arms.remove(&pos);
    }

    /// Start the moves due at `now`; returns the positions written.
    pub(crate) fn start_due(
        &mut self,
        registry: &BlockRegistry,
        world: &mut (impl Circuit + ?Sized),
        table: &Table,
        now: u64,
    ) -> Vec<IVec3> {
        let mut written = Vec::new();
        for (pos, extend) in self.due.remove(&now).unwrap_or_default() {
            let Some(state) = world.block_at(pos) else {
                continue;
            };
            let Kind::Consumer(Consumer::Piston { facing, sticky }) = table.get(state).kind else {
                self.arms.remove(&pos);
                continue;
            };
            if extend {
                self.extend(
                    registry,
                    world,
                    table,
                    pos,
                    facing,
                    sticky,
                    now,
                    &mut written,
                );
            } else {
                self.retract(
                    registry,
                    world,
                    table,
                    pos,
                    facing,
                    sticky,
                    now,
                    &mut written,
                );
            }
        }
        written
    }

    #[allow(clippy::too_many_arguments)]
    fn extend(
        &mut self,
        registry: &BlockRegistry,
        world: &mut (impl Circuit + ?Sized),
        table: &Table,
        pos: IVec3,
        facing: Face,
        sticky: bool,
        now: u64,
        written: &mut Vec<IVec3>,
    ) {
        let air = default_state(registry, "minecraft:air");
        let Some(structure) = structure(world, table, air, pos, facing, true) else {
            return;
        };
        let dir = facing.offset();
        // BDS's order: the piston's actor, the blocks, the moving blocks'
        // actors, the sound.
        world.put_actor(
            pos,
            arm_actor(
                pos,
                sticky,
                ArmState::Extending,
                0.0,
                &structure.moved,
                &structure.broken,
            ),
        );
        for broken in &structure.broken {
            world.break_block(*broken);
            written.push(*broken);
        }
        let actors = self.shift(registry, world, pos, &structure.moved, dir, true, written);
        let arm_name = if sticky {
            "minecraft:sticky_piston_arm_collision"
        } else {
            "minecraft:piston_arm_collision"
        };
        if let Some(arm) = state_with(
            registry,
            arm_name,
            "facing_direction",
            StateValue::Int(piston_facing_value(facing)),
        ) {
            world.write(pos + dir, arm);
            written.push(pos + dir);
        }
        for (at, actor) in actors {
            world.put_actor(at, actor);
        }
        world.sound("piston.out", corner(pos));
        self.arms.insert(
            pos,
            Arm {
                extended: true,
                busy_until: now + MOVE_TICKS,
            },
        );
        self.ends
            .entry(now + MOVE_TICKS)
            .or_default()
            .push((pos, true));
    }

    #[allow(clippy::too_many_arguments)]
    fn retract(
        &mut self,
        registry: &BlockRegistry,
        world: &mut (impl Circuit + ?Sized),
        table: &Table,
        pos: IVec3,
        facing: Face,
        sticky: bool,
        now: u64,
        written: &mut Vec<IVec3>,
    ) {
        let dir = facing.offset();
        let head = pos + dir;
        let air = default_state(registry, "minecraft:air");
        let mut attached = Vec::new();
        let pulled = if sticky {
            let front = pos + dir * 2;
            world
                .block_at(front)
                .filter(|state| table.get(*state).movement == Movement::PushPull)
                .and_then(|_| structure(world, table, air, pos, facing, false))
        } else {
            None
        };
        if let Some(pulled) = &pulled {
            attached.clone_from(&pulled.moved);
        }
        world.put_actor(
            pos,
            arm_actor(pos, sticky, ArmState::Retracting, 1.0, &attached, &[]),
        );
        if world
            .block_at(head)
            .is_some_and(|state| table.get(state).head)
        {
            world.write(head, air);
            written.push(head);
        }
        let actors = match pulled {
            Some(pulled) => self.shift(registry, world, pos, &pulled.moved, -dir, false, written),
            None => Vec::new(),
        };
        for (at, actor) in actors {
            world.put_actor(at, actor);
        }
        world.sound("piston.in", corner(pos));
        self.arms.insert(
            pos,
            Arm {
                extended: false,
                busy_until: now + MOVE_TICKS,
            },
        );
        self.ends
            .entry(now + MOVE_TICKS)
            .or_default()
            .push((pos, false));
    }

    /// Move `blocks` one step in `dir`: each becomes a moving block at its
    /// destination and its place air, in order. Returns the moving blocks'
    /// actors, which BDS sends after all the blocks.
    #[allow(clippy::too_many_arguments)]
    fn shift(
        &mut self,
        registry: &BlockRegistry,
        world: &mut (impl Circuit + ?Sized),
        piston: IVec3,
        blocks: &[IVec3],
        dir: IVec3,
        expanding: bool,
        written: &mut Vec<IVec3>,
    ) -> Vec<(IVec3, tag::Compound)> {
        let air = default_state(registry, "minecraft:air");
        let moving_block = default_state(registry, "minecraft:moving_block");
        let taken: Vec<(IVec3, u32, Option<tag::Compound>)> = blocks
            .iter()
            .map(|source| {
                let state = world.block_at(*source).unwrap_or(air);
                (*source, state, world.take_actor(*source))
            })
            .collect();
        let mut actors = Vec::new();
        for (order, (source, state, actor)) in taken.into_iter().enumerate() {
            let dest = source + dir;
            world.write(dest, moving_block);
            actors.push((
                dest,
                moving_actor(registry, dest, piston, state, expanding, actor.as_ref()),
            ));
            world.write(source, air);
            written.push(dest);
            written.push(source);
            self.moving.insert(
                dest,
                Moving {
                    state,
                    actor,
                    piston,
                    order,
                },
            );
        }
        actors
    }

    /// Land the blocks of the moves ending at `now`; returns the positions
    /// written.
    pub(crate) fn end_due(
        &mut self,
        registry: &BlockRegistry,
        world: &mut (impl Circuit + ?Sized),
        table: &Table,
        now: u64,
    ) -> Vec<IVec3> {
        let mut written = Vec::new();
        let moving_block = default_state(registry, "minecraft:moving_block");
        for (piston, extended) in self.ends.remove(&now).unwrap_or_default() {
            // BDS's order: the piston's actor, the landed blocks, their
            // actors.
            if let Some(state) = world.block_at(piston)
                && let Kind::Consumer(Consumer::Piston { sticky, .. }) = table.get(state).kind
            {
                let (arm, progress) = if extended {
                    (ArmState::Extended, 1.0)
                } else {
                    (ArmState::Retracted, 0.0)
                };
                world.put_actor(piston, arm_actor(piston, sticky, arm, progress, &[], &[]));
            }
            let mut landing: Vec<(usize, IVec3)> = self
                .moving
                .iter()
                .filter(|(_, moving)| moving.piston == piston)
                .map(|(dest, moving)| (moving.order, *dest))
                .collect();
            landing.sort_unstable_by_key(|(order, _)| *order);
            let mut actors = Vec::new();
            for (_, dest) in landing {
                let Some(moving) = self.moving.remove(&dest) else {
                    continue;
                };
                if world.block_at(dest) != Some(moving_block) {
                    continue;
                }
                let mut state = moving.state;
                // An observer lands powered and pulses (BDS 1.26.51).
                if matches!(table.get(state).kind, Kind::Observer { .. }) {
                    state = registry
                        .get_by_runtime_id(state)
                        .and_then(|entry| {
                            entry.with_state_value(state, "powered_bit", StateValue::Byte(1))
                        })
                        .unwrap_or(state);
                }
                world.take_actor(dest);
                world.write(dest, state);
                written.push(dest);
                if let Some(actor) = moving.actor {
                    actors.push((dest, moved_actor(actor, dest)));
                }
            }
            for (at, actor) in actors {
                world.put_actor(at, actor);
            }
        }
        written
    }
}
