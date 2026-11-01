use super::*;
use crate::entity::damage::DamageAttribution;
use crate::entity::player_life::hooks::{PlayerDamage, PlayerDamaged, PlayerLifecycleHandler};
use crate::entity::projectile::{self, LaunchSpec, ProjectileFlight};
use bevy_ecs::system::RunSystemOnce;

#[derive(Default)]
struct Seen {
    before: Vec<(DamageAttribution, Option<Entity>)>,
    after: Vec<DamageAttribution>,
}
struct Capture(Arc<Mutex<Seen>>);
impl PlayerLifecycleHandler for Capture {
    fn on_damage(&self, _: &mut World, event: &mut PlayerDamage) {
        self.0
            .lock()
            .unwrap()
            .before
            .push((*event.attribution(), event.hit.attacker));
    }
    fn on_damaged(&self, _: &mut World, event: &PlayerDamaged) {
        self.0.lock().unwrap().after.push(*event.attribution());
    }
}

/// Launch through the native projectile path, then remove/migrate its real owner.
async fn departed_shooter(migrate: bool) {
    let mut router = router();
    let (owner, _owner_rx) = spawn(&mut router, 0, 1);
    let (victim, _victim_rx) = spawn(&mut router, 0, 2);
    let seen = Arc::new(Mutex::new(Seen::default()));
    let source_world;
    let owner_uuid;
    let projectile;
    {
        let world = router.world_mut(0).unwrap().ecs.world_mut();
        world
            .resource_mut::<crate::world::rules::GameRules>()
            .do_mob_spawning = false;
        world.get_or_create_chunk(0, 0);
        source_world = world.id();
        owner_uuid = world.get::<PlayerUuid>(owner).unwrap().0;
        world.get_mut::<PlayerSession>(owner).unwrap().xuid = Some("12345".into());
        world.get_mut::<Position>(victim).unwrap().0 = DVec3::new(4.5, 17.62, 8.5);
        world.get_mut::<Rotation>(owner).unwrap().yaw = 0.0;
        world
            .get_resource_or_init::<crate::entity::player_life::hooks::PlayerLifecycleHooks>()
            .register(Capture(seen.clone()));
        projectile = projectile::launch_with(
            world,
            owner,
            unastar_data::actor_identifiers::ActorType::Arrow,
            None,
            LaunchSpec {
                speed: Some(1.0),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            world
                .get::<DamageAttribution>(projectile)
                .unwrap()
                .authenticated_owner()
                .unwrap()
                .uuid(),
            owner_uuid
        );
    }
    if migrate {
        request(&mut router, owner, 1);
        router.tick().await.unwrap();
        assert_eq!(router.locate_session(1).unwrap().0, 1);
    } else {
        router.despawn_player(1);
    }
    let world = router.world_mut(0).unwrap().ecs.world_mut();
    assert!(world.get_entity(owner).is_err());
    assert_eq!(
        world.get::<ProjectileFlight>(projectile).unwrap().owner,
        Some(owner)
    );
    for _ in 0..12 {
        world.resource_mut::<crate::ecs::TickCounter>().current += 1;
        world.run_system_once(projectile::tick_projectiles).unwrap();
        if !seen.lock().unwrap().after.is_empty() {
            break;
        }
    }
    assert!(world.get::<Health>(victim).unwrap().current < 20.0);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.before.len(), 1);
    assert_eq!(seen.after.len(), 1);
    assert!(
        seen.before[0].1.is_none(),
        "departed owner cannot be a live attacker"
    );
    let attribution = seen.after[0];
    assert_eq!(seen.before[0].0, attribution);
    assert_eq!(attribution.world_id(), source_world);
    assert_eq!(attribution.dimension(), Some(0));
    assert_eq!(attribution.source(), Some(projectile));
    let identity = attribution.authenticated_owner().unwrap();
    assert_eq!(identity.uuid(), owner_uuid);
    assert_eq!(identity.xuid(), 12345);
    assert_eq!(attribution.owner().unwrap().entity(), owner);
    assert_eq!(attribution.owner().unwrap().session_id(), Some(1));
    assert_eq!(
        attribution.owner().unwrap().feet_position(),
        Some(DVec3::new(4.5, 16.0, 4.5))
    );
}

#[tokio::test]
async fn launched_arrow_retains_authenticated_owner_after_actual_native_world_transfer() {
    departed_shooter(true).await;
}

#[tokio::test]
async fn launched_arrow_retains_authenticated_owner_after_native_disconnect_despawn() {
    departed_shooter(false).await;
}
