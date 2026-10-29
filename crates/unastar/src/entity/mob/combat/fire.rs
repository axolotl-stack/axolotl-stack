//! Mob fire: [`Burning`], the fire damage it deals, and
//! `minecraft:burns_in_daylight`.
//!
//! A burning mob carries [`Burning`], an absolute deadline, and the
//! `ONFIRE` actor flag (replicated with the other flags). While it burns it
//! takes the burn damage every burn interval through [`hurt_mob`] with the
//! `fire_tick` cause (so its damage sensor applies); water puts it out, and
//! a mob with the `FIRE_IMMUNE` flag never catches fire. Players burn
//! through `entity::player_life::environment` instead.
//!
//! Daylight burning, checked every [`DAYLIGHT_BURN_CHECK_TICKS`] ticks, sets
//! a mob alight for [`DAYLIGHT_BURN_TICKS`] when all of these hold, in the
//! native order: it is not burning or fire-immune; it is not in water (the
//! server has no weather, so it is never in rain); its protection slot is
//! empty; its box touches no [`DAYLIGHT_BURN_BLOCKER`]; and it sees
//! daylight: the sky is darkened by at most
//! [`DAYLIGHT_BURN_MAX_SKY_DARKEN`], the brightness of the light at
//! [`DAYLIGHT_BURN_LIGHT_HEIGHT`] of its height exceeds
//! [`DAYLIGHT_BURN_MIN_BRIGHTNESS`], and the top of its box is at or above
//! the height map of its column.

use bevy_ecs::prelude::*;
use glam::IVec3;
use jolyne::valentine::actor_flags;
use unastar_data::actor_components::body::ActorDamageCause;

use super::{Hit, hurt_mob};
use crate::ecs::resources::TickCounter;
use crate::entity::components::{PhysicsBody, Position};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::MobKind;
use crate::entity::mob::components::{BurnsInDaylightConfig, KnockbackPower};
use crate::entity::mob::constants::{
    DAYLIGHT_BURN_BLOCKER, DAYLIGHT_BURN_CHECK_TICKS, DAYLIGHT_BURN_LIGHT_HEIGHT,
    DAYLIGHT_BURN_MAX_SKY_DARKEN, DAYLIGHT_BURN_MIN_BRIGHTNESS, DAYLIGHT_BURN_TICKS,
    OVERWORLD_BRIGHTNESS_RAMP,
};
use crate::entity::mob::gear::{GearSlot, MobEquipment};
use crate::entity::mob::state::Dead;
use crate::entity::player_life::constants::{BURN_DAMAGE, BURN_INTERVAL_TICKS};
use crate::entity::systems::BlockSource;
use crate::entity::systems::physics::living_liquid;
use crate::registry::block::LiquidKind;
use crate::registry::collision::Aabb;
use crate::world::clock;
use crate::world::ecs::{ChunkData, ChunkManager, world_to_chunk_coords, world_to_local_coords};

/// A burning mob: it burns until `until_tick` and takes its next burn
/// damage at `next_burn_tick`.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct Burning {
    pub until_tick: u64,
    pub next_burn_tick: u64,
}

/// Whether `mob` is burning.
pub fn is_burning(world: &World, mob: Entity) -> bool {
    world.get::<Burning>(mob).is_some()
}

fn fire_immune(world: &World, mob: Entity) -> bool {
    world
        .get::<ActorFlags>(mob)
        .is_some_and(|flags| flags.get(actor_flags::FIRE_IMMUNE).unwrap_or(false))
}

fn set_on_fire_flag(world: &mut World, mob: Entity, on_fire: bool) {
    if let Some(mut flags) = world.get_mut::<ActorFlags>(mob) {
        flags
            .update(actor_flags::ONFIRE, on_fire)
            .expect("ONFIRE is a known actor flag");
    }
}

/// Set `mob` alight for at least `ticks` from now (a longer fire already
/// burning is kept). Fire-immune and dead mobs do not catch fire.
pub fn ignite(world: &mut World, mob: Entity, ticks: u64) {
    if ticks == 0 || world.get::<Dead>(mob).is_some() || fire_immune(world, mob) {
        return;
    }
    let now = world.resource::<TickCounter>().current;
    let burning = match world.get::<Burning>(mob).copied() {
        Some(burning) => Burning {
            until_tick: burning.until_tick.max(now + ticks),
            ..burning
        },
        None => Burning {
            until_tick: now + ticks,
            next_burn_tick: now + BURN_INTERVAL_TICKS,
        },
    };
    world.entity_mut(mob).insert(burning);
    set_on_fire_flag(world, mob, true);
}

/// Put out `mob`'s fire.
pub fn extinguish(world: &mut World, mob: Entity) {
    if world.entity_mut(mob).take::<Burning>().is_some() {
        set_on_fire_flag(world, mob, false);
    }
}

