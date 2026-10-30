//! Fishing rods: casting a `minecraft:fishing_hook`, waiting for a bite and
//! reeling in.
//!
//! Using a fishing rod in the air with no hook out casts one; using it again
//! reels the hook in. The hook is a projectile actor owned by the player:
//! its actor data `OWNER` is the player's runtime id, so clients draw the
//! line to the rod. It flies by its definition's `minecraft:projectile`
//! rules (`entity::projectile`) until it
//! - touches water: it then bobs at the surface ([`HookState::Bobbing`]);
//! - sticks in a block (`stick_in_ground`);
//! - would fly into a mob or another player on its next flight segment:
//!   it hooks the actor and rides along at 0.8 of its height
//!   ([`HookState::Hooked`]).
//!
//! While the hook bobs in water the bite cycle runs: a wait of 100–600
//! ticks less 100 per Lure level, a fish approaching for 20–80 ticks, then
//! a bite of 20–40 ticks during which the hook dips; the bite starts with
//! `ActorEvent` 13 on the hook. A missed bite starts a new wait. Reeling in
//! - during a bite rolls the hook's `minecraft:loot` table (the definition
//!   switches to the jungle table in jungle biomes) with the rod's Luck of
//!   the Sea level as luck, flings every item from the hook towards the
//!   owner, gives the owner 1–6 experience (split into orbs) and wears the
//!   rod by 1;
//! - with an actor hooked (the hook's actor data `TARGET` is its unique
//!   id) pulls it towards the owner by a tenth of the distance and wears
//!   the rod by 3;
//! - with the hook on the ground wears the rod by 2; otherwise nothing.
//!
//! The hook is removed when its owner leaves, dies, no longer holds a
//! fishing rod in the selected slot or is more than 32 blocks away, and
//! 1200 ticks after it stuck in a block.
//!
//! Measured on BDS 1.26.51 (client-diff `fishing`, items3 capture): the
//! `OWNER` and `TARGET` actor data, the bite event, the wear (catch 1,
//! ground 2, hooked cow 3, reel before a bite 0) and the removal on a slot
//! switch. Public references, unmeasured (recorded in `bds-quirks.md`): the
//! cast velocity and start, the bobbing motion, the bite timers, the pull,
//! the catch velocity and experience amount, the range. Not sent: the
//! approach events 12/14 with the fish position in `FISHX`/`FISHZ`/
//! `FISHANGLE`, the cast, splash and hooked sounds, and the hurt event of a
//! hooked actor. This server has no weather, so rain does not shorten the
//! wait, and the open sky is not checked.

use std::ops::RangeInclusive;
use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::{actor_data_i_ds as ids, actor_flags};
use rand::{Rng, SeedableRng};
use tracing::warn;
use unastar_data::actor_identifiers::ActorType;

use super::{held, push_player};
use crate::ecs::resources::TickCounter;
use crate::entity::components::{
    GameMode, Health, PhysicsBody, Player, Position, Projectile, Rotation, Velocity,
};
use crate::entity::definition::components::RawCollisionBox;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::item_drop::drop_thrower_id;
use crate::entity::mob::MobKind;
use crate::entity::mob::components::LootConfig;
use crate::entity::mob::state::Dead;
use crate::entity::projectile::flight::segment_entry;
use crate::entity::projectile::{LaunchSpec, ProjectileFlight, Stuck, launch_with};
use crate::entity::systems::physics::{BlockSource, collect_block_boxes, move_aabb};
use crate::entity::{EntityRandom, PLAYER_EYE_HEIGHT};
use crate::item::ItemStack;
use crate::item::enchant::{Enchantment, enchantment_level};
use crate::registry::block::LiquidKind;
use crate::registry::collision::Aabb;
use crate::server::game::durability::wear_held;
use crate::server::game::types::ItemRegistryResource;
use crate::server::game::{LootContext, loot_stacks, player_box};

pub const FISHING_ROD: &str = "minecraft:fishing_rod";

