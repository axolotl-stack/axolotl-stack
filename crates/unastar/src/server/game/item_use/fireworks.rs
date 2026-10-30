//! Firework rockets.
//!
//! A rocket used on a block launches a `minecraft:fireworks_rocket` actor
//! from the clicked face; one used in the air while gliding launches one
//! attached to the player. Either uses up one rocket outside creative and
//! plays `launch` at the rocket.
//!
//! Measured on BDS 1.26.51 (client-diff `itemuses-elytra-2`, and the
//! summon `operator/summons/fireworks_rocket.bin`): the rocket's actor data
//! carries the item's NBT compound (`{Fireworks: {Explosions, Flight}}`,
//! empty for a summon) in `DISPLAY_TILE_RUNTIME_ID` (16), its direction in
//! `DISPLAY_OFFSET` (17: `(0, 1, 0)` free, `(0, 0, 0)` attached) and the
//! attached player's actor ID in `CUSTOM_DISPLAY` (18, -1 free). A rocket
//! starts at a ±0.001 gaussian per axis plus `direction × 0.03` blocks per
//! tick and each tick its velocity becomes `v × 1.05 + direction × 0.03`
//! (the measured climb). The boosted player's client moves itself: BDS
//! sends the player no motion, so an attached rocket only stays at its
//! player. On exploding the rocket's viewers get `ActorEvent`
//! `FireworksExplode` and the rocket is removed.
//!
//! Public references (unmeasured): a rocket lives `10 × (Flight + 1) +
//! rand(0..6) + rand(0..7)` ticks (the measured 31 and 23 ticks of Flight 1
//! fit), a free rocket explodes early when a block stops it, each explosion
//! in the item plays `large.blast` (type 1) or `blast` and, when it
//! flickers, `twinkle`. Explosions do not hurt: rocket damage is not
//! implemented.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_data_i_ds as ids;
use jolyne::valentine::types::{
    DataItemCompoundTagPayload, DataItemEntryPayload, DataItemVec3Payload, EnumsActorEvent,
    EnumsDataItemType, Vec3,
};
use rand::Rng;
use tracing::warn;
use zuri_nbt::encoding::NetworkLittleEndian;
use zuri_nbt::{NBTTag, tag};

use super::elytra::Gliding;
use crate::entity::components::{GameMode, Position, RuntimeEntityId, Velocity};
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::item::ItemStack;
use crate::item::enchant::item_tag;
use crate::registry::collision::Aabb;

pub const FIREWORK: &str = "minecraft:firework_rocket";
pub const ROCKET: &str = "minecraft:fireworks_rocket";
/// Rocket box (definition `minecraft:collision_box`).
const ROCKET_SIZE: f64 = 0.25;

/// A flying rocket: the server tick it explodes at and the player it is
/// attached to.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Rocket {
    pub explode_at: u64,
    pub attached: Option<Entity>,
    /// The rocket item (its explosions play their sounds).
    pub item: ItemStack,
}

fn now(world: &World) -> u64 {
    world
        .get_resource::<crate::ecs::resources::TickCounter>()
        .map_or(0, |t| t.current)
}

/// The item's `Fireworks` compound.
fn fireworks(stack: &ItemStack) -> Option<tag::Compound> {
    match item_tag(stack)?.0.get("Fireworks") {
        Some(NBTTag::Compound(fireworks)) => Some(fireworks.clone()),
        _ => None,
    }
}

/// The item's flight duration (`Fireworks.Flight`, 0 when absent).
pub fn flight(stack: &ItemStack) -> i8 {
    match fireworks(stack).and_then(|f| f.0.get("Flight").cloned()) {
        Some(NBTTag::Byte(flight)) => flight.0 as i8,
        _ => 0,
    }
}

/// Ticks a rocket of `flight` lives.
pub fn lifetime(flight: i8, rng: &mut impl Rng) -> u64 {
    let base = 10 * (i64::from(flight) + 1);
    (base + rng.gen_range(0..6) + rng.gen_range(0..7)).max(0) as u64
}

/// The item compound a rocket carries in its actor data: the item's NBT.
fn item_compound(stack: &ItemStack) -> tag::Compound {
    item_tag(stack).unwrap_or_default()
}