/// System (exclusive): burning mobs. Water or the deadline puts the fire
/// out; otherwise the burn damage lands every burn interval.
#[allow(clippy::type_complexity)]
pub fn burn_mobs(
    world: &mut World,
    mobs: &mut QueryState<
        (Entity, &Burning, &Position, &PhysicsBody),
        (With<MobKind>, Without<Dead>),
    >,
    mut due: Local<Vec<(Entity, Burning, Aabb)>>,
) {
    let now = world.resource::<TickCounter>().current;
    due.extend(mobs.iter(world).map(|(entity, burning, position, body)| {
        (
            entity,
            *burning,
            Aabb::from_feet(position.0, body.width, body.height),
        )
    }));
    for (mob, burning, aabb) in due.drain(..) {
        if now >= burning.until_tick || living_liquid(world, aabb) == Some(LiquidKind::Water) {
            extinguish(world, mob);
            continue;
        }
        if now < burning.next_burn_tick {
            continue;
        }
        world.entity_mut(mob).insert(Burning {
            next_burn_tick: now + BURN_INTERVAL_TICKS,
            ..burning
        });
        hurt_mob(
            world,
            mob,
            Hit {
                damage: BURN_DAMAGE,
                cause: ActorDamageCause::FireTick,
                attacker: None,
                by_player: false,
                knockback_from: None,
                knockback_power: KnockbackPower::DEFAULT,
            },
        );
    }
}

/// System (exclusive): `minecraft:burns_in_daylight`.
#[allow(clippy::type_complexity)]
pub fn burn_in_daylight(
    world: &mut World,
    mobs: &mut QueryState<
        (Entity, &BurnsInDaylightConfig, &Position, &PhysicsBody),
        (With<MobKind>, Without<Dead>, Without<Burning>),
    >,
    mut due: Local<Vec<(Entity, GearSlot, Aabb)>>,
) {
    let now = world.resource::<TickCounter>().current;
    if !now.is_multiple_of(DAYLIGHT_BURN_CHECK_TICKS) {
        return;
    }
    let darken = clock::sky_darken(world);
    if darken > DAYLIGHT_BURN_MAX_SKY_DARKEN {
        return;
    }
    due.extend(mobs.iter(world).map(|(entity, config, position, body)| {
        (
            entity,
            config.protection_slot,
            Aabb::from_feet(position.0, body.width, body.height),
        )
    }));
    let blocker = world
        .registry()
        .default_state_id_by_name(DAYLIGHT_BURN_BLOCKER)
        .and_then(|id| world.registry().get_by_runtime_id(id))
        .map(|entry| entry.id);
    for (mob, slot, aabb) in due.drain(..) {
        if fire_immune(world, mob)
            || living_liquid(world, aabb) == Some(LiquidKind::Water)
            || world
                .get::<MobEquipment>(mob)
                .is_some_and(|gear| !gear.get(slot).is_empty())
            || blocker.is_some_and(|blocker| touches_block(world, aabb, blocker))
            || !sees_daylight(world, aabb, darken)
        {
            continue;
        }
        ignite(world, mob, DAYLIGHT_BURN_TICKS);
    }
}

/// Whether any block cell the box overlaps holds a state of the block whose
/// registry id is `block`.
fn touches_block(world: &World, aabb: Aabb, block: u32) -> bool {
    let min = aabb.min.floor().as_ivec3();
    let max = aabb.max.floor().as_ivec3();
    let registry = world.registry();
    (min.x..=max.x).any(|x| {
        (min.y..=max.y).any(|y| {
            (min.z..=max.z).any(|z| {
                world
                    .block_at(IVec3::new(x, y, z))
                    .and_then(|state| registry.get_by_runtime_id(state))
                    .is_some_and(|entry| entry.id == block)
            })
        })
    })
}

/// The native daylight test for a mob with box `aabb` under a sky darkened
/// by `darken` (at most [`DAYLIGHT_BURN_MAX_SKY_DARKEN`]).
fn sees_daylight(world: &World, aabb: Aabb, darken: u8) -> bool {
    let feet = aabb.min.with_x((aabb.min.x + aabb.max.x) / 2.0);
    let feet = feet.with_z((aabb.min.z + aabb.max.z) / 2.0);
    let height = aabb.max.y - aabb.min.y;
    let light_pos = (feet + glam::DVec3::Y * height * DAYLIGHT_BURN_LIGHT_HEIGHT)
        .floor()
        .as_ivec3();
    let light = clock::sky_brightness_with_darken(world, light_pos, darken);
    if OVERWORLD_BRIGHTNESS_RAMP[usize::from(light.min(15))] <= DAYLIGHT_BURN_MIN_BRIGHTNESS {
        return false;
    }
    let top = aabb.max.y.round() as i32;
    let column = feet.floor().as_ivec3();
    column_height(world, column.x, column.z).is_none_or(|height| top >= height)
}

/// The height map of the column at `x`, `z` (the lowest y with open sky);
/// `None` when its chunk is not loaded.
fn column_height(world: &World, x: i32, z: i32) -> Option<i32> {
    let (cx, cz) = world_to_chunk_coords(x, z);
    let chunk = world
        .get_resource::<ChunkManager>()?
        .get_by_coords(cx, cz)
        .and_then(|entity| world.get::<ChunkData>(entity))?;
    let (lx, _, lz) = world_to_local_coords(x, 0, z);
    Some(i32::from(chunk.inner.height_map().at(lx, lz)))
}