/// The hook is removed beyond this distance from its owner's feet.
const MAX_OWNER_DISTANCE: f64 = 32.0;
/// Ticks a hook stuck in a block stays.
const GROUND_LIFETIME: u64 = 1200;
/// The hook starts this far ahead of the eye, horizontally, and
/// [`CAST_DROP`] below it (measured on BDS 1.26.51: 0.15 and 0.38).
const CAST_FORWARD: f64 = 0.15;
const CAST_DROP: f64 = 0.38;
/// Cast speed: each axis of the cast vector (horizontal unit, vertical the
/// pitch tangent clamped to ±[`CAST_MAX_SLOPE`]) is scaled by
/// `CAST_SPEED / length + CAST_MODE ± CAST_SPREAD` (triangular).
const CAST_SPEED: f64 = 0.6;
const CAST_MODE: f64 = 0.5;
const CAST_SPREAD: f64 = 0.010_336_5;
const CAST_MAX_SLOPE: f64 = 5.0;
/// Height of a hooked actor the hook rides at, as a fraction.
const HOOKED_HEIGHT: f64 = 0.8;
/// Fraction of the hook-to-owner offset a reeled actor or catch moves by.
const PULL: f64 = 0.1;
/// Extra lift of a caught item per fourth root of its distance.
const CATCH_LIFT: f64 = 0.08;
const GRAVITY: f64 = 0.03;
const DRAG: f64 = 0.92;
/// Horizontal and vertical velocity kept when the flying hook lands in
/// water.
const SPLASH_KEEP: DVec3 = DVec3::new(0.3, 0.2, 0.3);
/// Horizontal velocity kept per bobbing tick.
const BOB_DRAG: f64 = 0.9;
const BOB_PULL: f64 = 0.2;
const BITE_DIP: f64 = 0.1;
const WAIT_TICKS: RangeInclusive<i32> = 100..=600;
const LURE_TICKS_PER_LEVEL: i32 = 100;
const APPROACH_TICKS: RangeInclusive<i32> = 20..=80;
const BITE_TICKS: RangeInclusive<i32> = 20..=40;
const CATCH_EXPERIENCE: RangeInclusive<u32> = 1..=6;
const WEAR_CATCH: i32 = 1;
const WEAR_GROUND: i32 = 2;
const WEAR_ACTOR: i32 = 3;

/// The hook a player has out.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct CastHook(pub Entity);

/// What a hook is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookState {
    /// Flying by its projectile rules (or stuck in a block).
    Flying,
    /// Moved by the fishing rules: floating in water, or falling.
    Bobbing,
    /// Riding on a hooked actor.
    Hooked(Entity),
}

/// A cast fishing hook and its bite cycle.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct FishingHook {
    pub owner: Entity,
    pub state: HookState,
    /// Lure level of the rod at the cast.
    pub lure: u8,
    /// Luck of the Sea level of the rod at the cast.
    pub luck: u8,
    /// Ticks until a fish starts approaching.
    pub until_lured: i32,
    /// Ticks until the approaching fish bites.
    pub until_hooked: i32,
    /// Ticks the current bite lasts.
    pub nibble: i32,
    /// The hook stopped on the ground while bobbing or falling.
    pub on_ground: bool,
}

impl FishingHook {
    fn new(owner: Entity, lure: u8, luck: u8) -> Self {
        Self {
            owner,
            state: HookState::Flying,
            lure,
            luck,
            until_lured: 0,
            until_hooked: 0,
            nibble: 0,
            on_ground: false,
        }
    }

    /// A fish is biting: reeling in now catches it.
    pub fn biting(&self) -> bool {
        self.nibble > 0
    }
}

/// Use a fishing rod in the air: reel the hook in, or cast one.
pub fn use_rod(world: &mut World, player: Entity, rod: &ItemStack) {
    if let Some(hook) = world.get::<CastHook>(player).map(|cast| cast.0)
        && world.get::<FishingHook>(hook).is_some()
    {
        let wear = reel(world, player, hook);
        remove_hook(world, hook);
        if wear > 0 {
            wear_held(world, player, wear);
        }
        return;
    }
    cast(world, player, rod);
}