fn network_nbt(compound: tag::Compound) -> jolyne::valentine::bedrock::codec::Nbt {
    let mut bytes = Vec::new();
    NBTTag::Compound(compound)
        .write(&mut bytes, NetworkLittleEndian)
        .expect("writing NBT into a Vec cannot fail");
    jolyne::valentine::bedrock::codec::Nbt(bytes.into())
}

/// Set the rocket rows of a rocket's actor data: `item` (`None` for an
/// empty compound, as a summoned rocket has), the attached actor ID (-1
/// free) and the matching direction.
pub fn set_rocket_data(metadata: &mut EntityMetadata, item: Option<&ItemStack>, attached: i64) {
    let compound = item.map(item_compound).unwrap_or_default();
    metadata.set_payload(
        ids::DISPLAY_TILE_RUNTIME_ID,
        DataItemEntryPayload::DataItemCompoundTagPayload(DataItemCompoundTagPayload {
            type_: EnumsDataItemType::Compoundtag,
            value: network_nbt(compound),
        }),
    );
    let direction = direction(attached != -1);
    metadata.set_payload(
        ids::DISPLAY_OFFSET,
        DataItemEntryPayload::DataItemVec3Payload(DataItemVec3Payload {
            type_: EnumsDataItemType::Vec3,
            value: Vec3 {
                x: direction.x as f32,
                y: direction.y as f32,
                z: direction.z as f32,
            },
        }),
    );
    metadata.set_long(ids::CUSTOM_DISPLAY, attached);
}

/// The direction a rocket accelerates in: up, or none when attached.
fn direction(attached: bool) -> DVec3 {
    if attached { DVec3::ZERO } else { DVec3::Y }
}

/// Per-tick acceleration along the direction (measured).
const THRUST: f64 = 0.03;
/// Per-tick velocity growth (measured).
const GROWTH: f64 = 1.05;

/// Spawn a rocket of `item` at `feet`, attached to `player` or flying free.
pub fn launch(
    world: &mut World,
    feet: DVec3,
    item: &ItemStack,
    attached: Option<Entity>,
) -> Option<Entity> {
    let rocket = match spawn_mob(world, ROCKET, feet, 0.0, SpawnCause::Spawned) {
        Ok(rocket) => rocket,
        Err(error) => {
            warn!(%error, "Launching a firework rocket failed");
            return None;
        }
    };
    let attached_id = attached
        .and_then(|player| world.get::<RuntimeEntityId>(player).map(|r| r.0))
        .unwrap_or(-1);
    let (lifetime, velocity) = {
        let rng = &mut world
            .get_resource_or_init::<crate::entity::EntityRandom>()
            .0;
        let lifetime = lifetime(flight(item), rng);
        let velocity = DVec3::new(gaussian(rng), gaussian(rng), gaussian(rng)) * 0.001
            + direction(attached.is_some()) * THRUST;
        (lifetime, velocity)
    };
    let explode_at = now(world) + lifetime;
    let mut entity = world.entity_mut(rocket);
    if let Some(mut metadata) = entity.get_mut::<EntityMetadata>() {
        set_rocket_data(&mut metadata, Some(item), attached_id);
    }
    entity.insert((
        Velocity(velocity),
        Rocket {
            explode_at,
            attached,
            item: item.clone(),
        },
    ));
    crate::entity::mob::net::sound::play_level_sound(world, Some(rocket), feet, "launch", -1);
    Some(rocket)
}

