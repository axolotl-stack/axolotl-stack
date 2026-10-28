//! Entity lifecycle systems.

use bevy_ecs::prelude::*;
use glam::DVec3;

use crate::entity::components::*;
use crate::registry::collision::Aabb;

/// System: Tick entity age.
pub fn tick_age(mut query: Query<&mut Age>) {
    for mut age in query.iter_mut() {
        age.tick();
    }
}

/// System: Despawn dead entities.
pub fn despawn_dead(mut commands: Commands, query: Query<(Entity, &Health), With<Living>>) {
    for (entity, health) in query.iter() {
        if health.is_dead() {
            // TODO: Send death event, play death animation, drop items
            commands.entity(entity).despawn();
        }
    }
}

/// System: Tick item pickup delay.
pub fn tick_item_pickup_delay(mut query: Query<&mut PickupDelay, With<DroppedItem>>) {
    for mut delay in query.iter_mut() {
        delay.tick();
    }
}

/// System: Tick item despawn timer.
pub fn tick_item_despawn(
    mut commands: Commands,
    mut query: Query<(Entity, &mut DespawnTimer), With<DroppedItem>>,
) {
    for (entity, mut timer) in query.iter_mut() {
        timer.tick();
        if timer.should_despawn() {
            commands.entity(entity).despawn();
        }
    }
}

/// How often (in ticks of age) an item looks for neighbours to merge with.
pub const ITEM_MERGE_INTERVAL: u64 = 25;

/// System: merge nearby identical item stacks.
///
/// Every 25 ticks of age an item searches the items around it (its box grown
/// by 0.5 horizontally); for each other item with the same item, aux value
/// and user data whose pickup delay is not "never", the smaller stack merges
/// into the larger one if their sum fits a stack. The survivor keeps the
/// larger pickup delay and the longer remaining lifetime; the absorbed item
/// is removed and viewers see the new count (`ActorEvent` stack-size
/// update). Behaviour to be confirmed by a BDS capture (see the entities
/// research doc).
#[allow(clippy::type_complexity)]
pub fn merge_item_entities(
    mut commands: Commands,
    mut items: Query<
        (
            Entity,
            &Position,
            &mut ItemEntityStack,
            &mut PickupDelay,
            &mut DespawnTimer,
            &Age,
        ),
        With<DroppedItem>,
    >,
) {
    let searchers: Vec<(Entity, DVec3)> = items
        .iter()
        .filter(|(_, _, _, delay, _, age)| {
            age.0 > 0 && age.0 % ITEM_MERGE_INTERVAL == 0 && !delay.is_never()
        })
        .map(|(entity, position, ..)| (entity, position.0))
        .collect();
    let mut removed: Vec<Entity> = Vec::new();
    for (searcher, position) in searchers {
        if removed.contains(&searcher) {
            continue;
        }
        let area = Aabb::from_feet(position, PhysicsBody::ITEM.width, PhysicsBody::ITEM.height)
            .inflate(DVec3::new(0.5, 0.0, 0.5));
        let neighbours: Vec<Entity> = items
            .iter()
            .filter(|(other, other_pos, _, delay, _, _)| {
                *other != searcher
                    && !removed.contains(other)
                    && !delay.is_never()
                    && Aabb::from_feet(
                        other_pos.0,
                        PhysicsBody::ITEM.width,
                        PhysicsBody::ITEM.height,
                    )
                    .intersects(&area)
            })
            .map(|(other, ..)| other)
            .collect();
        for other in neighbours {
            if removed.contains(&searcher) {
                break;
            }
            let Ok([a, b]) = items.get_many_mut([searcher, other]) else {
                continue;
            };
            let (mut keep, mut absorb) = if a.2.0.count >= b.2.0.count {
                (a, b)
            } else {
                (b, a)
            };
            let (keep_stack, absorb_stack) = (&keep.2.0, &absorb.2.0);
            let total = u16::from(keep_stack.count) + u16::from(absorb_stack.count);
            if !keep_stack.comparable(absorb_stack)
                || keep_stack.runtime_id != absorb_stack.runtime_id
                || total > u16::from(keep_stack.max_stack_size())
            {
                continue;
            }
            keep.2.0.count = total as u8;
            keep.3.merge(*absorb.3);
            keep.4.ticks_remaining = keep.4.ticks_remaining.max(absorb.4.ticks_remaining);
            absorb.2.0.count = 0;
            let absorbed = absorb.0;
            commands.entity(absorbed).despawn();
            removed.push(absorbed);
        }
    }
}

/// Entities this far below the world floor are removed (64 blocks under the
/// minimum build height, as dragonfly does).
pub const OUT_OF_WORLD_DEPTH: f64 = 64.0;

/// System: remove physics bodies that fell out of the world.
pub fn despawn_out_of_world(
    mut commands: Commands,
    query: Query<(Entity, &Position), With<PhysicsBody>>,
) {
    let floor = f64::from(crate::world::chunk::MIN_Y) - OUT_OF_WORLD_DEPTH;
    for (entity, position) in query.iter() {
        if position.0.y < floor {
            commands.entity(entity).despawn();
        }
    }
}

