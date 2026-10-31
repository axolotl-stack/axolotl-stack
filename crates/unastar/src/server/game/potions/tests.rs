//! End-to-end item launch, actual impact and cloud lifetime regressions.

use super::*;
use crate::ecs::resources::TickCounter;
use crate::entity::components::{
    EffectType, Effects, GameMode, Health, HeldSlot, Hunger, MainInventory, Player, PlayerInput,
    PlayerSession, PlayerState, Position, Projectile, Rotation, RuntimeEntityId, Velocity,
};
use crate::entity::mob::MobKind;
use crate::entity::projectile;
use crate::entity::projectile::hooks::{EffectApplication, PlayerTeleport, ProjectileBlockChange};
use crate::server::game::item_use::{self, ItemUseQueue, ItemUsing, UseRequest};
use crate::server::game::{GameServer, ServerConfig};
use crate::world::ChunkManager;
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkPosition, ChunkViewers};
use bevy_ecs::system::RunSystemOnce;
use glam::DVec3;
use std::sync::Arc;

/// Real host registries with a loaded stone floor and one connected source actor.
fn harness() -> (GameServer, Entity) {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..Default::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(crate::entity::EntityRandom::seeded(11));
    let mut data = Chunk::new(0, 0);
    for x in 0..16 {
        for z in 0..16 {
            data.set_block(x, 63, z, *blocks::STONE);
        }
    }
    let chunk = world
        .spawn((ChunkData::new(data), ChunkPosition::new(0, 0)))
        .id();
    world
        .resource_mut::<ChunkManager>()
        .insert(ChunkPosition::new(0, 0), chunk);
    let (tx, _rx) = tokio::sync::mpsc::channel(2048);
    let owner = world
        .spawn((
            Player,
            Position(DVec3::new(
                1.5,
                64.0 + crate::entity::PLAYER_EYE_HEIGHT,
                1.5,
            )),
            Rotation::default(),
            RuntimeEntityId(1),
            GameMode::Survival,
            Health::default(),
            MainInventory::default(),
            HeldSlot(0),
            Hunger::default(),
            PlayerInput::default(),
            PlayerState::default(),
            PlayerSession::new(1, "Thrower".into(), None, None, tx),
        ))
        .id();
    let mut viewers = ChunkViewers::default();
    viewers.insert(owner);
    world.entity_mut(chunk).insert(viewers);
    (server, owner)
}

/// Install an actual registered item/aux combination into the selected slot.
fn held(world: &mut World, player: Entity, name: &str, aux: i16, count: u8) {
    let stack = world
        .resource::<crate::server::game::types::ItemRegistryResource>()
        .0
        .stack(name, count)
        .unwrap()
        .with_damage(aux);
    world
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(0, stack)
        .unwrap();
}

/// Route ordinary air-use, launch, and retain its actual spawned projectile.
fn throw(world: &mut World, player: Entity, name: &str, aux: i16) -> Entity {
    held(world, player, name, aux, 2);
    world
        .resource_mut::<ItemUseQueue>()
        .0
        .push((player, UseRequest::UseAir));
    item_use::apply_item_use(world);
    projectile::apply_throws(world);
    world
        .query_filtered::<Entity, With<Projectile>>()
        .iter(world)
        .next()
        .expect("real projectile launch")
}

/// Drive a real collision with the top of the loaded stone floor.
fn floor_impact(world: &mut World, projectile: Entity, x: f64, z: f64) {
    world.entity_mut(projectile).insert((
        Position(DVec3::new(x, 65.0, z)),
        Velocity(DVec3::new(0.0, -2.0, 0.0)),
    ));
    world.resource_mut::<TickCounter>().current += 1;
    world.run_system_once(projectile::tick_projectiles).unwrap();
}

/// A stationary, living splash/cloud recipient using player coordinate conventions.
fn recipient(world: &mut World, x: f64, z: f64) -> Entity {
    world
        .spawn((
            Player,
            Position(DVec3::new(x, 64.0 + crate::entity::PLAYER_EYE_HEIGHT, z)),
            Health::with_current(1.0, 20.0),
            GameMode::Survival,
        ))
        .id()
}

