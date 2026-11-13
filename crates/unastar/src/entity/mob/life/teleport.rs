//! `minecraft:teleport` (compiled in `mob::components::teleport`): the
//! enderman's teleports, measured on BDS 1.26.51 (`sweep.py enderman`,
//! `docs/research/enderman-bds-1.26.51.md`).
//!
//! Native rules:
//! - An attempt draws a target uniformly in the random teleport cube around
//!   the feet (half the cube each way, x, y, z), drops it block by block
//!   while the block below it has no collision, and stands the mob there
//!   (the block's floor) when its box then collides with no block and
//!   touches no liquid. Otherwise the attempt fails and nothing is sent: a
//!   target drawn inside the ground fails, so on a superflat about half do.
//! - The random teleport timer starts expired, so a mob makes its first
//!   attempt in the tick it spawned; every attempt redraws it uniformly
//!   between the minimum and maximum random teleport time. Each tick the
//!   timer has not run out, a roll of the light or the dark chance (the
//!   feet block's brightness) makes an attempt too.
//! - A mob whose target is farther than `target_distance` rolls the
//!   target chance each tick and aims a teleport that far toward it (off by
//!   up to 4, 8 and 4 blocks); a failed aim falls back to a random attempt
//!   (BDS 1.26.51 `chase`: six of thirteen teleports of an enderman 17 to
//!   40 blocks from its target landed within 4 blocks of that aim).
//! - Every hurt the mob takes makes two independent attempts in the same
//!   tick (BDS 1.26.51 superflat: of 240 hurts, 64 teleported the enderman
//!   no time, 120 once, 54 twice, as two coin flips would).
//! - A projectile about to hit a mob with `teleports_on_projectile_hit`
//!   makes it try to teleport, again and again up to a bound; when one
//!   lands the projectile misses and flies on, otherwise it hits (BDS
//!   1.26.51: 13 of 13 dispensed arrows dodged on the superflat, where one
//!   attempt in two fails; in a sealed room every arrow hit).
//! - A teleport moves the mob at once: viewers get a `MoveActorDelta` with
//!   `force_move`, the trail particles (`LevelEventGeneric` 2023 from the
//!   old feet to the middle of the new feet block) and level event 1018
//!   at the old and at the new feet.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use rand::Rng;

use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{FallDistance, PhysicsBody, Position, Velocity};
use crate::entity::mob::MobKind;
use crate::entity::mob::components::TeleportConfig;
use crate::entity::mob::constants::{
    HURT_TELEPORT_ATTEMPTS, PROJECTILE_DODGE_ATTEMPTS, TELEPORT_LEVEL_EVENT,
    TELEPORT_LIGHT_BRIGHTNESS, TELEPORT_TOWARD_JITTER, TELEPORT_TRAIL_COUNT, TELEPORT_TRAIL_EVENT,
    TELEPORT_TRAIL_SCALE,
};
use crate::entity::mob::net::network::MobNetState;
use crate::entity::mob::net::sound;
use crate::entity::mob::state::{AttackTarget, Dead};
use crate::entity::systems::BlockSource;
use crate::entity::systems::physics::{collides_with_blocks, living_liquid};
use crate::registry::collision::{Aabb, block_collision_aabbs};

/// When a mob's random teleport timer runs out (absent: it has run out).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RandomTeleportTimer {
    pub due_tick: u64,
}

/// A mob that teleports and its random teleport timer.
type Teleporter = (
    Entity,
    &'static TeleportConfig,
    Option<&'static RandomTeleportTimer>,
);