/// A standard normal sample (Box-Muller).
fn gaussian(rng: &mut impl Rng) -> f64 {
    let u: f64 = rng.r#gen::<f64>().max(f64::MIN_POSITIVE);
    let v: f64 = rng.r#gen();
    (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
}

/// Use up one held rocket outside creative.
fn use_one(world: &mut World, player: Entity, slot: u8, stack: &ItemStack) {
    if world.get::<GameMode>(player) != Some(&GameMode::Creative) {
        super::set_slot(world, player, slot, stack.grow(-1));
    }
}

/// An air use of a rocket: boosts a gliding player, nothing otherwise.
pub fn use_in_air(world: &mut World, player: Entity, slot: u8, stack: &ItemStack) {
    if world.get::<Gliding>(player).is_none() {
        return;
    }
    let Some(eye) = world.get::<Position>(player).map(|p| p.0) else {
        return;
    };
    let feet = eye - DVec3::new(0.0, crate::entity::item_drop::PLAYER_EYE_HEIGHT, 0.0);
    if launch(world, feet, stack, Some(player)).is_some() {
        use_one(world, player, slot, stack);
    }
}

/// A rocket used on a block: launched from `at` (the clicked face).
pub fn use_on_block(world: &mut World, player: Entity, slot: u8, stack: &ItemStack, at: DVec3) {
    if launch(world, at, stack, None).is_some() {
        use_one(world, player, slot, stack);
    }
}

/// System (exclusive): fly rockets, keep attached ones at their players and
/// explode rockets whose time is up.
pub fn tick_rockets(
    world: &mut World,
    rockets: &mut QueryState<(Entity, &Rocket)>,
    mut due: Local<Vec<(Entity, Option<Entity>, u64)>>,
) {
    due.extend(
        rockets
            .iter(world)
            .map(|(rocket, flight)| (rocket, flight.attached, flight.explode_at)),
    );
    let now = now(world);
    for (rocket, attached, explode_at) in due.drain(..) {
        let stopped = match attached {
            Some(player) => {
                follow(world, rocket, player);
                false
            }
            None => fly(world, rocket),
        };
        if stopped || now >= explode_at {
            explode(world, rocket);
        }
    }
}

/// Keep an attached rocket at its player (whose client applies the boost).
fn follow(world: &mut World, rocket: Entity, player: Entity) {
    let Some(eye) = world.get::<Position>(player).map(|p| p.0) else {
        return;
    };
    let feet = eye - DVec3::new(0.0, crate::entity::item_drop::PLAYER_EYE_HEIGHT, 0.0);
    if let Some(mut position) = world.get_mut::<Position>(rocket) {
        position.0 = feet;
    }
}

/// Move a free rocket one tick (its velocity grows first, as measured);
/// returns whether a block stopped it.
fn fly(world: &mut World, rocket: Entity) -> bool {
    let (Some(feet), Some(velocity)) = (
        world.get::<Position>(rocket).map(|p| p.0),
        world.get::<Velocity>(rocket).map(|v| v.0),
    ) else {
        return false;
    };
    let velocity = velocity * GROWTH + direction(false) * THRUST;
    let moved = crate::entity::systems::physics::move_aabb(
        world,
        Aabb::from_feet(feet, ROCKET_SIZE, ROCKET_SIZE),
        velocity,
        0.0,
        false,
    );
    let stopped = moved.moved.distance_squared(velocity) > 1.0e-9;
    world
        .entity_mut(rocket)
        .insert((Position(moved.aabb.feet()), Velocity(velocity)));
    stopped
}

/// The sounds a rocket's explosions play.
pub fn explosion_sounds(stack: &ItemStack) -> Vec<&'static str> {
    let explosions = match fireworks(stack).and_then(|f| f.0.get("Explosions").cloned()) {
        Some(NBTTag::List(list)) => list.0,
        _ => Vec::new(),
    };
    let byte = |explosion: &tag::Compound, key: &str| match explosion.0.get(key) {
        Some(NBTTag::Byte(value)) => value.0,
        _ => 0,
    };
    let mut sounds = Vec::new();
    for explosion in explosions {
        let NBTTag::Compound(explosion) = explosion else {
            continue;
        };
        sounds.push(if byte(&explosion, "FireworkType") == 1 {
            "large.blast"
        } else {
            "blast"
        });
        if byte(&explosion, "FireworkFlicker") != 0 {
            sounds.push("twinkle");
        }
    }
    sounds
}

fn explode(world: &mut World, rocket: Entity) {
    crate::entity::mob::combat::send_actor_event(
        world,
        rocket,
        EnumsActorEvent::FireworksExplode,
        0,
    );
    if let (Some(at), Some(item)) = (
        world.get::<Position>(rocket).map(|p| p.0),
        world.get::<Rocket>(rocket).map(|r| r.item.clone()),
    ) {
        for sound in explosion_sounds(&item) {
            crate::entity::mob::net::sound::play_level_sound(world, Some(rocket), at, sound, -1);
        }
    }
    world.despawn(rocket);
}
