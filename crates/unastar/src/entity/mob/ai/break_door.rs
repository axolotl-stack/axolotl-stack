//! `minecraft:annotation.break_door`: a pathing mob breaks the closed door
//! its path runs into.
//!
//! This is a per-tick check, not a goal, run after the AI moved every mob
//! ([`tick_door_breakers`]). For each living door breaker:
//!
//! 1. Without a path being followed, everything is forgotten (the clean-up:
//!    a crack in progress is stopped, see below).
//! 2. The mob may break doors when the world difficulty is at least the
//!    annotation's `min_difficulty`, the `mobgriefing` rule is on and the
//!    block of the waypoint being walked to is not an iron door; otherwise
//!    clean-up.
//! 3. A new attack target clears the progress.
//! 4. A path index lower than last tick's (a new path) cleans up. The path
//!    must stay on the same waypoint for a tick before anything happens.
//! 5. Not breaking yet: a remembered door restarts with probability 1 in
//!    [`BREAK_DOOR_RESTART_ROLL`] per tick; otherwise the waypoint's block
//!    must be a door that blocks the path (its panel lies on the side the
//!    path enters the cell from, or on the far side while the mob is not yet
//!    in the cell), or the door is the path's last waypoint and the mob is
//!    not in its cell. Starting sends viewers `LevelEvent` StartBlockCracking
//!    (data `65535 / break ticks`) at the door cell's centre and sets the
//!    `BREAKING_OBSTRUCTION` actor flag.
//! 6. Breaking: off the ground the crack stops (`LevelEvent`
//!    StopBlockCracking, flag cleared, progress lost). Otherwise each tick
//!    plays the door bash (`LevelEvent` SoundZombieWoodenDoor) with
//!    probability 1 in [`BREAK_DOOR_BASH_ROLL`]; `break ticks + 1` ticks
//!    after the start the door half is removed without drops,
//!    `LevelEvent` SoundZombieDoorCrash and ParticlesDestroyBlockNoSound
//!    (the door's default state) play, the path counts the door waypoint as
//!    reached and the flag clears. The other door half, no longer supported,
//!    is destroyed with its block drops and destroy particles (the lower
//!    half drops the door item, the upper one nothing). BDS destroys it one
//!    tick later from its block tick queue; unastar has no block tick queue
//!    and destroys it in the same tick.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::LevelEventPacket;
use jolyne::valentine::actor_flags;
use jolyne::valentine::types::Vec3;
use rand::Rng;

use super::eat_block::replace_block;
use super::{MobBrain, PathBlockTable, set_flag};
use crate::entity::EntityRandom;
use crate::entity::components::{OnGround, PhysicsBody, Position};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::components::doors::{BreakDoorConfig, DoorBreakState};
use crate::entity::mob::constants::{
    BLOCK_CRACK_DATA_SCALE, BREAK_DOOR_BASH_ROLL, BREAK_DOOR_RESTART_ROLL, DOOR_UPPER_STATE,
    LEVEL_EVENT_PARTICLES_DESTROY_BLOCK, LEVEL_EVENT_PARTICLES_DESTROY_BLOCK_NO_SOUND,
    LEVEL_EVENT_SOUND_ZOMBIE_DOOR_CRASH, LEVEL_EVENT_SOUND_ZOMBIE_WOODEN_DOOR,
    LEVEL_EVENT_START_BLOCK_CRACKING, LEVEL_EVENT_STOP_BLOCK_CRACKING,
};
use crate::entity::mob::net::sound::level_event;
use crate::entity::mob::state::AttackTarget;
use crate::entity::navigation::WalkWaypoint;
use crate::entity::systems::BlockSource;
use crate::registry::block::StateValue;
use crate::registry::collision::{Aabb, block_collision_boxes};
use crate::world::rules::{Difficulty, GameRules};

/// Sides of a block cell, as door panels and path entries name them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    /// -x
    West,
    /// +x
    East,
    /// -z
    North,
    /// +z
    South,
}

impl Side {
    fn opposite(self) -> Self {
        match self {
            Self::West => Self::East,
            Self::East => Self::West,
            Self::North => Self::South,
            Self::South => Self::North,
        }
    }
}

/// Run the door-breaking check (see the module docs) for every living mob in
/// `mobs`.
pub(super) fn tick_door_breakers(
    world: &mut World,
    mobs: impl Iterator<Item = Entity>,
    tick: u64,
    boxes: &mut Vec<Aabb>,
) {
    for mob in mobs {
        let (Some(config), Some(state)) = (
            world.get::<BreakDoorConfig>(mob).copied(),
            world.get::<DoorBreakState>(mob).copied(),
        ) else {
            continue;
        };
        let mut state = state;
        check(world, mob, config, &mut state, tick, boxes);
        if let Some(mut stored) = world.get_mut::<DoorBreakState>(mob)
            && *stored != state
        {
            *stored = state;
        }
    }
}

