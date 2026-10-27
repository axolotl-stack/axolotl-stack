//! Spawning dropped item entities.
//!
//! [`spawn_item_drop`] is the single entry point for putting an item stack
//! into the world: block drops, container contents and player throws all go
//! through it. Physics, merging, despawn, network visibility and pickup are
//! handled by systems once the entity exists.

use bevy_ecs::prelude::*;
use glam::DVec3;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::bundles::ItemBundle;
use super::components::*;
use crate::item::ItemStack;
use crate::server::game::types::ItemEntityIdCounter;

/// Height of a player's eyes above the feet. `Position` of a player is the
/// eye position carried by `PlayerAuthInput`.
pub const PLAYER_EYE_HEIGHT: f64 = 1.62;

/// Random source for entity spawning (drop velocities and offsets).
///
/// Seedable so tests are deterministic; servers seed it from entropy.
#[derive(Resource)]
pub struct EntityRandom(pub StdRng);

impl Default for EntityRandom {
    fn default() -> Self {
        Self(StdRng::from_entropy())
    }
}

impl EntityRandom {
    pub fn seeded(seed: u64) -> Self {
        Self(StdRng::seed_from_u64(seed))
    }
}

/// Allocate a runtime/unique actor ID for a non-player entity.
pub fn next_actor_id(world: &mut World) -> i64 {
    let mut counter = world.get_resource_or_insert_with(|| ItemEntityIdCounter(100_000));
    let id = counter.0;
    counter.0 += 1;
    id
}

/// Native player throw: 0.3 along the look direction with horizontal spread.
/// The captured level throws have exactly zero vertical velocity; the
/// constructor's upward pop is replaced, not added to the throw.
fn throw_velocity(rotation: &Rotation, rng: &mut impl Rng) -> DVec3 {
    let [x, y, z] = unastar_rules::math::player_throw_direction(rotation.yaw, rotation.pitch);
    let speed = 0.3_f32;
    let mut velocity = DVec3::new(
        f64::from(x * speed),
        f64::from(y * speed),
        f64::from(z * speed),
    );
    let angle = rng.r#gen::<f64>() * std::f64::consts::TAU;
    let spread = 0.02 * rng.r#gen::<f64>();
    let (spread_sin, spread_cos) = angle.sin_cos();
    velocity.x += spread_cos * spread;
    velocity.z += spread_sin * spread;
    velocity
}

/// Initial velocity of an item that is popped into the world without a
/// thrower: `x`/`z` uniform in [-0.1, 0.1) and `y` 0.2.
fn pop_velocity(rng: &mut impl Rng) -> DVec3 {
    DVec3::new(
        rng.r#gen::<f64>() * 0.2 - 0.1,
        0.2,
        rng.r#gen::<f64>() * 0.2 - 0.1,
    )
}

