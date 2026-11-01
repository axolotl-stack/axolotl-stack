//! Canonical component observations distinguish exact native damage invocations.

use super::*;
use crate::entity::player_life::hooks::{PlayerLifecycleHandler, PlayerLifecycleHooks};
use std::sync::atomic::{AtomicBool, Ordering};

/// Preserve all 64 bits as deterministic printable nibbles through the bounded log import.
fn observe_id(callback: &str, argument: &str) -> String {
    let (offset, _) = fixture::field_layout(callback, argument, &["action-id"]);
    let mut body = String::new();
    for nibble in 0..16 {
        body.push_str(&format!(
            "i32.const {} local.get 0 i64.load offset={offset} i64.const {} i64.shr_u i64.const 15 i64.and i32.wrap_i64 i32.const 48 i32.add i32.store8 ",
            1024 + nibble, nibble * 4
        ));
    }
    body.push_str("i32.const 0 i32.const 1024 i32.const 16 call @log@");
    body
}

/// Start a second real hit after the outer component proposal has returned.
struct NestedHit(AtomicBool);
impl PlayerLifecycleHandler for NestedHit {
    fn on_damage(&self, world: &mut bevy_ecs::world::World, event: &mut PlayerDamage) {
        if !self.0.swap(true, Ordering::Relaxed) {
            let mut hit = event.hit;
            hit.damage = 2.0;
            assert_eq!(
                crate::entity::player_life::hurt_player(world, event.player(), &hit),
                2.0
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nested_same_actor_hits_expose_distinct_immutable_action_ids_through_actual_loss() {
    let proposed = observe_id("on-player-damage", "proposal");
    let actual = observe_id("on-player-damaged", "damage");
    let mut harness = Harness::new(&[
        ("on-player-damage", format!("{proposed} {}", tuning(""))),
        ("on-player-damaged", actual),
    ]);
    harness
        .world()
        .resource_mut::<PlayerLifecycleHooks>()
        .register(NestedHit(AtomicBool::new(false)));
    // Native hurt-window delta rules still apply after the inner full hit.
    assert_eq!(harness.hit(6.0), 4.0);
    let records = harness.logs();
    assert_eq!(records.len(), 4);
    assert_ne!(records[0], records[1], "nested hits reused the outer ID");
    assert_eq!(records[1], records[2], "inner committed ID changed");
    assert_eq!(
        records[0], records[3],
        "outer committed ID changed after reentry"
    );
    assert!(records.iter().all(|value| value != "0000000000000000"));
    assert!(harness.manager.lock().unwrap().check_required().is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn late_veto_zero_loss_and_hurt_suppression_never_emit_an_action_completion() {
    for mode in 0..4 {
        let proposed = observe_id("on-player-damage", "proposal");
        let edit = if mode == 1 {
            let (base, _) = fixture::field_layout("on-player-damage", "proposal", &["tuning"]);
            let (post, _) = fixture::field_layout(
                "on-player-damage",
                "proposal",
                &["tuning", "post-armour-multiplier"],
            );
            format!("i32.const {} f32.const 0 f32.store", 520 + post - base)
        } else {
            String::new()
        };
        let mut harness = Harness::new(&[
            ("on-player-damage", format!("{proposed} {}", tuning(&edit))),
            (
                "on-player-damaged",
                observe_id("on-player-damaged", "damage"),
            ),
        ]);
        if mode == 0 {
            harness
                .world()
                .add_observer(|mut event: bevy_ecs::prelude::On<PlayerDamage>| event.cancel());
        }
        if mode == 2 {
            assert_eq!(harness.hit(4.0), 4.0);
            assert_eq!(harness.logs().len(), 2);
        }
        let before = harness.logs().len();
        assert_eq!(harness.hit(if mode == 3 { 0.0 } else { 2.0 }), 0.0);
        assert_eq!(
            harness.logs().len(),
            before + 1,
            "uncommitted loss emitted a completion"
        );
        let records = harness.logs();
        assert!(records.iter().all(|value| value != "0000000000000000"));
        if mode == 2 {
            assert_ne!(records[0], records[2]);
        }
        let readiness = harness.manager.lock().unwrap().check_required();
        assert!(readiness.is_ok(), "{readiness:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn all_component_subscribers_observe_the_same_native_action_id() {
    let bodies = [
        (
            "on-player-damage",
            format!(
                "{} {}",
                observe_id("on-player-damage", "proposal"),
                tuning("")
            ),
        ),
        (
            "on-player-damaged",
            observe_id("on-player-damaged", "damage"),
        ),
    ];
    let mut harness = Harness::new(&bodies);
    let owner = harness.manager.clone();
    let mut manager = owner.lock().unwrap();
    let mut manifest = manager.plugins.values().next().unwrap().manifest.clone();
    manifest.id = PluginId("example.damage.peer".into());
    let pending = PendingPlugin {
        component: fixture::component(&manager.engine, &bodies),
        store: store(&manager.engine, &manifest.limits),
        manifest,
    };
    manager.pending.push(pending);
    manager.initialize_pending(harness.world());
    drop(manager);
    assert_eq!(harness.hit(3.0), 3.0);
    let manager = harness.manager.lock().unwrap();
    let logs = manager
        .plugins
        .values()
        .map(|plugin| plugin.store.data().test_logs.clone())
        .collect::<Vec<_>>();
    assert_eq!(logs.len(), 2);
    assert_eq!(logs[0], logs[1]);
    assert_eq!(logs[0].len(), 2);
    assert_eq!(logs[0][0], logs[0][1]);
    assert_ne!(logs[0][0], "0000000000000000");
    assert!(manager.check_required().is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_runtime_worlds_do_not_reuse_damage_action_ids() {
    let bodies = [
        (
            "on-player-damage",
            format!(
                "{} {}",
                observe_id("on-player-damage", "proposal"),
                tuning("")
            ),
        ),
        (
            "on-player-damaged",
            observe_id("on-player-damaged", "damage"),
        ),
    ];
    let mut first = Harness::new(&bodies);
    let mut second = Harness::new(&bodies);
    assert_eq!(first.hit(1.0), 1.0);
    assert_eq!(second.hit(1.0), 1.0);
    let first = first.logs();
    let second = second.logs();
    assert_eq!(first[0], first[1]);
    assert_eq!(second[0], second[1]);
    assert_ne!(first[0], second[0]);
}