/// Stop a crack in progress when `mob` loses the annotation.
pub(crate) fn stop_breaking_doors(mob: &mut EntityWorldMut<'_>) {
    let door = mob.get::<DoorBreakState>().and_then(|state| state.door);
    if let Some(door) = door {
        stop_crack(mob, door);
    }
}

fn centre(cell: IVec3) -> DVec3 {
    cell.as_dvec3() + DVec3::splat(0.5)
}

fn breaking(world: &World, mob: Entity) -> bool {
    world.get::<ActorFlags>(mob).is_some_and(|flags| {
        flags
            .get(actor_flags::BREAKING_OBSTRUCTION)
            .unwrap_or(false)
    })
}

/// If `mob` is breaking: tell viewers the crack at `door` stopped and clear
/// the flag.
fn stop_crack(mob: &mut EntityWorldMut<'_>, door: IVec3) {
    let id = mob.id();
    let was_breaking = mob.world_scope(|world| breaking(world, id));
    if !was_breaking {
        return;
    }
    mob.world_scope(|world| level_event(world, centre(door), LEVEL_EVENT_STOP_BLOCK_CRACKING, 0));
    set_flag(mob, actor_flags::BREAKING_OBSTRUCTION, false);
}

/// Forget the door, the progress, the path index and the target.
fn clean_up(world: &mut World, mob: Entity, state: &mut DoorBreakState) {
    if let Some(door) = state.door {
        stop_crack(&mut world.entity_mut(mob), door);
    }
    *state = DoorBreakState::default();
}

/// Lose the progress; the door stays remembered.
fn clear_progress(world: &mut World, mob: Entity, state: &mut DoorBreakState) {
    if let Some(door) = state.door {
        stop_crack(&mut world.entity_mut(mob), door);
    }
    state.breaks_at = None;
}

fn waypoint_cell(waypoint: &WalkWaypoint) -> IVec3 {
    waypoint.feet.floor().as_ivec3()
}

fn mob_box(world: &World, mob: Entity) -> Option<Aabb> {
    let feet = world.get::<Position>(mob)?.0;
    let body = world.get::<PhysicsBody>(mob)?;
    Some(Aabb::from_feet(feet, body.width, body.height))
}

fn cell_box(cell: IVec3) -> Aabb {
    Aabb::new(cell.as_dvec3(), cell.as_dvec3() + DVec3::ONE)
}

/// The side of its cell a door state's panel lies on, from its collision
/// shape.
fn panel_side(state: u32, boxes: &mut Vec<Aabb>) -> Option<Side> {
    boxes.clear();
    block_collision_boxes(state, boxes);
    let panel = boxes.first()?;
    let size = panel.max - panel.min;
    if size.x < size.z {
        Some(if panel.min.x <= 0.0 {
            Side::West
        } else {
            Side::East
        })
    } else if size.z < size.x {
        Some(if panel.min.z <= 0.0 {
            Side::North
        } else {
            Side::South
        })
    } else {
        None
    }
}

/// Whether the door `state` at `cell` (the waypoint at `index`) stands across
/// the path: its panel is on the side the path to the next waypoint enters
/// from, or on the far side while the mob is not yet in the cell.
fn door_blocks_path(
    state: u32,
    cell: IVec3,
    path: &[WalkWaypoint],
    index: usize,
    mob: Aabb,
    boxes: &mut Vec<Aabb>,
) -> bool {
    let Some(next) = path.get(index + 1).map(waypoint_cell) else {
        return false;
    };
    let delta = next - cell;
    let entry = if delta.x.abs() <= delta.z.abs() {
        if delta.z > 0 {
            Side::North
        } else {
            Side::South
        }
    } else if delta.x > 0 {
        Side::West
    } else {
        Side::East
    };
    let Some(panel) = panel_side(state, boxes) else {
        return false;
    };
    panel == entry || (panel == entry.opposite() && !mob.intersects(&cell_box(cell)))
}