/// Spawn a dropped item entity carrying `stack`.
///
/// - `thrower = None` (block drops, container contents): the item spawns at
///   `position` (feet of the 0.25 box) with the pop velocity and a 10-tick
///   pickup delay.
/// - `thrower = Some(player)`: `position` is the thrower's `Position` (eye
///   position for players); the item's feet spawn 0.5 below the eyes and it
///   flies along the thrower's `Rotation`. The ordinary pickup delay is 10;
///   an independent 40-update throw gate blocks pickup by every player.
///
/// Returns `None` for empty stacks or a source actor without its rotation
/// and network identity. The caller removes the source stack itself.
pub fn spawn_item_drop(
    world: &mut World,
    position: DVec3,
    stack: ItemStack,
    thrower: Option<Entity>,
) -> Option<Entity> {
    if stack.is_empty() {
        return None;
    }
    let source = if let Some(entity) = thrower {
        Some((
            *world.get::<Rotation>(entity)?,
            world.get::<RuntimeId>(entity)?.0,
        ))
    } else {
        None
    };
    let owner = source.map_or(-1, |(_, id)| id);
    let (feet, velocity, delay) = {
        let mut rng = world.get_resource_or_init::<EntityRandom>();
        match source {
            Some((rotation, _)) => (
                position - DVec3::new(0.0, 0.5, 0.0),
                throw_velocity(&rotation, &mut rng.0),
                PickupDelay::player_throw(),
            ),
            None => (position, pop_velocity(&mut rng.0), PickupDelay::default()),
        }
    };
    let runtime_id = next_actor_id(world);
    Some(
        world
            .spawn(ItemBundle {
                dropped_item: DroppedItem,
                stack: ItemEntityStack(stack),
                position: Position(feet),
                velocity: Velocity(velocity),
                rotation: Rotation::default(),
                on_ground: OnGround(false),
                physics: PhysicsBody::ITEM,
                runtime_id: RuntimeId(runtime_id),
                pickup_delay: delay,
                item_owner: ItemOwner(owner),
                despawn_timer: DespawnTimer::default(),
                age: Age(0),
                viewers: ActorViewers::default(),
            })
            .id(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_stacks_do_not_spawn() {
        let mut world = World::new();
        assert!(spawn_item_drop(&mut world, DVec3::ZERO, ItemStack::empty(), None).is_none());
        assert_eq!(world.entities().len(), 0);
    }

    #[test]
    fn block_drops_pop_upwards_with_a_short_pickup_delay() {
        let mut world = World::new();
        world.insert_resource(EntityRandom::seeded(7));
        let entity = spawn_item_drop(
            &mut world,
            DVec3::new(1.5, 64.0, 1.5),
            ItemStack::new("minecraft:cobblestone", 1),
            None,
        )
        .unwrap();
        let velocity = world.get::<Velocity>(entity).unwrap().0;
        assert_eq!(velocity.y, 0.2);
        assert!((-0.1..0.1).contains(&velocity.x) && (-0.1..0.1).contains(&velocity.z));
        assert_eq!(world.get::<PickupDelay>(entity).unwrap().0, 10);
        assert_eq!(
            world.get::<Position>(entity).unwrap().0,
            DVec3::new(1.5, 64.0, 1.5)
        );
    }

    fn native_drop_captures() -> &'static serde_json::Map<String, serde_json::Value> {
        static CAPTURE: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
        CAPTURE.get_or_init(|| {
            serde_json::from_str(include_str!(
                "systems/testdata/bds-1.26.51-player-drop.json"
            ))
            .unwrap()
        })["cases"]
            .as_object()
            .unwrap()
    }

    #[test]
    fn bds_hotbar_throw_has_native_offset_motion_and_pickup_gates() {
        let capture = &native_drop_captures()["hotbar_q_thrower_yaw0.0_pitch0.0_0"];
        let native = &capture["first_observation"];
        let vector = |value: &serde_json::Value| {
            DVec3::new(
                value[0].as_f64().unwrap(),
                value[1].as_f64().unwrap(),
                value[2].as_f64().unwrap(),
            )
        };
        let mut world = World::new();
        world.insert_resource(EntityRandom::seeded(1));
        let player = world.spawn((Rotation::new(0.0, 0.0), RuntimeId(1))).id();
        let eye = vector(&capture["source_eye"]);
        let entity = spawn_item_drop(
            &mut world,
            eye,
            ItemStack::new("minecraft:stone", 1),
            Some(player),
        )
        .unwrap();
        assert_eq!(
            world.get::<Position>(entity).unwrap().0,
            vector(&native["position"])
        );
        let velocity = world.get::<Velocity>(entity).unwrap().0;
        assert_eq!(
            velocity.y,
            native["velocity"][1].as_f64().unwrap(),
            "a level native throw has no vertical jitter"
        );
        assert!(velocity.x.abs() <= 0.02 && (0.28..=0.32).contains(&velocity.z));
        assert_eq!(world.get::<ItemOwner>(entity).unwrap().0, 1);
        let mut timers = world.get_mut::<PickupDelay>(entity).unwrap();
        assert_eq!(
            u64::from(timers.0),
            native["pickup_delay"].as_u64().unwrap()
        );
        // Native zero-delay controls still cannot be taken by either player
        // until the independent 40-update throw gate expires.
        timers.0 = 0;
        for _ in 0..capture["pickup_updates"].as_u64().unwrap() {
            assert!(!timers.can_pickup());
            timers.tick();
        }
        assert!(timers.can_pickup());
    }

    #[test]
    fn bds_player_drop_angles_retain_native_lookup_precision() {
        let mut world = World::new();
        world.insert_resource(EntityRandom::seeded(1));
        let player = world.spawn((Rotation::default(), RuntimeId(1))).id();
        let mut orientations = std::collections::HashSet::new();
        for (name, capture) in native_drop_captures() {
            let yaw = capture["source_yaw"].as_f64().unwrap() as f32;
            let pitch = capture["source_pitch"].as_f64().unwrap() as f32;
            if !orientations.insert((yaw.to_bits(), pitch.to_bits())) {
                continue;
            }
            *world.get_mut::<Rotation>(player).unwrap() = Rotation::new(yaw, pitch);
            let item = spawn_item_drop(
                &mut world,
                DVec3::ZERO,
                ItemStack::new("minecraft:stone", 1),
                Some(player),
            )
            .unwrap();
            let actual = world.get::<Velocity>(item).unwrap().0.y;
            let expected = capture["first_observation"]["velocity"][1]
                .as_f64()
                .unwrap();
            assert!(
                (actual - expected).abs() < 0.0000001,
                "{name}: {actual} vs native {expected}"
            );
            world.despawn(item);
        }
    }
}