fn triangle(rng: &mut impl Rng, mode: f64, deviation: f64) -> f64 {
    mode + deviation * (rng.r#gen::<f64>() - rng.r#gen::<f64>())
}

/// Start position and velocity of a hook cast from `eye` along `look`.
fn cast_motion(eye: DVec3, look: Rotation, rng: &mut impl Rng) -> (DVec3, DVec3) {
    let (sin_yaw, cos_yaw) = f64::from(look.yaw).to_radians().sin_cos();
    let forward = DVec3::new(-sin_yaw, 0.0, cos_yaw);
    let slope = (-f64::from(look.pitch).to_radians().tan()).clamp(-CAST_MAX_SLOPE, CAST_MAX_SLOPE);
    let direction = forward + DVec3::Y * slope;
    let length = direction.length();
    let mut scale = || CAST_SPEED / length + triangle(rng, CAST_MODE, CAST_SPREAD);
    let velocity = DVec3::new(
        direction.x * scale(),
        direction.y * scale(),
        direction.z * scale(),
    );
    (
        eye + forward * CAST_FORWARD - DVec3::Y * CAST_DROP,
        velocity,
    )
}

/// The rotation a hook flying along `velocity` faces (as projectiles do).
fn heading(velocity: DVec3) -> Rotation {
    let horizontal = velocity.x.hypot(velocity.z);
    Rotation::new(
        velocity.x.atan2(velocity.z).to_degrees() as f32,
        velocity.y.atan2(horizontal).to_degrees() as f32,
    )
}

fn cast(world: &mut World, player: Entity, rod: &ItemStack) {
    let (Some(eye), Some(look), Some(owner_id)) = (
        world.get::<Position>(player).map(|p| p.0),
        world.get::<Rotation>(player).copied(),
        drop_thrower_id(world, player),
    ) else {
        return;
    };
    let spec = LaunchSpec {
        throw_sound: false,
        ..LaunchSpec::default()
    };
    let hook = match launch_with(world, player, ActorType::FishingHook, None, spec) {
        Ok(hook) => hook,
        Err(error) => {
            warn!(%error, "Casting a fishing hook failed");
            return;
        }
    };
    let (start, velocity) = cast_motion(
        eye,
        look,
        &mut world.get_resource_or_init::<EntityRandom>().0,
    );
    let lure = enchantment_level(rod, Enchantment::Lure);
    let luck = enchantment_level(rod, Enchantment::LuckOfTheSea);
    let mut entity = world.entity_mut(hook);
    entity.insert((
        Position(start),
        Velocity(velocity),
        heading(velocity),
        FishingHook::new(player, lure, luck),
    ));
    if let Some(mut metadata) = entity.get_mut::<EntityMetadata>() {
        metadata.set_long(ids::OWNER, owner_id);
    }
    world.entity_mut(player).insert(CastHook(hook));
}

/// Remove `hook` and forget it on its owner.
pub fn remove_hook(world: &mut World, hook: Entity) {
    let owner = world.get::<FishingHook>(hook).map(|h| h.owner);
    if let Some(owner) = owner
        && world
            .get::<CastHook>(owner)
            .is_some_and(|cast| cast.0 == hook)
    {
        world.entity_mut(owner).remove::<CastHook>();
    }
    if let Ok(entity) = world.get_entity_mut(hook) {
        entity.despawn();
    }
}

/// The owner's feet (a player's `Position` is its eye).
fn owner_feet(world: &World, owner: Entity) -> Option<DVec3> {
    let position = world.get::<Position>(owner)?.0;
    Some(if world.get::<Player>(owner).is_some() {
        position - DVec3::Y * PLAYER_EYE_HEIGHT
    } else {
        position
    })
}

/// Reel `hook` in: what happens and the rod wear it costs.
fn reel(world: &mut World, owner: Entity, hook: Entity) -> i32 {
    let (Some(state), Some(position), Some(feet)) = (
        world.get::<FishingHook>(hook).copied(),
        world.get::<Position>(hook).map(|p| p.0),
        owner_feet(world, owner),
    ) else {
        return 0;
    };
    let offset = feet - position;
    let mut wear = match state.state {
        HookState::Hooked(target) if world.get_entity(target).is_ok() => {
            pull(world, target, offset * PULL);
            WEAR_ACTOR
        }
        HookState::Bobbing if state.biting() => {
            catch(world, owner, hook, &state, position, offset);
            WEAR_CATCH
        }
        _ => 0,
    };
    if state.on_ground || world.get::<Stuck>(hook).is_some() {
        wear = WEAR_GROUND;
    }
    wear
}

/// Add `motion` to a hooked actor's velocity.
fn pull(world: &mut World, target: Entity, motion: DVec3) {
    if world.get::<Player>(target).is_some() {
        let velocity = world.get::<Velocity>(target).map_or(DVec3::ZERO, |v| v.0);
        push_player(world, target, velocity + motion);
    } else if let Some(mut velocity) = world.get_mut::<Velocity>(target) {
        velocity.0 += motion;
    }
}

/// Roll the hook's loot and fling it to `owner`, with experience.
fn catch(
    world: &mut World,
    owner: Entity,
    hook: Entity,
    state: &FishingHook,
    position: DVec3,
    offset: DVec3,
) {
    let Some(table) = world.get::<LootConfig>(hook).map(|c| Arc::clone(&c.table)) else {
        return;
    };
    let Some(table) = unastar_data::loot::get(&table) else {
        warn!(%table, "Fishing loot table is not loaded");
        return;
    };
    let Some(items) = world
        .get_resource::<ItemRegistryResource>()
        .map(|r| Arc::clone(&r.0))
    else {
        return;
    };
    let seed = world.resource_mut::<EntityRandom>().0.r#gen::<u64>();
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let ctx = LootContext {
        luck: f32::from(state.luck),
        items: Some(&items),
        ..LootContext::default()
    };
    let drops =
        match unastar_rules::loot::roll(unastar_data::loot::vanilla(), table, &ctx, &mut rng) {
            Ok(drops) => drops,
            Err(error) => {
                warn!(table = table.name, %error, "Fishing loot table cannot be evaluated");
                return;
            }
        };
    let velocity = DVec3::new(
        offset.x * PULL,
        offset.y * PULL + offset.length().sqrt().sqrt() * CATCH_LIFT,
        offset.z * PULL,
    );
    for stack in loot_stacks(world, drops) {
        if let Some(item) = crate::entity::spawn_item_drop(world, position, stack, None) {
            world.entity_mut(item).insert(Velocity(velocity));
        }
    }
    let experience = rng.gen_range(CATCH_EXPERIENCE);
    if let Some(feet) = owner_feet(world, owner) {
        crate::entity::xp_orb::spawn_experience(
            world,
            feet + DVec3::new(0.0, 0.5, 0.5),
            experience,
        );
    }
}

/// Whether `owner` still fishes with a hook at `position`.
fn still_fishing(world: &World, owner: Entity, position: DVec3) -> bool {
    if world.get::<Health>(owner).is_some_and(Health::is_dead)
        || world.get::<GameMode>(owner) == Some(&GameMode::Spectator)
    {
        return false;
    }
    let holds_rod = held(world, owner).is_some_and(|(_, stack)| stack.item_id() == FISHING_ROD);
    holds_rod
        && owner_feet(world, owner)
            .is_some_and(|feet| feet.distance(position) <= MAX_OWNER_DISTANCE)
}

/// Water surface height within the block at `block`, if it holds water.
fn water_height(world: &World, block: glam::IVec3) -> Option<f64> {
    let state = world.registry().liquid_state(world.block_at(block)?)?;
    (state.kind == LiquidKind::Water).then(|| {
        if state.depth >= 8 {
            1.0
        } else {
            1.0 - f64::from(state.depth) / 9.0
        }
    })
}

fn hook_size(world: &World, hook: Entity) -> (f64, f64) {
    world
        .get::<RawCollisionBox>(hook)
        .map_or((0.0, 0.0), |b| (f64::from(b.width), f64::from(b.height)))
}

/// The box of a mob or a player that a hook may catch.
fn actor_box(world: &World, actor: Entity) -> Option<Aabb> {
    if world.get::<Dead>(actor).is_some() {
        return None;
    }
    if world.get::<Player>(actor).is_some() {
        if world.get::<Health>(actor).is_some_and(Health::is_dead)
            || world.get::<GameMode>(actor) == Some(&GameMode::Spectator)
        {
            return None;
        }
        return Some(player_box(world.get::<Position>(actor)?.0));
    }
    let body = world.get::<PhysicsBody>(actor)?;
    Some(Aabb::from_feet(
        world.get::<Position>(actor)?.0,
        body.width,
        body.height,
    ))
}

/// Mobs and players a flying hook may catch.
pub type Catchable = (Or<(With<MobKind>, With<Player>)>, Without<Projectile>);

/// Reused per-tick buffers.
#[derive(Default)]
pub struct Scratch {
    due: Vec<(Entity, FishingHook)>,
    boxes: Vec<Aabb>,
    actors: Vec<Entity>,
}

/// The nearest mob or other player the flying hook's next segment meets
/// before any block.
fn next_catch(
    world: &mut World,
    hook: Entity,
    owner: Entity,
    actors: &mut QueryState<Entity, Catchable>,
    scratch: &mut Scratch,
) -> Option<Entity> {
    let from = world.get::<Position>(hook)?.0;
    let velocity = world.get::<Velocity>(hook)?.0;
    let to = from + velocity;
    let swept = Aabb::new(from.min(to), from.max(to)).inflate(DVec3::splat(1e-7));
    scratch.boxes.clear();
    collect_block_boxes(world, swept, &mut scratch.boxes);
    let block = scratch
        .boxes
        .iter()
        .filter_map(|b| segment_entry(b, from, velocity))
        .fold(f64::INFINITY, f64::min);
    scratch.actors.clear();
    scratch.actors.extend(
        actors
            .iter(world)
            .filter(|actor| *actor != owner && *actor != hook),
    );
    scratch
        .actors
        .iter()
        .filter_map(|actor| {
            let aabb = actor_box(world, *actor)?;
            segment_entry(&aabb, from, velocity).map(|t| (t, *actor))
        })
        .filter(|(t, _)| *t < block)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, actor)| actor)
}

