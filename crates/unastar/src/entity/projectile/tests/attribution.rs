use super::*;
use crate::entity::components::{Player, PlayerUuid};
use crate::entity::damage::DamageAttribution;

/// Trusted embedding fixture with canonical, bound session identity.
fn bind(world: &mut World, player: Entity, account: u64) -> uuid::Uuid {
    let uuid = uuid::Uuid::from_u128(account as u128);
    world.entity_mut(player).insert(PlayerUuid(uuid));
    let mut session = world.get_mut::<PlayerSession>(player).unwrap();
    session.uuid = Some(uuid.to_string());
    session.xuid = Some(account.to_string());
    uuid
}

#[test]
fn real_melee_reflection_replaces_launch_identity_with_current_deflectors_identity() {
    let (mut server, shooter, _rx) = server();
    let world = server.ecs.world_mut();
    let first = bind(world, shooter, 111);
    let projectile = launch_with(
        world,
        shooter,
        ActorType::SmallFireball,
        None,
        LaunchSpec::default(),
    )
    .unwrap();
    assert_eq!(
        world
            .get::<DamageAttribution>(projectile)
            .unwrap()
            .authenticated_owner()
            .unwrap()
            .uuid(),
        first
    );
    world.get_mut::<Position>(projectile).unwrap().0 = DVec3::new(2.5, 65.4, 4.0);
    let (tx, _rx2) = tokio::sync::mpsc::channel(64);
    let deflector = world
        .spawn((
            Player,
            Position(DVec3::new(2.5, 65.62, 2.5)),
            Rotation::default(),
            Health::default(),
            GameMode::Survival,
            crate::entity::components::MainInventory::default(),
            crate::entity::components::HeldSlot(0),
            PlayerSession::new(2, "Deflector".into(), None, None, tx),
        ))
        .id();
    let second = bind(world, deflector, 222);
    player_attack(world, deflector, projectile, &Default::default());
    let attribution = *world.get::<DamageAttribution>(projectile).unwrap();
    assert_eq!(
        world.get::<ProjectileFlight>(projectile).unwrap().owner,
        Some(deflector)
    );
    assert_eq!(attribution.authenticated_owner().unwrap().uuid(), second);
    assert_ne!(second, first);
    world.despawn(deflector);
    assert_eq!(attribution.live_owner(world), None);
    assert_eq!(attribution.authenticated_owner().unwrap().xuid(), 222);
}

#[test]
fn projectile_reflection_inherits_departed_incoming_owner_without_a_proxy_actor() {
    let (mut server, shooter, _rx) = server();
    let world = server.ecs.world_mut();
    let uuid = bind(world, shooter, 111);
    let target = fireball(world, DVec3::new(5.5, 65.0, 5.5), DVec3::Z);
    let incoming = launch_with(
        world,
        shooter,
        ActorType::Arrow,
        None,
        LaunchSpec::default(),
    )
    .unwrap();
    let config = world.get::<ProjectileConfig>(incoming).unwrap().0.clone();
    world.despawn(shooter);
    let actors = world.entities().len();
    let position = world.get::<Position>(target).unwrap().0;
    hit::on_hit(
        world,
        incoming,
        Some(shooter),
        &config,
        position,
        flight::Target::Actor(target),
        position,
    );
    let origin = *world.get::<DamageAttribution>(target).unwrap();
    assert_eq!(origin.source(), Some(target));
    assert_eq!(origin.authenticated_owner().unwrap().uuid(), uuid);
    assert_eq!(origin.owner().unwrap().entity(), shooter);
    assert_eq!(origin.live_owner(world), None);
    assert_eq!(world.entities().len(), actors);
}
