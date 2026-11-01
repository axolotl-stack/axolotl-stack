//! Actual SDK observations follow physical native travel rather than a successful proposal.

use super::*;
use crate::entity::components::{Health, Velocity};
use crate::entity::projectile::{self, LaunchSpec};

impl Harness {
    /// Keep ordinary native actors, real canonical storage and the unchanged scoped SDK handler.
    fn observed(mode: u8, destination: u8, required: bool) -> Self {
        let h = Self::component_factory(true, true, required, &[mode, destination], |engine| {
            wasmtime::component::Component::new(engine, authorization::guest()).unwrap()
        });
        h.manager
            .lock()
            .unwrap()
            .plugins
            .get_mut(&PluginId("example.travel".into()))
            .unwrap()
            .manifest
            .subscriptions
            .insert(EventKind::PlayerTravelled);
        h
    }
    /// A final-guard log is not movement or an owned request result.
    async fn observed_finish(&mut self) {
        for _ in 0..12 {
            self.router.tick().await.unwrap();
            if self.logs().iter().any(|line| line.starts_with("result:")) {
                return;
            }
        }
        panic!("actual native request did not produce its terminal result");
    }
    /// Resolve an ordinary real native pearl collision without issuing a durable request.
    fn pearl(&mut self) {
        let player = self.player;
        let (actor, climate) = projectile::thrown_by("minecraft:ender_pearl").unwrap();
        let pearl = projectile::launch_with(
            self.world_mut(),
            player,
            actor,
            climate,
            LaunchSpec {
                throw_sound: false,
                ..LaunchSpec::default()
            },
        )
        .unwrap();
        self.world_mut()
            .entity_mut(pearl)
            .insert((Position(DVec3::new(8.5, 64.5, 8.5)), Velocity(DVec3::NEG_Y)));
        let mut tick = bevy_ecs::schedule::Schedule::default();
        tick.add_systems(projectile::tick_projectiles);
        tick.run(self.world_mut());
        assert!(self.world().get_entity(pearl).is_err());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_same_and_cross_world_moves_emit_owned_facts_before_the_request_result() {
    for destination in [0, 1] {
        let mut h = Harness::observed(0, destination, false);
        h.queue();
        h.observed_finish().await;
        assert_eq!(
            h.logs(),
            [
                "guard",
                if destination == 0 {
                    "committed:command"
                } else {
                    "committed:world-transfer"
                },
                "result:moved"
            ]
        );
        assert_eq!(h.provider.writes.load(Ordering::SeqCst), 1);
        assert_eq!(
            h.router.locate_session(73).unwrap().0,
            i32::from(destination)
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vetoed_native_destinations_never_emit_committed_observations() {
    for destination in [0, 1] {
        let mut h = Harness::observed(1, destination, false);
        h.queue();
        h.observed_finish().await;
        assert_eq!(h.logs(), ["guard", "result:rejected"]);
        assert_eq!(h.provider.writes.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_ordinary_pearl_commit_is_observed_without_fabricating_a_durable_request() {
    for denied in [false, true] {
        let mut h = Harness::observed(if denied { 7 } else { 6 }, 0, false);
        h.pearl();
        assert_eq!(
            h.logs(),
            if denied {
                vec!["guard"]
            } else {
                vec!["guard", "committed:projectile"]
            }
        );
        assert_eq!(h.provider.writes.load(Ordering::SeqCst), 0);
        assert_eq!(
            h.world().get::<Health>(h.player).unwrap().current,
            if denied { 20.0 } else { 15.0 }
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acknowledged_storage_is_not_a_native_movement_observation_when_acknowledgement_is_uncertain()
 {
    let mut h = Harness::observed(0, 0, false);
    h.provider.fail.store(true, Ordering::SeqCst);
    h.queue();
    h.observed_finish().await;
    assert!(h.logs().iter().any(|line| line == "result:uncertain"));
    assert!(!h.logs().iter().any(|line| line.starts_with("committed:")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_guest_keeps_owned_committed_facts_after_the_real_native_actor_departs() {
    let mut h = Harness::observed(0, 0, false);
    h.queue();
    h.observed_finish().await;
    h.manager
        .lock()
        .unwrap()
        .plugins
        .get_mut(&PluginId("example.travel".into()))
        .unwrap()
        .manifest
        .subscriptions
        .insert(EventKind::Tick);
    h.router.world_mut(0).unwrap().despawn_player(73);
    assert!(h.world().get_entity(h.player).is_err());
    h.router.tick().await.unwrap();
    assert!(h.logs().iter().any(|line| line == "owned-after-departure"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn committed_callback_traps_never_relabel_an_already_persisted_native_move_as_a_veto() {
    for required in [false, true] {
        let mut h = Harness::observed(if required { 9 } else { 8 }, 0, required);
        h.queue();
        let mut failure = false;
        for _ in 0..4 {
            failure |= h.router.tick().await.is_err();
        }
        assert_eq!(h.provider.writes.load(Ordering::SeqCst), 1);
        assert_eq!(failure, required);
        if required {
            assert!(crate::server::game::inventory_effects::is_recovery_fenced(
                h.world(),
                h.player
            ));
        } else {
            assert_eq!(
                h.world().get::<Position>(h.player).unwrap().0,
                DVec3::new(8.5, 65.62, 8.5)
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_actual_component_never_inherits_the_originating_request_correlation() {
    for destination in [0, 1] {
        let mut h = Harness::observed(0, destination, false);
        {
            let mut manager = h.manager.lock().unwrap();
            let manifest = PluginManifest {
                id: PluginId("example.travel-observer".into()),
                name: "Observer".into(),
                version: "1".into(),
                api_version: PLUGIN_API_VERSION.into(),
                required: false,
                config: None,
                capabilities: [PluginCapability::ReadPlayers].into(),
                subscriptions: [EventKind::PlayerTravelled].into(),
                limits: PluginLimits::default(),
            };
            let mut store = store(&manager.engine, &manifest.limits);
            store.data_mut().configuration = vec![6, destination];
            let component =
                wasmtime::component::Component::new(&manager.engine, authorization::guest())
                    .unwrap();
            manager.pending.push(PendingPlugin {
                component,
                store,
                manifest,
            });
        }
        h.router.tick().await.unwrap();
        h.router.tick().await.unwrap();
        h.queue();
        h.observed_finish().await;
        let manager = h.manager.lock().unwrap();
        let observer = &manager.plugins[&PluginId("example.travel-observer".into())];
        assert_eq!(
            observer.store.data().test_logs,
            [if destination == 0 {
                "committed:command"
            } else {
                "committed:world-transfer"
            }]
        );
    }
}