/// Stop flying by the projectile rules (the fishing rules move the hook
/// from now on) and make sure it shows.
fn leave_flight(world: &mut World, hook: Entity) {
    let mut entity = world.entity_mut(hook);
    entity.remove::<ProjectileFlight>();
    let Some(mut flags) = entity.get_mut::<ActorFlags>() else {
        return;
    };
    if flags.get(actor_flags::INVISIBLE).unwrap_or(false) {
        flags
            .update(actor_flags::INVISIBLE, false)
            .expect("catalogue flag index");
        let word = flags.snapshot().words()[0];
        if let Some(mut metadata) = entity.get_mut::<EntityMetadata>() {
            metadata.set_long(ids::RESERVED_0, word as i64);
        }
    }
}

/// System (exclusive, `PhysicsSet`, after the projectiles flew): move
/// bobbing and hooked fishing hooks, run the bite cycle and remove hooks
/// whose owner stopped fishing.
pub fn tick_fishing_hooks(
    world: &mut World,
    hooks: &mut QueryState<(Entity, &FishingHook)>,
    actors: &mut QueryState<Entity, Catchable>,
    mut scratch: Local<Scratch>,
) {
    let mut due = std::mem::take(&mut scratch.due);
    due.extend(hooks.iter(world).map(|(entity, hook)| (entity, *hook)));
    for (hook, state) in due.drain(..) {
        let Some(position) = world.get::<Position>(hook).map(|p| p.0) else {
            continue;
        };
        if world.get_entity(state.owner).is_err() || !still_fishing(world, state.owner, position) {
            remove_hook(world, hook);
            continue;
        }
        let next = match state.state {
            HookState::Flying => fly(world, hook, state, position, actors, &mut scratch),
            HookState::Hooked(target) => ride(world, hook, state, target),
            HookState::Bobbing => Some(bob(world, hook, state, position)),
        };
        match next {
            Some(next) => {
                if let Some(mut stored) = world.get_mut::<FishingHook>(hook) {
                    *stored = next;
                }
            }
            None => remove_hook(world, hook),
        }
    }
    scratch.due = due;
}