/// Whether a closed door `mob` may break stands across `path` at the
/// waypoint `index` (the native follower then neither drops the path as
/// stuck nor advances past the door).
pub(super) fn held_by_breakable_door(
    world: &World,
    mob: Entity,
    table: &PathBlockTable,
    path: &[WalkWaypoint],
    index: usize,
    mob_box: Aabb,
) -> bool {
    let Some(config) = world.get::<BreakDoorConfig>(mob) else {
        return false;
    };
    let cell = waypoint_cell(&path[index]);
    let Some(door) = world
        .block_at(cell)
        .filter(|&block| table.is_door(block) && !table.is_iron_door(block))
    else {
        return false;
    };
    *world.resource::<Difficulty>() >= config.min_difficulty
        && world.resource::<GameRules>().mob_griefing
        && door_blocks_path(door, cell, path, index, mob_box, &mut Vec::new())
}

fn check(
    world: &mut World,
    mob: Entity,
    config: BreakDoorConfig,
    state: &mut DoorBreakState,
    tick: u64,
    boxes: &mut Vec<Aabb>,
) {
    let current = world.get::<MobBrain>(mob).and_then(|brain| {
        brain
            .nav
            .current()
            .map(|(index, path)| (index, path.len(), waypoint_cell(&path[index])))
    });
    let Some((index, len, cell)) = current else {
        clean_up(world, mob, state);
        return;
    };
    let block = world.block_at(cell);
    let table = world.resource::<PathBlockTable>();
    let iron = block.is_some_and(|block| table.is_iron_door(block));
    let door = block.filter(|&block| table.is_door(block));
    if *world.resource::<Difficulty>() < config.min_difficulty
        || !world.resource::<GameRules>().mob_griefing
        || iron
    {
        clean_up(world, mob, state);
        return;
    }
    let target = world.get::<AttackTarget>(mob).map(|target| target.0);
    if target != state.target {
        clear_progress(world, mob, state);
        state.target = target;
    }
    if state
        .last_index
        .is_some_and(|last| index < last || last >= len)
    {
        clean_up(world, mob, state);
    }
    if state.last_index != Some(index) {
        state.last_index = Some(index);
        return;
    }

    if !breaking(world, mob) {
        let door_cell = match state.door {
            Some(remembered) => {
                if world
                    .resource_mut::<EntityRandom>()
                    .0
                    .gen_range(0..BREAK_DOOR_RESTART_ROLL)
                    != 0
                {
                    return;
                }
                remembered
            }
            None => {
                let (Some(door), Some(mob_box)) = (door, mob_box(world, mob)) else {
                    return;
                };
                let in_node = mob_box.intersects(&cell_box(cell));
                let path = &world
                    .get::<MobBrain>(mob)
                    .expect("checked above")
                    .nav
                    .current()
                    .expect("checked above")
                    .1;
                if !door_blocks_path(door, cell, path, index, mob_box, boxes)
                    && (index + 1 != len || in_node)
                {
                    return;
                }
                state.door = Some(cell);
                cell
            }
        };
        let data = (1.0 / config.break_ticks as f32 * BLOCK_CRACK_DATA_SCALE) as i32;
        level_event(
            world,
            centre(door_cell),
            LEVEL_EVENT_START_BLOCK_CRACKING,
            data,
        );
        set_flag(
            &mut world.entity_mut(mob),
            actor_flags::BREAKING_OBSTRUCTION,
            true,
        );
        state.breaks_at = Some(tick + u64::from(config.break_ticks) + 1);
        return;
    }

    let door_cell = state.door.expect("breaking remembers its door");
    if !world.get::<OnGround>(mob).is_some_and(|ground| ground.0) {
        stop_crack(&mut world.entity_mut(mob), door_cell);
        state.breaks_at = None;
        return;
    }
    if world
        .resource_mut::<EntityRandom>()
        .0
        .gen_range(0..BREAK_DOOR_BASH_ROLL)
        == 0
    {
        level_event(
            world,
            centre(door_cell),
            LEVEL_EVENT_SOUND_ZOMBIE_WOODEN_DOOR,
            0,
        );
    }
    if state.breaks_at.is_some_and(|at| tick < at) {
        return;
    }
    break_door(world, door_cell);
    state.breaks_at = None;
    state.door = None;
    if let Some(mut brain) = world.get_mut::<MobBrain>(mob) {
        brain.nav.skip_waypoint();
    }
    set_flag(
        &mut world.entity_mut(mob),
        actor_flags::BREAKING_OBSTRUCTION,
        false,
    );
}