#[test]
fn projectile_and_teleport_particles_use_the_actual_source_audience() {
    use crate::entity::components::ActorAudience;
    use jolyne::valentine::McpePacketData;
    for (item, aux) in [
        ("minecraft:splash_potion", 22),
        ("minecraft:ender_pearl", 0),
    ] {
        for restricted in [false, true] {
            let (mut server, owner) = harness();
            let world = server.ecs.world_mut();
            let (tx, mut rx) = tokio::sync::mpsc::channel(256);
            let viewer = world
                .spawn(PlayerSession::new(2, "Viewer".into(), None, None, tx))
                .id();
            let chunk = world
                .resource::<ChunkManager>()
                .get_by_coords(0, 0)
                .unwrap();
            world.get_mut::<ChunkViewers>(chunk).unwrap().insert(viewer);
            let projectile = throw(world, owner, item, aux);
            while rx.try_recv().is_ok() {}
            if restricted {
                world.entity_mut(owner).insert(ActorAudience::only([]));
                world.entity_mut(projectile).insert(ActorAudience::only([]));
            }
            floor_impact(world, projectile, 7.5, 7.5);
            let effects = std::iter::from_fn(|| rx.try_recv().ok())
                .filter(|packet| {
                    matches!(
                        packet.data,
                        McpePacketData::LevelEventPacket(_)
                            | McpePacketData::LevelSoundEventPacket(_)
                    )
                })
                .count();
            assert_eq!(effects > 0, !restricted, "{item} restricted={restricted}");
        }
    }
}

#[test]
fn potion_drinking_uses_aux_effects_and_returns_a_glass_bottle_once() {
    let (mut server, owner) = harness();
    let world = server.ecs.world_mut();
    held(world, owner, "minecraft:potion", 22, 1);
    world.get_mut::<Health>(owner).unwrap().current = 1.0;
    world
        .resource_mut::<ItemUseQueue>()
        .0
        .push((owner, UseRequest::UseAir));
    item_use::apply_item_use(world);
    assert!(world.get::<ItemUsing>(owner).is_some());
    world.resource_mut::<TickCounter>().current = 40;
    world.run_system_once(item_use::tick_using).unwrap();
    assert_eq!(world.get::<Health>(owner).unwrap().current, 9.0);
    assert_eq!(
        item_use::held(world, owner).unwrap().1.item_id(),
        "minecraft:glass_bottle"
    );
    assert!(world.get::<ItemUsing>(owner).is_none());
    world.run_system_once(item_use::tick_using).unwrap();
    assert_eq!(world.get::<Health>(owner).unwrap().current, 9.0);
}

#[test]
fn pearls_teleport_at_real_floor_impact_and_only_then_apply_owner_damage() {
    let (mut server, owner) = harness();
    let world = server.ecs.world_mut();
    let projectile = throw(world, owner, "minecraft:ender_pearl", 0);
    assert_eq!(item_use::held(world, owner).unwrap().1.count, 1);
    floor_impact(world, projectile, 7.5, 7.5);
    assert!(world.get_entity(projectile).is_err());
    assert_eq!(
        world.get::<Position>(owner).unwrap().0,
        DVec3::new(7.5, 64.0 + crate::entity::PLAYER_EYE_HEIGHT, 7.5)
    );
    assert_eq!(world.get::<Health>(owner).unwrap().current, 15.0);
}