/// A flying hook: lands in water, hooks an actor, or times out on the
/// ground.
fn fly(
    world: &mut World,
    hook: Entity,
    mut state: FishingHook,
    position: DVec3,
    actors: &mut QueryState<Entity, Catchable>,
    scratch: &mut Scratch,
) -> Option<FishingHook> {
    if let Some(stuck) = world.get::<Stuck>(hook) {
        let now = world.resource::<TickCounter>().current;
        return (now.saturating_sub(stuck.since) < GROUND_LIFETIME).then_some(state);
    }
    if water_height(world, position.floor().as_ivec3()).is_some() {
        leave_flight(world, hook);
        if let Some(mut velocity) = world.get_mut::<Velocity>(hook) {
            velocity.0 *= SPLASH_KEEP;
        }
        state.state = HookState::Bobbing;
    } else if let Some(target) = next_catch(world, hook, state.owner, actors, scratch) {
        leave_flight(world, hook);
        if let Some(target_id) = drop_thrower_id(world, target)
            && let Some(mut metadata) = world.get_mut::<EntityMetadata>(hook)
        {
            metadata.set_long(ids::TARGET, target_id);
        }
        state.state = HookState::Hooked(target);
        return ride(world, hook, state, target);
    }
    Some(state)
}

/// A hooked actor: the hook rides on it; it falls free once the actor is
/// gone.
fn ride(
    world: &mut World,
    hook: Entity,
    mut state: FishingHook,
    target: Entity,
) -> Option<FishingHook> {
    match actor_box(world, target) {
        Some(aabb) => {
            let position = DVec3::new(
                (aabb.min.x + aabb.max.x) / 2.0,
                aabb.min.y + (aabb.max.y - aabb.min.y) * HOOKED_HEIGHT,
                (aabb.min.z + aabb.max.z) / 2.0,
            );
            world
                .entity_mut(hook)
                .insert((Position(position), Velocity(DVec3::ZERO)));
        }
        None => state.state = HookState::Bobbing,
    }
    Some(state)
}