/// System (exclusive, `EntityLogicSet`): the random teleports, then the
/// teleports toward a far target.
pub fn tick_random_teleports(
    world: &mut World,
    mobs: &mut QueryState<Teleporter, (With<MobKind>, Without<Dead>)>,
    mut due: Local<Vec<(Entity, TeleportConfig, bool)>>,
) {
    let now = world.resource::<TickCounter>().current;
    due.extend(mobs.iter(world).map(|(entity, config, timer)| {
        (
            entity,
            config.clone(),
            timer.is_none_or(|timer| now >= timer.due_tick),
        )
    }));
    for (mob, config, expired) in due.drain(..) {
        let attempt = if !config.random_teleports {
            false
        } else if expired {
            let span = {
                let mut rng = world.resource_mut::<EntityRandom>();
                rng.0
                    .gen_range(config.min_random_ticks..=config.max_random_ticks)
            };
            world.entity_mut(mob).insert(RandomTeleportTimer {
                due_tick: now + u64::from(span),
            });
            true
        } else {
            let chance = if in_light(world, mob) {
                config.light_chance
            } else {
                config.dark_chance
            };
            world.resource_mut::<EntityRandom>().0.r#gen::<f32>() < chance
        };
        if attempt {
            random_teleport(world, mob, &config);
        }
        chase(world, mob, &config);
    }
}

/// A mob whose target is farther than `target_distance` rolls the target
/// chance each tick and teleports toward it: aimed `target_distance` along
/// the line to the target, off by up to [`TELEPORT_TOWARD_JITTER`] on each
/// axis. A failed aim falls back to a random attempt.
fn chase(world: &mut World, mob: Entity, config: &TeleportConfig) {
    let Some(target) = world.get::<AttackTarget>(mob).map(|t| t.0) else {
        return;
    };
    let (Some(feet), Some(goal)) = (
        world.get::<Position>(mob).map(|p| p.0),
        crate::entity::mob::ai::target_feet(world, target),
    ) else {
        return;
    };
    let reach = f64::from(config.target_distance);
    let away = goal - feet;
    if away.length() <= reach || away.length() <= f64::EPSILON {
        return;
    }
    let jitter = {
        let mut rng = world.resource_mut::<EntityRandom>();
        if rng.0.r#gen::<f32>() >= config.target_chance {
            return;
        }
        let j = TELEPORT_TOWARD_JITTER;
        DVec3::new(
            rng.0.gen_range(-j.x..=j.x),
            rng.0.gen_range(-j.y..=j.y),
            rng.0.gen_range(-j.z..=j.z),
        )
    };
    let aim = feet + away.normalize() * reach + jitter;
    if !teleport_to(world, mob, aim) {
        random_teleport(world, mob, config);
    }
}

/// The light roll applies at the feet block's brightness of at least
/// [`TELEPORT_LIGHT_BRIGHTNESS`] (sky light only: block light is not modelled).
fn in_light(world: &World, mob: Entity) -> bool {
    world.get::<Position>(mob).is_some_and(|feet| {
        crate::world::clock::sky_brightness(world, feet.0.floor().as_ivec3())
            >= TELEPORT_LIGHT_BRIGHTNESS
    })
}

/// A hurt mob with `minecraft:teleport` makes [`HURT_TELEPORT_ATTEMPTS`]
/// attempts, each from where the last one left it.
pub(crate) fn teleport_on_hurt(world: &mut World, mob: Entity) {
    if let Some(config) = world.get::<TeleportConfig>(mob).cloned() {
        for _ in 0..HURT_TELEPORT_ATTEMPTS {
            random_teleport(world, mob, &config);
        }
    }
}

/// A projectile is about to hit `mob`: one with `teleports_on_projectile_hit`
/// makes up to [`PROJECTILE_DODGE_ATTEMPTS`] attempts and, when one lands,
/// is not hit (the projectile flies on). Whether it dodged.
pub(crate) fn dodge_projectile(world: &mut World, mob: Entity) -> bool {
    let Some(config) = world
        .get::<TeleportConfig>(mob)
        .filter(|config| config.on_projectile_hit)
        .cloned()
    else {
        return false;
    };
    (0..PROJECTILE_DODGE_ATTEMPTS).any(|_| random_teleport(world, mob, &config))
}