#[test]
fn pearl_destination_veto_and_unsafe_redirect_do_not_teleport_or_charge_health() {
    for mode in [0, 1, 2] {
        let (mut server, owner) = harness();
        let world = server.ecs.world_mut();
        let original = world.get::<Position>(owner).unwrap().0;
        world.add_observer(
            |_: On<crate::entity::projectile::hooks::PlayerTeleported>| {
                panic!("rejected teleport cannot notify acceptance")
            },
        );
        world.add_observer(move |mut event: On<PlayerTeleport>| {
            if mode == 0 {
                event.cancel();
            } else if mode == 1 {
                event.destination = DVec3::new(7.5, 63.0, 7.5);
            } else {
                event.destination = DVec3::new(1e30, 64.0, 1e30);
            }
        });
        let projectile = throw(world, owner, "minecraft:ender_pearl", 0);
        floor_impact(world, projectile, 7.5, 7.5);
        assert_eq!(world.get::<Position>(owner).unwrap().0, original);
        assert_eq!(world.get::<Health>(owner).unwrap().current, 20.0);
    }
}

#[test]
fn actual_splash_uses_captured_aux_and_distance_and_checks_each_recipient() {
    let (mut server, owner) = harness();
    let world = server.ecs.world_mut();
    let near = recipient(world, 7.5, 7.5);
    let half = recipient(world, 9.5, 7.5);
    let vetoed = recipient(world, 7.5, 8.5);
    let far = recipient(world, 12.5, 7.5);
    world.add_observer(move |mut event: On<EffectApplication>| {
        if event.target() == vetoed {
            event.cancel();
        }
    });
    let projectile = throw(world, owner, "minecraft:splash_potion", 22);
    let source = world.get::<ProjectilePotion>(projectile).unwrap().clone();
    assert_eq!(source.aux, 22);
    let mut catalog = PotionCatalog::default();
    catalog
        .insert(
            22,
            PotionProfile {
                effects: vec![PotionEffect {
                    effect: EffectType::POISON,
                    amplifier: 0,
                    duration_ticks: 200,
                }],
                color: 0x00ff00,
            },
        )
        .unwrap();
    world.insert_resource(catalog);
    floor_impact(world, projectile, 7.5, 7.5);
    assert_eq!(world.get::<Health>(near).unwrap().current, 9.0);
    assert!(
        (world.get::<Health>(half).unwrap().current - (1.0 + 8.0 * (1.0 - 5.0f32.sqrt() / 4.0)))
            .abs()
            < 1e-5
    );
    assert_eq!(world.get::<Health>(vetoed).unwrap().current, 1.0);
    assert_eq!(world.get::<Health>(far).unwrap().current, 1.0);
    assert!(world.get::<Effects>(near).is_none());
}

#[test]
fn directly_struck_actor_receives_full_instant_potency_and_lasting_duration() {
    let (mut server, owner) = harness();
    let world = server.ecs.world_mut();
    let target = recipient(world, 7.5, 7.5);
    let projectile = throw(world, owner, "minecraft:splash_potion", 22);
    let potion = world.get::<ProjectilePotion>(projectile).unwrap().clone();
    crate::entity::projectile::potions::splash(
        world,
        projectile,
        &potion,
        DVec3::new(7.5, 65.5, 7.5),
        Some(target),
        1.0,
    );
    assert_eq!(world.get::<Health>(target).unwrap().current, 9.0);
    let profile = PotionProfile {
        effects: vec![PotionEffect {
            effect: EffectType::SPEED,
            amplifier: 1,
            duration_ticks: 1800,
        }],
        color: 0,
    };
    let potion = ProjectilePotion {
        aux: 16,
        profile: Arc::new(profile),
    };
    crate::entity::projectile::potions::splash(
        world,
        projectile,
        &potion,
        DVec3::new(7.5, 65.5, 7.5),
        Some(target),
        1.0,
    );
    assert_eq!(
        world
            .get::<Effects>(target)
            .unwrap()
            .get(EffectType::SPEED)
            .unwrap()
            .duration_ticks,
        1800
    );
}