/// One bobbing tick: float towards the water surface (or fall), run the
/// bite cycle in water, then move with drag.
fn bob(world: &mut World, hook: Entity, mut state: FishingHook, position: DVec3) -> FishingHook {
    let block = position.floor().as_ivec3();
    let surface = water_height(world, block);
    let mut velocity = world.get::<Velocity>(hook).map_or(DVec3::ZERO, |v| v.0);
    let mut random = world.resource_mut::<EntityRandom>();
    let rng = &mut random.0;
    let mut offset = position.y + velocity.y - f64::from(block.y) - surface.unwrap_or(0.0);
    if offset.abs() < 0.01 {
        offset += offset.signum() * 0.1;
    }
    velocity = DVec3::new(
        velocity.x * BOB_DRAG,
        velocity.y - offset * f64::from(rng.r#gen::<f32>()) * BOB_PULL,
        velocity.z * BOB_DRAG,
    );
    let mut bite_started = false;
    if surface.is_some() {
        if state.biting() {
            velocity.y -= BITE_DIP * f64::from(rng.r#gen::<f32>()) * f64::from(rng.r#gen::<f32>());
        }
        let was_biting = state.biting();
        bite_cycle(&mut state, rng);
        bite_started = state.biting() && !was_biting;
    } else {
        velocity.y -= GRAVITY;
    }
    if bite_started {
        crate::entity::mob::combat::send_actor_event(
            world,
            hook,
            jolyne::valentine::types::EnumsActorEvent::FishhookHooktime,
            0,
        );
    }
    let (width, height) = hook_size(world, hook);
    let moved = move_aabb(
        world,
        Aabb::from_feet(position, width, height),
        velocity,
        0.0,
        state.on_ground,
    );
    state.on_ground = moved.on_ground;
    world
        .entity_mut(hook)
        .insert((Position(moved.aabb.feet()), Velocity(moved.moved * DRAG)));
    state
}

/// One tick of waiting for a fish in water.
fn bite_cycle(state: &mut FishingHook, rng: &mut impl Rng) {
    if state.nibble > 0 {
        state.nibble -= 1;
        if state.nibble <= 0 {
            // Missed: wait for the next fish.
            state.until_lured = 0;
            state.until_hooked = 0;
        }
    } else if state.until_hooked > 0 {
        state.until_hooked -= 1;
        if state.until_hooked <= 0 {
            state.nibble = rng.gen_range(BITE_TICKS);
        }
    } else if state.until_lured > 0 {
        state.until_lured -= 1;
        if state.until_lured <= 0 {
            state.until_hooked = rng.gen_range(APPROACH_TICKS);
        }
    } else {
        state.until_lured =
            rng.gen_range(WAIT_TICKS) - i32::from(state.lure) * LURE_TICKS_PER_LEVEL;
    }
}

#[cfg(test)]
mod tests;
