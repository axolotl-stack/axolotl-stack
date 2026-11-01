use super::*;
use crate::ecs::resources::TickCounter;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::player_life::hooks::PlayerDamage;
use crate::entity::projectile::hooks::ProjectileImpact;
use crate::item::enchant::with_enchantments;
use crate::server::game::PlayerSpawnData;

/// Equip a real bow stack before the ordinary draw/release input path.
fn equip(r: &mut Range, power: u8, flame: bool) {
    let bow = with_enchantments(
        &r.slot(0),
        &[
            (Enchantment::Power, power),
            (Enchantment::Flame, u8::from(flame)),
        ],
    );
    let player = r.player;
    r.world()
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(0, bow)
        .unwrap();
}

/// Fully draw and release using the same input queue as ordinary gameplay.
fn shoot(r: &mut Range) -> Entity {
    r.send(UseRequest::UseAir);
    r.ticks(24);
    r.send(UseRequest::Release);
    let world = r.world();
    let mut query =
        world.query_filtered::<Entity, With<crate::entity::projectile::ProjectileFlight>>();
    query.single(world).unwrap()
}

/// A complete survival target with enough health to inspect the accepted hit.
fn player_target(r: &mut Range) -> Entity {
    let (tx, rx) = tokio::sync::mpsc::channel(65536);
    let target = r
        .server
        .spawn_player(PlayerSpawnData {
            session_id: 2,
            runtime_id: 2,
            display_name: "Target".into(),
            xuid: None,
            uuid: None,
            position: DVec3::new(4.5, 64.0 + crate::entity::PLAYER_EYE_HEIGHT, 12.5),
            outbound_tx: tx,
            saved_data: None,
            chunk_radius: 0,
        })
        .unwrap();
    // Keep the target transport alive for the native session identity checks.
    r.world().insert_resource(TargetTransport { _receiver: rx });
    r.world()
        .entity_mut(target)
        .insert((GameMode::Survival, Health::new(100.0)));
    target
}

#[derive(Resource)]
struct TargetTransport {
    _receiver: tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>,
}

#[derive(Resource, Default)]
struct Seen {
    impacts: usize,
    damage: usize,
}

#[test]
fn full_draw_power_damage_survives_a_weapon_switch_and_departed_owner() {
    for depart in [false, true] {
        let mut losses = Vec::new();
        for power in [0, 3, 4] {
            let mut r = range(&[("minecraft:bow", 1), ("minecraft:arrow", 8)]);
            equip(&mut r, power, false);
            let target = r.cow(DVec3::new(4.5, 64.0, 12.5));
            r.world().entity_mut(target).insert(Health::new(100.0));
            let arrow = shoot(&mut r);
            assert_eq!(
                crate::entity::projectile::enchantments::power_bonus(r.world(), arrow),
                if power == 0 {
                    0.0
                } else {
                    (f32::from(power) + 1.0) * 0.5
                }
            );
            let player = r.player;
            r.world().get_mut::<HeldSlot>(player).unwrap().0 = 1;
            if depart {
                r.world().despawn(player);
            }
            r.ticks(5);
            let loss = 100.0 - r.world().get::<Health>(target).unwrap().current;
            assert!(loss > 0.0, "the actual flight must hit: {power}, {depart}");
            losses.push(loss);
        }
        assert!(losses[1] > losses[0], "Power III: {losses:?}");
        assert!(losses[2] > losses[0], "Power IV: {losses:?}");
    }
}

#[test]
fn full_draw_flame_ignites_players_and_mobs_from_the_captured_bow() {
    for player_targeted in [false, true] {
        let mut r = range(&[("minecraft:bow", 1), ("minecraft:arrow", 8)]);
        equip(&mut r, 0, true);
        let target = if player_targeted {
            player_target(&mut r)
        } else {
            let target = r.cow(DVec3::new(4.5, 64.0, 12.5));
            r.world().entity_mut(target).insert(Health::new(100.0));
            target
        };
        let arrow = shoot(&mut r);
        assert!(
            r.world()
                .get::<ActorFlags>(arrow)
                .unwrap()
                .get(jolyne::valentine::actor_flags::ONFIRE)
                .unwrap()
        );
        let player = r.player;
        r.world().get_mut::<HeldSlot>(player).unwrap().0 = 1;
        if player_targeted {
            r.world().despawn(player);
        }
        r.ticks(5);
        let world = r.world();
        assert!(world.get::<Health>(target).unwrap().current < 100.0);
        let deadline = if player_targeted {
            world
                .get::<crate::entity::player_life::environment::Burning>(target)
                .unwrap()
                .until_tick
        } else {
            world
                .get::<crate::entity::mob::combat::fire::Burning>(target)
                .unwrap()
                .until_tick
        };
        let remaining = deadline - world.resource::<TickCounter>().current;
        assert!((95..=100).contains(&remaining));
    }
}