/// Remove the door half at `cell` without drops (crash sound and quiet
/// destroy particles), then destroy the unsupported other half with its
/// drops.
fn break_door(world: &mut World, cell: IVec3) {
    let Some(state) = world.block_at(cell) else {
        return;
    };
    let registry = world.registry();
    let Some(entry) = registry.get_by_runtime_id(state) else {
        return;
    };
    let Some(air) = registry.default_state_id_by_name("minecraft:air") else {
        return;
    };
    let default_state = entry.default_state_id;
    let upper = entry.state_value(state, DOOR_UPPER_STATE) == Some(StateValue::Byte(1));
    let other = if upper {
        cell - IVec3::Y
    } else {
        cell + IVec3::Y
    };
    let other_state = world
        .block_at(other)
        .filter(|&other| (entry.min_state_id..=entry.max_state_id).contains(&other));
    // Particle data names the block by its wire id.
    let ids = crate::server::game::types::block_network_ids(world);

    level_event(world, centre(cell), LEVEL_EVENT_SOUND_ZOMBIE_DOOR_CRASH, 0);
    replace_block(
        world,
        cell,
        air,
        LevelEventPacket {
            event_id: LEVEL_EVENT_PARTICLES_DESTROY_BLOCK_NO_SOUND,
            position: wire(centre(cell)),
            data: ids.to_wire(default_state) as i32,
        },
    );
    if let Some(other_state) = other_state {
        replace_block(
            world,
            other,
            air,
            LevelEventPacket {
                event_id: LEVEL_EVENT_PARTICLES_DESTROY_BLOCK,
                position: wire(centre(other)),
                data: ids.to_wire(other_state) as i32,
            },
        );
        crate::server::game::drop_block_items(world, other, other_state, None);
    }
}

fn wire(position: DVec3) -> Vec3 {
    Vec3 {
        x: position.x as f32,
        y: position.y as f32,
        z: position.z as f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::navigation::WalkMovement;
    use crate::registry::BlockRegistry;

    fn door(direction: &'static str, open: bool) -> u32 {
        let registry = BlockRegistry::vanilla();
        let entry = registry.get_by_name("minecraft:wooden_door").unwrap();
        let state = entry
            .with_state_value(
                entry.default_state_id,
                "minecraft:cardinal_direction",
                StateValue::Str(direction),
            )
            .unwrap();
        entry
            .with_state_value(state, "open_bit", StateValue::Byte(u8::from(open)))
            .unwrap()
    }

    fn waypoint(x: f64, z: f64) -> WalkWaypoint {
        WalkWaypoint {
            feet: DVec3::new(x, 64.0, z),
            movement: WalkMovement::Walk,
            lift_height: 0.0,
        }
    }

    /// Path along +x through the door cell (0, 64, 0).
    fn path() -> Vec<WalkWaypoint> {
        vec![waypoint(-1.5, 0.5), waypoint(0.5, 0.5), waypoint(1.5, 0.5)]
    }

    fn outside() -> Aabb {
        Aabb::from_feet(DVec3::new(-0.6, 64.0, 0.5), 0.6, 1.95)
    }

    #[test]
    fn closed_door_across_the_path_blocks_it_from_either_side() {
        let mut boxes = Vec::new();
        let mut blocking = Vec::new();
        for direction in ["north", "south", "east", "west"] {
            let state = door(direction, false);
            let side = panel_side(state, &mut boxes).unwrap();
            let blocks = door_blocks_path(
                state,
                IVec3::new(0, 64, 0),
                &path(),
                1,
                outside(),
                &mut boxes,
            );
            assert_eq!(
                blocks,
                matches!(side, Side::West | Side::East),
                "{direction}: panel on {side:?}"
            );
            if blocks {
                blocking.push(side);
            }
        }
        assert!(
            blocking.contains(&Side::West) && blocking.contains(&Side::East),
            "{blocking:?}"
        );
    }

    #[test]
    fn far_panel_does_not_block_a_mob_already_in_the_cell() {
        let mut boxes = Vec::new();
        let inside = Aabb::from_feet(DVec3::new(0.4, 64.0, 0.5), 0.6, 1.95);
        for direction in ["north", "south", "east", "west"] {
            let state = door(direction, false);
            if panel_side(state, &mut boxes) == Some(Side::East) {
                assert!(!door_blocks_path(
                    state,
                    IVec3::new(0, 64, 0),
                    &path(),
                    1,
                    inside,
                    &mut boxes
                ));
                assert!(door_blocks_path(
                    state,
                    IVec3::new(0, 64, 0),
                    &path(),
                    1,
                    outside(),
                    &mut boxes
                ));
            }
        }
    }

    #[test]
    fn last_waypoint_has_no_direction() {
        let mut boxes = Vec::new();
        let state = door("east", false);
        assert!(!door_blocks_path(
            state,
            IVec3::new(0, 64, 0),
            &path()[..2],
            1,
            outside(),
            &mut boxes
        ));
    }
}
