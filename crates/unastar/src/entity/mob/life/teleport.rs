//! `minecraft:teleport` (compiled in `mob::components::teleport`) and the
//! teleports of a class that teleports when hurt
//! (`NativeTraits::teleports_when_hurt`).
//!
//! Native rules, every tick for every living mob with the component:
//! - Light: with a positive light or dark chance, the brightness of the
//!   feet block picks the chance: above [`TELEPORT_LIGHT_BRIGHTNESS`] under
//!   the open sky `light_teleport_chance`, at or below it
//!   `dark_teleport_chance` (bright under a roof: none). A uniform roll below
//!   it teleports randomly and ends the tick.
//! - Without a target, with `random_teleports`: once the timer is due the
//!   mob draws the next interval (`nextInt(min, max)` ticks, at least one)
//!   and teleports randomly. The timer does not run while it has a target.
//! - With a target farther than `target_distance`: a uniform roll below
//!   `target_teleport_chance` teleports toward it.
//!
//! A random teleport aims at the mob's position plus `U(-0.5, 0.5)` times
//! `random_teleport_cube` on each axis; a teleport toward the target aims
//! `TELEPORT_TOWARD_*` around the mob, pulled back along the direction
//! from the target's head to the mob's middle. The aimed block column is
//! searched downwards (the chunk loaded, above the world's floor) for a
//! block whose collision box reaches its top; the mob's box standing on it
//! at the aimed x/z must touch no block box and no liquid, else nothing
//! happens. A landed teleport moves the mob (viewers get a teleporting
//! `MoveActorAbsolute`), sends the trail particles
//! ([`LEVEL_EVENT_PARTICLES_TELEPORT_TRAIL`]) and the teleport sound
//! ([`LEVEL_EVENT_SOUND_TELEPORT`]) at the old position and, unless the mob
//! is silent, at the new one.
//!
//! `teleports_on_projectile_hit` and `projectile_filter` are compiled but
//! do not act: the projectile rule is the native class's
//! ([`hurt_teleport`]).

use std::collections::BTreeMap;

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::{LevelEventGenericPacket, McpePacket, actor_flags};
use rand::Rng;
use unastar_data::actor_components::body::ActorDamageCause;
use unastar_packs::nbt::{Tag, encode_network_fields};
use valentine::bedrock::codec::NbtCompoundBody;

use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{FallDistance, Health, PhysicsBody, Position};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::MobKind;
use crate::entity::mob::ai::{full_top, stop_navigation};
use crate::entity::mob::combat::Hit;
use crate::entity::mob::components::teleport::{TeleportConfig, TeleportTimer};
use crate::entity::mob::constants::{
    HURT_TELEPORT_PROJECTILE_ATTEMPTS, HURT_TELEPORT_SKIP_ROLL,
    LEVEL_EVENT_PARTICLES_TELEPORT_TRAIL, LEVEL_EVENT_SOUND_TELEPORT, MOB_EYE_HEIGHT_RATIO,
    OVERWORLD_BRIGHTNESS_RAMP, TELEPORT_LIGHT_BRIGHTNESS, TELEPORT_TOWARD_PULL,
    TELEPORT_TOWARD_SPREAD, TELEPORT_TOWARD_VERTICAL_RANGE, TELEPORT_TRAIL_COUNT,
    TELEPORT_TRAIL_DIR_SCALE,
};
use crate::entity::mob::net::network::Teleported;
use crate::entity::mob::net::sound::{level_event, send_to_viewers};
use crate::entity::mob::state::{AttackTarget, Dead};
use crate::entity::systems::physics::{BlockSource, collides_with_blocks, living_liquid};
use crate::registry::collision::Aabb;
use crate::world::ChunkManager;
use crate::world::chunk::MIN_Y;
use crate::world::ecs::{ChunkData, world_to_chunk_coords, world_to_local_coords};

/// What a mob's teleport component does this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Random,
    Toward(Entity),
}

/// The overworld brightness of the light at `block` (raw brightness,
/// `world::light::brightness`).
fn brightness(world: &World, block: IVec3) -> f32 {
    let light = crate::world::light::brightness(world, block);
    OVERWORLD_BRIGHTNESS_RAMP[usize::from(light.min(15))]
}

/// Whether `block` is at or above its column's height map (nothing above
/// it blocks the sky); unloaded columns do not see the sky.
fn sees_sky(world: &World, block: IVec3) -> bool {
    let (cx, cz) = world_to_chunk_coords(block.x, block.z);
    let Some(chunk) = world
        .resource::<ChunkManager>()
        .get_by_coords(cx, cz)
        .and_then(|chunk| world.get::<ChunkData>(chunk))
    else {
        return false;
    };
    let (lx, _, lz) = world_to_local_coords(block.x, block.y, block.z);
    i32::from(chunk.inner.height_map().at(lx, lz)) <= block.y
}