#[test]
fn lingering_cloud_is_networked_nonliving_and_waits_then_reapplies_with_expiry() {
    let (mut server, owner) = harness();
    let world = server.ecs.world_mut();
    let target = recipient(world, 7.5, 7.5);
    let projectile = throw(world, owner, "minecraft:lingering_potion", 22);
    floor_impact(world, projectile, 7.5, 7.5);
    let cloud = world
        .query_filtered::<Entity, With<crate::entity::projectile::cloud::AreaEffectCloud>>()
        .iter(world)
        .next()
        .unwrap();
    assert_eq!(
        &*world.get::<MobKind>(cloud).unwrap().0.identifier,
        "minecraft:area_effect_cloud"
    );
    assert!(
        world
            .get::<crate::entity::components::PhysicsBody>(cloud)
            .is_none()
    );
    assert!(
        world
            .get::<crate::entity::components::Living>(cloud)
            .is_none()
    );
    let (tx, mut rx) = tokio::sync::mpsc::channel(2048);
    world.get_mut::<PlayerSession>(owner).unwrap().outbound_tx = tx;
    world
        .run_system_once(crate::entity::mob::net::network::sync_mob_viewers)
        .unwrap();
    let packets = std::iter::from_fn(|| rx.try_recv().ok()).collect::<Vec<_>>();
    assert!(packets.iter().any(|packet| matches!(&packet.data,jolyne::valentine::McpePacketData::AddActorPacket(packet) if &*packet.actor_type=="minecraft:area_effect_cloud")));
    let chunk = world
        .resource::<ChunkManager>()
        .get_by_coords(0, 0)
        .unwrap();
    let saved = crate::entity::mob::persistence::snapshot_chunks(world, &[chunk], false);
    assert!(!saved[0].mobs.contains(&cloud));
    world.get_mut::<Health>(target).unwrap().current = 1.0;
    let start = world.resource::<TickCounter>().current;
    for age in 1..=9 {
        world.resource_mut::<TickCounter>().current = start + age;
        crate::entity::projectile::cloud::tick_clouds(world);
    }
    assert_eq!(world.get::<Health>(target).unwrap().current, 1.0);
    world.resource_mut::<TickCounter>().current = start + 10;
    crate::entity::projectile::cloud::tick_clouds(world);
    assert_eq!(world.get::<Health>(target).unwrap().current, 9.0);
    let radius = world
        .get::<crate::entity::projectile::cloud::AreaEffectCloud>(cloud)
        .unwrap()
        .radius();
    assert!(radius < 2.51 && radius > 2.4);
    for age in 11..=49 {
        world.resource_mut::<TickCounter>().current = start + age;
        crate::entity::projectile::cloud::tick_clouds(world);
    }
    assert_eq!(world.get::<Health>(target).unwrap().current, 9.0);
    world.resource_mut::<TickCounter>().current = start + 50;
    crate::entity::projectile::cloud::tick_clouds(world);
    assert_eq!(world.get::<Health>(target).unwrap().current, 17.0);
    world.resource_mut::<TickCounter>().current = start + 600;
    crate::entity::projectile::cloud::tick_clouds(world);
    assert!(world.get_entity(cloud).is_err());
}

#[test]
fn invalid_aux_does_not_launch_or_consume_and_custom_profiles_are_validated() {
    let (mut server, owner) = harness();
    let world = server.ecs.world_mut();
    held(world, owner, "minecraft:splash_potion", i16::MAX, 2);
    world
        .resource_mut::<ItemUseQueue>()
        .0
        .push((owner, UseRequest::UseAir));
    item_use::apply_item_use(world);
    projectile::apply_throws(world);
    assert_eq!(
        world
            .query_filtered::<Entity, With<Projectile>>()
            .iter(world)
            .count(),
        0
    );
    assert_eq!(item_use::held(world, owner).unwrap().1.count, 2);
    let mut catalog = PotionCatalog::empty();
    assert_eq!(
        catalog
            .insert(
                7,
                PotionProfile {
                    effects: vec![],
                    color: 0xffffffff
                }
            )
            .unwrap_err(),
        PotionProfileError::Color
    );
    assert!(catalog.get(7).is_none());
    catalog
        .insert(
            77,
            PotionProfile {
                effects: vec![PotionEffect {
                    effect: EffectType::SPEED,
                    amplifier: 2,
                    duration_ticks: 600,
                }],
                color: 0x010203,
            },
        )
        .unwrap();
    world.insert_resource(catalog);
    let projectile = throw(world, owner, "minecraft:splash_potion", 77);
    assert_eq!(
        world
            .get::<ProjectilePotion>(projectile)
            .unwrap()
            .profile
            .effects[0]
            .amplifier,
        2
    );
}