#[test]
fn late_impact_and_damage_vetoes_never_ignite_or_remove_target_health() {
    for impact_veto in [false, true] {
        let mut r = range(&[("minecraft:bow", 1), ("minecraft:arrow", 8)]);
        equip(&mut r, 4, true);
        let target = player_target(&mut r);
        r.world().init_resource::<Seen>();
        r.world().add_observer(
            move |mut event: On<ProjectileImpact>, mut seen: ResMut<Seen>| {
                seen.impacts += 1;
                if impact_veto {
                    event.cancel();
                }
            },
        );
        r.world()
            .add_observer(move |mut event: On<PlayerDamage>, mut seen: ResMut<Seen>| {
                seen.damage += 1;
                event.cancel();
            });
        shoot(&mut r);
        r.ticks(5);
        let world = r.world();
        assert_eq!(world.resource::<Seen>().impacts, 1);
        assert_eq!(world.resource::<Seen>().damage, usize::from(!impact_veto));
        assert_eq!(world.get::<Health>(target).unwrap().current, 100.0);
        assert!(
            world
                .get::<crate::entity::player_life::environment::Burning>(target)
                .is_none()
        );
        assert!(
            world
                .get::<crate::entity::mob::combat::HurtTimer>(target)
                .is_none()
        );
    }
}

#[test]
fn immune_zero_loss_and_hurt_window_targets_do_not_catch_flame() {
    for rejected in ["creative", "fire-immune", "zero-loss", "hurt-window"] {
        let mut r = range(&[("minecraft:bow", 1), ("minecraft:arrow", 8)]);
        equip(&mut r, 4, true);
        let target = player_target(&mut r);
        r.world().init_resource::<Seen>();
        r.world()
            .add_observer(|_event: On<ProjectileImpact>, mut seen: ResMut<Seen>| seen.impacts += 1);
        match rejected {
            "creative" => {
                r.world().entity_mut(target).insert(GameMode::Creative);
            }
            "fire-immune" => {
                r.world().entity_mut(target).insert(ActorFlags::default());
                r.world()
                    .get_mut::<ActorFlags>(target)
                    .unwrap()
                    .update(jolyne::valentine::actor_flags::FIRE_IMMUNE, true)
                    .unwrap();
            }
            "zero-loss" => {
                r.world()
                    .add_observer(|mut event: On<PlayerDamage>| event.post_armour_multiplier = 0.0);
            }
            "hurt-window" => {
                r.world()
                    .entity_mut(target)
                    .insert(crate::entity::mob::combat::HurtTimer {
                        until_tick: 1000,
                        last_damage: 100.0,
                    });
            }
            _ => unreachable!(),
        }
        shoot(&mut r);
        r.ticks(5);
        let world = r.world();
        assert_eq!(world.resource::<Seen>().impacts, 1, "{rejected}");
        assert!(
            world
                .get::<crate::entity::player_life::environment::Burning>(target)
                .is_none(),
            "{rejected}"
        );
        let health = world.get::<Health>(target).unwrap().current;
        if rejected == "fire-immune" {
            assert!(health < 100.0);
        } else {
            assert_eq!(health, 100.0, "{rejected}");
        }
    }
}

#[test]
fn late_launch_veto_preserves_bow_and_ammunition() {
    use crate::server::game::item_actions::{ItemActionKind, PlayerItemAction};
    let mut r = range(&[("minecraft:bow", 1), ("minecraft:arrow", 8)]);
    equip(&mut r, 4, true);
    let before = r.slot(0);
    r.world().add_observer(|mut event: On<PlayerItemAction>| {
        if event.kind() == ItemActionKind::Launch {
            event.cancel();
        }
    });
    r.send(UseRequest::UseAir);
    r.ticks(24);
    r.send(UseRequest::Release);
    assert_eq!(r.slot(0), before);
    assert_eq!(r.slot(1).count, 8);
    let world = r.world();
    assert_eq!(
        world
            .query_filtered::<Entity, With<crate::entity::projectile::ProjectileFlight>>()
            .iter(world)
            .count(),
        0
    );
}

#[test]
fn stuck_flame_arrows_extinguish_in_water_and_at_their_deadline() {
    for water in [false, true] {
        let mut r = range(&[("minecraft:bow", 1), ("minecraft:arrow", 8)]);
        equip(&mut r, 0, true);
        let arrow = shoot(&mut r);
        let world = r.world();
        world.entity_mut(arrow).remove::<ProjectileItem>();
        world.entity_mut(arrow).insert(Stuck { since: 1 });
        if water {
            let at = world.get::<Position>(arrow).unwrap().0.floor().as_ivec3();
            let registry = world.resource::<crate::server::game::types::BlockRegistryResource>();
            let water = registry
                .0
                .default_state_id_by_name("minecraft:water")
                .unwrap();
            crate::server::game::blocks::set_block(world, at, water, false);
        } else {
            world.resource_mut::<TickCounter>().current += 100;
        }
        r.server.tick();
        let world = r.world();
        assert_eq!(
            crate::entity::projectile::enchantments::flame_ticks(world, arrow),
            0
        );
        assert!(
            !world
                .get::<ActorFlags>(arrow)
                .unwrap()
                .get(jolyne::valentine::actor_flags::ONFIRE)
                .unwrap()
        );
    }
}