/// The rolls of one mob's teleport tick.
fn decide(
    world: &World,
    rng: &mut impl Rng,
    mob: Entity,
    config: &TeleportConfig,
    timer: TeleportTimer,
    target: Option<Entity>,
    now: u64,
) -> (Option<Move>, Option<u64>) {
    let Some(feet) = world.get::<Position>(mob).map(|p| p.0) else {
        return (None, None);
    };
    if config.light_teleport_chance > 0.0 || config.dark_teleport_chance > 0.0 {
        let block = feet.floor().as_ivec3();
        let light = brightness(world, block);
        let chance = if light > TELEPORT_LIGHT_BRIGHTNESS {
            if sees_sky(world, block) {
                config.light_teleport_chance
            } else {
                0.0
            }
        } else {
            config.dark_teleport_chance
        };
        if rng.r#gen::<f32>() < chance {
            return (Some(Move::Random), None);
        }
    }
    let Some(target) = target else {
        if !config.random_teleports || now < timer.due {
            return (None, None);
        }
        let ticks = if config.max_random_ticks > config.min_random_ticks {
            rng.gen_range(config.min_random_ticks..config.max_random_ticks)
        } else {
            config.min_random_ticks
        };
        return (Some(Move::Random), Some(now + ticks.max(1) as u64));
    };
    // The timer does not run while the mob has a target.
    let paused = Some(timer.due.max(now) + 1);
    let Some(target_pos) = world.get::<Position>(target).map(|p| p.0) else {
        return (None, paused);
    };
    let range = config.target_distance;
    if feet.distance_squared(target_pos) > range * range
        && rng.r#gen::<f32>() < config.target_teleport_chance
    {
        return (Some(Move::Toward(target)), paused);
    }
    (None, paused)
}

/// System (exclusive, `EntityLogicSet`): random, light and target
/// teleports.
#[allow(clippy::type_complexity)]
pub fn tick_teleport(
    world: &mut World,
    mobs: &mut QueryState<
        (
            Entity,
            &TeleportConfig,
            &TeleportTimer,
            Option<&AttackTarget>,
        ),
        (With<MobKind>, Without<Dead>),
    >,
    mut work: Local<Vec<(Entity, Option<Move>, Option<u64>)>>,
) {
    let now = world.resource::<TickCounter>().current;
    world.get_resource_or_init::<EntityRandom>();
    world.resource_scope(|world, mut random: Mut<EntityRandom>| {
        for (mob, config, timer, target) in mobs.iter(world) {
            let (moved, due) = decide(
                world,
                &mut random.0,
                mob,
                config,
                *timer,
                target.map(|t| t.0),
                now,
            );
            if moved.is_some() || due.is_some() {
                work.push((mob, moved, due));
            }
        }
    });
    for (mob, moved, due) in work.drain(..) {
        if let Some(due) = due
            && let Some(mut timer) = world.get_mut::<TeleportTimer>(mob)
        {
            timer.due = due;
        }
        match moved {
            Some(Move::Random) => {
                random_teleport(world, mob);
            }
            Some(Move::Toward(target)) => {
                teleport_toward(world, mob, target);
            }
            None => {}
        }
    }
}

/// Teleport `mob` to a random spot in its teleport cube; whether it landed.
pub fn random_teleport(world: &mut World, mob: Entity) -> bool {
    let (Some(cube), Some(feet)) = (
        world.get::<TeleportConfig>(mob).map(|c| c.random_cube),
        world.get::<Position>(mob).map(|p| p.0),
    ) else {
        return false;
    };
    let rng = &mut world.get_resource_or_init::<EntityRandom>().0;
    let offset = DVec3::new(
        rng.r#gen::<f64>() - 0.5,
        rng.r#gen::<f64>() - 0.5,
        rng.r#gen::<f64>() - 0.5,
    );
    teleport(world, mob, feet + offset * cube)
}

/// Teleport `mob` toward `target`; whether it landed.
pub fn teleport_toward(world: &mut World, mob: Entity, target: Entity) -> bool {
    let (Some(feet), Some(height), Some(target_feet)) = (
        world.get::<Position>(mob).map(|p| p.0),
        world.get::<PhysicsBody>(mob).map(|b| b.height),
        world.get::<Position>(target).map(|p| p.0),
    ) else {
        return false;
    };
    let target_head = target_head(world, target, target_feet);
    let away = DVec3::new(
        feet.x - target_feet.x,
        feet.y + height * 0.5 - target_head,
        feet.z - target_feet.z,
    );
    let away = if away.length() >= 1.0e-4 {
        away.normalize()
    } else {
        DVec3::ZERO
    };
    let rng = &mut world.get_resource_or_init::<EntityRandom>().0;
    let x = rng.r#gen::<f64>() - 0.5;
    let y = rng.gen_range(0..TELEPORT_TOWARD_VERTICAL_RANGE) - TELEPORT_TOWARD_VERTICAL_RANGE / 2;
    let z = rng.r#gen::<f64>() - 0.5;
    let dest = DVec3::new(
        feet.x + x * TELEPORT_TOWARD_SPREAD - away.x * TELEPORT_TOWARD_PULL,
        feet.y + f64::from(y) - away.y * TELEPORT_TOWARD_PULL,
        feet.z + z * TELEPORT_TOWARD_SPREAD - away.z * TELEPORT_TOWARD_PULL,
    );
    teleport(world, mob, dest)
}

