//! Real SDK imports preserve native health, food processing and actor authority.

use super::*;
use crate::entity::components::Hunger;
use crate::server::game::hunger::{FoodTimer, PlayerFoodPolicy};

/// Apply a genuine native hit before testing an exact fractional healing intent.
fn hit(harness: &mut Harness, amount: f32) -> f32 {
    crate::entity::player_life::hurt_player(
        harness.server.ecs.world_mut(),
        harness.player,
        &crate::entity::mob::combat::Hit {
            damage: amount,
            cause: unastar_data::actor_components::body::ActorDamageCause::Override,
            attacker: None,
            by_player: false,
            knockback_from: None,
            knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
        },
    )
}

/// Read physical state only, without projecting a staged mutation into the assertion.
fn health(harness: &Harness) -> f32 {
    harness
        .server
        .ecs
        .world()
        .get::<Health>(harness.player)
        .unwrap()
        .current
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exact_fractional_healing_follows_real_damage_clamps_and_reaches_native_attributes() {
    let mut h = Harness::extended(true);
    assert_eq!(hit(&mut h, 3.25), 3.25);
    assert_eq!(health(&h), 16.75);
    h.command("probe heal");
    assert_eq!(health(&h), 16.75, "Staged is not an applied effect");
    h.apply();
    assert_eq!(health(&h), 18.0);
    let mut sync = bevy_ecs::schedule::Schedule::default();
    sync.add_systems(crate::server::game::attributes::sync_player_attributes);
    sync.run(h.server.ecs.world_mut());
    assert!(std::iter::from_fn(|| h.packets.try_recv().ok()).any(|packet| matches!(packet.data,
        McpePacketData::UpdateAttributesPacket(update) if update.attribute_list.iter().any(|value| value.name == "minecraft:health" && value.current_value == 18.0))));
    h.command("probe heal-clamp");
    h.apply();
    assert_eq!(health(&h), 20.0);
    assert!(h.logs().iter().any(|value| value == "heal-staged"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vitals_copy_actual_native_values_and_invalid_healing_does_not_mutate_them() {
    let mut h = Harness::extended(true);
    let world = h.server.ecs.world_mut();
    world.get_mut::<Health>(h.player).unwrap().current = 11.5;
    world.entity_mut(h.player).insert((
        Hunger {
            food_level: 14,
            saturation: 3.5,
            exhaustion: 0.75,
        },
        crate::server::game::attributes::Absorption(2.5),
    ));
    h.command("probe vitals");
    assert!(
        h.logs()
            .iter()
            .any(|value| value == "vitals:11.5,20,2.5,14,3.5,0.75,false")
    );
    h.command("probe heal-invalid");
    h.apply();
    assert_eq!(health(&h), 11.5);
    assert!(
        h.logs()
            .iter()
            .any(|value| value == "invalid-heal-rejected")
    );
    h.command("probe food-off");
    h.command("probe vitals");
    assert_eq!(
        h.logs().last().unwrap(),
        "vitals:11.5,20,2.5,14,3.5,0.75,false"
    );
    h.apply();
    h.command("probe vitals");
    assert_eq!(
        h.logs().last().unwrap(),
        "vitals:11.5,20,2.5,14,3.5,0.75,true"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scoped_food_policy_controls_real_exhaustion_regeneration_and_starvation_without_resetting_timers()
 {
    for food in [0, 20] {
        let mut h = Harness::extended(true);
        h.server
            .ecs
            .world_mut()
            .get_mut::<Health>(h.player)
            .unwrap()
            .current = 12.0;
        h.server.ecs.world_mut().entity_mut(h.player).insert((
            Hunger {
                food_level: food,
                saturation: 0.0,
                exhaustion: 0.0,
            },
            FoodTimer::default(),
        ));
        h.server
            .ecs
            .world_mut()
            .get_mut::<FoodTimer>(h.player)
            .unwrap()
            .ticks = 37;
        h.command("probe food-off");
        assert!(
            h.server
                .ecs
                .world()
                .get::<PlayerFoodPolicy>(h.player)
                .is_none()
        );
        h.apply();
        assert_eq!(
            h.server
                .ecs
                .world()
                .get::<FoodTimer>(h.player)
                .unwrap()
                .ticks,
            37
        );
        crate::server::game::hunger::exhaust_player(h.server.ecs.world_mut(), h.player, |values| {
            values.jump
        });
        assert_eq!(
            h.server
                .ecs
                .world()
                .get::<Hunger>(h.player)
                .unwrap()
                .exhaustion,
            0.0
        );
        let mut tick = bevy_ecs::schedule::Schedule::default();
        tick.add_systems(crate::server::game::hunger::tick_hunger);
        for _ in 0..160 {
            tick.run(h.server.ecs.world_mut());
        }
        assert_eq!(health(&h), 12.0);
        h.command("probe food-on");
        h.apply();
        assert!(
            h.server
                .ecs
                .world()
                .get::<PlayerFoodPolicy>(h.player)
                .is_none()
        );
        for _ in 0..80 {
            tick.run(h.server.ecs.world_mut());
        }
        assert_eq!(health(&h), if food == 0 { 11.0 } else { 13.0 });
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mutation_permission_and_optional_traps_preserve_vitals_and_food_policy() {
    for permitted in [false, true] {
        let mut h = Harness::component_factory(
            permitted,
            &[PluginCapability::Commands, PluginCapability::ScheduleTasks],
            false,
            false,
            |engine| wasmtime::component::Component::new(engine, guest()).unwrap(),
        );
        h.server
            .ecs
            .world_mut()
            .get_mut::<Health>(h.player)
            .unwrap()
            .current = 12.0;
        h.command(if permitted {
            "probe vitals-trap"
        } else {
            "probe vitals-denied"
        });
        h.apply();
        assert_eq!(health(&h), 12.0);
        assert!(
            h.server
                .ecs
                .world()
                .get::<PlayerFoodPolicy>(h.player)
                .is_none()
        );
        assert!(h.manager.lock().unwrap().readiness.check().is_ok());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn food_drain_is_independent_of_exhaustion_accumulation() {
    let mut h = Harness::extended(true);
    h.server.ecs.world_mut().entity_mut(h.player).insert((
        Hunger {
            food_level: 14,
            saturation: 3.5,
            exhaustion: 4.5,
        },
        FoodTimer::default(),
    ));
    h.command("probe food-no-drain");
    h.apply();
    let mut tick = bevy_ecs::schedule::Schedule::default();
    tick.add_systems(crate::server::game::hunger::tick_hunger);
    tick.run(h.server.ecs.world_mut());
    let food = h.server.ecs.world().get::<Hunger>(h.player).unwrap();
    assert_eq!(
        (food.food_level, food.saturation, food.exhaustion),
        (14, 3.5, 0.5)
    );
    h.command("probe food-on");
    h.apply();
    h.server
        .ecs
        .world_mut()
        .get_mut::<Hunger>(h.player)
        .unwrap()
        .exhaustion = 4.5;
    tick.run(h.server.ecs.world_mut());
    let food = h.server.ecs.world().get::<Hunger>(h.player).unwrap();
    assert_eq!(
        (food.food_level, food.saturation, food.exhaustion),
        (14, 2.5, 0.5)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_native_vitals_remain_unavailable_instead_of_becoming_guessed_defaults() {
    let mut h = Harness::extended(true);
    h.server
        .ecs
        .world_mut()
        .get_mut::<Hunger>(h.player)
        .unwrap()
        .exhaustion = f32::NAN;
    h.command("probe vitals-unavailable");
    assert!(h.logs().iter().any(|value| value == "vitals-unavailable"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stale_channels_component_unload_and_dead_sources_discard_staged_vitals_mutations() {
    for mode in ["channel", "owner", "dead", "identity"] {
        let mut h = Harness::extended(true);
        h.server
            .ecs
            .world_mut()
            .get_mut::<Health>(h.player)
            .unwrap()
            .current = 12.0;
        h.command("probe heal");
        h.command("probe food-off");
        let (outbound, _receiver) = tokio::sync::mpsc::channel(256);
        match mode {
            "channel" => {
                h.server
                    .ecs
                    .world_mut()
                    .get_mut::<PlayerSession>(h.player)
                    .unwrap()
                    .outbound_tx = outbound
            }
            "owner" => h.manager.lock().unwrap().plugins.clear(),
            "dead" => {
                h.server
                    .ecs
                    .world_mut()
                    .get_mut::<Health>(h.player)
                    .unwrap()
                    .current = 0.0
            }
            "identity" => {
                h.server
                    .ecs
                    .world_mut()
                    .entity_mut(h.player)
                    .insert(PlayerUuid(uuid::Uuid::from_u128(99)));
            }
            _ => unreachable!(),
        }
        h.apply();
        assert_eq!(
            health(&h),
            if mode == "dead" { 0.0 } else { 12.0 },
            "{mode}"
        );
        assert!(
            h.server
                .ecs
                .world()
                .get::<PlayerFoodPolicy>(h.player)
                .is_none(),
            "{mode}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_retained_actors_receive_healing_and_food_policy_without_a_transport_proxy() {
    for dead in [false, true] {
        let mut h = Harness::extended(true);
        h.server
            .ecs
            .world_mut()
            .get_mut::<Health>(h.player)
            .unwrap()
            .current = 12.0;
        h.command("probe remember-vitals");
        h.server.despawn_player(42);
        assert!(
            h.server
                .ecs
                .world()
                .get::<PlayerSession>(h.player)
                .is_none()
        );
        if dead {
            h.server
                .ecs
                .world_mut()
                .get_mut::<Health>(h.player)
                .unwrap()
                .current = 0.0;
        }
        let (router, manager, player) = tasks(h);
        let world = router.world(0).unwrap().ecs.world();
        assert_eq!(
            world.get::<Health>(player).unwrap().current,
            if dead { 0.0 } else { 13.25 }
        );
        assert_eq!(world.get::<PlayerFoodPolicy>(player).is_some(), !dead);
        let manager = manager.lock().unwrap();
        let logs = &manager.plugins[&PluginId("example.mechanics".into())]
            .store
            .data()
            .test_logs;
        assert!(logs.iter().any(|value| value
            == if dead {
                "retained-vitals-unavailable"
            } else {
                "retained-vitals-staged"
            }));
    }
}