/// One random teleport attempt; whether the mob moved.
pub(crate) fn random_teleport(world: &mut World, mob: Entity, config: &TeleportConfig) -> bool {
    let Some(feet) = world.get::<Position>(mob).map(|p| p.0) else {
        return false;
    };
    let reach = config.reach;
    let offset = {
        let mut rng = world.resource_mut::<EntityRandom>();
        let mut axis = |r: f64| {
            if r > 0.0 {
                rng.0.gen_range(-r..=r)
            } else {
                0.0
            }
        };
        DVec3::new(axis(reach.x), axis(reach.y), axis(reach.z))
    };
    teleport_to(world, mob, feet + offset)
}

/// Where a mob drawn at `target` would stand: the target dropped onto the
/// first block below with collision, if its box fits there and touches no
/// liquid.
pub fn landing(blocks: &impl BlockSource, body: &PhysicsBody, target: DVec3) -> Option<DVec3> {
    let mut cell = target.floor().as_ivec3();
    while cell.y > crate::world::chunk::MIN_Y && !solid(blocks, cell - IVec3::Y)? {
        cell.y -= 1;
    }
    if cell.y <= crate::world::chunk::MIN_Y {
        return None;
    }
    let feet = DVec3::new(target.x, f64::from(cell.y), target.z);
    let aabb = Aabb::from_feet(feet, body.width, body.height);
    (!collides_with_blocks(blocks, aabb) && living_liquid(blocks, aabb).is_none()).then_some(feet)
}

/// Whether the block at `pos` has any collision box (`None`: not loaded).
fn solid(blocks: &impl BlockSource, pos: IVec3) -> Option<bool> {
    blocks
        .block_at(pos)
        .map(|state| block_collision_aabbs(state).next().is_some())
}

/// Teleport `mob` to the landing for `target`, if there is one.
pub(crate) fn teleport_to(world: &mut World, mob: Entity, target: DVec3) -> bool {
    let (Some(from), Some(body)) = (
        world.get::<Position>(mob).map(|p| p.0),
        world.get::<PhysicsBody>(mob).copied(),
    ) else {
        return false;
    };
    let Some(to) = landing(world, &body, target) else {
        return false;
    };
    let mut entity = world.entity_mut(mob);
    entity.insert(Position(to));
    if let Some(mut velocity) = entity.get_mut::<Velocity>() {
        velocity.0 = DVec3::ZERO;
    }
    // It arrives standing: nothing it fell before counts.
    if let Some(mut fall) = entity.get_mut::<FallDistance>() {
        *fall = FallDistance::default();
    }
    if let Some(mut net) = entity.get_mut::<MobNetState>() {
        net.teleported();
    }
    let end = to.floor() + DVec3::splat(0.5);
    sound::level_event_generic(world, from, TELEPORT_TRAIL_EVENT, trail(from, end, &body));
    sound::level_event(world, from, TELEPORT_LEVEL_EVENT, 0);
    sound::level_event(world, to, TELEPORT_LEVEL_EVENT, 0);
    true
}

/// The trail particles' fields, as BDS 1.26.51 sends them.
fn trail(start: DVec3, end: DVec3, body: &PhysicsBody) -> zuri_nbt::tag::Compound {
    use zuri_nbt::NBTTag;
    use zuri_nbt::tag::{Float, Int};
    let float = |v: f64| NBTTag::Float(Float(v as f32));
    let fields = [
        ("Count", NBTTag::Int(Int(TELEPORT_TRAIL_COUNT))),
        ("DirScale", NBTTag::Float(Float(TELEPORT_TRAIL_SCALE))),
        ("Endx", float(end.x)),
        ("Endy", float(end.y)),
        ("Endz", float(end.z)),
        ("Startx", float(start.x)),
        ("Starty", float(start.y)),
        ("Startz", float(start.z)),
        ("Variationx", float(body.width)),
        ("Variationy", float(body.height)),
    ];
    zuri_nbt::tag::Compound(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}