/// The height of `target`'s head: a player's eye (its position), a mob's
/// eye height.
fn target_head(world: &World, target: Entity, position: DVec3) -> f64 {
    if world
        .get::<crate::entity::components::Player>(target)
        .is_some()
    {
        return position.y;
    }
    let height = world.get::<PhysicsBody>(target).map_or(0.0, |b| b.height);
    position.y + height * MOB_EYE_HEIGHT_RATIO
}

/// Move `mob` to the first valid spot in the column below `dest` (see the
/// module rules); whether it landed.
pub fn teleport(world: &mut World, mob: Entity, dest: DVec3) -> bool {
    let (Some(old), Some(body)) = (
        world.get::<Position>(mob).map(|p| p.0),
        world.get::<PhysicsBody>(mob).copied(),
    ) else {
        return false;
    };
    if !dest.is_finite() {
        return false;
    }
    let mut block = dest.floor().as_ivec3();
    if block.y <= MIN_Y || world.block_at(block).is_none() {
        return false;
    }
    while !full_top(world, block - IVec3::Y) {
        block.y -= 1;
        if block.y <= MIN_Y {
            return false;
        }
    }
    let feet = DVec3::new(dest.x, f64::from(block.y), dest.z);
    let aabb = Aabb::from_feet(feet, body.width, body.height);
    if collides_with_blocks(world, aabb) || living_liquid(world, aabb).is_some() {
        return false;
    }
    let mut entity = world.entity_mut(mob);
    entity.insert(Teleported);
    entity.get_mut::<Position>().expect("position read above").0 = feet;
    if let Some(mut fall) = entity.get_mut::<FallDistance>() {
        *fall = FallDistance::default();
    }
    let silent = entity
        .get::<ActorFlags>()
        .is_some_and(|flags| flags.get(actor_flags::SILENT).unwrap_or(false));
    stop_navigation(world, mob);
    send_trail(world, old, block, &body);
    level_event(world, old, LEVEL_EVENT_SOUND_TELEPORT, 0);
    if !silent {
        level_event(world, feet, LEVEL_EVENT_SOUND_TELEPORT, 0);
    }
    true
}

/// The teleport trail particles from `from` to the landing block.
fn send_trail(world: &World, from: DVec3, to: IVec3, body: &PhysicsBody) {
    let end = to.as_dvec3() + DVec3::splat(0.5);
    let fields = BTreeMap::from([
        ("Startx".to_owned(), Tag::Float(from.x as f32)),
        ("Starty".to_owned(), Tag::Float(from.y as f32)),
        ("Startz".to_owned(), Tag::Float(from.z as f32)),
        ("Endx".to_owned(), Tag::Float(end.x as f32)),
        ("Endy".to_owned(), Tag::Float(end.y as f32)),
        ("Endz".to_owned(), Tag::Float(end.z as f32)),
        ("Variationx".to_owned(), Tag::Float(body.width as f32)),
        ("Variationy".to_owned(), Tag::Float(body.height as f32)),
        ("DirScale".to_owned(), Tag::Float(TELEPORT_TRAIL_DIR_SCALE)),
        ("Count".to_owned(), Tag::Int(TELEPORT_TRAIL_COUNT)),
    ]);
    let mut nbt = encode_network_fields(&fields).expect("number tags always encode");
    // The compound's end tag.
    nbt.push(0);
    let packet = McpePacket::from(LevelEventGenericPacket {
        event_id: LEVEL_EVENT_PARTICLES_TELEPORT_TRAIL,
        __ctd__: NbtCompoundBody(nbt.into()),
    });
    send_to_viewers(world, from, &packet);
}

/// Native hurt teleports of a class that teleports when hurt, before `hit`
/// lands on `mob`; whether the hit is absorbed (deals nothing).
///
/// A projectile hit tries up to [`HURT_TELEPORT_PROJECTILE_ATTEMPTS`]
/// random teleports and is absorbed either way. Any other hit that leaves
/// the mob alive makes it teleport randomly unless
/// `nextInt(HURT_TELEPORT_SKIP_ROLL)` is 0, and then lands.
pub fn hurt_teleport(world: &mut World, mob: Entity, hit: &Hit) -> bool {
    if hit.cause == ActorDamageCause::Projectile {
        for _ in 0..HURT_TELEPORT_PROJECTILE_ATTEMPTS {
            if random_teleport(world, mob) {
                break;
            }
        }
        return true;
    }
    let skip = world
        .get_resource_or_init::<EntityRandom>()
        .0
        .gen_range(0..HURT_TELEPORT_SKIP_ROLL)
        == 0;
    let survives = world
        .get::<Health>(mob)
        .is_some_and(|health| hit.damage < health.current);
    if !skip && survives {
        random_teleport(world, mob);
    }
    false
}
