use super::*;
use crate::entity::components::PlayerUuid;
use crate::entity::damage::DamageAttribution;
use crate::entity::player_life::hooks::{PlayerDamage, PlayerDamaged};
use std::sync::Mutex;

/// Add the host ingress facts that the ordinary online runtime supplies.
fn bind(world: &mut World, owner: Entity) -> uuid::Uuid {
    let uuid = uuid::Uuid::from_u128(44);
    world.entity_mut(owner).insert(PlayerUuid(uuid));
    let mut session = world.get_mut::<PlayerSession>(owner).unwrap();
    session.uuid = Some(uuid.to_string());
    session.xuid = Some("12345".into());
    uuid
}

#[test]
fn actual_lingering_impact_and_cloud_damage_keep_departed_owner_identity() {
    let (mut server, owner) = harness();
    let world = server.ecs.world_mut();
    let uuid = bind(world, owner);
    let target = recipient(world, 7.5, 7.5);
    world.get_mut::<Health>(target).unwrap().current = 20.0;
    let effects = Arc::new(Mutex::new(Vec::<DamageAttribution>::new()));
    let losses = Arc::new(Mutex::new(Vec::<DamageAttribution>::new()));
    let observed = effects.clone();
    world.add_observer(move |event: On<EffectApplication>| {
        observed.lock().unwrap().push(*event.attribution());
    });
    let observed = losses.clone();
    world.add_observer(move |event: On<PlayerDamaged>| {
        assert!(event.hit.attacker.is_none());
        assert!(event.hit.by_player);
        observed.lock().unwrap().push(*event.attribution());
    });
    let projectile = throw(world, owner, "minecraft:lingering_potion", 24);
    world.despawn(owner);
    floor_impact(world, projectile, 7.5, 7.5);
    assert!(world.get::<Health>(target).unwrap().current < 20.0);
    let cloud = world
        .query_filtered::<Entity, With<projectile::cloud::AreaEffectCloud>>()
        .iter(world)
        .next()
        .unwrap();
    assert!(world.get_entity(projectile).is_err());
    let inherited = *world.get::<DamageAttribution>(cloud).unwrap();
    assert_eq!(inherited.source(), Some(cloud));
    assert_eq!(inherited.owner().unwrap().entity(), owner);
    assert_eq!(inherited.authenticated_owner().unwrap().uuid(), uuid);
    world.get_mut::<Health>(target).unwrap().current = 20.0;
    world
        .entity_mut(target)
        .remove::<crate::entity::mob::combat::HurtTimer>();
    world.resource_mut::<TickCounter>().current += 10;
    projectile::cloud::tick_clouds(world);
    assert!(world.get::<Health>(target).unwrap().current < 20.0);
    let losses = losses.lock().unwrap();
    assert_eq!(losses.len(), 2);
    assert_eq!(losses[0].source(), Some(projectile));
    assert_eq!(losses[1].source(), Some(cloud));
    for origin in losses.iter().chain(effects.lock().unwrap().iter()) {
        assert_eq!(origin.authenticated_owner().unwrap().uuid(), uuid);
        assert_eq!(origin.authenticated_owner().unwrap().xuid(), 12345);
    }
}

#[test]
fn stable_owner_is_available_for_real_effect_veto_before_any_damage() {
    let (mut server, owner) = harness();
    let world = server.ecs.world_mut();
    let uuid = bind(world, owner);
    let target = recipient(world, 7.5, 7.5);
    world.get_mut::<Health>(target).unwrap().current = 20.0;
    let denied = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = denied.clone();
    world.add_observer(move |mut event: On<EffectApplication>| {
        assert_eq!(
            event.attribution().authenticated_owner().unwrap().uuid(),
            uuid
        );
        count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        event.cancel();
    });
    world.add_observer(|_: On<PlayerDamage>| panic!("vetoed effect cannot attempt damage"));
    let projectile = throw(world, owner, "minecraft:splash_potion", 24);
    world.despawn(owner);
    floor_impact(world, projectile, 7.5, 7.5);
    assert_eq!(world.get::<Health>(target).unwrap().current, 20.0);
    assert_eq!(denied.load(std::sync::atomic::Ordering::Relaxed), 1);
}