#[test]
fn water_bottle_block_effects_are_authorized_per_affected_block() {
    use crate::entity::systems::physics::BlockSource;
    let (mut server, owner) = harness();
    let world = server.ecs.world_mut();
    let registry = world
        .resource::<crate::server::game::types::BlockRegistryResource>()
        .0
        .clone();
    let fire = registry
        .get_by_name("minecraft:fire")
        .unwrap()
        .default_state_id;
    let chunk = world
        .resource::<ChunkManager>()
        .get_by_coords(0, 0)
        .unwrap();
    world
        .get_mut::<ChunkData>(chunk)
        .unwrap()
        .inner
        .set_block(7, 64, 7, fire);
    world
        .get_mut::<ChunkData>(chunk)
        .unwrap()
        .inner
        .set_block(8, 64, 7, fire);
    world.add_observer(|mut event: On<ProjectileBlockChange>| {
        if event.position().x == 7 {
            event.cancel();
        }
    });
    let projectile = throw(world, owner, "minecraft:splash_potion", 0);
    floor_impact(world, projectile, 7.5, 7.5);
    assert_eq!(world.block_at(glam::IVec3::new(7, 64, 7)), Some(fire));
    assert_eq!(
        world.block_at(glam::IVec3::new(8, 64, 7)),
        Some(*blocks::AIR)
    );
}

#[test]
fn instant_harming_uses_magic_damage_and_per_recipient_authorization() {
    let (mut server, owner) = harness();
    let world = server.ecs.world_mut();
    let target = recipient(world, 7.5, 7.5);
    world.get_mut::<Health>(target).unwrap().current = 20.0;
    let projectile = throw(world, owner, "minecraft:splash_potion", 24);
    floor_impact(world, projectile, 7.5, 7.5);
    assert_eq!(world.get::<Health>(target).unwrap().current, 8.0);
    assert_eq!(
        world
            .get::<crate::entity::mob::state::LastHurt>(target)
            .unwrap()
            .cause,
        unastar_data::actor_components::body::ActorDamageCause::Magic
    );
}

#[test]
fn effect_callback_move_rejects_stale_application() {
    let (mut server, owner) = harness();
    let world = server.ecs.world_mut();
    let target = recipient(world, 7.5, 7.5);
    world.add_observer(
        move |event: On<EffectApplication>, mut positions: Query<&mut Position>| {
            if event.target() == target {
                positions.get_mut(target).unwrap().0.x += 1.0;
            }
        },
    );
    let projectile = throw(world, owner, "minecraft:splash_potion", 22);
    floor_impact(world, projectile, 7.5, 7.5);
    assert_eq!(world.get::<Health>(target).unwrap().current, 1.0);
}

#[test]
fn pearl_spawn_group_follows_live_boolean_game_rule_without_disabling_teleport() {
    let (mut server, owner) = harness();
    let world = server.ecs.world_mut();
    world
        .resource_mut::<crate::world::rules::GameRules>()
        .do_mob_spawning = false;
    let projectile = throw(world, owner, "minecraft:ender_pearl", 0);
    assert!(
        world
            .get::<projectile::ProjectileConfig>(projectile)
            .unwrap()
            .0
            .spawn_chance
            .is_none()
    );
    floor_impact(world, projectile, 7.5, 7.5);
    assert_eq!(world.get::<Health>(owner).unwrap().current, 15.0);
}

