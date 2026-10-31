//! Actual target/destination authorization regressions.

use super::*;
use crate::entity::components::{
    Effects, GameMode, Health, PlayerInput, Rotation, RuntimeEntityId,
};
use crate::server::game::item_use::{self, TeleportCause};
use tokio::sync::mpsc;

/// A player with a live transport and finite authoritative position.
fn actor() -> (World, Entity, mpsc::Receiver<jolyne::valentine::McpePacket>) {
    let mut world = World::new();
    let (tx, rx) = mpsc::channel(64);
    let player = world
        .spawn((
            Position(DVec3::new(0.5, 65.62, 0.5)),
            Rotation::default(),
            RuntimeEntityId(1),
            PlayerInput::default(),
            GameMode::Survival,
            Health::default(),
            PlayerSession::new(1, "Actor".into(), Some("123".into()), None, tx),
        ))
        .id();
    (world, player, rx)
}

#[test]
fn destination_veto_and_redirect_happen_before_position_or_teleport_packets() {
    let (mut world, player, mut rx) = actor();
    let from = world.get::<Position>(player).unwrap().0;
    world.add_observer(|mut event: On<PlayerTeleport>| event.cancel());
    assert!(!item_use::teleport_from(
        &mut world,
        player,
        None,
        DVec3::new(20.0, 64.0, 20.0),
        TeleportCause::Projectile
    ));
    assert_eq!(world.get::<Position>(player).unwrap().0, from);
    assert!(rx.try_recv().is_err());
    let (mut world, player, mut rx) = actor();
    world.add_observer(|mut event: On<PlayerTeleport>| {
        event.destination = DVec3::new(4.0, 64.0, 4.0)
    });
    assert!(item_use::teleport_from(
        &mut world,
        player,
        None,
        DVec3::new(20.0, 64.0, 20.0),
        TeleportCause::Projectile
    ));
    assert_eq!(
        world.get::<Position>(player).unwrap().0,
        DVec3::new(4.0, 64.0 + crate::entity::PLAYER_EYE_HEIGHT, 4.0)
    );
    assert!(rx.try_recv().is_ok());
}

#[test]
fn actual_effect_recipient_can_be_vetoed_or_capped_and_infinite_duration_remains_supported() {
    let (mut world, player, _) = actor();
    world.add_observer(|mut event: On<EffectApplication>| {
        if event.effect() == EffectType::POISON {
            event.cancel();
        } else {
            event.amplifier = event.amplifier.min(1);
        }
    });
    assert!(!crate::server::game::effects::add_effect_from(
        &mut world,
        player,
        None,
        EffectType::POISON,
        2,
        100
    ));
    assert!(world.get::<Effects>(player).is_none());
    assert!(crate::server::game::effects::add_effect_from(
        &mut world,
        player,
        None,
        EffectType::SPEED,
        4,
        u32::MAX
    ));
    let speed = world
        .get::<Effects>(player)
        .unwrap()
        .get(EffectType::SPEED)
        .unwrap();
    assert_eq!((speed.level, speed.duration_ticks), (1, u32::MAX));
}

#[test]
fn actual_impact_veto_or_callback_removal_prevents_normal_resolution() {
    let (mut world, owner, _) = actor();
    let projectile = world.spawn(Position(DVec3::new(1.0, 64.0, 1.0))).id();
    let target = world.spawn_empty().id();
    world.add_observer(|mut event: On<ProjectileImpact>| event.cancel());
    assert!(!impact(
        &mut world,
        projectile,
        Some(owner),
        ProjectileImpactTarget::Entity(target),
        DVec3::new(2.0, 64.0, 1.0)
    ));
    let (mut world, owner, _) = actor();
    let projectile = world.spawn(Position(DVec3::new(1.0, 64.0, 1.0))).id();
    world.add_observer(|event: On<ProjectileImpact>, mut commands: Commands| {
        commands.entity(event.projectile()).despawn();
    });
    assert!(!impact(
        &mut world,
        projectile,
        Some(owner),
        ProjectileImpactTarget::Block,
        DVec3::new(2.0, 64.0, 1.0)
    ));
}

/// A replacement projectile definition selected during an impact callback.
#[derive(Resource)]
struct Replacement(super::super::ProjectileConfig);

#[test]
fn callback_owner_or_projectile_definition_swap_cannot_resolve_a_stale_impact() {
    use crate::entity::mob::{SpawnCause, spawn_mob};
    use crate::server::game::{GameServer, ServerConfig};
    for replace_config in [false, true] {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        });
        let world = server.ecs.world_mut();
        let owner = world.spawn_empty().id();
        let projectile = spawn_mob(
            world,
            "minecraft:snowball",
            DVec3::new(1.0, 17.0, 1.0),
            0.0,
            SpawnCause::Spawned,
        )
        .unwrap();
        let other = spawn_mob(
            world,
            "minecraft:arrow",
            DVec3::new(1.0, 17.0, 1.0),
            0.0,
            SpawnCause::Spawned,
        )
        .unwrap();
        let replacement = world
            .get::<super::super::ProjectileConfig>(other)
            .unwrap()
            .clone();
        world.insert_resource(Replacement(replacement));
        world
            .entity_mut(projectile)
            .insert(super::super::ProjectileFlight {
                owner: Some(owner),
                launched_tick: 0,
                critical: false,
            });
        world.add_observer(
            move |event: On<ProjectileImpact>,
                  replacement: Res<Replacement>,
                  mut flight: Query<&mut super::super::ProjectileFlight>,
                  mut config: Query<&mut super::super::ProjectileConfig>| {
                if replace_config {
                    config.get_mut(event.projectile()).unwrap().0 = Arc::clone(&replacement.0.0);
                } else {
                    flight.get_mut(event.projectile()).unwrap().owner = None;
                }
            },
        );
        assert!(!impact(
            world,
            projectile,
            Some(owner),
            ProjectileImpactTarget::Block,
            DVec3::new(2.0, 17.0, 1.0)
        ));
    }
}

#[test]
fn accepted_teleport_notifies_final_coordinates_and_immutable_identity_only_after_commit() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (mut world, player, _) = actor();
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    world.add_observer(|mut event: On<PlayerTeleport>| {
        event.destination = DVec3::new(4.0, 64.0, 4.0)
    });
    world.add_observer(
        move |event: On<PlayerTeleported>, positions: Query<&Position>| {
            seen.fetch_add(1, Ordering::SeqCst);
            assert_eq!(event.player(), player);
            assert_eq!(event.destination(), DVec3::new(4.0, 64.0, 4.0));
            assert_eq!(
                positions.get(player).unwrap().0,
                event.destination() + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT
            );
            assert_eq!(event.identity().session_id(), Some(1));
            assert_eq!(event.identity().xuid(), Some("123"));
        },
    );
    assert!(item_use::teleport_from(
        &mut world,
        player,
        None,
        DVec3::new(20.0, 64.0, 20.0),
        TeleportCause::Projectile
    ));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    world.add_observer(|mut event: On<PlayerTeleport>| event.cancel());
    assert!(!item_use::teleport_from(
        &mut world,
        player,
        None,
        DVec3::new(30.0, 64.0, 30.0),
        TeleportCause::Projectile
    ));
    assert_eq!(count.load(Ordering::SeqCst), 1);
}