/// System: Tick projectile age and despawn old projectiles.
pub fn tick_projectile_lifetime(
    mut commands: Commands,
    query: Query<(Entity, &Age, &ProjectileHit), With<Projectile>>,
) {
    const MAX_PROJECTILE_AGE: u64 = 1200; // 1 minute
    const MAX_STUCK_TICKS: u32 = 1200; // 1 minute stuck in block

    for (entity, age, hit) in query.iter() {
        // Despawn if too old or stuck too long
        if age.0 > MAX_PROJECTILE_AGE || hit.stuck_ticks > MAX_STUCK_TICKS {
            commands.entity(entity).despawn();
        }
    }
}

/// System: Tick mob age (for baby growth).
pub fn tick_mob_age(mut query: Query<&mut MobAge, With<Mob>>) {
    for mut mob_age in query.iter_mut() {
        mob_age.tick();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{EntityRandom, spawn_item_drop};

    fn item(world: &mut World, x: f64, id: &str, count: u8, age: u64) -> Entity {
        let entity = spawn_item_drop(
            world,
            DVec3::new(x, 64.0, 0.5),
            crate::item::test_support::stack(id, count),
            None,
        )
        .unwrap();
        world.get_mut::<Age>(entity).unwrap().0 = age;
        entity
    }

    fn run_merge(world: &mut World) {
        let mut schedule = Schedule::default();
        schedule.add_systems(merge_item_entities);
        schedule.run(world);
    }

    #[test]
    fn nearby_identical_stacks_merge_into_the_larger_one_every_25_ticks() {
        let mut world = World::new();
        world.insert_resource(EntityRandom::seeded(1));
        let small = item(&mut world, 0.5, "minecraft:dirt", 3, 25);
        let large = item(&mut world, 1.2, "minecraft:dirt", 10, 7);
        world.get_mut::<PickupDelay>(small).unwrap().0 = 30;
        world
            .get_mut::<DespawnTimer>(small)
            .unwrap()
            .ticks_remaining = 5900;
        world
            .get_mut::<DespawnTimer>(large)
            .unwrap()
            .ticks_remaining = 100;

        run_merge(&mut world);

        assert!(
            world.get_entity(small).is_err(),
            "the smaller stack is absorbed"
        );
        assert_eq!(world.get::<ItemEntityStack>(large).unwrap().0.count, 13);
        assert_eq!(world.get::<PickupDelay>(large).unwrap().0, 30);
        assert_eq!(
            world.get::<DespawnTimer>(large).unwrap().ticks_remaining,
            5900
        );
    }

    #[test]
    fn merging_respects_cadence_distance_item_and_stack_limit() {
        let mut world = World::new();
        world.insert_resource(EntityRandom::seeded(1));
        let off_cadence = item(&mut world, 0.5, "minecraft:dirt", 1, 24);
        let _neighbour = item(&mut world, 0.6, "minecraft:dirt", 1, 3);
        run_merge(&mut world);
        assert!(world.get_entity(off_cadence).is_ok());

        let mut world = World::new();
        let a = item(&mut world, 0.5, "minecraft:dirt", 1, 25);
        let far = item(&mut world, 2.0, "minecraft:dirt", 1, 3);
        let other = item(&mut world, 0.6, "minecraft:stone", 1, 3);
        run_merge(&mut world);
        assert!(world.get_entity(far).is_ok() && world.get_entity(other).is_ok());
        assert_eq!(world.get::<ItemEntityStack>(a).unwrap().0.count, 1);

        let mut world = World::new();
        let a = item(&mut world, 0.5, "minecraft:dirt", 40, 25);
        let b = item(&mut world, 0.6, "minecraft:dirt", 30, 3);
        run_merge(&mut world);
        assert!(world.get_entity(a).is_ok() && world.get_entity(b).is_ok());

        let mut world = World::new();
        let a = item(&mut world, 0.5, "minecraft:dirt", 1, 25);
        let never = item(&mut world, 0.6, "minecraft:dirt", 1, 3);
        world.get_mut::<PickupDelay>(never).unwrap().0 = PickupDelay::NEVER;
        run_merge(&mut world);
        assert!(world.get_entity(a).is_ok() && world.get_entity(never).is_ok());
    }

    #[test]
    fn items_despawn_after_five_minutes_and_below_the_world() {
        let mut world = World::new();
        let old = item(&mut world, 0.5, "minecraft:dirt", 1, 0);
        world.get_mut::<DespawnTimer>(old).unwrap().ticks_remaining = 1;
        let fresh = item(&mut world, 0.5, "minecraft:dirt", 1, 0);
        let fallen = item(&mut world, 5.0, "minecraft:dirt", 1, 0);
        world.get_mut::<Position>(fallen).unwrap().0.y = -128.5;
        let mut schedule = Schedule::default();
        schedule.add_systems((tick_item_despawn, despawn_out_of_world));
        schedule.run(&mut world);
        assert!(world.get_entity(old).is_err());
        assert!(world.get_entity(fallen).is_err());
        assert!(world.get_entity(fresh).is_ok());
        assert_eq!(
            world.get::<DespawnTimer>(fresh).unwrap().ticks_remaining,
            DespawnTimer::ITEM_LIFETIME - 1
        );
    }
}