#[test]
fn water_extinguishes_actual_burning_recipients_unless_late_vetoed() {
    let (mut server, owner) = harness();
    let world = server.ecs.world_mut();
    let target = recipient(world, 7.5, 7.5);
    let vetoed = recipient(world, 8.5, 7.5);
    for actor in [target, vetoed] {
        world
            .entity_mut(actor)
            .insert(crate::entity::player_life::environment::Burning {
                until_tick: 100,
                next_burn_tick: 20,
            });
    }
    world.add_observer(
        move |mut event: On<crate::entity::projectile::hooks::ProjectileExtinguish>| {
            if event.target() == vetoed {
                event.cancel();
            }
        },
    );
    let projectile = throw(world, owner, "minecraft:splash_potion", 0);
    floor_impact(world, projectile, 7.5, 7.5);
    assert!(
        world
            .get::<crate::entity::player_life::environment::Burning>(target)
            .is_none()
    );
    assert!(
        world
            .get::<crate::entity::player_life::environment::Burning>(vetoed)
            .is_some()
    );
}

#[test]
fn committed_teleport_callback_cannot_transfer_owner_damage_to_a_replaced_connection() {
    let (mut server, owner) = harness();
    let world = server.ecs.world_mut();
    world.add_observer(
        move |_: On<crate::entity::projectile::hooks::PlayerTeleported>,
              mut sessions: Query<&mut PlayerSession>| {
            sessions.get_mut(owner).unwrap().session_id = 2;
        },
    );
    let projectile = throw(world, owner, "minecraft:ender_pearl", 0);
    floor_impact(world, projectile, 7.5, 7.5);
    assert_eq!(world.get::<Health>(owner).unwrap().current, 20.0);
    assert_eq!(world.get::<PlayerSession>(owner).unwrap().session_id, 2);
}

#[test]
fn water_block_callback_owner_connection_replacement_rejects_stale_source() {
    use crate::entity::systems::physics::BlockSource;
    let (mut server, owner) = harness();
    let world = server.ecs.world_mut();
    let fire = world
        .resource::<crate::server::game::types::BlockRegistryResource>()
        .0
        .get_by_name("minecraft:fire")
        .unwrap()
        .default_state_id;
    let chunk = world
        .resource::<ChunkManager>()
        .get_by_coords(0, 0)
        .unwrap();
    world
        .get_mut::<ChunkData>(chunk)
        .unwrap()
        .inner
        .set_block(7, 64, 7, fire);
    world.add_observer(
        move |_: On<ProjectileBlockChange>, mut sessions: Query<&mut PlayerSession>| {
            sessions.get_mut(owner).unwrap().session_id += 1;
        },
    );
    let projectile = throw(world, owner, "minecraft:splash_potion", 0);
    floor_impact(world, projectile, 7.5, 7.5);
    assert_eq!(world.block_at(glam::IVec3::new(7, 64, 7)), Some(fire));
}

#[test]
fn checked_public_teleport_rejects_a_late_rewrite_into_solid_geometry() {
    let (mut server, owner) = harness();
    let world = server.ecs.world_mut();
    let before = world.get::<Position>(owner).unwrap().0;
    world.add_observer(|mut event: On<PlayerTeleport>| {
        event.destination = DVec3::new(7.5, 63.0, 7.5)
    });
    world.add_observer(
        |_: On<crate::entity::projectile::hooks::PlayerTeleported>| {
            panic!("rejected final predicate cannot notify acceptance")
        },
    );
    assert!(!item_use::teleport_checked_from(
        world,
        owner,
        None,
        DVec3::new(4.5, 64.0, 4.5),
        item_use::TeleportCause::Projectile,
        |world, feet| {
            assert_eq!(feet, DVec3::new(7.5, 63.0, 7.5));
            let aabb =
                crate::server::game::player_box(feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT);
            let mut boxes = Vec::new();
            crate::entity::systems::physics::collect_block_boxes(world, aabb, &mut boxes);
            !boxes.iter().any(|block| block.intersects(&aabb))
        }
    ));
    assert_eq!(world.get::<Position>(owner).unwrap().0, before);
}
